//! Matching on behalf of a provider's plugin: searching GameNode's catalog and assigning one of
//! the plugin's own entries to a game.
//!
//! This is the match dialog's policy without the dialog, so a plugin never disturbs a dialog the
//! user has open. A game can be assigned only if the host already knows it (it is in the library's
//! game records) or a plugin search just offered it. A choice the user made is never replaced by a
//! plugin; a plugin may replace a choice it made earlier. Nothing here talks to the network: the
//! engine runs the search and reports the answer.

use std::collections::VecDeque;

use crate::game_api::GameSummary;

use super::machine::{Effect, LibraryMachine};
use super::store::{ChoiceSource, ChosenMatch};

/// Games offered by plugin searches that can still be assigned. Older offers drop out first.
pub const MAX_OFFERED_GAMES: usize = 256;

/// What assigning an entry to a game would do right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginMatch {
    /// The assignment would be stored.
    Assign,
    /// The entry already has an explicit choice of this game; nothing changes.
    Already,
    /// The user chose another game, or none, for this entry.
    UserChoice,
    /// The host does not know this game: it was never found or offered here.
    UnknownGame,
    /// The library has no such entry.
    UnknownEntry,
    /// No account library is open, so there is nowhere to keep a choice.
    NoAccount,
}

impl LibraryMachine {
    /// What [`Input::AssignPluginMatch`](super::Input::AssignPluginMatch) would do for `key`.
    pub fn plan_plugin_match(&self, key: &str, game_id: u64) -> PluginMatch {
        if self.account.is_none() {
            return PluginMatch::NoAccount;
        }
        if !self.entries.contains_key(key) {
            return PluginMatch::UnknownEntry;
        }
        if !self.games.contains_key(&game_id) && !self.offered.iter().any(|g| g.id == game_id) {
            return PluginMatch::UnknownGame;
        }
        match self.chosen.get(key) {
            Some(choice) if choice.game_id == Some(game_id) => PluginMatch::Already,
            Some(choice) if choice.source == ChoiceSource::User => PluginMatch::UserChoice,
            _ => PluginMatch::Assign,
        }
    }

    /// Remembers the games a plugin search returned so one of them can be assigned.
    pub(super) fn catalog_searched(&mut self, games: Vec<GameSummary>) {
        for game in games {
            self.offered.retain(|offered| offered.id != game.id);
            if self.offered.len() == MAX_OFFERED_GAMES {
                self.offered.pop_front();
            }
            self.offered.push_back(game);
        }
    }

    /// Stores a plugin's choice of `game_id` for `key` when [`plan_plugin_match`] allows it.
    ///
    /// [`plan_plugin_match`]: Self::plan_plugin_match
    pub(super) fn assign_plugin_match(&mut self, key: &str, game_id: u64, now: i64) -> Vec<Effect> {
        if self.plan_plugin_match(key, game_id) != PluginMatch::Assign {
            return vec![];
        }
        let Some(account) = self.account.as_ref().map(|a| a.id.clone()) else {
            return vec![];
        };
        let mut effects = Vec::new();
        if let Some(offer) = self.offered.iter().find(|g| g.id == game_id).cloned() {
            let games = self.merge_games(std::iter::once(&offer));
            effects.push(Effect::PersistMatches {
                games,
                matches: vec![],
            });
        }
        let choice = ChosenMatch {
            entry_key: key.to_owned(),
            game_id: Some(game_id),
            chosen_at: now,
            source: ChoiceSource::Plugin,
        };
        self.chosen.insert(key.to_owned(), choice.clone());
        effects.push(Effect::PersistChoice {
            account_id: account,
            choice,
        });
        effects.extend(self.plan_covers(None));
        effects
    }
}

pub(super) type Offered = VecDeque<GameSummary>;

#[cfg(test)]
mod tests;
