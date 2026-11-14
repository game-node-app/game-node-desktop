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
//! `Inspect`, `GetFingerprint`, `Compare` and `WatchAvailability` are not served yet.

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
use super::{BoxStream, ResourceApi, Served};
use crate::plugin_host::files::FileAccess;
use crate::plugin_host::library::LibraryFeed;
use crate::plugin_host::saves::{self as saves, Directory};
use crate::plugin_host::storage::Namespace;

/// Entries a size walk visits when the request does not say, and the most it may be asked to.
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
    Served::new("GetSize", INSPECT_PERMISSIONS, false, SCOPE),
    Served::new("GetAccess", INSPECT_PERMISSIONS, false, SCOPE),
    Served::new("GetLastModified", INSPECT_PERMISSIONS, false, SCOPE),
];

/// Stops a walk that is still running on a worker when the call that wanted it is dropped.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// What a place is, once looked up.
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
            Self::Handle { kind } if kind == "installation" => cap::GAME_STORAGE_INSPECT,
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

fn not_served(method: &str) -> Status {
    Status::unimplemented(format!(
        "Locations.{method} is not available in this GameNode build"
    ))
}

#[tonic::async_trait]
impl locations_server::Locations for ResourceApi {
    type WatchAvailabilityStream = BoxStream<LocationAvailabilityEvent>;

    async fn inspect(
        &self,
        _request: Request<LocationInspectRequest>,
    ) -> Result<Response<LocationInspectReply>, Status> {
        Err(not_served("Inspect"))
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
        _request: Request<LocationFingerprintRequest>,
    ) -> Result<Response<LocationFingerprintReply>, Status> {
        Err(not_served("GetFingerprint"))
    }

    async fn compare(
        &self,
        _request: Request<LocationCompareRequest>,
    ) -> Result<Response<LocationCompareReply>, Status> {
        Err(not_served("Compare"))
    }

    async fn watch_availability(
        &self,
        _request: Request<LocationAvailabilityWatchRequest>,
    ) -> Result<Response<Self::WatchAvailabilityStream>, Status> {
        Err(not_served("WatchAvailability"))
    }
}
