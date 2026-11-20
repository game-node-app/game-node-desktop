//! Owner-scoped installed runtime resources and game-content bindings.
//!
//! Plugin file handles are session scoped. Durable records keep only the host-owned approval id
//! and relative path; reads issue a fresh handle after checking the approval and current grant.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use gamenode_core::library::{
    ApprovedFileReference, LaunchPlanResource, RuntimeArgumentTemplate as CoreArgument,
    RuntimeContentBinding, RuntimeExtension, RuntimeMutation, RuntimeProvenance, RuntimeResource,
};
use gamenode_plugin_protocol::{
    capability as cap,
    extensions::{MAX_EXTENSION_BYTES, validate_payload},
    proto::*,
};
use tonic::{Request, Response, Status};

use super::mutations::{mutation_key, mutation_owner, request_hash, unavailable};
use super::{
    CursorBinding, CursorError, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, ResourceApi, Scope,
    json_to_struct, struct_to_json,
};
use crate::plugin_host::provider::ResourceCaller;
use crate::plugin_host::sharing_policy::AccessDecision;

const MAX_RUNTIME_ID_BYTES: usize = 128;
const MAX_BINDINGS_PER_ENTRY: usize = 512;
const WATCH_RECHECK: std::time::Duration = std::time::Duration::from_secs(5);
const WATCH_POLL: std::time::Duration = std::time::Duration::from_millis(500);
/// Projection names a common runtime-catalog reader may see. Executable handles, approvals and
/// private owner identifiers are never part of it.
const SHARED_RUNTIME_FIELDS: &[&str] = &[
    "id",
    "name",
    "version",
    "platform",
    "supported_formats",
    "argument_template",
    "extensions",
    "provenance",
    "revision",
    "observed_at",
];

struct ResolvedRuntimePath {
    path: String,
    resource: LaunchPlanResource,
}

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
    match selector {
        Some(selector) if !selector.selected_record_ids.is_empty() => Err(Status::unimplemented(
            "selected-record runtime catalog reads are not available in this build",
        )),
        Some(selector)
            if !selector.categories.is_empty()
                && selector.categories != [CommonDataCategory::RuntimeCatalog as i32] =>
        {
            Err(Status::unimplemented(
                "common runtime catalog projections support only the runtime catalog category",
            ))
        }
        _ => Ok(()),
    }
}

fn runtime_extension_payloads(
    record: &RuntimeRecord,
) -> Result<BTreeMap<String, RuntimeExtension>, Status> {
    let mut extensions = BTreeMap::new();
    let mut total = 0usize;
    for (namespace, payload) in &record.extensions {
        let valid_namespace = !namespace.is_empty()
            && namespace.len() <= MAX_RUNTIME_ID_BYTES
            && namespace
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
        if !valid_namespace
            || (payload.schema_id != *namespace
                && !payload.schema_id.starts_with(&format!("{namespace}.")))
        {
            return Err(Status::invalid_argument(
                "runtime extension namespace and schema do not match",
            ));
        }
        validate_payload(payload)
            .map_err(|_| Status::invalid_argument("invalid runtime extension payload"))?;
        let data = payload
            .data
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("runtime extension data is required"))?;
        let data = serde_json::Value::Object(struct_to_json(data)?);
        total = total
            .checked_add(serde_json::to_vec(&data).map_or(usize::MAX, |bytes| bytes.len()))
            .ok_or_else(|| Status::invalid_argument("runtime extension size overflow"))?;
        if total > MAX_EXTENSION_BYTES {
            return Err(Status::resource_exhausted(
                "runtime extensions exceed the accepted size",
            ));
        }
        extensions.insert(
            namespace.clone(),
            RuntimeExtension {
                schema_id: payload.schema_id.clone(),
                schema_version: payload.schema_version,
                schema_digest: payload.schema_digest.clone(),
                data,
            },
        );
    }
    Ok(extensions)
}

fn runtime_extension_proto(
    extensions: &BTreeMap<String, RuntimeExtension>,
) -> Result<HashMap<String, ExtensionPayload>, Status> {
    extensions
        .iter()
        .map(|(namespace, extension)| {
            let serde_json::Value::Object(data) = &extension.data else {
                return Err(Status::internal("stored runtime extension is invalid"));
            };
            Ok((
                namespace.clone(),
                ExtensionPayload {
                    schema_id: extension.schema_id.clone(),
                    schema_version: extension.schema_version,
                    schema_digest: extension.schema_digest.clone(),
                    data: Some(json_to_struct(data)),
                },
            ))
        })
        .collect()
}

fn runtime_provenance(caller: &ResourceCaller) -> RuntimeProvenance {
    let identity = caller
        .namespace
        .as_ref()
        .map(|namespace| namespace.identity());
    RuntimeProvenance {
        plugin_id: identity
            .map(|identity| identity.plugin_id.clone())
            .unwrap_or_default(),
        declared_developer: caller.declared_developer.clone(),
        origin: caller.origin.clone(),
        publisher_fingerprint: identity
            .map(|identity| identity.publisher_fingerprint.clone())
            .unwrap_or_default(),
        package_version: caller.package_version.clone(),
        package_digest: caller.package_digest.clone(),
    }
}

fn runtime_provenance_proto(provenance: &RuntimeProvenance) -> Option<DataProvenance> {
    (!provenance.plugin_id.is_empty()).then(|| DataProvenance {
        producer_plugin_id: provenance.plugin_id.clone(),
        declared_developer: provenance.declared_developer.clone(),
        origin: provenance.origin.clone(),
        publisher_fingerprint: provenance.publisher_fingerprint.clone(),
        package_version: provenance.package_version.clone(),
        package_digest: provenance.package_digest.clone(),
        ..Default::default()
    })
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RUNTIME_ID_BYTES
        && !value
            .chars()
            .any(|character| character.is_control() || character == '\0')
}

impl ResourceApi {
    pub(super) fn authorize_commit(
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
        host_initiated: bool,
    ) -> Result<ResolvedRuntimePath, Status> {
        if !reference.validate() {
            return Err(Status::failed_precondition(
                "approved file reference is invalid",
            ));
        }
        let files = self.files.clone();
        let feed = self.library.clone();
        let reference_for_open = reference.clone();
        let resolve = move |namespace: &mut crate::plugin_host::storage::Namespace<'_>| {
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
            let path = canonical
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| Status::failed_precondition("approved path is not valid Unicode"))?;
            Ok(ResolvedRuntimePath {
                path,
                resource: LaunchPlanResource {
                    id: format!(
                        "file-approval:{}:{}",
                        opened.approval.id,
                        reference_for_open.relative_path.join("/")
                    ),
                    revision: opened.approval.approved_at.max(1) as u64,
                },
            })
        };
        if host_initiated {
            self.in_host_namespace(caller, resolve).await
        } else {
            self.in_namespace(caller, resolve).await
        }
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
            extensions: runtime_extension_proto(&runtime.extensions)?,
            provenance: runtime_provenance_proto(&runtime.provenance),
        })
    }

    /// Shared catalog projection: no executable handle, approval or private owner key is exposed.
    fn runtime_proto_shared(runtime: RuntimeResource) -> Result<RuntimeRecord, Status> {
        Ok(RuntimeRecord {
            id: runtime.id,
            name: runtime.name,
            version: runtime.version,
            executable_handle: String::new(),
            supported_formats: runtime.supported_formats,
            revision: runtime.revision,
            observed_at: runtime.observed_at,
            platform: runtime.platform,
            argument_template: proto_arguments(&runtime.launch_arguments),
            extensions: runtime_extension_proto(&runtime.extensions)?,
            provenance: runtime_provenance_proto(&runtime.provenance),
        })
    }

    /// Returns whether this query explicitly selected the runtime catalog category and is
    /// currently granted. An empty selector keeps the query owner-private.
    fn shared_runtime_read(
        &self,
        caller: &ResourceCaller,
        selector: Option<&CommonDataSelector>,
    ) -> Result<bool, Status> {
        validate_common_selector(selector)?;
        let Some(selector) = selector.filter(|selector| !selector.categories.is_empty()) else {
            return Ok(false);
        };
        let recipient = caller
            .namespace
            .as_ref()
            .map(|namespace| namespace.identity().clone())
            .ok_or_else(|| {
                Status::failed_precondition(
                    "a third-party plugin identity is required for shared catalog reads",
                )
            })?;
        let fields = SHARED_RUNTIME_FIELDS
            .iter()
            .map(|field| (*field).to_owned())
            .collect::<BTreeSet<_>>();
        let decision: AccessDecision = self.shared_authorizer.common_category(
            &recipient,
            CommonDataCategory::RuntimeCatalog,
            &fields,
        );
        if !decision.allowed {
            return Err(Status::permission_denied(
                "common runtime catalog access is not granted",
            ));
        }
        debug_assert_eq!(
            selector.categories,
            [CommonDataCategory::RuntimeCatalog as i32]
        );
        Ok(true)
    }

    /// Resolves a recipe's runtime and selected binding under the authenticated plugin owner.
    /// All durable file refs are revalidated against current FILES_READ consent before any path
    /// reaches the engine worker.
    pub(super) async fn resolve_runtime_launch(
        &self,
        caller: &ResourceCaller,
        context: Option<&ServiceContext>,
        game_id: u64,
        installation_id: &str,
        runtime_id: &str,
        content_id: &str,
    ) -> Result<crate::plugin_host::library::ResolvedRuntimeLaunch, Status> {
        if !valid_id(runtime_id) {
            return Err(Status::invalid_argument("runtime id is required"));
        }
        if let Some(context) = context {
            self.providers
                .authorize_resource(Some(context), cap::FILES_READ)?;
        } else if !caller.has(cap::FILES_READ) {
            return Err(Status::permission_denied(
                "file read permission is required",
            ));
        }
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
            .resolve_reference_path(caller, runtime.executable.clone(), None, context.is_none())
            .await?;
        let mut resources = vec![executable.resource.clone()];
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
            let content = self
                .resolve_reference_path(
                    caller,
                    binding.content,
                    Some((game_id, installation_id.to_owned())),
                    context.is_none(),
                )
                .await?;
            resources.push(content.resource);
            content_path = Some(content.path);
        }
        if let Some(context) = context {
            self.authorize_commit(context, cap::FILES_READ, caller.grant_revision)?;
        }
        Ok(crate::plugin_host::library::ResolvedRuntimeLaunch {
            runtime_id: runtime.id,
            version: runtime.version,
            platform: runtime.platform,
            executable: executable.path,
            content_id: content_id.to_owned(),
            content_path,
            runtime_revision: runtime.revision,
            binding_revision,
            arguments: runtime.launch_arguments,
            resources,
            // This branch cannot create shared-save scopes. Data 1.8 must populate these keys
            // from its reviewed host-owned resource bindings before enabling those scopes.
            lease_keys: Vec::new(),
        })
    }

    fn runtime_core(
        request: RuntimeRecord,
        executable: ApprovedFileReference,
        id: String,
        revision: u64,
        observed_at: i64,
        provenance: RuntimeProvenance,
        extensions: BTreeMap<String, RuntimeExtension>,
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
            provenance,
            extensions,
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
        let shared = self.shared_runtime_read(&caller, page.common_data_categories.as_ref())?;
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
        let mut revision = if shared {
            host.shared_runtime_catalog_revision().await?
        } else {
            host.runtime_catalog_revision(owner.clone()).await?
        };
        let mut records = host
            .runtime_resources(owner.clone())
            .await?
            .into_iter()
            .map(|runtime| (false, runtime))
            .collect::<Vec<_>>();
        if shared {
            records.extend(
                host.shared_runtime_resources()
                    .await?
                    .into_iter()
                    .filter(|(owner_key, _)| *owner_key != owner)
                    .map(|(_, runtime)| (true, runtime)),
            );
        }
        let reread = if shared {
            host.shared_runtime_catalog_revision().await?
        } else {
            host.runtime_catalog_revision(owner.clone()).await?
        };
        if reread != revision {
            revision = reread;
            records = host
                .runtime_resources(owner.clone())
                .await?
                .into_iter()
                .map(|runtime| (false, runtime))
                .collect();
            if shared {
                records.extend(
                    host.shared_runtime_resources()
                        .await?
                        .into_iter()
                        .filter(|(owner_key, _)| *owner_key != owner)
                        .map(|(_, runtime)| (true, runtime)),
                );
            }
        }
        records.sort_by(|left, right| {
            left.1
                .id
                .cmp(&right.1.id)
                .then_with(|| left.0.cmp(&right.0))
        });
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
                // Reused as the shared-catalog marker for this route so a private cursor cannot
                // resume a category query, and vice versa.
                read_all: shared,
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
            .filter(|record| after.as_ref().is_none_or(|after| record.1.id > *after));
        let selected = remaining.by_ref().take(limit).collect::<Vec<_>>();
        let more = remaining.next().is_some();
        let next_cursor = match selected.last().filter(|_| more) {
            Some(last) => self.library.issue_cursor(&binding, revision, &last.1.id),
            None => String::new(),
        };
        let mut runtimes = Vec::with_capacity(selected.len());
        for (is_shared, record) in selected {
            runtimes.push(if is_shared {
                Self::runtime_proto_shared(record)?
            } else {
                self.runtime_proto(&caller, &context, record).await?
            });
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
        let shared = self.shared_runtime_read(&caller, request.shared_records.as_ref())?;
        if !valid_id(&request.resource_id) {
            return Err(Status::invalid_argument("runtime id is required"));
        }
        let host = self.library.host().ok_or_else(unavailable)?;
        let owner = mutation_owner(&caller);
        if let Some(runtime) = host
            .runtime_resource(owner.clone(), request.resource_id.clone())
            .await?
        {
            if request.expected_revision != 0 && request.expected_revision != runtime.revision {
                return Err(Status::aborted("runtime revision changed"));
            }
            let runtime = self.runtime_proto(&caller, &context, runtime).await?;
            self.providers
                .authorize_resource(request.context.as_ref(), cap::GAME_RUNTIME_READ)?;
            return Ok(Response::new(RuntimeRecordReply {
                revision: runtime.revision,
                runtime: Some(runtime),
            }));
        }
        if !shared {
            return Err(Status::not_found("runtime not found"));
        }
        let mut matches = host
            .shared_runtime_resources()
            .await?
            .into_iter()
            .filter(|(owner_key, runtime)| *owner_key != owner && runtime.id == request.resource_id)
            .map(|(_, runtime)| runtime)
            .collect::<Vec<_>>();
        match matches.len() {
            0 => Err(Status::not_found("runtime not found")),
            1 => {
                let runtime = Self::runtime_proto_shared(matches.remove(0))?;
                if request.expected_revision != 0 && request.expected_revision != runtime.revision {
                    return Err(Status::aborted("runtime revision changed"));
                }
                self.providers
                    .authorize_resource(request.context.as_ref(), cap::GAME_RUNTIME_READ)?;
                Ok(Response::new(RuntimeRecordReply {
                    revision: runtime.revision,
                    runtime: Some(runtime),
                }))
            }
            _ => Err(Status::failed_precondition(
                "several shared runtimes use this id; list the catalog instead",
            )),
        }
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
        let extensions = runtime_extension_payloads(&runtime_request)?;
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
            &format!("{:?}", extensions),
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
            runtime_provenance(&caller),
            extensions,
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
            "extensions",
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
            &format!("{:?}", runtime_request.extensions),
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
        let extensions = if mask.contains("extensions") {
            runtime_extension_payloads(&runtime_request)?
        } else {
            current.extensions.clone()
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
            provenance: current.provenance,
            extensions,
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
        // A discovery grant authorizes submitting observations, but it does not authorize
        // reading a content file. Require the independent file grant before resolving game or
        // runtime records, so discovery-only callers cannot probe those namespaces.
        self.providers
            .authorize_resource(Some(context), cap::FILES_READ)?;
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
        // A stable content id can have been selected from another runtime's discovery. Accept
        // that identity as a new owner-scoped binding after validating this caller's game,
        // installation and approved file handle; never read or mutate the foreign binding row.
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
        let shared = self.shared_runtime_read(&caller, request.shared_records.as_ref())?;
        let host = self.library.host().ok_or_else(unavailable)?;
        let owner = mutation_owner(&caller);
        let slot = self.watch_slot(&caller.session_id)?;
        let initial_revision = runtime_catalog_release(&host, &owner, shared).await?;
        if request.after_revision > initial_revision {
            return Err(Status::invalid_argument(
                "runtime watch revision is in the future",
            ));
        }
        let initial = runtime_catalog_records(&host, &owner, shared).await?;
        let shared_recipient = caller
            .namespace
            .as_ref()
            .map(|namespace| namespace.identity().clone());
        let shared_fields = SHARED_RUNTIME_FIELDS
            .iter()
            .map(|field| (*field).to_owned())
            .collect::<BTreeSet<_>>();
        let authorizer = self.shared_authorizer.clone();
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
                for (is_shared, runtime) in &last {
                    if !resource_id.is_empty() && runtime.id != resource_id {
                        continue;
                    }
                    match runtime_event_record(
                        &resource,
                        &caller,
                        &context,
                        *is_shared,
                        runtime.clone(),
                    )
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
                        if shared && !shared_category_allowed(&authorizer, shared_recipient.as_ref(), &shared_fields) {
                            let _ = sender.send(Err(Status::permission_denied(
                                "common runtime catalog access is not granted",
                            ))).await;
                            return;
                        }
                    }
                    _ = poll.tick() => {
                        let revision = match runtime_catalog_release(&host, &owner, shared).await {
                            Ok(revision) => revision,
                            Err(error) => { let _ = sender.send(Err(error)).await; return; }
                        };
                        if revision == last_revision { continue; }
                        let current = match runtime_catalog_records(&host, &owner, shared).await {
                            Ok(records) => records,
                            Err(error) => { let _ = sender.send(Err(error)).await; return; }
                        };
                        let removed = last.iter().any(|old| !current.iter().any(|new| new.0 == old.0 && new.1.id == old.1.id));
                        if removed {
                            if sender.send(Ok(RuntimeRecordEvent { revision, event_type: "reset".into(), runtime: None })).await.is_err() { return; }
                        }
                        for (is_shared, runtime) in &current {
                            let changed = last.iter().find(|old| old.0 == *is_shared && old.1.id == runtime.id).is_none_or(|old| old.1.revision != runtime.revision);
                            if !changed || (!resource_id.is_empty() && runtime.id != resource_id) { continue; }
                            match runtime_event_record(&resource, &caller, &context, *is_shared, runtime.clone()).await {
                                Ok(runtime) => {
                                    if sender.send(Ok(RuntimeRecordEvent { revision, event_type: "upserted".into(), runtime: Some(runtime) })).await.is_err() { return; }
                                }
                                Err(error) => { let _ = sender.send(Err(error)).await; return; }
                            }
                        }
                        if current.iter().all(|(is_shared, runtime)| last.iter().any(|old| old.0 == *is_shared && old.1.id == runtime.id && old.1.revision == runtime.revision)) {
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

async fn runtime_catalog_release(
    host: &std::sync::Arc<dyn crate::plugin_host::library::LibraryHost>,
    owner: &str,
    shared: bool,
) -> Result<u64, Status> {
    if shared {
        host.shared_runtime_catalog_revision().await
    } else {
        host.runtime_catalog_revision(owner.to_owned()).await
    }
}

async fn runtime_catalog_records(
    host: &std::sync::Arc<dyn crate::plugin_host::library::LibraryHost>,
    owner: &str,
    shared: bool,
) -> Result<Vec<(bool, RuntimeResource)>, Status> {
    let mut records = host
        .runtime_resources(owner.to_owned())
        .await?
        .into_iter()
        .map(|runtime| (false, runtime))
        .collect::<Vec<_>>();
    if shared {
        records.extend(
            host.shared_runtime_resources()
                .await?
                .into_iter()
                .filter(|(owner_key, _)| owner_key != owner)
                .map(|(_, runtime)| (true, runtime)),
        );
    }
    records.sort_by(|left, right| {
        left.1
            .id
            .cmp(&right.1.id)
            .then_with(|| left.0.cmp(&right.0))
    });
    Ok(records)
}

async fn runtime_event_record(
    resource: &ResourceApi,
    caller: &ResourceCaller,
    context: &ServiceContext,
    shared: bool,
    runtime: RuntimeResource,
) -> Result<RuntimeRecord, Status> {
    if shared {
        ResourceApi::runtime_proto_shared(runtime)
    } else {
        resource.runtime_proto(caller, context, runtime).await
    }
}

fn shared_category_allowed(
    authorizer: &std::sync::Arc<dyn crate::plugin_host::sharing_policy::Authorizer>,
    recipient: Option<&crate::plugin_host::grants::PluginIdentity>,
    fields: &BTreeSet<String>,
) -> bool {
    recipient.is_some_and(|recipient| {
        authorizer
            .common_category(recipient, CommonDataCategory::RuntimeCatalog, fields)
            .allowed
    })
}
