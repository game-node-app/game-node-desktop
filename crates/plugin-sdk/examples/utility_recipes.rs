//! Practical recipes for the utility services: private storage, files, network fetch and
//! notifications.
//!
//! These are plain functions over [`Resources`] (from `HostClient::resources()`); copy what you
//! need. They are compiled with the SDK and run against the desktop's fixture host in its service
//! tests, so they stay true to what the host does. Every refusal is the host's `tonic::Status`;
//! the permissions document explains each code. None of this is a sandbox: the services give a
//! plugin what the user approved through GameNode, and say nothing about what the plugin's own
//! process can do on the operating system.
#![allow(dead_code)]

use std::future::Future;
use std::time::Duration;

use gamenode_plugin_sdk::{
    Code, Durability, Fetch, Fetched, Notice, Outcome, Resources, Severity, Status, StoredWrite,
    Written, idempotency_key, proto::FileEntry,
};

/// How often a change is read again and retried after another writer got in first.
const ATTEMPTS: usize = 3;
/// How often one change is resent after a lost connection or timeout.
const SENDS: usize = 3;

/// Sends one change, resending it **with the same key** if the connection fails or times out.
/// The host remembers a committed key, so an answer lost on the way back is returned again
/// instead of the change being applied twice.
pub async fn send<T, F, Fut>(mut call: F) -> Result<T, Status>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Status>>,
{
    let mut last = Status::unavailable("nothing was sent");
    for attempt in 0..SENDS {
        match call().await {
            Err(status) if matches!(status.code(), Code::Unavailable | Code::DeadlineExceeded) => {
                last = status;
                tokio::time::sleep(Duration::from_millis(200 << attempt)).await;
            }
            done => return done,
        }
    }
    Err(last)
}

// ---- Storage ----

/// Reads a small value you keep for yourself; `None` when it was never written, was deleted or
/// has expired. Storage is not encrypted: never keep a token or password here (use the provider
/// `Secret` service).
pub async fn recall(resources: &Resources, key: &str) -> Result<Option<Vec<u8>>, Status> {
    let stored = resources.storage_get(key).await?;
    Ok(stored.found.then_some(stored.value))
}

/// Changes a stored value from what it was: reads it with its revision, applies `change`, and
/// writes the result with that revision. If someone else (another session of your plugin)
/// changed the key in between, the write is a conflict, so it reads again; `change` may
/// therefore run more than once and must not have side effects. Every attempt is a new change
/// and gets a new key; only a network retry of the same attempt reuses it.
///
/// `RESOURCE_EXHAUSTED` means the layer is full and nothing was changed: free space (delete, or
/// store with a `ttl`) and try again. `PERMISSION_DENIED` with [`Durability::Persistent`] means
/// the user has not approved durable storage for the plugin; the host does not quietly store it
/// in memory, so ask for [`Durability::SessionOnly`] explicitly if that is acceptable.
pub async fn update<F>(
    resources: &Resources,
    key: &str,
    durability: Durability,
    ttl: Option<Duration>,
    mut change: F,
) -> Result<StoredWrite, Status>
where
    F: FnMut(Option<Vec<u8>>) -> Vec<u8>,
{
    for _ in 0..ATTEMPTS {
        let stored = resources.storage_get(key).await?;
        let next = change(stored.found.then_some(stored.value));
        let write_key = idempotency_key();
        let written = send(|| {
            resources.storage_put(
                key,
                stored.revision,
                &write_key,
                next.clone(),
                durability,
                ttl,
            )
        })
        .await?;
        if written.outcome != Outcome::RevisionConflict {
            return Ok(written);
        }
    }
    Err(Status::aborted("the value kept changing; try again later"))
}

/// Forgets a key. Deleting what is not there succeeds (`Outcome::AlreadyApplied`). The revision
/// is read first so a delete does not remove a value another session just wrote.
pub async fn forget(resources: &Resources, key: &str) -> Result<Written, Status> {
    for _ in 0..ATTEMPTS {
        let stored = resources.storage_get(key).await?;
        let delete_key = idempotency_key();
        let written = send(|| resources.storage_delete(key, stored.revision, &delete_key)).await?;
        if written.outcome != Outcome::RevisionConflict {
            return Ok(written);
        }
    }
    Err(Status::aborted("the value kept changing; try again later"))
}

// ---- Files ----

/// What went wrong asking for a folder, in terms a plugin acts on.
#[derive(Debug)]
pub enum Access {
    /// The user did not approve (or the plugin lacks the grant). Do not ask again at once:
    /// explain what the plugin needs the folder for first.
    Declined,
    /// The handles are no longer valid; ask again.
    AskAgain,
    Other(Status),
}

/// Asks for read access to the installation folder of one of your own entries and returns its
/// root handle. The user chooses and confirms the folder in GameNode; you never see its path.
/// Ask for `write` only when you need to write: a read-only approval gives no write.
pub async fn open_installation(
    resources: &Resources,
    installation_id: &str,
    operations: &[&str],
) -> Result<String, Access> {
    let key = idempotency_key();
    match send(|| resources.files_request_access(installation_id, operations, &[], &key)).await {
        Ok(handles) => handles
            .into_iter()
            .next()
            .map(|handle| handle.handle)
            .ok_or(Access::Declined),
        Err(status) => Err(match status.code() {
            Code::PermissionDenied => Access::Declined,
            Code::FailedPrecondition => Access::AskAgain,
            _ => Access::Other(status),
        }),
    }
}

/// Finds the entry called `name` directly inside a directory handle.
pub async fn find(
    resources: &Resources,
    directory: &str,
    name: &str,
) -> Result<Option<FileEntry>, Status> {
    Ok(resources
        .files_list(directory)
        .await?
        .into_iter()
        .find(|entry| entry.name == name))
}

/// Reads a file of `directory` by name, at most `limit` bytes; `Ok(None)` when it is not there.
/// A longer file is `RESOURCE_EXHAUSTED` rather than read in part.
pub async fn read_named(
    resources: &Resources,
    directory: &str,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, Status> {
    let Some(entry) = find(resources, directory, name).await? else {
        return Ok(None);
    };
    if entry.directory {
        return Ok(None);
    }
    resources
        .files_read_all(&entry.handle, limit)
        .await
        .map(Some)
}

/// Writes (creating or replacing) the file `name` in a directory the user approved for `write`.
/// The name must be one plain component that is valid on every platform, so a file made on
/// Linux can be backed up on Windows. Returns the new file's handle.
pub async fn save_named(
    resources: &Resources,
    directory: &str,
    name: &str,
    contents: &[u8],
) -> Result<String, Status> {
    let key = idempotency_key();
    let written = send(|| resources.files_create(directory, name, contents.to_vec(), &key)).await?;
    Ok(written.resource_id)
}

// ---- Network ----

/// What a fetch tells a plugin to do next.
#[derive(Debug)]
pub enum FetchFailure {
    /// The origin is not one the user approved, or the server sent the fetch somewhere that is
    /// not approved. Nothing says where. Ask the user to approve the origin you need.
    NotApproved,
    /// The host is busy (too many fetches at once) or the response was over the budget you set.
    /// Retry later for the former; raise `max_response_bytes` (up to 8 MiB) for the latter.
    Busy(Status),
    Other(Status),
}

/// Fetches `url` (https, an origin the user approved) and returns the answer whatever its HTTP
/// status: a 404 is a normal answer. The host never sends a GameNode credential; add your own
/// `Authorization` header when your service needs one. The deadline covers redirects too.
pub async fn fetch_bytes(
    resources: &Resources,
    url: &str,
    budget_bytes: u64,
) -> Result<Fetched, FetchFailure> {
    let fetch = Fetch::get(url)
        .header("Accept", "application/json")
        .max_response_bytes(budget_bytes)
        .timeout(Duration::from_secs(10));
    resources
        .network_fetch(&fetch)
        .await
        .map_err(|status| match status.code() {
            Code::PermissionDenied => FetchFailure::NotApproved,
            Code::ResourceExhausted => FetchFailure::Busy(status),
            _ => FetchFailure::Other(status),
        })
}

// ---- Notifications ----

/// How long the host asked to wait, from a `RESOURCE_EXHAUSTED` answer.
pub fn retry_after(status: &Status) -> Option<Duration> {
    status
        .metadata()
        .get("retry-after-ms")?
        .to_str()
        .ok()?
        .parse()
        .map(Duration::from_millis)
        .ok()
}

/// Tells the user something in their language, as plain text from your plugin. GameNode picks
/// the translation that matches, then the same language without a region, then the main text.
/// Returns `Ok(false)` when the rate limit (5 a minute) says to wait; drop the notice or merge
/// it with the next one rather than queueing a burst. A retry after a lost answer reuses the key
/// and does not show the notice twice.
pub async fn tell_user(
    resources: &Resources,
    english: (&str, &str),
    portuguese: (&str, &str),
    severity: Severity,
) -> Result<bool, Status> {
    let notice = Notice::new("en", english.0)
        .body(english.1)
        .severity(severity)
        .translation("pt-BR", portuguese.0, portuguese.1);
    let key = idempotency_key();
    match send(|| resources.notify(&notice, &key)).await {
        Ok(_) => Ok(true),
        Err(status) if status.code() == Code::ResourceExhausted => Ok(false),
        Err(status) => Err(status),
    }
}

// ---- Put together (the examples of the development guide) ----

/// Counts a launch in a value that survives restarts. It reads the value and its revision,
/// writes with that revision, and reads again after a conflict.
pub async fn count_launch(resources: &Resources) -> Result<(), Status> {
    update(resources, "launches", Durability::Persistent, None, |old| {
        let count = old
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| text.parse::<u64>().ok())
            .unwrap_or(0);
        (count + 1).to_string().into_bytes()
    })
    .await
    .map(|_| ())
}

/// Reads a save file of one of your own entries. The user chooses and confirms the folder in
/// GameNode and you never see a path; ask for `read` first and for `write` only when you need it.
pub async fn read_slot(
    resources: &Resources,
    installation_id: &str,
) -> Result<Option<Vec<u8>>, Status> {
    let Ok(root) = open_installation(resources, installation_id, &["read"]).await else {
        return Ok(None); // declined or not available yet: carry on without it
    };
    read_named(resources, &root, "slot1.sav", 1 << 20).await
}

fn main() {}
