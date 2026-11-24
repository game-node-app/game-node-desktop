//! The `Locations` service, the common inspection of approved places (task 13.3, design
//! section 12): `GetSize`, `GetAccess` and `GetLastModified`.
//!
//! A place is named by a handle a plugin already holds for a folder the user approved (task
//! 6.2), or by the id of one of the plugin's save profiles (task 13.1), which stands for the
//! union of its folders narrowed by their naming rules. Inspection reads *metadata only*: no file
//! is opened for its content, and nothing is changed to find out (no permission, no timestamp, no
//! file). It needs `game.storage.inspect` for an installation folder and `game.saves.inspect` for
//! a save or prefix folder or a profile, and a standing approval; it does not need, and does not
//! give, the right to read a file.
//!
//! The walk is bounded (entries, time) and stops when the call is abandoned. A cut is reported as
//! `complete = false`; a folder that is gone or unreadable is reported through `availability`,
//! never as zero bytes. Sizes are logical bytes of the files, not allocated disk space.
//! Volume inspection and availability watches are implemented in `volumes`.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gamenode_platform::plugin_files::{ACCESS_METHOD, AccessInfo, Budget, Entry, FileError, Tri};
use gamenode_platform::plugin_storage::SaveProfileRow;
use gamenode_plugin_protocol::{capability as cap, proto::*};
use tonic::{Request, Response, Status};

use super::files::{
    Opened, approval_of_handle, file_status, open_approval_inspect, open_handle_inspect,
};
use super::mutations::request_hash;
use super::{BoxStream, ResourceApi, Served};
use crate::plugin_host::files::FileAccess;
use crate::plugin_host::fingerprints::{
    self as fingerprints, ALGORITHM, CODE_READY, DEFAULT_ENTRIES as DEFAULT_FINGERPRINT_ENTRIES,
    MAX_BYTES, MAX_ENTRIES as MAX_FINGERPRINT_ENTRIES, MAX_TIME, Record, Snapshot,
};
use crate::plugin_host::jobs::{JobOwner, Outcome, work};
use crate::plugin_host::library::LibraryFeed;
use crate::plugin_host::saves::{self as saves, Directory};
use crate::plugin_host::storage::Namespace;

/// Entries a size walk visits when the request does not say, and the most it may be asked to.
mod volumes;

const DEFAULT_ENTRIES: usize = 10_000;
const MAX_ENTRIES: usize = 100_000;
/// Entries a timestamp or access answer lists when the request does not say, and the most.
const DEFAULT_LISTED: usize = 100;
const MAX_LISTED: usize = 1_000;
/// Entry handles one access request may name.
const MAX_ENTRY_HANDLES: usize = 100;
/// The longest a walk runs.
const WALK_TIME: Duration = Duration::from_secs(5);
const MAX_HANDLE_BYTES: usize = 16 * 1024;

const INSPECT_PERMISSIONS: &str = "game.storage.inspect,game.saves.inspect";
const SCOPE: &str = "a folder the user approved for this plugin, or one of its save profiles; \
     game.storage.inspect for an installation folder, game.saves.inspect for saves";

/// The `Locations` methods this build serves, for `DescribeApi`.
pub const SERVED: &[Served] = &[
    Served::new("Inspect", INSPECT_PERMISSIONS, false, SCOPE),
    Served::new("WatchAvailability", INSPECT_PERMISSIONS, false, SCOPE),
    Served::new("GetSize", INSPECT_PERMISSIONS, false, SCOPE),
    Served::new("GetAccess", INSPECT_PERMISSIONS, false, SCOPE),
    Served::new("GetLastModified", INSPECT_PERMISSIONS, false, SCOPE),
    Served::new("GetFingerprint", INSPECT_PERMISSIONS, false, SCOPE),
    Served::new("Compare", INSPECT_PERMISSIONS, true, SNAPSHOT_SCOPE),
];

const SNAPSHOT_SCOPE: &str = "this plugin's own fingerprints, of places it may inspect";

/// Stops a walk that is still running on a worker when the call that wanted it is dropped.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// What a place is, once looked up.
#[allow(clippy::large_enum_variant)] // A short-lived temporary, not a stored collection.
enum Place {
    /// A folder or file below an approval.
    Handle {
        kind: String,
    },
    Profile {
        row: SaveProfileRow,
    },
}

impl Place {
    /// The permission the place needs.
    fn permission(&self) -> &'static str {
        match self {
            Self::Handle { kind } if matches!(kind.as_str(), "installation" | "library") => {
                cap::GAME_STORAGE_INSPECT
            }
            _ => cap::GAME_SAVES_INSPECT,
        }
    }
}

fn handle_text(handle: &str) -> Result<(), Status> {
    if handle.is_empty() || handle.len() > MAX_HANDLE_BYTES {
        return Err(Status::invalid_argument(
            "approved_resource_handle is required",
        ));
    }
    Ok(())
}

/// How a place that could not be inspected is reported.
fn availability_of(error: FileError) -> Option<InspectionAvailability> {
    match error {
        FileError::NotFound | FileError::RootChanged => Some(InspectionAvailability::Missing),
        FileError::Access => Some(InspectionAvailability::Inaccessible),
        FileError::Io => Some(InspectionAvailability::Unknown),
        // A path outside the approval is a refusal, not an availability.
        _ => None,
    }
}

fn or_unavailable<T>(
    result: Result<T, FileError>,
) -> Result<Result<T, InspectionAvailability>, Status> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(error) => availability_of(error)
            .map(Err)
            .ok_or_else(|| file_status(error)),
    }
}

/// What an access question is about, once looked up.
enum Target {
    Open(Box<Opened>),
    Unavailable(InspectionAvailability),
}

/// One entry seen on the way.
struct Seen {
    approval_id: String,
    path: Vec<String>,
    entry: Entry,
}

/// What a walk of a place found.
#[derive(Default)]
struct Measured {
    bytes: u64,
    files: u64,
    directories: u64,
    complete: bool,
    /// Entries below the place, in visit order, for the timestamp answer (bounded).
    seen: Vec<Seen>,
    /// The place's own folder, when it is one.
    own: Option<Seen>,
    /// The newest modification of a selected entry below the place.
    newest: Option<(i64, u32)>,
    unavailable: Option<InspectionAvailability>,
}

fn newer(a: Option<(i64, u32)>, b: (i64, u32)) -> Option<(i64, u32)> {
    Some(a.map_or(b, |a| a.max(b)))
}

/// Walks `start` below `approval_id`, counting what the folder's rules select once each.
#[allow(clippy::too_many_arguments)]
fn walk_folder(
    opened: &Opened,
    approval_id: &str,
    start: &[String],
    patterns: &[String],
    counted: &mut HashSet<(String, String)>,
    budget: &Budget<'_>,
    keep: usize,
    measured: &mut Measured,
) -> Result<(), FileError> {
    let root = opened.confined.stat(start)?;
    if !root.directory {
        // A single file: it is the whole place.
        if counted.insert((approval_id.to_owned(), start.join("/"))) {
            measured.files += 1;
            measured.bytes += root.size;
            measured.newest = newer(measured.newest, (root.modified, root.modified_nanos));
        }
        measured.seen.push(Seen {
            approval_id: approval_id.to_owned(),
            path: start.to_vec(),
            entry: root,
        });
        measured.complete = true;
        return Ok(());
    }
    if measured.own.is_none() {
        measured.own = Some(Seen {
            approval_id: approval_id.to_owned(),
            path: start.to_vec(),
            entry: root,
        });
    }
    let walk = opened
        .confined
        .walk_tree(start, budget, &mut |path, entry| {
            let relative = &path[start.len()..];
            if entry.directory {
                if counted.insert((approval_id.to_owned(), path.join("/"))) {
                    measured.directories += 1;
                }
                return;
            }
            if !saves::selected(patterns, relative) {
                return;
            }
            if counted.insert((approval_id.to_owned(), path.join("/"))) {
                measured.files += 1;
                measured.bytes += entry.size;
                measured.newest = newer(measured.newest, (entry.modified, entry.modified_nanos));
                if measured.seen.len() < keep {
                    measured.seen.push(Seen {
                        approval_id: approval_id.to_owned(),
                        path: path.to_vec(),
                        entry: entry.clone(),
                    });
                }
            }
        })?;
    measured.complete &= walk.complete;
    Ok(())
}

/// Looks the place up, on the namespace worker. A handle that is not this session's, not an
/// approval of this plugin or no longer standing is `NOT_FOUND`.
fn find_place(
    files: &FileAccess,
    feed: &LibraryFeed,
    namespace: &mut Namespace<'_>,
    session: &str,
    handle: &str,
) -> Result<Place, Status> {
    if handle.starts_with("profile-") {
        let row = namespace
            .save_profile(handle)?
            .ok_or_else(|| Status::not_found("save profile not found"))?;
        return Ok(Place::Profile { row });
    }
    let approval = approval_of_handle(files, feed, namespace, session, handle)?;
    Ok(Place::Handle {
        kind: approval.kind,
    })
}

impl ResourceApi {
    /// Finds the place and authorizes it: the permission is the one the place's kind needs, so
    /// the plugin is asked for it only once its handle is known to be real.
    async fn inspect_place(
        &self,
        context: Option<&ServiceContext>,
        handle: &str,
    ) -> Result<
        (
            crate::plugin_host::provider::ResourceCaller,
            Place,
            &'static str,
        ),
        Status,
    > {
        let caller = self.providers.authorize_resource_session(context)?;
        handle_text(handle)?;
        let (files, feed, session) = (
            self.files.clone(),
            self.library.clone(),
            caller.session_id.clone(),
        );
        let wanted = handle.to_owned();
        let place = self
            .in_namespace(&caller, move |namespace| {
                find_place(&files, &feed, namespace, &session, &wanted)
            })
            .await?;
        let permission = place.permission();
        let caller = self.providers.authorize_resource(context, permission)?;
        Ok((caller, place, permission))
    }

    /// Walks the place on a blocking worker under the plugin's namespace lock. The walk stops when
    /// this call is dropped.
    async fn measure(
        &self,
        caller: &crate::plugin_host::provider::ResourceCaller,
        place: Place,
        handle: String,
        max_entries: usize,
        keep: usize,
    ) -> Result<Measured, Status> {
        let cancel = Arc::new(AtomicBool::new(false));
        let _stop = StopOnDrop(cancel.clone());
        let (files, feed, session) = (
            self.files.clone(),
            self.library.clone(),
            caller.session_id.clone(),
        );
        self.in_namespace(caller, move |namespace| {
            let budget = Budget {
                max_entries,
                deadline: Instant::now() + WALK_TIME,
                cancel: &cancel,
            };
            let mut measured = Measured {
                complete: true,
                ..Measured::default()
            };
            let mut counted = HashSet::new();
            match place {
                Place::Handle { .. } => {
                    let opened =
                        match open_handle_inspect(&files, &feed, namespace, &session, &handle)? {
                            Ok(opened) => opened,
                            // The folder cannot be opened now: say what it is like instead.
                            Err(error) => {
                                measured.complete = false;
                                measured.unavailable =
                                    Some(availability_of(error).ok_or_else(|| file_status(error))?);
                                return Ok(measured);
                            }
                        };
                    let id = opened.approval.id.clone();
                    let start = opened.path.clone();
                    if let Err(error) = walk_folder(
                        &opened,
                        &id,
                        &start,
                        &[],
                        &mut counted,
                        &budget,
                        keep,
                        &mut measured,
                    ) {
                        measured = Measured::default();
                        measured.unavailable =
                            Some(availability_of(error).ok_or_else(|| file_status(error))?);
                    }
                }
                Place::Profile { row } => {
                    for directory in saves::directories_from_json(&row.directories) {
                        let Directory {
                            approval_id,
                            path,
                            naming_patterns,
                            ..
                        } = directory;
                        // A folder whose approval is gone (or moved to another entry) is not
                        // reachable: the total is then partial, not zero.
                        let opened = match open_approval_inspect(
                            &files,
                            &feed,
                            namespace,
                            &approval_id,
                            path.clone(),
                        ) {
                            Ok(Ok(opened))
                                if opened.approval.installation_id == row.installation_id =>
                            {
                                opened
                            }
                            _ => {
                                measured.complete = false;
                                continue;
                            }
                        };
                        if let Err(error) = walk_folder(
                            &opened,
                            &approval_id,
                            &path,
                            &naming_patterns,
                            &mut counted,
                            &budget,
                            keep,
                            &mut measured,
                        ) {
                            match availability_of(error) {
                                Some(_) => measured.complete = false,
                                None => return Err(file_status(error)),
                            }
                        }
                    }
                }
            }
            Ok(measured)
        })
        .await
    }

    pub(super) async fn get_location_size(
        &self,
        request: Request<LocationSizeRequest>,
    ) -> Result<Response<LocationSizeReply>, Status> {
        let request = request.into_inner();
        let context = request.context.clone();
        let (caller, place, permission) = self
            .inspect_place(context.as_ref(), &request.approved_resource_handle)
            .await?;
        let max = match request.max_entries as usize {
            0 => DEFAULT_ENTRIES,
            max => max.min(MAX_ENTRIES),
        };
        let measured = self
            .measure(&caller, place, request.approved_resource_handle, max, 0)
            .await?;
        // The permission was checked before a walk that may have taken seconds.
        self.providers
            .authorize_resource(context.as_ref(), permission)?;
        let reply = match measured.unavailable {
            Some(availability) => LocationSizeReply {
                logical_bytes: 0,
                file_count: 0,
                directory_count: 0,
                complete: false,
                availability: availability as i32,
            },
            None => LocationSizeReply {
                logical_bytes: measured.bytes,
                file_count: measured.files,
                directory_count: measured.directories,
                complete: measured.complete,
                availability: InspectionAvailability::Available as i32,
            },
        };
        Ok(Response::new(reply))
    }

    pub(super) async fn get_location_last_modified(
        &self,
        request: Request<LocationLastModifiedRequest>,
    ) -> Result<Response<LocationLastModifiedReply>, Status> {
        let request = request.into_inner();
        let context = request.context.clone();
        let (caller, place, permission) = self
            .inspect_place(context.as_ref(), &request.approved_resource_handle)
            .await?;
        let listed = match request.max_entries as usize {
            0 => DEFAULT_LISTED,
            max => max.min(MAX_LISTED),
        };
        let measured = self
            .measure(
                &caller,
                place,
                request.approved_resource_handle.clone(),
                DEFAULT_ENTRIES,
                listed,
            )
            .await?;
        self.providers
            .authorize_resource(context.as_ref(), permission)?;
        if let Some(availability) = measured.unavailable {
            return Ok(Response::new(LocationLastModifiedReply {
                entries: Vec::new(),
                aggregate_latest_at: None,
                availability: availability as i32,
            }));
        }
        let files = self.files.clone();
        let session = caller.session_id.clone();
        let stamp = |seen: &Seen, handle: Option<&str>| {
            // An entry the system gave no time for is left out, never reported as the epoch.
            if seen.entry.modified == 0 && seen.entry.modified_nanos == 0 {
                return None;
            }
            let entry_handle = handle.map(str::to_owned).unwrap_or_else(|| {
                if FileAccess::fits_handle(&seen.path) {
                    files.seal_handle(&session, &seen.approval_id, &seen.path)
                } else {
                    String::new()
                }
            });
            Some(LocationTimestamp {
                entry_handle,
                modified_at: seen.entry.modified,
                precision: if seen.entry.modified_nanos != 0 {
                    "subsecond"
                } else {
                    "whole_second"
                }
                .to_owned(),
            })
        };
        let mut entries = Vec::new();
        // The folder's own time comes first and apart from the entries below it.
        if let Some(own) = &measured.own
            && !request.approved_resource_handle.starts_with("profile-")
        {
            entries.extend(stamp(own, Some(&request.approved_resource_handle)));
        }
        entries.extend(measured.seen.iter().filter_map(|seen| stamp(seen, None)));
        Ok(Response::new(LocationLastModifiedReply {
            entries,
            aggregate_latest_at: measured.newest.map(|(seconds, _)| seconds),
            availability: InspectionAvailability::Available as i32,
        }))
    }

    pub(super) async fn get_location_access(
        &self,
        request: Request<LocationAccessRequest>,
    ) -> Result<Response<LocationAccessReply>, Status> {
        let request = request.into_inner();
        let context = request.context.clone();
        if request.entry_handles.len() > MAX_ENTRY_HANDLES {
            return Err(Status::invalid_argument(format!(
                "at most {MAX_ENTRY_HANDLES} entry handles per request"
            )));
        }
        for handle in &request.entry_handles {
            handle_text(handle)?;
        }
        let (caller, place, permission) = self
            .inspect_place(context.as_ref(), &request.approved_resource_handle)
            .await?;
        let (files, feed, session) = (
            self.files.clone(),
            self.library.clone(),
            caller.session_id.clone(),
        );
        let handle = request.approved_resource_handle.clone();
        let entry_handles = request.entry_handles.clone();
        let answers = self
            .in_namespace(&caller, move |namespace| {
                // What to look at: the entries named, or the place itself (a profile's folders).
                let mut targets: Vec<(String, Target)> = Vec::new();
                match &place {
                    Place::Handle { .. } => {
                        let approval =
                            approval_of_handle(&files, &feed, namespace, &session, &handle)?;
                        let names: Vec<&String> = if entry_handles.is_empty() {
                            vec![&handle]
                        } else {
                            entry_handles.iter().collect()
                        };
                        for name in names {
                            // Only entries of the same approval belong to this place.
                            let theirs =
                                approval_of_handle(&files, &feed, namespace, &session, name)?;
                            if theirs.id != approval.id {
                                return Err(Status::not_found("file handle not found"));
                            }
                            let target = match open_handle_inspect(
                                &files, &feed, namespace, &session, name,
                            )? {
                                Ok(opened) => Target::Open(Box::new(opened)),
                                Err(error) => Target::Unavailable(
                                    availability_of(error).ok_or_else(|| file_status(error))?,
                                ),
                            };
                            targets.push((name.clone(), target));
                        }
                    }
                    Place::Profile { row } => {
                        if !entry_handles.is_empty() {
                            return Err(Status::invalid_argument(
                                "a save profile is inspected as a whole; entry_handles name \
                                 entries of a folder",
                            ));
                        }
                        for directory in saves::directories_from_json(&row.directories) {
                            let sealed = files.seal_handle(
                                &session,
                                &directory.approval_id,
                                &directory.path,
                            );
                            // A folder whose approval is gone (or is another entry's) is reported
                            // as missing, not hidden.
                            let target = match open_approval_inspect(
                                &files,
                                &feed,
                                namespace,
                                &directory.approval_id,
                                directory.path.clone(),
                            ) {
                                Ok(Ok(opened))
                                    if opened.approval.installation_id == row.installation_id =>
                                {
                                    Target::Open(Box::new(opened))
                                }
                                Ok(Err(error)) => Target::Unavailable(
                                    availability_of(error)
                                        .unwrap_or(InspectionAvailability::Missing),
                                ),
                                _ => Target::Unavailable(InspectionAvailability::Missing),
                            };
                            // A lost folder has no handle to give.
                            let sealed = if matches!(target, Target::Open(_)) {
                                sealed
                            } else {
                                String::new()
                            };
                            targets.push((sealed, target));
                        }
                    }
                }
                let mut entries = Vec::new();
                for (entry_handle, target) in targets {
                    let unavailable =
                        |entry_handle: String, availability: InspectionAvailability| {
                            LocationAccessEntry {
                                entry_handle,
                                read_only_state: "unknown".into(),
                                effective_access: unknown_access(),
                                availability: availability as i32,
                            }
                        };
                    entries.push(match target {
                        Target::Unavailable(availability) => {
                            unavailable(entry_handle, availability)
                        }
                        Target::Open(opened) => {
                            match or_unavailable(opened.confined.access(&opened.path))? {
                                Ok(info) => access_entry(entry_handle, &info),
                                Err(availability) => unavailable(entry_handle, availability),
                            }
                        }
                    });
                }
                Ok(entries)
            })
            .await?;
        self.providers
            .authorize_resource(context.as_ref(), permission)?;
        Ok(Response::new(LocationAccessReply {
            entries: answers,
            availability: InspectionAvailability::Available as i32,
        }))
    }
}

/// One folder of a fingerprint, opened on the namespace worker and then walked away from it.
struct Folder {
    /// The profile's folder number, which starts its labels (`d<n>/`); `None` for a plain handle.
    index: Option<usize>,
    approval_id: String,
    start: Vec<String>,
    patterns: Vec<String>,
    opened: Opened,
}

/// What a fingerprint covers, ready to be read without the plugin's namespace lock.
struct Plan {
    scope: String,
    revision: u64,
    folders: Vec<Folder>,
    /// A folder of a profile whose approval is gone.
    partial: bool,
    unavailable: Option<InspectionAvailability>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

fn label_of(folder: &Folder, path: &[String]) -> String {
    match folder.index {
        Some(index) => format!("d{index}/{}", path.join("/")),
        None => {
            // Below the place's own folder; a single file is named by its own name.
            let below = path.get(folder.start.len()..).unwrap_or(&[]);
            if below.is_empty() {
                path.last().cloned().unwrap_or_default()
            } else {
                below.join("/")
            }
        }
    }
}

/// Reads the plan's folders and hashes their files. Blocking. `None` when cancelled.
fn build_snapshot(
    plan: &Plan,
    id: String,
    permission: &'static str,
    max_entries: usize,
    cancel: &AtomicBool,
    after_hash: Option<fingerprints::Hook>,
) -> Option<Snapshot> {
    let started_at = unix_now();
    let deadline = Instant::now() + MAX_TIME;
    let mut complete = !plan.partial;
    let mut unstable = false;
    let mut bytes_read = 0_u64;
    let mut records: Vec<Record> = Vec::new();
    let mut counted: HashSet<(String, String)> = HashSet::new();
    'folders: for folder in &plan.folders {
        let confined = &folder.opened.confined;
        let budget = Budget {
            max_entries: max_entries.saturating_sub(records.len()),
            deadline,
            cancel,
        };
        // What is below the start (the start itself when it is a file), as the walk saw it.
        let mut found: Vec<(Vec<String>, Entry)> = Vec::new();
        match confined.stat(&folder.start) {
            Ok(root) if !root.directory => found.push((folder.start.clone(), root)),
            Ok(_) => match confined.walk_tree(&folder.start, &budget, &mut |path, entry| {
                found.push((path.to_vec(), entry.clone()));
            }) {
                Ok(walk) => complete &= walk.complete,
                Err(_) => {
                    complete = false;
                    continue;
                }
            },
            Err(_) => {
                complete = false;
                continue;
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        for (path, entry) in found {
            let relative = path.get(folder.start.len()..).unwrap_or(&[]);
            if !entry.directory
                && !saves::selected(&folder.patterns, relative)
                && !relative.is_empty()
            {
                continue;
            }
            if !counted.insert((folder.approval_id.clone(), path.join("/"))) {
                continue;
            }
            let mut record = Record {
                label: label_of(folder, &path),
                approval_id: folder.approval_id.clone(),
                path: path.clone(),
                directory: entry.directory,
                size: entry.size,
                digest: None,
                unreadable: false,
                unstable: false,
            };
            if !entry.directory {
                match confined.hash_file(&path, cancel, deadline) {
                    Ok(Some(digest)) => {
                        if let Some(hook) = &after_hash {
                            hook(&path);
                        }
                        // The same file, as long as it still has the size and time it had.
                        let steady = confined.stat(&path).is_ok_and(|after| {
                            after.size == entry.size
                                && after.modified == entry.modified
                                && after.modified_nanos == entry.modified_nanos
                        }) && digest.bytes == entry.size;
                        record.digest = Some(digest.sha256);
                        record.size = digest.bytes;
                        if !steady {
                            record.unstable = true;
                            unstable = true;
                        }
                        bytes_read += digest.bytes;
                    }
                    Ok(None) => {
                        if cancel.load(Ordering::Relaxed) {
                            return None;
                        }
                        // The time ran out.
                        complete = false;
                        break 'folders;
                    }
                    Err(_) => {
                        record.unreadable = true;
                        complete = false;
                    }
                }
            }
            records.push(record);
            if bytes_read > MAX_BYTES {
                complete = false;
                break 'folders;
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    records.sort_by(|a, b| a.label.as_bytes().cmp(b.label.as_bytes()));
    let whole = complete && !unstable;
    Some(Snapshot {
        id,
        permission,
        scope: plan.scope.clone(),
        revision: plan.revision,
        digest: whole.then(|| fingerprints::tree_digest(&records)),
        entries: records,
        complete: whole,
        unstable,
        started_at,
        finished_at: unix_now(),
    })
}

fn pending(job: &gamenode_plugin_protocol::proto::JobReply) -> LocationFingerprintReply {
    LocationFingerprintReply {
        job_id: job.job_id.clone(),
        consistency: "pending".into(),
        availability: InspectionAvailability::Available as i32,
        algorithm: ALGORITHM.into(),
        ..LocationFingerprintReply::default()
    }
}

impl ResourceApi {
    /// What the job's reply stands for: the stored fingerprint if it finished, or how it stands.
    fn fingerprint_reply(
        &self,
        owner: &JobOwner,
        job: &JobReply,
    ) -> Result<LocationFingerprintReply, Status> {
        match job.state() {
            JobState::Queued | JobState::Running => Ok(pending(job)),
            JobState::Succeeded if job.result_code == CODE_READY => {
                let snapshot = self.snapshots.get(owner, &job.message).ok_or_else(|| {
                    Status::failed_precondition(
                        "that fingerprint is no longer kept; ask again with a new idempotency key",
                    )
                })?;
                Ok(LocationFingerprintReply {
                    snapshot_id: snapshot.id.clone(),
                    algorithm: ALGORITHM.into(),
                    digest: snapshot.digest.map(|d| d.to_vec()).unwrap_or_default(),
                    revision: snapshot.revision,
                    complete: snapshot.complete,
                    consistency: if snapshot.unstable {
                        "unstable"
                    } else {
                        "best_effort"
                    }
                    .into(),
                    job_id: job.job_id.clone(),
                    availability: InspectionAvailability::Available as i32,
                    observed_start_at: snapshot.started_at,
                    observed_end_at: snapshot.finished_at,
                })
            }
            JobState::Cancelled => Ok(LocationFingerprintReply {
                job_id: job.job_id.clone(),
                consistency: "cancelled".into(),
                availability: InspectionAvailability::Unknown as i32,
                algorithm: ALGORITHM.into(),
                ..LocationFingerprintReply::default()
            }),
            _ => Ok(LocationFingerprintReply {
                job_id: job.job_id.clone(),
                consistency: "failed".into(),
                availability: InspectionAvailability::Unknown as i32,
                algorithm: ALGORITHM.into(),
                ..LocationFingerprintReply::default()
            }),
        }
    }

    pub(super) async fn get_location_fingerprint(
        &self,
        request: Request<LocationFingerprintRequest>,
    ) -> Result<Response<LocationFingerprintReply>, Status> {
        let request = request.into_inner();
        let context = request.context.clone();
        if !matches!(request.algorithm.as_str(), "" | "sha256" | ALGORITHM) {
            return Err(Status::invalid_argument(
                "algorithm must be \"sha256\" (or empty)",
            ));
        }
        let key = request.idempotency_key.as_str();
        if key.is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
            return Err(Status::invalid_argument(
                "idempotency_key is required: 1 to 128 bytes without control characters",
            ));
        }
        let max_entries = match request.max_entries as usize {
            0 => DEFAULT_FINGERPRINT_ENTRIES,
            max => max.min(MAX_FINGERPRINT_ENTRIES),
        };
        let (caller, place, permission) = self
            .inspect_place(context.as_ref(), &request.approved_resource_handle)
            .await?;
        let owner = JobOwner::of(&caller)?;
        // Every fingerprint reads the content again: there is no cache to ask for, so
        // `require_fresh` changes nothing.
        let hash = request_hash(&[
            "location-fingerprint",
            &request.approved_resource_handle,
            &max_entries.to_string(),
        ]);

        let _serial = self.mutations.lock.lock().await;
        if let Some(job) = self.jobs.replay(&owner, key, hash)? {
            self.providers
                .authorize_resource(context.as_ref(), permission)?;
            return self.fingerprint_reply(&owner, &job).map(Response::new);
        }
        let running = self.snapshots.begin(&owner)?;

        // The folders are opened under the plugin's namespace lock, which is then let go: the
        // reading takes as long as it takes and must not hold up the plugin's other calls.
        let (files, feed, session) = (
            self.files.clone(),
            self.library.clone(),
            caller.session_id.clone(),
        );
        let handle = request.approved_resource_handle.clone();
        let plan = self
            .in_namespace(&caller, move |namespace| {
                let mut plan = Plan {
                    scope: String::new(),
                    revision: 0,
                    folders: Vec::new(),
                    partial: false,
                    unavailable: None,
                };
                match place {
                    Place::Handle { .. } => {
                        match open_handle_inspect(&files, &feed, namespace, &session, &handle)? {
                            Ok(opened) => {
                                plan.scope = format!(
                                    "handle:{}:{}",
                                    opened.approval.id,
                                    opened.path.join("/")
                                );
                                plan.folders.push(Folder {
                                    index: None,
                                    approval_id: opened.approval.id.clone(),
                                    start: opened.path.clone(),
                                    patterns: Vec::new(),
                                    opened,
                                });
                            }
                            Err(error) => {
                                plan.unavailable =
                                    Some(availability_of(error).ok_or_else(|| file_status(error))?);
                            }
                        }
                    }
                    Place::Profile { row } => {
                        plan.scope = format!("profile:{}", row.id);
                        plan.revision = row.revision;
                        for (index, directory) in saves::directories_from_json(&row.directories)
                            .into_iter()
                            .enumerate()
                        {
                            match open_approval_inspect(
                                &files,
                                &feed,
                                namespace,
                                &directory.approval_id,
                                directory.path.clone(),
                            ) {
                                Ok(Ok(opened))
                                    if opened.approval.installation_id == row.installation_id =>
                                {
                                    plan.folders.push(Folder {
                                        index: Some(index),
                                        approval_id: directory.approval_id,
                                        start: directory.path,
                                        patterns: directory.naming_patterns,
                                        opened,
                                    });
                                }
                                _ => plan.partial = true,
                            }
                        }
                    }
                }
                Ok(plan)
            })
            .await?;
        if let Some(availability) = plan.unavailable {
            // There is nothing to read: say why, with no digest and no job.
            return Ok(Response::new(LocationFingerprintReply {
                algorithm: ALGORITHM.into(),
                consistency: "unavailable".into(),
                availability: availability as i32,
                ..LocationFingerprintReply::default()
            }));
        }

        let snapshots = self.snapshots.clone();
        let (job_owner, id) = (owner.clone(), fingerprints::new_snapshot_id()?);
        let run = work(move |mut ctx| async move {
            // The slot stays taken until the job ends, however it ends.
            let _running = running;
            let cancel = std::sync::Arc::new(AtomicBool::new(false));
            let flag = cancel.clone();
            let hook = snapshots.after_hash();
            let mut blocking = tokio::task::spawn_blocking(move || {
                build_snapshot(&plan, id, permission, max_entries, &flag, hook)
            });
            let built = tokio::select! {
                built = &mut blocking => built,
                () = ctx.cancelled() => {
                    // The walk and the hashing stop at the next entry or chunk.
                    cancel.store(true, Ordering::Relaxed);
                    blocking.await
                }
            };
            match built {
                Ok(Some(snapshot)) => {
                    // Whatever happened to the permission while it ran, nothing is kept for a
                    // plugin that no longer may inspect.
                    if let Err(status) = ctx.check() {
                        return Outcome::stopped(&status);
                    }
                    let kept = snapshots.keep(&job_owner, snapshot);
                    Outcome::succeeded(CODE_READY, &kept.id)
                }
                Ok(None) => Outcome::cancelled("stopped before it finished"),
                Err(_) => Outcome::failed("internal", "the fingerprint could not be made"),
            }
        });
        let (job, _) = self.start_job(
            &caller,
            context.clone().unwrap_or_default(),
            permission,
            key,
            hash,
            None,
            run,
        )?;
        // A small place is done at once; a large one answers with its job.
        let job = self
            .jobs
            .wait_over(&owner, &job.job_id, self.snapshots.fast_wait())
            .await?;
        self.providers
            .authorize_resource(context.as_ref(), permission)?;
        self.fingerprint_reply(&owner, &job).map(Response::new)
    }

    pub(super) async fn compare_location_snapshots(
        &self,
        request: Request<LocationCompareRequest>,
    ) -> Result<Response<LocationCompareReply>, Status> {
        let request = request.into_inner();
        let context = request.context.clone();
        let caller = self
            .providers
            .authorize_resource_session(context.as_ref())?;
        let owner = JobOwner::of(&caller)?;
        let get = |id: &str| {
            self.snapshots
                .get(&owner, id)
                .ok_or_else(|| Status::not_found("fingerprint not found"))
        };
        let before = get(&request.before_snapshot_id)?;
        let after = get(&request.after_snapshot_id)?;
        // Comparing needs what taking them needed, for both.
        for snapshot in [&before, &after] {
            self.providers
                .authorize_resource(context.as_ref(), snapshot.permission)?;
        }
        if request.expected_revision != 0 && request.expected_revision != before.revision {
            return Err(Status::failed_precondition(
                "the place changed since that revision; take new fingerprints",
            ));
        }
        let size = match request.page_size as usize {
            0 => 100,
            size => size.min(500),
        };
        let start = if request.cursor.is_empty() {
            0
        } else {
            request
                .cursor
                .parse::<usize>()
                .map_err(|_| Status::invalid_argument("the cursor is not one this host gave"))?
        };
        let comparison = fingerprints::compare(&before, &after);
        let files = self.files.clone();
        let session = caller.session_id.clone();
        let page: Vec<LocationChange> = comparison
            .changes
            .iter()
            .skip(start)
            .take(size)
            .map(|change| LocationChange {
                entry_handle: match &change.current {
                    Some(now) if FileAccess::fits_handle(&now.path) => {
                        files.seal_handle(&session, &now.approval_id, &now.path)
                    }
                    _ => String::new(),
                },
                change_kind: change.kind.as_str().into(),
                previous_size_bytes: change.previous_size,
                current_size_bytes: change.current_size,
                relative_path: change.label.clone(),
            })
            .collect();
        let end = start + page.len();
        Ok(Response::new(LocationCompareReply {
            changes: page,
            next_cursor: if end < comparison.changes.len() {
                end.to_string()
            } else {
                String::new()
            },
            status: comparison.status.as_str().into(),
            revision: before.revision,
        }))
    }
}

fn word(answer: Tri) -> &'static str {
    match answer {
        Tri::Allowed => "allowed",
        Tri::Denied => "denied",
        Tri::Unknown => "unknown",
    }
}

fn unknown_access() -> String {
    "read=unknown write=unknown create=unknown delete=unknown method=none".into()
}

fn access_entry(entry_handle: String, info: &AccessInfo) -> LocationAccessEntry {
    LocationAccessEntry {
        entry_handle,
        read_only_state: if info.read_only {
            "read_only"
        } else {
            "writable"
        }
        .into(),
        effective_access: format!(
            "read={} write={} create={} delete={} method={ACCESS_METHOD}",
            word(info.read),
            word(info.write),
            word(info.create),
            word(info.delete)
        ),
        availability: InspectionAvailability::Available as i32,
    }
}

#[tonic::async_trait]
impl locations_server::Locations for ResourceApi {
    type WatchAvailabilityStream = BoxStream<LocationAvailabilityEvent>;

    async fn inspect(
        &self,
        request: Request<LocationInspectRequest>,
    ) -> Result<Response<LocationInspectReply>, Status> {
        self.inspect_location(request).await
    }

    async fn get_size(
        &self,
        request: Request<LocationSizeRequest>,
    ) -> Result<Response<LocationSizeReply>, Status> {
        self.get_location_size(request).await
    }

    async fn get_access(
        &self,
        request: Request<LocationAccessRequest>,
    ) -> Result<Response<LocationAccessReply>, Status> {
        self.get_location_access(request).await
    }

    async fn get_last_modified(
        &self,
        request: Request<LocationLastModifiedRequest>,
    ) -> Result<Response<LocationLastModifiedReply>, Status> {
        self.get_location_last_modified(request).await
    }

    async fn get_fingerprint(
        &self,
        request: Request<LocationFingerprintRequest>,
    ) -> Result<Response<LocationFingerprintReply>, Status> {
        self.get_location_fingerprint(request).await
    }

    async fn compare(
        &self,
        request: Request<LocationCompareRequest>,
    ) -> Result<Response<LocationCompareReply>, Status> {
        self.compare_location_snapshots(request).await
    }

    async fn watch_availability(
        &self,
        request: Request<LocationAvailabilityWatchRequest>,
    ) -> Result<Response<Self::WatchAvailabilityStream>, Status> {
        self.watch_location_availability(request).await
    }
}
