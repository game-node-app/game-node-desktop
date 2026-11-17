//! The part of the library window that is remembered across restarts.
//!
//! The search text is deliberately not here: it starts empty on every launch.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewMode {
    #[default]
    Grid,
    List,
}

/// How large the covers in the grid are. The list view ignores it. Older layouts load as
/// [`CoverSize::Medium`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CoverSize {
    Small,
    #[default]
    Medium,
    Large,
}

/// What the sidebar scopes the library to. A store shortcut is the `All` scope plus one value in
/// [`LibraryFilters::stores`]; a saved view only highlights itself until its payload is applied.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryScope {
    #[default]
    All,
    Installed,
    Favorites,
    Recent,
    Running,
    Collection(String),
    SavedView(i64),
}

/// The sidebar selection written by builds before scopes and store facets existed. Read on load
/// and never written: [`LibraryLayout::normalized`] folds it into `scope` and `stores`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SidebarSelection {
    #[default]
    All,
    /// A provider id, such as `steam`.
    Provider(String),
}

pub type ProviderFilter = SidebarSelection;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    #[default]
    Title,
    Provider,
    #[serde(alias = "match_status")]
    MatchStatus,
    GameNodeStatus,
    ReleaseYear,
    Playtime,
    LastPlayed,
    DateAdded,
}

impl SortKey {
    /// The direction that best answers the question the key asks: dates newest first and playtime
    /// most first, names and match quality from their natural start.
    pub fn default_direction(self) -> SortDirection {
        match self {
            Self::Title | Self::Provider | Self::MatchStatus | Self::GameNodeStatus => {
                SortDirection::Ascending
            }
            Self::ReleaseYear | Self::Playtime | Self::LastPlayed | Self::DateAdded => {
                SortDirection::Descending
            }
        }
    }
}

pub type SortField = SortKey;

/// The grouping that partitions the sorted rows into collapsible sections.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupBy {
    #[default]
    None,
    Store,
    GameNodeStatus,
    MatchState,
    Installation,
    ReleaseDecade,
}

/// Which availability facts an entry may satisfy. No selection means "any entry"; selecting more
/// than one keeps entries that satisfy any of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AvailabilityFilter {
    pub installed: bool,
    pub owned: bool,
    /// Shared through the provider's declared shared access; never selected together with
    /// ownership of the same entry. Layouts saved before the rename say `family`.
    #[serde(alias = "family")]
    pub shared: bool,
}

impl AvailabilityFilter {
    /// Whether no availability fact is selected (every entry passes).
    pub fn is_any(self) -> bool {
        !(self.installed || self.owned || self.shared)
    }

    /// Only "Installed" is selected; the special "nothing installed" empty state needs this.
    pub fn is_installed_only(self) -> bool {
        self.installed && !self.owned && !self.shared
    }
}

/// One selectable match state. Unlike the old single `match_state` field, several values are
/// alternatives within the facet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchFilterValue {
    #[default]
    Matched,
    Ambiguous,
    Unmatched,
    NoGame,
}

/// One GameNode status fact the library can be narrowed to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GameNodeStatusFilter {
    Playing,
    Finished,
    Planned,
    Ongoing,
    Dropped,
    InLibrary,
    NotInLibrary,
}

/// How much the selected playtime source reports for an entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaytimeThreshold {
    #[default]
    Any,
    AtLeast10h,
    AtLeast100h,
}

/// Whether the selected playtime source reports any play at all. Unknown playtime is neither
/// played nor unplayed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayedFilter {
    #[default]
    Any,
    Played,
    Unplayed,
}

/// A relative window over the entry's last played time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LastPlayedFilter {
    #[default]
    Any,
    Last7Days,
    Last30Days,
    Last90Days,
    LastYear,
    OlderThanYear,
}

/// The entry's GameNode release year.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseFilter {
    #[default]
    Any,
    /// Every year from `Decade` to `Decade + 9`, such as `2020` for the 2020s.
    Decade(u16),
    /// Before `FIRST_RELEASE_DECADE`.
    Earlier,
    /// The matched game has no release year.
    Unknown,
}

/// A relative window over the date the library first saw the entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AddedFilter {
    #[default]
    Any,
    Last7Days,
    Last30Days,
    Last90Days,
    LastYear,
    OlderThanYear,
}

/// The old single match-state choice. Kept only to read a layout written by a previous build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LegacyMatchFilter {
    Any,
    Matched,
    Ambiguous,
    Unmatched,
    NoGame,
}

/// The old mixed playtime choice. Kept only to read a layout written by a previous build; the
/// played/unplayed values move to [`PlayedFilter`] when the filters are normalized.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum LegacyPlaytime {
    #[default]
    Any,
    Played,
    Unplayed,
    AtLeast10h,
    AtLeast100h,
}

impl LegacyPlaytime {
    /// The threshold the value maps to. Played/unplayed report no threshold.
    fn threshold(self) -> PlaytimeThreshold {
        match self {
            Self::AtLeast10h => PlaytimeThreshold::AtLeast10h,
            Self::AtLeast100h => PlaytimeThreshold::AtLeast100h,
            Self::Any | Self::Played | Self::Unplayed => PlaytimeThreshold::Any,
        }
    }

    /// The played choice the value maps to, if it carried one.
    fn played(self) -> Option<PlayedFilter> {
        match self {
            Self::Played => Some(PlayedFilter::Played),
            Self::Unplayed => Some(PlayedFilter::Unplayed),
            Self::Any | Self::AtLeast10h | Self::AtLeast100h => None,
        }
    }
}

/// Every facet the toolbar can narrow the library by. All of them combine with AND, except the
/// alternatives inside availability, stores, match states and GameNode status.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFilters {
    pub availability: AvailabilityFilter,
    /// Store ids; empty means every store. Several values are alternatives.
    pub stores: Vec<String>,
    /// Match states; empty means every state. Several values are alternatives.
    pub match_states: Vec<MatchFilterValue>,
    /// GameNode statuses; empty means every status. Several values are alternatives.
    pub gamenode_status: Vec<GameNodeStatusFilter>,
    pub playtime: PlaytimeThreshold,
    pub played: PlayedFilter,
    pub last_played: LastPlayedFilter,
    pub release: ReleaseFilter,
    pub added: AddedFilter,
    pub favorites_only: bool,
    pub running_only: bool,
    /// Reveal hidden entries in the library, counts and navigation.
    pub show_hidden: bool,
    /// The old single match state, folded into `match_states` by [`Self::normalized`]. Never
    /// written: a new save only contains the new keys.
    #[serde(skip_serializing)]
    pub(crate) legacy_match_state: Option<LegacyMatchFilter>,
    /// The played/unplayed half of the old `playtime` value, folded into `played` by
    /// [`Self::normalized`]. Never written.
    #[serde(skip_serializing)]
    pub(crate) legacy_played: Option<PlayedFilter>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LibraryFiltersSeed {
    availability: AvailabilityFilter,
    stores: Vec<String>,
    match_states: Vec<MatchFilterValue>,
    gamenode_status: Vec<GameNodeStatusFilter>,
    playtime: LegacyPlaytime,
    played: PlayedFilter,
    last_played: LastPlayedFilter,
    release: ReleaseFilter,
    added: AddedFilter,
    favorites_only: bool,
    running_only: bool,
    show_hidden: bool,
    /// The previous build stored one value under this key; `any` meant no filter.
    #[serde(rename = "matchState")]
    legacy_match_state: Option<LegacyMatchFilter>,
}

impl<'de> Deserialize<'de> for LibraryFilters {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let seed = LibraryFiltersSeed::deserialize(deserializer)?;
        Ok(Self {
            availability: seed.availability,
            stores: seed.stores,
            match_states: seed.match_states,
            gamenode_status: seed.gamenode_status,
            playtime: seed.playtime.threshold(),
            played: seed.played,
            last_played: seed.last_played,
            release: seed.release,
            added: seed.added,
            favorites_only: seed.favorites_only,
            running_only: seed.running_only,
            show_hidden: seed.show_hidden,
            legacy_match_state: seed.legacy_match_state,
            legacy_played: seed.playtime.played(),
        })
    }
}

impl LibraryFilters {
    /// Every facet at its default; no filter narrows the view.
    pub fn is_default(&self) -> bool {
        self.clone().normalized() == Self::default()
    }

    /// Whether anything is selected, so the toolbar shows chips and the view counts filtered rows.
    pub fn is_active(&self) -> bool {
        !self.is_default()
    }

    /// Folds the fields a previous build saved into their new equivalents and drops the legacy
    /// copies, so a new save only writes the new keys.
    pub fn normalized(mut self) -> Self {
        if let Some(legacy) = self.legacy_match_state.take()
            && self.match_states.is_empty()
            && legacy != LegacyMatchFilter::Any
        {
            self.match_states.push(match legacy {
                LegacyMatchFilter::Any => unreachable!("the any state was handled above"),
                LegacyMatchFilter::Matched => MatchFilterValue::Matched,
                LegacyMatchFilter::Ambiguous => MatchFilterValue::Ambiguous,
                LegacyMatchFilter::Unmatched => MatchFilterValue::Unmatched,
                LegacyMatchFilter::NoGame => MatchFilterValue::NoGame,
            });
        }
        if let Some(played) = self.legacy_played.take()
            && self.played == PlayedFilter::Any
        {
            self.played = played;
        }
        // The old release facet had a 1980s decade; the new one starts at 1990 and folds
        // everything before it into `Earlier`.
        if let ReleaseFilter::Decade(decade) = self.release
            && decade < FIRST_RELEASE_DECADE
        {
            self.release = ReleaseFilter::Earlier;
        }
        self
    }
}

/// The seconds in a day, used by every relative window.
pub const DAY_SECONDS: i64 = 86_400;
/// 365 days.
pub const YEAR_SECONDS: i64 = 365 * DAY_SECONDS;
/// How far back the `Recent` scope looks.
pub const RECENT_DAYS: i64 = 14;
/// The first decade the release filter offers; everything older is `Earlier`.
pub const FIRST_RELEASE_DECADE: u16 = 1990;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

pub type SortOrder = SortDirection;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    pub key: SortKey,
    pub direction: SortDirection,
}

pub const DEFAULT_SIDEBAR_WIDTH: u32 = 240;
pub const DEFAULT_DETAIL_WIDTH: u32 = 360;

/// View mode, scope, filters, sort, grouping, collapsed groups, pane sizes (logical pixels) and
/// whether the detail pane or the sidebar is collapsed. Every field has a default, so a layout
/// saved by an older build still loads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryLayout {
    pub view: ViewMode,
    /// The cover size the grid uses; list view is unaffected.
    pub cover_size: CoverSize,
    /// The active sidebar scope. A store shortcut is `All` plus one value in `filters.stores`.
    pub scope: LibraryScope,
    pub sort: Sort,
    /// Every toolbar facet. A layout saved before filters existed selects Installed through
    /// [`LibraryLayout::effective_filters`]; new saves always write this field.
    pub filters: LibraryFilters,
    /// How the sorted rows are partitioned.
    pub group: GroupBy,
    /// The collapsed group ids, remembered separately for each grouping.
    pub collapsed_groups: BTreeMap<GroupBy, BTreeSet<String>>,
    /// The sidebar selection from before scopes existed. Read on load and never written: it is
    /// folded into `scope` and `filters.stores` by [`LibraryLayout::normalized`].
    #[serde(rename = "filter", alias = "sidebar", default, skip_serializing)]
    pub legacy_sidebar: Option<SidebarSelection>,
    /// The single "Installed only" toggle older builds stored. Read on load and never written:
    /// it is folded into `filters` by [`LibraryLayout::effective_filters`].
    #[serde(rename = "installedOnly", default, skip_serializing)]
    pub legacy_installed_only: Option<bool>,
    #[serde(alias = "sidebar_width")]
    pub sidebar_width: u32,
    #[serde(alias = "detail_width")]
    pub detail_width: u32,
    #[serde(alias = "detail_open")]
    pub detail_open: bool,
    #[serde(alias = "sidebar_collapsed")]
    pub sidebar_collapsed: bool,
}

impl Default for LibraryLayout {
    fn default() -> Self {
        Self {
            view: ViewMode::default(),
            cover_size: CoverSize::default(),
            scope: LibraryScope::default(),
            sort: Sort::default(),
            filters: LibraryFilters::default(),
            group: GroupBy::default(),
            collapsed_groups: BTreeMap::new(),
            legacy_sidebar: None,
            legacy_installed_only: None,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            detail_width: DEFAULT_DETAIL_WIDTH,
            detail_open: true,
            sidebar_collapsed: false,
        }
    }
}

impl LibraryLayout {
    /// The filters in effect: a layout saved before filters existed had `installedOnly` instead,
    /// which selects the Installed availability fact.
    pub fn effective_filters(&self) -> LibraryFilters {
        let mut filters = self.filters.clone().normalized();
        if filters.is_default() && self.legacy_installed_only == Some(true) {
            filters.availability.installed = true;
        }
        filters
    }

    /// The same layout with every legacy field folded into its new equivalent and dropped, so the
    /// machine and the view only ever see one representation and new saves write only new keys.
    pub fn normalized(mut self) -> Self {
        self.filters = self.effective_filters();
        self.legacy_installed_only = None;
        if let Some(legacy) = self.legacy_sidebar.take()
            && let SidebarSelection::Provider(id) = legacy
        {
            // The old provider selection becomes the All scope with that store selected.
            self.scope = LibraryScope::All;
            if self.filters.stores.is_empty() {
                self.filters.stores = vec![id];
            }
        }
        self
    }
}

pub const SIDEBAR_WIDTH_RANGE: std::ops::RangeInclusive<u32> = 160..=360;
pub const DETAIL_WIDTH_RANGE: std::ops::RangeInclusive<u32> = 280..=640;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_saved_before_installed_only_existed_still_loads() {
        let layout: LibraryLayout =
            serde_json::from_str(r#"{"view":"list","sidebar":{"provider":"steam"}}"#).unwrap();
        assert_eq!(layout.view, ViewMode::List);
        assert_eq!(
            layout.legacy_sidebar,
            Some(SidebarSelection::Provider("steam".to_owned()))
        );

        let normalized = layout.normalized();
        assert_eq!(normalized.scope, LibraryScope::All);
        assert_eq!(normalized.filters.stores, ["steam"]);
        assert!(normalized.legacy_sidebar.is_none());
        assert!(!normalized.filters.availability.installed);
    }

    #[test]
    fn a_layout_saved_before_cover_sizes_existed_uses_medium() {
        let layout: LibraryLayout = serde_json::from_str(r#"{"view":"list"}"#).unwrap();
        assert_eq!(layout.cover_size, CoverSize::Medium);

        for (size, name) in [
            (CoverSize::Small, "small"),
            (CoverSize::Medium, "medium"),
            (CoverSize::Large, "large"),
        ] {
            assert_eq!(serde_json::to_value(size).unwrap(), name);
            let layout = LibraryLayout {
                cover_size: size,
                ..LibraryLayout::default()
            };
            let json = serde_json::to_value(&layout).unwrap();
            assert_eq!(json.get("coverSize"), Some(&serde_json::json!(name)));
            assert_eq!(
                serde_json::from_value::<LibraryLayout>(json).unwrap(),
                layout
            );
        }
    }

    #[test]
    fn a_layout_saved_before_the_sidebar_collapsed_flag_existed_loads_it_open() {
        // No key: the sidebar is open and the layout still loads.
        let layout: LibraryLayout = serde_json::from_str(r#"{"view":"list"}"#).unwrap();
        assert_eq!(layout.view, ViewMode::List);
        assert!(!layout.sidebar_collapsed);
        assert_eq!(layout.sidebar_width, 240);
        assert_eq!(layout.detail_width, 360);

        // The snake_case spelling used by older saves is accepted.
        let old: LibraryLayout = serde_json::from_str(r#"{"sidebar_collapsed":true}"#).unwrap();
        assert!(old.sidebar_collapsed);

        // New saves write camelCase, matching every sibling field.
        let json = serde_json::to_value(LibraryLayout {
            sidebar_collapsed: true,
            ..LibraryLayout::default()
        })
        .unwrap();
        assert_eq!(json.get("sidebarCollapsed"), Some(&serde_json::json!(true)));
        assert!(json.get("sidebar_collapsed").is_none());
        assert_eq!(json.get("sidebarWidth"), Some(&serde_json::json!(240)));
        assert_eq!(json.get("detailWidth"), Some(&serde_json::json!(360)));
    }

    #[test]
    fn the_ui_filter_key_and_the_old_sidebar_key_both_load() {
        let from_filter: LibraryLayout =
            serde_json::from_str(r#"{"filter":{"provider":"steam"}}"#).unwrap();
        let from_sidebar: LibraryLayout =
            serde_json::from_str(r#"{"sidebar":{"provider":"steam"}}"#).unwrap();
        assert_eq!(from_filter.legacy_sidebar, from_sidebar.legacy_sidebar);
        assert_eq!(
            from_filter.legacy_sidebar,
            Some(SidebarSelection::Provider("steam".to_owned()))
        );

        // A new save writes the store facet in `filters` and no old key.
        let json = serde_json::to_value(LibraryLayout {
            filters: LibraryFilters {
                stores: vec!["steam".to_owned()],
                ..LibraryFilters::default()
            },
            ..LibraryLayout::default()
        })
        .unwrap();
        assert_eq!(json.get("scope"), Some(&serde_json::json!("all")));
        assert_eq!(
            json.get("filters").and_then(|f| f.get("stores")),
            Some(&serde_json::json!(["steam"]))
        );
        assert!(json.get("filter").is_none());
        assert!(json.get("sidebar").is_none());
    }

    #[test]
    fn a_legacy_provider_selection_folds_into_the_store_filter() {
        let layout: LibraryLayout =
            serde_json::from_str(r#"{"filter":{"provider":"epic"}}"#).unwrap();
        let normalized = layout.normalized();
        assert_eq!(normalized.scope, LibraryScope::All);
        assert_eq!(normalized.filters.stores, ["epic"]);

        let json = serde_json::to_value(&normalized).unwrap();
        assert!(json.get("filter").is_none());
        assert!(json.get("sidebar").is_none());
        assert_eq!(
            json.get("scope"),
            Some(&serde_json::json!("all")),
            "the provider selection becomes the All scope"
        );
        assert_eq!(
            json.get("filters").and_then(|f| f.get("stores")),
            Some(&serde_json::json!(["epic"]))
        );
    }

    #[test]
    fn a_legacy_single_match_state_folds_into_the_match_states() {
        for (legacy, expected) in [
            ("matched", MatchFilterValue::Matched),
            ("ambiguous", MatchFilterValue::Ambiguous),
            ("unmatched", MatchFilterValue::Unmatched),
            ("noGame", MatchFilterValue::NoGame),
        ] {
            let layout: LibraryLayout =
                serde_json::from_str(&format!(r#"{{"filters":{{"matchState":"{legacy}"}}}}"#))
                    .unwrap();
            let normalized = layout.normalized();
            assert_eq!(normalized.filters.match_states, [expected], "{legacy}");
        }

        let any: LibraryLayout =
            serde_json::from_str(r#"{"filters":{"matchState":"any"}}"#).unwrap();
        assert!(any.normalized().filters.match_states.is_empty());

        let json = serde_json::to_value(
            serde_json::from_str::<LibraryLayout>(r#"{"filters":{"matchState":"ambiguous"}}"#)
                .unwrap()
                .normalized(),
        )
        .unwrap();
        assert!(
            json.get("filters")
                .and_then(|f| f.get("matchState"))
                .is_none()
        );
        assert_eq!(
            json.get("filters").and_then(|f| f.get("matchStates")),
            Some(&serde_json::json!(["ambiguous"]))
        );
    }

    #[test]
    fn legacy_playtime_values_fold_into_thresholds_and_played() {
        for (legacy, playtime, played) in [
            ("played", PlaytimeThreshold::Any, PlayedFilter::Played),
            ("unplayed", PlaytimeThreshold::Any, PlayedFilter::Unplayed),
            (
                "atLeast10h",
                PlaytimeThreshold::AtLeast10h,
                PlayedFilter::Any,
            ),
            (
                "atLeast100h",
                PlaytimeThreshold::AtLeast100h,
                PlayedFilter::Any,
            ),
        ] {
            let layout: LibraryLayout =
                serde_json::from_str(&format!(r#"{{"filters":{{"playtime":"{legacy}"}}}}"#))
                    .unwrap();
            let normalized = layout.normalized();
            assert_eq!(normalized.filters.playtime, playtime, "{legacy}");
            assert_eq!(normalized.filters.played, played, "{legacy}");
            let json = serde_json::to_value(&normalized).unwrap();
            assert_eq!(
                json.get("filters").and_then(|f| f.get("playtime")),
                Some(&serde_json::to_value(playtime).unwrap()),
                "the old key now holds the new threshold"
            );
            assert_eq!(
                json.get("filters").and_then(|f| f.get("played")),
                Some(&serde_json::to_value(played).unwrap())
            );
        }
    }

    #[test]
    fn a_legacy_1980s_release_decade_folds_into_earlier() {
        let layout: LibraryLayout =
            serde_json::from_str(r#"{"filters":{"release":{"decade":1980}}}"#).unwrap();
        let normalized = layout.normalized();
        assert_eq!(normalized.filters.release, ReleaseFilter::Earlier);
        assert_eq!(FIRST_RELEASE_DECADE, 1990);
        let json = serde_json::to_value(&normalized).unwrap();
        assert_eq!(
            json.get("filters").and_then(|f| f.get("release")),
            Some(&serde_json::json!("earlier"))
        );
        assert!(!json.to_string().contains("1980"), "{json}");
    }

    #[test]
    fn a_new_layout_round_trips_with_only_the_new_keys() {
        let mut collapsed_groups = BTreeMap::new();
        collapsed_groups.insert(
            GroupBy::Store,
            BTreeSet::from(["steam".to_owned(), "epic".to_owned()]),
        );
        let layout = LibraryLayout {
            view: ViewMode::List,
            scope: LibraryScope::Collection("backlog".to_owned()),
            sort: Sort {
                key: SortKey::GameNodeStatus,
                direction: SortDirection::Ascending,
            },
            filters: LibraryFilters {
                availability: AvailabilityFilter {
                    installed: true,
                    owned: true,
                    shared: false,
                },
                stores: vec!["steam".to_owned()],
                match_states: vec![MatchFilterValue::Matched, MatchFilterValue::Ambiguous],
                gamenode_status: vec![
                    GameNodeStatusFilter::Playing,
                    GameNodeStatusFilter::NotInLibrary,
                ],
                playtime: PlaytimeThreshold::AtLeast10h,
                played: PlayedFilter::Played,
                last_played: LastPlayedFilter::Last90Days,
                release: ReleaseFilter::Decade(2020),
                added: AddedFilter::OlderThanYear,
                favorites_only: true,
                running_only: true,
                show_hidden: true,
                ..LibraryFilters::default()
            },
            group: GroupBy::Store,
            collapsed_groups,
            sidebar_width: 180,
            detail_width: 420,
            detail_open: false,
            sidebar_collapsed: true,
            ..LibraryLayout::default()
        };
        let json = serde_json::to_value(&layout).unwrap();

        // Only the new keys are written.
        for key in [
            "filter",
            "sidebar",
            "installedOnly",
            "matchState",
            "legacySidebar",
            "legacyInstalledOnly",
            "legacyMatchState",
            "legacyPlayed",
        ] {
            assert!(json.get(key).is_none(), "{key} should not be saved");
        }
        let filters = json.get("filters").unwrap();
        for key in ["matchState", "legacyMatchState", "legacyPlayed"] {
            assert!(filters.get(key).is_none(), "{key} should not be saved");
        }
        assert_eq!(
            json.get("scope"),
            Some(&serde_json::json!({ "collection": "backlog" }))
        );
        assert_eq!(json.get("group"), Some(&serde_json::json!("store")));
        assert_eq!(json.get("coverSize"), Some(&serde_json::json!("medium")));
        assert_eq!(
            json.get("collapsedGroups"),
            Some(&serde_json::json!({ "store": ["epic", "steam"] }))
        );
        assert_eq!(
            filters.get("playtime"),
            Some(&serde_json::json!("atLeast10h"))
        );
        assert_eq!(filters.get("played"), Some(&serde_json::json!("played")));
        assert_eq!(filters.get("showHidden"), Some(&serde_json::json!(true)));
        assert_eq!(
            filters.get("gamenodeStatus"),
            Some(&serde_json::json!(["playing", "notInLibrary"]))
        );
        assert_eq!(
            serde_json::from_value::<LibraryLayout>(json).unwrap(),
            layout
        );
    }

    #[test]
    fn every_legacy_field_in_one_old_layout_is_folded() {
        // Written by the build before scopes, grouping and the split filters existed.
        let old = r#"{
            "view":"list",
            "filter":{"provider":"steam"},
            "sort":{"key":"playtime","direction":"descending"},
            "filters":{
                "availability":{"installed":true,"owned":false,"shared":false},
                "runningOnly":true,
                "matchState":"ambiguous",
                "playtime":"unplayed",
                "lastPlayed":"last30Days",
                "release":{"decade":1980},
                "added":"last90Days",
                "favoritesOnly":true
            },
            "installedOnly":true,
            "sidebarWidth":260,
            "detailWidth":380,
            "detailOpen":false,
            "sidebarCollapsed":true
        }"#;
        let layout: LibraryLayout = serde_json::from_str(old).unwrap();
        let normalized = layout.normalized();

        assert_eq!(normalized.view, ViewMode::List);
        assert_eq!(normalized.scope, LibraryScope::All);
        assert_eq!(normalized.filters.stores, ["steam"]);
        assert_eq!(
            normalized.sort,
            Sort {
                key: SortKey::Playtime,
                direction: SortDirection::Descending,
            }
        );
        assert_eq!(
            normalized.filters.availability,
            AvailabilityFilter {
                installed: true,
                owned: false,
                shared: false,
            }
        );
        assert!(normalized.filters.running_only);
        assert_eq!(
            normalized.filters.match_states,
            [MatchFilterValue::Ambiguous]
        );
        assert_eq!(normalized.filters.playtime, PlaytimeThreshold::Any);
        assert_eq!(normalized.filters.played, PlayedFilter::Unplayed);
        assert_eq!(normalized.filters.last_played, LastPlayedFilter::Last30Days);
        assert_eq!(normalized.filters.release, ReleaseFilter::Earlier);
        assert_eq!(normalized.filters.added, AddedFilter::Last90Days);
        assert!(normalized.filters.favorites_only);
        assert_eq!(normalized.group, GroupBy::None);
        assert!(normalized.collapsed_groups.is_empty());
        assert_eq!(
            (
                normalized.sidebar_width,
                normalized.detail_width,
                normalized.detail_open,
                normalized.sidebar_collapsed,
            ),
            (260, 380, false, true)
        );
        assert!(normalized.legacy_sidebar.is_none());
        assert!(normalized.legacy_installed_only.is_none());

        let json = serde_json::to_value(&normalized).unwrap();
        let text = json.to_string();
        for key in [
            "\"filter\"",
            "\"sidebar\"",
            "\"installedOnly\"",
            "\"matchState\"",
            "\"legacySidebar\"",
            "\"legacyInstalledOnly\"",
            "\"legacyMatchState\"",
            "\"legacyPlayed\"",
            "1980",
        ] {
            assert!(!text.contains(key), "{key} should not be saved: {text}");
        }
        assert!(json.get("scope").is_some());
        assert!(json.get("group").is_some());
        assert!(json.get("collapsedGroups").is_some());
    }

    #[test]
    fn explicit_filters_win_over_the_legacy_toggle() {
        let layout: LibraryLayout = serde_json::from_str(
            r#"{"installedOnly":true,"filters":{"availability":{"owned":true}}}"#,
        )
        .unwrap();
        let filters = layout.effective_filters();
        assert!(!filters.availability.installed);
        assert!(filters.availability.owned);
    }

    #[test]
    fn an_old_installed_only_layout_selects_installed_and_is_saved_without_it() {
        let layout: LibraryLayout = serde_json::from_str(r#"{"installedOnly":true}"#).unwrap();
        assert!(layout.effective_filters().availability.installed);
        assert!(!layout.effective_filters().availability.owned);

        let normalized = layout.normalized();
        assert!(normalized.legacy_installed_only.is_none());
        assert!(normalized.filters.availability.installed);
        let json = serde_json::to_value(&normalized).unwrap();
        assert!(json.get("installedOnly").is_none());
        assert_eq!(
            json.get("filters").and_then(|f| f.get("availability")),
            Some(&serde_json::json!({
                "installed": true,
                "owned": false,
                "shared": false
            }))
        );
    }

    #[test]
    fn an_old_installed_only_layout_with_a_legacy_match_state_keeps_both() {
        let layout: LibraryLayout =
            serde_json::from_str(r#"{"installedOnly":true,"filters":{"matchState":"unmatched"}}"#)
                .unwrap();
        let filters = layout.effective_filters();
        assert!(
            !filters.availability.installed,
            "explicit filters still win over the old toggle"
        );
        assert_eq!(filters.match_states, [MatchFilterValue::Unmatched]);
    }

    #[test]
    fn a_layout_saved_with_the_family_filter_restores_it_as_shared() {
        let layout: LibraryLayout =
            serde_json::from_str(r#"{"filters":{"availability":{"family":true}}}"#).unwrap();
        assert!(layout.effective_filters().availability.shared);
        assert!(!layout.effective_filters().availability.owned);

        // New saves write the shared name, and the old key is accepted on load.
        let json = serde_json::to_value(LibraryLayout {
            filters: LibraryFilters {
                availability: AvailabilityFilter {
                    shared: true,
                    ..AvailabilityFilter::default()
                },
                ..LibraryFilters::default()
            },
            ..LibraryLayout::default()
        })
        .unwrap();
        assert_eq!(
            json.get("filters").and_then(|f| f.get("availability")),
            Some(&serde_json::json!({
                "installed": false,
                "owned": false,
                "shared": true
            }))
        );
    }

    #[test]
    fn the_layout_scope_group_and_collapsed_groups_round_trip() {
        for scope in [
            LibraryScope::All,
            LibraryScope::Installed,
            LibraryScope::Favorites,
            LibraryScope::Recent,
            LibraryScope::Running,
            LibraryScope::Collection("backlog".to_owned()),
            LibraryScope::SavedView(7),
        ] {
            let json = serde_json::to_value(&scope).unwrap();
            assert_eq!(serde_json::from_value::<LibraryScope>(json).unwrap(), scope);
        }
        assert_eq!(
            serde_json::to_value(LibraryScope::Collection("backlog".to_owned())).unwrap(),
            serde_json::json!({ "collection": "backlog" })
        );
        assert_eq!(
            serde_json::to_value(LibraryScope::SavedView(7)).unwrap(),
            serde_json::json!({ "savedView": 7 })
        );

        for group in [
            GroupBy::None,
            GroupBy::Store,
            GroupBy::GameNodeStatus,
            GroupBy::MatchState,
            GroupBy::Installation,
            GroupBy::ReleaseDecade,
        ] {
            let json = serde_json::to_value(group).unwrap();
            assert_eq!(serde_json::from_value::<GroupBy>(json).unwrap(), group);
        }
        assert_eq!(
            serde_json::to_value(GroupBy::GameNodeStatus).unwrap(),
            serde_json::json!("gameNodeStatus")
        );

        let mut collapsed = BTreeMap::new();
        collapsed.insert(GroupBy::Store, BTreeSet::from(["steam".to_owned()]));
        collapsed.insert(
            GroupBy::ReleaseDecade,
            BTreeSet::from(["2020".to_owned(), "unknown".to_owned()]),
        );
        let layout = LibraryLayout {
            group: GroupBy::Store,
            collapsed_groups: collapsed,
            ..LibraryLayout::default()
        };
        let json = serde_json::to_value(&layout).unwrap();
        assert_eq!(
            serde_json::from_value::<LibraryLayout>(json).unwrap(),
            layout
        );
    }

    #[test]
    fn sort_keys_have_natural_directions_and_serialize_in_camel_case() {
        for key in [
            SortKey::Title,
            SortKey::Provider,
            SortKey::MatchStatus,
            SortKey::GameNodeStatus,
        ] {
            assert_eq!(key.default_direction(), SortDirection::Ascending);
        }
        for key in [
            SortKey::ReleaseYear,
            SortKey::Playtime,
            SortKey::LastPlayed,
            SortKey::DateAdded,
        ] {
            assert_eq!(key.default_direction(), SortDirection::Descending);
        }
        assert_eq!(
            serde_json::to_value(SortKey::ReleaseYear).unwrap(),
            serde_json::json!("releaseYear")
        );
        assert_eq!(
            serde_json::to_value(SortKey::LastPlayed).unwrap(),
            serde_json::json!("lastPlayed")
        );
        assert_eq!(
            serde_json::to_value(SortKey::GameNodeStatus).unwrap(),
            serde_json::json!("gameNodeStatus")
        );
        assert_eq!(
            serde_json::to_value(ReleaseFilter::Decade(2020)).unwrap(),
            serde_json::json!({ "decade": 2020 })
        );
        assert_eq!(
            serde_json::to_value(PlaytimeThreshold::AtLeast10h).unwrap(),
            serde_json::json!("atLeast10h")
        );
        assert_eq!(
            serde_json::to_value(PlayedFilter::Unplayed).unwrap(),
            serde_json::json!("unplayed")
        );
        assert_eq!(
            serde_json::to_value(LastPlayedFilter::OlderThanYear).unwrap(),
            serde_json::json!("olderThanYear")
        );
        assert_eq!(
            serde_json::to_value(AddedFilter::OlderThanYear).unwrap(),
            serde_json::json!("olderThanYear")
        );
    }
}
