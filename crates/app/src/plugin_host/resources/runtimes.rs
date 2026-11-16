//! Owner-scoped installed runtime resources and game-content bindings.
//!
//! Plugin file handles are session scoped. Durable records keep only the host-owned approval id
//! and relative path; reads issue a fresh handle after checking the approval and current grant.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use gamenode_core::library::{
    ApprovedFileReference, RuntimeArgumentTemplate as CoreArgument, RuntimeContentBinding,
    RuntimeMutation, RuntimeResource,
};
use gamenode_plugin_protocol::{capability as cap, proto::*};
use tonic::{Request, Response, Status};

use super::mutations::{mutation_key, mutation_owner, request_hash, unavailable};
use super::{CursorBinding, CursorError, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, ResourceApi, Scope};
use crate::plugin_host::provider::ResourceCaller;

const MAX_RUNTIME_ID_BYTES: usize = 128;
const MAX_BINDINGS_PER_ENTRY: usize = 512;
const WATCH_RECHECK: std::time::Duration = std::time::Duration::from_secs(5);
const WATCH_POLL: std::time::Duration = std::time::Duration::from_millis(500);

fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    }
}

fn runtime_id() -> Result<String, Status> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Status::internal("no source of randomness"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_default()
}

fn mutation_outcome(value: RuntimeMutation) -> (MutationOutcome, u64, bool) {
    match value {
        RuntimeMutation::Applied { revision } => (MutationOutcome::Applied, revision, true),
        RuntimeMutation::Conflict { current } => {
            (MutationOutcome::RevisionConflict, current, false)
        }
        RuntimeMutation::Missing => (MutationOutcome::RevisionConflict, 0, false),
    }
}

fn runtime_request_shape(record: &RuntimeRecord, require_executable: bool) -> Result<(), Status> {
    if record.id.len() > MAX_RUNTIME_ID_BYTES
        || record.name.len() > 4096
        || record.version.len() > 4096
        || record.platform.len() > MAX_RUNTIME_ID_BYTES
        || (require_executable && record.executable_handle.is_empty())
        || record.executable_handle.len() > 16 * 1024
        || record.supported_formats.is_empty()
        || record.supported_formats.len() > 128
        || record.argument_template.len() > 128
        || !record.extensions.is_empty()
    {
        return Err(Status::invalid_argument("invalid runtime resource"));
    }
    Ok(())
}

fn core_arguments(values: &[RuntimeArgumentTemplate]) -> Result<Vec<CoreArgument>, Status> {
    values
        .iter()
        .map(|argument| match argument.value.as_ref() {
            Some(runtime_argument_template::Value::Literal(value)) => {
                Ok(CoreArgument::Literal(value.clone()))
            }
            Some(runtime_argument_template::Value::ContentPath(_)) => Ok(CoreArgument::ContentPath),
            None => Err(Status::invalid_argument(
                "runtime argument template must select a value",
            )),
        })
        .collect()
}

fn proto_arguments(values: &[CoreArgument]) -> Vec<RuntimeArgumentTemplate> {
    values
        .iter()
        .map(|argument| RuntimeArgumentTemplate {
            value: Some(match argument {
                CoreArgument::Literal(value) => {
                    runtime_argument_template::Value::Literal(value.clone())
                }
                CoreArgument::ContentPath => {
                    runtime_argument_template::Value::ContentPath(ContentPathArgument {})
                }
            }),
        })
        .collect()
}

fn validate_common_selector(selector: Option<&CommonDataSelector>) -> Result<(), Status> {
    if selector.is_some_and(|selector| {
        !selector.categories.is_empty() || !selector.selected_record_ids.is_empty()
    }) {
        return Err(Status::unimplemented(
            "common runtime catalog projections are not available in this build",
        ));
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RUNTIME_ID_BYTES
        && !value
            .chars()
            .any(|character| character.is_control() || character == '\0')
}

impl ResourceApi {
    fn authorize_commit(
        &self,
        context: &ServiceContext,
        permission: &'static str,
        initial_grant_revision: u64,
    ) -> Result<(), Status> {
        let caller = self
            .providers
            .authorize_resource(Some(context), permission)?;
        if caller.grant_revision != initial_grant_revision {
            return Err(Status::failed_precondition(
                "plugin permissions changed; try again",
            ));
        }
        Ok(())
    }

    /// Resolves a session token to a durable reference without retaining the token or a path.
    async fn capture_reference(
        &self,
        caller: &ResourceCaller,
        handle: String,
        target: Option<(u64, String)>,
    ) -> Result<ApprovedFileReference, Status> {
        let files = self.files.clone();
        let feed = self.library.clone();
        let session = caller.session_id.clone();
        self.in_namespace(caller, move |namespace| {
            let opened = super::files::open_handle(
                &files,
                &feed,
                namespace,
                &session,
                &handle,
                gamenode_platform::plugin_storage::operation::READ,
            )?;
            if !matches!(
                opened.approval.kind.as_str(),
                "installation" | "prefix" | "library"
            ) {
                return Err(Status::permission_denied(
                    "the handle is not an approved runtime or content file",
                ));
            }
            if let Some((game_id, installation_id)) = target {
                if opened.approval.kind != "library"
                    && (opened.approval.game_id.parse::<u64>().ok() != Some(game_id)
                        || opened.approval.installation_id != installation_id)
                {
                    return Err(Status::permission_denied(
                        "the content handle is not approved for this game installation",
                    ));
                }
            }
            let entry = opened
                .confined
                .stat(&opened.path)
                .map_err(super::files::file_status)?;
            if entry.directory || opened.path.is_empty() {
                return Err(Status::failed_precondition(
                    "runtime and content handles must identify files",
                ));
            }
            let reference = ApprovedFileReference {
                approval_id: opened.approval.id,
                relative_path: opened.path,
            };
            if reference.validate() {
                Ok(reference)
            } else {
                Err(Status::invalid_argument("invalid approved file reference"))
            }
        })
        .await
    }

    /// Revalidates a durable ref and creates a session-bound handle for a response.
    async fn session_handle(
        &self,
        caller: &ResourceCaller,
        reference: ApprovedFileReference,
    ) -> Result<String, Status> {
        if !reference.validate() {
            return Err(Status::failed_precondition(
                "stored runtime file reference is invalid",
            ));
        }
        let files = self.files.clone();
        let feed = self.library.clone();
        let session = caller.session_id.clone();
        let reference_for_open = reference.clone();
        self.in_namespace(caller, move |namespace| {
            let opened = super::files::open_approval(
                &files,
                &feed,
                namespace,
                &reference_for_open.approval_id,
                reference_for_open.relative_path.clone(),
                gamenode_platform::plugin_storage::operation::READ,
            )?;
            let entry = opened
                .confined
                .stat(&opened.path)
                .map_err(super::files::file_status)?;
            if entry.directory || opened.path.is_empty() {
                return Err(Status::failed_precondition(
                    "approved runtime file is no longer available",
                ));
            }
            Ok(files.seal_handle(&session, &reference.approval_id, &reference.relative_path))
        })
        .await
    }

    /// Resolves a durable host reference to a path only after checking that the approval still
    /// belongs to this plugin/profile and that the target remains inside its approved root.
    async fn resolve_reference_path(
        &self,
        caller: &ResourceCaller,
        reference: ApprovedFileReference,
        target: Option<(u64, String)>,
    ) -> Result<String, Status> {
        if !reference.validate() {
            return Err(Status::failed_precondition(
                "approved file reference is invalid",
            ));
        }
        let files = self.files.clone();
        let feed = self.library.clone();
        let reference_for_open = reference.clone();
        self.in_namespace(caller, move |namespace| {
            let opened = super::files::open_approval(
                &files,
                &feed,
                namespace,
                &reference_for_open.approval_id,
                reference_for_open.relative_path.clone(),
                gamenode_platform::plugin_storage::operation::READ,
            )?;
            if let Some((game_id, installation_id)) = target
                && opened.approval.kind != "library"
                && (opened.approval.game_id.parse::<u64>().ok() != Some(game_id)
                    || opened.approval.installation_id != installation_id)
            {
                return Err(Status::permission_denied(
                    "the content approval no longer matches this game installation",
                ));
            }
            let entry = opened
                .confined
                .stat(&opened.path)
                .map_err(super::files::file_status)?;
            if entry.directory || opened.path.is_empty() {
                return Err(Status::failed_precondition(
                    "approved runtime or content file is no longer available",
                ));
            }
            let root = std::fs::canonicalize(&opened.approval.root)
                .map_err(|_| Status::failed_precondition("approved runtime folder changed"))?;
            let mut target = root.clone();
            for component in &opened.path {
                target.push(component);
            }
            let canonical = std::fs::canonicalize(&target)
                .map_err(|_| Status::failed_precondition("approved file moved or changed"))?;
            if !canonical.starts_with(&root) || !canonical.is_file() {
                return Err(Status::permission_denied(
                    "approved file is outside its approved folder",
                ));
            }
            canonical
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| Status::failed_precondition("approved path is not valid Unicode"))
        })
        .await
    }

    async fn runtime_proto(
        &self,
        caller: &ResourceCaller,
        context: &ServiceContext,
        runtime: RuntimeResource,
    ) -> Result<RuntimeRecord, Status> {
        self.providers
            .authorize_resource(Some(context), cap::FILES_READ)?;
        let executable_handle = self
            .session_handle(caller, runtime.executable.clone())
            .await?;
        Ok(RuntimeRecord {
            id: runtime.id,
            name: runtime.name,
            version: runtime.version,
            executable_handle,
            supported_formats: runtime.supported_formats,
            revision: runtime.revision,
            observed_at: runtime.observed_at,
            platform: runtime.platform,
            argument_template: proto_arguments(&runtime.launch_arguments),
            extensions: Default::default(),
        })
    }

    /// Resolves a recipe's runtime and selected binding under the authenticated plugin owner.
    /// All durable file refs are revalidated against current FILES_READ consent before any path
    /// reaches the engine worker.
    pub(super) async fn resolve_runtime_launch(
        &self,
        caller: &ResourceCaller,
        context: &ServiceContext,
        game_id: u64,
        installation_id: &str,
        runtime_id: &str,
        content_id: &str,
    ) -> Result<crate::plugin_host::library::ResolvedRuntimeLaunch, Status> {
        if !valid_id(runtime_id) {
            return Err(Status::invalid_argument("runtime id is required"));
        }
        self.providers
            .authorize_resource(Some(context), cap::FILES_READ)?;
        let owner = mutation_owner(caller);
        let host = self.library.host().ok_or_else(unavailable)?;
        let runtime = host
            .runtime_resource(owner.clone(), runtime_id.to_owned())
            .await?
            .ok_or_else(|| Status::not_found("runtime not found"))?;
        let platform = current_platform();
        if runtime.platform != platform {
            return Err(Status::unimplemented(
                "the registered runtime targets a different platform",
            ));
        }
        let executable = self
            .resolve_reference_path(caller, runtime.executable.clone(), None)
            .await?;
        let mut content_path = None;
        let mut binding_revision = None;
        if content_id.is_empty() {
            if runtime
                .launch_arguments
                .iter()
                .any(|argument| matches!(argument, CoreArgument::ContentPath))
            {
                return Err(Status::failed_precondition(
                    "this runtime recipe requires a registered content binding",
                ));
            }
        } else {
            let bindings = host
                .runtime_content_bindings(owner, game_id, installation_id.to_owned())
                .await?;
            let binding = bindings
                .into_iter()
                .find(|binding| {
                    binding.content_id == content_id && binding.runtime_id == runtime_id
                })
                .ok_or_else(|| Status::not_found("runtime content binding not found"))?;
            if !runtime.supported_formats.contains(&binding.content_kind) {
                return Err(Status::failed_precondition(
                    "the runtime does not support this content kind",
                ));
            }
            if !binding.supported_platforms.is_empty()
                && !binding
                    .supported_platforms
                    .iter()
                    .any(|value| value == platform)
            {
                return Err(Status::failed_precondition(
                    "the content binding does not support this platform",
                ));
            }
            binding_revision = Some(binding.revision);
            content_path = Some(
                self.resolve_reference_path(
                    caller,
                    binding.content,
                    Some((game_id, installation_id.to_owned())),
                )
                .await?,
            );
        }
        self.authorize_commit(context, cap::FILES_READ, caller.grant_revision)?;
        Ok(crate::plugin_host::library::ResolvedRuntimeLaunch {
            runtime_id: runtime.id,
            platform: runtime.platform,
            executable,
            content_id: content_id.to_owned(),
            content_path,
            runtime_revision: runtime.revision,
            binding_revision,
            arguments: runtime.launch_arguments,
        })
    }

    fn runtime_core(
        request: RuntimeRecord,
        executable: ApprovedFileReference,
        id: String,
        revision: u64,
        observed_at: i64,
    ) -> Result<RuntimeResource, Status> {
        runtime_request_shape(&request, true)?;
        let runtime = RuntimeResource {
            id,
            name: request.name,
            version: request.version,
            executable,
            platform: request.platform,
            supported_formats: request.supported_formats,
            launch_arguments: core_arguments(&request.argument_template)?,
            revision,
            observed_at,
        };
        if runtime.validate() {
            Ok(runtime)
        } else {
            Err(Status::invalid_argument("invalid runtime resource"))
        }
    }

    pub(super) async fn list_runtimes(
        &self,
        request: Request<DomainListRequest>,
    ) -> Result<Response<RuntimesListReply>, Status> {
        let request = request.into_inner();
        let page = request.page.unwrap_or_default();
        let context = page
            .context
            .clone()
            .ok_or_else(|| Status::unauthenticated("resource context required"))?;
        let caller = self
            .providers
            .authorize_resource(page.context.as_ref(), cap::GAME_RUNTIME_READ)?;
        self.providers
            .authorize_resource(page.context.as_ref(), cap::FILES_READ)?;
        validate_common_selector(page.common_data_categories.as_ref())?;
        if !request.kind.is_empty() && request.kind != "runtime" {
            return Err(Status::invalid_argument("unsupported runtime kind"));
        }
        if !request.owner_filter.is_empty() && request.owner_filter != caller.provider_id {
            return Err(Status::permission_denied(
                "foreign runtime scope is not granted",
            ));
        }
        let owner = mutation_owner(&caller);
        let host = self.library.host().ok_or_else(unavailable)?;
        let mut revision = host.runtime_catalog_revision(owner.clone()).await?;
        let mut records = host.runtime_resources(owner.clone()).await?;
        if host.runtime_catalog_revision(owner.clone()).await? != revision {
            revision = host.runtime_catalog_revision(owner.clone()).await?;
            records = host.runtime_resources(owner.clone()).await?;
        }
        records.sort_by(|left, right| left.id.cmp(&right.id));
        let limit = match page.page_size as usize {
            0 => DEFAULT_PAGE_SIZE,
            size => size.min(MAX_PAGE_SIZE),
        };
        let binding = CursorBinding {
            route: "runtimes",
            session_id: caller.session_id.clone(),
            profile_epoch: caller.profile_epoch,
            grant_revision: caller.grant_revision,
            scope: Scope {
                own_provider: caller.provider_id.clone(),
                read_all: false,
                provider_filter: None,
            },
            search: None,
        };
        let after = if page.cursor.is_empty() {
            None
        } else {
            let (after, issued) =
                self.library
                    .open_cursor(&page.cursor, &binding)
                    .map_err(|error| match error {
                        CursorError::Malformed => {
                            Status::invalid_argument("invalid runtime cursor")
                        }
                        CursorError::Stale => super::stale_cursor(),
                    })?;
            if issued != revision {
                return Err(super::stale_cursor());
            }
            Some(after)
        };
        let mut remaining = records
            .into_iter()
            .filter(|record| after.as_ref().is_none_or(|after| record.id > *after));
        let selected = remaining.by_ref().take(limit).collect::<Vec<_>>();
        let more = remaining.next().is_some();
        let next_cursor = match selected.last().filter(|_| more) {
            Some(last) => self.library.issue_cursor(&binding, revision, &last.id),
            None => String::new(),
        };
        let mut runtimes = Vec::with_capacity(selected.len());
        for record in selected {
            runtimes.push(self.runtime_proto(&caller, &context, record).await?);
        }
        self.providers
            .authorize_resource(page.context.as_ref(), cap::GAME_RUNTIME_READ)?;
        Ok(Response::new(RuntimesListReply {
            runtimes,
            next_cursor,
            revision,
        }))
    }

    pub(super) async fn get_runtime(
        &self,
        request: Request<DomainGetRequest>,
    ) -> Result<Response<RuntimeRecordReply>, Status> {
        let request = request.into_inner();
        let context = request
            .context
            .clone()
            .ok_or_else(|| Status::unauthenticated("resource context required"))?;
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::GAME_RUNTIME_READ)?;
        self.providers
            .authorize_resource(request.context.as_ref(), cap::FILES_READ)?;
        validate_common_selector(request.shared_records.as_ref())?;
        if !valid_id(&request.resource_id) {
            return Err(Status::invalid_argument("runtime id is required"));
        }
        let host = self.library.host().ok_or_else(unavailable)?;
        let runtime = host
            .runtime_resource(mutation_owner(&caller), request.resource_id.clone())
            .await?
            .ok_or_else(|| Status::not_found("runtime not found"))?;
        if request.expected_revision != 0 && request.expected_revision != runtime.revision {
            return Err(Status::aborted("runtime revision changed"));
        }
        let runtime = self.runtime_proto(&caller, &context, runtime).await?;
        self.providers
            .authorize_resource(request.context.as_ref(), cap::GAME_RUNTIME_READ)?;
        Ok(Response::new(RuntimeRecordReply {
            revision: runtime.revision,
            runtime: Some(runtime),
        }))
    }

    pub(super) async fn register_runtime(
        &self,
        request: Request<RuntimeRegisterRequest>,
    ) -> Result<Response<RuntimeMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::GAME_RUNTIME_CREATE)?;
        let key = mutation_key(mutation)?;
        if mutation.expected_revision != 0 {
            return Err(Status::invalid_argument(
                "runtime registration expected_revision must be 0",
            ));
        }
        let mut runtime_request = request
            .runtime
            .ok_or_else(|| Status::invalid_argument("runtime is required"))?;
        runtime_request_shape(&runtime_request, true)?;
        let request_id = runtime_request.id.clone();
        let hash = request_hash(&[
            "runtime-register",
            &request_id,
            &runtime_request.name,
            &runtime_request.version,
            &runtime_request.executable_handle,
            &runtime_request.platform,
            &runtime_request.supported_formats.join("\0"),
            &format!("{:?}", runtime_request.argument_template),
            &mutation.expected_revision.to_string(),
        ]);
        let lock = self.mutations.lock.lock().await;
        if let Some(replay) = self.mutations.replay(&caller, key, hash)? {
            let runtime = self
                .library
                .host()
                .ok_or_else(unavailable)?
                .runtime_resource(mutation_owner(&caller), replay.resource_id.clone())
                .await?
                .ok_or_else(|| Status::not_found("runtime not found"))?;
            return Ok(Response::new(RuntimeMutationReply {
                runtime: Some(self.runtime_proto(&caller, context, runtime).await?),
                outcome: replay.outcome,
            }));
        }
        let id = if request_id.is_empty() {
            format!("runtime-{}", runtime_id()?)
        } else {
            request_id
        };
        self.providers
            .authorize_resource(Some(context), cap::FILES_READ)?;
        let executable = self
            .capture_reference(&caller, runtime_request.executable_handle.clone(), None)
            .await?;
        let core = Self::runtime_core(
            runtime_request.clone(),
            executable,
            id.clone(),
            1,
            unix_now(),
        )?;
        let host = self.library.host().ok_or_else(unavailable)?;
        self.authorize_commit(context, cap::GAME_RUNTIME_CREATE, caller.grant_revision)?;
        self.authorize_commit(context, cap::FILES_READ, caller.grant_revision)?;
        let result = host
            .save_runtime_resource(mutation_owner(&caller), core)
            .await?;
        let (outcome, revision, commit) = mutation_outcome(result);
        let reply = super::mutations::reply(&id, revision, outcome);
        if commit {
            self.mutations.remember(&caller, key, hash, &reply);
            runtime_request.id = id.clone();
            runtime_request.revision = revision;
            runtime_request.observed_at = unix_now();
            let record = self
                .library
                .host()
                .ok_or_else(unavailable)?
                .runtime_resource(mutation_owner(&caller), id.clone())
                .await?
                .ok_or_else(|| Status::internal("saved runtime disappeared"))?;
            drop(lock);
            return Ok(Response::new(RuntimeMutationReply {
                runtime: Some(self.runtime_proto(&caller, context, record).await?),
                outcome: outcome as i32,
            }));
        }
        drop(lock);
        Ok(Response::new(RuntimeMutationReply {
            runtime: None,
            outcome: outcome as i32,
        }))
    }

    pub(super) async fn update_runtime(
        &self,
        request: Request<RuntimeUpdateRequest>,
    ) -> Result<Response<RuntimeMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::GAME_RUNTIME_UPDATE)?;
        let key = mutation_key(mutation)?;
        let mut runtime_request = request
            .runtime
            .ok_or_else(|| Status::invalid_argument("runtime is required"))?;
        runtime_request_shape(&runtime_request, false)?;
        if !valid_id(&runtime_request.id) || mutation.expected_revision == 0 {
            return Err(Status::invalid_argument(
                "runtime id and expected_revision are required",
            ));
        }
        let known = [
            "name",
            "version",
            "executable_handle",
            "platform",
            "supported_formats",
            "argument_template",
        ];
        let mut mask = HashSet::new();
        if request.field_mask.is_empty()
            || request
                .field_mask
                .iter()
                .any(|field| !known.contains(&field.as_str()) || !mask.insert(field.as_str()))
        {
            return Err(Status::invalid_argument("invalid runtime field_mask"));
        }
        if mask.contains("executable_handle") && runtime_request.executable_handle.is_empty() {
            return Err(Status::invalid_argument(
                "executable_handle is required by field_mask",
            ));
        }
        let hash = request_hash(&[
            "runtime-update",
            &runtime_request.id,
            &mutation.expected_revision.to_string(),
            &request.field_mask.join("\0"),
            &runtime_request.name,
            &runtime_request.version,
            &runtime_request.executable_handle,
            &runtime_request.platform,
            &runtime_request.supported_formats.join("\0"),
            &format!("{:?}", runtime_request.argument_template),
        ]);
        let lock = self.mutations.lock.lock().await;
        if let Some(replay) = self.mutations.replay(&caller, key, hash)? {
            let runtime = self
                .library
                .host()
                .ok_or_else(unavailable)?
                .runtime_resource(mutation_owner(&caller), replay.resource_id.clone())
                .await?
                .ok_or_else(|| Status::not_found("runtime not found"))?;
            return Ok(Response::new(RuntimeMutationReply {
                runtime: Some(self.runtime_proto(&caller, context, runtime).await?),
                outcome: replay.outcome,
            }));
        }
        let host = self.library.host().ok_or_else(unavailable)?;
        let owner = mutation_owner(&caller);
        let Some(current) = host
            .runtime_resource(owner.clone(), runtime_request.id.clone())
            .await?
        else {
            return Err(Status::not_found("runtime not found"));
        };
        if current.revision != mutation.expected_revision {
            return Ok(Response::new(RuntimeMutationReply {
                runtime: None,
                outcome: MutationOutcome::RevisionConflict as i32,
            }));
        }
        let executable = if mask.contains("executable_handle") {
            self.providers
                .authorize_resource(Some(context), cap::FILES_READ)?;
            Some(
                self.capture_reference(&caller, runtime_request.executable_handle.clone(), None)
                    .await?,
            )
        } else {
            None
        };
        let updated = RuntimeResource {
            id: current.id.clone(),
            name: if mask.contains("name") {
                runtime_request.name
            } else {
                current.name
            },
            version: if mask.contains("version") {
                runtime_request.version
            } else {
                current.version
            },
            executable: executable.unwrap_or(current.executable),
            platform: if mask.contains("platform") {
                runtime_request.platform
            } else {
                current.platform
            },
            supported_formats: if mask.contains("supported_formats") {
                runtime_request.supported_formats
            } else {
                current.supported_formats
            },
            launch_arguments: if mask.contains("argument_template") {
                core_arguments(&runtime_request.argument_template)?
            } else {
                current.launch_arguments
            },
            revision: current.revision.saturating_add(1),
            observed_at: unix_now(),
        };
        if !updated.validate() {
            return Err(Status::invalid_argument("invalid runtime resource"));
        }
        self.authorize_commit(context, cap::GAME_RUNTIME_UPDATE, caller.grant_revision)?;
        if mask.contains("executable_handle") {
            self.authorize_commit(context, cap::FILES_READ, caller.grant_revision)?;
        }
        let result = host.save_runtime_resource(owner.clone(), updated).await?;
        let (outcome, revision, commit) = mutation_outcome(result);
        let reply = super::mutations::reply(&runtime_request.id, revision, outcome);
        if commit {
            self.mutations.remember(&caller, key, hash, &reply);
            runtime_request.revision = revision;
            runtime_request.observed_at = unix_now();
            let updated = host
                .runtime_resource(owner, runtime_request.id.clone())
                .await?
                .ok_or_else(|| Status::internal("updated runtime disappeared"))?;
            drop(lock);
            return Ok(Response::new(RuntimeMutationReply {
                runtime: Some(self.runtime_proto(&caller, context, updated).await?),
                outcome: outcome as i32,
            }));
        }
        drop(lock);
        Ok(Response::new(RuntimeMutationReply {
            runtime: None,
            outcome: outcome as i32,
        }))
    }

    pub(super) async fn remove_runtime(
        &self,
        request: Request<DomainRemoveRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::GAME_RUNTIME_DELETE)?;
        let key = mutation_key(mutation)?;
        if !valid_id(&request.resource_id) || mutation.expected_revision == 0 {
            return Err(Status::invalid_argument(
                "runtime id and expected_revision are required",
            ));
        }
        let hash = request_hash(&[
            "runtime-remove",
            &request.resource_id,
            &mutation.expected_revision.to_string(),
        ]);
        let lock = self.mutations.lock.lock().await;
        if let Some(replay) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(replay));
        }
        let host = self.library.host().ok_or_else(unavailable)?;
        let owner = mutation_owner(&caller);
        if host
            .runtime_resource(owner.clone(), request.resource_id.clone())
            .await?
            .is_none()
        {
            return Err(Status::not_found("runtime not found"));
        }
        self.authorize_commit(context, cap::GAME_RUNTIME_DELETE, caller.grant_revision)?;
        let result = host
            .remove_runtime_resource(
                owner,
                request.resource_id.clone(),
                mutation.expected_revision,
            )
            .await?;
        let (outcome, revision, commit) = mutation_outcome(result);
        let reply = super::mutations::reply(&request.resource_id, revision, outcome);
        if commit {
            self.mutations.remember(&caller, key, hash, &reply);
        }
        drop(lock);
        Ok(Response::new(reply))
    }

    pub(super) async fn register_content(
        &self,
        request: Request<ContentRegistrationRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::DISCOVERY)?;
        let key = mutation_key(mutation)?;
        if !valid_id(&request.installation_id)
            || !valid_id(&request.content_kind)
            || !valid_id(&request.runtime_id)
            || request.supported_platforms.len() > 128
            || request
                .supported_platforms
                .iter()
                .any(|platform| !valid_id(platform))
        {
            return Err(Status::invalid_argument("invalid runtime content binding"));
        }
        let game_id = super::mutations::game_id(&request.game_id)?
            .ok_or_else(|| Status::invalid_argument("game_id is required"))?;
        let context_target =
            self.action_target(&caller, context, &request.game_id, &request.installation_id)?;
        let (_entry_key, entry) = context_target;
        if entry.game_id != Some(game_id) {
            return Err(Status::not_found("library game not found"));
        }
        let owner = mutation_owner(&caller);
        let host = self.library.host().ok_or_else(unavailable)?;
        if host
            .runtime_resource(owner.clone(), request.runtime_id.clone())
            .await?
            .is_none()
        {
            return Err(Status::not_found("runtime not found"));
        }
        if !request.content_id.is_empty() && !valid_id(&request.content_id) {
            return Err(Status::invalid_argument("invalid content_id"));
        }
        let all_bindings = host
            .runtime_content_bindings(owner.clone(), game_id, request.installation_id.clone())
            .await?;
        if !request.content_id.is_empty()
            && !all_bindings
                .iter()
                .any(|binding| binding.content_id == request.content_id)
        {
            return Err(Status::not_found("runtime content not found"));
        }
        let requested_content_id = request.content_id.clone();
        let hash = request_hash(&[
            "runtime-content-register",
            &game_id.to_string(),
            &request.installation_id,
            &requested_content_id,
            &request.content_handle,
            &request.content_kind,
            &request.runtime_id,
            &request.supported_platforms.join("\0"),
            &mutation.expected_revision.to_string(),
        ]);
        let lock = self.mutations.lock.lock().await;
        if let Some(replay) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(replay));
        }
        let content_id = if requested_content_id.is_empty() {
            format!("content-{}", runtime_id()?)
        } else {
            requested_content_id
        };
        self.providers
            .authorize_resource(Some(context), cap::FILES_READ)?;
        let content = self
            .capture_reference(
                &caller,
                request.content_handle.clone(),
                Some((game_id, request.installation_id.clone())),
            )
            .await?;
        let rows = all_bindings
            .into_iter()
            .filter(|binding| binding.content_id == content_id)
            .collect::<Vec<_>>();
        if rows.len() > MAX_BINDINGS_PER_ENTRY {
            return Err(Status::resource_exhausted(
                "too many runtime content bindings",
            ));
        }
        let existing = rows
            .iter()
            .find(|binding| binding.runtime_id == request.runtime_id)
            .cloned();
        if existing
            .as_ref()
            .map(|binding| binding.revision)
            .unwrap_or(0)
            != mutation.expected_revision
        {
            return Ok(Response::new(super::mutations::reply(
                &content_id,
                existing.map(|binding| binding.revision).unwrap_or(0),
                MutationOutcome::RevisionConflict,
            )));
        }
        let binding_revision = mutation.expected_revision.saturating_add(1);
        let binding = RuntimeContentBinding {
            game_id,
            installation_id: request.installation_id.clone(),
            content_id: content_id.clone(),
            content: content.clone(),
            content_kind: request.content_kind.clone(),
            runtime_id: request.runtime_id.clone(),
            supported_platforms: request.supported_platforms.clone(),
            revision: binding_revision,
        };
        if !binding.validate() {
            return Err(Status::invalid_argument("invalid runtime content binding"));
        }
        self.authorize_commit(context, cap::DISCOVERY, caller.grant_revision)?;
        self.authorize_commit(context, cap::FILES_READ, caller.grant_revision)?;
        // A move/rename updates the current location for every runtime binding of this stable
        // content id, preserving the identity and the other selectable runtimes.
        let saved = host.save_runtime_content_binding(owner, binding).await?;
        let (outcome, revision, commit) = mutation_outcome(saved);
        let reply = super::mutations::reply(&content_id, revision, outcome);
        if commit {
            self.mutations.remember(&caller, key, hash, &reply);
        }
        drop(lock);
        Ok(Response::new(reply))
    }

    pub(super) async fn watch_runtimes(
        &self,
        request: Request<DomainWatchRequest>,
    ) -> Result<Response<super::BoxStream<RuntimeRecordEvent>>, Status> {
        let request = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::GAME_RUNTIME_READ)?;
        self.providers
            .authorize_resource(request.context.as_ref(), cap::FILES_READ)?;
        validate_common_selector(request.shared_records.as_ref())?;
        let host = self.library.host().ok_or_else(unavailable)?;
        let owner = mutation_owner(&caller);
        let slot = self.watch_slot(&caller.session_id)?;
        let initial_revision = host.runtime_catalog_revision(owner.clone()).await?;
        if request.after_revision > initial_revision {
            return Err(Status::invalid_argument(
                "runtime watch revision is in the future",
            ));
        }
        let mut initial = host.runtime_resources(owner.clone()).await?;
        initial.sort_by(|left, right| left.id.cmp(&right.id));
        let (sender, receiver) = mpsc::channel(32);
        let providers = self.providers.clone();
        let resource = self.clone();
        let context = request.context.unwrap_or_default();
        let resource_id = request.resource_id;
        tokio::spawn(async move {
            let _slot = slot;
            let mut last_revision = request.after_revision;
            let mut last = initial;
            if last_revision < initial_revision {
                let event = RuntimeRecordEvent {
                    revision: initial_revision,
                    event_type: "reset".into(),
                    runtime: None,
                };
                if sender.send(Ok(event)).await.is_err() {
                    return;
                }
                for runtime in &last {
                    if !resource_id.is_empty() && runtime.id != resource_id {
                        continue;
                    }
                    match resource
                        .runtime_proto(&caller, &context, runtime.clone())
                        .await
                    {
                        Ok(runtime) => {
                            if sender
                                .send(Ok(RuntimeRecordEvent {
                                    revision: initial_revision,
                                    event_type: "upserted".into(),
                                    runtime: Some(runtime),
                                }))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(error) => {
                            let _ = sender.send(Err(error)).await;
                            return;
                        }
                    }
                }
                last_revision = initial_revision;
            }
            let mut recheck = tokio::time::interval(WATCH_RECHECK);
            let mut poll = tokio::time::interval(WATCH_POLL);
            loop {
                tokio::select! {
                    _ = recheck.tick() => {
                        if let Err(error) = providers.authorize_resource(Some(&context), cap::GAME_RUNTIME_READ)
                            .and_then(|_| providers.authorize_resource(Some(&context), cap::FILES_READ)) {
                            let _ = sender.send(Err(error)).await;
                            return;
                        }
                    }
                    _ = poll.tick() => {
                        let revision = match host.runtime_catalog_revision(owner.clone()).await {
                            Ok(revision) => revision,
                            Err(error) => { let _ = sender.send(Err(error)).await; return; }
                        };
                        if revision == last_revision { continue; }
                        let mut current = match host.runtime_resources(owner.clone()).await {
                            Ok(records) => records,
                            Err(error) => { let _ = sender.send(Err(error)).await; return; }
                        };
                        current.sort_by(|left, right| left.id.cmp(&right.id));
                        let removed = last.iter().any(|old| !current.iter().any(|new| new.id == old.id));
                        if removed {
                            if sender.send(Ok(RuntimeRecordEvent { revision, event_type: "reset".into(), runtime: None })).await.is_err() { return; }
                        }
                        for runtime in &current {
                            let changed = last.iter().find(|old| old.id == runtime.id).is_none_or(|old| old.revision != runtime.revision);
                            if !changed || (!resource_id.is_empty() && runtime.id != resource_id) { continue; }
                            match resource.runtime_proto(&caller, &context, runtime.clone()).await {
                                Ok(runtime) => {
                                    if sender.send(Ok(RuntimeRecordEvent { revision, event_type: "upserted".into(), runtime: Some(runtime) })).await.is_err() { return; }
                                }
                                Err(error) => { let _ = sender.send(Err(error)).await; return; }
                            }
                        }
                        if current.iter().all(|runtime| last.iter().any(|old| old.id == runtime.id && old.revision == runtime.revision)) {
                            let _ = sender.send(Ok(RuntimeRecordEvent { revision, event_type: "bindings_changed".into(), runtime: None })).await;
                        }
                        last = current;
                        last_revision = revision;
                    }
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}
