//! Approved-profile backups. Paths and archive selectors are resolved by Core, never plugins.
use super::files::{open_approval_inspect, root_of};
use super::mutations::{mutation_key, request_hash};
use super::*;
use crate::plugin_host::intents::{Binding, ConfirmationRequest, Operation, Verdict};
use crate::plugin_host::jobs::{Fence, JobOwner, Outcome, work};
use crate::plugin_host::saves::{self, Directory};
use gamenode_core::drm_free::{self as domain, BackupAuthority, BackupOperation};
use gamenode_platform::plugin_files::{ApprovedRoot, Budget, Confined};
use gamenode_platform::plugin_storage::SaveProfileRow;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const MAX_FILES: usize = 1000;
const MAX_VISITED: usize = 10_000;

#[derive(Clone, PartialEq, Eq)]
struct Folder {
    directory: Directory,
    root: ApprovedRoot,
    volume_token: String,
}
#[derive(Clone, PartialEq, Eq)]
struct Frozen {
    profile: SaveProfileRow,
    folders: Vec<Folder>,
    entry_key: String,
    entry_revision: u64,
    game_name: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Archive {
    destination: String,
    backup: String,
}

struct CancelWorker(Option<Arc<AtomicBool>>);
impl Drop for CancelWorker {
    fn drop(&mut self) {
        if let Some(cancelled) = &self.0 {
            cancelled.store(true, Ordering::Relaxed);
        }
    }
}

async fn decode_report(json: String) -> Result<serde_json::Value, Status> {
    tokio::task::spawn_blocking(move || {
        serde_json::from_str(&json).map_err(|_| Status::unavailable("invalid backup engine report"))
    })
    .await
    .map_err(|_| Status::unavailable("backup report worker failed"))?
}

fn unavailable() -> Status {
    Status::unavailable("backups are unavailable for this context or platform")
}
fn not_found() -> Status {
    Status::not_found("backup profile or snapshot not found")
}
fn local(_: impl std::fmt::Display) -> domain::LocalError {
    domain::LocalError::Failed("backup scope is no longer available".into())
}
fn status(error: domain::LocalError) -> Status {
    match error {
        domain::LocalError::Unsupported => unavailable(),
        domain::LocalError::Cancelled => Status::cancelled("backup cancelled"),
        domain::LocalError::InvalidTarget => Status::failed_precondition("invalid backup target"),
        domain::LocalError::Failed(_) => {
            Status::failed_precondition("backup engine failed; no successful result is claimed")
        }
    }
}
fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
fn identifier(id: &str) -> Result<(), Status> {
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        return Err(Status::invalid_argument(
            "profile_id is required: 1 to 128 bytes",
        ));
    }
    Ok(())
}

impl Folder {
    fn open(
        &self,
        reserved: &gamenode_platform::plugin_files::Reserved,
    ) -> Result<Confined, domain::LocalError> {
        let info = gamenode_platform::volume::volume_info(&self.root.path)
            .ok_or_else(|| local("volume"))?;
        if self.volume_token.is_empty() || info.volume_identity_token != self.volume_token {
            return Err(local("volume changed"));
        }
        let confined = Confined::open(&self.root, reserved).map_err(local)?;
        if !confined
            .stat(&self.directory.path)
            .map_err(local)?
            .directory
        {
            return Err(local("not a folder"));
        }
        Ok(confined)
    }
    fn base(&self) -> PathBuf {
        self.directory
            .path
            .iter()
            .fold(self.root.path.clone(), |p, name| p.join(name))
    }
}

struct Authority {
    api: ResourceApi,
    caller: ResourceCaller,
    context: ServiceContext,
    permission: &'static str,
    frozen: Frozen,
    checkpoint: Fence,
    reviewed_targets: Option<Vec<String>>,
}
impl BackupAuthority for Authority {
    fn check(&self) -> Result<(), domain::LocalError> {
        (self.checkpoint)().map_err(|e| {
            if e.code() == tonic::Code::Cancelled {
                domain::LocalError::Cancelled
            } else {
                local(e)
            }
        })?;
        let current = self
            .api
            .providers
            .authorize_resource(Some(&self.context), self.permission)
            .map_err(local)?;
        if current.grant_revision != self.caller.grant_revision {
            return Err(local("grants changed"));
        }
        let actual = self
            .api
            .storage
            .on_backup_worker(&self.caller, |namespace| {
                self.api.freeze_in(
                    namespace,
                    &self.frozen.profile.id,
                    &self.frozen.entry_key,
                    self.frozen.entry_revision,
                )
            })
            .map_err(local)?;
        if actual != self.frozen {
            return Err(local("profile or approval changed"));
        }
        if self.api.library.revision_of(&self.frozen.entry_key) != Some(self.frozen.entry_revision)
        {
            return Err(local("installation changed"));
        }
        Ok(())
    }
    fn validate_targets(
        &self,
        files: &[String],
        registry: &[String],
    ) -> Result<(), domain::LocalError> {
        self.check()?;
        if !registry.is_empty() || files.len() > MAX_FILES {
            return Err(domain::LocalError::InvalidTarget);
        }
        if let Some(reviewed) = &self.reviewed_targets {
            let mut actual = files.to_vec();
            actual.sort();
            let mut expected = reviewed.clone();
            expected.sort();
            if actual.iter().any(|path| !expected.contains(path)) {
                return Err(domain::LocalError::InvalidTarget);
            }
        }
        for file in files {
            let path = Path::new(file);
            let mut accepted = false;
            for folder in &self.frozen.folders {
                let Ok(relative) = path.strip_prefix(folder.base()) else {
                    continue;
                };
                let components = relative
                    .components()
                    .map(|c| match c {
                        std::path::Component::Normal(name) => name.to_str().map(str::to_owned),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
                    .ok_or(domain::LocalError::InvalidTarget)?;
                if components.is_empty()
                    || !saves::selected(&folder.directory.naming_patterns, &components)
                {
                    continue;
                }
                // Check existing ancestors without following links. A missing leaf can be restored,
                // but an unavailable/replaced approval or a symlink cannot become a destination.
                let confined = folder.open(&self.api.files.reserved)?;
                let mut current = folder.directory.path.clone();
                for (index, component) in components.iter().enumerate() {
                    if !gamenode_platform::plugin_files::valid_component(component) {
                        return Err(domain::LocalError::InvalidTarget);
                    }
                    current.push(component.clone());
                    match confined.stat(&current) {
                        Ok(entry) if index + 1 < components.len() && !entry.directory => {
                            return Err(domain::LocalError::InvalidTarget);
                        }
                        Ok(_) => {}
                        Err(gamenode_platform::plugin_files::FileError::NotFound) => break,
                        Err(error) => return Err(local(error)),
                    }
                }
                accepted = true;
                break;
            }
            if !accepted {
                return Err(domain::LocalError::InvalidTarget);
            }
        }
        Ok(())
    }
}

impl ResourceApi {
    fn freeze_in(
        &self,
        namespace: &mut crate::plugin_host::storage::Namespace<'_>,
        profile_id: &str,
        entry_key: &str,
        entry_revision: u64,
    ) -> Result<Frozen, Status> {
        let profile = namespace.save_profile(profile_id)?.ok_or_else(not_found)?;
        let (_, entry) = self
            .library
            .target(
                &Scope {
                    own_provider: String::new(),
                    read_all: true,
                    provider_filter: None,
                },
                &profile.installation_id,
            )
            .filter(|(key, entry)| key == entry_key && entry.revision == entry_revision)
            .ok_or_else(|| Status::aborted("backup installation changed"))?;
        let game_name = entry.display_name.unwrap_or(entry.name);
        let directories = saves::directories_from_json(&profile.directories);
        if directories.is_empty() {
            return Err(Status::failed_precondition(
                "profile has no approved folders",
            ));
        }
        let mut folders = vec![];
        for directory in directories {
            let opened = open_approval_inspect(
                &self.files,
                &self.library,
                namespace,
                &directory.approval_id,
                directory.path.clone(),
            )?
            .map_err(|_| Status::failed_precondition("approved save folder unavailable"))?;
            if opened.approval.installation_id != profile.installation_id {
                return Err(not_found());
            }
            if !opened
                .confined
                .stat(&directory.path)
                .map_err(|_| not_found())?
                .directory
            {
                return Err(not_found());
            }
            folders.push(Folder {
                root: root_of(&opened.approval),
                volume_token: opened.approval.volume_token,
                directory,
            });
        }
        Ok(Frozen {
            profile,
            folders,
            entry_key: entry_key.into(),
            entry_revision,
            game_name,
        })
    }

    async fn backup_scope(
        &self,
        context: &ServiceContext,
        permission: &'static str,
        profile: &str,
        game: &str,
    ) -> Result<(ResourceCaller, Frozen), Status> {
        let caller = self
            .providers
            .authorize_resource(Some(context), permission)?;
        identifier(profile)?;
        let id = profile.to_owned();
        let row = self
            .in_namespace(&caller, move |n| n.save_profile(&id)?.ok_or_else(not_found))
            .await?;
        let wanted = super::mutations::game_id(game)?;
        if wanted.is_some_and(|id| row.game_id != id.to_string()) {
            return Err(not_found());
        }
        let (entry_key, entry) = self.action_target(&caller, context, "", &row.installation_id)?;
        let api = self.clone();
        let frozen = self
            .in_namespace(&caller, move |n| {
                api.freeze_in(n, &row.id, &entry_key, entry.revision)
            })
            .await?;
        Ok((caller, frozen))
    }

    async fn backup_request(
        &self,
        frozen: &Frozen,
        caller: &ResourceCaller,
        operation: BackupOperation,
        destination: Option<String>,
    ) -> Result<(Arc<dyn domain::BackupEngine>, domain::ScopedBackupRequest), Status> {
        let host = self.library.host().ok_or_else(unavailable)?;
        let mut context = host.backup_context(frozen.entry_key.clone()).await?;
        if !context.engine.scoped_available() {
            return Err(unavailable());
        }
        // Runtime session/resource coordination is not available before tasks 13.2/14.2. Never
        // assume a content exit means the shared emulator stopped.
        if !frozen.profile.runtime_id.is_empty() {
            return Err(Status::unavailable(
                "runtime backup coordination is unavailable",
            ));
        }
        context.target.installation_id =
            hex(Sha256::digest(frozen.profile.installation_id.as_bytes()));
        let owner = hex(caller
            .namespace
            .as_ref()
            .ok_or_else(unavailable)?
            .storage_owner()
            .key());
        let profile = domain::SaveProfile {
            id: frozen.profile.id.clone(),
            installation: frozen.profile.installation_id.clone(),
            revision: frozen.profile.revision,
            title: frozen.profile.id.clone(),
            manifest_hash: frozen.profile.revision.to_string(),
            definition: "{files: {}}".into(),
            files: vec![],
            registry: vec![],
            support: domain::SaveSupport::Supported,
            automatic: Some(false),
        };
        let destination = destination.unwrap_or(context.destination);
        let protected = frozen
            .folders
            .iter()
            .map(Folder::base)
            .chain(std::iter::once(PathBuf::from(&context.target.root)))
            .collect::<Vec<_>>();
        let chosen = destination.clone();
        tokio::task::spawn_blocking(move || {
            gamenode_platform::backups::validate_destination(Path::new(&chosen), &protected)
        })
        .await
        .map_err(|_| unavailable())?
        .map_err(status)?;
        Ok((
            context.engine,
            domain::ScopedBackupRequest {
                owner,
                target: context.target,
                profile,
                job_id: None,
                destination,
                operation,
            },
        ))
    }

    async fn execute_backup(
        &self,
        engine: Arc<dyn domain::BackupEngine>,
        mut request: domain::ScopedBackupRequest,
        authority: Authority,
        cancelled: Arc<AtomicBool>,
    ) -> Result<domain::BackupReport, Status> {
        let slot =
            self.backup_slots.clone().try_acquire_owned().map_err(|_| {
                Status::resource_exhausted("backup workers are busy; try again later")
            })?;
        let mut cancel_on_drop = CancelWorker(Some(cancelled.clone()));
        let result = tokio::task::spawn_blocking(move || {
            let _slot = slot;
            authority.check().map_err(status)?;
            let protected = authority
                .frozen
                .folders
                .iter()
                .map(Folder::base)
                .chain(std::iter::once(PathBuf::from(&request.target.root)))
                .collect::<Vec<_>>();
            gamenode_platform::backups::validate_destination(
                Path::new(&request.destination),
                &protected,
            )
            .map_err(status)?;
            if matches!(request.operation, BackupOperation::Backup) {
                let mut selected = BTreeSet::new();
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut visited = 0;
                for folder in &authority.frozen.folders {
                    let confined = folder.open(&authority.api.files.reserved).map_err(status)?;
                    let budget = Budget {
                        max_entries: MAX_VISITED.saturating_sub(visited),
                        deadline,
                        cancel: &cancelled,
                    };
                    let mut too_many = false;
                    let walk = confined
                        .walk_tree(&folder.directory.path, &budget, &mut |path, entry| {
                            visited += 1;
                            if (authority.checkpoint)().is_err() {
                                cancelled.store(true, Ordering::Relaxed);
                                return;
                            }
                            if !entry.directory
                                && saves::selected(
                                    &folder.directory.naming_patterns,
                                    &path[folder.directory.path.len()..],
                                )
                            {
                                let path = path
                                    .iter()
                                    .fold(folder.root.path.clone(), |p, part| p.join(part));
                                let path = path.to_string_lossy().into_owned();
                                if selected.len() >= MAX_FILES && !selected.contains(&path) {
                                    too_many = true;
                                    return;
                                }
                                selected.insert(path);
                            }
                        })
                        .map_err(|_| Status::failed_precondition("save enumeration failed"))?;
                    if cancelled.load(Ordering::Relaxed) {
                        return Err(Status::cancelled("backup cancelled"));
                    }
                    if too_many || !walk.complete {
                        return Err(Status::resource_exhausted(
                            "save selection exceeded its file, entry or time budget",
                        ));
                    }
                }
                let files = selected.into_iter().collect::<Vec<_>>();
                authority.validate_targets(&files, &[]).map_err(status)?;
                let definitions = files
                    .iter()
                    .map(|p| {
                        (
                            gamenode_platform::backups::glob_literal(p),
                            serde_json::json!({}),
                        )
                    })
                    .collect::<serde_json::Map<String, serde_json::Value>>();
                request.profile.files = files;
                request.profile.definition =
                    serde_json::json!({"files":definitions,"registry":{}}).to_string();
            }
            engine
                .execute_scoped(&request, &cancelled, &authority)
                .map_err(status)
        })
        .await
        .map_err(|_| Status::unavailable("backup worker failed"))?;
        cancel_on_drop.0 = None;
        result
    }

    fn backup_authority(
        &self,
        caller: &ResourceCaller,
        context: &ServiceContext,
        permission: &'static str,
        frozen: &Frozen,
        checkpoint: Fence,
    ) -> Authority {
        Authority {
            api: self.clone(),
            caller: caller.clone(),
            context: context.clone(),
            permission,
            frozen: frozen.clone(),
            checkpoint,
            reviewed_targets: None,
        }
    }

    pub(super) async fn list_backups(
        &self,
        request: Request<BackupsListRequest>,
    ) -> Result<Response<BackupsListReply>, Status> {
        tokio::time::timeout(Duration::from_secs(30), self.list_backup_history(request))
            .await
            .map_err(|_| Status::deadline_exceeded("backup history deadline exceeded"))?
    }

    async fn list_backup_history(
        &self,
        request: Request<BackupsListRequest>,
    ) -> Result<Response<BackupsListReply>, Status> {
        let request = request.into_inner();
        let page = request.page.unwrap_or_default();
        let context = page.context.clone().unwrap_or_default();
        let (caller, frozen) = self
            .backup_scope(
                &context,
                cap::BACKUPS_READ,
                &request.profile_id,
                &request.game_id,
            )
            .await?;
        no_common_data(page.common_data_categories.as_ref())?;
        let (engine, template) = self
            .backup_request(&frozen, &caller, BackupOperation::History, None)
            .await?;
        let id = frozen.profile.id.clone();
        let mut sources = self
            .in_namespace(&caller, move |n| n.backup_sources(&id))
            .await?;
        if !sources.contains(&template.destination) {
            sources.push(template.destination.clone());
        }
        let mut records = vec![];
        let mut history_binding = vec![];
        let check: Fence = Arc::new(|| Ok(()));
        for destination in sources {
            let mut operation = template.clone();
            operation.destination = destination.clone();
            let authority =
                self.backup_authority(&caller, &context, cap::BACKUPS_READ, &frozen, check.clone());
            let report = self
                .execute_backup(
                    engine.clone(),
                    operation,
                    authority,
                    Arc::new(AtomicBool::new(false)),
                )
                .await?;
            if report.state == domain::JobState::Partial {
                return Err(Status::failed_precondition("backup history is incomplete"));
            }
            let json = decode_report(report.json).await?;
            let backups = json
                .get("games")
                .and_then(|g| g.get(&template.profile.title))
                .and_then(|g| g.get("backups"))
                .and_then(serde_json::Value::as_array);
            if let Some(backups) = backups {
                if backups.len() > 100 {
                    return Err(Status::resource_exhausted("backup history limit exceeded"));
                }
                for item in backups {
                    let backup = item
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .filter(|s| !s.is_empty() && s.len() <= 256)
                        .ok_or_else(|| Status::unavailable("invalid snapshot identity"))?;
                    let archive = Archive {
                        destination: destination.clone(),
                        backup: backup.into(),
                    };
                    history_binding.push(format!(
                        "{}:{}",
                        serde_json::to_string(&archive).map_err(|_| unavailable())?,
                        item.get("when")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                    ));
                    let binding =
                        format!("backup:{}:{}", frozen.profile.id, frozen.profile.revision);
                    let backup_id = self.files.seal_cursor(
                        &caller.session_id,
                        &binding,
                        &serde_json::to_string(&archive).map_err(|_| unavailable())?,
                    );
                    let mut preview_request = template.clone();
                    preview_request.destination = destination.clone();
                    preview_request.operation = BackupOperation::RestorePreview {
                        backup: backup.into(),
                    };
                    let authority = self.backup_authority(
                        &caller,
                        &context,
                        cap::BACKUPS_READ,
                        &frozen,
                        check.clone(),
                    );
                    let preview = self
                        .execute_backup(
                            engine.clone(),
                            preview_request,
                            authority,
                            Arc::new(AtomicBool::new(false)),
                        )
                        .await?;
                    let preview_json = decode_report(preview.json).await?;
                    let size_bytes = item
                        .get("size_bytes")
                        .and_then(serde_json::Value::as_u64)
                        .or_else(|| {
                            preview_json
                                .pointer("/overall/totalBytes")
                                .and_then(serde_json::Value::as_u64)
                        })
                        .unwrap_or(0);
                    history_binding.push(format!("{size_bytes}:{:?}", preview.state));
                    records.push(BackupRecord {
                        backup_id,
                        game_id: frozen.profile.game_id.clone(),
                        created_at: item
                            .get("created_at")
                            .and_then(serde_json::Value::as_i64)
                            .or_else(|| {
                                item.get("when")
                                    .and_then(serde_json::Value::as_str)
                                    .and_then(timestamp)
                            })
                            .unwrap_or(0),
                        size_bytes,
                        state: if preview.state == domain::JobState::Partial {
                            "partial"
                        } else {
                            "available"
                        }
                        .into(),
                    });
                }
            }
        }
        let binding = format!(
            "backup-page:{}:{}:{}:{}",
            frozen.profile.id,
            frozen.profile.revision,
            caller.grant_revision,
            hex(request_hash(
                &history_binding
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
            ))
        );
        let after = if page.cursor.is_empty() {
            0
        } else {
            self.files
                .open_cursor(&caller.session_id, &binding, &page.cursor)
                .and_then(|p| p.parse::<usize>().ok())
                .filter(|p| *p <= records.len())
                .ok_or_else(|| Status::failed_precondition("backup history changed; list again"))?
        };
        let count = if page.page_size == 0 {
            DEFAULT_PAGE_SIZE
        } else {
            (page.page_size as usize).min(MAX_PAGE_SIZE)
        };
        let end = (after + count).min(records.len());
        let next_cursor = if end < records.len() {
            self.files
                .seal_cursor(&caller.session_id, &binding, &end.to_string())
        } else {
            String::new()
        };
        let (current, current_scope) = self
            .backup_scope(
                &context,
                cap::BACKUPS_READ,
                &request.profile_id,
                &request.game_id,
            )
            .await?;
        if current.grant_revision != caller.grant_revision || current_scope != frozen {
            return Err(Status::failed_precondition(
                "backup scope changed; list again",
            ));
        }
        Ok(Response::new(BackupsListReply {
            backups: records[after..end].to_vec(),
            next_cursor,
        }))
    }

    pub(super) async fn create_backup(
        &self,
        request: Request<BackupCreateRequest>,
    ) -> Result<Response<JobReply>, Status> {
        let r = request.into_inner();
        if r.reason.len() > 512 || r.reason.chars().any(char::is_control) {
            return Err(Status::invalid_argument("invalid backup reason"));
        }
        self.request_backup(
            r.mutation,
            r.game_id,
            r.profile_id,
            None,
            String::new(),
            r.reason,
        )
        .await
        .map(Response::new)
    }
    pub(super) async fn restore_backup(
        &self,
        request: Request<BackupRestoreRequest>,
    ) -> Result<Response<JobReply>, Status> {
        let r = request.into_inner();
        self.request_backup(
            r.mutation,
            r.game_id,
            r.profile_id,
            Some(r.backup_id),
            r.user_intent_id,
            String::new(),
        )
        .await
        .map(Response::new)
    }

    async fn request_backup(
        &self,
        mutation: Option<MutationContext>,
        game: String,
        profile: String,
        backup: Option<String>,
        intent: String,
        reason: String,
    ) -> Result<JobReply, Status> {
        let mutation = mutation.ok_or_else(|| Status::invalid_argument("mutation required"))?;
        let context = mutation.context.clone().unwrap_or_default();
        let permission = if backup.is_some() {
            cap::BACKUPS_RESTORE
        } else {
            cap::BACKUPS_CREATE
        };
        let (caller, frozen) = self
            .backup_scope(&context, permission, &profile, &game)
            .await?;
        if frozen.profile.revision != mutation.expected_revision {
            return Err(Status::aborted("save profile changed; read it again"));
        }
        let key = mutation_key(&mutation)?.to_owned();
        let hash = request_hash(&[
            "backup",
            permission,
            &game,
            &profile,
            backup.as_deref().unwrap_or(""),
            &intent,
            &reason,
            &mutation.expected_revision.to_string(),
        ]);
        let owner = JobOwner::of(&caller)?;
        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.jobs.replay(&owner, &key, hash)? {
            return Ok(original);
        }
        let archive = if let Some(id) = backup {
            let binding = format!("backup:{}:{}", frozen.profile.id, frozen.profile.revision);
            let value = self
                .files
                .open_cursor(&caller.session_id, &binding, &id)
                .ok_or_else(not_found)?;
            Some(serde_json::from_str::<Archive>(&value).map_err(|_| not_found())?)
        } else {
            None
        };
        let operation = archive
            .as_ref()
            .map(|a| BackupOperation::RestorePreview {
                backup: a.backup.clone(),
            })
            .unwrap_or(BackupOperation::Backup);
        let (engine, request) = self
            .backup_request(
                &frozen,
                &caller,
                operation,
                archive.as_ref().map(|a| a.destination.clone()),
            )
            .await?;
        if archive.is_some() && !self.intents.can_confirm() {
            return Err(Status::failed_precondition(
                "restore confirmation dialog unavailable",
            ));
        }
        let binding = Binding {
            plugin_id: caller
                .namespace
                .as_ref()
                .unwrap()
                .identity()
                .plugin_id
                .clone(),
            publisher_fingerprint: caller
                .namespace
                .as_ref()
                .unwrap()
                .identity()
                .publisher_fingerprint
                .clone(),
            profile_epoch: caller.profile_epoch,
            game_id: frozen.profile.game_id.clone(),
            installation_id: frozen.profile.installation_id.clone(),
            operation: Operation::BackupRestore,
            target_revision: frozen.profile.revision,
            recipe_id: String::new(),
            recipe_revision: 0,
            plan_fingerprint: String::new(),
        };
        // The restore snapshot is not part of a legacy intent binding. Always ask for this exact
        // preview; do not let a launch/install intent authorize a restore.
        if !intent.is_empty() {
            return Err(Status::failed_precondition(
                "restore requires confirmation of its exact snapshot",
            ));
        }
        let pending = if archive.is_some() {
            Some(self.intents.begin_pending(&binding)?)
        } else {
            None
        };
        let api = self.clone();
        let c = caller.clone();
        let cx = context.clone();
        let run = work(move |mut ctx| async move {
            let _pending = pending;
            let cancelled = Arc::new(AtomicBool::new(false));
            let mut request = request;
            request.job_id = Some(ctx.id().into());
            let mut reviewed_targets = None;
            let checkpoint = ctx.checkpoint();
            if let Some(archive) = archive {
                let authority =
                    api.backup_authority(&c, &cx, permission, &frozen, checkpoint.clone());
                let preview = match api
                    .execute_backup(
                        engine.clone(),
                        request.clone(),
                        authority,
                        cancelled.clone(),
                    )
                    .await
                {
                    Ok(p) => p,
                    Err(e) => return Outcome::stopped(&e),
                };
                if preview.state == domain::JobState::Partial {
                    return Outcome::failed("preview_failed", "restore preview failed");
                }
                let json = match decode_report(preview.json).await {
                    Ok(json) => json,
                    Err(e) => return Outcome::stopped(&e),
                };
                let Some(target) = json
                    .get("games")
                    .and_then(|g| g.get(&request.profile.title))
                else {
                    return Outcome::failed("preview_failed", "snapshot unavailable");
                };
                let files = target
                    .get("files")
                    .and_then(serde_json::Value::as_object)
                    .map(|m| m.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                let registry = target
                    .get("registry")
                    .and_then(serde_json::Value::as_object)
                    .map(|m| m.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                let authority =
                    api.backup_authority(&c, &cx, permission, &frozen, checkpoint.clone());
                let validation = tokio::task::spawn_blocking(move || {
                    authority
                        .validate_targets(&files, &registry)
                        .map(|()| files)
                })
                .await;
                let files = match validation {
                    Ok(Ok(files)) => files,
                    _ => {
                        return Outcome::failed(
                            "scope_denied",
                            "restore targets are outside the approved profile",
                        );
                    }
                };
                reviewed_targets = Some(files.clone());
                let mut details = vec![frozen.profile.name.clone(), archive.backup.clone()];
                details.extend(files);
                let confirmation = ConfirmationRequest {
                    plugin_id: binding.plugin_id,
                    publisher_fingerprint: binding.publisher_fingerprint,
                    game_name: frozen.game_name.clone(),
                    operation: Operation::BackupRestore,
                    details,
                };
                let answer = tokio::select! {
                    answer = api.intents.confirm(confirmation) => answer,
                    () = ctx.cancelled() => return Outcome::cancelled("restore cancelled"),
                };
                match answer {
                    Ok(Verdict::Approved { .. }) => {}
                    Ok(Verdict::Declined) => {
                        return Outcome::failed("declined", "restore declined");
                    }
                    Ok(Verdict::TimedOut) => {
                        return Outcome::failed(
                            "confirmation_timeout",
                            "restore confirmation timed out",
                        );
                    }
                    Err(e) => return Outcome::stopped(&e),
                }
                if let Err(e) = ctx.start().and_then(|()| ctx.check()) {
                    return Outcome::stopped(&e);
                }
                request.operation = BackupOperation::Restore {
                    backup: archive.backup,
                };
            } else {
                let id = frozen.profile.id.clone();
                let destination = request.destination.clone();
                let guard = api.guard(cx.clone(), permission, c.grant_revision);
                if let Err(e) = api
                    .in_namespace(&c, move |n| {
                        n.remember_backup_source(&id, &destination, &guard)
                    })
                    .await
                {
                    return Outcome::stopped(&e);
                }
            }
            let mut authority = api.backup_authority(&c, &cx, permission, &frozen, checkpoint);
            authority.reviewed_targets = reviewed_targets;
            match api
                .execute_backup(engine, request, authority, cancelled)
                .await
            {
                Ok(report)
                    if matches!(
                        report.state,
                        domain::JobState::Successful | domain::JobState::Unchanged
                    ) =>
                {
                    Outcome::succeeded("backup_complete", "backup operation completed")
                }
                Ok(_) => Outcome::failed(
                    "partial_failure",
                    "backup operation was incomplete; recovery data is retained",
                ),
                Err(e) => Outcome::stopped(&e),
            }
        });
        let (job, _) = self.start_job(
            &caller,
            context,
            permission,
            &key,
            hash,
            if pending_restore(permission) {
                Some("awaiting restore confirmation")
            } else {
                None
            },
            run,
        )?;
        Ok(job)
    }
}
fn pending_restore(permission: &str) -> bool {
    permission == cap::BACKUPS_RESTORE
}

fn timestamp(text: &str) -> Option<i64> {
    let number = |a, b| text.get(a..b)?.parse::<i64>().ok();
    if text.get(4..5)? != "-"
        || text.get(7..8)? != "-"
        || text.get(10..11)? != "T"
        || text.get(13..14)? != ":"
        || text.get(16..17)? != ":"
    {
        return None;
    }
    let (year, month, day, hour, minute, second) = (
        number(0, 4)?,
        number(5, 7)?,
        number(8, 10)?,
        number(11, 13)?,
        number(14, 16)?,
        number(17, 19)?,
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if !(1..=9999).contains(&year)
        || !(1..=days).contains(&day)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..60).contains(&second)
    {
        return None;
    }
    let mut suffix = text.get(19..)?;
    if let Some(fraction) = suffix.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        suffix = &fraction[digits..];
    }
    let offset = if suffix == "Z" {
        0
    } else {
        if suffix.len() != 6 || !matches!(suffix.get(..1)?, "+" | "-") || suffix.get(3..4)? != ":" {
            return None;
        }
        let h = suffix.get(1..3)?.parse::<i64>().ok()?;
        let m = suffix.get(4..6)?.parse::<i64>().ok()?;
        if h > 23 || m > 59 {
            return None;
        }
        (h * 3600 + m * 60) * if suffix.starts_with('-') { -1 } else { 1 }
    };
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Some(days * 86400 + hour * 3600 + minute * 60 + second - offset)
}
#[cfg(test)]
mod timestamp_tests {
    #[test]
    fn helper_dates_have_explicit_offsets_and_valid_gregorian_components() {
        assert_eq!(super::timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            super::timestamp("2024-02-29T00:00:00.123Z"),
            Some(1709164800)
        );
        assert_eq!(
            super::timestamp("2024-02-28T21:00:00-03:00"),
            Some(1709164800)
        );
        for bad in [
            "2023-02-29T00:00:00Z",
            "2024-02-29T24:00:00Z",
            "2024-02-29T00:00:00",
            "2024-02-29T00:00:00+24:00",
        ] {
            assert_eq!(super::timestamp(bad), None);
        }
    }
}
