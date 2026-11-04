//! Which actions the selection offers, and why a disabled one is disabled.
//!
//! Play and Install are local: they only need the provider's rule and a complete record, so they
//! work before the session is verified and while GameNode is unreachable. Matching, game search
//! and Add to GameNode need a verified session and the network.

use super::machine::{Access, LibraryMachine};
use super::matching::MatchStatus;
use super::membership::{EntryStatus, Membership};

/// Why an action is disabled. The UI turns each into a localized sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    NothingSelected,
    /// The action works on one game at a time.
    MultipleSelected,
    /// The provider declares no launch rule.
    NoLaunchRule,
    NoInstallRule,
    /// The entry lacks a field the provider's rule needs.
    MissingField,
    /// No verified session yet (still verifying, or GameNode was unreachable at startup).
    NeedsVerifiedSession,
    /// The session is verified but GameNode cannot be reached now.
    Offline,
    /// The entry has no GameNode game: unmatched, ambiguous, or marked as not on GameNode.
    NotMatched,
    /// None of the selected games has a GameNode match.
    NoMatchedGames,
    CheckingMembership,
    /// GameNode could not say whether the game is already in the library.
    MembershipUnavailable,
    AlreadyInLibrary(Option<EntryStatus>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Available,
    Disabled(Reason),
}

impl Availability {
    pub fn is_available(self) -> bool {
        self == Self::Available
    }

    pub fn reason(self) -> Option<Reason> {
        match self {
            Self::Available => None,
            Self::Disabled(reason) => Some(reason),
        }
    }
}

/// The four actions for the current selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    /// How many selected entries are shown.
    pub selected: usize,
    pub play: Availability,
    pub install: Availability,
    /// Match, or Change match when `change_match` is set.
    pub match_game: Availability,
    pub add_to_gamenode: Availability,
    /// The single selected entry has a match (or a "no game" choice) to change.
    pub change_match: bool,
}

impl Actions {
    fn none(reason: Reason) -> Self {
        Self {
            selected: 0,
            play: Availability::Disabled(reason),
            install: Availability::Disabled(reason),
            match_game: Availability::Disabled(reason),
            add_to_gamenode: Availability::Disabled(reason),
            change_match: false,
        }
    }
}

/// A local action: the provider must declare the rule and the record must fill it.
fn rule(declared: bool, fillable: bool, missing_rule: Reason) -> Availability {
    match (declared, fillable) {
        (true, true) => Availability::Available,
        (true, false) => Availability::Disabled(Reason::MissingField),
        (false, _) => Availability::Disabled(missing_rule),
    }
}

impl LibraryMachine {
    /// Why network actions are unavailable right now, if they are.
    pub fn network_block(&self) -> Option<Reason> {
        match self.access {
            Access::Unverified => Some(Reason::NeedsVerifiedSession),
            Access::Offline => Some(Reason::Offline),
            Access::Verified => None,
        }
    }

    /// What is known about the game's standing in the GameNode library.
    pub fn membership(&self, game_id: u64) -> Membership {
        self.membership.get(&game_id).copied().unwrap_or_default()
    }

    /// The actions for the entries selected and shown.
    pub fn actions(&self) -> Actions {
        let selected = self.selected_keys();
        match selected.as_slice() {
            [] => Actions::none(Reason::NothingSelected),
            [key] => self.single_actions(key),
            many => self.multiple_actions(many),
        }
    }

    fn single_actions(&self, key: &str) -> Actions {
        let Some(entry) = self.entries.get(key) else {
            return Actions::none(Reason::NothingSelected);
        };
        let provider = self.provider(&entry.provider);
        let record = entry.record();
        let play = rule(
            provider.is_some_and(|p| p.has_launch()),
            provider.is_some_and(|p| p.can_launch(&record)),
            Reason::NoLaunchRule,
        );
        let install = rule(
            provider.is_some_and(|p| p.has_install()),
            provider.is_some_and(|p| p.can_install(&record)),
            Reason::NoInstallRule,
        );

        let status = self.match_status(entry);
        let match_game = self
            .network_block()
            .map_or(Availability::Available, Availability::Disabled);
        let add_to_gamenode = match (self.network_block(), status.game_id()) {
            (Some(reason), _) => Availability::Disabled(reason),
            (None, None) => Availability::Disabled(Reason::NotMatched),
            (None, Some(game_id)) => match self.membership(game_id) {
                Membership::NotInLibrary => Availability::Available,
                Membership::NotChecked | Membership::Checking => {
                    Availability::Disabled(Reason::CheckingMembership)
                }
                Membership::Unavailable => Availability::Disabled(Reason::MembershipUnavailable),
                Membership::InLibrary(status) => {
                    Availability::Disabled(Reason::AlreadyInLibrary(status))
                }
            },
        };
        Actions {
            selected: 1,
            play,
            install,
            match_game,
            add_to_gamenode,
            change_match: matches!(status, MatchStatus::Matched { .. } | MatchStatus::NoGame),
        }
    }

    /// With several games selected only Add to GameNode applies: it adds the matched ones that are
    /// not in the library yet, which is checked for each one when the user confirms.
    fn multiple_actions(&self, keys: &[String]) -> Actions {
        let any_matched = keys.iter().any(|key| {
            self.entries
                .get(key)
                .is_some_and(|entry| self.match_status(entry).game_id().is_some())
        });
        let add_to_gamenode = match self.network_block() {
            Some(reason) => Availability::Disabled(reason),
            None if any_matched => Availability::Available,
            None => Availability::Disabled(Reason::NoMatchedGames),
        };
        let single_only = Availability::Disabled(Reason::MultipleSelected);
        Actions {
            selected: keys.len(),
            play: single_only,
            install: single_only,
            match_game: single_only,
            add_to_gamenode,
            change_match: false,
        }
    }
}

#[cfg(test)]
mod tests;
