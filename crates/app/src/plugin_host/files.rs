//! Host side of the plugins' file access: who approves a folder, how handles are issued, and
//! which games a folder affects.
//!
//! A plugin never names a path. It asks (`FilesRequestAccess`) for access to one installation of
//! a game; the host asks the *user*, through a [`RootApprover`], to choose the folder and to
//! confirm the final scope, and keeps the approval per plugin and account profile
//! ([`gamenode_platform::plugin_storage::FileApproval`]). The plugin gets opaque handles: sealed
//! tokens ([`gamenode_platform::sealed::Sealer`]) that carry the approval and a relative path,
//! bound to the session that received them, so they cannot be read, forged, edited, shared with
//! another session or kept past the session. Every use re-derives what the handle may do from
//! the stored approval, the plugin's current grants and the folder as it is on disk now.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use gamenode_platform::plugin_files::Reserved;
use gamenode_platform::sealed::Sealer;
use tonic::Status;

use super::library::{EntryFolder, LibraryFeed};

/// What a folder is for. A plugin may ask for any of them for one installation.
pub const KINDS: [&str; 3] = ["installation", "save", "prefix"];
/// The most bytes one read returns when the caller names no limit.
pub const DEFAULT_READ_BYTES: usize = 1024 * 1024;
/// The most bytes one read returns, and the most one write carries.
pub const MAX_CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// How long the user has to answer an approval before it counts as declined.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// How long the canonical form of a library folder is remembered.
const FOLDER_TTL: Duration = Duration::from_secs(60);
const HANDLE: u8 = 1;
const CURSOR: u8 = 2;
/// The longest relative path a handle carries, in bytes.
const MAX_HANDLE_PATH: usize = 2048;

/// What the host asks the user to choose.
#[derive(Clone, Debug)]
pub struct RootRequest {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    /// The library name of the installation, never a path.
    pub installation_name: String,
    pub kind: String,
    /// `read`, `write` and `delete`, the ones the plugin asked for.
    pub operations: Vec<&'static str>,
    /// Where the installation lives, to start the choice there; `None` when unknown. The host
    /// knows it from the library, so it is trustworthy (installation kind only).
    pub suggested: Option<PathBuf>,
    /// Folders the plugin suggested for this kind, in the plugin's priority order. They are
    /// **declared by the plugin and not verified**: the dialog must label them so, and a plugin
    /// can neither learn which exist nor which the user took.
    pub declared: Vec<DeclaredFolder>,
    /// The folder to start on: the host's own suggestion, else the first declared one that
    /// exists. `None` when there is nothing to propose.
    pub preselected: Option<PathBuf>,
}

/// A folder a plugin suggested, expanded for this system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredFolder {
    pub path: PathBuf,
    /// One line of plain text from the plugin; show it as text, never as markup.
    pub description: String,
    /// Whether the folder exists now (checked only for its existence; nothing was listed or read).
    pub exists: bool,
}

/// A game that shares the chosen folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AffectedGame {
    pub name: String,
    pub game_id: Option<u64>,
}

/// The final scope the user confirms: exactly what the approval will allow.
#[derive(Clone, Debug)]
pub struct RootScope {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub installation_name: String,
    pub kind: String,
    pub operations: Vec<&'static str>,
    pub root: PathBuf,
    /// Whether the chosen folder is one the plugin suggested (compared by canonical path), as
    /// opposed to one the user picked or the host suggested.
    pub declared_by_plugin: bool,
    /// Every library entry whose folder is the root, inside it or contains it: the complete
    /// set the approval covers.
    pub affected: Vec<AffectedGame>,
}

/// The user's side of a file approval. The host calls it off the UI thread, and a call that
/// takes longer than [`APPROVAL_TIMEOUT`] counts as declined.
#[async_trait]
pub trait RootApprover: Send + Sync {
    /// Lets the user choose the folder. `None` is a refusal.
    async fn choose_root(&self, request: RootRequest) -> Result<Option<PathBuf>, Status>;
    /// Shows the final scope and asks for consent.
    async fn confirm(&self, scope: RootScope) -> Result<bool, Status>;
}

/// File access state shared by every plugin session.
pub struct FileAccess {
    pub(crate) reserved: Reserved,
    approver: Mutex<Option<Arc<dyn RootApprover>>>,
    sealer: Sealer,
    waiting: Mutex<HashSet<[u8; 32]>>,
    canonical: Mutex<HashMap<String, (Instant, Option<PathBuf>)>>,
}

impl Default for FileAccess {
    fn default() -> Self {
        Self::new(Reserved::default())
    }
}

impl FileAccess {
    /// `reserved` are the host folders no plugin may reach, such as the application data folder.
    pub fn new(reserved: Reserved) -> Self {
        Self {
            reserved,
            approver: Mutex::default(),
            sealer: Sealer::new(),
            waiting: Mutex::default(),
            canonical: Mutex::default(),
        }
    }

    /// Connects the user's side. Until it is set, a new approval cannot be made (existing ones
    /// keep working).
    pub fn set_approver(&self, approver: Arc<dyn RootApprover>) {
        *self.approver.lock().unwrap_or_else(|p| p.into_inner()) = Some(approver);
    }

    pub(crate) fn approver(&self) -> Option<Arc<dyn RootApprover>> {
        self.approver
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Marks an approval of this namespace as waiting for the user. Only one at a time: the
    /// user is never asked twice at once by one plugin.
    pub(crate) fn begin_approval(self: &Arc<Self>, owner: [u8; 32]) -> Option<Waiting> {
        let mut waiting = self.waiting.lock().unwrap_or_else(|p| p.into_inner());
        waiting.insert(owner).then(|| Waiting {
            access: self.clone(),
            owner,
        })
    }

    /// A handle for `path` below the approval `approval_id`, usable only by `session_id`.
    pub(crate) fn seal_handle(
        &self,
        session_id: &str,
        approval_id: &str,
        path: &[String],
    ) -> String {
        let mut payload = approval_id.as_bytes().to_vec();
        for part in path {
            payload.push(0);
            payload.extend_from_slice(part.as_bytes());
        }
        self.sealer.seal(HANDLE, session_id, &payload)
    }

    /// The approval id and relative path in a handle this session was given.
    pub(crate) fn open_handle(
        &self,
        session_id: &str,
        handle: &str,
    ) -> Option<(String, Vec<String>)> {
        let payload = self.sealer.open(HANDLE, session_id, handle)?;
        if payload.len() > MAX_HANDLE_PATH + 256 {
            return None;
        }
        let mut parts = payload.split(|byte| *byte == 0);
        let approval = String::from_utf8(parts.next()?.to_vec()).ok()?;
        let path = parts
            .map(|part| String::from_utf8(part.to_vec()).ok())
            .collect::<Option<Vec<_>>>()?;
        Some((approval, path))
    }

    /// Whether a path of this size can be put in a handle.
    pub(crate) fn fits_handle(path: &[String]) -> bool {
        path.iter().map(|part| part.len() + 1).sum::<usize>() <= MAX_HANDLE_PATH
    }

    /// A paging cursor over the directory behind `handle`, resuming after `last`.
    pub(crate) fn seal_cursor(&self, session_id: &str, handle: &str, last: &str) -> String {
        self.sealer
            .seal(CURSOR, &format!("{session_id}\n{handle}"), last.as_bytes())
    }

    pub(crate) fn open_cursor(
        &self,
        session_id: &str,
        handle: &str,
        cursor: &str,
    ) -> Option<String> {
        let payload = self
            .sealer
            .open(CURSOR, &format!("{session_id}\n{handle}"), cursor)?;
        String::from_utf8(payload).ok()
    }

    /// The library entries whose folder is `root`, lies inside it or contains it, in entry-key
    /// order. Blocking: canonicalizes library folders (briefly remembered).
    pub(crate) fn overlapping(&self, feed: &LibraryFeed, root: &Path) -> Vec<EntryFolder> {
        let now = Instant::now();
        let mut cache = self.canonical.lock().unwrap_or_else(|p| p.into_inner());
        cache.retain(|_, (at, _)| now.duration_since(*at) < FOLDER_TTL);
        feed.folders()
            .into_iter()
            .filter(|entry| {
                let (_, canonical) = cache
                    .entry(entry.folder.clone())
                    .or_insert_with(|| (now, std::fs::canonicalize(&entry.folder).ok()));
                // A folder that no longer exists is compared as written.
                let folder = canonical
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(&entry.folder));
                folder.starts_with(root) || root.starts_with(&folder)
            })
            .collect()
    }
}

/// Releases a namespace's pending-approval slot when dropped.
pub(crate) struct Waiting {
    access: Arc<FileAccess>,
    owner: [u8; 32],
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.access
            .waiting
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.owner);
    }
}

/// The sorted entry keys of `entries`, the form an approval records.
pub(crate) fn affected_keys(entries: &[EntryFolder]) -> Vec<String> {
    let mut keys: Vec<String> = entries
        .iter()
        .map(|entry| entry.entry_key.clone())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

#[cfg(all(test, unix))]
mod tests;
