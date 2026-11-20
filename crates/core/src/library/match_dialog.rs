//! The match dialog: choosing which GameNode game a library entry is.
//!
//! The dialog opens on the single selected entry with a search prefilled from the detected title.
//! When the server mapped the store id to several games, those come first. The user can pick a
//! game, mark the entry as having no GameNode game, or forget their own choice so the automatic
//! match applies again. Nothing changes until one of those is done: a failed search only shows
//! an error, and closing the dialog leaves the entry as it was.

use crate::game_api::{ApiError, GameSummary};

use serde::{Deserialize, Serialize};

use super::actions::Reason;
use super::machine::{Effect, LibraryMachine};
use super::matching::{MatchSource, MatchStatus};
use super::store::{ChoiceSource, ChosenMatch};
use super::suggest::{
    SuggestionTone, confidence, detected_years, percent_of, search_query, title_score, tone,
};

/// Why the dialog could not show search results.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    /// The game the preview shows. `None` while there is nothing to offer.
    selected: Option<u64>,
    /// The user selected an option, so an arriving result no longer replaces it.
    manual_selection: bool,
}

/// Why a match option's confidence is what it is. Each one is only shown under its own
/// condition, per the design.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchReason {
    /// Whole-percent title similarity. Positive from 80% up; a warning below.
    TitleSimilarity { percent: u8 },
    /// The option's release year is one detected in the entry's name or path.
    SameYear,
    /// The option's release year differs from a detected year. Only when one was detected.
    DifferentYear { year: i32 },
    /// The server offered this game for the entry's store id.
    StoreIdRecognized,
    /// The entry is already matched to this game because the user chose it.
    ChosenByYou,
}

/// A game the user can pick.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchOption {
    pub game_id: u64,
    pub name: String,
    pub release_year: Option<i32>,
    pub cover_url: Option<String>,
    /// The downloaded cover, when this game's cover is already on disk.
    pub cover_file: Option<String>,
    /// This is the game the entry is matched to now.
    pub current: bool,
    /// Whole percent, 0–100, scored like `suggest.rs`.
    pub confidence: u8,
    pub tone: SuggestionTone,
    /// Platform abbreviations the search answer reported, when known.
    pub platforms: Vec<String>,
    pub reasons: Vec<MatchReason>,
}

pub type CandidateOption = MatchOption;
pub type SearchResultItem = MatchOption;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDialogView {
    pub entry_key: String,
    /// The title the provider detected.
    pub detected_name: String,
    pub provider_name: String,
    pub query: String,
    pub searching: bool,
    /// The games the server offered for this store id, when it offered several, ordered by
    /// confidence.
    pub candidates: Vec<MatchOption>,
    /// Search results, without the candidates already listed above, in the server's order.
    pub results: Vec<MatchOption>,
    /// The option selected for preview, with its confidence and reasons.
    pub selected: Option<MatchOption>,
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
            selected: None,
            manual_selection: false,
        });
        // The current match, else the most confident candidate, is selected and previewed
        // before the search answer arrives.
        self.apply_default_selection();
        let candidates = self
            .entries
            .get(key)
            .map(|entry| self.match_status(entry).candidates().to_vec())
            .unwrap_or_default();
        let mut effects = self.search_match();
        effects.extend(self.plan_option_covers(candidates));
        let selected = self.selected_option_id();
        effects.extend(self.plan_option_covers(selected));
        effects
    }

    /// The currently selected game id, if the dialog has one.
    fn selected_option_id(&self) -> Option<u64> {
        self.dialog.as_ref().and_then(|dialog| dialog.selected)
    }

    /// Selects the current match, else the most confident option, unless the user already
    /// chose. Called on open and after every search answer.
    fn apply_default_selection(&mut self) {
        if self
            .dialog
            .as_ref()
            .is_some_and(|dialog| dialog.manual_selection)
        {
            return;
        }
        let selected = self.default_selection_id();
        if let Some(dialog) = &mut self.dialog {
            dialog.selected = selected;
        }
    }

    fn default_selection_id(&self) -> Option<u64> {
        let dialog = self.dialog.as_ref()?;
        let entry = self.entries.get(&dialog.key)?;
        let current = self.match_status(entry);
        if let Some(game_id) = current.game_id()
            && self.games.contains_key(&game_id)
        {
            return Some(game_id);
        }
        let mut best: Option<MatchOption> = None;
        let candidates = self.candidate_options(dialog, &current);
        let results = self.result_options(dialog, current.candidates());
        for option in candidates.into_iter().chain(results) {
            // Ties keep the earlier option, so candidates win over search results.
            if best
                .as_ref()
                .is_none_or(|best| option.confidence > best.confidence)
            {
                best = Some(option);
            }
        }
        best.map(|option| option.game_id)
    }

    /// The option for one game, scored against the dialog entry's detected title and years.
    fn option_for(
        &self,
        dialog: &MatchDialog,
        game_id: u64,
        summary: Option<&GameSummary>,
        candidate: bool,
    ) -> Option<MatchOption> {
        let entry = self.entries.get(&dialog.key)?;
        let known = self.games.get(&game_id);
        if summary.is_none() && known.is_none() {
            return None;
        }
        let name = summary
            .map(|game| game.name.clone())
            .filter(|name| !name.is_empty())
            .or_else(|| known.map(|game| game.name.clone()))
            .unwrap_or_default();
        let release_year = summary
            .and_then(|game| game.release_year)
            .or_else(|| known.and_then(|game| game.release_year));
        let cover_url = summary
            .and_then(|game| game.cover_url.clone())
            .or_else(|| known.and_then(|game| game.cover_url.clone()));
        let cover_file = known.and_then(|game| game.cover_file.clone());
        let platforms = match summary {
            Some(game) if !game.platforms.is_empty() => game.platforms.clone(),
            _ => known.map(|game| game.platforms.clone()).unwrap_or_default(),
        };

        let detected_title = search_query(entry);
        let years = detected_years(entry);
        let year_matches = release_year.is_some_and(|year| years.contains(&year));
        let similarity = title_score(&detected_title, &name);
        let score = confidence(similarity, year_matches);
        let status = self.match_status(entry);
        let is_current = status.game_id() == Some(game_id);

        let mut reasons = Vec::new();
        if !name.is_empty() {
            reasons.push(MatchReason::TitleSimilarity {
                percent: percent_of(similarity),
            });
        }
        if let Some(year) = release_year
            && !years.is_empty()
        {
            if years.contains(&year) {
                reasons.push(MatchReason::SameYear);
            } else {
                reasons.push(MatchReason::DifferentYear { year });
            }
        }
        if candidate {
            reasons.push(MatchReason::StoreIdRecognized);
        }
        if is_current
            && matches!(
                status,
                MatchStatus::Matched {
                    source: MatchSource::Chosen,
                    ..
                }
            )
        {
            reasons.push(MatchReason::ChosenByYou);
        }

        Some(MatchOption {
            game_id,
            name,
            release_year,
            cover_url,
            cover_file,
            current: is_current,
            confidence: percent_of(score),
            tone: tone(score),
            platforms,
            reasons,
        })
    }

    /// The server's candidates, ordered by confidence (then title and id for determinism).
    fn candidate_options(&self, dialog: &MatchDialog, current: &MatchStatus) -> Vec<MatchOption> {
        let mut options: Vec<MatchOption> = current
            .candidates()
            .iter()
            .filter_map(|game_id| self.option_for(dialog, *game_id, None, true))
            .collect();
        options.sort_by(|left, right| {
            right
                .confidence
                .cmp(&left.confidence)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.game_id.cmp(&right.game_id))
        });
        options
    }

    /// The search answer's games in the server's order, without the candidates.
    fn result_options(&self, dialog: &MatchDialog, candidate_ids: &[u64]) -> Vec<MatchOption> {
        let Search::Done(games) = &dialog.search else {
            return Vec::new();
        };
        games
            .iter()
            .filter(|game| !candidate_ids.contains(&game.id))
            .filter_map(|game| self.option_for(dialog, game.id, Some(game), false))
            .collect()
    }

    /// Whether the dialog currently offers this game, so a stale selection input changes nothing.
    fn dialog_offers(&self, dialog: &MatchDialog, game_id: u64) -> bool {
        let Some(entry) = self.entries.get(&dialog.key) else {
            return false;
        };
        let status = self.match_status(entry);
        status.game_id() == Some(game_id)
            || status.candidates().contains(&game_id)
            || matches!(&dialog.search, Search::Done(games) if games.iter().any(|game| game.id == game_id))
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
        let running = self.dialog.as_ref().is_some_and(
            |dialog| matches!(dialog.search, Search::Running { op: running } if running == op),
        );
        if !running {
            return vec![];
        }
        let mut effects = Vec::new();
        let search = match result {
            Ok(games) => {
                // Search results are shown with their own covers, which are downloaded for the
                // dialog before the user picks one. They stay in memory until a pick stores them.
                self.merge_games(games.iter());
                effects = self.plan_option_covers(games.iter().map(|game| game.id));
                Search::Done(games)
            }
            Err(ApiError::Rejected) => Search::Failed(SearchFailure::Rejected),
            Err(ApiError::Failed) => Search::Failed(SearchFailure::Failed),
        };
        if let Some(dialog) = &mut self.dialog {
            dialog.search = search;
        }
        // A new answer preselects the current match, else the best option, unless the user has
        // already selected one.
        self.apply_default_selection();
        effects
    }

    /// Selects an option for preview. Only an option the dialog offers can be selected, and
    /// nothing about the entry changes until the user confirms.
    pub(super) fn select_match(&mut self, game_id: u64) -> Vec<Effect> {
        let Some(dialog) = self.dialog.as_ref() else {
            return vec![];
        };
        if !self.dialog_offers(dialog, game_id) {
            return vec![];
        }
        if let Some(dialog) = &mut self.dialog {
            dialog.selected = Some(game_id);
            dialog.manual_selection = true;
        }
        vec![]
    }

    /// Matches the dialog's entry to the option the user selected. Without a selection, nothing
    /// happens and the dialog stays open.
    pub(super) fn confirm_match(&mut self, now: i64) -> Vec<Effect> {
        let Some(game_id) = self.selected_option_id() else {
            return vec![];
        };
        self.pick_match(game_id, now)
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
        let status = self
            .entries
            .get(&dialog.key)
            .map(|entry| self.match_status(entry));
        let is_candidate = status
            .as_ref()
            .is_some_and(|status| status.candidates().contains(&game_id));
        // The current match may not be listed (a matched entry keeps it out of the dialog lists),
        // but confirming it is still a valid pick.
        let is_current = status
            .as_ref()
            .is_some_and(|status| status.game_id() == Some(game_id));
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
            None if (is_candidate || is_current) && self.games.contains_key(&game_id) => {}
            None => {
                // Not something the dialog offered: ignore it and keep the dialog open.
                self.dialog = Some(dialog);
                return vec![];
            }
        }
        effects.extend(self.choose(&dialog.key, Some(game_id), now));
        effects.extend(self.plan_covers(None));
        effects
    }

    /// Marks the dialog's entry as having no GameNode game.
    pub(super) fn mark_no_game(&mut self, now: i64) -> Vec<Effect> {
        let Some(dialog) = self.dialog.take() else {
            return vec![];
        };
        self.mark_key_no_game(&dialog.key, now)
    }

    /// Marks one entry as having no GameNode game without the match dialog, as the detail pane's
    /// Not a game action does. The entry must still be the single shown selection, and must still
    /// be unmatched: a stale action after the selection moved or a lookup matched the entry
    /// changes nothing. The choice is persisted exactly like the dialog's, and the entry's cached
    /// suggestions are dropped.
    pub(super) fn mark_entry_no_game(&mut self, key: &str, now: i64) -> Vec<Effect> {
        let selected = self.selected_keys();
        if selected.len() != 1 || selected[0] != key {
            return vec![];
        }
        let Some(entry) = self.entries.get(key) else {
            return vec![];
        };
        if !matches!(self.match_status(entry), MatchStatus::Unmatched) {
            return vec![];
        }
        if self.dialog.as_ref().is_some_and(|dialog| dialog.key == key) {
            self.dialog = None;
        }
        self.mark_key_no_game(key, now)
    }

    /// Stores the user's "no GameNode game" choice and forgets what was suggested for the entry.
    fn mark_key_no_game(&mut self, key: &str, now: i64) -> Vec<Effect> {
        self.suggestions.remove(key);
        self.suggestion_requests.remove(key);
        self.choose(key, None, now)
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

    pub(super) fn choose(&mut self, key: &str, game_id: Option<u64>, now: i64) -> Vec<Effect> {
        let Some(account) = self.account.as_ref().map(|a| a.id.clone()) else {
            return vec![];
        };
        // A plugin's earlier choice of the same game still becomes the user's own, so a plugin
        // can no longer replace it.
        if self
            .chosen
            .get(key)
            .is_some_and(|c| c.game_id == game_id && c.source == ChoiceSource::User)
        {
            return vec![];
        }
        let choice = ChosenMatch {
            entry_key: key.to_owned(),
            game_id,
            chosen_at: now,
            source: ChoiceSource::User,
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
        let candidates = self.candidate_options(dialog, &current);
        let results = self.result_options(dialog, current.candidates());
        let selected = dialog.selected.and_then(|game_id| {
            let summary = match &dialog.search {
                Search::Done(games) => games.iter().find(|game| game.id == game_id),
                _ => None,
            };
            let candidate = current.candidates().contains(&game_id);
            self.option_for(dialog, game_id, summary, candidate)
        });
        let (searching, error, searched) = match &dialog.search {
            Search::Idle => (false, None, false),
            Search::Running { .. } => (true, None, false),
            Search::Done(_) => (false, None, true),
            Search::Failed(failure) => (false, Some(*failure), false),
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
            selected,
            error,
            search_blocked: self.network_block(),
            can_clear: self.chosen.contains_key(&dialog.key),
            current,
        })
    }
}

#[cfg(test)]
mod tests;
