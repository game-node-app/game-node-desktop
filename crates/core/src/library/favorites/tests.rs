use super::*;
use crate::game_api::ApiError;
use crate::library::machine::Input;
use crate::library::machine::tests::{account, finish_scan, loaded, record, stored};
use crate::library::store::{AutoMatch, LibraryCache};

fn portal() -> LibraryCache {
    LibraryCache {
        entries: vec![stored("steam", "620", "portal 2", true)],
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "620".to_owned(),
            game_ids: vec![7],
            checked_at: 1,
        }],
        ..LibraryCache::default()
    }
}

fn machine_with_portal() -> LibraryMachine {
    let (mut machine, _) = loaded(false, portal());
    finish_scan(&mut machine, vec![record("620", "Portal 2", "/p")], 9);
    machine
}

fn pushes(effects: &[Effect]) -> Vec<(u64, u64, bool)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::PushFavorite {
                op,
                game_id,
                favorite,
            } => Some((*op, *game_id, *favorite)),
            _ => None,
        })
        .collect()
}

fn syncs(effects: &[Effect]) -> Vec<(u64, String)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::SyncFavorites { op, user_id } => Some((*op, user_id.clone())),
            _ => None,
        })
        .collect()
}

fn persisted(effects: &[Effect]) -> Vec<FavoriteRecord> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::PersistFavorite(record) => Some(record.clone()),
            _ => None,
        })
        .collect()
}

fn toggle(machine: &mut LibraryMachine, key: &str, favorite: bool) -> Vec<Effect> {
    machine.handle(Input::SetFavorite {
        key: key.to_owned(),
        favorite,
        now: 50,
    })
}

#[test]
fn a_toggle_is_local_first_and_persisted_for_the_account() {
    let mut machine = machine_with_portal();
    let effects = toggle(&mut machine, "steam:620", true);

    let state = machine.favorite_state(7).unwrap();
    assert!(state.is_favorite && state.pending);
    assert!(pushes(&effects).is_empty(), "sync is off");
    let saved = persisted(&effects);
    assert_eq!(saved.len(), 1);
    assert_eq!((saved[0].account_id.as_str(), saved[0].game_id), ("a", 7));
    assert!(saved[0].is_favorite && saved[0].pending);
    // Every row matched to the same game shows the star.
    assert!(machine.visible_rows("")[0].is_favorite);
}

#[test]
fn an_unmatched_entry_cannot_be_favorited() {
    let mut machine = machine_with_portal();
    machine.entries.get_mut("steam:620").unwrap().store_id = "999".to_owned();
    let effects = toggle(&mut machine, "steam:620", true);
    assert!(effects.is_empty(), "no game id, no favorite");
    assert!(machine.favorite_state(7).is_none());
}

/// Turns sync on and answers the read it starts, returning the pushes the merge emitted.
fn activate(machine: &mut LibraryMachine) -> Vec<Effect> {
    let effects = machine.handle(Input::SetSyncActive(true));
    let sync = syncs(&effects);
    assert_eq!(sync.len(), 1);
    assert!(
        pushes(&effects).is_empty(),
        "a read and a push never race: the read goes first"
    );
    assert!(machine.favorites_sync_active());
    machine.handle(Input::FavoritesSynced {
        op: sync[0].0,
        result: Ok(vec![]),
        now: 60,
    })
}

#[test]
fn activating_sync_reads_before_it_sends_pending_favorites() {
    let mut machine = machine_with_portal();
    toggle(&mut machine, "steam:620", true);

    let effects = activate(&mut machine);
    let push = pushes(&effects);
    assert_eq!(push.len(), 1);
    assert_eq!((push[0].1, push[0].2), (7, true));

    // Repeating activation does not start a second read while one is in flight.
    let effects = machine.handle(Input::SetSyncActive(true));
    assert!(effects.is_empty());
    // A requested retry waits for the push in flight rather than racing it.
    assert!(syncs(&machine.handle(Input::SyncFavoritesRequested)).is_empty());
    // ...and starts once it answers.
    let effects = machine.handle(Input::FavoritePushed {
        op: push[0].0,
        game_id: 7,
        favorite: true,
        result: Ok(()),
        now: 70,
    });
    assert_eq!(syncs(&effects).len(), 1);
}

#[test]
fn a_completed_read_adds_server_favorites_and_removes_untouched_local_ones() {
    let mut machine = machine_with_portal();
    machine.handle(Input::SetSyncActive(true));
    // A local favorite the server no longer has, with no pending change: a confirmed removal.
    machine.favorites.insert(
        7,
        FavoriteState {
            is_favorite: true,
            pending: false,
        },
    );
    let effects = machine.handle(Input::FavoritesSynced {
        op: machine.favorites_sync_op.unwrap(),
        result: Ok(vec![7, 8]),
        now: 70,
    });

    assert!(machine.favorite_state(7).unwrap().is_favorite);
    assert!(!machine.favorite_state(7).unwrap().pending);
    assert!(machine.favorite_state(8).unwrap().is_favorite);
    assert!(!machine.favorite_state(8).unwrap().pending);
    let saved: Vec<u64> = persisted(&effects).iter().map(|r| r.game_id).collect();
    assert_eq!(saved, [8]);

    let effects = machine.handle(Input::FavoritesSynced {
        op: machine.favorites_sync_op.unwrap_or(0),
        result: Ok(vec![8]),
        now: 80,
    });
    // There is no read in flight, so a stray answer changes nothing.
    assert!(persisted(&effects).is_empty());
    assert!(machine.favorite_state(7).unwrap().is_favorite);
}

#[test]
fn a_server_removal_clears_a_confirmed_local_favorite() {
    let mut machine = machine_with_portal();
    machine.handle(Input::SetSyncActive(true));
    machine.favorites.insert(
        7,
        FavoriteState {
            is_favorite: true,
            pending: false,
        },
    );
    // Read the op before the answer clears it.
    let op = machine.favorites_sync_op.unwrap();
    let effects = machine.handle(Input::FavoritesSynced {
        op,
        result: Ok(vec![]),
        now: 70,
    });
    assert!(!machine.favorite_state(7).unwrap().is_favorite);
    let saved = persisted(&effects);
    assert_eq!(saved.len(), 1);
    assert!(!saved[0].is_favorite && !saved[0].pending);
}

#[test]
fn a_pending_local_change_wins_over_a_stale_server_value() {
    let mut machine = machine_with_portal();
    machine.handle(Input::SetSyncActive(true));
    let op = machine.favorites_sync_op.unwrap();
    // The user removes a favorite before the read that still reports it arrives.
    toggle(&mut machine, "steam:620", true);
    let effects = toggle(&mut machine, "steam:620", false);
    assert!(!persisted(&effects).is_empty());

    let effects = machine.handle(Input::FavoritesSynced {
        op,
        result: Ok(vec![7]),
        now: 70,
    });
    let state = machine.favorite_state(7).unwrap();
    assert!(!state.is_favorite, "the local removal stays");
    assert!(state.pending, "and is sent again");
    // No favorite was confirmed on the server by this read.
    assert!(
        persisted(&effects)
            .iter()
            .all(|r| !r.is_favorite || r.pending)
    );
}

#[test]
fn a_failed_read_changes_nothing_and_is_retried_later() {
    let mut machine = machine_with_portal();
    machine.favorites.insert(
        7,
        FavoriteState {
            is_favorite: true,
            pending: false,
        },
    );
    machine.handle(Input::SetSyncActive(true));
    let op = machine.favorites_sync_op.unwrap();
    let effects = machine.handle(Input::FavoritesSynced {
        op,
        result: Err(ApiError::Failed),
        now: 70,
    });
    assert!(effects.is_empty());
    assert!(machine.favorite_state(7).unwrap().is_favorite);

    let effects = machine.handle(Input::SyncFavoritesRequested);
    assert_eq!(syncs(&effects).len(), 1, "a later trigger retries the read");
}

#[test]
fn a_confirmed_push_clears_pending_and_a_failed_one_keeps_it() {
    let mut machine = machine_with_portal();
    activate(&mut machine);
    let effects = toggle(&mut machine, "steam:620", true);
    let push = pushes(&effects);
    assert_eq!(push.len(), 1);

    let effects = machine.handle(Input::FavoritePushed {
        op: push[0].0,
        game_id: 7,
        favorite: true,
        result: Ok(()),
        now: 80,
    });
    let state = machine.favorite_state(7).unwrap();
    assert!(state.is_favorite && !state.pending);
    let saved = persisted(&effects);
    assert!(saved.iter().any(|record| !record.pending));

    // A failure keeps the change for the next trigger without a tight retry loop.
    let mut machine = machine_with_portal();
    activate(&mut machine);
    let effects = toggle(&mut machine, "steam:620", true);
    let push = pushes(&effects)[0];
    let effects = machine.handle(Input::FavoritePushed {
        op: push.0,
        game_id: 7,
        favorite: true,
        result: Err(FavoriteError::Failed),
        now: 80,
    });
    assert!(pushes(&effects).is_empty());
    assert!(machine.favorite_state(7).unwrap().pending);
}

#[test]
fn not_in_library_and_rejected_keep_the_change_pending() {
    for error in [FavoriteError::NotInLibrary, FavoriteError::Rejected] {
        let mut machine = machine_with_portal();
        activate(&mut machine);
        let effects = toggle(&mut machine, "steam:620", true);
        let push = pushes(&effects)[0];
        let effects = machine.handle(Input::FavoritePushed {
            op: push.0,
            game_id: 7,
            favorite: true,
            result: Err(error),
            now: 80,
        });
        assert!(pushes(&effects).is_empty(), "{error:?} must not spin");
        assert!(machine.favorite_state(7).unwrap().pending, "{error:?}");

        // The add flow retries it without a user action: the read goes first, then the push.
        let effects = machine.handle(Input::SyncFavoritesRequested);
        let sync = syncs(&effects);
        assert_eq!(sync.len(), 1, "{error:?}");
        assert!(pushes(&effects).is_empty(), "{error:?}");
        let effects = machine.handle(Input::FavoritesSynced {
            op: sync[0].0,
            result: Ok(vec![]),
            now: 90,
        });
        assert_eq!(pushes(&effects).len(), 1, "{error:?}");
    }
}

#[test]
fn toggling_back_while_a_push_is_in_flight_sends_the_new_value() {
    let mut machine = machine_with_portal();
    activate(&mut machine);
    let effects = toggle(&mut machine, "steam:620", true);
    let first = pushes(&effects)[0];
    // The user changes their mind before GameNode answers.
    let effects = toggle(&mut machine, "steam:620", false);
    assert!(pushes(&effects).is_empty(), "one push per game at a time");
    let state = machine.favorite_state(7).unwrap();
    assert!(!state.is_favorite && state.pending);

    let effects = machine.handle(Input::FavoritePushed {
        op: first.0,
        game_id: 7,
        favorite: true,
        result: Ok(()),
        now: 80,
    });
    let state = machine.favorite_state(7).unwrap();
    assert!(!state.is_favorite, "the newer local value stays");
    assert!(state.pending);
    let push = pushes(&effects);
    assert_eq!(push.len(), 1);
    assert_eq!((push[0].1, push[0].2), (7, false));
}

#[test]
fn stale_answers_are_ignored() {
    let mut machine = machine_with_portal();
    activate(&mut machine);
    let effects = toggle(&mut machine, "steam:620", true);
    let push = pushes(&effects)[0];
    let effects = machine.handle(Input::FavoritePushed {
        op: push.0 + 1,
        game_id: 7,
        favorite: true,
        result: Ok(()),
        now: 80,
    });
    assert!(effects.is_empty());
    assert!(machine.favorite_state(7).unwrap().pending);
}

#[test]
fn sync_off_sends_nothing_and_signing_out_keeps_the_favorites_out_of_view() {
    let mut machine = machine_with_portal();
    let effects = toggle(&mut machine, "steam:620", true);
    assert!(pushes(&effects).is_empty());
    assert!(syncs(&effects).is_empty());

    machine.handle(Input::AccountChanged(Some(account("b"))));
    assert!(machine.favorite_state(7).is_none());
    assert!(machine.visible_rows("").is_empty());
}

#[test]
fn cached_favorites_are_visible_before_any_request() {
    let cache = LibraryCache {
        favorites: vec![FavoriteRecord {
            account_id: "a".to_owned(),
            game_id: 7,
            is_favorite: true,
            pending: true,
            updated_at: 1,
        }],
        ..portal()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("620", "Portal 2", "/p")], 9);
    let row = &machine.visible_rows("")[0];
    assert!(row.is_favorite);
    assert!(machine.favorite_state(7).unwrap().pending);
}
