//! The part of the library window that is remembered across restarts.
//!
//! The search text is deliberately not here: it starts empty on every launch.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    #[default]
    Grid,
    List,
}

/// What the sidebar filters by.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarSelection {
    #[default]
    All,
    /// A provider id, such as `steam`.
    Provider(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Title,
    Provider,
    MatchStatus,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    pub key: SortKey,
    pub direction: SortDirection,
}

pub const DEFAULT_SIDEBAR_WIDTH: u32 = 220;
pub const DEFAULT_DETAIL_WIDTH: u32 = 340;

/// View mode, sidebar selection, sort, pane sizes (logical pixels) and whether the detail pane
/// is open. Every field has a default, so a layout saved by an older build still loads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryLayout {
    pub view: ViewMode,
    pub sidebar: SidebarSelection,
    pub sort: Sort,
    pub sidebar_width: u32,
    pub detail_width: u32,
    pub detail_open: bool,
}

impl Default for LibraryLayout {
    fn default() -> Self {
        Self {
            view: ViewMode::default(),
            sidebar: SidebarSelection::default(),
            sort: Sort::default(),
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            detail_width: DEFAULT_DETAIL_WIDTH,
            detail_open: true,
        }
    }
}

pub const SIDEBAR_WIDTH_RANGE: std::ops::RangeInclusive<u32> = 160..=360;
pub const DETAIL_WIDTH_RANGE: std::ops::RangeInclusive<u32> = 280..=640;
