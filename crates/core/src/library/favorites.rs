//! Favorites: the local-first star on a matched GameNode game and its two-way sync.
//!
//! A favorite belongs to the GameNode game, not to one provider entry, so every entry matched to
//! the same game shows the same star. Only matched entries can be favorited: the server flag
//! lives on the game's collection entry, and an unmatched entry has no game to address.
//!
//! The local state is authoritative for changes the server has not confirmed (`pending`). A
//! completed read is authoritative for everything else: favorites the server reports appear,
//! favorites it no longer reports disappear, and pending changes are kept and sent again.

use std::collections::BTreeSet;

use crate::game_api::{ApiError, FavoriteError};

use super::machine::{Effect, LibraryMachine, Phase};
use super::store::FavoriteRecord;

/// What the library knows about one game's favorite state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FavoriteState {
    pub is_favorite: bool,
    /// The local value has not been confirmed by GameNode yet.
    pub pending: bool,
}

impl LibraryMachine {
    /// The current time the relative filters use. The engine refreshes it before every input.
    pub fn set_now(&mut self, now: i64) {
        self.now = now;
    }

    /// Unix seconds of the last input; the library never reads the clock itself.
    pub fn now(&self) -> i64 {
        self.now
    }

    /// The favorite state of one GameNode game, when the library knows it.
    pub fn favorite_state(&self, game_id: u64) -> Option<FavoriteState> {
        self.favorites.get(&game_id).copied()
    }

    /// Whether favorites may leave this PC right now.
    pub fn favorites_sync_active(&self) -> bool {
        self.favorites_sync_active
    }

    /// The engine calls this whenever the gate (`sync enabled`, `explained`, `verified`)
    /// changes. Becoming active reads the server set once and sends every pending change.
    pub fn set_sync_active(&mut self, active: bool) -> Vec<Effect> {
        if active == self.favorites_sync_active {
            return vec![];
        }
        self.favorites_sync_active = active;
        if active {
            self.favorites_sync_pending = true;
            self.plan_favorites()
        } else {
            // Requests already in flight are ignored when their answers arrive.
            self.favorites_sync_pending = false;
            self.favorites_sync_op = None;
            self.favorite_pushes.clear();
            vec![]
        }
    }

    /// Asks for a favorite read and for every pending push to be retried.
    pub fn request_favorites_sync(&mut self) -> Vec<Effect> {
        if !self.favorites_sync_active {
            return vec![];
        }
        self.favorites_sync_pending = true;
        self.plan_favorites()
    }

    /// Starts the read and the pushes that are due. Shared with the load path, which has an
    /// account only once the cache is installed.
    pub(super) fn plan_favorites(&mut self) -> Vec<Effect> {
        plan(self)
    }

    pub(super) fn on_set_favorite(&mut self, key: String, favorite: bool, now: i64) -> Vec<Effect> {
        let Some(entry) = self.entries.get(&key) else {
            return vec![];
        };
        let Some(game_id) = self.match_status(entry).game_id() else {
            // Unmatched entries have no server game to favorite.
            return vec![];
        };
        let Some(account_id) = self.account.as_ref().map(|account| account.id.clone()) else {
            return vec![];
        };
        self.favorites.insert(
            game_id,
            FavoriteState {
                is_favorite: favorite,
                pending: true,
            },
        );
        let mut effects = vec![Effect::PersistFavorite(FavoriteRecord {
            account_id,
            game_id,
            is_favorite: favorite,
            pending: true,
            updated_at: now,
        })];
        effects.extend(self.plan_favorites());
        effects
    }

    pub(super) fn on_favorites_synced(
        &mut self,
        op: u64,
        result: Result<Vec<u64>, ApiError>,
        now: i64,
    ) -> Vec<Effect> {
        if self.favorites_sync_op != Some(op) {
            return vec![];
        }
        self.favorites_sync_op = None;
        // A failed or incomplete read changes nothing; a later trigger retries it. Pushes that
        // waited for the read may now go.
        let Ok(server) = result else {
            return self.plan_favorites();
        };
        let Some(account_id) = self.account.as_ref().map(|account| account.id.clone()) else {
            return vec![];
        };
        let server: BTreeSet<u64> = server.into_iter().collect();
        let mut effects = Vec::new();
        let persist = |game_id: u64, is_favorite: bool, pending: bool| {
            Effect::PersistFavorite(FavoriteRecord {
                account_id: account_id.clone(),
                game_id,
                is_favorite,
                pending,
                updated_at: now,
            })
        };

        // Games the server reports become favorites unless a local change is still pending.
        for &game_id in &server {
            let state = self.favorites.entry(game_id).or_default();
            if !state.pending && !state.is_favorite {
                state.is_favorite = true;
                effects.push(persist(game_id, true, false));
            }
        }
        // Games the server no longer reports stop being favorites, again unless pending.
        let removed: Vec<u64> = self
            .favorites
            .iter()
            .filter(|(game_id, state)| {
                state.is_favorite && !state.pending && !server.contains(game_id)
            })
            .map(|(game_id, _)| *game_id)
            .collect();
        for game_id in removed {
            self.favorites.insert(game_id, FavoriteState::default());
            effects.push(persist(game_id, false, false));
        }

        effects.extend(self.plan_favorites());
        effects
    }

    pub(super) fn on_favorite_pushed(
        &mut self,
        op: u64,
        game_id: u64,
        favorite: bool,
        result: Result<(), FavoriteError>,
        now: i64,
    ) -> Vec<Effect> {
        if self.favorite_pushes.get(&game_id) != Some(&op) {
            return vec![];
        }
        self.favorite_pushes.remove(&game_id);
        let Some(account_id) = self.account.as_ref().map(|account| account.id.clone()) else {
            return vec![];
        };
        let state = self.favorites.get(&game_id).copied().unwrap_or_default();
        match result {
            // The server has the value that was sent only if the user did not toggle again in
            // the meantime. Otherwise the newer local value stays pending and is sent again.
            Ok(()) if state.is_favorite == favorite => {
                if !state.pending {
                    return vec![];
                }
                self.favorites.insert(
                    game_id,
                    FavoriteState {
                        is_favorite: favorite,
                        pending: false,
                    },
                );
                let mut effects = vec![Effect::PersistFavorite(FavoriteRecord {
                    account_id,
                    game_id,
                    is_favorite: favorite,
                    pending: false,
                    updated_at: now,
                })];
                effects.extend(self.plan_favorites());
                effects
            }
            Ok(()) => self.plan_favorites(),
            // Not in the GameNode library, offline, or a rejected session: the change stays
            // local and pending for the next trigger.
            Err(FavoriteError::NotInLibrary | FavoriteError::Failed | FavoriteError::Rejected) => {
                vec![]
            }
        }
    }
}

/// Starts the favorite read and the pending pushes that are due.
///
/// A read and the pushes never run at the same time: a read that started before a push was
/// confirmed would otherwise see a server snapshot without that change and erase it. One read may
/// be in flight and one push per game; the op ids discard stale answers.
pub(super) fn plan(machine: &mut LibraryMachine) -> Vec<Effect> {
    if !machine.favorites_sync_active || !matches!(machine.phase, Phase::Ready) {
        return vec![];
    }
    let mut effects = Vec::new();
    let pushes_in_flight = !machine.favorite_pushes.is_empty();
    if machine.favorites_sync_pending
        && machine.favorites_sync_op.is_none()
        && !pushes_in_flight
        && let Some(user_id) = machine.account.as_ref().map(|account| account.id.clone())
    {
        let op = machine.next_op();
        machine.favorites_sync_op = Some(op);
        machine.favorites_sync_pending = false;
        effects.push(Effect::SyncFavorites { op, user_id });
    }
    if machine.favorites_sync_op.is_some() {
        return effects;
    }
    let due: Vec<(u64, bool)> = machine
        .favorites
        .iter()
        .filter(|(game_id, state)| state.pending && !machine.favorite_pushes.contains_key(game_id))
        .map(|(game_id, state)| (*game_id, state.is_favorite))
        .collect();
    for (game_id, favorite) in due {
        let op = machine.next_op();
        machine.favorite_pushes.insert(game_id, op);
        effects.push(Effect::PushFavorite {
            op,
            game_id,
            favorite,
        });
    }
    effects
}

#[cfg(test)]
mod tests;
