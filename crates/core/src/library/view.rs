//! What the window renders for the library: plain data built from the machine.
//!
//! The view is rebuilt only after an input or a state change marked it dirty, and then compared
//! with the previous one. When nothing a user could see changed, the old view (and its generation
//! number) is kept; rows that did not change keep the same `Arc`, so the UI can update only what
//! differs.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::actions::Actions;
use super::add_dialog::AddDialogView;
use super::add_run::{AddSummary, AddSummaryView};
use super::batch::{CollectionRunView, StatusRunView, UndoAddView};
use super::catalog::CatalogView;
use super::layout::{LibraryLayout, LibraryScope};
use super::machine::{Access, LibraryMachine};
use super::match_dialog::MatchDialogView;
use super::matching::MatchStatus;
use super::membership::Membership;
use super::organization::{HiddenNotice, SavedViewNotice, ViewError, ViewWriteView};
use super::query::{
    CollectionCount, FilterCounts, ProviderCount, Row, RowGroup, SavedViewCount, ScopeCounts,
    TitleSource, TitleSourceKind,
};
use super::suggest::SuggestionView;

/// The full details of the single selected entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailView {
    pub running: bool,
    pub row: Arc<Row>,
    pub install_dir: String,
    pub release_year: Option<i32>,
    pub has_artwork: bool,
    /// The selected game's downloaded landscape artwork, when GameNode has one.
    #[serde(skip)]
    pub artwork: Option<Arc<[u8]>>,
    /// Whether the matched game is in the user's GameNode library (`NotChecked` without a match).
    pub membership: Membership,
    /// Whether this entry's provider declares an achievement backend.
    #[serde(default)]
    pub achievement_backend: bool,
    /// The compact achievement summary, present when a cache row exists.
    #[serde(default)]
    pub achievements: Option<crate::achievements::AchievementSummaryView>,
    /// Scored suggestions for an unmatched or ambiguous entry, best first, at most two.
    #[serde(default)]
    pub suggestions: Vec<SuggestionView>,
}

impl DetailView {
    pub fn set_artwork(&mut self, artwork: Option<Arc<[u8]>>) {
        self.has_artwork = artwork.is_some();
        self.artwork = artwork;
    }
}

/// Why the list is empty, when it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum EmptyState {
    /// A scan finished and found no games. Names the stores looked for.
    NoGames { stores: Vec<String> },
    /// The installed filter hides every entry while the library has games. `provider` names the
    /// sidebar's provider when one is selected.
    NoInstalled { provider: Option<String> },
    /// There are games, but the sidebar choice, installed filter and search hide all of them.
    NothingMatches,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    /// Changes whenever anything in the view changes, so the UI can skip identical updates.
    pub generation: u64,
    pub layout: LibraryLayout,
    pub search: String,
    pub access: Access,
    /// Visible games across all providers.
    pub total: usize,
    /// Visible games installed on this PC.
    pub installed_total: usize,
    pub providers: Vec<ProviderCount>,
    pub rows: Arc<[Arc<Row>]>,
    /// Faceted option counts for the filter popover.
    pub filter_counts: FilterCounts,
    /// Sidebar scope counts.
    pub scope_counts: ScopeCounts,
    /// Group headers, in their fixed order. Collapsed groups contribute no `rows`.
    pub groups: Vec<RowGroup>,
    /// The account's saved views in sidebar order, each with its current row count and whether it
    /// is the highlighted one.
    pub saved_views: Vec<SavedViewCount>,
    /// The account's GameNode collections with the number of local rows in each.
    pub collections: Vec<CollectionCount>,
    /// Why the last view name was refused (`empty` or `duplicate`), until a valid attempt. The
    /// attempt-scoped result is `view_write`, which also covers storage failures.
    pub view_error: Option<ViewError>,
    /// The latest saved-view attempt (save or rename) and its outcome. The dialog baselines `run`
    /// and reacts to a larger one, so a failed write ends its spinner and can be retried.
    pub view_write: Option<ViewWriteView>,
    /// The last successful view save, for the notification.
    pub last_saved_view: Option<SavedViewNotice>,
    /// The last hide run, for the notification and its Show again action.
    pub last_hidden: Option<HiddenNotice>,
    /// Whether the current payload differs from the default library and can be saved.
    pub can_save_view: bool,
    /// What names the header: scope, single store, collection or saved view.
    pub title_source: TitleSource,
    /// The in-app GameNode catalog search view.
    #[serde(default)]
    pub catalog: CatalogView,
    /// Per-provider scan progress, plus the DRM-free folders pseudo row.
    #[serde(default)]
    pub scan_progress: Vec<ScanProgress>,
    /// Lightweight entries for the command palette: every non-hidden row, independent of the
    /// active filters, scopes and grouping.
    #[serde(default)]
    pub palette_index: Vec<PaletteEntry>,
    pub scanning: bool,
    /// Names of providers whose latest scan failed (their previous games are still listed).
    pub failed_providers: Vec<String>,
    /// Keys of the selected rows that are shown, in display order.
    pub selected: Vec<String>,
    pub focused: Option<String>,
    pub actions: Actions,
    pub detail: Option<DetailView>,
    pub empty: Option<EmptyState>,
    /// The match dialog, while it is open.
    pub match_dialog: Option<MatchDialogView>,
    /// The Add to GameNode dialog, while it is open.
    pub add_dialog: Option<AddDialogView>,
    /// The latest finished add, kept after its dialog closes for the notification and its Undo.
    pub add_summary: Option<AddSummaryView>,
    /// The latest undo result, for the notification.
    pub undo: Option<UndoAddView>,
    /// The latest finished batch status change, for the notification.
    pub status_run: Option<StatusRunView>,
    /// The latest finished collection add, for the notification.
    pub collection_run: Option<CollectionRunView>,
}

/// How far one provider's scan has come.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanProgressState {
    #[default]
    Waiting,
    Scanning,
    Done,
    Failed,
}

/// One row of the scanning panel. `found` is the number of records the provider reported in its
/// `ProviderScanned` answer, when it succeeded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub provider: String,
    pub name: String,
    pub state: ScanProgressState,
    pub found: Option<usize>,
}

/// One command-palette game entry. Deliberately smaller than a full row, so the palette can hold
/// thousands without carrying filters, matches or collections.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaletteEntry {
    pub key: String,
    pub title: String,
    pub provider_name: String,
    pub installed: bool,
    /// A short, language-neutral playtime such as `42 h` or `45 min`; `None` when unknown.
    pub playtime_label: Option<String>,
}

impl LibraryMachine {
    /// The view to render, or `None` while no account is set. Calling it again without a change
    /// returns the cached view: same generation, same row `Arc`s, and no rebuild.
    pub fn view(&mut self) -> Option<LibraryView> {
        if !self.is_active() {
            self.view_cache = None;
            return None;
        }
        if !self.view_dirty
            && let Some(view) = &self.view_cache
        {
            return Some(view.clone());
        }
        let previous = self.view_cache.take();
        let mut next = self.build_view(previous.as_ref());
        let view = match previous {
            Some(previous) if previous == next => previous,
            Some(previous) => {
                next.generation = previous.generation + 1;
                next
            }
            None => {
                next.generation = 1;
                next
            }
        };
        self.view_cache = Some(view.clone());
        self.view_dirty = false;
        Some(view)
    }

    fn build_view(&self, previous: Option<&LibraryView>) -> LibraryView {
        let query = self.query(&self.search);
        let fresh = query.rows;
        let rows: Arc<[Arc<Row>]> = match previous {
            // An unchanged list reuses the whole slice.
            Some(prev) if rows_equal(&prev.rows, &fresh) => prev.rows.clone(),
            _ => {
                let reusable: BTreeMap<&str, &Arc<Row>> = previous
                    .map(|view| {
                        view.rows
                            .iter()
                            .map(|row| (row.key.as_str(), row))
                            .collect()
                    })
                    .unwrap_or_default();
                fresh
                    .into_iter()
                    .map(|row| match reusable.get(row.key.as_str()) {
                        Some(old) if ***old == row => Arc::clone(old),
                        _ => Arc::new(row),
                    })
                    .collect()
            }
        };

        let shown: Vec<String> = rows.iter().map(|row| row.key.clone()).collect();
        let selected = self.selection.visible(&shown);
        let focused = self.selection.focus(&shown).map(str::to_owned);
        let detail = match selected.as_slice() {
            [key] => rows
                .iter()
                .find(|row| &row.key == key)
                .map(|row| self.detail(row)),
            _ => None,
        };
        let total = query.total;
        let installed_total = query.installed_total;
        let providers = self.provider_counts_from(&query.filter_counts.stores);
        let installed_in_view = query.installed_in_scope;
        let title_source = query.title_source.unwrap_or(TitleSource {
            kind: TitleSourceKind::Scope,
            id: "all".to_owned(),
        });

        let empty = if total == 0 {
            let scanned_everywhere = self
                .providers
                .iter()
                .all(|provider| self.scans.contains_key(provider.id()));
            (!self.scan_running() && scanned_everywhere).then(|| EmptyState::NoGames {
                stores: self.providers.iter().map(|p| p.name().to_owned()).collect(),
            })
        } else if (self.layout.filters.availability.is_installed_only()
            || matches!(self.effective_scope(), LibraryScope::Installed))
            && installed_in_view == 0
        {
            // The Installed scope and the installed-only availability fact both hide every
            // uninstalled game, so they get the actionable "nothing installed" state rather than
            // a generic no-match message. Other facets that hide installed rows keep it.
            Some(EmptyState::NoInstalled {
                provider: match title_source.kind {
                    TitleSourceKind::Store => Some(title_source.id.clone()),
                    // An Installed scope or view can still carry a single-store facet; the
                    // actionable message stays about that store.
                    _ => match self.layout.filters.stores.as_slice() {
                        [store] => Some(store.clone()),
                        _ => None,
                    },
                },
            })
        } else if query.visible_count == 0 {
            Some(EmptyState::NothingMatches)
        } else {
            None
        };

        LibraryView {
            generation: previous.map_or(0, |view| view.generation),
            layout: self.layout.clone(),
            search: self.search.clone(),
            access: self.access,
            total,
            installed_total,
            providers,
            rows,
            filter_counts: query.filter_counts,
            scope_counts: query.scope_counts,
            groups: query.groups,
            saved_views: self.saved_view_counts(),
            collections: self.collection_counts(),
            view_error: self.view_error,
            view_write: self.view_write.clone(),
            last_saved_view: self.last_saved_view.clone(),
            last_hidden: self.last_hidden.clone(),
            can_save_view: self.can_save_view(),
            title_source,
            catalog: self.catalog_view(),
            scan_progress: self.scan_progress_view(),
            palette_index: self.palette_index(),
            scanning: self.scan_running(),
            failed_providers: self
                .providers
                .iter()
                .filter(|provider| {
                    self.scans
                        .get(provider.id())
                        .is_some_and(|state| state.last_error.is_some())
                })
                .map(|provider| provider.name().to_owned())
                .collect(),
            selected,
            focused,
            actions: self.actions(),
            detail,
            empty,
            match_dialog: self.match_dialog_view(),
            add_dialog: self.add_dialog_view(),
            add_summary: self.last_add.as_ref().map(AddSummary::view),
            undo: self.last_undo.clone(),
            status_run: self.last_status_run.clone(),
            collection_run: self.last_collection_run.clone(),
        }
    }

    fn detail(&self, row: &Arc<Row>) -> DetailView {
        let entry = self.entries.get(&row.key);
        let game_id = row.status.game_id();
        let artwork = game_id
            .filter(|id| self.artwork_focus == Some(*id))
            .and_then(|_| self.artwork.clone());
        let has_artwork = artwork.is_some();
        DetailView {
            running: row.running,
            row: Arc::clone(row),
            install_dir: entry.map(|e| e.install_dir.clone()).unwrap_or_default(),
            release_year: game_id
                .and_then(|id| self.games.get(&id))
                .and_then(|game| game.release_year),
            has_artwork,
            artwork,
            membership: game_id.map_or(Membership::NotChecked, |id| self.cached_membership(id)),
            achievement_backend: false,
            achievements: None,
            suggestions: self.detail_suggestions(row),
        }
    }

    /// Per-provider progress in registry order, followed by the DRM-free folders pseudo row. The
    /// manual provider feeds that row instead of appearing as a store; its state comes from the
    /// bridged discovery flag.
    fn scan_progress_view(&self) -> Vec<ScanProgress> {
        let mut progress: Vec<ScanProgress> = self
            .providers
            .iter()
            .filter(|provider| provider.id() != "manual")
            .filter_map(|provider| self.scan_progress.get(provider.id()).cloned())
            .collect();
        progress.push(ScanProgress {
            provider: "drm-free".to_owned(),
            name: "DRM-free folders".to_owned(),
            state: if self.discovery_scanning {
                ScanProgressState::Scanning
            } else {
                ScanProgressState::Done
            },
            found: self
                .scan_progress
                .get("manual")
                .and_then(|progress| progress.found),
        });
        progress
    }

    /// Every non-hidden visible row, regardless of the filters, search, scope or grouping.
    fn palette_index(&self) -> Vec<PaletteEntry> {
        self.all_rows()
            .into_iter()
            .filter(|row| !row.hidden)
            .map(|row| PaletteEntry {
                key: row.key,
                title: row.title,
                provider_name: row.provider_name,
                installed: row.installed,
                playtime_label: playtime_label(row.playtime.seconds),
            })
            .collect()
    }
}

fn playtime_label(seconds: Option<i64>) -> Option<String> {
    let seconds = seconds.filter(|seconds| *seconds >= 0)?;
    let hours = seconds / 3600;
    if hours > 0 {
        return Some(format!("{hours} h"));
    }
    let minutes = (seconds % 3600) / 60;
    Some(format!("{minutes} min"))
}

fn rows_equal(shown: &[Arc<Row>], next: &[Row]) -> bool {
    shown.len() == next.len() && shown.iter().zip(next).all(|(a, b)| **a == *b)
}

impl MatchStatus {
    /// Candidate game ids the match dialog lists first (empty unless ambiguous).
    pub fn candidates(&self) -> &[u64] {
        match self {
            Self::Ambiguous { candidates } => candidates,
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests;
