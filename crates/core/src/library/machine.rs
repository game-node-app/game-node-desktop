//! The library state machine.
//!
//! Like [`AuthMachine`](crate::AuthMachine) it is a pure reducer: callers feed it [`Input`]s and
//! run the returned [`Effect`]s (storage, scanning, opening URIs), then feed the results back.
//! Results carry the operation id of the effect that produced them, so an answer that arrives
//! after the account changed or a newer load started is ignored.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use crate::game_api::{ApiError, Collection, ExternalMatch, GameSummary, LibraryEntry};
use crate::provider::{Provider, Record};

use super::add_dialog::AddDialog;
use super::cover::CoverUrl;
use super::cover_queue::CoverQueue;
use super::favorites::FavoriteState;
use super::layout::{
    DETAIL_WIDTH_RANGE, LibraryFilters, LibraryLayout, SIDEBAR_WIDTH_RANGE, SidebarSelection, Sort,
    ViewMode,
};
use super::lookup::LookupRun;
use super::match_dialog::MatchDialog;
use super::membership::{EntryStatus, Membership};
use super::selection::{Direction, SelectMode, Selection};
use super::store::{
    AutoMatch, CatalogError, CatalogRecord, CatalogUpdate, ChosenMatch, FavoriteRecord, GameRecord,
    LastAccount, LibraryCache, ProviderScanState, StoredEntry, entry_key,
};
use super::view::LibraryView;

use serde::{Deserialize, Serialize};

/// What the application can reach right now. Matching, game search and Add to GameNode need
/// `Verified`; everything local works in every state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    /// No verified session yet (still verifying, or GameNode could not be reached at startup).
    #[default]
    Unverified,
    /// A verified session, but GameNode is currently unreachable.
    Offline,
    Verified,
}

/// The result of scanning one provider, as it is stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanOutcome {
    Scanned(Vec<Record>),
    /// The provider's roots could not be read. The message is for the log and the detail view.
    Failed(String),
}

/// Work for the caller to perform.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Read the cache for this account (none: no chosen matches).
    LoadCache {
        op: u64,
        account_id: Option<String>,
    },
    /// Scan one provider off the UI thread and report with [`Input::ProviderScanned`].
    RunScan {
        op: u64,
        provider: String,
    },
    PersistScan {
        provider: String,
        outcome: ScanOutcome,
        now: i64,
    },
    /// Fetch the owned and family catalog of one provider and report with
    /// [`Input::CatalogSynced`]. Emitted for every scan of a catalog provider and when the
    /// account connects; the engine decides whether a credential exists.
    SyncCatalog {
        op: u64,
        provider: String,
    },
    PersistLayout(LibraryLayout),
    PersistExcluded(Vec<String>),
    /// Store one favorite change for the account, so it survives restart and stays pending until
    /// GameNode confirms it.
    PersistFavorite(FavoriteRecord),
    /// Read the account's favorite games (`GameApi::list_favorites`) and report with
    /// [`Input::FavoritesSynced`]. Only sent while favorites sync is active.
    SyncFavorites {
        op: u64,
        user_id: String,
    },
    /// Send one favorite change (`GameApi::set_favorite`) and report with
    /// [`Input::FavoritePushed`]. At most one is outstanding per game.
    PushFavorite {
        op: u64,
        game_id: u64,
        favorite: bool,
    },
    /// Ask GameNode which games these store ids are (`GameApi::lookup_external`) and report with
    /// [`Input::LookupDone`]. Only sent with a verified session.
    Lookup {
        op: u64,
        category: u32,
        uids: Vec<String>,
    },
    /// Search GameNode's games for the match dialog (`GameApi::search_games`) and report with
    /// [`Input::MatchSearched`].
    SearchGames {
        op: u64,
        query: String,
    },
    /// Store the account's choice for an entry. `games` for a picked search result are written
    /// first, with [`Effect::PersistMatches`].
    PersistChoice {
        account_id: String,
        choice: ChosenMatch,
    },
    /// Delete the account's choice for an entry.
    ForgetChoice {
        account_id: String,
        entry_key: String,
    },
    /// Download a cover (`GameApi::fetch_cover`), write it to `<covers>/<game_id>.jpg` and report
    /// with [`Input::CoverDone`]. At most four are outstanding; only sent with a verified session.
    FetchCover {
        game_id: u64,
        url: CoverUrl,
    },
    /// Fetch the selected game's landscape artwork and report it with [`Input::ArtworkDone`].
    FetchArtwork {
        op: u64,
        game_id: u64,
    },
    /// Read whether the game is in the user's library (`GameApi::library_entry`) and report with
    /// [`Input::EntryRead`]. Only sent with a verified session.
    ReadEntry {
        op: u64,
        game_id: u64,
    },
    /// Read the user's collections (`GameApi::list_collections`) and report with
    /// [`Input::CollectionsListed`].
    ListCollections {
        op: u64,
        user_id: String,
    },
    /// Add a game to the library (`GameApi::create_entry`) and report with
    /// [`Input::EntryCreated`]. Only sent right after a read found no entry.
    CreateEntry {
        op: u64,
        game_id: u64,
        status: EntryStatus,
        collection_ids: Vec<String>,
    },
    UpdateEntry {
        op: u64,
        game_id: u64,
        status: EntryStatus,
        collection_ids: Vec<String>,
        existing: LibraryEntry,
    },
    PersistCoverReady {
        game_id: u64,
        file: String,
    },
    PersistCoverFailed {
        game_id: u64,
        at: i64,
    },
    /// Store what a lookup learned. `games` must be written before `matches`.
    PersistMatches {
        games: Vec<GameRecord>,
        matches: Vec<AutoMatch>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Input {
    /// The account whose library is shown changed: the last signed-in account when the app
    /// starts, a different one after a sign-in, or none after sign-out or a rejected session.
    AccountChanged(Option<LastAccount>),
    Loaded {
        op: u64,
        result: Result<LibraryCache, ()>,
    },
    /// Start a scan of every provider (window opened, user asked).
    ScanRequested,
    ProviderScanned {
        op: u64,
        provider: String,
        result: Result<Vec<Record>, String>,
        /// Unix seconds.
        now: i64,
    },
    /// The answer to an [`Effect::SyncCatalog`]. `owned`/`family` are `None` when that fact set
    /// was not attempted; `Err` keeps the previous set and records the category.
    CatalogSynced {
        op: u64,
        provider: String,
        owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
        family: Option<Result<Vec<CatalogRecord>, CatalogError>>,
        /// Unix seconds.
        now: i64,
    },
    /// The connected store account changed or signed out: drop its owned and family facts but
    /// keep entries, matches and covers.
    CatalogCleared {
        provider: String,
    },
    /// Ask every provider with a catalog to sync now.
    RequestCatalog,
    /// Replace the toolbar filters.
    SetFilters(LibraryFilters),
    /// Mark or unmark the entry's matched game as a favorite. `now` is unix seconds.
    SetFavorite {
        key: String,
        favorite: bool,
        now: i64,
    },
    /// The answer to an [`Effect::SyncFavorites`]: the complete server favorite game ids.
    FavoritesSynced {
        op: u64,
        result: Result<Vec<u64>, ApiError>,
        now: i64,
    },
    /// The answer to an [`Effect::PushFavorite`].
    FavoritePushed {
        op: u64,
        game_id: u64,
        favorite: bool,
        result: Result<(), crate::game_api::FavoriteError>,
        now: i64,
    },
    /// Whether favorites may leave this PC: sync is on, explained and the session is verified.
    SetSyncActive(bool),
    /// Retry a favorite read and every pending push at the next opportunity.
    SyncFavoritesRequested,
    /// What the application can reach now. `now` is unix seconds.
    SetAccess {
        access: Access,
        now: i64,
    },
    /// Open the match dialog on the selected entry and search for its detected title.
    OpenMatch,
    CloseMatch,
    /// The dialog's search text changed. Nothing is sent until [`Input::SearchMatch`].
    SetMatchQuery(String),
    SearchMatch,
    MatchSearched {
        op: u64,
        result: Result<Vec<GameSummary>, ApiError>,
    },
    /// Match the entry to a candidate or search result. `now` is unix seconds.
    PickMatch {
        game_id: u64,
        now: i64,
    },
    /// Mark the entry as having no GameNode game; later lookups never change it.
    MarkNoGame {
        now: i64,
    },
    /// Forget the user's choice so the automatic match, if any, applies again.
    ClearMatch,
    /// The outcome of an [`Effect::FetchCover`]: the file name it was stored under, or failure.
    CoverDone {
        game_id: u64,
        result: Result<String, ()>,
        now: i64,
    },
    /// The outcome of an [`Effect::FetchArtwork`].
    ArtworkDone {
        op: u64,
        game_id: u64,
        result: Result<Option<Vec<u8>>, ApiError>,
    },
    /// The answer to an [`Effect::Lookup`]. A failure of any kind changes nothing.
    LookupDone {
        op: u64,
        category: u32,
        result: Result<Vec<ExternalMatch>, ApiError>,
        now: i64,
    },
    /// A click on an entry.
    Select {
        key: String,
        mode: SelectMode,
    },
    /// An arrow, Home or End key; `extend` is Shift.
    Move {
        direction: Direction,
        extend: bool,
    },
    /// How many entries fit in a grid row at the current window width.
    SetColumns(usize),
    /// Grid or list.
    SetView(ViewMode),
    /// All games, or one provider.
    SetFilter(SidebarSelection),
    /// The search box text. Not remembered across restarts.
    SetSearch(String),
    SetSort(Sort),
    /// Pane sizes in logical pixels, clamped to a usable range.
    SetSidebarWidth(u32),
    SetDetailWidth(u32),
    SetDetailOpen(bool),
    /// Enter on the focused entry: select it and show its details.
    OpenDetails,
    /// The answer to an [`Effect::ReadEntry`]: `Ok(None)` means the game is not in the library.
    EntryRead {
        op: u64,
        game_id: u64,
        result: Result<Option<LibraryEntry>, ApiError>,
    },
    /// Read the selected game's standing again (after a failed read).
    RefreshMembership,
    /// Open the Add to GameNode dialog for the selected entries.
    OpenAdd,
    CloseAdd,
    SetAddStatus(EntryStatus),
    ToggleAddCollection(String),
    CollectionsListed {
        op: u64,
        result: Result<Vec<Collection>, ApiError>,
    },
    RetryCollections,
    /// Add the dialog's games: each is checked first and only created if absent.
    ConfirmAdd,
    /// The answer to an [`Effect::CreateEntry`].
    EntryCreated {
        op: u64,
        game_id: u64,
        result: Result<(), ApiError>,
    },
    /// Remove an entry from the library (`allow_rediscovery = true` hides it until the next
    /// scan; `false` excludes it across future scans and restarts).
    RemoveEntry {
        key: String,
        allow_rediscovery: bool,
    },
    /// Clear all permanent entry exclusions so current and future scans show them again.
    ResetExcluded,
}

#[derive(Debug)]
pub(super) enum Phase {
    /// No account: the library is not shown.
    Inactive,
    Loading,
    Ready,
}

#[derive(Debug)]
pub(super) struct ScanRun {
    pub(super) op: u64,
    pub(super) waiting: BTreeSet<String>,
}

/// Which independent fact set a catalog update covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CatalogFact {
    Owned,
    Family,
}

pub struct LibraryMachine {
    pub(super) local_inferences: BTreeMap<String, u64>,
    pub(super) playtime: BTreeMap<String, crate::playtime::PlaytimeView>,
    pub(super) running: BTreeSet<String>,
    pub(super) providers: Vec<Provider>,
    pub(super) phase: Phase,
    pub(super) account: Option<LastAccount>,
    pub(super) access: Access,
    pub(super) op: u64,
    pub(super) entries: BTreeMap<String, StoredEntry>,
    pub(super) excluded: BTreeSet<String>,
    pub(super) scans: BTreeMap<String, ProviderScanState>,
    /// The catalog sync op in flight for each provider; older answers are ignored.
    pub(super) catalog_ops: BTreeMap<String, u64>,
    pub(super) auto_matches: BTreeMap<(u32, String), AutoMatch>,
    pub(super) chosen: BTreeMap<String, ChosenMatch>,
    pub(super) games: BTreeMap<u64, GameRecord>,
    pub(super) scan: Option<ScanRun>,
    pub(super) rescan_pending: bool,
    pub(super) layout: LibraryLayout,
    /// Unix seconds used by the relative filters and windows. The engine refreshes it before
    /// every library input; tests set it explicitly.
    pub(super) now: i64,
    /// The account's favorite state per GameNode game. Pending changes are local wins.
    pub(super) favorites: std::collections::BTreeMap<u64, FavoriteState>,
    /// Whether favorites may leave this PC (`enabled && explained && verified`).
    pub(super) favorites_sync_active: bool,
    /// A read has been requested and not started yet.
    pub(super) favorites_sync_pending: bool,
    /// The favorite read in flight; stale answers are ignored.
    pub(super) favorites_sync_op: Option<u64>,
    /// The favorite push in flight per game; at most one per game.
    pub(super) favorite_pushes: std::collections::BTreeMap<u64, u64>,
    /// The search box text. Not part of the layout: it starts empty on every launch.
    pub(super) search: String,
    pub(super) selection: Selection,
    pub(super) grid_columns: usize,
    /// What GameNode last said about each matched game, read when its entry is selected.
    pub(super) membership: BTreeMap<u64, Membership>,
    pub(super) account_entries: BTreeMap<u64, LibraryEntry>,
    /// The membership read outstanding for each game (its op id); other answers are ignored.
    pub(super) membership_reads: BTreeMap<u64, u64>,
    /// The game a read was planned for since the selection last changed.
    pub(super) membership_focus: Option<u64>,
    /// The Add to GameNode dialog, while it is open.
    pub(super) add: Option<AddDialog>,
    /// Lookups sent and not answered yet, by external category.
    pub(super) lookups: BTreeMap<u32, LookupRun>,
    /// The match dialog, while it is open.
    pub(super) dialog: Option<MatchDialog>,
    pub(super) covers: CoverQueue,
    /// Artwork is loaded on demand; the engine checks its disk cache before downloading.
    pub(super) artwork_focus: Option<u64>,
    pub(super) artwork_requests: BTreeMap<u64, u64>,
    /// A small recent-image cache makes revisiting a game immediate.
    pub(super) artwork_cache: VecDeque<(u64, Option<Arc<[u8]>>)>,
    pub(super) artwork_attempted: bool,
    pub(super) artwork: Option<Arc<[u8]>>,
    pub(super) view_cache: Option<LibraryView>,
    /// Whether something the view shows may have changed since `view_cache` was built.
    pub(super) view_dirty: bool,
}

impl LibraryMachine {
    pub fn new(providers: Vec<Provider>, layout: LibraryLayout) -> Self {
        Self {
            local_inferences: BTreeMap::new(),
            playtime: BTreeMap::new(),
            running: BTreeSet::new(),
            providers,
            phase: Phase::Inactive,
            account: None,
            access: Access::default(),
            op: 0,
            entries: BTreeMap::new(),
            excluded: BTreeSet::new(),
            scans: BTreeMap::new(),
            catalog_ops: BTreeMap::new(),
            auto_matches: BTreeMap::new(),
            chosen: BTreeMap::new(),
            games: BTreeMap::new(),
            scan: None,
            rescan_pending: false,
            layout: layout.normalized(),
            now: 0,
            favorites: BTreeMap::new(),
            favorites_sync_active: false,
            favorites_sync_pending: false,
            favorites_sync_op: None,
            favorite_pushes: BTreeMap::new(),
            search: String::new(),
            selection: Selection::default(),
            grid_columns: 1,
            membership: BTreeMap::new(),
            account_entries: BTreeMap::new(),
            membership_reads: BTreeMap::new(),
            membership_focus: None,
            add: None,
            lookups: BTreeMap::new(),
            dialog: None,
            covers: CoverQueue::default(),
            artwork_focus: None,
            artwork_requests: BTreeMap::new(),
            artwork_cache: VecDeque::new(),
            artwork_attempted: false,
            artwork: None,
            view_cache: None,
            view_dirty: false,
        }
    }

    pub(super) fn next_op(&mut self) -> u64 {
        self.op += 1;
        self.op
    }

    pub fn providers(&self) -> &[Provider] {
        &self.providers
    }

    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|provider| provider.id() == id)
    }

    /// Whether the library is shown (an account is set, even before it is verified).
    pub fn is_active(&self) -> bool {
        !matches!(self.phase, Phase::Inactive)
    }

    /// Replaces the playtime shown per entry. Returns whether anything changed, so callers that
    /// publish can skip a no-op update.
    pub fn set_playtime(
        &mut self,
        playtime: BTreeMap<String, crate::playtime::PlaytimeView>,
    ) -> bool {
        if self.playtime == playtime {
            return false;
        }
        self.playtime = playtime;
        self.view_dirty = true;
        true
    }

    /// Replaces the running set. Returns whether it changed.
    pub fn set_running(&mut self, running: BTreeSet<String>) -> bool {
        if self.running == running {
            return false;
        }
        self.running = running;
        self.view_dirty = true;
        true
    }

    pub fn account(&self) -> Option<&LastAccount> {
        self.account.as_ref()
    }

    pub fn access(&self) -> Access {
        self.access
    }

    pub fn scan_running(&self) -> bool {
        self.scan.is_some()
    }

    pub fn scan_state(&self, provider: &str) -> Option<&ProviderScanState> {
        self.scans.get(provider)
    }

    pub fn entry(&self, key: &str) -> Option<&StoredEntry> {
        self.entries.get(key)
    }

    /// The entries a scan currently finds installed, in key order.
    pub fn installed_entries(&self) -> impl Iterator<Item = &StoredEntry> {
        self.entries
            .values()
            .filter(|entry| entry.installed && !self.excluded.contains(&entry.key))
    }

    /// Every entry the library shows: installed, owned or family-shared, minus exclusions.
    pub fn visible_entries(&self) -> impl Iterator<Item = &StoredEntry> {
        self.entries
            .values()
            .filter(|entry| entry.is_visible() && !self.excluded.contains(&entry.key))
    }

    pub fn all_entries(&self) -> impl Iterator<Item = &StoredEntry> {
        self.entries.values()
    }

    pub fn chosen_match(&self, key: &str) -> Option<&ChosenMatch> {
        self.chosen.get(key)
    }

    pub fn set_local_inference(
        &mut self,
        key: String,
        game_id: Option<u64>,
        game: Option<GameRecord>,
    ) -> Vec<Effect> {
        if let Some(game_id) = game_id {
            self.local_inferences.insert(key, game_id);
        } else {
            self.local_inferences.remove(&key);
        }
        if let Some(game) = game {
            self.games.insert(game.game_id, game);
        }
        self.view_dirty = true;
        self.plan_covers(None)
    }
    /// The downloaded landscape artwork of `game_id` while that game is the one whose details
    /// are shown. Cheap: it only clones a handle.
    pub fn artwork_for(&self, game_id: u64) -> Option<Arc<[u8]>> {
        if self.artwork_focus == Some(game_id) {
            self.artwork.clone()
        } else {
            None
        }
    }

    pub fn game(&self, game_id: u64) -> Option<&GameRecord> {
        self.games.get(&game_id)
    }

    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        // Any input may change what the view shows; `view` rebuilds once and then reuses the
        // result until the next input.
        self.view_dirty = true;
        let mut effects = self.dispatch(input);
        effects.extend(self.plan_membership());
        effects.extend(self.plan_artwork());
        effects
    }

    fn plan_artwork(&mut self) -> Vec<Effect> {
        let game_id = if self.access == Access::Verified && matches!(self.phase, Phase::Ready) {
            self.selected_game()
        } else {
            None
        };
        if game_id != self.artwork_focus {
            self.artwork_focus = game_id;
            self.artwork_attempted = false;
            self.artwork = None;
            if let Some(index) = self
                .artwork_cache
                .iter()
                .position(|(id, _)| Some(*id) == game_id)
            {
                let cached = self.artwork_cache.remove(index).unwrap();
                self.artwork = cached.1.clone();
                self.artwork_attempted = true;
                self.artwork_cache.push_back(cached);
            }
        }
        if !self.layout.detail_open || self.artwork_attempted {
            return vec![];
        }
        let Some(game_id) = game_id else {
            return vec![];
        };
        if self.artwork_requests.contains_key(&game_id) {
            return vec![];
        }
        let op = self.next_op();
        self.artwork_requests.insert(game_id, op);
        vec![Effect::FetchArtwork { op, game_id }]
    }

    fn on_artwork_done(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<Option<Vec<u8>>, ApiError>,
    ) {
        if self.artwork_requests.get(&game_id) != Some(&op) {
            return;
        }
        self.artwork_requests.remove(&game_id);
        let successful = result.is_ok();
        let image = result.ok().flatten().map(Arc::<[u8]>::from);
        if successful {
            self.artwork_cache.push_back((game_id, image.clone()));
            if self.artwork_cache.len() > 8 {
                self.artwork_cache.pop_front();
            }
        }
        if self.artwork_focus == Some(game_id) {
            self.artwork_attempted = true;
            self.artwork = image;
        }
    }

    fn dispatch(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::AccountChanged(account) => self.on_account_changed(account),
            Input::Loaded { op, result } => self.on_loaded(op, result),
            Input::ScanRequested => self.on_scan_requested(),
            Input::ProviderScanned {
                op,
                provider,
                result,
                now,
            } => self.on_provider_scanned(op, provider, result, now),
            Input::CatalogSynced {
                op,
                provider,
                owned,
                family,
                now,
            } => self.on_catalog_synced(op, provider, owned, family, now),
            Input::CatalogCleared { provider } => {
                self.clear_catalog(&provider);
                vec![]
            }
            Input::RequestCatalog => self.request_catalog(),
            Input::SetFilters(filters) => self.update_layout(|layout| layout.filters = filters),
            Input::SetFavorite { key, favorite, now } => self.on_set_favorite(key, favorite, now),
            Input::FavoritesSynced { op, result, now } => self.on_favorites_synced(op, result, now),
            Input::FavoritePushed {
                op,
                game_id,
                favorite,
                result,
                now,
            } => self.on_favorite_pushed(op, game_id, favorite, result, now),
            Input::SetSyncActive(active) => self.set_sync_active(active),
            Input::SyncFavoritesRequested => self.request_favorites_sync(),
            Input::SetAccess { access, now } => {
                self.access = access;
                let mut effects = self.plan_lookups(now);
                effects.extend(self.plan_covers(None));
                effects
            }
            Input::CoverDone {
                game_id,
                result,
                now,
            } => self.on_cover_done(game_id, result, now),
            Input::ArtworkDone {
                op,
                game_id,
                result,
            } => {
                self.on_artwork_done(op, game_id, result);
                vec![]
            }
            Input::LookupDone {
                op,
                category,
                result,
                now,
            } => self.on_lookup_done(op, category, result, now),
            Input::OpenMatch => self.open_match(),
            Input::CloseMatch => {
                self.close_match();
                vec![]
            }
            Input::SetMatchQuery(text) => {
                self.set_match_query(text);
                vec![]
            }
            Input::SearchMatch => self.search_match(),
            Input::MatchSearched { op, result } => self.on_match_searched(op, result),
            Input::PickMatch { game_id, now } => self.pick_match(game_id, now),
            Input::MarkNoGame { now } => self.mark_no_game(now),
            Input::ClearMatch => self.clear_match(),
            Input::Select { key, mode } => {
                self.select(&key, mode);
                vec![]
            }
            Input::Move { direction, extend } => {
                self.step(direction, extend);
                vec![]
            }
            Input::SetColumns(columns) => {
                self.grid_columns = columns.max(1);
                vec![]
            }
            Input::SetSearch(text) => {
                self.search = text;
                vec![]
            }
            Input::OpenDetails => {
                if let Some(key) = self.focused_key() {
                    self.select(&key, SelectMode::Replace);
                }
                self.update_layout(|layout| layout.detail_open = true)
            }
            Input::SetView(view) => self.update_layout(|layout| layout.view = view),
            Input::SetFilter(sidebar) => self.update_layout(|layout| layout.sidebar = sidebar),
            Input::SetSort(sort) => self.update_layout(|layout| layout.sort = sort),
            Input::SetSidebarWidth(width) => self.update_layout(|layout| {
                layout.sidebar_width =
                    width.clamp(*SIDEBAR_WIDTH_RANGE.start(), *SIDEBAR_WIDTH_RANGE.end());
            }),
            Input::SetDetailWidth(width) => self.update_layout(|layout| {
                layout.detail_width =
                    width.clamp(*DETAIL_WIDTH_RANGE.start(), *DETAIL_WIDTH_RANGE.end());
            }),
            Input::SetDetailOpen(open) => self.update_layout(|layout| layout.detail_open = open),
            Input::EntryRead {
                op,
                game_id,
                result,
            } => {
                if self.membership_reads.get(&game_id) == Some(&op) {
                    self.on_membership_read(game_id, result);
                    vec![]
                } else {
                    self.on_add_checked(op, game_id, result)
                }
            }
            Input::RefreshMembership => {
                self.membership_focus = None;
                if let Some(game_id) = self.selected_game()
                    && self.membership(game_id) == Membership::Unavailable
                {
                    self.membership.remove(&game_id);
                }
                vec![]
            }
            Input::OpenAdd => self.open_add(),
            Input::CloseAdd => {
                self.close_add();
                vec![]
            }
            Input::SetAddStatus(status) => {
                self.set_add_status(status);
                vec![]
            }
            Input::ToggleAddCollection(id) => {
                self.toggle_add_collection(&id);
                vec![]
            }
            Input::CollectionsListed { op, result } => self.on_collections_listed(op, result),
            Input::RetryCollections => self.retry_collections(),
            Input::ConfirmAdd => self.confirm_add(),
            Input::EntryCreated {
                op,
                game_id,
                result,
            } => self.on_entry_created(op, game_id, result),
            Input::RemoveEntry {
                key,
                allow_rediscovery,
            } => {
                if self.match_dialog_view().is_some_and(|d| d.entry_key == key) {
                    self.dialog = None;
                }
                if allow_rediscovery {
                    if let Some(entry) = self.entries.get_mut(&key) {
                        entry.installed = false;
                    }
                    self.prune_selection();
                    vec![]
                } else {
                    let changed = self.excluded.insert(key);
                    self.prune_selection();
                    if changed {
                        vec![Effect::PersistExcluded(
                            self.excluded.iter().cloned().collect(),
                        )]
                    } else {
                        vec![]
                    }
                }
            }
            Input::ResetExcluded => {
                if self.excluded.is_empty() {
                    return vec![];
                }
                self.excluded.clear();
                let mut effects = vec![Effect::PersistExcluded(Vec::new())];
                effects.extend(self.plan_lookups(0));
                effects.extend(self.plan_covers(None));
                effects
            }
        }
    }

    /// Applies a change to the remembered layout and asks for it to be saved if it changed.
    fn update_layout(&mut self, change: impl FnOnce(&mut LibraryLayout)) -> Vec<Effect> {
        let before = self.layout.clone();
        change(&mut self.layout);
        if self.layout == before {
            return vec![];
        }
        vec![Effect::PersistLayout(self.layout.clone())]
    }

    fn reset_data(&mut self) {
        self.local_inferences.clear();
        self.entries.clear();
        self.excluded.clear();
        self.scans.clear();
        self.catalog_ops.clear();
        self.auto_matches.clear();
        self.chosen.clear();
        self.games.clear();
        self.scan = None;
        self.rescan_pending = false;
        self.selection = Selection::default();
        self.membership.clear();
        self.account_entries.clear();
        self.membership_reads.clear();
        self.membership_focus = None;
        self.add = None;
        self.lookups.clear();
        self.dialog = None;
        self.favorites.clear();
        self.favorites_sync_pending = false;
        self.favorites_sync_op = None;
        self.favorite_pushes.clear();
        self.covers.clear();
        self.artwork_focus = None;
        self.artwork_requests.clear();
        self.artwork_cache.clear();
        self.artwork_attempted = false;
        self.artwork = None;
    }

    fn on_account_changed(&mut self, account: Option<LastAccount>) -> Vec<Effect> {
        if account == self.account && self.is_active() == account.is_some() {
            return vec![];
        }
        // Whatever was loading or scanning belongs to the previous account.
        let op = self.next_op();
        self.reset_data();
        self.account = account;
        let Some(account) = &self.account else {
            self.phase = Phase::Inactive;
            return vec![];
        };
        self.phase = Phase::Loading;
        vec![Effect::LoadCache {
            op,
            account_id: Some(account.id.clone()),
        }]
    }

    fn on_loaded(&mut self, op: u64, result: Result<LibraryCache, ()>) -> Vec<Effect> {
        if !matches!(self.phase, Phase::Loading) || op != self.op {
            return vec![];
        }
        match result {
            Ok(cache) => {
                self.install_cache(cache);
                self.prune_selection();
            }
            // An unreadable cache is not fatal: the scan below rebuilds the entries.
            Err(()) => tracing::warn!("the library cache could not be read; starting empty"),
        }
        self.phase = Phase::Ready;
        let mut effects = self.start_scan();
        // A sync activation that happened while the cache loaded had no account to read yet.
        effects.extend(super::favorites::plan(self));
        effects
    }

    fn install_cache(&mut self, cache: LibraryCache) {
        self.entries = cache
            .entries
            .into_iter()
            .map(|entry| (entry.key.clone(), entry))
            .collect();
        self.scans = cache
            .scans
            .into_iter()
            .map(|state| (state.provider.clone(), state))
            .collect();
        self.auto_matches = cache
            .auto_matches
            .into_iter()
            .map(|auto| ((auto.category, auto.uid.clone()), auto))
            .collect();
        self.chosen = cache
            .chosen_matches
            .into_iter()
            .map(|choice| (choice.entry_key.clone(), choice))
            .collect();
        self.games = cache
            .games
            .into_iter()
            .map(|game| (game.game_id, game))
            .collect();
        self.excluded = cache.excluded.into_iter().collect();
        self.favorites = cache
            .favorites
            .into_iter()
            .map(|favorite| {
                (
                    favorite.game_id,
                    FavoriteState {
                        is_favorite: favorite.is_favorite,
                        pending: favorite.pending,
                    },
                )
            })
            .collect();
    }

    fn on_scan_requested(&mut self) -> Vec<Effect> {
        match self.phase {
            // Nothing to scan for, or the load that is under way ends with a scan anyway.
            Phase::Inactive | Phase::Loading => vec![],
            Phase::Ready if self.scan.is_some() => {
                self.rescan_pending = true;
                vec![]
            }
            Phase::Ready => self.start_scan(),
        }
    }

    fn start_scan(&mut self) -> Vec<Effect> {
        let op = self.next_op();
        self.rescan_pending = false;
        let waiting: BTreeSet<String> = self
            .providers
            .iter()
            .map(|provider| provider.id().to_owned())
            .collect();
        let mut effects: Vec<Effect> = waiting
            .iter()
            .map(|provider| Effect::RunScan {
                op,
                provider: provider.clone(),
            })
            .collect();
        // The installed scan is never gated by the catalog: both start together.
        effects.extend(self.catalog_sync_effects());
        self.scan = (!waiting.is_empty()).then_some(ScanRun { op, waiting });
        effects
    }

    /// Emits a catalog sync for every provider that declares one and records its op.
    fn catalog_sync_effects(&mut self) -> Vec<Effect> {
        let providers: Vec<String> = self
            .providers
            .iter()
            .filter(|provider| provider.has_catalog())
            .map(|provider| provider.id().to_owned())
            .collect();
        providers
            .into_iter()
            .map(|provider| {
                let op = self.next_op();
                self.catalog_ops.insert(provider.clone(), op);
                Effect::SyncCatalog { op, provider }
            })
            .collect()
    }

    fn request_catalog(&mut self) -> Vec<Effect> {
        if !matches!(self.phase, Phase::Ready) {
            return vec![];
        }
        self.catalog_sync_effects()
    }

    fn on_catalog_synced(
        &mut self,
        op: u64,
        provider: String,
        owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
        family: Option<Result<Vec<CatalogRecord>, CatalogError>>,
        now: i64,
    ) -> Vec<Effect> {
        if self.catalog_ops.get(&provider) != Some(&op) {
            return vec![];
        }
        self.catalog_ops.remove(&provider);
        self.merge_catalog(&provider, &CatalogUpdate { owned, family, now });
        self.prune_selection();
        let mut effects = self.plan_lookups(now);
        effects.extend(self.plan_covers(None));
        effects
    }

    /// Applies a catalog update exactly as `LibraryStore::save_catalog` applies it on disk.
    /// Owned is applied before family, so an owned app never keeps the family flag.
    fn merge_catalog(&mut self, provider: &str, update: &CatalogUpdate) {
        if let Some(owned) = &update.owned {
            self.apply_catalog_fact(provider, owned, update.now, CatalogFact::Owned);
        }
        if let Some(family) = &update.family {
            self.apply_catalog_fact(provider, family, update.now, CatalogFact::Family);
        }
    }

    fn apply_catalog_fact(
        &mut self,
        provider: &str,
        result: &Result<Vec<CatalogRecord>, CatalogError>,
        now: i64,
        fact: CatalogFact,
    ) {
        let records = match result {
            Err(error) => {
                let state = self.scan_state_mut(provider);
                match fact {
                    CatalogFact::Owned => state.catalog_error = Some(*error),
                    CatalogFact::Family => state.family_error = Some(*error),
                }
                return;
            }
            Ok(records) => records,
        };

        let excluded: BTreeSet<&str> = self
            .provider(provider)
            .map(|provider| {
                provider
                    .manifest()
                    .exclude_ids
                    .iter()
                    .map(String::as_str)
                    .collect()
            })
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        let mut dropped = 0usize;
        let usable: Vec<&CatalogRecord> = records
            .iter()
            .filter(|record| {
                let id = record.id.trim();
                let name = record.name.trim();
                if id.is_empty() || name.is_empty() || excluded.contains(id) {
                    dropped += 1;
                    false
                } else {
                    seen.insert(id.to_owned())
                }
            })
            .collect();
        if dropped > 0 {
            tracing::info!(
                provider,
                dropped,
                "ignored unusable catalog records (empty id/name, duplicate or excluded)"
            );
        }

        for entry in self
            .entries
            .values_mut()
            .filter(|entry| entry.provider == provider)
        {
            match fact {
                CatalogFact::Owned => entry.owned = false,
                CatalogFact::Family => entry.family_shared = false,
            }
        }
        for record in usable {
            let store_id = record.id.trim().to_owned();
            let catalog_name = record.name.trim();
            let key = entry_key(provider, &store_id);
            match self.entries.get_mut(&key) {
                Some(entry) => {
                    match fact {
                        CatalogFact::Owned => entry.owned = true,
                        // An entry the owned sync just claimed wins over family sharing.
                        CatalogFact::Family => entry.family_shared = !entry.owned,
                    }
                    // An installed entry keeps the name the installed scan gave it.
                    let name_is_empty = entry
                        .name
                        .as_deref()
                        .is_none_or(|name| name.trim().is_empty());
                    if !entry.installed && name_is_empty {
                        entry.name = Some(catalog_name.to_owned());
                    }
                    entry.last_seen = now;
                }
                None => {
                    self.entries.insert(
                        key.clone(),
                        StoredEntry {
                            key,
                            provider: provider.to_owned(),
                            store_id,
                            name: Some(catalog_name.to_owned()),
                            // Catalog-only entries are not installed anywhere.
                            install_dir: String::new(),
                            fields: BTreeMap::new(),
                            installed: false,
                            owned: fact == CatalogFact::Owned,
                            family_shared: fact == CatalogFact::Family,
                            first_seen: now,
                            last_seen: now,
                        },
                    );
                }
            }
        }

        let state = self.scan_state_mut(provider);
        match fact {
            CatalogFact::Owned => {
                state.last_catalog_at = Some(now);
                state.catalog_error = None;
            }
            CatalogFact::Family => {
                state.last_family_at = Some(now);
                state.family_error = None;
            }
        }
    }

    fn scan_state_mut(&mut self, provider: &str) -> &mut ProviderScanState {
        self.scans
            .entry(provider.to_owned())
            .or_insert_with(|| ProviderScanState {
                provider: provider.to_owned(),
                ..ProviderScanState::default()
            })
    }

    /// Drops the account's ownership facts without deleting entries, matches or covers.
    fn clear_catalog(&mut self, provider: &str) {
        for entry in self
            .entries
            .values_mut()
            .filter(|entry| entry.provider == provider)
        {
            entry.owned = false;
            entry.family_shared = false;
        }
        if let Some(state) = self.scans.get_mut(provider) {
            state.last_catalog_at = None;
            state.catalog_error = None;
            state.last_family_at = None;
            state.family_error = None;
        }
        self.catalog_ops.remove(provider);
        self.prune_selection();
    }

    fn on_provider_scanned(
        &mut self,
        op: u64,
        provider: String,
        result: Result<Vec<Record>, String>,
        now: i64,
    ) -> Vec<Effect> {
        let Some(run) = &mut self.scan else {
            return vec![];
        };
        if run.op != op || !run.waiting.remove(&provider) {
            return vec![];
        }
        let finished = run.waiting.is_empty();

        let outcome = match result {
            Ok(records) => ScanOutcome::Scanned(records),
            Err(message) => ScanOutcome::Failed(message),
        };
        self.merge_scan(&provider, &outcome, now);
        self.prune_selection();
        let mut effects = vec![Effect::PersistScan {
            provider,
            outcome,
            now,
        }];

        if finished {
            self.scan = None;
            if self.rescan_pending {
                effects.extend(self.start_scan());
            } else {
                effects.extend(self.plan_lookups(now));
                effects.extend(self.plan_covers(Some(now)));
            }
        }
        effects
    }

    /// Applies a scan to memory exactly as `LibraryStore::save_scan` / `save_scan_failure` apply
    /// it on disk: entries the scan finds are inserted or refreshed and installed, the provider's
    /// other entries are marked uninstalled (never deleted), and a failure changes nothing but the
    /// provider's recorded error.
    fn merge_scan(&mut self, provider: &str, outcome: &ScanOutcome, now: i64) {
        let state = self.scan_state_mut(provider);
        match outcome {
            ScanOutcome::Failed(message) => {
                state.last_error = Some(message.clone());
            }
            ScanOutcome::Scanned(records) => {
                state.last_success_at = Some(now);
                state.last_error = None;
                for entry in self
                    .entries
                    .values_mut()
                    .filter(|entry| entry.provider == provider)
                {
                    entry.installed = false;
                }
                for record in records {
                    let key = entry_key(provider, &record.id);
                    let existing = self.entries.get(&key);
                    let first_seen = existing.map_or(now, |entry| entry.first_seen);
                    let (owned, family_shared) = existing
                        .map(|entry| (entry.owned, entry.family_shared))
                        .unwrap_or_default();
                    self.entries.insert(
                        key.clone(),
                        StoredEntry {
                            key,
                            provider: provider.to_owned(),
                            store_id: record.id.clone(),
                            name: record.name.clone(),
                            install_dir: record.install_dir.clone(),
                            fields: record.fields.clone(),
                            installed: true,
                            owned,
                            family_shared,
                            first_seen,
                            last_seen: now,
                        },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
