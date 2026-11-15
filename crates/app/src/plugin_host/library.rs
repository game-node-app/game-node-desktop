//! Host-owned projection of the game library for plugin reads (`Library.List/Get/Watch`).
//!
//! The engine feeds every library change as plain [`LibrarySource`] rows. A row becomes visible
//! to plugins only once its opaque installation id is durable (assigned by the library store),
//! so an id a plugin has seen never changes for its entry. The projection carries descriptive
//! fields only: never the store id, install folder, scan fields or anything account-private.
//!
//! Revisions are global and monotonic. Each change batch advances the revision once; its events
//! carry that revision. A bounded history lets watchers resume; older positions must resync.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gamenode_core::library::{ArtworkWrite, DisplayWrite};
use gamenode_platform::plugin_artwork::Prepared;
use gamenode_plugin_protocol::proto::LibraryEntry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use tonic::Status;

/// The most entries one page may return.
pub const MAX_PAGE_SIZE: usize = 200;
/// The page size used when a request leaves it unset.
pub const DEFAULT_PAGE_SIZE: usize = 50;
/// How many recent events a watcher can resume from.
pub const HISTORY_EVENTS: usize = 256;
/// How long a list cursor stays valid.
pub const CURSOR_TTL: Duration = Duration::from_secs(5 * 60);
/// Longest accepted search text, in bytes.
pub const MAX_SEARCH_BYTES: usize = 256;
/// Longest accepted cursor, in bytes.
const MAX_CURSOR_BYTES: usize = 2048;
/// After a failed id request, syncs wait this long before asking again.
const ID_RETRY: Duration = Duration::from_secs(30);

/// Whether the entry is installed on this PC, or only owned or shared with the account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallState {
    Installed,
    Owned,
    Shared,
}

impl InstallState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Owned => "owned",
            Self::Shared => "shared",
        }
    }
}

/// One visible library entry as the engine sees it, before it has an installation id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarySource {
    pub entry_key: String,
    pub provider_id: String,
    /// The current GameNode match, if exactly one game is matched.
    pub game_id: Option<u64>,
    pub name: String,
    pub install_state: InstallState,
    /// The provider plugin's presentation override, if one is set.
    pub display_name: Option<String>,
    /// Revision of that override for this entry; 0 until it is first written.
    pub metadata_revision: u64,
    /// Revision of the provider plugin's cover override for this entry; 0 until it is first
    /// written.
    pub artwork_revision: u64,
    /// Where the entry is installed, or empty when unknown. Host-only: it is never part of the
    /// projection a plugin reads; it lets file approvals find the games that share a folder.
    pub install_dir: String,
}

/// One entry's install folder, for file approvals. Host-only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryFolder {
    pub entry_key: String,
    pub game_id: Option<u64>,
    pub name: String,
    pub folder: String,
}

/// One entry as plugins see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projected {
    pub installation_id: String,
    pub provider_id: String,
    pub game_id: Option<u64>,
    pub name: String,
    pub install_state: InstallState,
    pub display_name: Option<String>,
    pub metadata_revision: u64,
    pub artwork_revision: u64,
    /// The revision of the entry's last change.
    pub revision: u64,
}

impl Projected {
    pub fn to_proto(&self) -> LibraryEntry {
        LibraryEntry {
            game_id: self.game_id.map(|id| id.to_string()).unwrap_or_default(),
            installation_id: self.installation_id.clone(),
            provider_id: self.provider_id.clone(),
            name: self.name.clone(),
            version: String::new(),
            install_state: self.install_state.as_str().to_owned(),
            revision: self.revision,
            display_name: self.display_name.clone().unwrap_or_default(),
            metadata_revision: self.metadata_revision,
            artwork_revision: self.artwork_revision,
        }
    }

    fn same_content(&self, other: &Self) -> bool {
        self.provider_id == other.provider_id
            && self.game_id == other.game_id
            && self.name == other.name
            && self.install_state == other.install_state
            && self.display_name == other.display_name
            && self.metadata_revision == other.metadata_revision
            && self.artwork_revision == other.artwork_revision
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Upserted,
    Removed,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upserted => "upserted",
            Self::Removed => "removed",
        }
    }
}

/// A change event. A removal carries the entry as it was last seen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub revision: u64,
    pub kind: ChangeKind,
    pub entry: Projected,
}

/// Who may see which entries: the caller's own provider, or every provider with
/// `library.read.all`. An optional provider filter narrows either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub own_provider: String,
    pub read_all: bool,
    pub provider_filter: Option<String>,
}

impl Scope {
    pub fn admits(&self, entry: &Projected) -> bool {
        (self.read_all || entry.provider_id == self.own_provider)
            && self
                .provider_filter
                .as_ref()
                .is_none_or(|provider| *provider == entry.provider_id)
    }
}

/// Why a watcher cannot resume from the position it asked for.
#[derive(Debug, PartialEq, Eq)]
pub enum ResumeError {
    /// The position is older than the retained history; list again and watch from there.
    Resync,
    /// The position is newer than anything this host issued.
    Future,
}

#[derive(Default)]
struct FeedState {
    revision: u64,
    /// The engine's latest rows by entry key.
    sources: BTreeMap<String, LibrarySource>,
    /// Durable installation ids by entry key.
    ids: HashMap<String, String>,
    /// The entry key of each installation id.
    keys: HashMap<String, String>,
    /// Keys whose ids are being requested.
    requested: BTreeSet<String>,
    /// No new id request before this instant, after a failure.
    retry_after: Option<std::time::Instant>,
    /// The projection by installation id; this order is the list order.
    entries: BTreeMap<String, Projected>,
    history: VecDeque<Change>,
    /// Events at or below this revision are no longer retained.
    floor: u64,
}

/// The shared projection. Cheap to read; writes come from the engine.
pub struct LibraryFeed {
    state: Mutex<FeedState>,
    host: Mutex<Option<Arc<dyn LibraryHost>>>,
    live: broadcast::Sender<Change>,
    cursor_key: [u8; 32],
}

impl Default for LibraryFeed {
    fn default() -> Self {
        let mut cursor_key = [0; 32];
        // A per-process key: cursors from an earlier run are rejected as stale.
        if getrandom::fill(&mut cursor_key).is_err() {
            tracing::error!("could not create the library cursor key");
        }
        Self {
            state: Mutex::default(),
            host: Mutex::default(),
            live: broadcast::channel(HISTORY_EVENTS).0,
            cursor_key,
        }
    }
}

impl LibraryFeed {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Connects the engine's write side. Without it, mutating services are unavailable.
    pub fn set_host(&self, host: Arc<dyn LibraryHost>) {
        *self.host.lock().unwrap_or_else(|p| p.into_inner()) = Some(host);
    }

    pub fn host(&self) -> Option<Arc<dyn LibraryHost>> {
        self.host.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn lock(&self) -> MutexGuard<'_, FeedState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Replaces the engine rows. Returns the keys that still need an installation id and are not
    /// already being requested; the caller requests them and reports back with
    /// [`assign_ids`](Self::assign_ids) or [`ids_failed`](Self::ids_failed).
    pub fn sync(&self, sources: Vec<LibrarySource>) -> Vec<String> {
        let mut state = self.lock();
        let sources: BTreeMap<String, LibrarySource> = sources
            .into_iter()
            .map(|source| (source.entry_key.clone(), source))
            .collect();
        let changed = sources != state.sources;
        state.sources = sources;
        let waiting = state
            .retry_after
            .is_some_and(|at| std::time::Instant::now() < at);
        let missing: Vec<String> = state
            .sources
            .keys()
            .filter(|_| !waiting)
            .filter(|key| !state.ids.contains_key(*key) && !state.requested.contains(*key))
            .cloned()
            .collect();
        state.requested.extend(missing.iter().cloned());
        if changed {
            self.rebuild(&mut state);
        }
        missing
    }

    /// Records the durable ids answered for `requested`. An id already known for a key is kept;
    /// a requested key without an answer is asked for again by a later sync.
    pub fn assign_ids(&self, requested: &[String], ids: Vec<(String, String)>) {
        let mut state = self.lock();
        for key in requested {
            state.requested.remove(key);
        }
        for (key, id) in ids {
            if !state.ids.contains_key(&key) {
                state.keys.insert(id.clone(), key.clone());
                state.ids.insert(key, id);
            }
        }
        self.rebuild(&mut state);
    }

    /// Releases a failed request so the next sync asks again. Those entries stay hidden.
    pub fn ids_failed(&self, keys: &[String]) {
        let mut state = self.lock();
        for key in keys {
            state.requested.remove(key);
        }
        state.retry_after = Some(std::time::Instant::now() + ID_RETRY);
    }

    fn rebuild(&self, state: &mut FeedState) {
        let next = state.revision + 1;
        let mut fresh: BTreeMap<String, Projected> = BTreeMap::new();
        for source in state.sources.values() {
            let Some(id) = state.ids.get(&source.entry_key) else {
                continue;
            };
            fresh.insert(
                id.clone(),
                Projected {
                    installation_id: id.clone(),
                    provider_id: source.provider_id.clone(),
                    game_id: source.game_id,
                    name: source.name.clone(),
                    install_state: source.install_state,
                    display_name: source.display_name.clone(),
                    metadata_revision: source.metadata_revision,
                    artwork_revision: source.artwork_revision,
                    revision: next,
                },
            );
        }
        let mut changes = Vec::new();
        for (id, entry) in &mut fresh {
            match state.entries.get(id) {
                Some(old) if old.same_content(entry) => entry.revision = old.revision,
                _ => changes.push(Change {
                    revision: next,
                    kind: ChangeKind::Upserted,
                    entry: entry.clone(),
                }),
            }
        }
        for (id, old) in &state.entries {
            if !fresh.contains_key(id) {
                let mut entry = old.clone();
                entry.revision = next;
                changes.push(Change {
                    revision: next,
                    kind: ChangeKind::Removed,
                    entry,
                });
            }
        }
        if changes.is_empty() {
            return;
        }
        state.revision = next;
        state.entries = fresh;
        for change in changes {
            if state.history.len() == HISTORY_EVENTS
                && let Some(dropped) = state.history.pop_front()
            {
                state.floor = state.floor.max(dropped.revision);
            }
            state.history.push_back(change.clone());
            // No receivers is fine; lagging receivers learn it from the channel.
            let _ = self.live.send(change);
        }
    }

    pub fn revision(&self) -> u64 {
        self.lock().revision
    }

    /// One page of the entries the scope admits, in installation-id order after `after`.
    /// Returns the current revision, the page and whether more entries follow.
    pub fn page(
        &self,
        scope: &Scope,
        search: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> (u64, Vec<Projected>, bool) {
        let state = self.lock();
        let search = search.map(str::to_lowercase);
        let range = match after {
            Some(after) => state
                .entries
                .range::<str, _>((std::ops::Bound::Excluded(after), std::ops::Bound::Unbounded)),
            None => state.entries.range::<str, _>(..),
        };
        let mut page = Vec::new();
        let mut more = false;
        for entry in range.map(|(_, entry)| entry).filter(|entry| {
            scope.admits(entry)
                && search
                    .as_ref()
                    .is_none_or(|text| entry.name.to_lowercase().contains(text))
        }) {
            if page.len() == limit {
                more = true;
                break;
            }
            page.push(entry.clone());
        }
        (state.revision, page, more)
    }

    /// The entry with this installation id, if the scope admits it.
    pub fn get(&self, scope: &Scope, installation_id: &str) -> (u64, Option<Projected>) {
        let state = self.lock();
        let entry = state
            .entries
            .get(installation_id)
            .filter(|entry| scope.admits(entry))
            .cloned();
        (state.revision, entry)
    }

    /// The install folder of every entry that has one, in entry-key order. Host-only.
    pub fn folders(&self) -> Vec<EntryFolder> {
        self.lock()
            .sources
            .values()
            .filter(|source| !source.install_dir.trim().is_empty())
            .map(|source| EntryFolder {
                entry_key: source.entry_key.clone(),
                game_id: source.game_id,
                name: source.name.clone(),
                folder: source.install_dir.clone(),
            })
            .collect()
    }

    /// The install folder of the entry with this key, if it has one. Host-only.
    pub fn folder_of(&self, entry_key: &str) -> Option<String> {
        self.lock()
            .sources
            .get(entry_key)
            .map(|source| source.install_dir.clone())
            .filter(|folder| !folder.trim().is_empty())
    }

    /// The current revision of the entry with this entry key, once it has an installation id.
    pub fn revision_of(&self, entry_key: &str) -> Option<u64> {
        let state = self.lock();
        let id = state.ids.get(entry_key)?;
        state.entries.get(id).map(|entry| entry.revision)
    }

    /// The entry key and projection of `installation_id`, if the scope admits it. Host-side only:
    /// the entry key never reaches a plugin.
    pub fn target(&self, scope: &Scope, installation_id: &str) -> Option<(String, Projected)> {
        let state = self.lock();
        let entry = state
            .entries
            .get(installation_id)
            .filter(|entry| scope.admits(entry))?;
        let key = state.keys.get(installation_id)?;
        Some((key.clone(), entry.clone()))
    }

    /// The admitted entries matched to `game_id`.
    pub fn by_game(&self, scope: &Scope, game_id: u64) -> (u64, Vec<Projected>) {
        let state = self.lock();
        let entries = state
            .entries
            .values()
            .filter(|entry| entry.game_id == Some(game_id) && scope.admits(entry))
            .cloned()
            .collect();
        (state.revision, entries)
    }

    /// Subscribes after `after_revision`: zero starts at the current revision with no backlog;
    /// otherwise the retained events after that revision are returned first. The receiver is
    /// taken under the same lock, so no event falls between backlog and live delivery.
    pub fn subscribe(
        &self,
        after_revision: u64,
    ) -> Result<(u64, Vec<Change>, broadcast::Receiver<Change>), ResumeError> {
        let state = self.lock();
        let receiver = self.live.subscribe();
        if after_revision == 0 {
            return Ok((state.revision, Vec::new(), receiver));
        }
        if after_revision > state.revision {
            return Err(ResumeError::Future);
        }
        if after_revision < state.floor {
            return Err(ResumeError::Resync);
        }
        let backlog = state
            .history
            .iter()
            .filter(|change| change.revision > after_revision)
            .cloned()
            .collect();
        Ok((state.revision, backlog, receiver))
    }

    /// Signs a cursor for the page after `after`.
    pub fn issue_cursor(&self, binding: &CursorBinding, revision: u64, after: &str) -> String {
        let claims = CursorClaims {
            v: 1,
            binding: binding.digest(),
            revision,
            after: after.to_owned(),
            issued: unix_seconds(),
        };
        let payload = serde_json::to_vec(&claims).unwrap_or_default();
        let mac = hmac_sha256(&self.cursor_key, &payload);
        format!("{}.{}", super::hex(&payload), super::hex(&mac))
    }

    /// Checks a cursor against the query and returns the position it continues after and the
    /// revision it was issued for. The caller rejects the page when the library has moved past
    /// that revision. A foreign, mismatched or expired cursor is a [`CursorError`].
    pub fn open_cursor(
        &self,
        cursor: &str,
        binding: &CursorBinding,
    ) -> Result<(String, u64), CursorError> {
        if cursor.len() > MAX_CURSOR_BYTES {
            return Err(CursorError::Malformed);
        }
        let (payload, mac) = cursor.split_once('.').ok_or(CursorError::Malformed)?;
        let payload = unhex(payload).ok_or(CursorError::Malformed)?;
        let mac = unhex(mac).ok_or(CursorError::Malformed)?;
        if !super::constant_time_eq(&mac, &hmac_sha256(&self.cursor_key, &payload)) {
            return Err(CursorError::Malformed);
        }
        let claims: CursorClaims =
            serde_json::from_slice(&payload).map_err(|_| CursorError::Malformed)?;
        let now = unix_seconds();
        if claims.v != 1
            || claims.binding != binding.digest()
            || claims.issued > now
            || now - claims.issued > CURSOR_TTL.as_secs()
        {
            return Err(CursorError::Stale);
        }
        Ok((claims.after, claims.revision))
    }
}

/// Rechecks the caller's authorization immediately before a side effect. Run it inside the
/// critical section that commits, so a revocation cannot slip in between check and commit.
pub type CommitGuard = Box<dyn Fn() -> Result<(), Status> + Send + 'static>;

/// How a plugin favorite change ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FavoriteWrite {
    Applied,
    /// The game already had this favorite state.
    AlreadyApplied,
    /// None of the caller's own entries is currently matched to the game.
    NoMatchedEntry,
}

/// A catalog game a plugin search found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogGame {
    pub id: u64,
    pub name: String,
    pub release_year: Option<i32>,
}

/// How an assignment ended. `revision` is the entry's revision after the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchWrite {
    Applied {
        revision: u64,
    },
    /// The entry already has an explicit choice of this game.
    AlreadyApplied {
        revision: u64,
    },
    /// The entry changed since the revision the caller expected; nothing was written.
    Conflict {
        current: u64,
    },
    /// The host does not know this game; it must be found with a search first.
    UnknownGame,
    /// The user chose this entry's match; a plugin cannot replace it.
    UserChoice,
}

/// A cover read back for an entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredArtwork {
    pub bytes: Vec<u8>,
    pub media_type: &'static str,
    /// The entry's artwork revision: 0 while the cover is the game's own, not a plugin's.
    pub revision: u64,
}

/// One approved contribution to Core executable discovery. Physical paths stay host-only.
#[derive(Clone)]
pub struct PluginScanRoot {
    pub root: gamenode_platform::plugin_storage::PluginLibraryRoot,
    pub valid: Arc<dyn Fn() -> Result<(), Status> + Send + Sync>,
}

/// The engine's side of plugin library services: everything that reads or changes state the
/// library machine owns. Implementations take the engine's own locks; callers never hold one.
#[derive(Clone)]
pub struct BackupHostContext {
    pub engine: Arc<dyn gamenode_core::drm_free::BackupEngine>,
    pub target: gamenode_core::drm_free::BackupTarget,
    pub destination: String,
}

#[async_trait::async_trait]
pub trait LibraryHost: Send + Sync {
    fn ports_supported(&self) -> bool {
        false
    }
    async fn ports_catalog(
        &self,
        _detail: Option<String>,
    ) -> Result<Vec<gamenode_plugin_protocol::proto::PortRecord>, Status> {
        Err(Status::unavailable("ports catalog unavailable"))
    }
    async fn request_port(
        &self,
        _port_id: String,
        _review: gamenode_core::ports::view::PortPluginRequest,
        _check: super::jobs::Fence,
    ) -> Result<(), Status> {
        Err(Status::unavailable("port installation unavailable"))
    }

    fn tools_supported(&self) -> bool {
        false
    }
    async fn tools_list(
        &self,
        _entry_key: String,
    ) -> Result<Vec<gamenode_plugin_protocol::proto::ToolRecord>, Status> {
        Err(Status::unavailable(
            "tools are unavailable on this platform",
        ))
    }
    async fn request_tool(
        &self,
        _entry_key: String,
        _tool_id: String,
        _review: gamenode_core::tools::ToolPluginRequest,
        _check: super::jobs::Fence,
    ) -> Result<(), Status> {
        Err(Status::unavailable(
            "tools are unavailable on this platform",
        ))
    }

    /// Resolves the real installation and the user's backup destination on workers.
    async fn backup_context(&self, _entry_key: String) -> Result<BackupHostContext, Status> {
        Err(Status::unavailable("scoped backups are unavailable"))
    }
    fn plugin_roots_supported(&self) -> bool {
        false
    }
    fn set_plugin_roots(&self, _owner: String, _roots: Vec<PluginScanRoot>) -> Result<(), Status> {
        Err(Status::unimplemented(
            "Core library-root discovery is unavailable",
        ))
    }

    /// The favorite state of every GameNode game matched by an entry of `provider_id`, by game
    /// id. Games with no entry of that provider are not disclosed.
    fn favorites(&self, provider_id: &str) -> Vec<(u64, bool)>;

    /// Marks or unmarks `game_id` through the same local-first path as the user's own star.
    /// Requires an entry of `provider_id` currently matched to the game; the favorite stays on
    /// the game, never on the entry.
    fn set_favorite(
        &self,
        provider_id: &str,
        game_id: u64,
        favorite: bool,
        guard: &dyn Fn() -> Result<(), Status>,
    ) -> Result<FavoriteWrite, Status>;

    /// Searches GameNode's catalog by title with the user's verified session and returns at most
    /// `limit` games, in the server's order. The games become assignable by
    /// [`assign_match`](Self::assign_match). `UNAVAILABLE` when there is no verified, online
    /// session or the catalog could not answer.
    async fn search_catalog(&self, query: String, limit: usize)
    -> Result<Vec<CatalogGame>, Status>;

    /// Assigns `entry_key` to `game_id` through the library's own match policy. Checked and
    /// committed under the engine's lock: the entry's revision must be `expected_revision`, and a
    /// choice the user made is never replaced.
    fn assign_match(
        &self,
        entry_key: &str,
        game_id: u64,
        expected_revision: u64,
        guard: &dyn Fn() -> Result<(), Status>,
    ) -> Result<MatchWrite, Status>;

    /// The cover `entry_key` shows: its plugin override, else the matched game's own. Reads at
    /// most `max_bytes` on a worker thread. `NOT_FOUND` when the entry has no cover on this
    /// device; `RESOURCE_EXHAUSTED` when the image is larger than `max_bytes`. Nothing is
    /// downloaded.
    async fn read_artwork(
        &self,
        entry_key: String,
        max_bytes: u64,
    ) -> Result<StoredArtwork, Status>;

    /// Sets the cover override of `entry_key` to the prepared image, or clears it with `None`,
    /// if `expected_revision` is current. The file is written before the row and the replaced
    /// file removed after it; the call returns once the library applied the change.
    async fn set_artwork(
        &self,
        entry_key: String,
        expected_revision: u64,
        image: Option<Prepared>,
        guard: CommitGuard,
    ) -> Result<ArtworkWrite, Status>;

    /// Hands the entry's Play (`install` false) or Install link to the operating system through
    /// the library's own path, so the user's own Play button and a plugin's request behave the
    /// same: the same launch status, the same refusals, and playtime detection as before. The
    /// call returns once the link was handled. `UNIMPLEMENTED` when the provider has no such
    /// action; `FAILED_PRECONDITION` when the game is already running or being opened, or when
    /// the entry lacks data or the store application is missing; `UNAVAILABLE` when the link
    /// could not be opened.
    async fn open_entry(&self, entry_key: String, install: bool) -> Result<(), Status> {
        let _ = (entry_key, install);
        Err(Status::unimplemented("starting games is not available"))
    }

    /// A stable number for what a launch of `entry_key` would start now: it changes when the
    /// detected target changes (another launch URI, another approved executable, its arguments or
    /// working directory) and not otherwise. A remembered confirmation and a pre-issued intent
    /// are bound to it, so a changed target asks the user again. `NOT_FOUND` for an unknown
    /// entry; `FAILED_PRECONDITION` when the entry has no launchable target.
    async fn launch_fingerprint(&self, entry_key: String) -> Result<u64, Status> {
        let _ = entry_key;
        Ok(0)
    }

    /// Stores the display-name override of `entry_key` if `expected_revision` is current,
    /// returning only after the write reached the database and the library applied it.
    async fn set_display_name(
        &self,
        entry_key: String,
        expected_revision: u64,
        display_name: Option<String>,
        guard: CommitGuard,
    ) -> Result<DisplayWrite, Status>;
}

/// Everything a cursor is bound to besides the library revision.
#[derive(Debug, PartialEq, Eq)]
pub struct CursorBinding {
    /// Which list the cursor belongs to, so one list's cursor cannot page another.
    pub route: &'static str,
    pub session_id: String,
    pub profile_epoch: u64,
    pub grant_revision: u64,
    pub scope: Scope,
    pub search: Option<String>,
}

impl CursorBinding {
    fn digest(&self) -> String {
        let mut hash = Sha256::new();
        for part in [
            self.route,
            self.session_id.as_str(),
            &self.profile_epoch.to_string(),
            &self.grant_revision.to_string(),
            &self.scope.own_provider,
            if self.scope.read_all { "all" } else { "own" },
            self.scope.provider_filter.as_deref().unwrap_or("\u{0}"),
            self.search.as_deref().unwrap_or("\u{0}"),
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part.as_bytes());
        }
        super::hex(&hash.finalize())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CursorError {
    /// Not a cursor this host issued.
    Malformed,
    /// Issued for another query, scope or session, expired, or the library changed since.
    Stale,
}

#[derive(Serialize, Deserialize)]
struct CursorClaims {
    v: u8,
    binding: String,
    revision: u64,
    after: String,
    issued: u64,
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// HMAC-SHA256 (RFC 2104) with a 32-byte key.
fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    block[..32].copy_from_slice(key);
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

#[cfg(test)]
mod tests;
