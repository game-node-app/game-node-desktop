//! File access of `ResourceHost`: `FilesRequestAccess`, `FilesList`, `FilesRead`, `FilesWrite`
//! and `FilesDelete`.
//!
//! Nothing is reachable at installation: `files.read`, `files.write` and `files.delete` are
//! separate grants the user gives, and even with them a plugin touches only a folder the user
//! approved for that plugin and one installation of one game. A plugin asks
//! (`FilesRequestAccess`), the host asks the user ([`RootApprover`]) to choose the folder and to
//! confirm the complete scope, including every other game that shares it, and the plugin gets
//! opaque handles. Each later call re-derives its authority from the stored approval, the grants
//! the session holds now and the folder as it is on disk, and runs under the plugin's namespace
//! lock, so a revocation cannot slip between the check and the change.
//!
//! The approval is per plugin, installation and kind (`installation`, `save`, `prefix`); the
//! plugin names no path. Write and delete are independent of each other and of read, in the
//! grants and in what the user approved.

use std::path::{Path, PathBuf};

use gamenode_platform::plugin_files::{
    self as confined, ApprovedRoot, Confined, FileError, Identity, Reserved, Target,
};
use gamenode_platform::plugin_locations::{
    self as locations, Location, MAX_PER_KIND, Os, SystemFolders,
};
use gamenode_platform::plugin_storage::{FileApproval, ORIGIN_DECLARED, ORIGIN_USER, operation};
use gamenode_plugin_protocol::{capability as cap, proto::*};
use sha2::{Digest, Sha256};
use tonic::{Request, Response, Status};

use super::mutations::{game_id, mutation_key, reply, request_hash};
use super::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, ResourceApi};
use crate::plugin_host::files::{
    APPROVAL_TIMEOUT, AffectedGame, DEFAULT_READ_BYTES, DeclaredFolder, FileAccess, KINDS,
    MAX_CHUNK_BYTES, RootRequest, RootScope, affected_keys,
};
use crate::plugin_host::library::{CommitGuard, LibraryFeed};
use crate::plugin_host::notifications::is_plain_line;
use crate::plugin_host::provider::ResourceCaller;
use crate::plugin_host::storage::Namespace;

const MAX_INSTALLATION_ID_BYTES: usize = 128;
const MAX_HANDLE_BYTES: usize = 16 * 1024;

fn permission(op: u8) -> &'static str {
    match op {
        operation::READ => cap::FILES_READ,
        operation::WRITE => cap::FILES_WRITE,
        _ => cap::FILES_DELETE,
    }
}

fn op_name(op: u8) -> &'static str {
    match op {
        operation::READ => "read",
        operation::WRITE => "write",
        _ => "delete",
    }
}

/// How a refusal of the file layer reaches the plugin. None of them carries a path.
pub(super) fn file_status(error: FileError) -> Status {
    match error {
        FileError::Denied => Status::permission_denied("the path is outside the approved access"),
        FileError::Access => Status::permission_denied("the operating system refused access"),
        FileError::NotFound => Status::not_found("not found"),
        FileError::NotAFile => Status::failed_precondition("not a regular file"),
        FileError::NotADirectory => Status::failed_precondition("not a directory"),
        FileError::TooLarge => Status::resource_exhausted("too large"),
        FileError::OutOfRange => Status::out_of_range("the offset is beyond the end of the file"),
        FileError::RootChanged => Status::failed_precondition(
            "the approved folder changed or is unavailable; request access again",
        ),
        FileError::Io => Status::unavailable("the file operation failed"),
    }
}

pub(super) fn root_of(approval: &FileApproval) -> ApprovedRoot {
    ApprovedRoot {
        path: PathBuf::from(&approval.root),
        identity: Identity {
            volume: approval.root_volume,
            node: approval.root_node,
        },
    }
}

fn handle_text(handle: &str) -> Result<(), Status> {
    if handle.is_empty() || handle.len() > MAX_HANDLE_BYTES {
        return Err(Status::invalid_argument("a file handle is required"));
    }
    Ok(())
}

/// The operations asked for, as approval bits in a fixed order.
fn parse_operations(requested: &[String]) -> Result<Vec<u8>, Status> {
    let mut ops = Vec::new();
    for name in requested {
        let op = match name.as_str() {
            "read" => operation::READ,
            "write" => operation::WRITE,
            "delete" => operation::DELETE,
            _ => {
                return Err(Status::invalid_argument(
                    "requested_operations holds only \"read\", \"write\" and \"delete\"",
                ));
            }
        };
        if !ops.contains(&op) {
            ops.push(op);
        }
    }
    if ops.is_empty() {
        return Err(Status::invalid_argument("requested_operations is empty"));
    }
    ops.sort_unstable();
    Ok(ops)
}

fn parse_kinds(requested: &[String]) -> Result<Vec<String>, Status> {
    if requested.is_empty() {
        return Ok(vec!["installation".to_owned()]);
    }
    let mut kinds = Vec::new();
    for kind in requested {
        if !KINDS.contains(&kind.as_str()) {
            return Err(Status::invalid_argument(
                "resource_kinds holds only \"installation\", \"save\" and \"prefix\"",
            ));
        }
        if !kinds.contains(kind) {
            kinds.push(kind.clone());
        }
    }
    Ok(kinds)
}

/// The longest description of a declared folder, in bytes.
const MAX_DESCRIPTION_BYTES: usize = 120;

/// What a plugin declared for one kind: well-formed locations with their descriptions, in the
/// plugin's priority order.
type Declared = Vec<(Location, String)>;

/// Checks the declared suggestions of the request and groups them by kind. Only well-formedness
/// is checked here (a refusal is the plugin's mistake and it is told); whether a suggestion
/// applies to this system is decided later, silently.
fn parse_declared(
    declared: &[DeclaredLocation],
) -> Result<std::collections::HashMap<String, Declared>, Status> {
    let mut by_kind: std::collections::HashMap<String, Declared> = std::collections::HashMap::new();
    for entry in declared {
        match entry.kind.as_str() {
            "save" | "prefix" => {}
            "installation" => {
                return Err(Status::invalid_argument(
                    "declared_locations are for \"save\" and \"prefix\"; the host knows where an installation is",
                ));
            }
            _ => {
                return Err(Status::invalid_argument(
                    "declared_locations hold only the kinds \"save\" and \"prefix\"",
                ));
            }
        }
        let location = locations::parse(&entry.location)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let description = entry.description.trim();
        if description.len() > MAX_DESCRIPTION_BYTES {
            return Err(Status::invalid_argument(format!(
                "a description is longer than {MAX_DESCRIPTION_BYTES} bytes"
            )));
        }
        if !is_plain_line(description) {
            return Err(Status::invalid_argument(
                "a description must be plain text on one line: no control characters, bidirectional overrides or invisible separators",
            ));
        }
        let list = by_kind.entry(entry.kind.clone()).or_default();
        if list.len() >= MAX_PER_KIND {
            return Err(Status::invalid_argument(format!(
                "at most {MAX_PER_KIND} declared locations per kind"
            )));
        }
        list.push((location, description.to_owned()));
    }
    Ok(by_kind)
}

fn random_id() -> Result<String, Status> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Status::internal("could not create an approval id"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The folders of `declared` that apply to this system: expanded, without duplicates and
/// without any that lie in a folder GameNode keeps private (those are dropped quietly, like a
/// suggestion for another system). Blocking: it checks which of them exist.
fn declared_folders(declared: &Declared, reserved: &Reserved) -> Vec<DeclaredFolder> {
    let mut folders: Vec<DeclaredFolder> = Vec::new();
    for (location, description) in declared {
        let Some(path) = location.expand(Os::current(), &SystemFolders) else {
            continue;
        };
        if reserved.is_reserved(&path) || folders.iter().any(|folder| folder.path == path) {
            continue;
        }
        let exists = std::fs::metadata(&path).is_ok_and(|meta| meta.is_dir());
        folders.push(DeclaredFolder {
            path,
            description: description.clone(),
            exists,
        });
    }
    folders
}

/// What a handle opens, once every check passed.
pub(super) struct Opened {
    pub(super) approval: FileApproval,
    pub(super) confined: Confined,
    pub(super) path: Vec<String>,
}

/// Re-derives the authority of `handle` for `op`: it must belong to this session, name an
/// approval of this namespace that is not revoked and allows `op`, the set of games sharing the
/// folder must be what the user confirmed, and the folder must still be the one approved.
/// `op` 0 asks only that the approval stand, for operations that read no content (inspection).
pub(super) fn open_handle(
    files: &FileAccess,
    feed: &LibraryFeed,
    namespace: &mut Namespace<'_>,
    session_id: &str,
    handle: &str,
    op: u8,
) -> Result<Opened, Status> {
    // A malformed handle, another session's, a revoked approval's and an unknown one are the
    // same answer.
    let (id, path) = files
        .open_handle(session_id, handle)
        .ok_or_else(|| Status::not_found("file handle not found"))?;
    open_approval(files, feed, namespace, &id, path, op)
}

/// The approval `approval_id` as it stands: this namespace's, not revoked, allowing `op` (0: any),
/// and the set of games sharing its folder still what the user confirmed.
fn standing_approval(
    files: &FileAccess,
    feed: &LibraryFeed,
    namespace: &mut Namespace<'_>,
    approval_id: &str,
    op: u8,
) -> Result<FileApproval, Status> {
    let approval = namespace
        .approval(approval_id)?
        .filter(|approval| approval.revoked_at.is_none())
        .ok_or_else(|| Status::not_found("file handle not found"))?;
    if op != 0 && approval.operations & op == 0 {
        return Err(Status::permission_denied(format!(
            "this folder was not approved for {}",
            op_name(op)
        )));
    }
    let current = affected_keys(&files.overlapping(feed, Path::new(&approval.root)));
    if current != approval.affected {
        return Err(Status::failed_precondition(
            "the games that share this folder changed; request access again",
        ));
    }
    Ok(approval)
}

/// [`open_handle`] for an approval and a path below it that the host already holds (a save
/// profile's folder), rather than a handle a plugin presented.
pub(super) fn open_approval(
    files: &FileAccess,
    feed: &LibraryFeed,
    namespace: &mut Namespace<'_>,
    approval_id: &str,
    path: Vec<String>,
    op: u8,
) -> Result<Opened, Status> {
    let approval = standing_approval(files, feed, namespace, approval_id, op)?;
    check_volume(&approval).map_err(file_status)?;
    let confined = Confined::open(&root_of(&approval), &files.reserved).map_err(file_status)?;
    Ok(Opened {
        approval,
        confined,
        path,
    })
}

/// The standing approval a handle of this session names, without opening its folder.
pub(super) fn approval_of_handle(
    files: &FileAccess,
    feed: &LibraryFeed,
    namespace: &mut Namespace<'_>,
    session_id: &str,
    handle: &str,
) -> Result<FileApproval, Status> {
    let (id, _) = files
        .open_handle(session_id, handle)
        .ok_or_else(|| Status::not_found("file handle not found"))?;
    standing_approval(files, feed, namespace, &id, 0)
}

/// [`open_handle`] for operations that only describe a place (inspection): the approval must
/// stand and the handle be the session's, as always, but a folder that cannot be opened (gone,
/// moved, replaced, off limits) is an answer the caller reports, not a refusal: the outer error is
/// a refusal, the inner one says what the place is like.
pub(super) fn open_handle_inspect(
    files: &FileAccess,
    feed: &LibraryFeed,
    namespace: &mut Namespace<'_>,
    session_id: &str,
    handle: &str,
) -> Result<Result<Opened, FileError>, Status> {
    let (id, path) = files
        .open_handle(session_id, handle)
        .ok_or_else(|| Status::not_found("file handle not found"))?;
    open_approval_inspect(files, feed, namespace, &id, path)
}

/// [`open_handle_inspect`] for an approval and a path the host already holds.
pub(super) fn open_approval_inspect(
    files: &FileAccess,
    feed: &LibraryFeed,
    namespace: &mut Namespace<'_>,
    approval_id: &str,
    path: Vec<String>,
) -> Result<Result<Opened, FileError>, Status> {
    let approval = standing_approval(files, feed, namespace, approval_id, 0)?;
    Ok(check_volume(&approval)
        .and_then(|()| Confined::open(&root_of(&approval), &files.reserved))
        .map(|confined| Opened {
            approval,
            confined,
            path,
        }))
}

impl ResourceApi {
    /// Authorizes every operation in `ops` and returns the caller as of the last check.
    fn authorize_files(
        &self,
        context: &ServiceContext,
        ops: &[u8],
    ) -> Result<ResourceCaller, Status> {
        let mut caller = None;
        for op in ops {
            caller = Some(
                self.providers
                    .authorize_resource(Some(context), permission(*op))?,
            );
        }
        caller.ok_or_else(|| Status::invalid_argument("requested_operations is empty"))
    }

    /// A commit-time check for every operation in `ops`.
    fn guard_files(
        &self,
        context: &ServiceContext,
        ops: &[u8],
        grant_revision: u64,
    ) -> CommitGuard {
        let guards: Vec<CommitGuard> = ops
            .iter()
            .map(|op| self.guard(context.clone(), permission(*op), grant_revision))
            .collect();
        Box::new(move || guards.iter().try_for_each(|guard| guard()))
    }

    pub(super) async fn request_files_access(
        &self,
        request: Request<FilesRequestAccessRequest>,
    ) -> Result<Response<FileAccessReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation.context.clone().unwrap_or_default();
        let ops = parse_operations(&request.requested_operations)?;
        let caller = self.authorize_files(&context, &ops)?;
        mutation_key(mutation)?;
        let kinds = parse_kinds(&request.resource_kinds)?;
        let mut declared = parse_declared(&request.declared_locations)?;
        if request.installation_id.is_empty() {
            return Err(Status::invalid_argument("installation_id is required"));
        }
        if request.installation_id.len() > MAX_INSTALLATION_ID_BYTES {
            return Err(Status::invalid_argument("installation_id is too long"));
        }
        let expected_game = game_id(&request.game_id)?;
        let (entry_key, entry) =
            self.own_entry(&caller, &request.installation_id, expected_game)?;
        let name = entry
            .display_name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| entry.name.clone());

        let mut handles = Vec::new();
        for kind in kinds {
            let approval = self
                .approval_for(
                    &caller,
                    &context,
                    &ops,
                    &entry_key,
                    &entry.installation_id,
                    &name,
                    &kind,
                    entry.game_id,
                    declared.remove(&kind).unwrap_or_default(),
                )
                .await?;
            handles.push(FileHandle {
                handle: self
                    .files
                    .seal_handle(&caller.session_id, &approval.id, &[]),
                display_name: format!("{name} ({kind})"),
                resource_kind: kind,
                writable: approval.operations & operation::WRITE != 0,
            });
        }
        // The call may have waited for the user; its grants may have changed meanwhile.
        self.authorize_files(&context, &ops)?;
        Ok(Response::new(FileAccessReply { handles }))
    }

    /// The approval that covers `ops` for this installation and kind: the stored one if it is
    /// still complete and valid, otherwise a new one the user makes.
    #[allow(clippy::too_many_arguments)]
    async fn approval_for(
        &self,
        caller: &ResourceCaller,
        context: &ServiceContext,
        ops: &[u8],
        entry_key: &str,
        installation_id: &str,
        installation_name: &str,
        kind: &str,
        game: Option<u64>,
        declared: Declared,
    ) -> Result<FileApproval, Status> {
        let wanted = ops.iter().fold(0, |bits, op| bits | op);
        let files = self.files.clone();
        let feed = self.library.clone();
        let (kind_owned, installation_owned) = (kind.to_owned(), installation_id.to_owned());
        let reusable = self
            .in_namespace(caller, {
                let (files, feed) = (files.clone(), feed.clone());
                move |namespace| {
                    let Some(approval) =
                        namespace.approval_for(&installation_owned, &kind_owned)?
                    else {
                        return Ok(None);
                    };
                    let complete = approval.revoked_at.is_none()
                        && approval.operations & wanted == wanted
                        && affected_keys(&files.overlapping(&feed, Path::new(&approval.root)))
                            == approval.affected
                        && check_volume(&approval).is_ok()
                        && Confined::open(&root_of(&approval), &files.reserved).is_ok();
                    Ok(complete.then_some(approval))
                }
            })
            .await?;
        if let Some(approval) = reusable {
            return Ok(approval);
        }

        let approver = files.approver().ok_or_else(|| {
            Status::failed_precondition("file access approval is not available in this build")
        })?;
        let resolver = caller.namespace.as_ref().ok_or_else(|| {
            Status::failed_precondition("official providers use trusted host adapters")
        })?;
        let identity = resolver.identity().clone();
        let _waiting = files
            .begin_approval(*resolver.storage_owner().key())
            .ok_or_else(|| Status::aborted("a file approval is already waiting for the user"))?;
        let names: Vec<&'static str> = ops.iter().map(|op| op_name(*op)).collect();

        // The plugin's suggestions are expanded and checked for existence off the async thread.
        // Nothing is listed or read, and none of it goes back to the plugin.
        let host_suggestion = (kind == "installation")
            .then(|| feed.folder_of(entry_key).map(PathBuf::from))
            .flatten();
        let reserved_files = files.clone();
        let declared_folders = tokio::task::spawn_blocking(move || {
            declared_folders(&declared, &reserved_files.reserved)
        })
        .await
        .map_err(|_| Status::internal("the suggestions could not be checked"))?;
        let preselected = host_suggestion.clone().or_else(|| {
            declared_folders
                .iter()
                .find(|folder| folder.exists)
                .map(|folder| folder.path.clone())
        });

        let chosen = tokio::time::timeout(
            APPROVAL_TIMEOUT,
            approver.choose_root(RootRequest {
                plugin_id: identity.plugin_id.clone(),
                publisher_fingerprint: identity.publisher_fingerprint.clone(),
                installation_name: installation_name.to_owned(),
                kind: kind.to_owned(),
                operations: names.clone(),
                suggested: host_suggestion,
                declared: declared_folders.clone(),
                preselected,
            }),
        )
        .await
        .map_err(|_| Status::deadline_exceeded("the user did not answer in time"))??
        .ok_or_else(|| Status::permission_denied("the user declined to approve a folder"))?;

        // The chosen folder is validated, and the games sharing it found, off the async thread.
        let (root, overlapping, declared_by_plugin) = {
            let (files, feed) = (files.clone(), feed.clone());
            tokio::task::spawn_blocking(move || {
                let root = confined::approve_root(&chosen, &files.reserved).map_err(|error| {
                    match error {
                        FileError::Denied => Status::failed_precondition(
                            "that folder cannot be approved: it is, or is inside, a folder GameNode keeps private",
                        ),
                        FileError::NotFound | FileError::NotADirectory => {
                            Status::failed_precondition("the chosen folder does not exist")
                        }
                        other => file_status(other),
                    }
                })?;
                let overlapping = files.overlapping(&feed, &root.path);
                // The folder counts as the plugin's suggestion only if it is the same folder as
                // one it declared, by canonical path, and not the one the host suggested.
                let declared_by_plugin = declared_folders.iter().any(|folder| {
                    folder.exists
                        && std::fs::canonicalize(&folder.path).is_ok_and(|path| path == root.path)
                });
                Ok::<_, Status>((root, overlapping, declared_by_plugin))
            })
            .await
            .map_err(|_| Status::internal("the folder could not be checked"))??
        };

        let confirmed = tokio::time::timeout(
            APPROVAL_TIMEOUT,
            approver.confirm(RootScope {
                plugin_id: identity.plugin_id.clone(),
                publisher_fingerprint: identity.publisher_fingerprint.clone(),
                installation_name: installation_name.to_owned(),
                kind: kind.to_owned(),
                operations: names,
                root: root.path.clone(),
                declared_by_plugin,
                affected: overlapping
                    .iter()
                    .map(|entry| AffectedGame {
                        name: entry.name.clone(),
                        game_id: entry.game_id,
                    })
                    .collect(),
            }),
        )
        .await
        .map_err(|_| Status::deadline_exceeded("the user did not answer in time"))??;
        if !confirmed {
            return Err(Status::permission_denied("the user declined the access"));
        }

        // The user took their time: the grants may have changed, so check them again.
        let fresh = self.authorize_files(context, ops)?;
        let guard = self.guard_files(context, ops, fresh.grant_revision);
        let approval = FileApproval {
            id: random_id()?,
            installation_id: installation_id.to_owned(),
            game_id: game.map(|game| game.to_string()).unwrap_or_default(),
            kind: kind.to_owned(),
            root: root
                .path
                .to_str()
                .ok_or_else(|| Status::failed_precondition("that folder cannot be approved"))?
                .to_owned(),
            volume_token: String::new(),
            root_volume: root.identity.volume,
            root_node: root.identity.node,
            operations: wanted,
            affected: affected_keys(&overlapping),
            approved_at: 0,
            revoked_at: None,
            origin: if declared_by_plugin {
                ORIGIN_DECLARED
            } else {
                ORIGIN_USER
            }
            .to_owned(),
        };
        self.in_namespace(&fresh, move |namespace| {
            let token = gamenode_platform::volume::volume_info(Path::new(&approval.root))
                .map(|info| info.volume_identity_token)
                .filter(|token| !token.is_empty())
                .ok_or_else(|| {
                    Status::failed_precondition(
                        "volume identity is unknown; folder cannot be approved",
                    )
                })?;
            let approval = FileApproval {
                volume_token: token,
                approved_at: namespace.now(),
                ..approval
            };
            namespace.save_approval(&approval, &guard)?;
            Ok(approval)
        })
        .await
    }

    pub(super) async fn list_files(
        &self,
        request: Request<FilesListRequest>,
    ) -> Result<Response<FilesListReply>, Status> {
        let request = request.into_inner();
        let context = request.context.clone().unwrap_or_default();
        let caller = self.authorize_files(&context, &[operation::READ])?;
        handle_text(&request.directory_handle)?;
        let limit = match request.page_size as usize {
            0 => DEFAULT_PAGE_SIZE,
            size => size.min(MAX_PAGE_SIZE),
        };
        let (files, feed) = (self.files.clone(), self.library.clone());
        let session = caller.session_id.clone();
        let reply = self
            .in_namespace(&caller, move |namespace| {
                let opened = open_handle(
                    &files,
                    &feed,
                    namespace,
                    &session,
                    &request.directory_handle,
                    operation::READ,
                )?;
                let after = if request.cursor.is_empty() {
                    None
                } else {
                    Some(
                        files
                            .open_cursor(&session, &request.directory_handle, &request.cursor)
                            .ok_or_else(|| Status::invalid_argument("invalid files cursor"))?,
                    )
                };
                let (entries, more) = opened
                    .confined
                    .list(&opened.path, after.as_deref(), limit)
                    .map_err(file_status)?;
                let next_cursor = match entries.last() {
                    Some(last) if more => {
                        files.seal_cursor(&session, &request.directory_handle, &last.name)
                    }
                    _ => String::new(),
                };
                let entries = entries
                    .into_iter()
                    .filter_map(|entry| {
                        let mut child = opened.path.clone();
                        child.push(entry.name.clone());
                        FileAccess::fits_handle(&child).then(|| FileEntry {
                            handle: files.seal_handle(&session, &opened.approval.id, &child),
                            name: entry.name,
                            directory: entry.directory,
                            size_bytes: entry.size,
                            modified_at: entry.modified,
                        })
                    })
                    .collect();
                Ok(FilesListReply {
                    entries,
                    next_cursor,
                })
            })
            .await?;
        // The listing was made for a call that may since have lost its permission.
        self.authorize_files(&context, &[operation::READ])?;
        Ok(Response::new(reply))
    }

    pub(super) async fn read_file(
        &self,
        request: Request<FilesReadRequest>,
    ) -> Result<Response<FileReadReply>, Status> {
        let request = request.into_inner();
        let context = request.context.clone().unwrap_or_default();
        let caller = self.authorize_files(&context, &[operation::READ])?;
        handle_text(&request.file_handle)?;
        let budget = match request.max_bytes {
            0 => DEFAULT_READ_BYTES,
            bytes => usize::try_from(bytes)
                .unwrap_or(usize::MAX)
                .min(MAX_CHUNK_BYTES),
        };
        let (files, feed) = (self.files.clone(), self.library.clone());
        let session = caller.session_id.clone();
        let reply = self
            .in_namespace(&caller, move |namespace| {
                let opened = open_handle(
                    &files,
                    &feed,
                    namespace,
                    &session,
                    &request.file_handle,
                    operation::READ,
                )?;
                let (contents, eof) = opened
                    .confined
                    .read(&opened.path, request.offset, budget)
                    .map_err(file_status)?;
                Ok(FileReadReply {
                    next_offset: request.offset + contents.len() as u64,
                    contents,
                    eof,
                })
            })
            .await?;
        self.authorize_files(&context, &[operation::READ])?;
        Ok(Response::new(reply))
    }

    /// The bytes of the file `handle` names, for a picture that is about to become the cover of
    /// `installation_id`: at most `limit + 1` bytes, so the caller can tell that the file is over
    /// its bound without reading all of it. The handle must be one of this session, from an
    /// approval of this same installation that allows reading, and every check of a normal read
    /// applies. Runs on a blocking worker under the plugin's namespace lock.
    pub(super) async fn read_handle_image(
        &self,
        caller: &ResourceCaller,
        handle: &str,
        installation_id: &str,
        limit: usize,
    ) -> Result<Vec<u8>, Status> {
        handle_text(handle)?;
        let (files, feed) = (self.files.clone(), self.library.clone());
        let session = caller.session_id.clone();
        let handle = handle.to_owned();
        let installation_id = installation_id.to_owned();
        self.in_namespace(caller, move |namespace| {
            let opened = open_handle(&files, &feed, namespace, &session, &handle, operation::READ)?;
            // A file the user approved for another game's folder is not a picture for this one.
            if opened.approval.installation_id != installation_id {
                return Err(Status::permission_denied(
                    "this file handle belongs to another library entry",
                ));
            }
            let (bytes, _) = opened
                .confined
                .read(&opened.path, 0, limit.saturating_add(1))
                .map_err(file_status)?;
            Ok(bytes)
        })
        .await
    }

    pub(super) async fn write_file(
        &self,
        request: Request<FilesWriteRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation.context.clone().unwrap_or_default();
        let caller = self.authorize_files(&context, &[operation::WRITE])?;
        let idempotency = mutation_key(mutation)?.to_owned();
        if mutation.expected_revision != 0 {
            return Err(Status::invalid_argument(
                "files carry no revision; expected_revision must be 0",
            ));
        }
        handle_text(&request.file_handle)?;
        if !request.name.is_empty() && !confined::valid_new_name(&request.name) {
            return Err(Status::invalid_argument(
                "name must be one plain file name: no separators, no `.`/`..`, no \\ : * ? \" < > |, \
                 no trailing space or dot and no reserved device name",
            ));
        }
        if request.contents.len() > MAX_CHUNK_BYTES {
            return Err(Status::invalid_argument(format!(
                "contents are larger than {MAX_CHUNK_BYTES} bytes; write in several calls"
            )));
        }
        let hash = request_hash(&[
            "files.write",
            &request.file_handle,
            &request.name,
            &request.offset.to_string(),
            if request.truncate { "t" } else { "k" },
            &format!("{:x}", Sha256::digest(&request.contents)),
        ]);
        let guard = self.guard_files(&context, &[operation::WRITE], caller.grant_revision);
        let (files, feed) = (self.files.clone(), self.library.clone());
        let mutations = self.mutations.clone();
        let remember_as = caller.clone();
        let session = caller.session_id.clone();
        let answered = self
            .in_namespace(&caller, move |namespace| {
                if let Some((original, _)) =
                    mutations.replay_with(&remember_as, &idempotency, hash)?
                {
                    return Ok(original);
                }
                let opened = open_handle(
                    &files,
                    &feed,
                    namespace,
                    &session,
                    &request.file_handle,
                    operation::WRITE,
                )?;
                let (target, written) = if request.name.is_empty() {
                    (Target::Existing(&opened.path), opened.path.clone())
                } else {
                    let mut child = opened.path.clone();
                    child.push(request.name.clone());
                    (
                        Target::Create {
                            directory: &opened.path,
                            name: &request.name,
                        },
                        child,
                    )
                };
                if !FileAccess::fits_handle(&written) {
                    return Err(Status::invalid_argument("the path is too long"));
                }
                guard()?;
                opened
                    .confined
                    .write(target, request.offset, &request.contents, request.truncate)
                    .map_err(file_status)?;
                let handle = files.seal_handle(&session, &opened.approval.id, &written);
                let answer = reply(&handle, 0, MutationOutcome::Applied);
                mutations.remember_with(&remember_as, &idempotency, hash, &answer, 0);
                Ok(answer)
            })
            .await?;
        Ok(Response::new(answered))
    }

    pub(super) async fn delete_file(
        &self,
        request: Request<FilesDeleteRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation.context.clone().unwrap_or_default();
        let caller = self.authorize_files(&context, &[operation::DELETE])?;
        let idempotency = mutation_key(mutation)?.to_owned();
        if mutation.expected_revision != 0 {
            return Err(Status::invalid_argument(
                "files carry no revision; expected_revision must be 0",
            ));
        }
        handle_text(&request.file_handle)?;
        let hash = request_hash(&["files.delete", &request.file_handle]);
        let guard = self.guard_files(&context, &[operation::DELETE], caller.grant_revision);
        let (files, feed) = (self.files.clone(), self.library.clone());
        let mutations = self.mutations.clone();
        let remember_as = caller.clone();
        let session = caller.session_id.clone();
        let answered = self
            .in_namespace(&caller, move |namespace| {
                if let Some((original, _)) =
                    mutations.replay_with(&remember_as, &idempotency, hash)?
                {
                    return Ok(original);
                }
                let opened = open_handle(
                    &files,
                    &feed,
                    namespace,
                    &session,
                    &request.file_handle,
                    operation::DELETE,
                )?;
                guard()?;
                opened.confined.delete(&opened.path).map_err(file_status)?;
                let answer = reply(&request.file_handle, 0, MutationOutcome::Applied);
                mutations.remember_with(&remember_as, &idempotency, hash, &answer, 0);
                Ok(answer)
            })
            .await?;
        Ok(Response::new(answered))
    }
}

// Blocking; callers run in_namespace on a worker. Never refresh approval from observation.
fn check_volume(approval: &FileApproval) -> Result<(), FileError> {
    let current = gamenode_platform::volume::volume_info(Path::new(&approval.root));
    if approval.volume_token.is_empty()
        || !current.is_some_and(|info| {
            !info.volume_identity_token.is_empty()
                && info.volume_identity_token == approval.volume_token
        })
    {
        return Err(FileError::RootChanged);
    }
    Ok(())
}
