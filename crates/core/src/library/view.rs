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
use super::layout::{LibraryLayout, SidebarSelection};
use super::machine::{Access, LibraryMachine};
use super::match_dialog::MatchDialogView;
use super::matching::MatchStatus;
use super::membership::Membership;
use super::query::{ProviderCount, Row};

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
        let fresh = self.visible_rows(&self.search);
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
        let total = self.visible_entries().count();
        let installed_total = self
            .visible_entries()
            .filter(|entry| entry.installed)
            .count();
        let providers = self.provider_counts();
        let installed_in_view = match self.effective_sidebar() {
            SidebarSelection::All => installed_total,
            SidebarSelection::Provider(id) => providers
                .iter()
                .find(|count| count.id == id)
                .map_or(0, |count| count.installed_count),
        };

        let empty = if total == 0 {
            let scanned_everywhere = self
                .providers
                .iter()
                .all(|provider| self.scans.contains_key(provider.id()));
            (!self.scan_running() && scanned_everywhere).then(|| EmptyState::NoGames {
                stores: self.providers.iter().map(|p| p.name().to_owned()).collect(),
            })
        } else if self.layout.filters.availability.is_installed_only() && installed_in_view == 0 {
            Some(EmptyState::NoInstalled {
                provider: match self.effective_sidebar() {
                    SidebarSelection::All => None,
                    SidebarSelection::Provider(id) => Some(id),
                },
            })
        } else if rows.is_empty() {
            Some(EmptyState::NothingMatches)
        } else {
            None
        };

        LibraryView {
            generation: previous.map_or(0, |view| view.generation),
            layout: LibraryLayout {
                sidebar: self.effective_sidebar(),
                ..self.layout.clone()
            },
            search: self.search.clone(),
            access: self.access,
            total,
            installed_total,
            providers,
            rows,
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
            membership: game_id.map_or(Membership::NotChecked, |id| self.membership(id)),
        }
    }
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
