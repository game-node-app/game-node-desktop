//! The rows the library shows, and how they are filtered, searched, counted, sorted and grouped.
//!
//! Everything here is a pure function over [`Row`]s, so the rules are tested without a machine.
//! The pipeline of [`LibraryMachine::query`] is the one design decision D3 describes:
//!
//! ```text
//! scoped     = rows ∩ scope ∩ hidden-rule ∩ search
//! per facet  = scoped ∩ every other active facet
//! counts     = |per facet ∩ option|
//! visible    = scoped ∩ all facets
//! sorted     = sort(visible)
//! grouped    = stable partition of sorted by group key, in the group's fixed order
//! rows       = grouped minus rows in collapsed groups
//! ```
//!
//! Each row's facet facts are computed once into [`RowFacts`], so the per-facet passes are cheap
//! and the facet rules exist in exactly one place.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::layout::{
    AddedFilter, AvailabilityFilter, DAY_SECONDS, FIRST_RELEASE_DECADE, GroupBy, LastPlayedFilter,
    LibraryFilters, LibraryLayout, LibraryScope, MatchFilterValue, PlayedFilter, PlaytimeThreshold,
    RECENT_DAYS, ReleaseFilter, Sort, SortDirection, SortKey, YEAR_SECONDS,
};
use super::machine::LibraryMachine;
use super::matching::{self, MatchStatus};
use super::membership::EntryStatus;
use super::organization::SavedView;
use super::store::{GameRecord, StoredEntry};

/// One visible game as the view shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub playtime: crate::playtime::PlaytimeView,
    pub running: bool,
    /// Unix second the current execution began, when the game is running. `None` when stopped or
    /// when the tracker cannot know it; never fabricated from a view mount.
    #[serde(default)]
    pub session_started_at: Option<i64>,
    pub key: String,
    /// The matched game's title when there is one, else the detected title.
    pub title: String,
    /// The title the provider detected (the store id when it gave none).
    pub detected_name: String,
    /// The matched GameNode game's title, when it is known.
    pub matched_name: Option<String>,
    pub provider: String,
    pub provider_name: String,
    /// Found by the installed scan on this PC.
    pub installed: bool,
    /// Found in the connected account's owned catalog.
    pub owned: bool,
    /// Shared through the provider's declared shared access; never true when the same entry is
    /// owned.
    pub shared: bool,
    pub status: MatchStatus,
    /// File name of the cached cover under the per-user `covers/` folder.
    pub cover_file: Option<String>,
    /// The matched game's release year; unknown for unmatched entries and games without one.
    pub release_year: Option<i32>,
    /// Unix seconds of the scan that first detected the entry.
    pub added_at: i64,
    /// Whether the matched game is a favorite of the signed-in account.
    pub is_favorite: bool,
    /// Hidden from every scope, count and selection until Show hidden is on. The hidden entry is
    /// still scanned, matched, tracked and launched; hiding is only a display preference.
    #[serde(default)]
    pub hidden: bool,
    /// The cached GameNode library entry of the matched game, when the account library has one.
    /// Unmatched rows and matched games with no cached entry stay `None` and count as
    /// "not in library".
    #[serde(default)]
    pub gamenode: Option<RowGameNode>,
}

/// The part of one cached GameNode library entry a row shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowGameNode {
    /// `None` when GameNode sent a status this build does not know. The game is still in the
    /// library.
    pub status: Option<EntryStatus>,
    pub collection_ids: Vec<String>,
    /// GameNode's id for this entry.
    pub entry_id: String,
}

/// How many visible games a provider has, and how many of them by fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCount {
    /// The provider id, such as `steam`; the sidebar sends it back as its filter.
    pub id: String,
    pub name: String,
    /// Entries the active filters (except the store facet and the active scope) leave.
    pub count: usize,
    /// Entries installed on this PC: a raw fact for the sidebar subline and the store card.
    pub installed_count: usize,
    /// Entries the connected account owns: a raw fact for the store card.
    pub owned_count: usize,
    /// Entries shared through the provider's declared shared access and not owned: a raw fact
    /// for the store card.
    pub shared_count: usize,
    /// The provider's declared shared-access label, if it declares any. Only labelled providers
    /// offer the Shared filter, counts and warnings.
    pub shared_label: Option<String>,
}

/// One store option of the Store facet: how many rows it would leave under every other facet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreCount {
    pub id: String,
    pub name: String,
    pub count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityCounts {
    pub installed: usize,
    pub owned: usize,
    pub shared: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchStateCounts {
    pub matched: usize,
    pub ambiguous: usize,
    pub unmatched: usize,
    pub no_game: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameNodeStatusCounts {
    pub playing: usize,
    pub finished: usize,
    pub planned: usize,
    pub ongoing: usize,
    pub dropped: usize,
    pub in_library: usize,
    pub not_in_library: usize,
    /// False until this account's GameNode library has been read at least once. The UI disables
    /// the status facet rather than presenting every game as "not in library".
    pub available: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaytimeCounts {
    pub any: usize,
    pub at_least10h: usize,
    pub at_least100h: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayedCounts {
    pub any: usize,
    pub played: usize,
    pub unplayed: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastPlayedCounts {
    pub any: usize,
    pub last7_days: usize,
    pub last30_days: usize,
    pub last90_days: usize,
    pub last_year: usize,
    pub older_than_year: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedCounts {
    pub any: usize,
    pub last7_days: usize,
    pub last30_days: usize,
    pub last90_days: usize,
    pub last_year: usize,
    pub older_than_year: usize,
}

/// One release option the popover offers. Only options with at least one row are listed; the
/// decades come newest first, then `Earlier` and `Unknown`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCount {
    pub value: ReleaseFilter,
    pub count: usize,
}

/// The faceted counts the filter popover shows. Every count is the number of rows the option
/// would leave given the active scope, the search and every other active facet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterCounts {
    pub stores: Vec<StoreCount>,
    pub availability: AvailabilityCounts,
    pub match_states: MatchStateCounts,
    pub gamenode_status: GameNodeStatusCounts,
    pub playtime: PlaytimeCounts,
    pub played: PlayedCounts,
    pub last_played: LastPlayedCounts,
    pub release: Vec<ReleaseCount>,
    pub added: AddedCounts,
    pub favorites_only: usize,
    pub running_only: usize,
    /// How many currently hidden rows the Show hidden option would reveal, under the active
    /// scope, search and facets. Hidden rows never contribute to any other count unless
    /// `show_hidden` is on.
    pub show_hidden: usize,
}

/// The sidebar scope counts. Each applies the active facets except stores, so a store shortcut
/// never zeroes the other scopes. `all` equals the sum of the store counts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeCounts {
    pub all: usize,
    pub installed: usize,
    pub favorites: usize,
    pub recent: usize,
    pub running: usize,
}

/// The number of local rows in one GameNode collection, over the hidden-filtered rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionCount {
    pub id: String,
    pub name: String,
    pub count: usize,
}

/// The number of local rows one saved view would show now, over the hidden-filtered rows. A view
/// is `active` while its marker is the layout's highlight; its count always uses its own payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedViewCount {
    pub id: i64,
    pub name: String,
    pub count: usize,
    pub active: bool,
}

/// One group header and its rows. `first_index` indexes into the view's `rows`; a collapsed group
/// starts where the next visible group starts and contributes no rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowGroup {
    /// Stable group id, also used in `layout.collapsedGroups`.
    pub id: String,
    /// The translation key the group header renders; store groups carry their display name.
    pub label_key: String,
    /// The full row count of the group, even while collapsed.
    pub count: usize,
    pub collapsed: bool,
    /// The index of the group's first row in the view's `rows`.
    pub first_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TitleSourceKind {
    Scope,
    Store,
    Collection,
    SavedView,
}

/// What names the library header: the active scope, a single-store shortcut, a collection or a
/// saved view, with its stable id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleSource {
    pub kind: TitleSourceKind,
    pub id: String,
}

/// One view build's rows, groups and counts. The view keeps these in one pass so the counts and
/// the rows always agree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueryResult {
    pub total: usize,
    pub installed_total: usize,
    pub installed_in_scope: usize,
    /// Rows the scope, search and facets leave before grouping and collapse.
    pub visible_count: usize,
    pub rows: Vec<Row>,
    pub groups: Vec<RowGroup>,
    pub filter_counts: FilterCounts,
    pub scope_counts: ScopeCounts,
    pub title_source: Option<TitleSource>,
}

impl LibraryMachine {
    /// How an entry maps to a GameNode game right now.
    pub fn match_status(&self, entry: &StoredEntry) -> MatchStatus {
        if !self.chosen.contains_key(&entry.key)
            && let Some(game_id) = self.local_inferences.get(&entry.key)
        {
            return MatchStatus::Matched {
                game_id: *game_id,
                source: super::matching::MatchSource::Automatic,
            };
        }
        let auto = self
            .lookup_key(entry)
            .and_then(|(category, uid)| self.auto_matches.get(&(category, uid)));
        matching::resolve(self.chosen.get(&entry.key), auto)
    }

    /// The cover an entry shows: its provider plugin's override if one is set, else the matched
    /// game's own.
    fn cover_for(&self, key: &str, game: Option<&GameRecord>) -> Option<String> {
        self.artwork_overrides
            .get(key)
            .and_then(|over| over.file.clone())
            .or_else(|| game.and_then(|game| game.cover_file.clone()))
    }

    /// The cover file of the entry `key` as the library shows it, if the entry is known and has
    /// one.
    pub fn entry_cover_file(&self, key: &str) -> Option<String> {
        let entry = self.entries.get(key)?;
        let game = self
            .match_status(entry)
            .game_id()
            .and_then(|id| self.games.get(&id));
        self.cover_for(key, game)
    }

    fn row(&self, entry: &StoredEntry) -> Row {
        let status = self.match_status(entry);
        let game = status.game_id().and_then(|id| self.games.get(&id));
        let detected_name = entry.name.clone().unwrap_or_else(|| entry.store_id.clone());
        let matched_name = game
            .map(|game| game.name.clone())
            .filter(|name| !name.is_empty());
        let is_favorite = status
            .game_id()
            .and_then(|id| self.favorites.get(&id))
            .is_some_and(|favorite| favorite.is_favorite);
        let gamenode = status.game_id().and_then(|game_id| {
            self.account_library
                .as_ref()
                .and_then(|library| library.entries.get(&game_id))
                .map(|entry| RowGameNode {
                    status: entry.status,
                    collection_ids: entry.collection_ids.clone(),
                    entry_id: entry.id.clone(),
                })
        });
        Row {
            playtime: self.playtime.get(&entry.key).copied().unwrap_or_default(),
            running: self.running.contains_key(&entry.key),
            // The execution start the live session clock counts from; `None` when not running.
            session_started_at: self.running.get(&entry.key).copied(),
            key: entry.key.clone(),
            // A plugin's display override wins, then the matched game's name, then the detected one.
            title: self
                .display_overrides
                .get(&entry.key)
                .and_then(|over| over.display_name.clone())
                .or_else(|| matched_name.clone())
                .unwrap_or_else(|| detected_name.clone()),
            detected_name,
            matched_name,
            provider: entry.provider.clone(),
            provider_name: self
                .provider(&entry.provider)
                .map_or_else(|| entry.provider.clone(), |p| p.name().to_owned()),
            installed: entry.installed,
            owned: entry.owned,
            shared: entry.shared && !entry.owned,
            status,
            cover_file: self.cover_for(&entry.key, game),
            release_year: game.and_then(|game| game.release_year),
            added_at: entry.first_seen,
            is_favorite,
            hidden: self.hidden.contains(&entry.key),
            gamenode,
        }
    }

    /// Every visible entry as a row, unsorted.
    pub fn all_rows(&self) -> Vec<Row> {
        self.visible_entries()
            .map(|entry| self.row(entry))
            .collect()
    }

    /// The provider order and display names the Store facet and grouping use.
    pub fn provider_order(&self) -> Vec<(String, String)> {
        self.providers
            .iter()
            .map(|provider| (provider.id().to_owned(), provider.name().to_owned()))
            .collect()
    }

    /// The title of the first installed game that is running, without building every row.
    pub fn running_title(&self) -> Option<String> {
        self.installed_entries()
            .find(|entry| self.running.contains_key(&entry.key))
            .map(|entry| self.row(entry).title)
    }

    /// The rows for the current scope, filters, search text, sort and grouping, with rows in
    /// collapsed groups left out.
    pub fn visible_rows(&self, search: &str) -> Vec<Row> {
        self.query(search).rows
    }

    /// Builds the D3 pipeline in one pass: row facts, facets, counts, sort and groups.
    pub fn query(&self, search: &str) -> QueryResult {
        let rows = self.all_rows();
        let provider_order = self.provider_order();
        let prepared = Prepared::new(
            &rows,
            self.effective_scope(),
            &self.layout.filters,
            search,
            self.now,
        );
        let filter_counts = prepared.filter_counts(&provider_order, self.account_library.is_some());
        let scope_counts = prepared.scope_counts();
        let title_source = title_source(&self.layout, &provider_order);
        let total = prepared.total();
        let installed_total = prepared.installed_total();
        let installed_in_scope = prepared.installed_in_scope();
        let mut visible: Vec<Row> = prepared.visible_rows();
        let visible_count = visible.len();
        sort(&mut visible, self.layout.sort);
        let collapsed = self
            .layout
            .collapsed_groups
            .get(&self.layout.group)
            .cloned()
            .unwrap_or_default();
        let (rows, groups) = group_rows(visible, self.layout.group, &collapsed, &provider_order);
        QueryResult {
            total,
            installed_total,
            installed_in_scope,
            visible_count,
            rows,
            groups,
            filter_counts,
            scope_counts,
            title_source: Some(title_source),
        }
    }

    /// Visible games per loaded provider, in provider order. A provider's count reflects every
    /// active facet except stores, with no scope, so it never promises rows its shortcut would
    /// not show. The raw installed/owned/shared facts stay unfiltered (but hide hidden games).
    pub fn provider_counts(&self) -> Vec<ProviderCount> {
        let query = self.query(&self.search);
        self.provider_counts_from(&query.filter_counts.stores)
    }

    /// Builds the legacy provider cards from the store counts of one query.
    pub fn provider_counts_from(&self, stores: &[StoreCount]) -> Vec<ProviderCount> {
        let mut facts = BTreeMap::<&str, (usize, usize, usize)>::new();
        for entry in self.visible_entries() {
            if self.hidden.contains(&entry.key) && !self.layout.filters.show_hidden {
                continue;
            }
            let counts = facts.entry(entry.provider.as_str()).or_default();
            if entry.installed {
                counts.0 += 1;
            }
            if entry.owned {
                counts.1 += 1;
            } else if entry.shared {
                counts.2 += 1;
            }
        }
        stores
            .iter()
            .map(|store| {
                let (installed, owned, shared) =
                    facts.get(store.id.as_str()).copied().unwrap_or_default();
                ProviderCount {
                    id: store.id.clone(),
                    name: store.name.clone(),
                    count: store.count,
                    installed_count: installed,
                    owned_count: owned,
                    shared_count: shared,
                    shared_label: self
                        .provider(&store.id)
                        .and_then(|provider| provider.shared_label())
                        .map(str::to_owned),
                }
            })
            .collect()
    }

    /// The number of local rows in each GameNode collection, over the hidden-filtered rows.
    /// Collections are empty until the account library has been read.
    pub fn collection_counts(&self) -> Vec<CollectionCount> {
        let Some(library) = &self.account_library else {
            return Vec::new();
        };
        let show_hidden = self.layout.filters.show_hidden;
        let rows = self.all_rows();
        library
            .collections
            .iter()
            .map(|collection| CollectionCount {
                id: collection.id.clone(),
                name: collection.name.clone(),
                count: rows
                    .iter()
                    .filter(|row| show_hidden || !row.hidden)
                    .filter(|row| {
                        row.gamenode.as_ref().is_some_and(|gamenode| {
                            gamenode
                                .collection_ids
                                .iter()
                                .any(|id| id == &collection.id)
                        })
                    })
                    .count(),
            })
            .collect()
    }

    /// The number of local rows each saved view would show now: its own payload is evaluated over
    /// the hidden-filtered rows. Applying a view is a later organization concern; this only
    /// counts.
    pub fn saved_view_counts(&self) -> Vec<SavedViewCount> {
        let rows = self.all_rows();
        self.saved_views
            .iter()
            .map(|view| SavedViewCount {
                id: view.id,
                name: view.name.clone(),
                count: saved_view_count(&rows, view, self.now),
                active: matches!(self.layout.scope, LibraryScope::SavedView(active) if active == view.id),
            })
            .collect()
    }
}

fn saved_view_count(rows: &[Row], view: &SavedView, now: i64) -> usize {
    let payload = view.payload.normalized();
    filter(rows, &payload.scope, &payload.filters, "", now).len()
}

const FACET_AVAILABILITY: u16 = 1 << 0;
const FACET_STORES: u16 = 1 << 1;
const FACET_MATCH_STATES: u16 = 1 << 2;
const FACET_GAMENODE_STATUS: u16 = 1 << 3;
const FACET_PLAYTIME: u16 = 1 << 4;
const FACET_PLAYED: u16 = 1 << 5;
const FACET_LAST_PLAYED: u16 = 1 << 6;
const FACET_RELEASE: u16 = 1 << 7;
const FACET_ADDED: u16 = 1 << 8;
const FACET_FAVORITES: u16 = 1 << 9;
const FACET_RUNNING: u16 = 1 << 10;
const F_ALL: u16 = (1 << 11) - 1;

const AVAILABILITY_INSTALLED: u8 = 1 << 0;
const AVAILABILITY_OWNED: u8 = 1 << 1;
const AVAILABILITY_SHARED: u8 = 1 << 2;

const MATCH_MATCHED: u8 = 1 << 0;
const MATCH_AMBIGUOUS: u8 = 1 << 1;
const MATCH_UNMATCHED: u8 = 1 << 2;
const MATCH_NO_GAME: u8 = 1 << 3;

const STATUS_PLAYING: u8 = 1 << 0;
const STATUS_FINISHED: u8 = 1 << 1;
const STATUS_PLANNED: u8 = 1 << 2;
const STATUS_ONGOING: u8 = 1 << 3;
const STATUS_DROPPED: u8 = 1 << 4;
const STATUS_IN_LIBRARY: u8 = 1 << 5;
const STATUS_NOT_IN_LIBRARY: u8 = 1 << 6;

const PLAYTIME_10H: u8 = 1 << 0;
const PLAYTIME_100H: u8 = 1 << 1;

const PLAYED: u8 = 1 << 0;
const UNPLAYED: u8 = 1 << 1;

const WINDOW_7_DAYS: u8 = 1 << 0;
const WINDOW_30_DAYS: u8 = 1 << 1;
const WINDOW_90_DAYS: u8 = 1 << 2;
const WINDOW_YEAR: u8 = 1 << 3;
const WINDOW_OLDER: u8 = 1 << 4;

/// Everything one row is compared by, computed once. The facet bitmask lets the counts ask
/// "does this row satisfy every facet except this one?" without re-evaluating any facet.
struct RowFacts {
    /// Facets that pass for this row; inactive facets always pass.
    facet_pass: u16,
    search: bool,
    hidden: bool,
    availability: u8,
    match_states: u8,
    gamenode_status: u8,
    playtime: u8,
    played: u8,
    last_played: u8,
    added: u8,
    release_year: Option<i32>,
    is_favorite: bool,
    running: bool,
}

impl RowFacts {
    fn new(row: &Row, filters: &LibraryFilters, needle: &str, now: i64) -> Self {
        let availability = (u8::from(row.installed) * AVAILABILITY_INSTALLED)
            | (u8::from(row.owned) * AVAILABILITY_OWNED)
            | (u8::from(row.shared) * AVAILABILITY_SHARED);
        let match_states = match row.status {
            MatchStatus::Matched { .. } => MATCH_MATCHED,
            MatchStatus::Ambiguous { .. } => MATCH_AMBIGUOUS,
            MatchStatus::Unmatched => MATCH_UNMATCHED,
            MatchStatus::NoGame => MATCH_NO_GAME,
        };
        let gamenode_status = gamenode_status_mask(row);
        let playtime = playtime_mask(row.playtime.seconds);
        let played = played_mask(row.playtime.seconds);
        let last_played = window_mask(row.playtime.last_played_at, now);
        let added = window_mask(Some(row.added_at), now);

        let mut facet_pass = 0;
        if matches_availability(filters.availability, row) {
            facet_pass |= FACET_AVAILABILITY;
        }
        if matches_stores(&filters.stores, row) {
            facet_pass |= FACET_STORES;
        }
        if matches_match_masks(&filters.match_states, match_states) {
            facet_pass |= FACET_MATCH_STATES;
        }
        if matches_gamenode_masks(&filters.gamenode_status, gamenode_status) {
            facet_pass |= FACET_GAMENODE_STATUS;
        }
        if matches_playtime_mask(filters.playtime, playtime) {
            facet_pass |= FACET_PLAYTIME;
        }
        if matches_played_mask(filters.played, played) {
            facet_pass |= FACET_PLAYED;
        }
        if matches_last_played_mask(filters.last_played, last_played) {
            facet_pass |= FACET_LAST_PLAYED;
        }
        if matches_release(filters.release, row.release_year) {
            facet_pass |= FACET_RELEASE;
        }
        if matches_added_mask(filters.added, added) {
            facet_pass |= FACET_ADDED;
        }
        if !filters.favorites_only || row.is_favorite {
            facet_pass |= FACET_FAVORITES;
        }
        if !filters.running_only || row.running {
            facet_pass |= FACET_RUNNING;
        }

        Self {
            facet_pass,
            search: needle.is_empty() || contains(row, needle),
            hidden: row.hidden,
            availability,
            match_states,
            gamenode_status,
            playtime,
            played,
            last_played,
            added,
            release_year: row.release_year,
            is_favorite: row.is_favorite,
            running: row.running,
        }
    }

    fn passes(&self) -> bool {
        self.facet_pass == F_ALL
    }

    fn passes_others(&self, facet: u16) -> bool {
        self.facet_pass | facet == F_ALL
    }
}

struct Prepared<'a> {
    rows: &'a [Row],
    facts: Vec<RowFacts>,
    filters: &'a LibraryFilters,
    scope: &'a LibraryScope,
    now: i64,
}

impl<'a> Prepared<'a> {
    fn new(
        rows: &'a [Row],
        scope: &'a LibraryScope,
        filters: &'a LibraryFilters,
        search: &str,
        now: i64,
    ) -> Self {
        let needle = search.trim().to_lowercase();
        Self {
            rows,
            facts: rows
                .iter()
                .map(|row| RowFacts::new(row, filters, &needle, now))
                .collect(),
            filters,
            scope,
            now,
        }
    }

    fn hidden_allowed(&self, facts: &RowFacts) -> bool {
        self.filters.show_hidden || !facts.hidden
    }

    fn scoped(&self, row: &Row, facts: &RowFacts) -> bool {
        self.hidden_allowed(facts) && facts.search && matches_scope(self.scope, row, self.now)
    }

    fn visible(&self, index: usize) -> bool {
        let row = &self.rows[index];
        let facts = &self.facts[index];
        self.scoped(row, facts) && facts.passes()
    }

    fn visible_rows(&self) -> Vec<Row> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(index, _)| self.visible(*index))
            .map(|(_, row)| row.clone())
            .collect()
    }

    fn total(&self) -> usize {
        self.facts
            .iter()
            .filter(|facts| self.hidden_allowed(facts))
            .count()
    }

    fn installed_total(&self) -> usize {
        self.rows
            .iter()
            .zip(&self.facts)
            .filter(|(row, facts)| row.installed && self.hidden_allowed(facts))
            .count()
    }

    fn installed_in_scope(&self) -> usize {
        self.rows
            .iter()
            .zip(&self.facts)
            .filter(|(row, facts)| {
                row.installed
                    && self.hidden_allowed(facts)
                    && matches_scope(self.scope, row, self.now)
                    && matches_stores(&self.filters.stores, row)
            })
            .count()
    }

    fn filter_counts(
        &self,
        providers: &[(String, String)],
        gamenode_available: bool,
    ) -> FilterCounts {
        let mut counts = FilterCounts {
            gamenode_status: GameNodeStatusCounts {
                available: gamenode_available,
                ..GameNodeStatusCounts::default()
            },
            ..FilterCounts::default()
        };

        // Store counts ignore the store facet and the active scope: selecting a store resets the
        // scope to All, so its sidebar count must not depend on either.
        counts.stores = self.store_counts(providers);

        for (index, facts) in self.facts.iter().enumerate() {
            let row = &self.rows[index];
            if !self.scoped(row, facts) {
                continue;
            }

            if facts.passes_others(FACET_AVAILABILITY) {
                if facts.availability & AVAILABILITY_INSTALLED != 0 {
                    counts.availability.installed += 1;
                }
                if facts.availability & AVAILABILITY_OWNED != 0 {
                    counts.availability.owned += 1;
                }
                if facts.availability & AVAILABILITY_SHARED != 0 {
                    counts.availability.shared += 1;
                }
            }

            if facts.passes_others(FACET_MATCH_STATES) {
                if facts.match_states & MATCH_MATCHED != 0 {
                    counts.match_states.matched += 1;
                }
                if facts.match_states & MATCH_AMBIGUOUS != 0 {
                    counts.match_states.ambiguous += 1;
                }
                if facts.match_states & MATCH_UNMATCHED != 0 {
                    counts.match_states.unmatched += 1;
                }
                if facts.match_states & MATCH_NO_GAME != 0 {
                    counts.match_states.no_game += 1;
                }
            }

            if facts.passes_others(FACET_GAMENODE_STATUS) {
                if facts.gamenode_status & STATUS_PLAYING != 0 {
                    counts.gamenode_status.playing += 1;
                }
                if facts.gamenode_status & STATUS_FINISHED != 0 {
                    counts.gamenode_status.finished += 1;
                }
                if facts.gamenode_status & STATUS_PLANNED != 0 {
                    counts.gamenode_status.planned += 1;
                }
                if facts.gamenode_status & STATUS_ONGOING != 0 {
                    counts.gamenode_status.ongoing += 1;
                }
                if facts.gamenode_status & STATUS_DROPPED != 0 {
                    counts.gamenode_status.dropped += 1;
                }
                if facts.gamenode_status & STATUS_IN_LIBRARY != 0 {
                    counts.gamenode_status.in_library += 1;
                }
                if facts.gamenode_status & STATUS_NOT_IN_LIBRARY != 0 {
                    counts.gamenode_status.not_in_library += 1;
                }
            }

            if facts.passes_others(FACET_PLAYTIME) {
                counts.playtime.any += 1;
                if facts.playtime & PLAYTIME_10H != 0 {
                    counts.playtime.at_least10h += 1;
                }
                if facts.playtime & PLAYTIME_100H != 0 {
                    counts.playtime.at_least100h += 1;
                }
            }

            if facts.passes_others(FACET_PLAYED) {
                counts.played.any += 1;
                if facts.played & PLAYED != 0 {
                    counts.played.played += 1;
                }
                if facts.played & UNPLAYED != 0 {
                    counts.played.unplayed += 1;
                }
            }

            if facts.passes_others(FACET_LAST_PLAYED) {
                counts.last_played.any += 1;
                if facts.last_played & WINDOW_7_DAYS != 0 {
                    counts.last_played.last7_days += 1;
                }
                if facts.last_played & WINDOW_30_DAYS != 0 {
                    counts.last_played.last30_days += 1;
                }
                if facts.last_played & WINDOW_90_DAYS != 0 {
                    counts.last_played.last90_days += 1;
                }
                if facts.last_played & WINDOW_YEAR != 0 {
                    counts.last_played.last_year += 1;
                }
                if facts.last_played & WINDOW_OLDER != 0 {
                    counts.last_played.older_than_year += 1;
                }
            }

            if facts.passes_others(FACET_ADDED) {
                counts.added.any += 1;
                if facts.added & WINDOW_7_DAYS != 0 {
                    counts.added.last7_days += 1;
                }
                if facts.added & WINDOW_30_DAYS != 0 {
                    counts.added.last30_days += 1;
                }
                if facts.added & WINDOW_90_DAYS != 0 {
                    counts.added.last90_days += 1;
                }
                if facts.added & WINDOW_YEAR != 0 {
                    counts.added.last_year += 1;
                }
                if facts.added & WINDOW_OLDER != 0 {
                    counts.added.older_than_year += 1;
                }
            }

            if facts.is_favorite && facts.passes_others(FACET_FAVORITES) {
                counts.favorites_only += 1;
            }
            if facts.running && facts.passes_others(FACET_RUNNING) {
                counts.running_only += 1;
            }
        }

        counts.release = self.release_counts();
        counts.show_hidden = self.hidden_count();
        counts
    }

    fn store_counts(&self, providers: &[(String, String)]) -> Vec<StoreCount> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for (index, facts) in self.facts.iter().enumerate() {
            if self.hidden_allowed(facts) && facts.search && facts.passes_others(FACET_STORES) {
                *counts
                    .entry(self.rows[index].provider.as_str())
                    .or_default() += 1;
            }
        }
        let mut result: Vec<StoreCount> = providers
            .iter()
            .map(|(id, name)| StoreCount {
                id: id.clone(),
                name: name.clone(),
                count: counts.remove(id.as_str()).unwrap_or(0),
            })
            .collect();
        // A provider that is no longer registered but still has rows keeps a count, so the sum
        // always equals the All scope count.
        for (id, count) in counts {
            result.push(StoreCount {
                id: id.to_owned(),
                name: id.to_owned(),
                count,
            });
        }
        result
    }

    fn release_counts(&self) -> Vec<ReleaseCount> {
        let mut decades = BTreeMap::<u16, usize>::new();
        let mut earlier = 0usize;
        let mut unknown = 0usize;
        for (index, facts) in self.facts.iter().enumerate() {
            let row = &self.rows[index];
            if !self.scoped(row, facts) || !facts.passes_others(FACET_RELEASE) {
                continue;
            }
            match facts.release_year {
                Some(year) if year < i32::from(FIRST_RELEASE_DECADE) => earlier += 1,
                Some(year) => {
                    let decade = (year / 10 * 10) as u16;
                    *decades.entry(decade).or_default() += 1;
                }
                None => unknown += 1,
            }
        }
        let mut release: Vec<ReleaseCount> = decades
            .into_iter()
            .rev()
            .map(|(decade, count)| ReleaseCount {
                value: ReleaseFilter::Decade(decade),
                count,
            })
            .collect();
        if earlier > 0 {
            release.push(ReleaseCount {
                value: ReleaseFilter::Earlier,
                count: earlier,
            });
        }
        if unknown > 0 {
            release.push(ReleaseCount {
                value: ReleaseFilter::Unknown,
                count: unknown,
            });
        }
        release
    }

    fn hidden_count(&self) -> usize {
        self.rows
            .iter()
            .zip(&self.facts)
            .filter(|(row, facts)| {
                facts.hidden
                    && facts.search
                    && matches_scope(self.scope, row, self.now)
                    && facts.passes()
            })
            .count()
    }

    fn scope_counts(&self) -> ScopeCounts {
        let mut counts = ScopeCounts::default();
        for (index, facts) in self.facts.iter().enumerate() {
            let row = &self.rows[index];
            if !self.hidden_allowed(facts) || !facts.search || !facts.passes_others(FACET_STORES) {
                continue;
            }
            counts.all += 1;
            if row.installed {
                counts.installed += 1;
            }
            if row.is_favorite {
                counts.favorites += 1;
            }
            if is_recent(row, self.now) {
                counts.recent += 1;
            }
            if row.running {
                counts.running += 1;
            }
        }
        counts
    }
}

/// Keeps the rows that the scope, every active facet and `search` allow, in input order.
///
/// Facets combine with AND; availability, stores, match states and GameNode statuses are
/// alternatives within their facet. Unknown playtime, last played and release never satisfy a
/// threshold or window. Blank search keeps everything; search is title-only and case-insensitive.
pub fn filter(
    rows: &[Row],
    scope: &LibraryScope,
    filters: &LibraryFilters,
    search: &str,
    now: i64,
) -> Vec<Row> {
    Prepared::new(rows, scope, filters, search, now).visible_rows()
}

/// Whether `row` contains `needle` (already trimmed and lowercased) in its detected or matched
/// title.
fn contains(row: &Row, needle: &str) -> bool {
    row.detected_name.to_lowercase().contains(needle)
        || row
            .matched_name
            .as_ref()
            .is_some_and(|name| name.to_lowercase().contains(needle))
}

/// The scope predicate. `SavedView` is only a highlight marker; applying a view copies its
/// payload into the layout, so the pipeline treats it as All.
fn matches_scope(scope: &LibraryScope, row: &Row, now: i64) -> bool {
    match scope {
        LibraryScope::All | LibraryScope::SavedView(_) => true,
        LibraryScope::Installed => row.installed,
        LibraryScope::Favorites => row.is_favorite,
        LibraryScope::Recent => is_recent(row, now),
        LibraryScope::Running => row.running,
        LibraryScope::Collection(id) => row
            .gamenode
            .as_ref()
            .is_some_and(|gamenode| gamenode.collection_ids.iter().any(|item| item == id)),
    }
}

fn is_recent(row: &Row, now: i64) -> bool {
    row.playtime
        .last_played_at
        .is_some_and(|at| at >= now.saturating_sub(RECENT_DAYS * DAY_SECONDS))
}

fn matches_availability(filter: AvailabilityFilter, row: &Row) -> bool {
    filter.is_any()
        || (filter.installed && row.installed)
        || (filter.owned && row.owned)
        || (filter.shared && row.shared)
}

fn matches_stores(stores: &[String], row: &Row) -> bool {
    stores.is_empty() || stores.iter().any(|store| store == &row.provider)
}

fn matches_match_masks(filters: &[MatchFilterValue], mask: u8) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| match filter {
            MatchFilterValue::Matched => mask & MATCH_MATCHED != 0,
            MatchFilterValue::Ambiguous => mask & MATCH_AMBIGUOUS != 0,
            MatchFilterValue::Unmatched => mask & MATCH_UNMATCHED != 0,
            MatchFilterValue::NoGame => mask & MATCH_NO_GAME != 0,
        })
}

fn matches_gamenode_masks(filters: &[super::layout::GameNodeStatusFilter], mask: u8) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            let bit = match filter {
                super::layout::GameNodeStatusFilter::Playing => STATUS_PLAYING,
                super::layout::GameNodeStatusFilter::Finished => STATUS_FINISHED,
                super::layout::GameNodeStatusFilter::Planned => STATUS_PLANNED,
                super::layout::GameNodeStatusFilter::Ongoing => STATUS_ONGOING,
                super::layout::GameNodeStatusFilter::Dropped => STATUS_DROPPED,
                super::layout::GameNodeStatusFilter::InLibrary => STATUS_IN_LIBRARY,
                super::layout::GameNodeStatusFilter::NotInLibrary => STATUS_NOT_IN_LIBRARY,
            };
            mask & bit != 0
        })
}

/// Unknown playtime never satisfies a threshold; only `Any` lets it through.
fn playtime_mask(seconds: Option<i64>) -> u8 {
    let Some(seconds) = seconds else {
        return 0;
    };
    let mut mask = 0;
    if seconds >= 10 * 3600 {
        mask |= PLAYTIME_10H;
    }
    if seconds >= 100 * 3600 {
        mask |= PLAYTIME_100H;
    }
    mask
}

fn matches_playtime_mask(filter: PlaytimeThreshold, mask: u8) -> bool {
    match filter {
        PlaytimeThreshold::Any => true,
        PlaytimeThreshold::AtLeast10h => mask & PLAYTIME_10H != 0,
        PlaytimeThreshold::AtLeast100h => mask & PLAYTIME_100H != 0,
    }
}

/// Unknown playtime is neither played nor unplayed.
fn played_mask(seconds: Option<i64>) -> u8 {
    match seconds {
        Some(seconds) if seconds > 0 => PLAYED,
        Some(0) => UNPLAYED,
        _ => 0,
    }
}

fn matches_played_mask(filter: PlayedFilter, mask: u8) -> bool {
    match filter {
        PlayedFilter::Any => true,
        PlayedFilter::Played => mask & PLAYED != 0,
        PlayedFilter::Unplayed => mask & UNPLAYED != 0,
    }
}

/// A timestamp can satisfy several nested windows. Unknown satisfies none.
fn window_mask(at: Option<i64>, now: i64) -> u8 {
    let Some(at) = at else {
        return 0;
    };
    if at < now.saturating_sub(YEAR_SECONDS) {
        WINDOW_OLDER
    } else if at >= now.saturating_sub(7 * DAY_SECONDS) {
        WINDOW_7_DAYS | WINDOW_30_DAYS | WINDOW_90_DAYS | WINDOW_YEAR
    } else if at >= now.saturating_sub(30 * DAY_SECONDS) {
        WINDOW_30_DAYS | WINDOW_90_DAYS | WINDOW_YEAR
    } else if at >= now.saturating_sub(90 * DAY_SECONDS) {
        WINDOW_90_DAYS | WINDOW_YEAR
    } else {
        WINDOW_YEAR
    }
}

fn matches_last_played_mask(filter: LastPlayedFilter, mask: u8) -> bool {
    match filter {
        LastPlayedFilter::Any => true,
        LastPlayedFilter::Last7Days => mask & WINDOW_7_DAYS != 0,
        LastPlayedFilter::Last30Days => mask & WINDOW_30_DAYS != 0,
        LastPlayedFilter::Last90Days => mask & WINDOW_90_DAYS != 0,
        LastPlayedFilter::LastYear => mask & WINDOW_YEAR != 0,
        // A known timestamp strictly older than a year; unknown never matches.
        LastPlayedFilter::OlderThanYear => mask & WINDOW_OLDER != 0,
    }
}

fn matches_added_mask(filter: AddedFilter, mask: u8) -> bool {
    match filter {
        AddedFilter::Any => true,
        AddedFilter::Last7Days => mask & WINDOW_7_DAYS != 0,
        AddedFilter::Last30Days => mask & WINDOW_30_DAYS != 0,
        AddedFilter::Last90Days => mask & WINDOW_90_DAYS != 0,
        AddedFilter::LastYear => mask & WINDOW_YEAR != 0,
        AddedFilter::OlderThanYear => mask & WINDOW_OLDER != 0,
    }
}

fn matches_release(filter: ReleaseFilter, year: Option<i32>) -> bool {
    match filter {
        ReleaseFilter::Any => true,
        ReleaseFilter::Decade(decade) => {
            let start = i32::from(decade);
            year.is_some_and(|year| (start..start + 10).contains(&year))
        }
        ReleaseFilter::Earlier => year.is_some_and(|year| year < i32::from(FIRST_RELEASE_DECADE)),
        ReleaseFilter::Unknown => year.is_none(),
    }
}

fn gamenode_status_mask(row: &Row) -> u8 {
    match &row.gamenode {
        None => STATUS_NOT_IN_LIBRARY,
        Some(gamenode) => match gamenode.status {
            Some(EntryStatus::Playing) => STATUS_PLAYING,
            Some(EntryStatus::Finished) => STATUS_FINISHED,
            Some(EntryStatus::Planned) => STATUS_PLANNED,
            Some(EntryStatus::Ongoing) => STATUS_ONGOING,
            Some(EntryStatus::Dropped) => STATUS_DROPPED,
            None => STATUS_IN_LIBRARY,
        },
    }
}

/// The rank `SortKey::GameNodeStatus` uses: playing, ongoing, planned, finished, dropped, in
/// library without a status, not on GameNode. This deliberately differs from the group order.
fn gamenode_sort_rank(row: &Row) -> u8 {
    match gamenode_status_mask(row) {
        STATUS_PLAYING => 0,
        STATUS_ONGOING => 1,
        STATUS_PLANNED => 2,
        STATUS_FINISHED => 3,
        STATUS_DROPPED => 4,
        STATUS_IN_LIBRARY => 5,
        _ => 6,
    }
}

fn folded(text: &str) -> String {
    text.to_lowercase()
}

/// The values each row is compared by, computed once per row instead of once per comparison.
struct SortKeys {
    title: String,
    /// Only filled when sorting by provider.
    provider: String,
}

fn by_title(a: &Row, a_keys: &SortKeys, b: &Row, b_keys: &SortKeys) -> Ordering {
    a_keys
        .title
        .cmp(&b_keys.title)
        .then_with(|| a.title.cmp(&b.title))
}

/// Compares two optional values so that a known value always comes before an unknown one,
/// whichever direction the user chose. The direction only orders two known values.
fn compare_option<T: Ord>(a: Option<T>, b: Option<T>, direction: SortDirection) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => match direction {
            SortDirection::Ascending => a.cmp(&b),
            SortDirection::Descending => b.cmp(&a),
        },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn directed(order: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Ascending => order,
        SortDirection::Descending => order.reverse(),
    }
}

fn compare(a: (&Row, &SortKeys), b: (&Row, &SortKeys), sort: Sort) -> Ordering {
    let direction = sort.direction;
    match sort.key {
        SortKey::Title => directed(by_title(a.0, a.1, b.0, b.1), direction),
        SortKey::Provider => directed(a.1.provider.cmp(&b.1.provider), direction),
        SortKey::MatchStatus => directed(a.0.status.rank().cmp(&b.0.status.rank()), direction),
        SortKey::GameNodeStatus => directed(
            gamenode_sort_rank(a.0).cmp(&gamenode_sort_rank(b.0)),
            direction,
        ),
        // Unknown-aware keys apply the direction inside `compare_option`, so unknown values stay
        // last whichever direction the user chose.
        SortKey::ReleaseYear => compare_option(a.0.release_year, b.0.release_year, direction),
        SortKey::Playtime => compare_option(a.0.playtime.seconds, b.0.playtime.seconds, direction),
        SortKey::LastPlayed => compare_option(
            a.0.playtime.last_played_at,
            b.0.playtime.last_played_at,
            direction,
        ),
        SortKey::DateAdded => compare_option(Some(a.0.added_at), Some(b.0.added_at), direction),
    }
}

/// Ties are broken the same way in both directions: only the leading key flips. The GameNode
/// status sort explicitly ties by title, then key.
fn tie_break(a: (&Row, &SortKeys), b: (&Row, &SortKeys), key: SortKey) -> Ordering {
    match key {
        SortKey::Title => Ordering::Equal
            .then_with(|| a.0.provider.cmp(&b.0.provider))
            .then_with(|| a.0.key.cmp(&b.0.key)),
        SortKey::GameNodeStatus => by_title(a.0, a.1, b.0, b.1).then_with(|| a.0.key.cmp(&b.0.key)),
        _ => by_title(a.0, a.1, b.0, b.1)
            .then_with(|| a.0.provider.cmp(&b.0.provider))
            .then_with(|| a.0.key.cmp(&b.0.key)),
    }
}

/// Sorts in place. The sort is total (title, then provider, then key break ties), so the order
/// never depends on the order the rows arrived in.
pub fn sort(rows: &mut [Row], by: Sort) {
    let keys: Vec<SortKeys> = rows
        .iter()
        .map(|row| SortKeys {
            title: folded(&row.title),
            provider: if by.key == SortKey::Provider {
                folded(&row.provider_name)
            } else {
                String::new()
            },
        })
        .collect();
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&a, &b| {
        let (a, b) = ((&rows[a], &keys[a]), (&rows[b], &keys[b]));
        compare(a, b, by).then_with(|| tie_break(a, b, by.key))
    });
    // `target[i]` is where the row now at `i` must end up; follow each cycle with swaps.
    let mut target = vec![0; rows.len()];
    for (position, &from) in order.iter().enumerate() {
        target[from] = position;
    }
    for i in 0..rows.len() {
        while target[i] != i {
            let t = target[i];
            rows.swap(i, t);
            target.swap(i, t);
        }
    }
}

/// Partitions already-sorted rows into the fixed group order of `by`. Rows inside a group keep the
/// caller's sort. Empty groups are omitted. Collapsed groups keep their header, count and
/// `first_index`, but contribute no rows.
pub fn group_rows(
    rows: Vec<Row>,
    by: GroupBy,
    collapsed: &BTreeSet<String>,
    providers: &[(String, String)],
) -> (Vec<Row>, Vec<RowGroup>) {
    if by == GroupBy::None {
        return (rows, Vec::new());
    }

    let candidates = group_candidates(&rows, by, providers);
    let index_of: BTreeMap<&str, usize> = candidates
        .iter()
        .enumerate()
        .map(|(index, (id, _))| (id.as_str(), index))
        .collect();
    let mut buckets: Vec<Vec<Row>> = (0..candidates.len()).map(|_| Vec::new()).collect();
    let mut extra = Vec::new();
    for row in rows {
        match index_of.get(group_id(&row, by).as_str()) {
            Some(&index) => buckets[index].push(row),
            // A provider no longer in the registry still gets a group at the end.
            None => extra.push(row),
        }
    }

    let mut grouped = Vec::new();
    let mut groups = Vec::new();
    for ((id, label_key), bucket) in candidates.into_iter().zip(buckets) {
        if bucket.is_empty() {
            continue;
        }
        let first_index = grouped.len();
        let is_collapsed = collapsed.contains(&id);
        let count = bucket.len();
        if !is_collapsed {
            grouped.extend(bucket);
        }
        groups.push(RowGroup {
            id,
            label_key,
            count,
            collapsed: is_collapsed,
            first_index,
        });
    }
    if !extra.is_empty() {
        let first_index = grouped.len();
        let count = extra.len();
        grouped.extend(extra);
        groups.push(RowGroup {
            id: "other".to_owned(),
            label_key: "Library::Unknown".to_owned(),
            count,
            collapsed: false,
            first_index,
        });
    }
    (grouped, groups)
}

fn group_candidates(
    rows: &[Row],
    by: GroupBy,
    providers: &[(String, String)],
) -> Vec<(String, String)> {
    match by {
        GroupBy::None => Vec::new(),
        GroupBy::Store => {
            let mut candidates: Vec<(String, String)> = providers.to_vec();
            let known: BTreeSet<&str> = providers.iter().map(|(id, _)| id.as_str()).collect();
            let mut extras: BTreeSet<&str> = BTreeSet::new();
            for row in rows {
                if !known.contains(row.provider.as_str()) {
                    extras.insert(row.provider.as_str());
                }
            }
            for id in extras {
                candidates.push((id.to_owned(), id.to_owned()));
            }
            candidates
        }
        GroupBy::GameNodeStatus => vec![
            ("playing".to_owned(), "Library::Playing".to_owned()),
            ("finished".to_owned(), "Library::Finished".to_owned()),
            ("planned".to_owned(), "Library::Planned".to_owned()),
            ("ongoing".to_owned(), "Library::Ongoing".to_owned()),
            ("dropped".to_owned(), "Library::Dropped".to_owned()),
            ("inLibrary".to_owned(), "Library::In library".to_owned()),
            (
                "notInLibrary".to_owned(),
                "Library::Not on GameNode".to_owned(),
            ),
        ],
        GroupBy::MatchState => vec![
            ("matched".to_owned(), "Library::Matched".to_owned()),
            ("ambiguous".to_owned(), "Library::Needs review".to_owned()),
            ("unmatched".to_owned(), "Library::No match".to_owned()),
            (
                "noGame".to_owned(),
                "Library::Marked as not a game".to_owned(),
            ),
        ],
        GroupBy::Installation => vec![
            (
                "installed".to_owned(),
                "Library::Installed on this PC".to_owned(),
            ),
            (
                "notInstalled".to_owned(),
                "Library::Not installed".to_owned(),
            ),
        ],
        GroupBy::ReleaseDecade => {
            let mut decades: BTreeSet<u16> = BTreeSet::new();
            let mut earlier = false;
            let mut unknown = false;
            for row in rows {
                match row.release_year {
                    Some(year) if year < i32::from(FIRST_RELEASE_DECADE) => earlier = true,
                    Some(year) => {
                        decades.insert((year / 10 * 10) as u16);
                    }
                    None => unknown = true,
                }
            }
            let mut candidates: Vec<(String, String)> = decades
                .into_iter()
                .rev()
                .map(|decade| (decade.to_string(), "Library::{}s".to_owned()))
                .collect();
            if earlier {
                candidates.push(("earlier".to_owned(), "Library::Before 1990".to_owned()));
            }
            if unknown {
                candidates.push(("unknown".to_owned(), "Library::Unknown release".to_owned()));
            }
            candidates
        }
    }
}

pub(super) fn group_id(row: &Row, by: GroupBy) -> String {
    match by {
        GroupBy::None => String::new(),
        GroupBy::Store => row.provider.clone(),
        GroupBy::GameNodeStatus => match gamenode_status_mask(row) {
            STATUS_PLAYING => "playing",
            STATUS_FINISHED => "finished",
            STATUS_PLANNED => "planned",
            STATUS_ONGOING => "ongoing",
            STATUS_DROPPED => "dropped",
            STATUS_IN_LIBRARY => "inLibrary",
            _ => "notInLibrary",
        }
        .to_owned(),
        GroupBy::MatchState => match row.status {
            MatchStatus::Matched { .. } => "matched",
            MatchStatus::Ambiguous { .. } => "ambiguous",
            MatchStatus::Unmatched => "unmatched",
            MatchStatus::NoGame => "noGame",
        }
        .to_owned(),
        GroupBy::Installation => {
            if row.installed {
                "installed".to_owned()
            } else {
                "notInstalled".to_owned()
            }
        }
        GroupBy::ReleaseDecade => match row.release_year {
            Some(year) if year < i32::from(FIRST_RELEASE_DECADE) => "earlier".to_owned(),
            Some(year) => (year / 10 * 10).to_string(),
            None => "unknown".to_owned(),
        },
    }
}

/// The header title source for the active layout.
pub fn title_source(layout: &LibraryLayout, providers: &[(String, String)]) -> TitleSource {
    match &layout.scope {
        LibraryScope::Collection(id) => TitleSource {
            kind: TitleSourceKind::Collection,
            id: id.clone(),
        },
        LibraryScope::SavedView(id) => TitleSource {
            kind: TitleSourceKind::SavedView,
            id: id.to_string(),
        },
        LibraryScope::All => match layout.filters.stores.as_slice() {
            [id] if providers.iter().any(|(provider, _)| provider == id) => TitleSource {
                kind: TitleSourceKind::Store,
                id: id.clone(),
            },
            _ => TitleSource {
                kind: TitleSourceKind::Scope,
                id: "all".to_owned(),
            },
        },
        LibraryScope::Installed => TitleSource {
            kind: TitleSourceKind::Scope,
            id: "installed".to_owned(),
        },
        LibraryScope::Favorites => TitleSource {
            kind: TitleSourceKind::Scope,
            id: "favorites".to_owned(),
        },
        LibraryScope::Recent => TitleSource {
            kind: TitleSourceKind::Scope,
            id: "recent".to_owned(),
        },
        LibraryScope::Running => TitleSource {
            kind: TitleSourceKind::Scope,
            id: "running".to_owned(),
        },
    }
}

/// Counts rows per provider, in the order `providers` gives them (id, name). Providers with no
/// games still appear with a count of zero; the fact counts start at zero and are filled by the
/// caller when it wants them.
pub fn counts<'a>(
    rows: &[Row],
    providers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<ProviderCount> {
    providers
        .into_iter()
        .map(|(id, name)| ProviderCount {
            id: id.to_owned(),
            name: name.to_owned(),
            count: rows.iter().filter(|row| row.provider == id).count(),
            installed_count: 0,
            owned_count: 0,
            shared_count: 0,
            shared_label: None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
