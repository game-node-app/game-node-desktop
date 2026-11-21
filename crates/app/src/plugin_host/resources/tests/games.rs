//! `GamesRequestLaunch` and `GamesRequestInstall` over the real local socket (tasks 7.2 and
//! 10.10): the user's intent, the dialog, remembered confirmations and the scope per game.

use std::collections::VecDeque;
use std::sync::Mutex as StdMutex;

use tokio::sync::Notify;

use super::*;
use crate::plugin_host::intents::{Answer, Binding, ConfirmationRequest, Operation, Prompter};
use crate::plugin_host::jobs::JobOwner;

const GRANTS: &[&str] = &[cap::GAME_LAUNCH, cap::GAMES_INSTALL];
const ENTRY: &str = "fixture:1";
const INSTALLATION: &str = "id-fixture:1";

/// The user's side of the dialog: answers in order (declining by default), optionally held until
/// the test lets it go, remembering what it was shown.
#[derive(Default)]
struct Dialog {
    answers: StdMutex<VecDeque<Answer>>,
    shown: StdMutex<Vec<ConfirmationRequest>>,
    held: Option<Arc<Notify>>,
}

#[tonic::async_trait]
impl Prompter for Dialog {
    async fn confirm(&self, request: ConfirmationRequest) -> Result<Answer, Status> {
        self.shown.lock().unwrap().push(request);
        if let Some(gate) = &self.held {
            gate.notified().await;
        }
        Ok(self.answers.lock().unwrap().pop_front().unwrap_or(Answer {
            approved: false,
            remember: false,
        }))
    }
}

const APPROVE: Answer = Answer {
    approved: true,
    remember: false,
};
const APPROVE_AND_REMEMBER: Answer = Answer {
    approved: true,
    remember: true,
};

impl Fixture {
    /// A fixture with one own entry, a fake engine side and (when `dialog` is given) a dialog.
    async fn game(
        grants: &[&'static str],
        dialog: Option<Dialog>,
    ) -> (Self, Arc<FakeHost>, Option<Arc<Dialog>>) {
        let fixture = Self::new(grants).await;
        fixture.library(&[(ENTRY, Some(3), "Game"), ("steam:1", Some(5), "Other")]);
        let host = fixture.with_host();
        let dialog = dialog.map(Arc::new);
        if let Some(dialog) = &dialog {
            fixture.intents.set_prompter(dialog.clone());
        }
        (fixture, host, dialog)
    }

    async fn extension_game(
        grants: &[&'static str],
        target: TargetGameContext,
        dialog: Option<Dialog>,
    ) -> (Self, Arc<FakeHost>, Option<Arc<Dialog>>) {
        let fixture = Fixture::start_with_target(grants, true, None, Some(target)).await;
        fixture.library(&[
            ("steam:1", Some(5), "Selected Steam game"),
            ("steam:2", Some(6), "Other Steam game"),
            ("manual:1", Some(9), "Manual game"),
        ]);
        let host = fixture.with_host();
        let dialog = dialog.map(Arc::new);
        if let Some(dialog) = &dialog {
            fixture.intents.set_prompter(dialog.clone());
        }
        (fixture, host, dialog)
    }

    /// The revision of the fixture's own entry, as the host holds it.
    async fn revision(&self) -> u64 {
        let own = Scope {
            own_provider: "fixture".into(),
            read_all: false,
            provider_filter: None,
        };
        self.feed.target(&own, INSTALLATION).unwrap().1.revision
    }

    async fn launch_game(&self, key: &str, intent: &str) -> Result<GameLaunchReply, Status> {
        let revision = self.revision().await;
        self.launch_at(key, INSTALLATION, revision, intent).await
    }

    async fn launch_at(
        &self,
        key: &str,
        installation_id: &str,
        expected_revision: u64,
        intent: &str,
    ) -> Result<GameLaunchReply, Status> {
        self.client()
            .games_request_launch(GameLaunchRequest {
                context: Some(self.context()),
                game_id: String::new(),
                installation_id: installation_id.into(),
                expected_revision,
                user_intent_id: intent.into(),
                idempotency_key: key.into(),
            })
            .await
            .map(Response::into_inner)
    }

    async fn install_game(&self, key: &str, intent: &str) -> Result<JobReply, Status> {
        self.client()
            .games_request_install(GameInstallRequest {
                context: Some(self.context()),
                game_id: String::new(),
                installation_id: INSTALLATION.into(),
                user_intent_id: intent.into(),
                idempotency_key: key.into(),
            })
            .await
            .map(Response::into_inner)
    }

    /// What the host binds an intent to for the fixture's own entry.
    async fn binding(&self, operation: Operation, target_revision: u64) -> Binding {
        self.binding_for(operation, target_revision, "3", INSTALLATION, "")
            .await
    }

    async fn binding_for(
        &self,
        operation: Operation,
        target_revision: u64,
        game_id: &str,
        installation_id: &str,
        recipe_id: &str,
    ) -> Binding {
        let permission = match operation {
            Operation::Launch => cap::GAME_LAUNCH,
            Operation::CustomLaunch => cap::GAME_CUSTOM_LAUNCH,
            Operation::Install => cap::GAMES_INSTALL,
            Operation::BackupRestore => cap::BACKUPS_RESTORE,
            Operation::RuntimeSessionContext => cap::GAME_SESSIONS_REPORT,
        };
        let caller = self
            .api
            .authorize_resource(Some(&self.context()), permission)
            .unwrap();
        let (_, entry) = self
            .feed
            .target(
                &super::super::Scope {
                    own_provider: caller.provider_id.clone(),
                    read_all: true,
                    provider_filter: None,
                },
                installation_id,
            )
            .unwrap();
        let (recipe, dispatch, resources) = if operation == Operation::CustomLaunch {
            let host = self.feed.host().unwrap();
            let recipe = host
                .custom_launch_recipe(
                    super::super::mutations::mutation_owner(&caller),
                    recipe_id.to_owned(),
                )
                .await
                .unwrap()
                .unwrap();
            let dispatch = host
                .prepare_custom_launch(recipe.clone(), None, None, None)
                .await
                .unwrap();
            (Some(recipe), Some(dispatch), Vec::new())
        } else {
            (None, None, Vec::new())
        };
        let plan = super::super::super::launch_plan::resolve(
            &caller,
            &entry,
            target_revision,
            recipe.as_ref(),
            dispatch.as_ref(),
            &resources,
        )
        .unwrap();
        let plan_fingerprint = if matches!(operation, Operation::Launch | Operation::CustomLaunch) {
            plan.fingerprint().unwrap()
        } else {
            String::new()
        };
        Binding {
            plugin_id: "fixture.plugin".into(),
            publisher_fingerprint: "ab".repeat(32),
            profile_epoch: caller.profile_epoch,
            game_id: game_id.into(),
            installation_id: installation_id.into(),
            operation,
            target_revision,
            recipe_id: recipe_id.into(),
            recipe_revision: u64::from(operation == Operation::CustomLaunch),
            plan_fingerprint,
        }
    }

    fn game_owner(&self) -> JobOwner {
        let permission = [
            cap::GAME_LAUNCH,
            cap::GAME_CUSTOM_LAUNCH,
            cap::GAMES_INSTALL,
            cap::BACKUPS_RESTORE,
        ]
        .into_iter()
        .find(|permission| self.capabilities.contains(permission))
        .expect("game fixture needs an action grant");
        JobOwner::of(
            &self
                .api
                .authorize_resource(Some(&self.context()), permission)
                .unwrap(),
        )
        .unwrap()
    }

    /// Waits until the job is over, reading the host's own record.
    async fn ended(&self, id: &str) -> JobReply {
        self.ended_for(&self.game_owner(), id).await
    }

    async fn ended_for(&self, owner: &JobOwner, id: &str) -> JobReply {
        for _ in 0..400 {
            let reply = self.jobs.get(owner, id).unwrap();
            if !matches!(reply.state(), JobState::Queued | JobState::Running) {
                return reply;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the job did not end");
    }

    async fn dialog_shown(dialog: &Dialog, count: usize) {
        for _ in 0..400 {
            if dialog.shown.lock().unwrap().len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the dialog was not shown");
    }
}

fn held(answers: &[Answer]) -> (Dialog, Arc<Notify>) {
    let gate = Arc::new(Notify::new());
    (
        Dialog {
            answers: StdMutex::new(answers.iter().copied().collect()),
            held: Some(gate.clone()),
            ..Dialog::default()
        },
        gate,
    )
}

fn answering(answers: &[Answer]) -> Dialog {
    Dialog {
        answers: StdMutex::new(answers.iter().copied().collect()),
        ..Dialog::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn without_an_intent_the_job_waits_for_the_user_and_an_approval_starts_the_game() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let dialog = dialog.unwrap();

    let reply = fixture.launch_game("one", "").await.unwrap();
    assert_eq!(
        (reply.state(), reply.message.as_str()),
        (JobState::Queued, "awaiting confirmation")
    );
    Fixture::dialog_shown(&dialog, 1).await;
    // Nothing starts while the user has not answered.
    assert!(host.opened.lock().unwrap().is_empty());
    let shown = dialog.shown.lock().unwrap()[0].clone();
    assert_eq!(
        (
            shown.plugin_id.as_str(),
            shown.game_name.as_str(),
            shown.operation
        ),
        ("fixture.plugin", "Game", Operation::Launch)
    );
    assert_eq!(
        fixture.job(&reply.job_id).await.unwrap().state(),
        JobState::Queued
    );

    gate.notify_one();
    let done = fixture.ended(&reply.job_id).await;
    assert_eq!(
        (done.state(), done.result_code.as_str()),
        (JobState::Succeeded, "opened")
    );
    assert_eq!(*host.opened.lock().unwrap(), [(ENTRY.to_owned(), false)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn declining_starts_nothing() {
    let (fixture, host, _) = Fixture::game(GRANTS, Some(answering(&[]))).await;
    let reply = fixture.launch_game("one", "").await.unwrap();
    let done = fixture.ended(&reply.job_id).await;
    assert_eq!(
        (done.state(), done.result_code.as_str()),
        (JobState::Failed, "declined")
    );
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_waiting_request_withdraws_it_and_frees_the_game() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let dialog = dialog.unwrap();
    let reply = fixture.launch_game("one", "").await.unwrap();
    Fixture::dialog_shown(&dialog, 1).await;

    // One dialog per game at a time: another request while it waits is refused.
    assert_eq!(
        fixture.launch_game("two", "").await.unwrap_err().code(),
        Code::FailedPrecondition
    );
    let withdrawn = fixture.cancel_job("c", &reply.job_id).await.unwrap();
    assert_eq!(withdrawn.state(), JobState::Cancelled);
    // A late approval changes nothing.
    gate.notify_one();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(host.opened.lock().unwrap().is_empty());
    // The slot is free again.
    let again = fixture.launch_game("three", "").await.unwrap();
    assert_eq!(again.state(), JobState::Queued);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_an_install_review_prevents_the_provider_install_flow() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let reply = fixture.install_game("cancel-install", "").await.unwrap();
    Fixture::dialog_shown(&dialog.unwrap(), 1).await;

    assert_eq!(reply.message, "awaiting confirmation");
    assert!(host.opened.lock().unwrap().is_empty());
    let cancelled = fixture
        .cancel_job("cancel-install-job", &reply.job_id)
        .await
        .unwrap();
    assert_eq!(cancelled.state(), JobState::Cancelled);
    gate.notify_one();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pre_issued_install_intent_skips_only_the_plugin_prompt() {
    // Core's explicit action supplies an install intent; the request still enters the provider's
    // install flow and has no launch target revision to validate or bind.
    let (fixture, host, _) = Fixture::game(GRANTS, None).await;
    let binding = fixture.binding(Operation::Install, 0).await;
    let id = fixture.intents.issue(binding).unwrap();
    let reply = fixture
        .install_game("preissued-install", &id)
        .await
        .unwrap();
    let done = fixture.ended(&reply.job_id).await;
    assert_eq!(
        (done.state(), done.result_code.as_str()),
        (JobState::Succeeded, "opened")
    );
    assert_eq!(*host.opened.lock().unwrap(), [(ENTRY.to_owned(), true)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remembered_install_confirmation_is_scoped_to_install() {
    let (fixture, host, dialog) =
        Fixture::game(GRANTS, Some(answering(&[APPROVE_AND_REMEMBER, APPROVE]))).await;
    let dialog = dialog.unwrap();

    let first = fixture
        .install_game("remember-install-one", "")
        .await
        .unwrap();
    assert_eq!(first.message, "awaiting confirmation");
    assert_eq!(
        fixture.ended(&first.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);

    let remembered = fixture
        .install_game("remember-install-two", "")
        .await
        .unwrap();
    assert_ne!(remembered.message, "awaiting confirmation");
    assert_eq!(
        fixture.ended(&remembered.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);
    assert_eq!(host.opened.lock().unwrap().len(), 2);

    // The same stored approval does not authorize launch.
    let launch = fixture
        .launch_game("remember-install-launch", "")
        .await
        .unwrap();
    assert_eq!(launch.message, "awaiting confirmation");
    assert_eq!(dialog.shown.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn revocation_during_install_review_prevents_dispatch() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let owner = fixture.game_owner();
    let reply = fixture.install_game("revoke-install", "").await.unwrap();
    Fixture::dialog_shown(&dialog.unwrap(), 1).await;

    fixture
        .api
        .revoke_session_permission("fixture", cap::GAMES_INSTALL)
        .await;
    gate.notify_one();
    let done = fixture.ended_for(&owner, &reply.job_id).await;
    assert_eq!(done.state(), JobState::Failed);
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retry_opens_no_second_dialog() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let dialog = dialog.unwrap();
    let first = fixture.launch_game("same", "").await.unwrap();
    Fixture::dialog_shown(&dialog, 1).await;
    let retry = fixture.launch_game("same", "").await.unwrap();
    assert_eq!(retry.job_id, first.job_id);
    gate.notify_one();
    fixture.ended(&first.job_id).await;
    // Even after it ended, the same request is still the same job.
    assert_eq!(
        fixture.launch_game("same", "").await.unwrap().job_id,
        first.job_id
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);
    assert_eq!(host.opened.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_launch_recipes_are_idempotent_requestable_and_removable() {
    let (fixture, host, _) = Fixture::game(
        &[cap::GAME_LAUNCH, cap::GAME_CUSTOM_LAUNCH],
        Some(answering(&[APPROVE])),
    )
    .await;
    host.launch_target.store(1, Ordering::SeqCst);
    let revision = fixture.revision().await;
    let proposal = CustomLaunchRequest {
        context: Some(fixture.context()),
        game_id: "3".into(),
        installation_id: INSTALLATION.into(),
        expected_revision: revision,
        idempotency_key: "propose-custom-1".into(),
        uri: "steam://rungameid/3".into(),
        ..CustomLaunchRequest::default()
    };

    let first = fixture
        .client()
        .games_propose_custom_launch(proposal.clone())
        .await
        .unwrap()
        .into_inner();
    let replay = fixture
        .client()
        .games_propose_custom_launch(proposal.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(replay.recipe_id, first.recipe_id);
    assert_eq!(replay.recipe_revision, first.recipe_revision);
    assert_eq!(host.custom_launch_recipes.lock().unwrap().len(), 1);

    let mut changed = proposal;
    changed.uri = "steam://rungameid/4".into();
    assert_eq!(
        fixture
            .client()
            .games_propose_custom_launch(changed)
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );

    let intent = fixture
        .intents
        .issue(
            fixture
                .binding_for(
                    Operation::CustomLaunch,
                    1,
                    "3",
                    INSTALLATION,
                    &first.recipe_id,
                )
                .await,
        )
        .unwrap();
    let request = CustomLaunchRequest {
        context: Some(fixture.context()),
        game_id: "3".into(),
        installation_id: INSTALLATION.into(),
        expected_revision: revision,
        user_intent_id: intent,
        idempotency_key: "execute-custom-1".into(),
        recipe_id: first.recipe_id.clone(),
        recipe_revision: first.recipe_revision,
        ..CustomLaunchRequest::default()
    };
    let reply = fixture
        .client()
        .games_request_custom_launch(request)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        fixture.ended(&reply.job_id).await.state(),
        JobState::Succeeded
    );
    assert!(matches!(
        host.custom_launch_dispatches.lock().unwrap().as_slice(),
        [crate::plugin_host::library::CustomLaunchDispatch::Uri { uri, .. }]
            if uri.as_str() == "steam://rungameid/3"
    ));

    let removed = fixture
        .client()
        .games_remove_custom_launch(CustomLaunchRemoveRequest {
            mutation: Some(fixture.mutation("remove-custom-1", first.recipe_revision)),
            game_id: "3".into(),
            recipe_id: first.recipe_id,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(removed.outcome, MutationOutcome::Applied as i32);
    assert!(host.custom_launch_recipes.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn user_play_reviews_selected_recipe_remembers_exact_plan_and_never_falls_back() {
    let (fixture, host, dialog) = Fixture::game(
        &[cap::GAME_LAUNCH, cap::GAME_CUSTOM_LAUNCH],
        Some(answering(&[APPROVE_AND_REMEMBER])),
    )
    .await;
    host.launch_target.store(1, Ordering::SeqCst);

    let profiles = fixture.api.profiles();
    let layout = crate::plugin_host::profile::PluginStateLayout::new(
        fixture.plugin_data.join("host-launch-profile"),
    );
    profiles.set_state_layout(layout.clone());
    let profile = profiles.current().unwrap();
    let identity = crate::plugin_host::grants::PluginIdentity {
        plugin_id: "fixture.plugin".into(),
        publisher_fingerprint: "ab".repeat(32),
        profile_id: profile.profile_id().to_owned(),
    };
    let scope = crate::plugin_host::grants::GrantScope::Declared {
        value: "host-selected-game".into(),
    };
    let request = crate::plugin_host::grants::PermissionRequest {
        permission_id: cap::GAME_CUSTOM_LAUNCH.into(),
        required: false,
        allowed_scopes: [scope.clone()].into(),
    };
    layout
        .open_grants(&profile)
        .unwrap()
        .replace_grants(
            identity.clone(),
            &[request],
            &[crate::plugin_host::grants::PermissionDecision {
                permission_id: cap::GAME_CUSTOM_LAUNCH.into(),
                scope,
                granted: true,
            }],
        )
        .unwrap();
    layout
        .open_activation(&profile)
        .unwrap()
        .set_enabled(&identity.plugin_id, true)
        .unwrap();

    let revision = fixture.revision().await;
    let recipe = fixture
        .client()
        .games_propose_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "host-play-recipe".into(),
            uri: "steam://rungameid/3".into(),
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    let resolver = profiles
        .resolver("fixture.plugin", &"ab".repeat(32), None)
        .unwrap();
    host.launch_recipe_selections.lock().unwrap().insert(
        "fixture:3".into(),
        gamenode_core::library::LaunchRecipeSelection {
            entry_key: "fixture:3".into(),
            owner_key: resolver.library_recipe_owner_key(),
            plugin_id: identity.plugin_id.clone(),
            publisher_fingerprint: identity.publisher_fingerprint.clone(),
            recipe_id: recipe.recipe_id,
            recipe_revision: recipe.recipe_revision,
            base_target_revision: 1,
            approved_plan_fingerprint: String::new(),
        },
    );

    let api = fixture._server.resource_api();
    assert!(
        api.launch_selected_recipe("fixture:3".into())
            .await
            .unwrap()
    );
    let shown = dialog.as_ref().unwrap().shown.lock().unwrap().clone();
    assert_eq!(shown.len(), 1);
    let reviewed = shown[0].launch_plan.as_ref().unwrap();
    assert_eq!(reviewed.source_id, "fixture");
    assert_eq!(reviewed.target, "steam://rungameid/3");
    assert!(!reviewed.fingerprint.is_empty());

    // The exact same plan uses the user-approved fingerprint and does not prompt again.
    assert!(
        api.launch_selected_recipe("fixture:3".into())
            .await
            .unwrap()
    );
    assert_eq!(dialog.as_ref().unwrap().shown.lock().unwrap().len(), 1);
    assert_eq!(host.custom_launch_dispatches.lock().unwrap().len(), 2);

    // A changed detected target invalidates the selected recipe and blocks all dispatch.
    host.launch_target.store(2, Ordering::SeqCst);
    assert_eq!(
        api.launch_selected_recipe("fixture:3".into())
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(host.custom_launch_dispatches.lock().unwrap().len(), 2);

    // Revoking custom-launch permission after approval is also fail-closed.
    let revoked_request = crate::plugin_host::grants::PermissionRequest {
        permission_id: cap::GAME_CUSTOM_LAUNCH.into(),
        required: false,
        allowed_scopes: [crate::plugin_host::grants::GrantScope::Declared {
            value: "host-selected-game".into(),
        }]
        .into(),
    };
    layout
        .open_grants(&profile)
        .unwrap()
        .replace_grants(identity, &[revoked_request], &[])
        .unwrap();
    host.launch_target.store(1, Ordering::SeqCst);
    assert_eq!(
        api.launch_selected_recipe("fixture:3".into())
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(host.custom_launch_dispatches.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn user_play_reviews_runtime_overlay_and_reprompts_when_runtime_revision_changes() {
    use crate::plugin_host::profile::PluginStateLayout;
    use gamenode_plugin_protocol::proto::{
        ContentPathArgument, RuntimeArgumentTemplate as RuntimeArgumentProto, RuntimeRecord,
        runtime_argument_template::Value as RuntimeArgumentValue,
    };

    let target = TargetGameContext {
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        profile_id: "account-a".into(),
        revision: "1".into(),
    };
    let (fixture, host, dialog) = Fixture::extension_game(
        &[
            cap::DISCOVERY,
            cap::FILES_READ,
            cap::GAME_CUSTOM_LAUNCH,
            cap::GAME_RUNTIME_CREATE,
        ],
        target,
        Some(answering(&[
            APPROVE_AND_REMEMBER,
            Answer {
                approved: false,
                remember: false,
            },
        ])),
    )
    .await;
    let dialog = dialog.unwrap();
    host.launch_target.store(1, Ordering::SeqCst);

    let content_root = fixture.game_dir(
        "steam-runtime",
        &[("zelda.sfc", b"rom"), ("emulator", b"runtime")],
    );
    fixture.library_in(&[("steam:1", Some(5), "Selected Steam game", &content_root)]);
    fixture.user_approving(&content_root);
    let root = fixture.root_handle("id-steam:1", &["read"]).await;
    let runtime_executable = fixture.child(&root, "emulator").await;
    let content_handle = fixture.child(&root, "zelda.sfc").await;
    let context = fixture.context();
    let caller = fixture
        .api
        .authorize_resource(Some(&context), cap::GAME_RUNTIME_CREATE)
        .unwrap();
    let owner = super::super::mutations::mutation_owner(&caller);

    fixture
        .client()
        .runtimes_register(RuntimeRegisterRequest {
            mutation: Some(fixture.mutation("register-runtime-for-play", 0)),
            runtime: Some(RuntimeRecord {
                id: "runtime:snes".into(),
                name: "SNES runtime".into(),
                version: "1.0".into(),
                executable_handle: runtime_executable,
                supported_formats: vec!["snes-rom".into()],
                platform: std::env::consts::OS.into(),
                argument_template: vec![RuntimeArgumentProto {
                    value: Some(RuntimeArgumentValue::ContentPath(ContentPathArgument {})),
                }],
                ..Default::default()
            }),
        })
        .await
        .unwrap();
    fixture
        .client()
        .games_register_content(ContentRegistrationRequest {
            mutation: Some(fixture.mutation("register-content-for-play", 0)),
            game_id: "5".into(),
            installation_id: "id-steam:1".into(),
            content_handle,
            content_kind: "snes-rom".into(),
            runtime_id: "runtime:snes".into(),
            supported_platforms: vec![std::env::consts::OS.into()],
            content_id: "rom:zelda".into(),
        })
        .await
        .unwrap();

    let revision = fixture.feed.revision_of("steam:1").unwrap();
    let recipe = fixture
        .client()
        .games_propose_custom_launch(CustomLaunchRequest {
            context: Some(context),
            game_id: "5".into(),
            installation_id: "id-steam:1".into(),
            expected_revision: revision,
            idempotency_key: "runtime-recipe-for-play".into(),
            runtime_handle: "runtime:snes".into(),
            content_id: "rom:zelda".into(),
            argv: vec!["--pixel-perfect".into()],
            environment_set: [("GAME_MODE".into(), "fixture".into())].into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    let profiles = fixture.api.profiles();
    let layout = PluginStateLayout::new(fixture.plugin_data.join("host-runtime-play-profile"));
    profiles.set_state_layout(layout.clone());
    let profile = profiles.current().unwrap();
    let identity = crate::plugin_host::grants::PluginIdentity {
        plugin_id: "fixture.plugin".into(),
        publisher_fingerprint: "ab".repeat(32),
        profile_id: profile.profile_id().to_owned(),
    };
    let scope = crate::plugin_host::grants::GrantScope::Declared {
        value: "host-selected-game".into(),
    };
    let requests = [cap::GAME_CUSTOM_LAUNCH, cap::FILES_READ]
        .into_iter()
        .map(
            |permission_id| crate::plugin_host::grants::PermissionRequest {
                permission_id: permission_id.into(),
                required: false,
                allowed_scopes: [scope.clone()].into(),
            },
        )
        .collect::<Vec<_>>();
    let decisions = [cap::GAME_CUSTOM_LAUNCH, cap::FILES_READ]
        .into_iter()
        .map(
            |permission_id| crate::plugin_host::grants::PermissionDecision {
                permission_id: permission_id.into(),
                scope: scope.clone(),
                granted: true,
            },
        )
        .collect::<Vec<_>>();
    layout
        .open_grants(&profile)
        .unwrap()
        .replace_grants(identity.clone(), &requests, &decisions)
        .unwrap();
    layout
        .open_activation(&profile)
        .unwrap()
        .set_enabled("fixture.plugin", true)
        .unwrap();
    let resolver = profiles
        .resolver("fixture.plugin", &"ab".repeat(32), None)
        .unwrap();
    let stored_recipe = host
        .custom_launch_recipe(
            resolver.library_recipe_owner_key(),
            recipe.recipe_id.clone(),
        )
        .await
        .unwrap()
        .unwrap();
    host.launch_recipe_selections.lock().unwrap().insert(
        "steam:1".into(),
        gamenode_core::library::LaunchRecipeSelection {
            entry_key: "steam:1".into(),
            owner_key: resolver.library_recipe_owner_key(),
            plugin_id: identity.plugin_id,
            publisher_fingerprint: identity.publisher_fingerprint,
            recipe_id: stored_recipe.recipe_id.clone(),
            recipe_revision: stored_recipe.revision,
            base_target_revision: stored_recipe.base_target_revision,
            approved_plan_fingerprint: String::new(),
        },
    );

    let api = fixture._server.resource_api();
    assert!(api.launch_selected_recipe("steam:1".into()).await.unwrap());
    let first_review = dialog.shown.lock().unwrap()[0].launch_plan.clone().unwrap();
    assert_eq!(first_review.source_id, "steam");
    assert_eq!(first_review.requester_plugin_id, "fixture.plugin");
    assert_eq!(first_review.arguments.len(), 2);
    assert!(first_review.arguments[0].ends_with("/zelda.sfc"));
    assert_eq!(first_review.arguments[1], "--pixel-perfect");
    assert_eq!(first_review.environment_set["GAME_MODE"], "fixture");
    assert!(
        first_review
            .components
            .iter()
            .any(|component| component.kind == "runtime" && component.id == "runtime:snes")
    );
    assert!(
        first_review
            .differences
            .contains(&"known-components".into())
    );
    assert!(
        first_review
            .differences
            .contains(&"custom-arguments".into())
    );
    assert!(
        first_review
            .differences
            .contains(&"environment-changes".into())
    );

    // The same immutable plan reuses its approval and dispatches without a second dialog.
    assert!(api.launch_selected_recipe("steam:1".into()).await.unwrap());
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);

    // A changed runtime revision changes the reviewed plan and a user refusal stops dispatch.
    let mut updated = host
        .runtime_resource(owner.clone(), "runtime:snes".into())
        .await
        .unwrap()
        .unwrap();
    updated.version = "1.1".into();
    updated.revision += 1;
    host.save_runtime_resource(owner, updated).await.unwrap();
    assert!(!api.launch_selected_recipe("steam:1".into()).await.unwrap());
    assert_eq!(dialog.shown.lock().unwrap().len(), 2);
    assert_eq!(host.custom_launch_dispatches.lock().unwrap().len(), 2);
    let stored = host
        .custom_launch_recipes
        .lock()
        .unwrap()
        .values()
        .find(|stored| stored.recipe_id == recipe.recipe_id)
        .cloned()
        .unwrap();
    assert!(stored.validate());
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_launch_recipe_removal_during_confirmation_blocks_dispatch() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) =
        Fixture::game(&[cap::GAME_LAUNCH, cap::GAME_CUSTOM_LAUNCH], Some(dialog)).await;
    host.launch_target.store(1, Ordering::SeqCst);
    let revision = fixture.revision().await;
    let recipe = fixture
        .client()
        .games_propose_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "propose-recipe-before-confirmation".into(),
            uri: "steam://rungameid/3".into(),
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    let pending = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "execute-recipe-after-confirmation".into(),
            recipe_id: recipe.recipe_id.clone(),
            recipe_revision: recipe.recipe_revision,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    Fixture::dialog_shown(&dialog.unwrap(), 1).await;

    let removed = fixture
        .client()
        .games_remove_custom_launch(CustomLaunchRemoveRequest {
            mutation: Some(fixture.mutation("remove-during-confirmation", recipe.recipe_revision)),
            game_id: "3".into(),
            recipe_id: recipe.recipe_id,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(removed.outcome, MutationOutcome::Applied as i32);
    gate.notify_one();

    let ended = fixture.ended(&pending.job_id).await;
    assert_eq!(ended.state(), JobState::Failed);
    assert_eq!(ended.result_code, "recipe_changed");
    assert!(host.custom_launch_dispatches.lock().unwrap().is_empty());
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn remembered_custom_launch_approval_is_bound_to_recipe_identity() {
    let (fixture, host, dialog) = Fixture::game(
        &[cap::GAME_LAUNCH, cap::GAME_CUSTOM_LAUNCH],
        Some(answering(&[APPROVE_AND_REMEMBER, APPROVE])),
    )
    .await;
    let dialog = dialog.unwrap();
    host.launch_target.store(1, Ordering::SeqCst);
    let revision = fixture.revision().await;
    let propose = |key: &'static str, uri: &'static str| CustomLaunchRequest {
        context: Some(fixture.context()),
        game_id: "3".into(),
        installation_id: INSTALLATION.into(),
        expected_revision: revision,
        idempotency_key: key.into(),
        uri: uri.into(),
        ..CustomLaunchRequest::default()
    };
    let first = fixture
        .client()
        .games_propose_custom_launch(propose("remember-recipe-a", "steam://rungameid/3"))
        .await
        .unwrap()
        .into_inner();
    let first_recipe_id = first.recipe_id.clone();
    let first_recipe_revision = first.recipe_revision;
    let first_job = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "run-recipe-a".into(),
            recipe_id: first.recipe_id,
            recipe_revision: first.recipe_revision,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        fixture.ended(&first_job.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);
    let remembered_job = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "run-remembered-recipe-a".into(),
            recipe_id: first_recipe_id,
            recipe_revision: first_recipe_revision,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        fixture.ended(&remembered_job.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);

    let second = fixture
        .client()
        .games_propose_custom_launch(propose("remember-recipe-b", "steam://rungameid/4"))
        .await
        .unwrap()
        .into_inner();
    let second_job = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "run-recipe-b".into(),
            recipe_id: second.recipe_id,
            recipe_revision: second.recipe_revision,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        fixture.ended(&second_job.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pre_issued_intent_skips_the_dialog_and_is_spent_once() {
    // No dialog at all in this build: the intent is enough.
    let (fixture, host, _) = Fixture::game(GRANTS, None).await;
    let id = fixture
        .intents
        .issue(fixture.binding(Operation::Launch, 0).await)
        .unwrap();
    let reply = fixture.launch_game("one", &id).await.unwrap();
    let done = fixture.ended(&reply.job_id).await;
    assert_eq!(done.state(), JobState::Succeeded);
    assert_eq!(host.opened.lock().unwrap().len(), 1);

    // The same request again is the same job; the intent is not spent twice.
    assert_eq!(
        fixture.launch_game("one", &id).await.unwrap().job_id,
        reply.job_id
    );
    // The spent intent is worthless for a new request.
    assert_eq!(
        fixture.launch_game("two", &id).await.unwrap_err().code(),
        Code::FailedPrecondition
    );
    assert_eq!(host.opened.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_intent_falls_back_to_confirmation_for_the_current_action() {
    let dialog = Dialog {
        answers: StdMutex::new(vec![APPROVE; 3].into()),
        ..Dialog::default()
    };
    let (fixture, host, shown) = Fixture::game(GRANTS, Some(dialog)).await;
    // Issued for an install: not a launch.
    let install = fixture
        .intents
        .issue(fixture.binding(Operation::Install, 0).await)
        .unwrap();
    // Issued for a target that is not the current one.
    let stale = fixture
        .intents
        .issue(fixture.binding(Operation::Launch, 99).await)
        .unwrap();
    for (key, intent) in [
        ("a", install.as_str()),
        ("b", stale.as_str()),
        ("c", "intent-forged"),
    ] {
        let reply = fixture.launch_game(key, intent).await.unwrap();
        assert_eq!(
            fixture.ended(&reply.job_id).await.state(),
            JobState::Succeeded
        );
    }
    assert_eq!(shown.unwrap().shown.lock().unwrap().len(), 3);
    assert_eq!(host.opened.lock().unwrap().len(), 3);
    // The mismatched intent was not spent: its operation can still redeem it.
    let reply = fixture.install_game("d", &install).await.unwrap();
    assert_eq!(
        fixture.ended(&reply.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(host.opened.lock().unwrap().len(), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_build_without_a_dialog_cannot_ask_and_starts_no_job() {
    let (fixture, host, _) = Fixture::game(GRANTS, None).await;
    let status = fixture.launch_game("one", "").await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert!(host.opened.lock().unwrap().is_empty());
    assert_eq!(fixture.jobs.active(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remembered_confirmation_skips_the_dialog_until_the_target_or_operation_changes() {
    let (fixture, host, dialog) = Fixture::game(
        GRANTS,
        Some(answering(&[
            APPROVE_AND_REMEMBER,
            APPROVE,
            APPROVE_AND_REMEMBER,
        ])),
    )
    .await;
    let dialog = dialog.unwrap();

    // The first launch asks, and the user chooses to remember.
    let first = fixture.launch_game("one", "").await.unwrap();
    assert_eq!(
        fixture.ended(&first.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);

    // The next one starts at once: no dialog, no waiting job.
    let second = fixture.launch_game("two", "").await.unwrap();
    assert_ne!(second.message, "awaiting confirmation");
    assert_eq!(
        fixture.ended(&second.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);
    assert_eq!(host.opened.lock().unwrap().len(), 2);

    // The game's detected target changed: it asks again.
    host.launch_target.store(41, Ordering::SeqCst);
    let third = fixture.launch_game("three", "").await.unwrap();
    assert_eq!(third.message, "awaiting confirmation");
    fixture.ended(&third.job_id).await;
    assert_eq!(dialog.shown.lock().unwrap().len(), 2);

    // Launching and installing are different approvals.
    let install = fixture.install_game("four", "").await.unwrap();
    assert_eq!(install.message, "awaiting confirmation");
    fixture.ended(&install.job_id).await;
    assert_eq!(dialog.shown.lock().unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remembered_confirmation_survives_a_restart_and_can_be_revoked() {
    let (fixture, _, dialog) =
        Fixture::game(GRANTS, Some(answering(&[APPROVE_AND_REMEMBER, APPROVE]))).await;
    let first = fixture.launch_game("one", "").await.unwrap();
    fixture.ended(&first.job_id).await;

    let caller = fixture
        .api
        .authorize_resource(Some(&fixture.context()), GRANTS[0])
        .unwrap();
    let listed = fixture
        .storage
        .run_for_host(
            caller.namespace.as_ref().unwrap(),
            caller.profile_epoch,
            |namespace| namespace.intent_approvals(),
        )
        .await
        .unwrap();
    let [approval] = listed.try_into().unwrap();
    assert_eq!(
        (
            approval.operation.as_str(),
            approval.game_id.as_str(),
            approval.game_name.as_str()
        ),
        ("launch", "3", "Game")
    );

    // Revoked from the settings: the next request asks again.
    let id = approval.id.clone();
    assert!(
        fixture
            .storage
            .run_for_host(
                caller.namespace.as_ref().unwrap(),
                caller.profile_epoch,
                move |namespace| namespace.revoke_intent_approval(&id),
            )
            .await
            .unwrap()
    );
    let again = fixture.launch_game("two", "").await.unwrap();
    assert_eq!(again.message, "awaiting confirmation");
    fixture.ended(&again.job_id).await;
    assert_eq!(dialog.unwrap().shown.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn losing_the_permission_while_the_dialog_is_open_prevents_the_start() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let reply = fixture.launch_game("one", "").await.unwrap();
    Fixture::dialog_shown(&dialog.unwrap(), 1).await;

    // The host's own record needs no session, so keep the owner before the permission goes.
    let owner = fixture.game_owner();
    fixture
        .api
        .revoke_session_permission("fixture", cap::GAME_LAUNCH)
        .await;
    gate.notify_one();
    let done = fixture.ended_for(&owner, &reply.job_id).await;
    assert_eq!(done.state(), JobState::Failed);
    assert!(host.opened.lock().unwrap().is_empty());
    // And the plugin can no longer ask for anything.
    assert_eq!(
        fixture.launch_game("two", "").await.unwrap_err().code(),
        Code::PermissionDenied
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_target_that_changed_while_the_dialog_was_open_is_not_started() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let reply = fixture.launch_game("one", "").await.unwrap();
    Fixture::dialog_shown(&dialog.unwrap(), 1).await;

    host.launch_target.store(7, Ordering::SeqCst);
    gate.notify_one();
    let done = fixture.ended(&reply.job_id).await;
    assert_eq!(
        (done.state(), done.result_code.as_str()),
        (JobState::Failed, "target_changed")
    );
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_install_opens_the_providers_flow_and_reports_what_it_cannot_do() {
    let (fixture, host, _) = Fixture::game(GRANTS, Some(answering(&[APPROVE, APPROVE]))).await;
    let reply = fixture.install_game("one", "").await.unwrap();
    assert_eq!(fixture.ended(&reply.job_id).await.result_code, "opened");
    assert_eq!(*host.opened.lock().unwrap(), [(ENTRY.to_owned(), true)]);

    // A provider with no such action: the job says so, instead of pretending.
    *host.open_fails.lock().unwrap() = Some(Code::Unimplemented);
    let unsupported = fixture.install_game("two", "").await.unwrap();
    let done = fixture.ended(&unsupported.job_id).await;
    assert_eq!(
        (done.state(), done.result_code.as_str()),
        (JobState::Failed, "unsupported")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_permission_of_the_operation_and_a_valid_request_get_this_far() {
    let (dialog, _gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(&[cap::GAME_LAUNCH], Some(dialog)).await;
    let dialog = dialog.unwrap();
    // The launch grant is not an install grant, and nothing is asked.
    assert_eq!(
        fixture.install_game("a", "").await.unwrap_err().code(),
        Code::PermissionDenied
    );
    // A stale revision, a foreign game, an empty installation and a bad key never ask either.
    let revision = fixture.revision().await;
    assert_eq!(
        fixture
            .launch_at("b", INSTALLATION, revision + 1, "")
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(
        fixture
            .launch_at("c", "id-steam:1", 0, "")
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        fixture.launch_at("d", "", 0, "").await.unwrap_err().code(),
        Code::InvalidArgument
    );
    assert_eq!(
        fixture
            .launch_at("", INSTALLATION, revision, "")
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert_eq!(
        fixture
            .launch_at("e", INSTALLATION, revision, &"x".repeat(129))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert!(dialog.shown.lock().unwrap().is_empty());
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_extension_session_reaches_only_its_target_and_reading_the_library_adds_nothing() {
    let (fixture, _, _) = Fixture::game(&[cap::GAME_LAUNCH, cap::LIBRARY_READ_ALL], None).await;
    let caller = fixture
        .api
        .authorize_resource(Some(&fixture.context()), GRANTS[0])
        .unwrap();
    let service = fixture.service();
    let extension = |installation: &str, game: &str| {
        let mut context = fixture.context();
        context.session_kind = SessionKind::Extension as i32;
        context.target_game = Some(TargetGameContext {
            game_id: game.into(),
            installation_id: installation.into(),
            profile_id: String::new(),
            revision: String::new(),
        });
        context
    };

    // The selected game of another provider is reachable, whole library or not.
    let (key, entry) = service
        .action_target(&caller, &extension("id-steam:1", "5"), "", "id-steam:1")
        .unwrap();
    assert_eq!(
        (key.as_str(), entry.provider_id.as_str()),
        ("steam:1", "steam")
    );
    // Any other game is not, even though the plugin can read it.
    for (context, installation) in [
        (extension("id-steam:1", "5"), INSTALLATION),
        (extension("id-steam:1", "6"), "id-steam:1"),
        (extension(INSTALLATION, "3"), "id-steam:1"),
    ] {
        assert_eq!(
            service
                .action_target(&caller, &context, "", installation)
                .unwrap_err()
                .code(),
            Code::NotFound
        );
    }
    // An extension session with no selected game names nothing.
    let mut bare = fixture.context();
    bare.session_kind = SessionKind::Extension as i32;
    assert_eq!(
        service
            .action_target(&caller, &bare, "", INSTALLATION)
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    // A provider session reaches only its own entries, whatever library.read.all says.
    assert_eq!(
        service
            .action_target(&caller, &fixture.context(), "", "id-steam:1")
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert!(
        service
            .action_target(&caller, &fixture.context(), "", INSTALLATION)
            .is_ok()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_content_discovery_requires_a_scoped_context_and_separate_file_grant() {
    let target = TargetGameContext {
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        profile_id: "account-a".into(),
        revision: "1".into(),
    };
    let (fixture, _, _) =
        Fixture::extension_game(&[cap::DISCOVERY, cap::GAME_CUSTOM_LAUNCH], target, None).await;
    let context = fixture.context();
    assert!(
        context.provider_id.is_empty(),
        "extensions do not impersonate providers"
    );
    let request = |context: Option<ServiceContext>, game_id: &str, installation_id: &str| {
        ContentRegistrationRequest {
            mutation: Some(MutationContext {
                context,
                operation_id: String::new(),
                idempotency_key: "runtime-content-discovery".into(),
                expected_revision: 0,
            }),
            game_id: game_id.into(),
            installation_id: installation_id.into(),
            content_handle: "approved-content-handle".into(),
            content_kind: "snes-rom".into(),
            runtime_id: "runtime-zsnes".into(),
            supported_platforms: vec!["linux".into()],
            content_id: "rom:zelda".into(),
        }
    };

    // The extension's host-issued context is bound to the selected game. Omitting it or
    // substituting another session/target cannot turn discovery into provider impersonation.
    assert_eq!(
        fixture
            .client()
            .games_register_content(request(None, "5", "id-steam:1"))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    let mut unscoped = context.clone();
    unscoped.target_game = None;
    assert_eq!(
        fixture
            .client()
            .games_register_content(request(Some(unscoped), "5", "id-steam:1"))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let foreign = Fixture::new(&[cap::DISCOVERY]).await;
    assert_eq!(
        fixture
            .client()
            .games_register_content(request(Some(foreign.context()), "5", "id-steam:1",))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    assert_eq!(
        fixture
            .client()
            .games_register_content(request(Some(context.clone()), "6", "id-steam:2"))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied,
        "discovery alone cannot read an approved content handle"
    );

    // A provider session with discovery alone cannot launch its own game; execution has an
    // independent permission. Extensions also need a game-action grant to negotiate a session.
    let launch_discovery_only = Fixture::new(&[cap::DISCOVERY]).await;
    launch_discovery_only.library(&[("fixture:1", Some(5), "Observed game")]);
    assert_eq!(
        launch_discovery_only
            .client()
            .games_request_launch(GameLaunchRequest {
                context: Some(launch_discovery_only.context()),
                game_id: "5".into(),
                installation_id: "id-fixture:1".into(),
                expected_revision: 0,
                user_intent_id: String::new(),
                idempotency_key: "discovery-is-not-launch".into(),
            })
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );

    // A properly scoped but foreign game remains outside the extension's selected context even
    // when file reading is independently granted.
    let (with_file_read, _, _) = Fixture::extension_game(
        &[cap::DISCOVERY, cap::FILES_READ, cap::GAME_CUSTOM_LAUNCH],
        TargetGameContext {
            game_id: "5".into(),
            installation_id: "id-steam:1".into(),
            profile_id: "account-a".into(),
            revision: "1".into(),
        },
        None,
    )
    .await;
    assert_eq!(
        with_file_read
            .client()
            .games_register_content(request(Some(with_file_read.context()), "6", "id-steam:2",))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_plugins_reconcile_observations_as_selectable_bindings_on_one_game() {
    use gamenode_core::library::{ApprovedFileReference, RuntimeArgumentTemplate, RuntimeResource};

    let target = TargetGameContext {
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        profile_id: "account-a".into(),
        revision: "1".into(),
    };
    let (fixture, host, _) = Fixture::extension_game(
        &[cap::DISCOVERY, cap::FILES_READ, cap::GAME_CUSTOM_LAUNCH],
        target,
        None,
    )
    .await;
    let content_root = fixture.game_dir("selected-rom", &[("zelda.sfc", b"rom bytes")]);
    fixture.library_in(&[
        ("steam:1", Some(5), "Selected Steam game", &content_root),
        ("steam:2", Some(6), "Other Steam game", &content_root),
    ]);
    let library_revision = fixture.feed.revision();
    fixture.user_approving(&content_root);
    let root = fixture.root_handle("id-steam:1", &["read"]).await;
    let content_handle = fixture.child(&root, "zelda.sfc").await;
    let context = fixture.context();
    let caller = fixture
        .api
        .authorize_resource(Some(&context), cap::DISCOVERY)
        .unwrap();
    let owner = super::super::mutations::mutation_owner(&caller);

    for id in ["runtime-zsnes", "runtime-bsnes"] {
        host.seed_runtime_resource(
            owner.clone(),
            RuntimeResource {
                id: id.into(),
                name: id.into(),
                version: "1.0".into(),
                executable: ApprovedFileReference {
                    approval_id: format!("runtime-approval-{id}"),
                    relative_path: vec!["bin".into(), "emulator".into()],
                },
                platform: "linux".into(),
                supported_formats: vec!["snes-rom".into()],
                launch_arguments: vec![RuntimeArgumentTemplate::ContentPath],
                revision: 1,
                observed_at: 1,
                provenance: Default::default(),
                extensions: Default::default(),
            },
        );
    }

    // The stable id came from the user's selected existing content context; this owner's
    // namespace has no previous binding for it.
    let content_id = "rom:zelda".to_owned();
    for (id, key) in [
        ("runtime-zsnes", "discover-zsnes"),
        ("runtime-bsnes", "discover-bsnes"),
    ] {
        let reply = fixture
            .client()
            .games_register_content(ContentRegistrationRequest {
                mutation: Some(MutationContext {
                    context: Some(context.clone()),
                    operation_id: String::new(),
                    idempotency_key: key.into(),
                    expected_revision: 0,
                }),
                game_id: "5".into(),
                installation_id: "id-steam:1".into(),
                content_handle: content_handle.clone(),
                content_kind: "snes-rom".into(),
                runtime_id: id.into(),
                supported_platforms: vec!["linux".into()],
                content_id: content_id.clone(),
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(reply.revision, 1);
        assert_eq!(reply.outcome, MutationOutcome::Applied as i32);
        assert_eq!(reply.resource_id, content_id);
    }

    let bindings = host
        .runtime_content_bindings(owner, 5, "id-steam:1".into())
        .await
        .unwrap();
    assert_eq!(bindings.len(), 2);
    assert!(
        bindings
            .iter()
            .all(|binding| binding.content_id == content_id)
    );
    assert_eq!(
        bindings
            .iter()
            .map(|binding| binding.runtime_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        ["runtime-bsnes", "runtime-zsnes"].into()
    );
    assert_eq!(
        fixture.feed.revision(),
        library_revision,
        "runtime observations attach to existing entries and never create another library entry"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_content_discovery_keeps_legacy_owner_scope_and_requires_host_context() {
    use gamenode_core::library::{ApprovedFileReference, RuntimeArgumentTemplate, RuntimeResource};

    let (fixture, host, _) = Fixture::game(&[cap::DISCOVERY, cap::FILES_READ], None).await;
    let content_root = fixture.game_dir("provider-content", &[("game.rom", b"fixture bytes")]);
    fixture.library_in(&[("fixture:1", Some(3), "Provider game", &content_root)]);
    fixture.user_approving(&content_root);
    let root = fixture.root_handle("id-fixture:1", &["read"]).await;
    let content_handle = fixture.child(&root, "game.rom").await;
    let context = fixture.context();

    // Legacy provider sessions have their provider identity and no selected-game extension
    // authority. RegisterContent remains available through the host-issued resource context.
    assert_eq!(context.session_kind(), SessionKind::Unspecified);
    assert!(context.target_game.is_none());
    let caller = fixture
        .api
        .authorize_resource(Some(&context), cap::DISCOVERY)
        .unwrap();
    let owner = super::super::mutations::mutation_owner(&caller);
    host.seed_runtime_resource(
        owner,
        RuntimeResource {
            id: "provider-runtime".into(),
            name: "Provider runtime".into(),
            version: "1.0".into(),
            executable: ApprovedFileReference {
                approval_id: "runtime-approval-provider".into(),
                relative_path: vec!["bin".into(), "runtime".into()],
            },
            platform: "linux".into(),
            supported_formats: vec!["fixture-rom".into()],
            launch_arguments: vec![RuntimeArgumentTemplate::ContentPath],
            revision: 1,
            observed_at: 1,
            provenance: Default::default(),
            extensions: Default::default(),
        },
    );

    let registration = |context: Option<ServiceContext>, game_id: &str, installation_id: &str| {
        ContentRegistrationRequest {
            mutation: Some(MutationContext {
                context,
                operation_id: String::new(),
                idempotency_key: format!("provider-content-{game_id}-{installation_id}"),
                expected_revision: 0,
            }),
            game_id: game_id.into(),
            installation_id: installation_id.into(),
            content_handle: content_handle.clone(),
            content_kind: "fixture-rom".into(),
            runtime_id: "provider-runtime".into(),
            supported_platforms: vec!["linux".into()],
            content_id: "provider-game:content".into(),
        }
    };

    let applied = fixture
        .client()
        .games_register_content(registration(Some(context.clone()), "3", "id-fixture:1"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(applied.outcome, MutationOutcome::Applied as i32);
    assert_eq!(applied.resource_id, "provider-game:content");

    // The provider may only discover content for its own catalog, even when it supplies a valid
    // host-issued context and a file handle. A missing or altered context cannot use this route.
    assert_eq!(
        fixture
            .client()
            .games_register_content(registration(Some(context.clone()), "5", "id-steam:1"))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        fixture
            .client()
            .games_register_content(registration(None, "3", "id-fixture:1"))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    let mut altered = context.clone();
    altered.target_game = Some(TargetGameContext {
        game_id: "3".into(),
        installation_id: "id-fixture:1".into(),
        profile_id: "account-a".into(),
        revision: "1".into(),
    });
    assert_eq!(
        fixture
            .client()
            .games_register_content(registration(Some(altered), "3", "id-fixture:1"))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let foreign = Fixture::new(&[cap::DISCOVERY, cap::FILES_READ]).await;
    assert_eq!(
        fixture
            .client()
            .games_register_content(registration(Some(foreign.context()), "3", "id-fixture:1"))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn extension_custom_overlay_targets_selected_steam_without_read_or_provider_identity() {
    let target = TargetGameContext {
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        profile_id: "account-a".into(),
        revision: "1".into(),
    };
    let (fixture, host, _) =
        Fixture::extension_game(&[cap::GAME_CUSTOM_LAUNCH], target, None).await;
    host.launch_target.store(1, Ordering::SeqCst);
    let context = fixture.context();
    assert_eq!(context.session_kind(), SessionKind::Extension);
    assert!(context.provider_id.is_empty());
    assert_eq!(
        context.target_game.as_ref().unwrap().installation_id,
        "id-steam:1"
    );
    assert_eq!(
        fixture.list("").await.unwrap_err().code(),
        Code::PermissionDenied
    );

    let revision = fixture.feed.revision_of("steam:1").unwrap();
    assert_eq!(
        fixture
            .client()
            .games_request_launch(GameLaunchRequest {
                context: Some(context.clone()),
                game_id: "5".into(),
                installation_id: "id-steam:1".into(),
                expected_revision: revision,
                user_intent_id: String::new(),
                idempotency_key: "normal-without-grant".into(),
            })
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let proposal = CustomLaunchRequest {
        context: Some(context.clone()),
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        expected_revision: revision,
        idempotency_key: "extension-custom-proposal".into(),
        uri: "steam://rungameid/5".into(),
        ..CustomLaunchRequest::default()
    };
    let recipe = fixture
        .client()
        .games_propose_custom_launch(proposal.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(host.custom_launch_recipes.lock().unwrap().len(), 1);
    let stored = host
        .custom_launch_recipes
        .lock()
        .unwrap()
        .values()
        .next()
        .cloned()
        .unwrap();
    assert_eq!(
        (stored.game_id, stored.installation_id.as_str()),
        (5, "id-steam:1")
    );

    let mut forged_provider = proposal.clone();
    forged_provider.context.as_mut().unwrap().provider_id = "steam".into();
    forged_provider.idempotency_key = "forged-provider".into();
    assert!(
        fixture
            .client()
            .games_propose_custom_launch(forged_provider)
            .await
            .is_err()
    );
    let mut other_game = proposal;
    other_game.game_id = "6".into();
    other_game.installation_id = "id-steam:2".into();
    other_game.idempotency_key = "other-steam-game".into();
    assert_eq!(
        fixture
            .client()
            .games_propose_custom_launch(other_game)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );

    let intent = fixture
        .intents
        .issue(
            fixture
                .binding_for(
                    Operation::CustomLaunch,
                    1,
                    "5",
                    "id-steam:1",
                    &recipe.recipe_id,
                )
                .await,
        )
        .unwrap();
    let request = CustomLaunchRequest {
        context: Some(context),
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        expected_revision: 1,
        user_intent_id: intent,
        idempotency_key: "extension-custom-execution".into(),
        recipe_id: recipe.recipe_id.clone(),
        recipe_revision: recipe.recipe_revision,
        ..CustomLaunchRequest::default()
    };
    let executed = fixture
        .client()
        .games_request_custom_launch(request.clone())
        .await
        .unwrap()
        .into_inner();
    let replay = fixture
        .client()
        .games_request_custom_launch(request.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(replay.job_id, executed.job_id);
    assert_eq!(
        fixture.ended(&executed.job_id).await.state(),
        JobState::Succeeded
    );
    let mut reused_intent = request;
    reused_intent.idempotency_key = "extension-custom-reuse-intent".into();
    assert_eq!(
        fixture
            .client()
            .games_request_custom_launch(reused_intent)
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(host.custom_launch_dispatches.lock().unwrap().len(), 1);
    assert!(host.opened.lock().unwrap().is_empty());
    let steam_scope = Scope {
        own_provider: "steam".into(),
        read_all: false,
        provider_filter: None,
    };
    let (_, selected_steam_entry) = fixture.feed.target(&steam_scope, "id-steam:1").unwrap();
    assert_eq!(selected_steam_entry.name, "Selected Steam game");
    assert_eq!(selected_steam_entry.game_id, Some(5));
}

#[tokio::test(flavor = "multi_thread")]
async fn normal_and_custom_launch_permissions_are_independent_for_extensions() {
    let target = TargetGameContext {
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        profile_id: "account-a".into(),
        revision: "1".into(),
    };
    let (fixture, host, _) = Fixture::extension_game(&[cap::GAME_LAUNCH], target, None).await;
    host.launch_target.store(1, Ordering::SeqCst);
    let revision = fixture.feed.revision_of("steam:1").unwrap();
    let custom_proposal = fixture
        .client()
        .games_propose_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "5".into(),
            installation_id: "id-steam:1".into(),
            expected_revision: revision,
            idempotency_key: "without-custom-grant".into(),
            uri: "steam://rungameid/5".into(),
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap_err();
    assert_eq!(custom_proposal.code(), Code::PermissionDenied);

    let intent = fixture
        .intents
        .issue(
            fixture
                .binding_for(Operation::Launch, 1, "5", "id-steam:1", "")
                .await,
        )
        .unwrap();
    let launched = fixture
        .client()
        .games_request_launch(GameLaunchRequest {
            context: Some(fixture.context()),
            game_id: "5".into(),
            installation_id: "id-steam:1".into(),
            expected_revision: 1,
            user_intent_id: intent,
            idempotency_key: "normal-launch".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        fixture.ended(&launched.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(*host.opened.lock().unwrap(), [("steam:1".into(), false)]);
    assert!(host.custom_launch_recipes.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_launch_commit_serializes_with_permission_revocation() {
    let (fixture, host, _) = Fixture::game(
        &[cap::GAME_LAUNCH, cap::GAME_CUSTOM_LAUNCH],
        Some(answering(&[APPROVE])),
    )
    .await;
    host.launch_target.store(1, Ordering::SeqCst);
    let revision = fixture.revision().await;
    let recipe = fixture
        .client()
        .games_propose_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "race-recipe".into(),
            uri: "steam://rungameid/3".into(),
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    let intent = fixture
        .intents
        .issue(
            fixture
                .binding_for(
                    Operation::CustomLaunch,
                    1,
                    "3",
                    INSTALLATION,
                    &recipe.recipe_id,
                )
                .await,
        )
        .unwrap();
    let gate = Arc::new(Notify::new());
    *host.custom_launch_gate.lock().unwrap() = Some(gate.clone());
    let entered = host.custom_launch_entered.notified();
    let owner = fixture.game_owner();
    let reply = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            user_intent_id: intent,
            idempotency_key: "race-execute".into(),
            recipe_id: recipe.recipe_id,
            recipe_revision: recipe.recipe_revision,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .unwrap();
    let provider = fixture.api.clone();
    let mut revoke = tokio::spawn(async move {
        provider
            .revoke_session_permission("fixture", cap::GAME_CUSTOM_LAUNCH)
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut revoke)
            .await
            .is_err(),
        "revocation must wait for the launch commit"
    );
    gate.notify_one();
    tokio::time::timeout(Duration::from_secs(5), revoke)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture.ended_for(&owner, &reply.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(host.custom_launch_dispatches.lock().unwrap().len(), 1);
    assert!(
        fixture
            .api
            .authorize_resource(Some(&fixture.context()), cap::GAME_CUSTOM_LAUNCH)
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_launch_revoked_before_approval_never_dispatches() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) =
        Fixture::game(&[cap::GAME_LAUNCH, cap::GAME_CUSTOM_LAUNCH], Some(dialog)).await;
    host.launch_target.store(1, Ordering::SeqCst);
    let revision = fixture.revision().await;
    let recipe = fixture
        .client()
        .games_propose_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "revoke-recipe".into(),
            uri: "steam://rungameid/3".into(),
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    let owner = fixture.game_owner();
    let reply = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "revoke-execute".into(),
            recipe_id: recipe.recipe_id,
            recipe_revision: recipe.recipe_revision,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    Fixture::dialog_shown(&dialog.unwrap(), 1).await;
    fixture
        .api
        .revoke_session_permission("fixture", cap::GAME_CUSTOM_LAUNCH)
        .await;
    gate.notify_one();
    let done = fixture.ended_for(&owner, &reply.job_id).await;
    assert_ne!(done.state(), JobState::Succeeded);
    assert!(host.custom_launch_dispatches.lock().unwrap().is_empty());
}

/// A second plugin session that may read the shared runtime catalog and the common content
/// availability projection, but has no runtime mutation or discovery authority.
async fn runtime_catalog_consumer(fixture: &Fixture) -> Session {
    let capabilities = [
        cap::CONTEXT,
        cap::LIBRARY_READ,
        cap::LIBRARY_READ_ALL,
        cap::GAME_RUNTIME_READ,
        cap::FILES_READ,
    ];
    fixture
        .api
        .register(Registration {
            provider_id: "runtime-consumer".into(),
            plugin_id: "org.example.runtime-consumer".into(),
            kind: crate::plugin_host::provider::RegistrationKind::Provider,
            declared_developer: "Runtime consumer developer".into(),
            package_digest: vec![7; 32],
            origin: RegistrationOrigin::ThirdParty {
                publisher_fingerprint: "cd".repeat(32),
            },
            capabilities: capabilities.iter().map(|cap| (*cap).into()).collect(),
            shared_catalog: false,
            playtime: Default::default(),
            services: Arc::new(NoServices),
        })
        .unwrap();
    let launch = fixture.api.begin_launch("runtime-consumer").unwrap();
    let credential = (0..launch.credential.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&launch.credential[i..i + 2], 16).unwrap())
        .collect();
    let session = Session::open_with_options(
        Launch {
            endpoint: fixture.endpoint.clone(),
            credential,
            operation_id: "runtime-consumer-launch".into(),
        },
        "org.example.runtime-consumer",
        "runtime-consumer",
        "1.0",
        &capabilities,
        SessionOptions {
            min_api_major: 1,
            min_api_minor: cap::THIRD_PARTY_API_MINOR,
            max_api_major: 1,
            max_api_minor: gamenode_plugin_protocol::API_MINOR,
            ..SessionOptions::default()
        },
    )
    .await
    .unwrap();
    launch.ready.await.unwrap();
    session
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_runtime_catalog_and_content_availability_need_no_pairwise_grant() {
    use crate::plugin_host::resources::ResourceApi;
    use crate::plugin_host::shared_data::DataBroker;
    use crate::plugin_host::sharing_policy::{ActivationReadGrant, SharedSharingPolicies};
    use gamenode_plugin_protocol::{
        prost_types,
        proto::{
            CommonDataCategory, ContentPathArgument, ExtensionPayload,
            RuntimeArgumentTemplate as RuntimeArgumentProto,
            runtime_argument_template::Value as RuntimeArgumentValue,
        },
    };
    use tokio::task::JoinHandle;
    use tokio_stream::wrappers::TcpListenerStream;

    async fn serve(service: ResourceApi) -> (ResourceHostClient<Channel>, JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(resource_host_server::ResourceHostServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let client = ResourceHostClient::connect(format!("http://{address}"))
            .await
            .unwrap();
        (client, server)
    }

    let target = TargetGameContext {
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        profile_id: "account-a".into(),
        revision: "1".into(),
    };
    let (fixture, host, _) = Fixture::extension_game(
        &[
            cap::DISCOVERY,
            cap::FILES_READ,
            cap::GAME_CUSTOM_LAUNCH,
            cap::GAME_RUNTIME_CREATE,
            cap::GAME_RUNTIME_READ,
        ],
        target,
        Some(answering(&[APPROVE])),
    )
    .await;
    let content_root = fixture.game_dir("steam-runtime-catalog", &[("emulator", b"runtime")]);
    fixture.user_approving(&content_root);
    let root = fixture.root_handle("id-steam:1", &["read"]).await;
    let runtime_executable = fixture.child(&root, "emulator").await;

    // A registers an owned runtime with a plugin-declared compatibility namespace.
    let registration = fixture
        .client()
        .runtimes_register(RuntimeRegisterRequest {
            mutation: Some(fixture.mutation("register-catalog-runtime", 0)),
            runtime: Some(RuntimeRecord {
                id: "runtime:catalog-snes".into(),
                name: "Catalog SNES".into(),
                version: "2.0".into(),
                executable_handle: runtime_executable,
                supported_formats: vec!["snes-rom".into(), "org.example.snes-patch".into()],
                platform: std::env::consts::OS.into(),
                argument_template: vec![RuntimeArgumentProto {
                    value: Some(RuntimeArgumentValue::ContentPath(ContentPathArgument {})),
                }],
                extensions: [(
                    "org.example.compatibility".into(),
                    ExtensionPayload {
                        schema_id: "org.example.compatibility.v1".into(),
                        schema_version: 1,
                        schema_digest: vec![3; 32],
                        data: Some(prost_types::Struct {
                            fields: [(
                                "console".into(),
                                prost_types::Value {
                                    kind: Some(prost_types::value::Kind::StringValue(
                                        "snes".into(),
                                    )),
                                },
                            )]
                            .into(),
                        }),
                    },
                )]
                .into(),
                ..Default::default()
            }),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(registration.outcome, MutationOutcome::Applied as i32);
    assert_eq!(
        registration
            .runtime
            .as_ref()
            .unwrap()
            .provenance
            .as_ref()
            .unwrap()
            .producer_plugin_id,
        "fixture.plugin"
    );
    let context_a = fixture.context();
    let caller_a = fixture
        .api
        .authorize_resource(Some(&context_a), cap::GAME_RUNTIME_CREATE)
        .unwrap();
    let owner_a = super::super::mutations::mutation_owner(&caller_a);
    host.runtime_bindings.lock().unwrap().insert(
        (owner_a, 5, "id-steam:1".into()),
        vec![gamenode_core::library::RuntimeContentBinding {
            game_id: 5,
            installation_id: "id-steam:1".into(),
            content_id: "rom:zelda".into(),
            content: gamenode_core::library::ApprovedFileReference {
                approval_id: "content-approval".into(),
                relative_path: vec!["zelda.sfc".into()],
            },
            content_kind: "snes-rom".into(),
            runtime_id: "runtime:catalog-snes".into(),
            supported_platforms: vec![std::env::consts::OS.into()],
            revision: 1,
        }],
    );

    // B gets only the two common category reads, with no source-to-recipient policy.
    let consumer = runtime_catalog_consumer(&fixture).await;
    let context_b = consumer.host.resource_context().unwrap().clone();
    let identity_b = {
        let caller = fixture
            .api
            .authorize_resource_session(Some(&context_b))
            .unwrap();
        ResourceApi::data_identity(&caller).unwrap()
    };
    let policies = Arc::new(SharedSharingPolicies::default());
    policies
        .set_activation_grants(
            &identity_b,
            ActivationReadGrant::new(
                [
                    CommonDataCategory::RuntimeCatalog,
                    CommonDataCategory::ContentAvailability,
                ],
                [],
            )
            .unwrap(),
        )
        .unwrap();
    let service = ResourceApi::new(
        fixture.api.clone(),
        fixture.feed.clone(),
        fixture.storage.clone(),
        fixture.files.clone(),
        fixture.network.clone(),
        fixture.notifications.clone(),
        fixture.jobs.clone(),
        fixture.intents.clone(),
        fixture.saves.clone(),
        fixture.snapshots.clone(),
    )
    .with_shared_data(Arc::new(DataBroker::new()), policies.clone());
    let (mut client, server) = serve(service).await;

    // Private-only default: B sees no foreign runtime and no availability.
    let private = client
        .runtimes_list(DomainListRequest {
            page: Some(PageRequest {
                context: Some(context_b.clone()),
                page_size: 10,
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert!(private.runtimes.is_empty());
    let private_entry = client
        .library_get(LibraryGetRequest {
            context: Some(context_b.clone()),
            installation_id: "id-steam:1".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner()
        .entry
        .unwrap();
    assert!(private_entry.runtime_availability.is_empty());

    // Without the current category grant, the explicit selector is denied rather than narrowed.
    let selector = CommonDataSelector {
        categories: vec![CommonDataCategory::RuntimeCatalog as i32],
        selected_record_ids: Vec::new(),
    };
    policies.clear_activation_grants(&identity_b).unwrap();
    let denied = client
        .runtimes_list(DomainListRequest {
            page: Some(PageRequest {
                context: Some(context_b.clone()),
                page_size: 10,
                common_data_categories: Some(selector.clone()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    policies
        .set_activation_grants(
            &identity_b,
            ActivationReadGrant::new(
                [
                    CommonDataCategory::RuntimeCatalog,
                    CommonDataCategory::ContentAvailability,
                ],
                [],
            )
            .unwrap(),
        )
        .unwrap();

    // The explicit common selector returns A's runtime with provenance and its metadata, but no
    // executable handle and no filesystem path.
    let shared = client
        .runtimes_list(DomainListRequest {
            page: Some(PageRequest {
                context: Some(context_b.clone()),
                page_size: 10,
                common_data_categories: Some(selector.clone()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(shared.runtimes.len(), 1);
    let runtime = &shared.runtimes[0];
    assert_eq!(runtime.id, "runtime:catalog-snes");
    assert_eq!(runtime.name, "Catalog SNES");
    assert_eq!(runtime.version, "2.0");
    assert_eq!(
        runtime.supported_formats,
        ["snes-rom", "org.example.snes-patch"]
    );
    assert!(runtime.executable_handle.is_empty());
    assert_eq!(runtime.argument_template.len(), 1);
    let provenance = runtime.provenance.as_ref().unwrap();
    assert_eq!(provenance.producer_plugin_id, "fixture.plugin");
    assert_eq!(provenance.publisher_fingerprint, "ab".repeat(32));
    let compatibility = runtime.extensions.get("org.example.compatibility").unwrap();
    assert_eq!(compatibility.schema_id, "org.example.compatibility.v1");
    assert_eq!(compatibility.schema_version, 1);
    let field = &compatibility.data.as_ref().unwrap().fields["console"];
    assert_eq!(
        field.kind,
        Some(prost_types::value::Kind::StringValue("snes".into()))
    );

    // The same runtime is reachable by id, and a watch replays it after a reset.
    let by_id = client
        .runtimes_get(DomainGetRequest {
            context: Some(context_b.clone()),
            resource_id: "runtime:catalog-snes".into(),
            shared_records: Some(selector.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        by_id
            .runtime
            .unwrap()
            .provenance
            .unwrap()
            .producer_plugin_id,
        "fixture.plugin"
    );
    let mut watch = client
        .runtimes_watch(DomainWatchRequest {
            context: Some(context_b.clone()),
            after_revision: 0,
            shared_records: Some(selector.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    let reset = tokio::time::timeout(std::time::Duration::from_secs(2), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(reset.event_type, "reset");
    let upserted = tokio::time::timeout(std::time::Duration::from_secs(2), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(upserted.event_type, "upserted");
    assert_eq!(upserted.runtime.unwrap().id, "runtime:catalog-snes");

    // Common content availability reaches B's Library projection without a path or handle.
    let availability_selector = CommonDataSelector {
        categories: vec![CommonDataCategory::ContentAvailability as i32],
        selected_record_ids: Vec::new(),
    };
    let entry = client
        .library_get(LibraryGetRequest {
            context: Some(context_b.clone()),
            installation_id: "id-steam:1".into(),
            common_data_categories: Some(availability_selector),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner()
        .entry
        .unwrap();
    assert_eq!(entry.runtime_availability.len(), 1);
    let available = &entry.runtime_availability[0];
    assert_eq!(available.content_id, "rom:zelda");
    assert_eq!(available.content_kind, "snes-rom");
    assert_eq!(available.runtime_id, "runtime:catalog-snes");
    assert_eq!(available.runtime_name, "Catalog SNES");
    assert_eq!(available.publisher_id, "fixture.plugin");
    assert_eq!(available.platform, std::env::consts::OS);

    // A Library watch delivers the same safe projection when an entry changes.
    let listed = client
        .library_list(LibraryListRequest {
            provider_id: "steam".into(),
            page: Some(PageRequest {
                context: Some(context_b.clone()),
                page_size: 10,
                common_data_categories: Some(CommonDataSelector {
                    categories: vec![CommonDataCategory::ContentAvailability as i32],
                    selected_record_ids: Vec::new(),
                }),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    let mut availability_watch = client
        .library_watch(LibraryWatchRequest {
            context: Some(context_b.clone()),
            after_revision: listed.revision,
            provider_id: "steam".into(),
            common_data_categories: Some(CommonDataSelector {
                categories: vec![CommonDataCategory::ContentAvailability as i32],
                selected_record_ids: Vec::new(),
            }),
        })
        .await
        .unwrap()
        .into_inner();
    fixture.library(&[
        ("steam:1", Some(5), "Selected Steam game renamed"),
        ("steam:2", Some(6), "Other Steam game"),
        ("manual:1", Some(9), "Manual game"),
    ]);
    let changed = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        availability_watch.message(),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(changed.event_type, "upserted");
    let watched = changed.entry.unwrap();
    assert_eq!(watched.name, "Selected Steam game renamed");
    assert_eq!(watched.runtime_availability.len(), 1);
    assert_eq!(
        watched.runtime_availability[0].runtime_id,
        "runtime:catalog-snes"
    );

    // A category read is not a mutation authority.
    let update = client
        .runtimes_update(RuntimeUpdateRequest {
            mutation: Some(fixture.mutation("consumer-updates-runtime", 1)),
            runtime: Some(RuntimeRecord {
                id: "runtime:catalog-snes".into(),
                name: "Hijacked".into(),
                ..Default::default()
            }),
            field_mask: vec!["name".into()],
            extension_patch: None,
        })
        .await
        .unwrap_err();
    assert_eq!(update.code(), Code::PermissionDenied);
    let remove = client
        .runtimes_remove(DomainRemoveRequest {
            mutation: Some(fixture.mutation("consumer-removes-runtime", 1)),
            resource_id: "runtime:catalog-snes".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(remove.code(), Code::PermissionDenied);

    let snapshot = policies.recipient_snapshot(&identity_b).unwrap();
    assert!(snapshot.source_recipient_policies.is_empty());
    server.abort();
}
