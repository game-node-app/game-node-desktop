//! The part of the library window that is remembered across restarts.
//!
//! The search text is deliberately not here: it starts empty on every launch.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewMode {
    #[default]
    Grid,
    List,
}

/// What the sidebar filters by.
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
            Self::Title | Self::Provider | Self::MatchStatus => SortDirection::Ascending,
            Self::ReleaseYear | Self::Playtime | Self::LastPlayed | Self::DateAdded => {
                SortDirection::Descending
            }
        }
    }
}

pub type SortField = SortKey;

/// Which availability facts an entry may satisfy. No selection means "any entry"; selecting more
/// than one keeps entries that satisfy any of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AvailabilityFilter {
    pub installed: bool,
    pub owned: bool,
    /// Shared through Steam Family; never selected together with ownership of the same entry.
    pub family: bool,
}

impl AvailabilityFilter {
    /// Whether no availability fact is selected (every entry passes).
    pub fn is_any(self) -> bool {
        !(self.installed || self.owned || self.family)
    }

    /// Only "Installed" is selected; the special "nothing installed" empty state needs this.
    pub fn is_installed_only(self) -> bool {
        self.installed && !self.owned && !self.family
    }
}

/// How an entry's match state is filtered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchFilter {
    #[default]
    Any,
    Matched,
    Ambiguous,
    Unmatched,
    NoGame,
}

/// How much the selected playtime source reports for an entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaytimeFilter {
    #[default]
    Any,
    /// Exactly zero reported seconds; unknown playtime is not unplayed.
    Unplayed,
    /// More than zero reported seconds.
    Played,
    AtLeast10h,
    AtLeast100h,
}

/// A relative window over the entry's last played time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LastPlayedFilter {
    #[default]
    Any,
    Last7Days,
    Last30Days,
    LastYear,
}

/// The entry's GameNode release year.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseFilter {
    #[default]
    Any,
    /// Every year from `Decade` to `Decade + 9`, such as `2020` for the 2020s.
    Decade(u16),
    /// Before 1980.
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
}

/// Every facet the toolbar can narrow the library by. All of them combine with AND, except the
/// availability facts, which are alternatives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryFilters {
    pub availability: AvailabilityFilter,
    pub running_only: bool,
    pub match_state: MatchFilter,
    pub playtime: PlaytimeFilter,
    pub last_played: LastPlayedFilter,
    pub release: ReleaseFilter,
    pub added: AddedFilter,
    pub favorites_only: bool,
}

impl LibraryFilters {
    /// Every facet at its default; no filter narrows the view.
    pub fn is_default(self) -> bool {
        self == Self::default()
    }

    /// Whether anything is selected, so the toolbar shows chips and the view counts filtered rows.
    pub fn is_active(self) -> bool {
        !self.is_default()
    }
}

/// The seconds in a day, used by every relative window.
pub const DAY_SECONDS: i64 = 86_400;
/// 365 days.
pub const YEAR_SECONDS: i64 = 365 * DAY_SECONDS;
/// The first decade the release filter offers.
pub const FIRST_RELEASE_DECADE: u16 = 1980;

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

pub const DEFAULT_SIDEBAR_WIDTH: u32 = 248;
pub const DEFAULT_DETAIL_WIDTH: u32 = 340;

/// View mode, sidebar selection, filters, sort, pane sizes (logical pixels) and whether the
/// detail pane or the sidebar is collapsed. Every field has a default, so a layout saved by an
/// older build still loads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryLayout {
    pub view: ViewMode,
    /// The React UI reads and writes this as `filter`; older saves used `sidebar`.
    #[serde(rename = "filter", alias = "sidebar")]
    pub sidebar: SidebarSelection,
    pub sort: Sort,
    /// Every toolbar facet. A layout saved before filters existed selects Installed through
    /// [`LibraryLayout::effective_filters`]; new saves always write this field.
    pub filters: LibraryFilters,
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
            sidebar: SidebarSelection::default(),
            sort: Sort::default(),
            filters: LibraryFilters::default(),
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
        let mut filters = self.filters;
        if filters.is_default() && self.legacy_installed_only == Some(true) {
            filters.availability.installed = true;
        }
        filters
    }

    /// The same layout with the legacy field folded into `filters` and dropped, so the machine
    /// and the view only ever see one representation.
    pub fn normalized(mut self) -> Self {
        self.filters = self.effective_filters();
        self.legacy_installed_only = None;
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
            layout.sidebar,
            SidebarSelection::Provider("steam".to_owned())
        );
        assert!(!layout.effective_filters().availability.installed);
    }

    #[test]
    fn a_layout_saved_before_the_sidebar_collapsed_flag_existed_loads_it_open() {
        // No key: the sidebar is open and the layout still loads.
        let layout: LibraryLayout = serde_json::from_str(r#"{"view":"list"}"#).unwrap();
        assert_eq!(layout.view, ViewMode::List);
        assert!(!layout.sidebar_collapsed);

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
    }

    #[test]
    fn the_ui_filter_key_and_the_old_sidebar_key_both_load() {
        let from_filter: LibraryLayout =
            serde_json::from_str(r#"{"filter":{"provider":"steam"}}"#).unwrap();
        let from_sidebar: LibraryLayout =
            serde_json::from_str(r#"{"sidebar":{"provider":"steam"}}"#).unwrap();
        assert_eq!(from_filter.sidebar, from_sidebar.sidebar);
        assert_eq!(
            from_filter.sidebar,
            SidebarSelection::Provider("steam".to_owned())
        );

        let json = serde_json::to_value(LibraryLayout {
            sidebar: SidebarSelection::Provider("steam".to_owned()),
            ..LibraryLayout::default()
        })
        .unwrap();
        assert_eq!(
            json.get("filter"),
            Some(&serde_json::json!({ "provider": "steam" }))
        );
        assert!(json.get("sidebar").is_none());
    }

    #[test]
    fn the_filters_round_trip() {
        let filters = LibraryFilters {
            availability: AvailabilityFilter {
                installed: true,
                owned: true,
                family: false,
            },
            running_only: true,
            match_state: MatchFilter::Ambiguous,
            playtime: PlaytimeFilter::AtLeast10h,
            last_played: LastPlayedFilter::Last30Days,
            release: ReleaseFilter::Decade(2020),
            added: AddedFilter::Last90Days,
            favorites_only: true,
        };
        let layout = LibraryLayout {
            filters,
            ..LibraryLayout::default()
        };
        let json = serde_json::to_value(&layout).unwrap();
        assert_eq!(
            json.get("filters"),
            Some(&serde_json::to_value(filters).unwrap())
        );
        assert!(json.get("installedOnly").is_none());
        assert_eq!(
            serde_json::from_value::<LibraryLayout>(json).unwrap(),
            layout
        );
    }

    #[test]
    fn an_old_installed_only_layout_selects_installed_and_is_saved_without_it() {
        let layout: LibraryLayout = serde_json::from_str(r#"{"installedOnly":true}"#).unwrap();
        assert_eq!(layout.effective_filters().availability.installed, true);
        assert_eq!(layout.effective_filters().availability.owned, false);

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
                "family": false
            }))
        );
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
    fn sort_keys_have_natural_directions_and_serialize_in_camel_case() {
        for key in [SortKey::Title, SortKey::Provider, SortKey::MatchStatus] {
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
            serde_json::to_value(ReleaseFilter::Decade(2020)).unwrap(),
            serde_json::json!({ "decade": 2020 })
        );
        assert_eq!(
            serde_json::to_value(PlaytimeFilter::AtLeast10h).unwrap(),
            serde_json::json!("atLeast10h")
        );
    }
}
