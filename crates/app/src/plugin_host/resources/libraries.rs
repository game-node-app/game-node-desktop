//! Logical libraries and approved scan roots. Metadata and physical paths stay in the
//! authenticated account namespace, separate from global DRM-free watch configuration.
use super::mutations::{mutation_key, request_hash};
use super::*;
use crate::plugin_host::files::{APPROVAL_TIMEOUT, AffectedGame, RootRequest, RootScope};
use crate::plugin_host::library::PluginScanRoot;
use gamenode_platform::plugin_files::{ApprovedRoot, Confined, Identity, approve_root};
use gamenode_platform::plugin_locations::{Os, SystemFolders};
use gamenode_platform::plugin_storage::{PluginLibraryRoot as Root, PluginLibraryRow as Row};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const MAX_LIBRARIES: usize = gamenode_platform::plugin_storage::MAX_PLUGIN_LIBRARIES;
const MAX_ROOTS: usize = gamenode_platform::plugin_storage::MAX_PLUGIN_LIBRARY_ROOTS;

pub(super) struct Libraries {
    instance: u64,
    cancelled: Arc<AtomicBool>,
    monitors: Mutex<HashMap<String, (ResourceCaller, ServiceContext)>>,
    epochs: Mutex<HashMap<[u8; 32], Arc<AtomicU64>>>,
}

impl Default for Libraries {
    fn default() -> Self {
        static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);
        Self {
            instance: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
            cancelled: Arc::default(),
            monitors: Mutex::default(),
            epochs: Mutex::default(),
        }
    }
}

#[derive(Debug)]
pub(super) enum Edit {
    Create(String, String),
    Update(String, String, String, Vec<String>),
    Remove(String),
    Add(String, String, String, Vec<DeclaredLocation>),
    Root(
        String,
        String,
        String,
        String,
        Vec<DeclaredLocation>,
        Vec<String>,
        bool,
    ),
    RemoveRoot(String, String),
}
impl Edit {
    fn permission(&self) -> &'static str {
        match self {
            Self::Create(..) | Self::Add(..) => cap::LIBRARY_ROOTS_CREATE,
            Self::Update(..) | Self::Root(..) => cap::LIBRARY_ROOTS_UPDATE,
            _ => cap::LIBRARY_ROOTS_DELETE,
        }
    }
    fn library(&self) -> Option<&str> {
        match self {
            Self::Create(..) => None,
            Self::Update(id, ..)
            | Self::Remove(id)
            | Self::Add(id, ..)
            | Self::Root(id, ..)
            | Self::RemoveRoot(id, ..) => Some(id),
        }
    }
}
fn name(value: &str) -> Result<String, Status> {
    crate::plugin_host::saves::check_name("name", value)
}
fn description(value: &str) -> Result<(), Status> {
    if value.len() > 2048 || !crate::plugin_host::notifications::is_plain_line(value) {
        return Err(Status::invalid_argument(
            "description must be plain text, at most 2048 bytes",
        ));
    }
    Ok(())
}
fn id(prefix: &str) -> Result<String, Status> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Status::internal("no randomness"))?;
    Ok(format!(
        "{prefix}{}",
        bytes.iter().map(|v| format!("{v:02x}")).collect::<String>()
    ))
}
fn physical(root: &Root) -> Result<(), Status> {
    Confined::open(
        &ApprovedRoot {
            path: PathBuf::from(&root.path),
            identity: Identity {
                volume: root.volume,
                node: root.node,
            },
        },
        &Default::default(),
    )
    .map_err(|_| Status::failed_precondition("approved root is unavailable or replaced"))?;
    if gamenode_platform::volume::volume_info(Path::new(&root.path)).is_none_or(|v| {
        v.volume_identity_token.is_empty() || v.volume_identity_token != root.volume_token
    }) {
        return Err(Status::failed_precondition(
            "approved volume is unavailable or replaced",
        ));
    }
    Ok(())
}
fn record(row: &Row, plugin: &str, files: &FileAccess, session: &str) -> LibraryRecord {
    LibraryRecord {
        id: row.id.clone(),
        name: row.name.clone(),
        description: row.description.clone(),
        creator_plugin_id: plugin.into(),
        root_ids: row.roots.iter().map(|r| r.id.clone()).collect(),
        revision: row.revision,
        roots: row
            .roots
            .iter()
            .map(|root| LibraryRootRecord {
                id: root.id.clone(),
                display_name: root.name.clone(),
                watching: root.watching,
                availability: if physical(root).is_ok() {
                    InspectionAvailability::Available
                } else {
                    InspectionAvailability::Disconnected
                } as i32,
                approved_root_handle: files.seal_handle(session, &root.handle_id(), &[]),
                discovery_state: if cfg!(windows) {
                    "available"
                } else {
                    "unsupported_platform"
                }
                .into(),
            })
            .collect(),
    }
}
struct RootMonitoring {
    task: tokio::task::JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for RootMonitoring {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.task.abort();
    }
}

impl ResourceApi {
    pub(crate) fn monitor_library_roots(&self) -> impl Drop + Send + use<> {
        let api = self.clone();
        let cancelled = self.libraries.cancelled.clone();
        let task = tokio::spawn(async move {
            loop {
                for context in api.providers.resource_session_contexts() {
                    let running = api
                        .libraries
                        .monitors
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .contains_key(&context.session_id);
                    if !running
                        && let Ok(caller) = api.providers.authorize_resource_session(Some(&context))
                        && caller.namespace.is_some()
                    {
                        if let Err(error) = api.start_library_monitor(&caller, &context).await {
                            tracing::debug!(code = ?error.code(), "library-root monitoring is unavailable");
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        RootMonitoring { task, cancelled }
    }

    fn libraries_epoch(&self, caller: &ResourceCaller) -> Result<Arc<AtomicU64>, Status> {
        let owner = caller
            .namespace
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("third-party account namespace required"))?
            .storage_owner();
        Ok(self
            .libraries
            .epochs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(*owner.key())
            .or_default()
            .clone())
    }
    async fn sync_library_scans(
        &self,
        caller: &ResourceCaller,
        context: &ServiceContext,
    ) -> Result<(), Status> {
        let host = self
            .library
            .host()
            .ok_or_else(|| Status::unavailable("Core library host unavailable"))?;
        let epoch = self.libraries_epoch(caller)?;
        let observed = epoch.load(Ordering::Acquire);
        let rows = self.in_namespace(caller, |n| n.plugin_libraries()).await?;
        let providers = self.providers.clone();
        let context = context.clone();
        let cancelled = self.libraries.cancelled.clone();
        let roots = rows
            .into_iter()
            .filter(|r| !r.removed)
            .flat_map(|r| r.roots)
            .filter(|r| r.watching)
            .map(|root| {
                let (epoch, providers, context, cancelled) = (
                    epoch.clone(),
                    providers.clone(),
                    context.clone(),
                    cancelled.clone(),
                );
                PluginScanRoot {
                    root,
                    valid: Arc::new(move || {
                        if cancelled.load(Ordering::Acquire)
                            || epoch.load(Ordering::Acquire) != observed
                        {
                            return Err(Status::cancelled("library roots changed"));
                        }
                        providers.authorize_resource_session(Some(&context))?;
                        Ok(())
                    }),
                }
            })
            .collect();
        if epoch.load(Ordering::Acquire) != observed {
            return Ok(());
        }
        host.set_plugin_roots(
            format!("logical:{}:{}", self.libraries.instance, caller.session_id),
            roots,
        )
    }
    async fn start_library_monitor(
        &self,
        caller: &ResourceCaller,
        context: &ServiceContext,
    ) -> Result<(), Status> {
        // Without a connected Core host, metadata remains independently readable.
        if self
            .library
            .host()
            .is_none_or(|host| !host.plugin_roots_supported())
        {
            return Ok(());
        }
        let session = caller.session_id.clone();
        let is_new = {
            let mut monitors = self
                .libraries
                .monitors
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if !monitors.contains_key(&session) && monitors.len() >= 64 {
                return Err(Status::resource_exhausted(
                    "Core root monitor limit reached",
                ));
            }
            monitors
                .insert(session.clone(), (caller.clone(), context.clone()))
                .is_none()
        };
        if let Err(error) = self.sync_library_scans(caller, context).await {
            if is_new {
                self.libraries
                    .monitors
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&session);
            }
            return Err(error);
        }
        if !is_new {
            return Ok(());
        }
        let (api, context) = (self.clone(), context.clone());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if api.libraries.cancelled.load(Ordering::Acquire) {
                    break;
                }
                let fresh = match api.providers.authorize_resource_session(Some(&context)) {
                    Ok(fresh) => fresh,
                    Err(_) => break,
                };
                if api.sync_library_scans(&fresh, &context).await.is_err() {
                    break;
                }
            }
            if let Some(host) = api.library.host() {
                let _ = host.set_plugin_roots(
                    format!("logical:{}:{}", api.libraries.instance, session),
                    vec![],
                );
            }
            api.libraries
                .monitors
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&session);
        });
        Ok(())
    }
    pub(super) async fn list_libraries(
        &self,
        request: Request<DomainListRequest>,
    ) -> Result<Response<LibrariesListReply>, Status> {
        let request = request.into_inner();
        let page = request.page.unwrap_or_default();
        let caller = self
            .providers
            .authorize_resource(page.context.as_ref(), cap::LIBRARY_ROOTS_READ)?;
        no_common_data(page.common_data_categories.as_ref())?;
        if !request.kind.is_empty() && request.kind != "libraries" {
            return Err(Status::invalid_argument("unsupported library kind"));
        }
        let plugin = caller
            .namespace
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("account namespace required"))?
            .identity()
            .plugin_id
            .clone();
        if !request.owner_filter.is_empty() && request.owner_filter != plugin {
            return Err(Status::permission_denied(
                "foreign library scope is not granted",
            ));
        }
        let (files, session) = (self.files.clone(), caller.session_id.clone());
        let cursor = page.cursor;
        let grant = caller.grant_revision;
        let max = if page.page_size == 0 {
            50
        } else {
            page.page_size.min(MAX_PAGE_SIZE as u32)
        } as usize;
        let result = self
            .in_namespace(&caller, move |n| {
                let revision = n.revision()?;
                let binding = format!("logical-libraries:{revision}:{grant}");
                let after = if cursor.is_empty() {
                    String::new()
                } else {
                    files
                        .open_cursor(&session, &binding, &cursor)
                        .ok_or_else(|| {
                            Status::failed_precondition("library cursor changed; list again")
                        })?
                };
                let mut rows = n
                    .plugin_libraries()?
                    .into_iter()
                    .filter(|r| !r.removed && r.id > after)
                    .collect::<Vec<_>>();
                let more = rows.len() > max;
                rows.truncate(max);
                let next_cursor = if more {
                    files.seal_cursor(&session, &binding, &rows.last().unwrap().id)
                } else {
                    String::new()
                };
                Ok(LibrariesListReply {
                    libraries: rows
                        .iter()
                        .map(|r| record(r, &plugin, &files, &session))
                        .collect(),
                    next_cursor,
                    revision,
                })
            })
            .await?;
        self.providers
            .authorize_resource(page.context.as_ref(), cap::LIBRARY_ROOTS_READ)?;
        if let Some(context) = page.context {
            self.start_library_monitor(&caller, &context).await?;
        }
        Ok(Response::new(result))
    }
    pub(super) async fn get_library(
        &self,
        request: Request<DomainGetRequest>,
    ) -> Result<Response<LibraryRecordReply>, Status> {
        let request = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::LIBRARY_ROOTS_READ)?;
        no_common_data(request.shared_records.as_ref())?;
        let plugin = caller
            .namespace
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("account namespace required"))?
            .identity()
            .plugin_id
            .clone();
        let (files, session) = (self.files.clone(), caller.session_id.clone());
        let result = self
            .in_namespace(&caller, move |n| {
                let row = n
                    .plugin_libraries()?
                    .into_iter()
                    .find(|r| r.id == request.resource_id && !r.removed)
                    .ok_or_else(|| Status::not_found("library not found"))?;
                if request.expected_revision != 0 && request.expected_revision != row.revision {
                    return Err(Status::aborted("library revision changed"));
                }
                Ok(LibraryRecordReply {
                    revision: row.revision,
                    library: Some(record(&row, &plugin, &files, &session)),
                })
            })
            .await?;
        self.providers
            .authorize_resource(request.context.as_ref(), cap::LIBRARY_ROOTS_READ)?;
        Ok(Response::new(result))
    }
    async fn choose_library_root(
        &self,
        caller: &ResourceCaller,
        library: &str,
        handle: &str,
        display: &str,
        suggestions: &[DeclaredLocation],
    ) -> Result<Root, Status> {
        if self
            .library
            .host()
            .is_none_or(|host| !host.plugin_roots_supported())
        {
            return Err(Status::failed_precondition(
                "Core root discovery is not connected",
            ));
        }
        if !handle.is_empty() {
            let (approved_id, path) = self
                .files
                .open_handle(&caller.session_id, handle)
                .filter(|(_, p)| p.is_empty())
                .ok_or_else(|| Status::not_found("library-root handle not found"))?;
            let _ = path;
            let root = self
                .in_namespace(caller, move |n| {
                    n.plugin_libraries().map(|rows| {
                        rows.into_iter()
                            .filter(|r| !r.removed)
                            .flat_map(|r| r.roots)
                            .find(|r| r.handle_id() == approved_id)
                    })
                })
                .await?
                .ok_or_else(|| Status::not_found("library-root handle not found"))?;
            let mut root = tokio::task::spawn_blocking(move || {
                physical(&root)?;
                Ok::<_, Status>(root)
            })
            .await
            .map_err(|_| Status::internal("root validation failed"))??;
            root.id = id("lroot-")?;
            root.name = name(display)?;
            root.watching = true;
            return Ok(root);
        }
        if suggestions.len() > 8 {
            return Err(Status::invalid_argument("at most eight root suggestions"));
        }
        let mut parsed = Vec::new();
        for suggestion in suggestions {
            if !matches!(suggestion.kind.as_str(), "" | "library") {
                return Err(Status::invalid_argument(
                    "root suggestion kind must be library",
                ));
            }
            description(&suggestion.description)?;
            parsed.push((
                gamenode_platform::plugin_locations::parse(&suggestion.location)
                    .map_err(|_| Status::invalid_argument("invalid declared root"))?,
                suggestion.description.clone(),
            ));
        }
        let files = self.files.clone();
        let reserved = files.reserved.clone();
        let declared = tokio::task::spawn_blocking(move || {
            let mut folders = Vec::new();
            for (location, description) in parsed {
                if let Some(path) = location.expand(Os::current(), &SystemFolders)
                    && !reserved.is_reserved(&path)
                    && !folders
                        .iter()
                        .any(|f: &crate::plugin_host::files::DeclaredFolder| f.path == path)
                {
                    let exists = std::fs::metadata(&path).is_ok_and(|m| m.is_dir());
                    folders.push(crate::plugin_host::files::DeclaredFolder {
                        path,
                        description,
                        exists,
                    });
                }
            }
            folders
        })
        .await
        .map_err(|_| Status::internal("root suggestions failed"))?;
        let identity = caller
            .namespace
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("account namespace required"))?
            .identity()
            .clone();
        let _waiting = files
            .begin_approval(*caller.namespace.as_ref().unwrap().storage_owner().key())
            .ok_or_else(|| Status::aborted("a folder approval is already pending"))?;
        let approver = files
            .approver()
            .ok_or_else(|| Status::failed_precondition("folder approval dialog unavailable"))?;
        let display = name(display)?;
        let preselected = declared.iter().find(|f| f.exists).map(|f| f.path.clone());
        let originals = declared.clone();
        let chosen = tokio::time::timeout(
            APPROVAL_TIMEOUT,
            approver.choose_root(RootRequest {
                plugin_id: identity.plugin_id.clone(),
                publisher_fingerprint: identity.publisher_fingerprint.clone(),
                installation_name: library.into(),
                kind: "library".into(),
                operations: vec!["read", "plugin_discovery", "core_executable_discovery"],
                previously_approved: None,
                suggested: None,
                preselected,
                declared,
            }),
        )
        .await
        .map_err(|_| Status::deadline_exceeded("folder approval timed out"))??
        .ok_or_else(|| Status::permission_denied("folder selection declined"))?;
        let reserved = files.reserved.clone();
        let feed = self.library.clone();
        let (approved, declared, token, affected) = tokio::task::spawn_blocking(move || {
            let approved = approve_root(&chosen, &reserved)
                .map_err(|_| Status::failed_precondition("folder cannot be approved"))?;
            let declared = originals
                .iter()
                .any(|f| std::fs::canonicalize(&f.path).ok().as_ref() == Some(&approved.path));
            let token = gamenode_platform::volume::volume_info(&approved.path)
                .map(|v| v.volume_identity_token)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| Status::failed_precondition("volume identity unknown"))?;
            let affected = files
                .overlapping(&feed, &approved.path)
                .into_iter()
                .map(|entry| AffectedGame {
                    name: entry.name,
                    game_id: entry.game_id,
                })
                .collect();
            Ok::<_, Status>((approved, declared, token, affected))
        })
        .await
        .map_err(|_| Status::internal("folder validation failed"))??;
        if !tokio::time::timeout(
            APPROVAL_TIMEOUT,
            approver.confirm(RootScope {
                plugin_id: identity.plugin_id,
                publisher_fingerprint: identity.publisher_fingerprint,
                installation_name: library.into(),
                kind: "library".into(),
                operations: vec!["read", "plugin_discovery", "core_executable_discovery"],
                root: approved.path.clone(),
                declared_by_plugin: declared,
                affected,
            }),
        )
        .await
        .map_err(|_| Status::deadline_exceeded("folder approval timed out"))??
        {
            return Err(Status::permission_denied("root approval declined"));
        }
        Ok(Root {
            id: id("lroot-")?,
            name: display,
            path: approved.path.to_string_lossy().into_owned(),
            volume: approved.identity.volume,
            node: approved.identity.node,
            volume_token: token,
            declared,
            watching: true,
        })
    }
    pub(super) async fn edit_library(
        &self,
        mutation: Option<MutationContext>,
        edit: Edit,
    ) -> Result<MutationReply, Status> {
        let mutation = mutation.ok_or_else(|| Status::invalid_argument("mutation required"))?;
        let context = mutation
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), edit.permission())?;
        let epoch = self.libraries_epoch(&caller)?;
        let key = mutation_key(&mutation)?.to_owned();
        let hash = request_hash(&[
            "logical-library",
            &format!("{edit:?}"),
            &mutation.expected_revision.to_string(),
        ]);
        {
            let _lock = self.mutations.lock.lock().await;
            if let Some(replay) = self.mutations.replay(&caller, &key, hash)? {
                return Ok(replay);
            }
        }
        let library_id = edit.library().map(str::to_owned);
        let mut row = self
            .in_namespace(&caller, move |n| {
                let rows = n.plugin_libraries()?;
                match library_id {
                    Some(id) => rows
                        .into_iter()
                        .find(|r| r.id == id && !r.removed)
                        .ok_or_else(|| Status::not_found("library not found")),
                    None => {
                        if rows.len() >= MAX_LIBRARIES {
                            return Err(Status::resource_exhausted("library record limit reached"));
                        }
                        Ok(Row {
                            id: id("library-")?,
                            name: String::new(),
                            description: String::new(),
                            roots: vec![],
                            removed: false,
                            revision: 0,
                        })
                    }
                }
            })
            .await?;
        if row.revision != mutation.expected_revision {
            return Ok(super::mutations::reply(
                &row.id,
                row.revision,
                MutationOutcome::RevisionConflict,
            ));
        }
        let permission = edit.permission();
        let mut validate_roots = Vec::new();
        let mut result_id = row.id.clone();
        match edit {
            Edit::Create(label, text) => {
                row.name = name(&label)?;
                description(&text)?;
                row.description = text;
            }
            Edit::Update(_, label, text, mask) => {
                if mask.is_empty()
                    || mask
                        .iter()
                        .any(|f| !matches!(f.as_str(), "name" | "description"))
                {
                    return Err(Status::invalid_argument(
                        "field_mask must select name or description",
                    ));
                }
                if mask.iter().any(|f| f == "name") {
                    row.name = name(&label)?;
                }
                if mask.iter().any(|f| f == "description") {
                    description(&text)?;
                    row.description = text;
                }
            }
            Edit::Remove(_) => {
                row.removed = true;
                row.roots.clear();
            }
            Edit::Add(_, handle, label, suggestions) => {
                let total = self
                    .in_namespace(&caller, |n| {
                        n.plugin_libraries()
                            .map(|rows| rows.iter().map(|r| r.roots.len()).sum::<usize>())
                    })
                    .await?;
                if total >= MAX_ROOTS {
                    return Err(Status::resource_exhausted("root limit reached"));
                }
                let root = self
                    .choose_library_root(&caller, &row.name, &handle, &label, &suggestions)
                    .await?;
                result_id = root.id.clone();
                validate_roots.push(root.id.clone());
                row.roots.push(root);
            }
            Edit::Root(_, root_id, handle, label, suggestions, mask, watching) => {
                if mask.is_empty()
                    || mask
                        .iter()
                        .any(|f| !matches!(f.as_str(), "display_name" | "watching" | "folder"))
                {
                    return Err(Status::invalid_argument(
                        "field_mask must select display_name, watching or folder",
                    ));
                }
                let root = row
                    .roots
                    .iter_mut()
                    .find(|r| r.id == root_id)
                    .ok_or_else(|| Status::not_found("root not found"))?;
                if mask.iter().any(|f| f == "folder") {
                    let mut replacement = self
                        .choose_library_root(&caller, &row.name, &handle, &root.name, &suggestions)
                        .await?;
                    replacement.id = root.id.clone();
                    replacement.watching = root.watching;
                    validate_roots.push(root.id.clone());
                    *root = replacement;
                }
                if mask.iter().any(|f| f == "display_name") {
                    root.name = name(&label)?;
                }
                if mask.iter().any(|f| f == "watching") {
                    root.watching = watching;
                }
                result_id = root.id.clone();
            }
            Edit::RemoveRoot(_, root_id) => {
                let before = row.roots.len();
                row.roots.retain(|r| r.id != root_id);
                if before == row.roots.len() {
                    return Err(Status::not_found("root not found"));
                }
                result_id = root_id;
            }
        }
        let _lock = self.mutations.lock.lock().await;
        if let Some(replay) = self.mutations.replay(&caller, &key, hash)? {
            return Ok(replay);
        }
        let guard = self.guard(context.clone(), permission, caller.grant_revision);
        let expected = mutation.expected_revision;
        let invalidate = epoch.clone();
        let revision = self
            .in_namespace(&caller, move |n| {
                // Revalidate newly approved/reused roots after user wait, including volume identity.
                for root in &row.roots {
                    if validate_roots.contains(&root.id) {
                        physical(root)?;
                    }
                }
                guard()?;
                invalidate.fetch_add(1, Ordering::AcqRel);
                n.put_plugin_library(row, expected, &guard)
            })
            .await?;
        let owner = caller.namespace.as_ref().unwrap().storage_owner();
        let contexts: Vec<_> = self
            .libraries
            .monitors
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|(c, _)| {
                c.namespace
                    .as_ref()
                    .is_some_and(|n| n.storage_owner().key() == owner.key())
            })
            .map(|(_, context)| context.clone())
            .collect();
        for context in contexts {
            if let Ok(fresh) = self.providers.authorize_resource_session(Some(&context)) {
                self.sync_library_scans(&fresh, &context).await?;
            }
        }
        let reply = super::mutations::reply(&result_id, revision, MutationOutcome::Applied);
        self.mutations.remember(&caller, &key, hash, &reply);
        drop(_lock);
        // Metadata-only operations remain usable when Core discovery is not connected.
        // Root work is dispatched only through the authenticated host contribution.
        self.start_library_monitor(&caller, &context).await?;
        Ok(reply)
    }
    pub(super) async fn watch_libraries(
        &self,
        request: Request<DomainWatchRequest>,
    ) -> Result<Response<BoxStream<LibraryRecordEvent>>, Status> {
        let request = request.into_inner();
        let context = request.context.clone();
        let caller = self
            .providers
            .authorize_resource(context.as_ref(), cap::LIBRARY_ROOTS_READ)?;
        no_common_data(request.shared_records.as_ref())?;
        let current = self.in_namespace(&caller, |n| n.revision()).await?;
        if request.after_revision != 0 && request.after_revision != current {
            return Err(Status::failed_precondition(
                "library history is not retained; list and watch from its revision",
            ));
        }
        let plugin = caller
            .namespace
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("account namespace required"))?
            .identity()
            .plugin_id
            .clone();
        let slot = self.watch_slot(&caller.session_id)?;
        let (tx, rx) = mpsc::channel(WATCH_QUEUE);
        let api = self.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let mut last = request.after_revision;
            let mut previous: HashMap<String, Vec<i32>> = HashMap::new();
            let mut initial = true;
            loop {
                let result = async {
                    api.providers
                        .authorize_resource(context.as_ref(), cap::LIBRARY_ROOTS_READ)?;
                    let (files, session, plugin, filter) = (
                        api.files.clone(),
                        caller.session_id.clone(),
                        plugin.clone(),
                        request.resource_id.clone(),
                    );
                    let old = previous.clone();
                    let (revision, events, states) = api
                        .in_namespace(&caller, move |n| {
                            let revision = n.revision()?;
                            let mut rows = n.plugin_libraries()?;
                            rows.sort_by_key(|r| r.revision);
                            let mut events = Vec::new();
                            let mut states = HashMap::new();
                            for r in rows
                                .into_iter()
                                .filter(|r| filter.is_empty() || r.id == filter)
                            {
                                let projected = record(&r, &plugin, &files, &session);
                                let availability = projected
                                    .roots
                                    .iter()
                                    .map(|r| r.availability)
                                    .collect::<Vec<_>>();
                                let availability_changed = !initial
                                    && old.get(&r.id).is_some_and(|prior| prior != &availability);
                                if r.revision > last || availability_changed {
                                    events.push(LibraryRecordEvent {
                                        revision,
                                        event_type: if r.removed {
                                            "removed"
                                        } else if r.revision > last {
                                            "updated"
                                        } else {
                                            "availability_changed"
                                        }
                                        .into(),
                                        library: Some(projected),
                                    });
                                }
                                states.insert(r.id, availability);
                            }
                            Ok((revision, events, states))
                        })
                        .await?;
                    api.providers
                        .authorize_resource(context.as_ref(), cap::LIBRARY_ROOTS_READ)?;
                    for event in events {
                        tx.try_send(Ok(event)).map_err(|_| {
                            Status::resource_exhausted("library watch fell behind; list again")
                        })?;
                    }
                    last = revision;
                    previous = states;
                    initial = false;
                    Ok::<_, Status>(())
                }
                .await;
                if let Err(error) = result {
                    let _ = tx.send(Err(error)).await;
                    return;
                }
                tokio::select! {_=tx.closed()=>return,_=tokio::time::sleep(Duration::from_secs(1))=>{}}
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}
