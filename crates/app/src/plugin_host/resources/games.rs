//! `GamesRequestLaunch` and `GamesRequestInstall` of `ResourceHost` (tasks 7.2 and 10.10,
//! design sections 11 and 20).
//!
//! Both start something on the user's computer, so neither runs without the user's intent (see
//! `plugin_host::intents`). The request names a game and carries no parameter of what to start:
//! a launch starts the game's detected target unchanged, an install opens the provider's own
//! install flow. The answer is always a job (task 7.1): with no intent the job waits, queued,
//! while the host asks the user, who can approve, decline, or let it time out, and the plugin
//! can cancel it.

use gamenode_plugin_protocol::{capability as cap, proto::*};
use tonic::{Request, Response, Status};

use super::mutations::{game_id, request_hash, unavailable};
use super::{ResourceApi, Scope};
use crate::plugin_host::intents::{Binding, ConfirmationRequest, Operation, Pending, Verdict};
use crate::plugin_host::jobs::{JobContext, JobOwner, Outcome, work};
use crate::plugin_host::library::Projected;
use crate::plugin_host::provider::ResourceCaller;

const MAX_KEY_BYTES: usize = 128;
const MAX_INSTALLATION_ID_BYTES: usize = 128;
/// What a job says while the user has not answered.
const AWAITING: &str = "awaiting confirmation";

/// One request, whatever its operation.
struct Action {
    context: Option<ServiceContext>,
    permission: &'static str,
    operation: Operation,
    game_id: String,
    installation_id: String,
    /// The entry revision the plugin read; `None` for an install, which has none.
    expected_revision: Option<u64>,
    intent: String,
    key: String,
    custom_recipe: Option<gamenode_core::library::CustomLaunchRecipe>,
    custom_dispatch: Option<crate::plugin_host::library::CustomLaunchDispatch>,
    custom_resources: Vec<gamenode_core::library::LaunchPlanResource>,
}

struct ExtensionActionShutdown {
    providers: crate::plugin_host::provider::ProviderApi,
    session_id: String,
}

impl Drop for ExtensionActionShutdown {
    fn drop(&mut self) {
        self.providers.request_shutdown_session(&self.session_id);
    }
}

/// How the request was authorized before its job started.
enum Authorized {
    /// A pre-issued intent was spent, or the user had remembered this plugin and game.
    Already,
    /// The user must be asked; the slot is held until the answer.
    Ask(Pending),
}

fn text_ok(text: &str, limit: usize) -> bool {
    !text.is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
}

fn code_word(status: &Status) -> &'static str {
    match status.code() {
        tonic::Code::Unimplemented => "unsupported",
        tonic::Code::FailedPrecondition => "unavailable",
        tonic::Code::NotFound => "not_found",
        _ => "failed",
    }
}

fn not_found() -> Status {
    Status::not_found("library entry not found")
}

impl ResourceApi {
    pub(super) async fn request_custom_action(
        &self,
        request: CustomLaunchRequest,
        recipe: gamenode_core::library::CustomLaunchRecipe,
        custom_dispatch: crate::plugin_host::library::CustomLaunchDispatch,
        custom_resources: Vec<gamenode_core::library::LaunchPlanResource>,
    ) -> Result<JobReply, Status> {
        self.request_action(Action {
            context: request.context,
            permission: cap::GAME_CUSTOM_LAUNCH,
            operation: Operation::CustomLaunch,
            game_id: request.game_id,
            installation_id: request.installation_id,
            expected_revision: Some(request.expected_revision),
            intent: request.user_intent_id,
            key: request.idempotency_key,
            custom_recipe: Some(recipe),
            custom_dispatch: Some(custom_dispatch),
            custom_resources,
        })
        .await
    }

    pub(super) async fn request_launch(
        &self,
        request: Request<GameLaunchRequest>,
    ) -> Result<Response<GameLaunchReply>, Status> {
        let request = request.into_inner();
        let job = self
            .request_action(Action {
                context: request.context,
                permission: cap::GAME_LAUNCH,
                operation: Operation::Launch,
                game_id: request.game_id,
                installation_id: request.installation_id,
                expected_revision: Some(request.expected_revision),
                intent: request.user_intent_id,
                key: request.idempotency_key,
                custom_recipe: None,
                custom_dispatch: None,
                custom_resources: Vec::new(),
            })
            .await?;
        Ok(Response::new(GameLaunchReply {
            job_id: job.job_id,
            state: job.state,
            refusal_code: String::new(),
            message: job.message,
        }))
    }

    pub(super) async fn request_install(
        &self,
        request: Request<GameInstallRequest>,
    ) -> Result<Response<JobReply>, Status> {
        let request = request.into_inner();
        self.request_action(Action {
            context: request.context,
            permission: cap::GAMES_INSTALL,
            operation: Operation::Install,
            game_id: request.game_id,
            installation_id: request.installation_id,
            expected_revision: None,
            intent: request.user_intent_id,
            key: request.idempotency_key,
            custom_recipe: None,
            custom_dispatch: None,
            custom_resources: Vec::new(),
        })
        .await
        .map(Response::new)
    }

    /// The entry a request names, within what the caller may start: a provider session names its
    /// own entries; an extension session names only the game the user selected for it.
    pub(super) fn action_target(
        &self,
        caller: &ResourceCaller,
        context: &ServiceContext,
        game: &str,
        installation_id: &str,
    ) -> Result<(String, Projected), Status> {
        if !text_ok(installation_id, MAX_INSTALLATION_ID_BYTES) {
            return Err(Status::invalid_argument(
                "installation_id is required: 1 to 128 bytes",
            ));
        }
        let wanted = game_id(game)?;
        if context.session_kind() != SessionKind::Extension {
            return self.own_entry(caller, installation_id, wanted);
        }
        let target = context.target_game.as_ref().ok_or_else(not_found)?;
        if target.installation_id != installation_id {
            return Err(not_found());
        }
        // The user chose this game for the extension; reading the whole library is not needed
        // and does not widen it.
        let everything = Scope {
            own_provider: caller.provider_id.clone(),
            read_all: true,
            provider_filter: None,
        };
        self.library
            .target(&everything, installation_id)
            .filter(|(_, entry)| {
                entry.game_id.map(|id| id.to_string()).unwrap_or_default() == target.game_id
                    && (wanted.is_none() || wanted == entry.game_id)
            })
            .ok_or_else(not_found)
    }

    async fn request_action(&self, action: Action) -> Result<JobReply, Status> {
        let caller = self
            .providers
            .authorize_resource(action.context.as_ref(), action.permission)?;
        let context = action.context.clone().unwrap_or_default();
        if !text_ok(&action.key, MAX_KEY_BYTES) {
            return Err(Status::invalid_argument(
                "idempotency_key is required: 1 to 128 bytes without control characters",
            ));
        }
        if !action.intent.is_empty() && !text_ok(&action.intent, MAX_KEY_BYTES) {
            return Err(Status::invalid_argument(
                "user_intent_id is at most 128 bytes without control characters",
            ));
        }
        let owner = JobOwner::of(&caller)?;
        let identity = caller
            .namespace
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("official providers launch through Core"))?
            .identity()
            .clone();
        let (entry_key, entry) =
            self.action_target(&caller, &context, &action.game_id, &action.installation_id)?;
        let current_revision = if context.session_kind() == SessionKind::Extension {
            context
                .target_game
                .as_ref()
                .and_then(|target| target.revision.parse::<u64>().ok())
        } else {
            Some(entry.revision)
        };
        if action
            .expected_revision
            .is_some_and(|expected| current_revision != Some(expected))
        {
            return Err(Status::failed_precondition(
                "the game changed since it was read; read it again",
            ));
        }
        let host = self.library.host().ok_or_else(unavailable)?;
        let launching = matches!(
            action.operation,
            Operation::Launch | Operation::CustomLaunch
        );
        let fingerprint = if launching {
            host.launch_fingerprint(entry_key.clone()).await?
        } else {
            0
        };
        let (plan_fingerprint, launch_plan_review) = if launching {
            let plan = super::super::launch_plan::resolve(
                &caller,
                &entry,
                fingerprint,
                action.custom_recipe.as_ref(),
                action.custom_dispatch.as_ref(),
                &action.custom_resources,
            )?;
            tokio::task::spawn_blocking(move || {
                let fingerprint = plan
                    .fingerprint()
                    .map_err(|_| Status::failed_precondition("the launch plan is invalid"))?;
                let review = super::super::launch_plan::review(&plan, fingerprint.clone());
                Ok::<_, Status>((fingerprint, Some(review)))
            })
            .await
            .map_err(|_| Status::internal("launch plan fingerprint worker failed"))??
        } else {
            (String::new(), None)
        };
        let game = entry.game_id.map(|id| id.to_string()).unwrap_or_default();
        let recipe_revision = action
            .custom_recipe
            .as_ref()
            .map(|recipe| recipe.revision)
            .unwrap_or_default();
        let binding = Binding {
            plugin_id: identity.plugin_id.clone(),
            publisher_fingerprint: identity.publisher_fingerprint.clone(),
            profile_epoch: caller.profile_epoch,
            game_id: game.clone(),
            installation_id: entry.installation_id.clone(),
            operation: action.operation,
            target_revision: fingerprint,
            recipe_id: action
                .custom_recipe
                .as_ref()
                .map(|recipe| recipe.recipe_id.clone())
                .unwrap_or_default(),
            recipe_revision,
            plan_fingerprint: plan_fingerprint.clone(),
        };
        let custom_recipe_id = action
            .custom_recipe
            .as_ref()
            .map(|recipe| recipe.recipe_id.as_str())
            .unwrap_or_default();
        let custom_recipe_revision = action
            .custom_recipe
            .as_ref()
            .map(|recipe| recipe.revision.to_string())
            .unwrap_or_default();
        let hash = request_hash(&[
            "game-action",
            action.operation.as_str(),
            &action.game_id,
            &action.installation_id,
            &action.expected_revision.unwrap_or(0).to_string(),
            &action.intent,
            custom_recipe_id,
            &custom_recipe_revision,
            &plan_fingerprint,
        ]);

        // Retries and first requests are decided together, so a retry never spends a second
        // intent or opens a second dialog.
        let _serial = self.mutations.lock.lock().await;
        if let Some(job) = self.jobs.replay(&owner, &action.key, hash)? {
            return Ok(job);
        }
        let authorized =
            if !action.intent.is_empty() && self.intents.redeem(&binding, &action.intent).is_ok() {
                Authorized::Already
            } else if self.remembered(&caller, &binding).await? {
                Authorized::Already
            } else {
                if !self.intents.can_confirm() {
                    return Err(Status::failed_precondition(
                        "this GameNode build cannot ask for confirmations yet",
                    ));
                }
                Authorized::Ask(self.intents.begin_pending(&binding)?)
            };
        let awaiting = matches!(authorized, Authorized::Ask(_)).then_some(AWAITING);

        let api = self.clone();
        let work_caller = caller.clone();
        let work_context = context.clone();
        let extension_session = context.session_kind() == SessionKind::Extension;
        let extension_session_id = context.session_id.clone();
        let request = ConfirmationRequest {
            plugin_id: identity.plugin_id,
            publisher_fingerprint: identity.publisher_fingerprint,
            game_name: entry
                .display_name
                .clone()
                .unwrap_or_else(|| entry.name.clone()),
            operation: action.operation,
            details: vec![],
            launch_plan: launch_plan_review,
        };
        let (game_arg, installation_arg, operation) = (
            action.game_id.clone(),
            action.installation_id.clone(),
            action.operation,
        );
        let custom_recipe = action.custom_recipe.clone();
        let custom_dispatch = action.custom_dispatch.clone();
        let custom_resources = action.custom_resources.clone();
        let permission = action.permission;
        let run = work(move |ctx| async move {
            let _shutdown = extension_session.then(|| ExtensionActionShutdown {
                providers: api.providers.clone(),
                session_id: extension_session_id,
            });
            api.run_action(
                ctx,
                &work_caller,
                work_context,
                permission,
                authorized,
                request,
                binding,
                (&game_arg, &installation_arg),
                operation,
                custom_recipe,
                custom_dispatch,
                custom_resources,
            )
            .await
        });
        let (job, _) = self.start_job(
            &caller,
            context,
            action.permission,
            &action.key,
            hash,
            awaiting,
            run,
        )?;
        Ok(job)
    }

    /// Whether the user remembered this plugin for this game and operation, and the game's
    /// target is still the one they approved.
    async fn remembered(&self, caller: &ResourceCaller, binding: &Binding) -> Result<bool, Status> {
        let (installation, operation) =
            (binding.installation_id.clone(), binding.operation.as_str());
        let approval = self
            .in_namespace(caller, move |namespace| {
                namespace.intent_approval(&installation, operation)
            })
            .await?;
        Ok(approval.is_some_and(|approval| {
            approval.game_id == binding.game_id
                && approval.target_revision == binding.target_revision
                && approval.recipe_id == binding.recipe_id
                && approval.recipe_revision == binding.recipe_revision
                && approval.plan_fingerprint == binding.plan_fingerprint
        }))
    }

    /// The job's work: wait for the user if needed, remember the answer if asked to, check
    /// everything again right before dispatching, then hand the entry to the library's own path.
    #[allow(clippy::too_many_arguments)]
    async fn run_action(
        &self,
        mut ctx: JobContext,
        caller: &ResourceCaller,
        context: ServiceContext,
        permission: &'static str,
        authorized: Authorized,
        request: ConfirmationRequest,
        binding: Binding,
        names: (&str, &str),
        operation: Operation,
        custom_recipe: Option<gamenode_core::library::CustomLaunchRecipe>,
        custom_dispatch: Option<crate::plugin_host::library::CustomLaunchDispatch>,
        custom_resources: Vec<gamenode_core::library::LaunchPlanResource>,
    ) -> Outcome {
        if let Authorized::Ask(pending) = authorized {
            let verdict = tokio::select! {
                verdict = self.intents.confirm(request.clone()) => verdict,
                () = ctx.cancelled() => return Outcome::cancelled("withdrawn before the user answered"),
            };
            drop(pending);
            match verdict {
                Ok(Verdict::Approved { remember }) => {
                    if remember
                        && let Err(status) = self
                            .remember_answer(caller, &context, permission, &binding, &request)
                            .await
                    {
                        return Outcome::stopped(&status);
                    }
                }
                Ok(Verdict::Declined) => {
                    return Outcome::failed("declined", "the user declined");
                }
                Ok(Verdict::TimedOut) => {
                    return Outcome::failed("timed_out", "the user did not answer in time");
                }
                Err(_) => {
                    return Outcome::failed("unavailable", "the host could not ask the user");
                }
            }
        }
        if ctx.start().is_err() {
            return Outcome::cancelled("withdrawn before it started");
        }
        // The user may have taken minutes, or the intent seconds: nothing earlier is trusted.
        if let Err(status) = ctx.check() {
            return Outcome::stopped(&status);
        }
        let Some(host) = self.library.host() else {
            return Outcome::failed("unavailable", "the library is not available");
        };
        let (entry_key, entry) = match self.action_target(caller, &context, names.0, names.1) {
            Ok(found) => found,
            Err(status) => return Outcome::failed(code_word(&status), "the game is not available"),
        };
        if matches!(operation, Operation::Launch | Operation::CustomLaunch) {
            match host.launch_fingerprint(entry_key.clone()).await {
                Ok(current) if current == binding.target_revision => {}
                Ok(_) => {
                    return Outcome::failed(
                        "target_changed",
                        "the game's launch target changed; ask again",
                    );
                }
                Err(status) => return Outcome::failed(code_word(&status), status.message()),
            }
        }
        if entry.game_id.map(|id| id.to_string()).unwrap_or_default() != binding.game_id {
            return Outcome::failed("target_changed", "the game changed; ask again");
        }
        if ctx.check().is_err() {
            return Outcome::cancelled("cancelled before it was started");
        }
        if operation == Operation::CustomLaunch
            && let Some(recipe) = custom_recipe.as_ref()
        {
            let Some(dispatch) = custom_dispatch.as_ref() else {
                return Outcome::failed("unavailable", "the custom launch recipe is unavailable");
            };
            let current_resources = match self
                .revalidate_custom_handles(caller, &context, recipe, dispatch)
                .await
            {
                Ok(resources) => resources,
                Err(status) => return Outcome::stopped(&status),
            };
            match host.launch_fingerprint(entry_key.clone()).await {
                Ok(current) if current == binding.target_revision => {}
                Ok(_) => {
                    return Outcome::failed(
                        "target_changed",
                        "the game's launch target changed; ask again",
                    );
                }
                Err(status) => return Outcome::stopped(&status),
            }
            match host
                .custom_launch_recipe(
                    super::mutations::mutation_owner(caller),
                    recipe.recipe_id.clone(),
                )
                .await
            {
                Ok(Some(current)) if current == *recipe => {}
                Ok(_) => {
                    return Outcome::failed(
                        "recipe_changed",
                        "the custom launch recipe changed; ask again",
                    );
                }
                Err(status) => return Outcome::stopped(&status),
            }
            if let Err(status) = ctx.check() {
                return Outcome::stopped(&status);
            }
            let current_plan = match super::super::launch_plan::resolve(
                caller,
                &entry,
                binding.target_revision,
                Some(recipe),
                Some(dispatch),
                &current_resources,
            ) {
                Ok(plan) => plan,
                Err(status) => return Outcome::stopped(&status),
            };
            let current_fingerprint = match tokio::task::spawn_blocking(move || {
                current_plan.fingerprint()
            })
            .await
            {
                Ok(Ok(fingerprint)) => fingerprint,
                _ => {
                    return Outcome::failed("target_changed", "the launch plan changed; ask again");
                }
            };
            if current_fingerprint != binding.plan_fingerprint {
                return Outcome::failed("target_changed", "the launch plan changed; ask again");
            }
        } else if operation == Operation::Launch {
            let current_plan = match super::super::launch_plan::resolve(
                caller,
                &entry,
                binding.target_revision,
                None,
                None,
                &custom_resources,
            ) {
                Ok(plan) => plan,
                Err(status) => return Outcome::stopped(&status),
            };
            let current_fingerprint = match tokio::task::spawn_blocking(move || {
                current_plan.fingerprint()
            })
            .await
            {
                Ok(Ok(fingerprint)) => fingerprint,
                _ => {
                    return Outcome::failed("target_changed", "the launch plan changed; ask again");
                }
            };
            if current_fingerprint != binding.plan_fingerprint {
                return Outcome::failed("target_changed", "the launch plan changed; ask again");
            }
        }
        let dispatch = if operation == Operation::CustomLaunch {
            match custom_dispatch {
                Some(dispatch) => host.launch_custom(dispatch).await,
                None => Err(tonic::Status::failed_precondition(
                    "custom launch recipe is unavailable",
                )),
            }
        } else {
            host.open_entry(entry_key, operation == Operation::Install)
                .await
        };
        match dispatch {
            Ok(()) => Outcome::succeeded(
                "opened",
                match operation {
                    Operation::Launch => "the game was started",
                    Operation::CustomLaunch => "the custom game launch was started",
                    Operation::Install => "the provider's install flow was opened",
                    Operation::BackupRestore => unreachable!("restores use the backup adapter"),
                    Operation::RuntimeSessionContext => {
                        unreachable!("runtime contexts do not dispatch game actions")
                    }
                },
            ),
            Err(status) => Outcome::failed(code_word(&status), status.message()),
        }
    }

    /// Records that the user allowed this plugin to do this to this game.
    async fn remember_answer(
        &self,
        caller: &ResourceCaller,
        context: &ServiceContext,
        permission: &'static str,
        binding: &Binding,
        request: &ConfirmationRequest,
    ) -> Result<(), Status> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| Status::internal("no source of randomness"))?;
        let approval = gamenode_platform::plugin_storage::IntentApproval {
            id: bytes.iter().map(|b| format!("{b:02x}")).collect(),
            game_id: binding.game_id.clone(),
            installation_id: binding.installation_id.clone(),
            operation: binding.operation.as_str().to_owned(),
            target_revision: binding.target_revision,
            recipe_id: binding.recipe_id.clone(),
            recipe_revision: binding.recipe_revision,
            plan_fingerprint: binding.plan_fingerprint.clone(),
            game_name: request.game_name.chars().take(120).collect(),
            approved_at: 0,
        };
        let guard = self.guard(context.clone(), permission, caller.grant_revision);
        self.in_namespace(caller, move |namespace| {
            let approval = gamenode_platform::plugin_storage::IntentApproval {
                approved_at: namespace.now(),
                ..approval
            };
            namespace.save_intent_approval(&approval, &guard)
        })
        .await
    }
}
