//! The match dialog: choosing which GameNode game a library entry is.
//!
//! The dialog opens on the single selected entry with a search prefilled from the detected title.
//! When the server mapped the store id to several games, those come first. The user can pick a
//! game, mark the entry as having no GameNode game, or forget their own choice so the automatic
//! match applies again. Nothing changes until one of those is done: a failed search only shows
//! an error, and closing the dialog leaves the entry as it was.

use crate::game_api::{ApiError, GameSummary};

use super::actions::Reason;
use super::machine::{Effect, LibraryMachine};
use super::matching::MatchStatus;
use super::store::ChosenMatch;

/// Why the dialog could not show search results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchFailure {
    /// No verified session or no network: nothing was sent.
    Blocked(Reason),
    /// GameNode refused the session.
    Rejected,
    /// The request failed or timed out.
    Failed,
}

#[derive(Debug)]
enum Search {
    Idle,
    Running { op: u64 },
    Done(Vec<GameSummary>),
    Failed(SearchFailure),
}

/// An open dialog. Lives in the machine; the view is [`MatchDialogView`].
#[derive(Debug)]
pub(super) struct MatchDialog {
    key: String,
    query: String,
    search: Search,
}

/// A game the user can pick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchOption {
    pub game_id: u64,
    pub name: String,
    pub release_year: Option<i32>,
    pub cover_url: Option<String>,
    /// The downloaded cover, when this game's cover is already on disk.
    pub cover_file: Option<String>,
    /// This is the game the entry is matched to now.
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchDialogView {
    pub entry_key: String,
    /// The title the provider detected.
    pub detected_name: String,
    pub provider_name: String,
    pub query: String,
    pub searching: bool,
    /// The games the server offered for this store id, when it offered several.
    pub candidates: Vec<MatchOption>,
    /// Search results, without the candidates already listed above.
    pub results: Vec<MatchOption>,
    /// Set after a search that returned nothing, so the UI can say so instead of showing a blank.
    pub no_results: bool,
    pub error: Option<SearchFailure>,
    /// Why searching is unavailable right now, if it is.
    pub search_blocked: Option<Reason>,
    pub current: MatchStatus,
    /// The user has chosen for this entry, so there is a choice to forget.
    pub can_clear: bool,
}

impl LibraryMachine {
    /// Opens the dialog for the single selected entry, if Match is available for it.
    pub(super) fn open_match(&mut self) -> Vec<Effect> {
        let actions = self.actions();
        let selected = self.selected_keys();
        let [key] = selected.as_slice() else {
            return vec![];
        };
        if !actions.match_game.is_available() {
            return vec![];
        }
        let Some(entry) = self.entries.get(key) else {
            return vec![];
        };
        let query = entry.name.clone().unwrap_or_else(|| entry.store_id.clone());
        self.dialog = Some(MatchDialog {
            key: key.clone(),
            query,
            search: Search::Idle,
        });
        self.search_match()
    }

    pub(super) fn close_match(&mut self) {
        self.dialog = None;
    }

    pub(super) fn set_match_query(&mut self, text: String) {
        if let Some(dialog) = &mut self.dialog {
            dialog.query = text;
        }
    }

    /// Searches GameNode for the dialog's query. A blank query searches nothing.
    pub(super) fn search_match(&mut self) -> Vec<Effect> {
        let blocked = self.network_block();
        let Some(query) = self.dialog.as_ref().map(|d| d.query.trim().to_owned()) else {
            return vec![];
        };
        if blocked.is_none() && query.is_empty() {
            if let Some(dialog) = &mut self.dialog {
                dialog.search = Search::Idle;
            }
            return vec![];
        }
        let op = self.next_op();
        let Some(dialog) = &mut self.dialog else {
            return vec![];
        };
        if let Some(reason) = blocked {
            dialog.search = Search::Failed(SearchFailure::Blocked(reason));
            return vec![];
        }
        dialog.search = Search::Running { op };
        vec![Effect::SearchGames { op, query }]
    }

    pub(super) fn on_match_searched(
        &mut self,
        op: u64,
        result: Result<Vec<GameSummary>, ApiError>,
    ) -> Vec<Effect> {
        let Some(dialog) = &mut self.dialog else {
            return vec![];
        };
        if !matches!(dialog.search, Search::Running { op: running } if running == op) {
            return vec![];
        }
        dialog.search = match result {
            Ok(games) => Search::Done(games),
            Err(ApiError::Rejected) => Search::Failed(SearchFailure::Rejected),
            Err(ApiError::Failed) => Search::Failed(SearchFailure::Failed),
        };
        vec![]
    }

    /// Matches the dialog's entry to a game from the candidates or the search results.
    pub(super) fn pick_match(&mut self, game_id: u64, now: i64) -> Vec<Effect> {
        let Some(dialog) = self.dialog.take() else {
            return vec![];
        };
        let from_search = match &dialog.search {
            Search::Done(games) => games.iter().find(|game| game.id == game_id).cloned(),
            _ => None,
        };
        let is_candidate = self
            .entries
            .get(&dialog.key)
            .map(|entry| self.match_status(entry))
            .is_some_and(|status| status.candidates().contains(&game_id));
        let mut effects = Vec::new();
        match from_search {
            Some(summary) => {
                let games = self.merge_games(std::iter::once(&summary));
                effects.push(Effect::PersistMatches {
                    games,
                    matches: vec![],
                });
            }
            // Candidates were stored when the lookup that found them completed.
            None if is_candidate && self.games.contains_key(&game_id) => {}
            None => {
                // Not something the dialog offered: ignore it and keep the dialog open.
                self.dialog = Some(dialog);
                return vec![];
            }
        }
        effects.extend(self.choose(&dialog.key, Some(game_id), now));
        effects.extend(self.plan_covers(false));
        effects
    }

    /// Marks the dialog's entry as having no GameNode game.
    pub(super) fn mark_no_game(&mut self, now: i64) -> Vec<Effect> {
        let Some(dialog) = self.dialog.take() else {
            return vec![];
        };
        self.choose(&dialog.key, None, now)
    }

    /// Forgets the user's choice for the dialog's entry. The automatic match applies again.
    pub(super) fn clear_match(&mut self) -> Vec<Effect> {
        let Some(dialog) = self.dialog.take() else {
            return vec![];
        };
        let Some(account) = self.account.as_ref().map(|a| a.id.clone()) else {
            return vec![];
        };
        if self.chosen.remove(&dialog.key).is_none() {
            return vec![];
        }
        vec![Effect::ForgetChoice {
            account_id: account,
            entry_key: dialog.key,
        }]
    }

    fn choose(&mut self, key: &str, game_id: Option<u64>, now: i64) -> Vec<Effect> {
        let Some(account) = self.account.as_ref().map(|a| a.id.clone()) else {
            return vec![];
        };
        if self.chosen.get(key).map(|c| c.game_id) == Some(game_id) {
            return vec![];
        }
        let choice = ChosenMatch {
            entry_key: key.to_owned(),
            game_id,
            chosen_at: now,
        };
        self.chosen.insert(key.to_owned(), choice.clone());
        vec![Effect::PersistChoice {
            account_id: account,
            choice,
        }]
    }

    pub(super) fn match_dialog_view(&self) -> Option<MatchDialogView> {
        let dialog = self.dialog.as_ref()?;
        let entry = self.entries.get(&dialog.key)?;
        let current = self.match_status(entry);
        let current_game = current.game_id();
        let option = |game_id: u64, summary: Option<&GameSummary>| {
            let known = self.games.get(&game_id);
            MatchOption {
                game_id,
                name: summary
                    .map(|game| game.name.clone())
                    .or_else(|| known.map(|game| game.name.clone()))
                    .unwrap_or_default(),
                release_year: summary
                    .and_then(|game| game.release_year)
                    .or_else(|| known.and_then(|game| game.release_year)),
                cover_url: summary
                    .and_then(|game| game.cover_url.clone())
                    .or_else(|| known.and_then(|game| game.cover_url.clone())),
                cover_file: known.and_then(|game| game.cover_file.clone()),
                current: current_game == Some(game_id),
            }
        };

        let candidates: Vec<MatchOption> = current
            .candidates()
            .iter()
            .filter(|id| self.games.contains_key(id))
            .map(|id| option(*id, None))
            .collect();
        let (results, searching, error, searched) = match &dialog.search {
            Search::Idle => (vec![], false, None, false),
            Search::Running { .. } => (vec![], true, None, false),
            Search::Done(games) => (
                games
                    .iter()
                    .filter(|game| !candidates.iter().any(|c| c.game_id == game.id))
                    .map(|game| option(game.id, Some(game)))
                    .collect(),
                false,
                None,
                true,
            ),
            Search::Failed(failure) => (vec![], false, Some(*failure), false),
        };
        Some(MatchDialogView {
            entry_key: dialog.key.clone(),
            detected_name: entry.name.clone().unwrap_or_else(|| entry.store_id.clone()),
            provider_name: self
                .provider(&entry.provider)
                .map(|p| p.name().to_owned())
                .unwrap_or_default(),
            query: dialog.query.clone(),
            searching,
            no_results: searched && results.is_empty() && candidates.is_empty(),
            candidates,
            results,
            error,
            search_blocked: self.network_block(),
            can_clear: self.chosen.contains_key(&dialog.key),
            current,
        })
    }
}

#[cfg(test)]
mod tests;
