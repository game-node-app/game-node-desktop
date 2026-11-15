//! Content fingerprints of approved places and their comparison (task 13.4, design section 12).
//!
//! A fingerprint is built from the *content* of the files, read again every time: a SHA-256 for
//! each file and one digest for the whole tree. Size and modification time are never taken as
//! proof of equality in either direction. The tree digest (`sha256-tree-v1`) is the SHA-256 of a
//! domain tag followed, for each entry in the order of its label's UTF-8 bytes, by the label's
//! length (8 bytes, big endian) and bytes, a type byte (0 directory, 1 file), the logical size
//! (8 bytes, big endian) and the file's own SHA-256 (32 zero bytes for a directory). Empty
//! directories are entries, case is kept, and a rename is a removal plus an addition. Dates and
//! permissions are metadata, not input.
//!
//! A snapshot keeps every entry's digest so two of them can be compared. It says whether it is
//! *complete*: a file that could not be read, changed while it was read, or a walk that was cut
//! makes it incomplete, and an incomplete snapshot never yields a digest or an "unchanged"
//! verdict. The consistency is `best_effort`: the files are not frozen while they are read.
//!
//! Snapshots live in the host's memory, a few per plugin, for an hour.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tonic::Status;

use super::jobs::JobOwner;

/// The algorithm id a plugin asks for as `sha256` and gets back as this.
pub const ALGORITHM: &str = "sha256-tree-v1";
/// Snapshots one plugin keeps; the oldest go first.
pub const MAX_SNAPSHOTS_PER_OWNER: usize = 8;
/// How long a snapshot is kept.
pub const SNAPSHOT_TTL: Duration = Duration::from_secs(60 * 60);
/// Entries a fingerprint covers when the request does not say, and the most it may be asked to.
pub const DEFAULT_ENTRIES: usize = 10_000;
pub const MAX_ENTRIES: usize = 50_000;
/// The most bytes one fingerprint reads before it stops and says it is incomplete.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// The longest one fingerprint runs.
pub const MAX_TIME: Duration = Duration::from_secs(10 * 60);
/// How long a request waits for its fingerprint before answering with the job.
pub const FAST_WAIT: Duration = Duration::from_secs(2);
/// Fingerprints one plugin may have running, and all plugins together: reading is disk work.
pub const RUNNING_PER_OWNER: usize = 1;
pub const RUNNING_TOTAL: usize = 2;

/// The job code of a finished fingerprint; its message is the snapshot id.
pub const CODE_READY: &str = "fingerprint_ready";

/// One entry of a snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The entry's path below the place, `/`-separated; for a profile `d<n>/` comes first.
    pub label: String,
    /// What to seal a handle from, for an entry that still exists.
    pub approval_id: String,
    pub path: Vec<String>,
    pub directory: bool,
    pub size: u64,
    /// The file's SHA-256; `None` for a directory or a file that could not be hashed.
    pub digest: Option<[u8; 32]>,
    /// Could not be read.
    pub unreadable: bool,
    /// Changed while it was read.
    pub unstable: bool,
}

/// A fingerprint of a place at a moment.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub id: String,
    /// The permission that inspecting the place needs; comparing needs it too.
    pub permission: &'static str,
    /// What was fingerprinted: the place and its selection. Two snapshots compare only if equal.
    pub scope: String,
    pub revision: u64,
    pub entries: Vec<Record>,
    pub complete: bool,
    pub unstable: bool,
    /// The tree digest; only for a complete snapshot.
    pub digest: Option<[u8; 32]>,
    /// Unix seconds when the observation began and ended.
    pub started_at: i64,
    pub finished_at: i64,
}

/// The digest of the tree these entries make (see the module documentation).
pub fn tree_digest(entries: &[Record]) -> [u8; 32] {
    let mut sorted: Vec<&Record> = entries.iter().collect();
    sorted.sort_by(|a, b| a.label.as_bytes().cmp(b.label.as_bytes()));
    let mut hash = Sha256::new();
    hash.update(b"gamenode.location.tree.v1\0");
    for entry in sorted {
        hash.update((entry.label.len() as u64).to_be_bytes());
        hash.update(entry.label.as_bytes());
        hash.update([u8::from(!entry.directory)]);
        hash.update(entry.size.to_be_bytes());
        hash.update(entry.digest.unwrap_or([0; 32]));
    }
    hash.finalize().into()
}

/// How two snapshots compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Both complete and identical.
    Unchanged,
    /// Both complete and different.
    Changed,
    /// One of them is incomplete or unstable: what differs is real, but nothing is certified.
    Indeterminate,
    /// They do not describe the same place, selection, algorithm or revision.
    Incomparable,
}

impl Verdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Changed => "changed",
            Self::Indeterminate => "indeterminate",
            Self::Incomparable => "incomparable",
        }
    }
}

/// How an entry differs. Declared in the order two changes of one label are listed in: what went
/// away comes before what took its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Removed,
    Added,
    Modified,
}

impl Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Modified => "modified",
        }
    }
}

/// One entry that differs.
#[derive(Clone, Debug)]
pub struct Change {
    pub label: String,
    pub kind: Kind,
    pub previous_size: u64,
    pub current_size: u64,
    /// The entry as it is now, when it still exists.
    pub current: Option<Record>,
}

pub struct Comparison {
    pub status: Verdict,
    /// Sorted by label.
    pub changes: Vec<Change>,
}

/// Compares `before` with `after`. A file that became a directory (or the reverse) is a removal
/// plus an addition; a file is modified when its content digest or size differs, and an entry
/// that could not be read in either snapshot is never reported as unchanged.
pub fn compare(before: &Snapshot, after: &Snapshot) -> Comparison {
    if before.scope != after.scope || before.revision != after.revision {
        return Comparison {
            status: Verdict::Incomparable,
            changes: Vec::new(),
        };
    }
    let old: HashMap<&str, &Record> = before
        .entries
        .iter()
        .map(|r| (r.label.as_str(), r))
        .collect();
    let new: HashMap<&str, &Record> = after
        .entries
        .iter()
        .map(|r| (r.label.as_str(), r))
        .collect();
    let mut changes = Vec::new();
    for (label, record) in &new {
        match old.get(label) {
            None => changes.push(Change {
                label: (*label).to_owned(),
                kind: Kind::Added,
                previous_size: 0,
                current_size: record.size,
                current: Some((*record).clone()),
            }),
            Some(previous) if previous.directory != record.directory => {
                changes.push(Change {
                    label: (*label).to_owned(),
                    kind: Kind::Removed,
                    previous_size: previous.size,
                    current_size: 0,
                    current: None,
                });
                changes.push(Change {
                    label: (*label).to_owned(),
                    kind: Kind::Added,
                    previous_size: 0,
                    current_size: record.size,
                    current: Some((*record).clone()),
                });
            }
            Some(previous) => {
                let same = !record.directory
                    && previous.size == record.size
                    && previous.digest.is_some()
                    && previous.digest == record.digest
                    && !previous.unreadable
                    && !record.unreadable;
                // An entry that could not be read in either snapshot is not known to have changed.
                if !record.directory && !same && !previous.unreadable && !record.unreadable {
                    changes.push(Change {
                        label: (*label).to_owned(),
                        kind: Kind::Modified,
                        previous_size: previous.size,
                        current_size: record.size,
                        current: Some((*record).clone()),
                    });
                }
            }
        }
    }
    for (label, record) in &old {
        if !new.contains_key(label) {
            changes.push(Change {
                label: (*label).to_owned(),
                kind: Kind::Removed,
                previous_size: record.size,
                current_size: 0,
                current: None,
            });
        }
    }
    changes.sort_by(|a, b| {
        a.label
            .as_bytes()
            .cmp(b.label.as_bytes())
            .then((a.kind as u8).cmp(&(b.kind as u8)))
    });
    let status = if !before.complete || !after.complete || before.unstable || after.unstable {
        Verdict::Indeterminate
    } else if changes.is_empty() && before.digest == after.digest {
        Verdict::Unchanged
    } else {
        Verdict::Changed
    };
    Comparison { status, changes }
}

type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

struct Stored {
    owner: JobOwner,
    snapshot: Arc<Snapshot>,
    at: Instant,
}

/// The snapshots the host holds and the fingerprints it is running.
pub struct Snapshots {
    stored: Mutex<HashMap<String, Stored>>,
    running: Mutex<HashMap<JobOwner, usize>>,
    clock: Clock,
    #[cfg(test)]
    hook: Mutex<Option<Hook>>,
    #[cfg(test)]
    fast_wait: Mutex<Option<Duration>>,
}

/// Called after each file is hashed, so a test can change it as a concurrent writer would.
pub type Hook = Arc<dyn Fn(&[String]) + Send + Sync>;

impl Default for Snapshots {
    fn default() -> Self {
        Self {
            stored: Mutex::default(),
            running: Mutex::default(),
            clock: Arc::new(Instant::now),
            #[cfg(test)]
            hook: Mutex::default(),
            #[cfg(test)]
            fast_wait: Mutex::default(),
        }
    }
}

/// Releases a running-fingerprint slot when dropped.
pub struct Running {
    snapshots: Arc<Snapshots>,
    owner: JobOwner,
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut running = self
            .snapshots
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(count) = running.get_mut(&self.owner) {
            *count -= 1;
            if *count == 0 {
                running.remove(&self.owner);
            }
        }
    }
}

impl Snapshots {
    #[cfg(test)]
    pub(crate) fn with_clock(
        mut self,
        clock: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// Makes the next fingerprints call `hook` after hashing each file.
    #[cfg(test)]
    pub(crate) fn set_hook(&self, hook: impl Fn(&[String]) + Send + Sync + 'static) {
        *self.hook.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(hook));
    }

    /// How long a request waits for its fingerprint before answering with the job.
    pub fn fast_wait(&self) -> Duration {
        #[cfg(test)]
        if let Some(wait) = *self.fast_wait.lock().unwrap_or_else(|p| p.into_inner()) {
            return wait;
        }
        FAST_WAIT
    }

    /// Shortens the wait of the next requests, for tests of the job path.
    #[cfg(test)]
    pub(crate) fn set_fast_wait(&self, wait: Duration) {
        *self.fast_wait.lock().unwrap_or_else(|p| p.into_inner()) = Some(wait);
    }

    /// The test hook, or nothing outside tests.
    pub fn after_hash(&self) -> Option<Hook> {
        #[cfg(test)]
        {
            self.hook.lock().unwrap_or_else(|p| p.into_inner()).clone()
        }
        #[cfg(not(test))]
        {
            None
        }
    }

    /// Claims the right to run a fingerprint, or refuses when this plugin already has one running
    /// or the host as a whole has [`RUNNING_TOTAL`].
    pub fn begin(self: &Arc<Self>, owner: &JobOwner) -> Result<Running, Status> {
        let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
        if running.get(owner).copied().unwrap_or(0) >= RUNNING_PER_OWNER {
            return Err(Status::resource_exhausted(
                "a fingerprint of this plugin is already running; wait for it or cancel it",
            ));
        }
        if running.values().sum::<usize>() >= RUNNING_TOTAL {
            return Err(Status::resource_exhausted(
                "the host is fingerprinting as much as it allows; try again later",
            ));
        }
        *running.entry(owner.clone()).or_default() += 1;
        Ok(Running {
            snapshots: Arc::clone(self),
            owner: owner.clone(),
        })
    }

    /// Keeps `snapshot` for `owner`, dropping what expired and the owner's oldest beyond the cap.
    pub fn keep(&self, owner: &JobOwner, snapshot: Snapshot) -> Arc<Snapshot> {
        let now = (self.clock)();
        let snapshot = Arc::new(snapshot);
        let mut stored = self.stored.lock().unwrap_or_else(|p| p.into_inner());
        stored.retain(|_, kept| now.duration_since(kept.at) < SNAPSHOT_TTL);
        let own: Vec<_> = stored
            .iter()
            .filter(|(_, kept)| kept.owner == *owner)
            .map(|(id, kept)| (kept.at, id.clone()))
            .collect();
        if own.len() >= MAX_SNAPSHOTS_PER_OWNER {
            let mut own = own;
            own.sort();
            for (_, id) in &own[..own.len() + 1 - MAX_SNAPSHOTS_PER_OWNER] {
                stored.remove(id);
            }
        }
        stored.insert(
            snapshot.id.clone(),
            Stored {
                owner: owner.clone(),
                snapshot: snapshot.clone(),
                at: now,
            },
        );
        snapshot
    }

    /// The owner's snapshot `id`; another owner's, an unknown one and an expired one are the same.
    pub fn get(&self, owner: &JobOwner, id: &str) -> Option<Arc<Snapshot>> {
        let now = (self.clock)();
        let mut stored = self.stored.lock().unwrap_or_else(|p| p.into_inner());
        stored.retain(|_, kept| now.duration_since(kept.at) < SNAPSHOT_TTL);
        stored
            .get(id)
            .filter(|kept| kept.owner == *owner)
            .map(|kept| kept.snapshot.clone())
    }
}

/// A new snapshot id.
pub fn new_snapshot_id() -> Result<String, Status> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Status::internal("no source of randomness"))?;
    Ok(format!(
        "snap-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

#[cfg(test)]
mod tests;
