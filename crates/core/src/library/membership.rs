//! A game's standing in the user's GameNode library, as last read from GameNode.

use serde::{Deserialize, Serialize};

/// The status of a GameNode library entry, as GameNode names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryStatus {
    Playing,
    Finished,
    Planned,
    Dropped,
    Ongoing,
}

pub type GameStatus = EntryStatus;

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

pub type StandingView = Membership;

use crate::game_api::{AccountEntry, ApiError, LibraryEntry};

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

    /// The membership the detail pane shows: the cached complete read first, then the
    /// per-selection read. `actions` keeps using [`LibraryMachine::membership`], so an unreadable
    /// standing still disables editing instead of guessing from the stale cache.
    pub(super) fn cached_membership(&self, game_id: u64) -> Membership {
        if let Some(entry) = self
            .account_library
            .as_ref()
            .and_then(|library| library.entries.get(&game_id))
        {
            return Membership::InLibrary(entry.status);
        }
        self.membership(game_id)
    }

    /// Remembers one game's confirmed local state while a complete read is in flight. The read's
    /// answer is merged under it, so data read before the confirmation cannot revert it.
    pub(super) fn record_local_account_entry(&mut self, game_id: u64, entry: Option<AccountEntry>) {
        if self.account_library_sync.is_some() {
            self.account_library_writes.insert(game_id, entry);
        }
    }

    /// Records what one game read said: its standing, the full entry the add dialog edits, and,
    /// once a complete read exists, that game's row in the account library. It never creates the
    /// complete-read marker, so one refresh cannot make the cache look complete.
    ///
    /// While a complete read is in flight the game is also remembered as a local update, so the
    /// late answer keeps this fresher state instead of the snapshot's older one.
    pub(super) fn record_entry_read(
        &mut self,
        game_id: u64,
        result: &Result<Option<LibraryEntry>, ApiError>,
    ) -> Vec<Effect> {
        self.account_entries.remove(&game_id);
        match result {
            Ok(Some(entry)) => {
                self.account_entries.insert(game_id, entry.clone());
                self.membership
                    .insert(game_id, Membership::InLibrary(entry.status));
            }
            Ok(None) => {
                self.membership.insert(game_id, Membership::NotInLibrary);
            }
            Err(_) => {
                self.membership.insert(game_id, Membership::Unavailable);
            }
        }
        let Ok(read) = result else {
            return vec![];
        };
        let Some(account_id) = self.account.as_ref().map(|account| account.id.clone()) else {
            return vec![];
        };
        let entry = read.as_ref().map(|entry| AccountEntry {
            id: entry.id.clone(),
            game_id,
            status: entry.status,
            collection_ids: entry.collection_ids.clone(),
        });
        if let Some(library) = self.account_library.as_mut() {
            match &entry {
                Some(entry) => {
                    library.entries.insert(game_id, entry.clone());
                }
                None => {
                    library.entries.remove(&game_id);
                }
            }
        }
        let complete = self.account_library.is_some();
        self.record_local_account_entry(game_id, entry.clone());
        if !complete {
            return vec![];
        }
        vec![Effect::PersistAccountEntry {
            account_id,
            game_id,
            entry,
        }]
    }

    /// Applies a successful create or edit to the cached entry, when the complete read has one.
    /// Before the first complete read it still remembers the confirmed state, using the full entry
    /// the preceding check read supplied, so a late first read cannot hide or revert the game.
    pub(super) fn update_cached_entry(
        &mut self,
        game_id: u64,
        status: EntryStatus,
        collection_ids: &[String],
    ) -> Option<Effect> {
        let account_id = self.account.as_ref()?.id.clone();
        let cached = self.account_library.as_mut().and_then(|library| {
            library.entries.get_mut(&game_id).map(|entry| {
                entry.status = Some(status);
                entry.collection_ids = collection_ids.to_vec();
                entry.clone()
            })
        });
        let entry = match cached {
            Some(entry) => entry,
            None => AccountEntry {
                id: self.account_entries.get(&game_id)?.id.clone(),
                game_id,
                status: Some(status),
                collection_ids: collection_ids.to_vec(),
            },
        };
        let complete = self.account_library.is_some();
        self.record_local_account_entry(game_id, Some(entry.clone()));
        complete.then_some(Effect::PersistAccountEntry {
            account_id,
            game_id,
            entry: Some(entry),
        })
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
    ) -> Vec<Effect> {
        self.membership_reads.remove(&game_id);
        self.record_entry_read(game_id, &result)
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
