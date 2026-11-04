//! A game's standing in the user's GameNode library, as last read from GameNode.

/// The status of a GameNode library entry, as GameNode names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryStatus {
    Playing,
    Finished,
    Planned,
    Dropped,
    Ongoing,
}

impl EntryStatus {
    pub const ALL: [EntryStatus; 5] = [
        Self::Playing,
        Self::Finished,
        Self::Planned,
        Self::Dropped,
        Self::Ongoing,
    ];

    /// The value GameNode's API uses.
    pub fn api_name(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Finished => "finished",
            Self::Planned => "planned",
            Self::Dropped => "dropped",
            Self::Ongoing => "ongoing",
        }
    }

    /// Reads a status from the API; a value this build does not know gives `None`.
    pub fn from_api_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.api_name() == name)
    }
}

/// Whether a matched game is in the user's GameNode library. Read from GameNode when the entry is
/// selected; never guessed from local data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Membership {
    /// Not read yet (or not readable right now because no verified session).
    #[default]
    NotChecked,
    Checking,
    /// In the library. The status is `None` if GameNode sent one this build does not know.
    InLibrary(Option<EntryStatus>),
    NotInLibrary,
    /// The read failed; Add stays disabled and nothing is sent.
    Unavailable,
}

use crate::game_api::{ApiError, LibraryEntry};

use super::machine::{Effect, LibraryMachine};

impl LibraryMachine {
    /// The GameNode game of the single selected entry, if it has one.
    pub(super) fn selected_game(&self) -> Option<u64> {
        let keys = self.selected_keys();
        let [key] = keys.as_slice() else {
            return None;
        };
        let entry = self.entries.get(key)?;
        self.match_status(entry).game_id()
    }

    /// Reads the selected game's standing in the library once per selection (and again when the
    /// session becomes usable). Nothing is read without a verified session, and nothing here ever
    /// writes.
    pub(super) fn plan_membership(&mut self) -> Vec<Effect> {
        if !self.is_active() || self.network_block().is_some() {
            // Read again when the session is usable.
            self.membership_focus = None;
            return vec![];
        }
        let Some(game_id) = self.selected_game() else {
            self.membership_focus = None;
            return vec![];
        };
        if self.membership_focus == Some(game_id) {
            return vec![];
        }
        self.membership_focus = Some(game_id);
        let op = self.next_op();
        self.membership_reads.insert(game_id, op);
        self.membership.insert(game_id, Membership::Checking);
        vec![Effect::ReadEntry { op, game_id }]
    }

    pub(super) fn on_membership_read(
        &mut self,
        game_id: u64,
        result: Result<Option<LibraryEntry>, ApiError>,
    ) {
        self.membership_reads.remove(&game_id);
        let membership = match result {
            Ok(Some(entry)) => Membership::InLibrary(entry.status),
            Ok(None) => Membership::NotInLibrary,
            Err(_) => Membership::Unavailable,
        };
        self.membership.insert(game_id, membership);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip_through_their_api_names() {
        for status in EntryStatus::ALL {
            assert_eq!(EntryStatus::from_api_name(status.api_name()), Some(status));
        }
        assert_eq!(EntryStatus::from_api_name("PLAYING"), None);
        assert_eq!(EntryStatus::from_api_name("abandoned"), None);
    }
}
