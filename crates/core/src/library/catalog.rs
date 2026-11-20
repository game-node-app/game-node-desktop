//! The in-app GameNode catalog search (D8).
//!
//! A search is sent only for a trimmed query of at least two characters and only with a verified
//! session. The latest query owns the view: an older operation's answer, or an answer that echoes
//! another query, is dropped. Results carry no local row of their own; the view adds whether the
//! account library has the game and which local row is matched to it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::game_api::{ApiError, GameSummary, SearchPage};

use super::machine::{Access, Effect, LibraryMachine};
use super::store::GameRecord;

/// What the catalog view is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogStatus {
    /// The query is shorter than two characters after trimming.
    #[default]
    Idle,
    Loading,
    Ready,
    /// The request failed; Try again repeats the current query.
    Error,
    /// No verified session or GameNode is unreachable. No request is sent.
    Offline,
}

/// One catalog search result as the view shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogResult {
    pub game_id: u64,
    pub title: String,
    pub year: Option<i32>,
    pub cover_url: Option<String>,
    pub cover_file: Option<String>,
    pub has_artwork: bool,
    /// The account library cache has an entry for this game.
    pub in_library: bool,
    /// The first local library row whose match is this game.
    pub local_key: Option<String>,
}

/// The catalog view. It replaces the library content while open but never leaks account-only
/// games into the library rows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogView {
    pub query: String,
    pub status: CatalogStatus,
    pub results: Vec<CatalogResult>,
    /// The search reported that its answer may be incomplete.
    pub incomplete: bool,
}

/// The machine's raw catalog state. Results are kept as search summaries so `inLibrary` and
/// `localKey` can be recomputed after an add, undo or account-library refresh.
#[derive(Debug, Default)]
pub(super) struct CatalogSession {
    pub(super) focused_game: Option<u64>,
    pub(super) query: String,
    pub(super) status: CatalogStatus,
    pub(super) op: Option<u64>,
    pub(super) games: Vec<GameSummary>,
    pub(super) incomplete: bool,
}

impl CatalogSession {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

impl LibraryMachine {
    /// Stores the new query and starts its search when it is long enough and the session is
    /// verified. A short or offline query invalidates the previous operation, so its late answer
    /// cannot replace the idle/offline view.
    pub(super) fn catalog_search(&mut self, query: String) -> Vec<Effect> {
        let trimmed = query.trim().to_owned();
        if trimmed.chars().count() < 2 {
            self.catalog.clear();
            self.catalog.query = query;
            return vec![];
        }
        if self.access != Access::Verified {
            self.catalog.clear();
            self.catalog.query = query;
            self.catalog.status = CatalogStatus::Offline;
            return vec![];
        }
        let op = self.next_op();
        self.catalog.query = query;
        self.catalog.status = CatalogStatus::Loading;
        self.catalog.op = Some(op);
        self.catalog.focused_game = None;
        self.catalog.games.clear();
        self.catalog.incomplete = false;
        vec![Effect::CatalogSearch { op, query: trimmed }]
    }

    /// Applies the answer of the latest operation, provided it echoes the current query. The
    /// search metadata is stored so the add dialog can name the game and later suggestions can
    /// reuse it.
    pub(super) fn on_catalog_search_done(
        &mut self,
        op: u64,
        query: String,
        result: Result<SearchPage, ApiError>,
    ) -> Vec<Effect> {
        if self.catalog.op != Some(op) || query.trim() != self.catalog.query.trim() {
            return vec![];
        }
        self.catalog.op = None;
        match result {
            Ok(page) => {
                let games = page.games;
                let changed = self.merge_games(games.iter());
                self.catalog.games = games;
                self.catalog.incomplete = page.complete == Some(false);
                self.catalog.status = CatalogStatus::Ready;
                let mut effects = Vec::new();
                if !changed.is_empty() {
                    effects.push(Effect::PersistMatches {
                        games: changed,
                        matches: Vec::new(),
                    });
                }
                effects.extend(
                    self.plan_option_covers(
                        self.catalog
                            .games
                            .iter()
                            .map(|game| game.id)
                            .collect::<Vec<_>>(),
                    ),
                );
                effects
            }
            Err(_) => {
                self.catalog.games.clear();
                self.catalog.incomplete = false;
                self.catalog.status = CatalogStatus::Error;
                vec![]
            }
        }
    }

    pub(super) fn catalog_clear(&mut self) {
        self.catalog.clear();
    }

    /// Records the metadata a catalog add needs before opening the existing add dialog for that
    /// GameNode game. No local library row is created for it.
    pub(super) fn open_add_for_catalog_game(
        &mut self,
        game_id: u64,
        title: String,
        cover_url: Option<String>,
    ) -> Vec<Effect> {
        let existing = self.games.get(&game_id).cloned();
        let title = title.trim();
        let cover_url = cover_url.filter(|url| !url.trim().is_empty());
        let summary = GameSummary {
            id: game_id,
            name: if title.is_empty() {
                existing
                    .as_ref()
                    .map(|game| game.name.clone())
                    .unwrap_or_default()
            } else {
                title.to_owned()
            },
            release_year: existing.as_ref().and_then(|game| game.release_year),
            platforms: existing
                .as_ref()
                .map(|game| game.platforms.clone())
                .unwrap_or_default(),
            cover_url: cover_url.or_else(|| existing.and_then(|game| game.cover_url)),
        };
        let mut effects = Vec::new();
        let changed: Vec<GameRecord> = self.merge_games(std::iter::once(&summary));
        if !changed.is_empty() {
            effects.push(Effect::PersistMatches {
                games: changed,
                matches: Vec::new(),
            });
        }
        effects.extend(self.open_add_for_game(game_id));
        effects
    }

    /// Builds the view from the raw results. Membership is read from the account-library cache and
    /// the local key from the first visible row matched to the game, so both follow add, undo and
    /// cache refreshes without re-running the search.
    pub(super) fn catalog_view(&self) -> CatalogView {
        let mut local_keys = BTreeMap::<u64, String>::new();
        for entry in self.visible_entries() {
            if let Some(game_id) = self.match_status(entry).game_id() {
                local_keys
                    .entry(game_id)
                    .or_insert_with(|| entry.key.clone());
            }
        }
        let status =
            if self.catalog.status == CatalogStatus::Idle || self.access == Access::Verified {
                self.catalog.status
            } else {
                CatalogStatus::Offline
            };
        CatalogView {
            query: self.catalog.query.clone(),
            status,
            results: self
                .catalog
                .games
                .iter()
                .map(|game| CatalogResult {
                    game_id: game.id,
                    title: game.name.clone(),
                    year: game.release_year,
                    cover_url: game.cover_url.clone(),
                    cover_file: self
                        .games
                        .get(&game.id)
                        .and_then(|record| record.cover_file.clone()),
                    has_artwork: self.artwork_for(game.id).is_some(),
                    in_library: self
                        .account_library
                        .as_ref()
                        .is_some_and(|library| library.entries.contains_key(&game.id)),
                    local_key: local_keys.get(&game.id).cloned(),
                })
                .collect(),
            incomplete: self.catalog.incomplete,
        }
    }
}

#[cfg(test)]
mod tests;
