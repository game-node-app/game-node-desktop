//! Plugin-owned custom launch recipe lifecycle.
//!
//! Permission is checked before inspecting the requested game or recipe. In particular, a custom
//! launch request never falls through to the standard `game.launch` route.

use gamenode_core::library::{CustomLaunchMutation, CustomLaunchRecipe};
use gamenode_plugin_protocol::{capability as cap, proto::*};
use tonic::{Request, Response, Status};

use super::mutations::{game_id, mutation_owner, request_hash};
use super::{ResourceApi, Scope};

const MAX_KEY_BYTES: usize = 128;
const MAX_RECIPE_ID_BYTES: usize = 128;

pub(super) fn resolve_handle_path(
    files: std::sync::Arc<crate::plugin_host::files::FileAccess>,
    feed: std::sync::Arc<crate::plugin_host::library::LibraryFeed>,
    session: String,
    handle: String,
    installation_id: String,
    directory: bool,
) -> impl FnOnce(&mut crate::plugin_host::storage::Namespace<'_>) -> Result<String, Status>
+ Send
+ 'static {
    move |namespace| {
        let opened = super::files::open_handle(
            &files,
            &feed,
            namespace,
            &session,
            &handle,
            gamenode_platform::plugin_storage::operation::READ,
        )?;
        if opened.approval.installation_id != installation_id
            || !matches!(opened.approval.kind.as_str(), "installation" | "prefix")
        {
            return Err(Status::permission_denied(
                "the handle is not approved for this game installation",
            ));
        }
        let root = std::path::PathBuf::from(&opened.approval.root);
        let mut requested = root.clone();
        for part in opened.path {
            requested.push(part);
        }
        let canonical = std::fs::canonicalize(&requested)
            .map_err(|_| Status::failed_precondition("approved launch handle changed"))?;
        if !canonical.starts_with(&root)
            || (directory && !canonical.is_dir())
            || (!directory && !canonical.is_file())
        {
            return Err(Status::permission_denied(
                "the handle is outside its approved game folder",
            ));
        }
        canonical
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| Status::failed_precondition("launch path is not valid Unicode"))
    }
}

fn recipe_id() -> Result<String, Status> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Status::internal("no source of randomness"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn outcome(mutation: CustomLaunchMutation, id: &str) -> MutationReply {
    match mutation {
        CustomLaunchMutation::Applied { revision } => MutationReply {
            resource_id: id.to_owned(),
            revision,
            outcome: MutationOutcome::Applied as i32,
        },
        CustomLaunchMutation::Conflict { current } => MutationReply {
            resource_id: id.to_owned(),
            revision: current,
            outcome: MutationOutcome::RevisionConflict as i32,
        },
        CustomLaunchMutation::Missing => MutationReply {
            resource_id: id.to_owned(),
            revision: 0,
            outcome: MutationOutcome::RevisionConflict as i32,
        },
    }
}

impl ResourceApi {
    pub(super) async fn revalidate_custom_handles(
        &self,
        caller: &crate::plugin_host::provider::ResourceCaller,
        context: &ServiceContext,
        recipe: &CustomLaunchRecipe,
        dispatch: &crate::plugin_host::library::CustomLaunchDispatch,
    ) -> Result<(), Status> {
        if !recipe.runtime_handle.is_empty() {
            let crate::plugin_host::library::CustomLaunchDispatch::Runtime {
                command,
                runtime_id,
                runtime_revision,
                content_id,
                binding_revision,
            } = dispatch
            else {
                return Err(Status::failed_precondition(
                    "the approved runtime dispatch changed before launch",
                ));
            };
            if recipe.runtime_handle != *runtime_id || recipe.content_id != *content_id {
                return Err(Status::failed_precondition(
                    "the approved runtime or content selection changed before launch",
                ));
            }
            let resolved = self
                .resolve_runtime_launch(
                    caller,
                    context,
                    recipe.game_id,
                    &recipe.installation_id,
                    &recipe.runtime_handle,
                    &recipe.content_id,
                )
                .await?;
            if resolved.runtime_revision != *runtime_revision
                || resolved.binding_revision != *binding_revision
            {
                return Err(Status::failed_precondition(
                    "the approved runtime or content binding changed before launch",
                ));
            }
            let working_directory = if recipe.working_directory_handle.is_empty() {
                None
            } else {
                self.providers
                    .authorize_resource(Some(context), cap::FILES_READ)?;
                Some(
                    self.in_namespace(
                        caller,
                        resolve_handle_path(
                            self.files.clone(),
                            self.library.clone(),
                            caller.session_id.clone(),
                            recipe.working_directory_handle.clone(),
                            recipe.installation_id.clone(),
                            true,
                        ),
                    )
                    .await?,
                )
            };
            let host = self
                .library
                .host()
                .ok_or_else(|| Status::unavailable("the library is not available"))?;
            let current = host
                .prepare_custom_launch(recipe.clone(), None, working_directory, Some(resolved))
                .await?;
            match current {
                crate::plugin_host::library::CustomLaunchDispatch::Runtime {
                    command: current,
                    runtime_id: current_runtime,
                    runtime_revision: current_revision,
                    content_id: current_content,
                    binding_revision: current_binding,
                } if &current == command
                    && current_runtime == *runtime_id
                    && current_revision == *runtime_revision
                    && current_content == *content_id
                    && current_binding == *binding_revision =>
                {
                    return Ok(());
                }
                _ => {
                    return Err(Status::failed_precondition(
                        "the approved runtime launch plan changed before launch",
                    ));
                }
            }
        }
        let is_uri = !recipe.uri.is_empty();
        if recipe.executable_handle.is_empty() && recipe.working_directory_handle.is_empty() {
            return Ok(());
        }
        self.providers
            .authorize_resource(Some(context), cap::FILES_READ)?;
        if !is_uri {
            let command = match dispatch {
                crate::plugin_host::library::CustomLaunchDispatch::Executable(command) => command,
                crate::plugin_host::library::CustomLaunchDispatch::Runtime { command, .. } => {
                    command
                }
                crate::plugin_host::library::CustomLaunchDispatch::Uri { .. } => {
                    return Err(Status::failed_precondition(
                        "custom executable dispatch no longer matches its recipe",
                    ));
                }
            };
            let executable = self
                .in_namespace(
                    caller,
                    resolve_handle_path(
                        self.files.clone(),
                        self.library.clone(),
                        caller.session_id.clone(),
                        recipe.executable_handle.clone(),
                        recipe.installation_id.clone(),
                        false,
                    ),
                )
                .await?;
            if executable != command.executable {
                return Err(Status::failed_precondition(
                    "approved executable handle changed during confirmation",
                ));
            }
        }
        if !recipe.working_directory_handle.is_empty() {
            let working_directory = self
                .in_namespace(
                    caller,
                    resolve_handle_path(
                        self.files.clone(),
                        self.library.clone(),
                        caller.session_id.clone(),
                        recipe.working_directory_handle.clone(),
                        recipe.installation_id.clone(),
                        true,
                    ),
                )
                .await?;
            let approved_working_directory = match dispatch {
                crate::plugin_host::library::CustomLaunchDispatch::Executable(command)
                | crate::plugin_host::library::CustomLaunchDispatch::Runtime { command, .. } => {
                    &command.working_directory
                }
                crate::plugin_host::library::CustomLaunchDispatch::Uri {
                    working_directory,
                    ..
                } => working_directory,
            };
            if &working_directory != approved_working_directory {
                return Err(Status::failed_precondition(
                    "approved working directory changed during confirmation",
                ));
            }
        }
        Ok(())
    }

    pub(super) async fn propose_custom_launch(
        &self,
        request: Request<CustomLaunchRequest>,
    ) -> Result<Response<CustomLaunchReply>, Status> {
        let request = request.into_inner();
        // Keep this first: an ungranted custom request is always an explicit permission denial.
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::GAME_CUSTOM_LAUNCH)?;
        if request.idempotency_key.is_empty() || request.idempotency_key.len() > MAX_KEY_BYTES {
            return Err(Status::invalid_argument("invalid idempotency key"));
        }
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| Status::unauthenticated("resource context required"))?;
        let (_entry_key, entry) =
            self.action_target(&caller, context, &request.game_id, &request.installation_id)?;
        if entry.revision != request.expected_revision {
            return Err(Status::failed_precondition("library entry changed"));
        }
        let game_id = game_id(&request.game_id)?
            .ok_or_else(|| Status::invalid_argument("custom launch requires a matched game"))?;
        let host = self
            .library
            .host()
            .ok_or_else(|| Status::unavailable("custom launch recipes are unavailable"))?;
        let target_revision = host.launch_fingerprint(_entry_key.clone()).await?;
        let recipe = CustomLaunchRecipe {
            recipe_id: recipe_id()?,
            revision: 1,
            game_id,
            installation_id: entry.installation_id,
            base_target_revision: target_revision,
            executable_handle: request.executable_handle,
            content_id: request.content_id,
            uri: request.uri,
            arguments: request.argv,
            working_directory_handle: request.working_directory_handle,
            runtime_handle: request.runtime_handle,
            environment_set: request.environment_set.into_iter().collect(),
            environment_unset: request.environment_unset.into_iter().collect(),
        };
        if !recipe.validate()
            || (!recipe.runtime_handle.is_empty() && !recipe.executable_handle.is_empty())
        {
            return Err(Status::invalid_argument("invalid custom launch recipe"));
        }
        let saved = host
            .save_custom_launch_recipe(mutation_owner(&caller), recipe.clone())
            .await?;
        match saved {
            CustomLaunchMutation::Applied { revision } => Ok(Response::new(CustomLaunchReply {
                recipe_id: recipe.recipe_id,
                recipe_revision: revision,
                job_id: String::new(),
                state: JobState::Unspecified as i32,
                refusal_code: String::new(),
                message: "recipe saved; execution requires a separately approved request".into(),
            })),
            CustomLaunchMutation::Conflict { .. } => {
                Err(Status::aborted("recipe revision conflict"))
            }
            CustomLaunchMutation::Missing => {
                Err(Status::not_found("custom launch recipe not found"))
            }
        }
    }

    pub(super) async fn request_custom_launch(
        &self,
        request: Request<CustomLaunchRequest>,
    ) -> Result<Response<CustomLaunchReply>, Status> {
        let request = request.into_inner();
        // A custom recipe cannot be downgraded to standard launch when this grant is absent.
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::GAME_CUSTOM_LAUNCH)?;
        if !request.executable_handle.is_empty()
            || !request.content_id.is_empty()
            || !request.uri.is_empty()
            || !request.argv.is_empty()
            || !request.working_directory_handle.is_empty()
            || !request.runtime_handle.is_empty()
            || !request.environment_set.is_empty()
            || !request.environment_unset.is_empty()
        {
            return Err(Status::invalid_argument(
                "custom execution must reference an approved recipe revision",
            ));
        }
        if request.recipe_id.is_empty() || request.recipe_id.len() > MAX_RECIPE_ID_BYTES {
            return Err(Status::invalid_argument("invalid recipe id"));
        }
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| Status::unauthenticated("resource context required"))?;
        let (entry_key, entry) =
            self.action_target(&caller, context, &request.game_id, &request.installation_id)?;
        if entry.revision != request.expected_revision {
            return Err(Status::failed_precondition("library entry changed"));
        }
        let host = self
            .library
            .host()
            .ok_or_else(|| Status::unavailable("custom launch recipes are unavailable"))?;
        let recipe = host
            .custom_launch_recipe(mutation_owner(&caller), request.recipe_id.clone())
            .await?
            .ok_or_else(|| Status::not_found("custom launch recipe not found"))?;
        let target_revision = host.launch_fingerprint(entry_key).await?;
        if recipe.revision != request.recipe_revision
            || recipe.game_id.to_string() != request.game_id
            || recipe.installation_id != request.installation_id
            || recipe.base_target_revision != target_revision
        {
            return Err(Status::aborted("custom launch recipe revision changed"));
        }
        let runtime_launch = if recipe.runtime_handle.is_empty() {
            None
        } else {
            Some(
                self.resolve_runtime_launch(
                    &caller,
                    context,
                    recipe.game_id,
                    &recipe.installation_id,
                    &recipe.runtime_handle,
                    &recipe.content_id,
                )
                .await?,
            )
        };
        let (executable_path, working_directory_path) =
            if recipe.uri.is_empty() && recipe.runtime_handle.is_empty() {
                // A custom recipe grant authorizes asking for the launch; file handles remain subject
                // to their separate read grant and approval.
                self.providers
                    .authorize_resource(request.context.as_ref(), cap::FILES_READ)?;
                let executable_handle = recipe.executable_handle.clone();
                let executable = self
                    .in_namespace(
                        &caller,
                        resolve_handle_path(
                            self.files.clone(),
                            self.library.clone(),
                            caller.session_id.clone(),
                            executable_handle,
                            recipe.installation_id.clone(),
                            false,
                        ),
                    )
                    .await?;
                let working = if recipe.working_directory_handle.is_empty() {
                    None
                } else {
                    Some(
                        self.in_namespace(
                            &caller,
                            resolve_handle_path(
                                self.files.clone(),
                                self.library.clone(),
                                caller.session_id.clone(),
                                recipe.working_directory_handle.clone(),
                                recipe.installation_id.clone(),
                                true,
                            ),
                        )
                        .await?,
                    )
                };
                (Some(executable), working)
            } else {
                let working = if recipe.working_directory_handle.is_empty() {
                    None
                } else {
                    self.providers
                        .authorize_resource(request.context.as_ref(), cap::FILES_READ)?;
                    Some(
                        self.in_namespace(
                            &caller,
                            resolve_handle_path(
                                self.files.clone(),
                                self.library.clone(),
                                caller.session_id.clone(),
                                recipe.working_directory_handle.clone(),
                                recipe.installation_id.clone(),
                                true,
                            ),
                        )
                        .await?,
                    )
                };
                (None, working)
            };
        let dispatch = host
            .prepare_custom_launch(
                recipe.clone(),
                executable_path,
                working_directory_path,
                runtime_launch,
            )
            .await?;
        self.request_custom_action(request, recipe.clone(), dispatch)
            .await
            .map(|job| {
                Response::new(CustomLaunchReply {
                    recipe_id: recipe.recipe_id,
                    recipe_revision: recipe.revision,
                    job_id: job.job_id,
                    state: job.state,
                    refusal_code: String::new(),
                    message: job.message,
                })
            })
    }

    pub(super) async fn remove_custom_launch(
        &self,
        request: Request<CustomLaunchRemoveRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .ok_or_else(|| Status::invalid_argument("mutation context is required"))?;
        let caller = self
            .providers
            .authorize_resource(mutation.context.as_ref(), cap::GAME_CUSTOM_LAUNCH)?;
        if request.recipe_id.is_empty() || request.recipe_id.len() > MAX_RECIPE_ID_BYTES {
            return Err(Status::invalid_argument("invalid recipe id"));
        }
        let key = mutation.idempotency_key.as_str();
        if key.is_empty() || key.len() > MAX_KEY_BYTES {
            return Err(Status::invalid_argument("invalid idempotency key"));
        }
        let lock = self.mutations.lock.lock().await;
        let hash = request_hash(&[
            "custom-launch-remove",
            &request.game_id,
            &request.recipe_id,
            &mutation.expected_revision.to_string(),
        ]);
        if let Some(reply) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(reply));
        }
        let id = game_id(&request.game_id)?
            .ok_or_else(|| Status::invalid_argument("game_id is required"))?;
        let scope = Scope {
            own_provider: caller.provider_id.clone(),
            read_all: false,
            provider_filter: None,
        };
        if self.library.by_game(&scope, id).1.is_empty() {
            return Err(Status::not_found("library entry not found"));
        }
        let host = self
            .library
            .host()
            .ok_or_else(|| Status::unavailable("custom launch recipes are unavailable"))?;
        let owner = mutation_owner(&caller);
        let recipe = host
            .custom_launch_recipe(owner.clone(), request.recipe_id.clone())
            .await?
            .ok_or_else(|| Status::not_found("custom launch recipe not found"))?;
        if recipe.game_id != id {
            return Err(Status::not_found("custom launch recipe not found"));
        }
        let result = host
            .remove_custom_launch_recipe(
                owner,
                request.recipe_id.clone(),
                mutation.expected_revision,
            )
            .await?;
        let reply = outcome(result, &request.recipe_id);
        self.mutations.remember(&caller, key, hash, &reply);
        drop(lock);
        Ok(Response::new(reply))
    }
}
