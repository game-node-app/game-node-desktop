use std::collections::BTreeMap;

use super::*;
use crate::game_api::{AccountEntry, Collection, LibraryEntry};
use crate::library::add_dialog::tests::NOW;
use crate::library::add_dialog::tests::library;
use crate::library::add_run::{AddSummary, CreatedEntry};
use crate::library::store::AccountLibrary;
use crate::library::{Input, SelectMode};

fn account_entry(
    game_id: u64,
    id: &str,
    status: Option<EntryStatus>,
    collections: &[&str],
) -> AccountEntry {
    AccountEntry {
        id: id.to_owned(),
        game_id,
        status,
        collection_ids: collections.iter().map(|id| (*id).to_owned()).collect(),
    }
}

fn full_entry(game_id: u64, status: EntryStatus, collections: &[&str]) -> LibraryEntry {
    LibraryEntry {
        id: format!("entry-{game_id}"),
        status: Some(status),
        collection_ids: collections.iter().map(|id| (*id).to_owned()).collect(),
        platform_ids: vec![6],
        finished_at: Some("2026-01-01T00:00:00Z".to_owned()),
    }
}

/// The five-game fixture, verified, with a complete account library read: Portal 2 (7) Finished
/// in `backlog`, Left 4 Dead (8) with a status this build does not know, Hades (9) Planned in
/// `coop`, and Celeste (10) not in the account library. Dota 2 (570) is unmatched.
fn status_machine() -> LibraryMachine {
    let mut machine = library();
    machine.account_library = Some(AccountLibrary {
        entries: BTreeMap::from([
            (
                7,
                account_entry(7, "entry-7", Some(EntryStatus::Finished), &["backlog"]),
            ),
            (8, account_entry(8, "entry-8", None, &[])),
            (
                9,
                account_entry(9, "entry-9", Some(EntryStatus::Planned), &["coop"]),
            ),
        ]),
        collections: vec![
            Collection {
                id: "backlog".to_owned(),
                name: "Backlog".to_owned(),
                default_status: None,
            },
            Collection {
                id: "coop".to_owned(),
                name: "Coop".to_owned(),
                default_status: None,
            },
        ],
        read_at: NOW,
    });
    machine
}

fn reads(effects: &[Effect]) -> Vec<(u64, u64)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::ReadEntry { op, game_id } => Some((*op, *game_id)),
            _ => None,
        })
        .collect()
}

fn updates(effects: &[Effect]) -> Vec<(u64, u64, EntryStatus, Vec<String>, LibraryEntry)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::UpdateEntry {
                op,
                game_id,
                status,
                collection_ids,
                existing,
            } => Some((
                *op,
                *game_id,
                *status,
                collection_ids.clone(),
                existing.clone(),
            )),
            _ => None,
        })
        .collect()
}

fn deletes(effects: &[Effect]) -> Vec<(u64, u64, String)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::DeleteEntry {
                op,
                game_id,
                entry_id,
            } => Some((*op, *game_id, entry_id.clone())),
            _ => None,
        })
        .collect()
}

fn selected_keys(ids: &[&str]) -> Vec<String> {
    ids.iter()
        .map(|id| crate::library::entry_key("steam", id))
        .collect()
}

#[test]
fn a_batch_status_change_skips_with_reasons_and_preserves_the_full_entry() {
    let mut machine = status_machine();
    let keys = selected_keys(&["620", "500", "570", "504230", "1145360"]);
    let effects = machine.handle(Input::SetStatus {
        keys,
        status: EntryStatus::Dropped,
    });
    let [(op, game_id)] = reads(&effects)[..] else {
        panic!("one game at a time: {effects:?}");
    };
    assert_eq!(game_id, 7);

    // The fresh read supplies the platforms and completion date the cache cannot.
    let fresh = full_entry(7, EntryStatus::Finished, &["backlog"]);
    let effects = machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Ok(Some(fresh.clone())),
    });
    let [(update_op, update_game, status, ref collection_ids, ref existing)] =
        updates(&effects)[..]
    else {
        panic!("{effects:?}");
    };
    assert_eq!((update_game, status), (7, EntryStatus::Dropped));
    assert_eq!(
        collection_ids.as_slice(),
        ["backlog"],
        "collections are preserved"
    );
    assert_eq!(*existing, fresh, "platforms and finished_at are sent back");
    assert_eq!(existing.platform_ids, [6]);

    let effects = machine.handle(Input::EntryUpdated {
        op: update_op,
        game_id: update_game,
        result: Ok(()),
    });
    // The write is followed by a confirmation read, not the next game yet.
    let [(refresh_op, refresh_game)] = reads(&effects)[..] else {
        panic!("{effects:?}");
    };
    assert_eq!(refresh_game, 7);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::PersistAccountEntry { game_id: 7, .. })),
        "the cache is updated: {effects:?}"
    );
    let effects = machine.handle(Input::EntryRead {
        op: refresh_op,
        game_id: refresh_game,
        result: Ok(Some(full_entry(7, EntryStatus::Dropped, &["backlog"]))),
    });
    let [(op, game_id)] = reads(&effects)[..] else {
        panic!("the next game follows: {effects:?}");
    };
    assert_eq!(game_id, 9, "Left 4 Dead was skipped without a request");

    let effects = machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Ok(Some(full_entry(9, EntryStatus::Planned, &["coop"]))),
    });
    let [(update_op, update_game, ..)] = updates(&effects)[..] else {
        panic!("{effects:?}");
    };
    let effects = machine.handle(Input::EntryUpdated {
        op: update_op,
        game_id: update_game,
        result: Ok(()),
    });
    let [(refresh_op, refresh_game)] = reads(&effects)[..] else {
        panic!("{effects:?}");
    };
    machine.handle(Input::EntryRead {
        op: refresh_op,
        game_id: refresh_game,
        result: Ok(Some(full_entry(9, EntryStatus::Dropped, &["coop"]))),
    });

    let run = machine.last_status_run.clone().expect("the run finished");
    assert_eq!(run.status, EntryStatus::Dropped);
    assert_eq!((run.changed, run.failed), (2, 0));
    assert_eq!(
        (
            run.skipped,
            run.skipped_unmatched,
            run.skipped_not_in_library,
            run.skipped_unknown_status
        ),
        (3, 1, 1, 1)
    );
    assert_eq!(
        machine
            .account_library
            .as_ref()
            .unwrap()
            .entries
            .get(&7)
            .unwrap()
            .status,
        Some(EntryStatus::Dropped)
    );
    assert_eq!(
        machine
            .account_library
            .as_ref()
            .unwrap()
            .entries
            .get(&7)
            .unwrap()
            .collection_ids,
        ["backlog"],
        "the status change kept the collections"
    );
    let view = machine.view().unwrap();
    assert_eq!(view.status_run.unwrap().run, run.run);
}

#[test]
fn a_failed_status_write_counts_as_failed_and_the_rest_still_runs() {
    let mut machine = status_machine();
    let keys = selected_keys(&["620", "1145360"]);
    let effects = machine.handle(Input::SetStatus {
        keys,
        status: EntryStatus::Finished,
    });
    let (op, game_id) = reads(&effects)[0];
    let effects = machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Ok(Some(full_entry(7, EntryStatus::Playing, &[]))),
    });
    let (update_op, update_game, ..) = updates(&effects)[0].clone();
    let effects = machine.handle(Input::EntryUpdated {
        op: update_op,
        game_id: update_game,
        result: Err(ApiError::Failed),
    });
    // The failed game is read back before the run moves on.
    let (refresh_op, refresh_game) = reads(&effects)[0];
    assert_eq!(refresh_game, 7);
    let effects = machine.handle(Input::EntryRead {
        op: refresh_op,
        game_id: refresh_game,
        result: Ok(Some(full_entry(7, EntryStatus::Playing, &[]))),
    });
    let (op, game_id) = reads(&effects)[0];
    assert_eq!(game_id, 9, "the failed game does not stop the other");
    let effects = machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Ok(Some(full_entry(9, EntryStatus::Planned, &["coop"]))),
    });
    let (update_op, update_game, ..) = updates(&effects)[0].clone();
    let effects = machine.handle(Input::EntryUpdated {
        op: update_op,
        game_id: update_game,
        result: Ok(()),
    });
    let (op, game_id) = reads(&effects)[0];
    machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Ok(Some(full_entry(9, EntryStatus::Finished, &["coop"]))),
    });

    let run = machine.last_status_run.clone().unwrap();
    assert_eq!((run.changed, run.failed, run.skipped), (1, 1, 0));
}

#[test]
fn a_stale_status_answer_after_the_run_changes_nothing() {
    let mut machine = status_machine();
    let effects = machine.handle(Input::SetStatus {
        keys: selected_keys(&["620"]),
        status: EntryStatus::Dropped,
    });
    let stale = reads(&effects)[0];
    let effects = machine.handle(Input::EntryRead {
        op: stale.0,
        game_id: stale.1,
        result: Ok(Some(full_entry(7, EntryStatus::Playing, &[]))),
    });
    let (update_op, update_game, ..) = updates(&effects)[0].clone();
    let effects = machine.handle(Input::EntryUpdated {
        op: update_op,
        game_id: update_game,
        result: Ok(()),
    });
    let (refresh_op, refresh_game) = reads(&effects)[0];
    machine.handle(Input::EntryRead {
        op: refresh_op,
        game_id: refresh_game,
        result: Ok(Some(full_entry(7, EntryStatus::Dropped, &[]))),
    });
    let before = machine.last_status_run.clone();

    // A late answer from the first read cannot restart or change the finished run.
    let effects = machine.handle(Input::EntryRead {
        op: stale.0,
        game_id: stale.1,
        result: Ok(Some(full_entry(7, EntryStatus::Playing, &[]))),
    });
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(machine.last_status_run, before);
}

#[test]
fn a_collection_add_sends_only_games_not_already_in_it() {
    let mut machine = status_machine();
    let keys = selected_keys(&["620", "500", "570", "504230", "1145360"]);
    let effects = machine.handle(Input::AddToCollection {
        keys,
        collection_id: "backlog".to_owned(),
    });
    let [(op, game_id)] = reads(&effects)[..] else {
        panic!("{effects:?}");
    };
    assert_eq!(game_id, 9, "Portal 2 is already in backlog");

    let effects = machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Ok(Some(full_entry(9, EntryStatus::Planned, &["coop"]))),
    });
    let [(update_op, update_game, status, ref collection_ids, ref existing)] =
        updates(&effects)[..]
    else {
        panic!("{effects:?}");
    };
    assert_eq!(update_game, 9);
    assert_eq!(status, EntryStatus::Planned, "the status is kept");
    assert_eq!(collection_ids.as_slice(), ["coop", "backlog"]);
    assert_eq!(existing.platform_ids, [6]);
    let effects = machine.handle(Input::EntryUpdated {
        op: update_op,
        game_id: update_game,
        result: Ok(()),
    });
    let [(refresh_op, refresh_game)] = reads(&effects)[..] else {
        panic!("{effects:?}");
    };
    machine.handle(Input::EntryRead {
        op: refresh_op,
        game_id: refresh_game,
        result: Ok(Some(full_entry(
            9,
            EntryStatus::Planned,
            &["coop", "backlog"],
        ))),
    });

    let run = machine
        .last_collection_run
        .clone()
        .expect("the run finished");
    assert_eq!(run.collection_name, "Backlog");
    assert_eq!((run.changed, run.failed, run.skipped), (1, 0, 4));
    assert_eq!(
        (
            run.skipped_unmatched,
            run.skipped_not_in_library,
            run.skipped_unknown_status,
            run.skipped_already_in_collection
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(
        machine
            .account_library
            .as_ref()
            .unwrap()
            .entries
            .get(&9)
            .unwrap()
            .collection_ids,
        ["coop", "backlog"]
    );
}

#[test]
fn a_failed_collection_write_is_counted() {
    let mut machine = status_machine();
    let effects = machine.handle(Input::AddToCollection {
        keys: selected_keys(&["1145360"]),
        collection_id: "backlog".to_owned(),
    });
    let (op, game_id) = reads(&effects)[0];
    let effects = machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Ok(Some(full_entry(9, EntryStatus::Planned, &["coop"]))),
    });
    let (update_op, update_game, ..) = updates(&effects)[0].clone();
    let effects = machine.handle(Input::EntryUpdated {
        op: update_op,
        game_id: update_game,
        result: Err(ApiError::Failed),
    });
    let (refresh_op, refresh_game) = reads(&effects)[0];
    assert_eq!(refresh_game, 9);
    machine.handle(Input::EntryRead {
        op: refresh_op,
        game_id: refresh_game,
        result: Ok(Some(full_entry(9, EntryStatus::Planned, &["coop"]))),
    });
    let run = machine.last_collection_run.clone().unwrap();
    assert_eq!((run.changed, run.failed), (0, 1));
}

#[test]
fn undo_deletes_only_the_created_ids_one_at_a_time() {
    let mut machine = status_machine();
    machine.last_add = Some(AddSummary {
        run: 42,
        items: Vec::new(),
        created: vec![
            CreatedEntry {
                game_id: 7,
                entry_id: "entry-7".to_owned(),
            },
            CreatedEntry {
                game_id: 9,
                entry_id: "entry-9".to_owned(),
            },
        ],
    });
    let effects = machine.handle(Input::UndoAdd { run: 42 });
    let [(op, game_id, ref entry_id)] = deletes(&effects)[..] else {
        panic!("{effects:?}");
    };
    assert_eq!((game_id, entry_id.as_str()), (7, "entry-7"));

    let effects = machine.handle(Input::EntryDeleted {
        op,
        game_id,
        result: Ok(()),
    });
    assert!(
        !machine
            .account_library
            .as_ref()
            .unwrap()
            .entries
            .contains_key(&7),
        "the cache no longer shows the deleted entry"
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::PersistAccountEntry {
                game_id: 7,
                entry: None,
                ..
            }
        )),
        "{effects:?}"
    );
    let [(_, read_game)] = reads(&effects)[..] else {
        panic!("the deleted game is refreshed: {effects:?}");
    };
    assert_eq!(read_game, 7);
    let [(next_op, next_game, ref next_id)] = deletes(&effects)[..] else {
        panic!("the next delete follows: {effects:?}");
    };
    assert_eq!((next_game, next_id.as_str()), (9, "entry-9"));
    assert_ne!(next_op, op);

    let effects = machine.handle(Input::EntryDeleted {
        op: next_op,
        game_id: next_game,
        result: Ok(()),
    });
    let undo = machine.last_undo.clone().expect("the undo finished");
    assert_eq!(
        (undo.run, undo.state, undo.deleted, undo.failed),
        (42, UndoState::Done, 2, 0)
    );
    assert!(
        machine
            .last_add
            .as_ref()
            .is_some_and(|summary| summary.created.is_empty()),
        "an undone add cannot be undone twice"
    );
    // The refresh of the second game does not start another delete.
    let (refresh_op, refresh_game) = reads(&effects)[0];
    let effects = machine.handle(Input::EntryRead {
        op: refresh_op,
        game_id: refresh_game,
        result: Ok(None),
    });
    assert!(deletes(&effects).is_empty());
    assert!(machine.handle(Input::UndoAdd { run: 42 }).is_empty());
}

#[test]
fn a_partial_undo_and_a_failed_undo_are_reported() {
    let mut machine = status_machine();
    let created = |game_id| CreatedEntry {
        game_id,
        entry_id: format!("entry-{game_id}"),
    };
    machine.last_add = Some(AddSummary {
        run: 7,
        items: Vec::new(),
        created: vec![created(7), created(9)],
    });
    let effects = machine.handle(Input::UndoAdd { run: 7 });
    let (op, game_id, _) = deletes(&effects)[0].clone();
    let effects = machine.handle(Input::EntryDeleted {
        op,
        game_id,
        result: Ok(()),
    });
    let (op, game_id, _) = deletes(&effects)[0].clone();
    machine.handle(Input::EntryDeleted {
        op,
        game_id,
        result: Err(ApiError::Failed),
    });
    let undo = machine.last_undo.clone().unwrap();
    assert_eq!(
        (undo.state, undo.deleted, undo.failed),
        (UndoState::Partial, 1, 1)
    );

    // A run where every delete fails is `failed`, not `partial`.
    machine.last_add = Some(AddSummary {
        run: 8,
        items: Vec::new(),
        created: vec![created(7), created(9)],
    });
    let effects = machine.handle(Input::UndoAdd { run: 8 });
    let (op, game_id, _) = deletes(&effects)[0].clone();
    let effects = machine.handle(Input::EntryDeleted {
        op,
        game_id,
        result: Err(ApiError::Failed),
    });
    let (op, game_id, _) = deletes(&effects)[0].clone();
    machine.handle(Input::EntryDeleted {
        op,
        game_id,
        result: Err(ApiError::Failed),
    });
    let undo = machine.last_undo.clone().unwrap();
    assert_eq!(
        (undo.state, undo.deleted, undo.failed),
        (UndoState::Failed, 0, 2)
    );
}

#[test]
fn a_stale_undo_run_is_rejected() {
    let mut machine = status_machine();
    machine.last_add = Some(AddSummary {
        run: 42,
        items: Vec::new(),
        created: vec![CreatedEntry {
            game_id: 7,
            entry_id: "entry-7".to_owned(),
        }],
    });
    assert!(machine.handle(Input::UndoAdd { run: 41 }).is_empty());
    assert!(deletes(&machine.handle(Input::UndoAdd { run: 42 })).len() == 1);

    // A different account cannot answer the open undo.
    machine.handle(Input::AccountChanged(None));
    assert!(machine.undo.is_none());
    assert!(machine.last_add.is_none());
}

#[test]
fn a_single_game_detail_status_uses_the_same_run() {
    let mut machine = status_machine();
    machine.handle(Input::Select {
        key: crate::library::entry_key("steam", "620"),
        mode: SelectMode::Replace,
    });
    let effects = machine.handle(Input::SetStatus {
        keys: selected_keys(&["620"]),
        status: EntryStatus::Finished,
    });
    let (op, game_id) = reads(&effects)[0];
    assert_eq!(game_id, 7);
    let effects = machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Ok(Some(full_entry(7, EntryStatus::Finished, &["backlog"]))),
    });
    let (update_op, update_game, ..) = updates(&effects)[0].clone();
    let effects = machine.handle(Input::EntryUpdated {
        op: update_op,
        game_id: update_game,
        result: Ok(()),
    });
    let (refresh_op, refresh_game) = reads(&effects)[0];
    machine.handle(Input::EntryRead {
        op: refresh_op,
        game_id: refresh_game,
        result: Ok(Some(full_entry(7, EntryStatus::Finished, &["backlog"]))),
    });
    assert_eq!(
        machine.last_status_run.clone().unwrap().status,
        EntryStatus::Finished
    );
}

#[test]
fn a_late_complete_read_does_not_resurrect_an_entry_an_undo_deleted() {
    let mut machine = status_machine();
    let sync_op = machine
        .account_library_sync
        .expect("the verified session started a complete read");
    machine.last_add = Some(AddSummary {
        run: 42,
        items: Vec::new(),
        created: vec![CreatedEntry {
            game_id: 7,
            entry_id: "entry-7".to_owned(),
        }],
    });

    let effects = machine.handle(Input::UndoAdd { run: 42 });
    let (op, game_id, _) = deletes(&effects)[0].clone();
    machine.handle(Input::EntryDeleted {
        op,
        game_id,
        result: Ok(()),
    });
    assert!(
        !machine
            .account_library
            .as_ref()
            .unwrap()
            .entries
            .contains_key(&7)
    );

    // The read's snapshot still had the entry: the confirmed delete wins and is persisted.
    let stale = vec![
        account_entry(7, "entry-7", Some(EntryStatus::Finished), &["backlog"]),
        account_entry(9, "entry-9", Some(EntryStatus::Planned), &["coop"]),
    ];
    let effects = machine.handle(Input::AccountLibrarySynced {
        op: sync_op,
        result: Ok((stale, Vec::new())),
        now: NOW + 10,
    });
    let [Effect::PersistAccountLibrary { entries, .. }] = effects.as_slice() else {
        panic!("one complete replace: {effects:?}");
    };
    assert!(
        !entries.iter().any(|entry| entry.game_id == 7),
        "the deleted entry is not written back: {entries:?}"
    );
    assert!(
        entries.iter().any(|entry| entry.game_id == 9),
        "untouched entries still come from the complete read"
    );
    assert!(
        !machine
            .account_library
            .as_ref()
            .unwrap()
            .entries
            .contains_key(&7)
    );
    assert!(
        machine
            .account_library
            .as_ref()
            .unwrap()
            .entries
            .contains_key(&9)
    );
}
