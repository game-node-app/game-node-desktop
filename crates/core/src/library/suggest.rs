//! Scored match suggestions for unmatched and ambiguous entries (D7).
//!
//! Scoring is pure: titles are normalized, compared with Sørensen–Dice character-bigram
//! similarity, and given a year bonus when the entry's detected name or path names the game's
//! release year. The detail pane shows the best two results at 40% confidence or more.
//!
//! A suggestion search is only started for the single selected unmatched entry, with a verified
//! session, and its answer is applied only while that entry is still selected and the request is
//! the latest one. Ambiguous entries have candidates already, so they are scored without a search.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::game_api::{ApiError, GameSummary, SearchPage};

use super::machine::{Effect, LibraryMachine};
use super::matching::MatchStatus;
use super::query::Row;
use super::store::{GameRecord, StoredEntry};

/// The lowest confidence a suggestion may have and still be shown.
pub const SUGGESTION_THRESHOLD: f32 = 0.40;
/// How many suggestions the detail pane shows.
pub const SUGGESTION_LIMIT: usize = 2;
/// How much of the confidence the title similarity carries; the rest is the year bonus.
const TITLE_WEIGHT: f32 = 0.9;
const YEAR_BONUS: f32 = 0.1;
const EPSILON: f32 = 1e-6;

/// How confident a suggestion is, as the detail pane colors it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SuggestionTone {
    Success,
    Warning,
    Neutral,
}

/// One scored game the detail pane offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionView {
    pub game_id: u64,
    pub title: String,
    pub year: Option<i32>,
    pub cover_url: Option<String>,
    /// Whole percent, 0–100.
    pub confidence: u8,
    pub tone: SuggestionTone,
}

/// Lowercases, turns punctuation and separators (`_`, `-`, `.`) into spaces, and collapses
/// whitespace. `"Hollow_Knight"` and `"Hollow Knight"` normalize to the same title.
pub fn normalize_title(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars().flat_map(|character| character.to_lowercase()) {
        if character.is_alphanumeric() {
            if pending_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            pending_space = false;
            normalized.push(character);
        } else {
            pending_space = true;
        }
    }
    normalized
}

/// Sørensen–Dice similarity of two titles' character bigrams, in `0..=1`. The inputs are
/// normalized first, so punctuation and case never change the score.
pub fn title_score(left: &str, right: &str) -> f32 {
    let left = normalize_title(left);
    let right = normalize_title(right);
    if left == right {
        return if left.is_empty() { 0.0 } else { 1.0 };
    }
    if left.chars().count() < 2 || right.chars().count() < 2 {
        return 0.0;
    }
    let left_bigrams = bigrams(&left);
    let right_bigrams = bigrams(&right);
    let left_total: usize = left_bigrams.values().sum();
    let right_total: usize = right_bigrams.values().sum();
    if left_total == 0 || right_total == 0 {
        return 0.0;
    }
    let intersection: usize = left_bigrams
        .iter()
        .map(|(bigram, count)| {
            right_bigrams
                .get(bigram)
                .map_or(0, |other| (*count).min(*other))
        })
        .sum();
    (2.0 * intersection as f32) / (left_total + right_total) as f32
}

fn bigrams(normalized: &str) -> BTreeMap<(char, char), usize> {
    let characters: Vec<char> = normalized.chars().collect();
    let mut counts = BTreeMap::new();
    for window in characters.windows(2) {
        *counts.entry((window[0], window[1])).or_insert(0) += 1;
    }
    counts
}

/// The confidence of a suggestion: title similarity weighted to 0.9, plus the year bonus,
/// capped at 1.
pub fn confidence(title_similarity: f32, year_matches: bool) -> f32 {
    (title_similarity * TITLE_WEIGHT + if year_matches { YEAR_BONUS } else { 0.0 }).clamp(0.0, 1.0)
}

/// The first four-digit year (1900–2100) in a detected name or path.
pub fn year_in(text: &str) -> Option<i32> {
    years_in(text).into_iter().next()
}

/// Every four-digit year (1900–2100) in a detected name or path, without duplicates.
pub fn years_in(text: &str) -> Vec<i32> {
    let mut years = Vec::new();
    for token in text.split(|character: char| !character.is_ascii_digit()) {
        if token.len() != 4 {
            continue;
        }
        if let Ok(year) = token.parse::<i32>()
            && (1900..=2100).contains(&year)
            && !years.contains(&year)
        {
            years.push(year);
        }
    }
    years
}

/// The tone the detail pane uses for a confidence.
pub fn tone(confidence: f32) -> SuggestionTone {
    if confidence >= 0.80 {
        SuggestionTone::Success
    } else if confidence >= 0.50 {
        SuggestionTone::Warning
    } else {
        SuggestionTone::Neutral
    }
}

fn percent(confidence: f32) -> u8 {
    (confidence * 100.0).round().clamp(0.0, 100.0) as u8
}

/// Scores the search results for a detected title and returns the best two at or above the
/// threshold. Ties are deterministic: higher confidence, then title, then game id.
pub fn rank_suggestions(
    query: &str,
    detected_years: &[i32],
    games: &[GameSummary],
) -> Vec<SuggestionView> {
    let mut ranked: Vec<SuggestionView> = games
        .iter()
        .filter_map(|game| {
            let year_matches = game
                .release_year
                .is_some_and(|year| detected_years.contains(&year));
            let score = confidence(title_score(query, &game.name), year_matches);
            if score + EPSILON < SUGGESTION_THRESHOLD {
                return None;
            }
            Some(SuggestionView {
                game_id: game.id,
                title: game.name.clone(),
                year: game.release_year,
                cover_url: game.cover_url.clone(),
                confidence: percent(score),
                tone: tone(score),
            })
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .confidence
            .cmp(&left.confidence)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.game_id.cmp(&right.game_id))
    });
    ranked.truncate(SUGGESTION_LIMIT);
    ranked
}

fn game_summary(record: &GameRecord) -> GameSummary {
    GameSummary {
        id: record.game_id,
        name: record.name.clone(),
        release_year: record.release_year,
        cover_url: record.cover_url.clone(),
    }
}

fn search_query(entry: &StoredEntry) -> String {
    entry
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&entry.store_id)
        .to_owned()
}

fn detected_years(entry: &StoredEntry) -> Vec<i32> {
    let mut text = entry.install_dir.clone();
    if let Some(name) = &entry.name {
        text.push(' ');
        text.push_str(name);
    }
    years_in(&text)
}

impl LibraryMachine {
    /// Plans the suggestion work for the current selection. An unmatched entry with no cached
    /// result and no request in flight asks for a search; ambiguous candidates are scored from
    /// the metadata that is already stored, with no request.
    pub(super) fn plan_suggestions(&mut self) -> Vec<Effect> {
        if self.access != super::machine::Access::Verified {
            return vec![];
        }
        let selected = self.selected_keys();
        let [key] = selected.as_slice() else {
            return vec![];
        };
        let Some(entry) = self.entries.get(key) else {
            return vec![];
        };
        let status = self.match_status(entry);
        match status {
            MatchStatus::Unmatched => {
                if self.suggestions.contains_key(key) || self.suggestion_requests.contains_key(key)
                {
                    return vec![];
                }
                let query = search_query(entry);
                let op = self.next_op();
                self.suggestion_requests.insert(key.clone(), op);
                vec![Effect::SuggestMatches {
                    op,
                    key: key.clone(),
                    query,
                }]
            }
            MatchStatus::Ambiguous { candidates } => {
                if self.suggestions.contains_key(key) {
                    return vec![];
                }
                let query = search_query(entry);
                let years = detected_years(entry);
                let games: Vec<GameSummary> = candidates
                    .iter()
                    .filter_map(|game_id| self.games.get(game_id))
                    .map(game_summary)
                    .collect();
                let suggestions = rank_suggestions(&query, &years, &games);
                self.suggestions.insert(key.clone(), suggestions);
                vec![]
            }
            MatchStatus::Matched { .. } | MatchStatus::NoGame => vec![],
        }
    }

    /// Applies a suggestion search to the entry it was requested for. An answer for another key,
    /// an older request, another account (whose state was cleared), or an entry that is no longer
    /// selected and unmatched/ambiguous is ignored. The games are stored so Use can persist the
    /// choice exactly like a match-dialog pick.
    pub(super) fn on_suggestions_found(
        &mut self,
        op: u64,
        key: String,
        result: Result<SearchPage, ApiError>,
    ) -> Vec<Effect> {
        if self.suggestion_requests.get(&key).copied() != Some(op) {
            return vec![];
        }
        let still_wanted = self.selected_keys().as_slice() == [key.as_str()]
            && self.entries.get(&key).is_some_and(|entry| {
                matches!(
                    self.match_status(entry),
                    MatchStatus::Unmatched | MatchStatus::Ambiguous { .. }
                )
            });
        if !still_wanted {
            self.suggestion_requests.remove(&key);
            return vec![];
        }
        self.suggestion_requests.remove(&key);
        let Ok(page) = result else {
            // Remember that this key has no usable answer, so selection changes cannot turn into a
            // request per input. A later account change clears it.
            self.suggestions.insert(key, Vec::new());
            return vec![];
        };
        let Some(entry) = self.entries.get(&key) else {
            return vec![];
        };
        let query = search_query(entry);
        let years = detected_years(entry);
        let games = page.games;
        let mut effects = Vec::new();
        let changed = self.merge_games(games.iter());
        if !changed.is_empty() {
            effects.push(Effect::PersistMatches {
                games: changed,
                matches: Vec::new(),
            });
        }
        self.suggestions
            .insert(key, rank_suggestions(&query, &years, &games));
        effects
    }

    /// Uses one cached suggestion: the same chosen-match persistence and cover plan as a pick in
    /// the match dialog. Only a game this key was actually offered can be used.
    pub(super) fn use_suggestion(&mut self, key: &str, game_id: u64, now: i64) -> Vec<Effect> {
        let offered = self
            .suggestions
            .get(key)
            .is_some_and(|suggestions| suggestions.iter().any(|item| item.game_id == game_id));
        if !offered || !self.entries.contains_key(key) || !self.games.contains_key(&game_id) {
            return vec![];
        }
        self.suggestions.remove(key);
        self.suggestion_requests.remove(key);
        let mut effects = self.choose(key, Some(game_id), now);
        effects.extend(self.plan_covers(None));
        effects
    }

    /// The suggestions the detail pane shows for one row.
    pub(super) fn detail_suggestions(&self, row: &Row) -> Vec<SuggestionView> {
        if self.access != super::machine::Access::Verified
            || !matches!(
                row.status,
                MatchStatus::Unmatched | MatchStatus::Ambiguous { .. }
            )
        {
            return Vec::new();
        }
        self.suggestions.get(&row.key).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
