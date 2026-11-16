use super::*;
use crate::game_api::{AccountEntry, CreateCollectionError, LibraryEntry};
use crate::library::machine::tests::{loaded, record, stored};
use crate::library::store::{AccountLibrary, AutoMatch, GameRecord, LibraryCache};
use crate::library::{Access, Input, Membership, SelectMode, entry_key};
use std::collections::BTreeMap;

pub(crate) const NOW: i64 = 5_000;

fn game(id: u64, name: &str) -> GameRecord {
    GameRecord {
        game_id: id,
        name: name.to_owned(),
        release_year: None,
        cover_url: None,
        cover_file: None,
        cover_failed_at: None,
        platforms: Vec::new(),
    }
}

/// A verified library: Portal 2 (620 → game 7), Dota 2 (570, unknown to GameNode), Left 4 Dead
/// (500 → 8), Hades (1145360 → 9), Celeste (504230 → 10). Nothing is selected.
pub(crate) fn library() -> LibraryMachine {
    let games = [
        ("620", "Portal 2", Some(7)),
        ("570", "Dota 2", None),
        ("500", "Left 4 Dead", Some(8)),
        ("1145360", "Hades", Some(9)),
        ("504230", "Celeste", Some(10)),
    ];
    let mut cache = LibraryCache::default();
    for (id, name, matched) in games {
        cache.entries.push(stored("steam", id, name, true));
        cache.auto_matches.push(AutoMatch {
            category: 1,
            uid: id.to_owned(),
            game_ids: matched.into_iter().collect(),
            checked_at: NOW,
        });
        if let Some(game_id) = matched {
            cache.games.push(game(game_id, name));
        }
    }
    let (mut machine, _) = loaded(false, cache);
    let op = machine.scan.as_ref().unwrap().op;
    machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(games
            .iter()
            .map(|(id, name, _)| record(id, name, "/g"))
            .collect()),
        now: NOW,
    });
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    machine
}

pub(crate) fn select(machine: &mut LibraryMachine, id: &str) -> Vec<Effect> {
    machine.handle(Input::Select {
        key: entry_key("steam", id),
        mode: SelectMode::Replace,
    })
}

pub(crate) fn select_all(machine: &mut LibraryMachine, ids: &[&str]) -> Vec<Effect> {
    let mut effects = select(machine, ids[0]);
    for id in &ids[1..] {
        effects.extend(machine.handle(Input::Select {
            key: entry_key("steam", id),
            mode: SelectMode::Toggle,
        }));
    }
    effects
}

pub(crate) fn reads(effects: &[Effect]) -> Vec<(u64, u64)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::ReadEntry { op, game_id } => Some((*op, *game_id)),
            _ => None,
        })
        .collect()
}

pub(crate) fn creates(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|effect| matches!(effect, Effect::CreateEntry { .. }))
        .collect()
}

fn list_ops(effects: &[Effect]) -> Vec<(u64, String)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::ListCollections { op, user_id } => Some((*op, user_id.clone())),
            _ => None,
        })
        .collect()
}

pub(crate) fn answer_read(
    machine: &mut LibraryMachine,
    effects: &[Effect],
    result: Result<Option<LibraryEntry>, ApiError>,
) -> Vec<Effect> {
    let (op, game_id) = reads(effects)[0];
    machine.handle(Input::EntryRead {
        op,
        game_id,
        result,
    })
}

pub(crate) fn entry(status: Option<EntryStatus>) -> LibraryEntry {
    LibraryEntry {
        status,
        ..Default::default()
    }
}

pub(crate) fn collection(id: &str, name: &str, default: Option<EntryStatus>) -> Collection {
    Collection {
        id: id.to_owned(),
        name: name.to_owned(),
        default_status: default,
    }
}

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

/// Every collection create that reached the effects, as `(op, user, name)`.
pub(crate) fn collection_creates(effects: &[Effect]) -> Vec<(u64, String, String)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::CreateCollection { op, user_id, name } => {
                Some((*op, user_id.clone(), name.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Selects `id`, answers "not in the library" and opens the dialog with these collections.
pub(crate) fn open_with(machine: &mut LibraryMachine, ids: &[&str], collections: Vec<Collection>) {
    let effects = select_all(machine, ids);
    for read in reads(&effects) {
        machine.handle(Input::EntryRead {
            op: read.0,
            game_id: read.1,
            result: Ok(None),
        });
    }
    let effects = machine.handle(Input::OpenAdd);
    let (op, _) = list_ops(&effects)[0].clone();
    machine.handle(Input::CollectionsListed {
        op,
        result: Ok(collections),
    });
}

pub(crate) fn dialog(machine: &mut LibraryMachine) -> AddDialogView {
    machine
        .view()
        .unwrap()
        .add_dialog
        .expect("the dialog is open")
}

#[test]
fn selecting_a_matched_entry_reads_its_standing_once() {
    let mut machine = library();
    let effects = select(&mut machine, "620");
    assert_eq!(reads(&effects).len(), 1);
    assert_eq!(reads(&effects)[0].1, 7);

    let view = machine.view().unwrap();
    assert_eq!(
        view.detail.unwrap().membership,
        Membership::Checking,
        "nothing is assumed while the read is out"
    );
    assert_eq!(
        view.actions.add_to_gamenode,
        crate::library::Availability::Disabled(Reason::CheckingMembership)
    );

    // Selecting the same entry again does not read again.
    assert!(reads(&select(&mut machine, "620")).is_empty());
    // Another entry does, and coming back to the first reads it again: it may have changed.
    assert_eq!(reads(&select(&mut machine, "500")).len(), 1);
    assert_eq!(reads(&select(&mut machine, "620")).len(), 1);
}

#[test]
fn entries_without_a_match_and_multi_selections_read_nothing() {
    let mut machine = library();
    assert!(reads(&select(&mut machine, "570")).is_empty());
    select(&mut machine, "620");
    let effects = machine.handle(Input::Select {
        key: entry_key("steam", "500"),
        mode: SelectMode::Toggle,
    });
    assert!(
        reads(&effects).is_empty(),
        "two games selected: {effects:?}"
    );
}

#[test]
fn nothing_is_read_without_a_verified_session_until_there_is_one() {
    let mut machine = library();
    for access in [Access::Unverified, Access::Offline] {
        machine.handle(Input::SetAccess { access, now: NOW });
        assert!(reads(&select(&mut machine, "620")).is_empty());
    }
    let effects = machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    assert_eq!(reads(&effects).len(), 1, "read as soon as it can be");
}

#[test]
fn an_existing_detected_game_can_be_edited_with_saved_choices() {
    let mut machine = library();
    let effects = select(&mut machine, "620");
    let existing = LibraryEntry {
        id: "e1".into(),
        status: Some(EntryStatus::Finished),
        collection_ids: vec!["c".into()],
        platform_ids: vec![6, 48],
        finished_at: Some("2026-01-01T00:00:00Z".into()),
    };
    answer_read(&mut machine, &effects, Ok(Some(existing.clone())));
    assert!(machine.actions().add_to_gamenode.is_available());
    let effects = machine.handle(Input::OpenAdd);
    assert!(creates(&effects).is_empty());
    let (op, _) = list_ops(&effects)[0].clone();
    machine.handle(Input::CollectionsListed {
        op,
        result: Ok(vec![collection("c", "Favorites", None)]),
    });
    let view = dialog(&mut machine);
    assert!(view.editing_existing && view.can_confirm);
    assert_eq!(view.status, EntryStatus::Finished);
    let CollectionsView::Loaded(choices) = view.collections else {
        panic!()
    };
    assert!(choices[0].picked);
    machine.handle(Input::SetAddStatus(EntryStatus::Planned));
    machine.handle(Input::ToggleAddCollection("c".into()));
    let effects = machine.handle(Input::ConfirmAdd);
    let updates = answer_read(&mut machine, &effects, Ok(Some(existing.clone())));
    let [
        Effect::UpdateEntry {
            op,
            game_id,
            status,
            collection_ids,
            existing: saved,
        },
    ] = updates.as_slice()
    else {
        panic!("{updates:?}")
    };
    assert_eq!((*game_id, *status), (7, EntryStatus::Planned));
    assert!(collection_ids.is_empty());
    assert_eq!(saved, &existing);
    let (op, game_id) = (*op, *game_id);
    machine.handle(Input::EntryUpdated {
        op,
        game_id,
        result: Err(ApiError::Failed),
    });
    let view = dialog(&mut machine);
    assert_eq!(view.status, EntryStatus::Planned);
    assert!(view.can_confirm);
    let effects = machine.handle(Input::ConfirmAdd);
    let updates = answer_read(&mut machine, &effects, Ok(Some(existing)));
    let [Effect::UpdateEntry { op, game_id, .. }] = updates.as_slice() else {
        panic!()
    };
    machine.handle(Input::EntryUpdated {
        op: *op,
        game_id: *game_id,
        result: Ok(()),
    });
    assert_eq!(
        machine.membership(7),
        Membership::InLibrary(Some(EntryStatus::Planned))
    );
    machine.handle(Input::CloseAdd);
    assert_eq!(
        machine.installed_entries().count(),
        5,
        "editing does not import GameNode games"
    );
    let view = machine.view().unwrap();
    assert!(
        view.add_summary.is_some_and(|summary| !summary.undoable),
        "an edit has nothing to undo"
    );
}

#[test]
fn an_unreadable_standing_disables_add_with_a_reason_and_can_be_retried() {
    let mut machine = library();
    let effects = select(&mut machine, "620");
    answer_read(&mut machine, &effects, Err(ApiError::Failed));

    let view = machine.view().unwrap();
    assert_eq!(view.detail.unwrap().membership, Membership::Unavailable);
    assert_eq!(
        view.actions.add_to_gamenode,
        crate::library::Availability::Disabled(Reason::MembershipUnavailable)
    );
    assert!(machine.handle(Input::OpenAdd).is_empty(), "nothing is sent");

    // An unrelated change does not retry in a loop...
    assert!(reads(&machine.handle(Input::SetSearch(String::new()))).is_empty());
    // ...the user asks.
    let effects = machine.handle(Input::RefreshMembership);
    assert_eq!(reads(&effects).len(), 1);
    answer_read(&mut machine, &effects, Ok(None));
    assert!(
        machine
            .view()
            .unwrap()
            .actions
            .add_to_gamenode
            .is_available()
    );
}

#[test]
fn a_stale_standing_is_ignored() {
    let mut machine = library();
    let first = select(&mut machine, "620");
    select(&mut machine, "500");
    let second = select(&mut machine, "620");

    // The answer to the first read of game 7 arrives after the second was sent.
    answer_read(&mut machine, &first, Ok(Some(entry(None))));
    assert_eq!(machine.membership(7), Membership::Checking);
    answer_read(&mut machine, &second, Ok(None));
    assert_eq!(machine.membership(7), Membership::NotInLibrary);
}

#[test]
fn opening_the_dialog_lists_collections_and_sends_nothing_else() {
    let mut machine = library();
    let effects = select(&mut machine, "620");
    answer_read(&mut machine, &effects, Ok(None));

    let effects = machine.handle(Input::OpenAdd);
    assert_eq!(effects.len(), 1, "{effects:?}");
    let (op, user) = list_ops(&effects)[0].clone();
    assert_eq!(user, "a");

    let view = dialog(&mut machine);
    assert_eq!(view.collections, CollectionsView::Loading);
    assert!(!view.can_confirm, "wait for the collections");
    assert_eq!(view.targets.len(), 1);
    assert_eq!(view.targets[0].title, "Portal 2");
    assert_eq!(view.status, EntryStatus::Playing);

    machine.handle(Input::CollectionsListed {
        op,
        result: Ok(vec![collection("c1", "Favorites", None)]),
    });
    let view = dialog(&mut machine);
    assert!(view.can_confirm);
    assert_eq!(
        view.collections,
        CollectionsView::Loaded(vec![CollectionOption {
            id: "c1".to_owned(),
            name: "Favorites".to_owned(),
            default_status: None,
            picked: false,
            entry_count: None,
        }])
    );

    // Closing sends nothing.
    assert!(machine.handle(Input::CloseAdd).is_empty());
    assert!(machine.view().unwrap().add_dialog.is_none());
}

#[test]
fn a_collections_default_status_is_shown_as_the_one_that_applies() {
    let mut machine = library();
    open_with(
        &mut machine,
        &["620"],
        vec![
            collection("plain", "Plain", None),
            collection("backlog", "Backlog", Some(EntryStatus::Planned)),
        ],
    );

    let view = dialog(&mut machine);
    assert_eq!(
        (view.applied_status, view.applied_from),
        (EntryStatus::Playing, None)
    );

    machine.handle(Input::SetAddStatus(EntryStatus::Dropped));
    machine.handle(Input::ToggleAddCollection("plain".to_owned()));
    let view = dialog(&mut machine);
    assert_eq!(
        view.applied_status,
        EntryStatus::Dropped,
        "no default: the picker applies"
    );

    machine.handle(Input::ToggleAddCollection("backlog".to_owned()));
    let view = dialog(&mut machine);
    assert_eq!(
        view.status,
        EntryStatus::Dropped,
        "the picker keeps what the user chose"
    );
    assert_eq!(view.applied_status, EntryStatus::Planned);
    assert_eq!(view.applied_from.as_deref(), Some("Backlog"));

    machine.handle(Input::ToggleAddCollection("backlog".to_owned()));
    let view = dialog(&mut machine);
    assert_eq!(
        (view.applied_status, view.applied_from),
        (EntryStatus::Dropped, None)
    );

    // Unknown collections cannot be picked.
    machine.handle(Input::ToggleAddCollection("nope".to_owned()));
    let CollectionsView::Loaded(list) = dialog(&mut machine).collections else {
        panic!("loaded");
    };
    assert!(list.iter().filter(|option| option.picked).count() == 1);
}

#[test]
fn the_first_picked_collection_with_a_default_decides() {
    let mut machine = library();
    open_with(
        &mut machine,
        &["620"],
        vec![
            collection("a", "A", Some(EntryStatus::Finished)),
            collection("b", "B", Some(EntryStatus::Planned)),
        ],
    );
    machine.handle(Input::ToggleAddCollection("b".to_owned()));
    machine.handle(Input::ToggleAddCollection("a".to_owned()));
    let view = dialog(&mut machine);
    assert_eq!(view.applied_status, EntryStatus::Finished);
    assert_eq!(view.applied_from.as_deref(), Some("A"));
}

#[test]
fn collections_that_cannot_be_read_can_be_retried_and_keep_the_choices() {
    let mut machine = library();
    let effects = select(&mut machine, "620");
    answer_read(&mut machine, &effects, Ok(None));
    let effects = machine.handle(Input::OpenAdd);
    let (op, _) = list_ops(&effects)[0].clone();
    machine.handle(Input::SetAddStatus(EntryStatus::Finished));
    machine.handle(Input::CollectionsListed {
        op,
        result: Err(ApiError::Failed),
    });

    let view = dialog(&mut machine);
    assert_eq!(view.collections, CollectionsView::Failed(ApiError::Failed));
    assert_eq!(
        view.status,
        EntryStatus::Finished,
        "the choice is still there"
    );

    let effects = machine.handle(Input::RetryCollections);
    let (retry, _) = list_ops(&effects)[0].clone();
    assert_ne!(retry, op);
    // The answer to the first request is stale now.
    machine.handle(Input::CollectionsListed {
        op,
        result: Ok(vec![]),
    });
    assert_eq!(dialog(&mut machine).collections, CollectionsView::Loading);
    machine.handle(Input::CollectionsListed {
        op: retry,
        result: Ok(vec![collection("c", "C", None)]),
    });
    assert!(matches!(
        dialog(&mut machine).collections,
        CollectionsView::Loaded(_)
    ));
}

#[test]
fn the_dialog_lists_every_selected_game_and_flags_the_unmatched_ones() {
    let mut machine = library();
    open_with(&mut machine, &["620", "570"], vec![]);
    let view = dialog(&mut machine);
    let listed: Vec<(&str, bool)> = view
        .targets
        .iter()
        .map(|target| (target.title.as_str(), target.matched))
        .collect();
    assert_eq!(listed, [("Dota 2", false), ("Portal 2", true)]);
    assert!(view.can_confirm, "one matched game is enough");
}

#[test]
fn leaving_the_verified_state_blocks_confirming_but_keeps_the_dialog() {
    let mut machine = library();
    open_with(&mut machine, &["620"], vec![]);
    machine.handle(Input::SetAccess {
        access: Access::Offline,
        now: NOW,
    });
    let view = dialog(&mut machine);
    assert_eq!(view.blocked, Some(Reason::Offline));
    assert!(!view.can_confirm);
    assert!(machine.handle(Input::ConfirmAdd).is_empty());
}

#[test]
fn a_different_account_closes_the_dialog() {
    let mut machine = library();
    open_with(&mut machine, &["620"], vec![]);
    machine.handle(Input::AccountChanged(None));
    assert!(machine.add.is_none());
}

#[test]
fn edit_waits_for_complete_collections_and_cancel_sends_no_write() {
    let mut machine = library();
    let effects = select(&mut machine, "620");
    answer_read(
        &mut machine,
        &effects,
        Ok(Some(LibraryEntry {
            status: Some(EntryStatus::Finished),
            collection_ids: vec!["c".into()],
            platform_ids: vec![6],
            ..Default::default()
        })),
    );
    let effects = machine.handle(Input::OpenAdd);
    let (op, _) = list_ops(&effects)[0].clone();
    machine.handle(Input::CollectionsListed {
        op,
        result: Ok(vec![]),
    });
    assert!(
        !dialog(&mut machine).can_confirm,
        "missing saved collection must not be silently removed"
    );
    assert!(machine.handle(Input::ConfirmAdd).is_empty());
    let retry = machine.handle(Input::RetryCollections);
    let (op, _) = list_ops(&retry)[0].clone();
    machine.handle(Input::CollectionsListed {
        op,
        result: Ok(vec![collection("c", "Favorites", None)]),
    });
    assert!(dialog(&mut machine).can_confirm);
    machine.handle(Input::ToggleAddCollection("c".into()));
    assert!(machine.handle(Input::CloseAdd).is_empty());
    assert_eq!(
        machine.membership(7),
        Membership::InLibrary(Some(EntryStatus::Finished))
    );
}

#[test]
fn collection_counts_come_from_the_complete_account_library_read() {
    let mut machine = library();
    machine.account_library = Some(AccountLibrary {
        entries: BTreeMap::from([
            (
                7,
                account_entry(7, "e7", Some(EntryStatus::Finished), &["backlog"]),
            ),
            (
                8,
                account_entry(8, "e8", Some(EntryStatus::Playing), &["backlog", "coop"]),
            ),
            (
                9,
                account_entry(9, "e9", Some(EntryStatus::Planned), &["coop"]),
            ),
        ]),
        collections: vec![
            collection("backlog", "Backlog", None),
            collection("coop", "Coop", None),
        ],
        read_at: NOW,
    });
    // Celeste (10) is not in the cached read, so opening the dialog does not rewrite it.
    open_with(
        &mut machine,
        &["504230"],
        vec![
            collection("backlog", "Backlog", None),
            collection("coop", "Coop", None),
            collection("empty", "Empty", None),
        ],
    );
    let CollectionsView::Loaded(list) = dialog(&mut machine).collections else {
        panic!("loaded")
    };
    assert_eq!(
        list.iter()
            .map(|collection| (collection.id.as_str(), collection.entry_count))
            .collect::<Vec<_>>(),
        [("backlog", Some(2)), ("coop", Some(2)), ("empty", Some(0))],
        "a complete read counts entries; a collection with none is zero"
    );
}

#[test]
fn collection_counts_stay_unknown_without_a_complete_read() {
    let mut machine = library();
    assert!(machine.account_library.is_none());
    open_with(
        &mut machine,
        &["620"],
        vec![
            collection("backlog", "Backlog", None),
            collection("empty", "Empty", None),
        ],
    );
    let CollectionsView::Loaded(list) = dialog(&mut machine).collections else {
        panic!("loaded")
    };
    assert!(
        list.iter()
            .all(|collection| collection.entry_count.is_none()),
        "unknown counts are never shown as zero"
    );
}

#[test]
fn creating_a_collection_picks_the_new_id_and_adds_no_entry() {
    let mut machine = library();
    open_with(
        &mut machine,
        &["620"],
        vec![collection("backlog", "Backlog", None)],
    );
    machine.handle(Input::SetAddStatus(EntryStatus::Finished));
    machine.handle(Input::ToggleAddCollection("backlog".to_owned()));

    assert!(machine.handle(Input::StartNewCollection).is_empty());
    machine.handle(Input::SetNewCollectionName("Coop no sofá".to_owned()));
    let view = dialog(&mut machine);
    let new = view.new_collection.expect("the inline field is open");
    assert_eq!(new.name, "Coop no sofá");
    assert!(new.can_create && !new.creating && new.error.is_none());

    let effects = machine.handle(Input::CreateCollection);
    let sent = collection_creates(&effects);
    let [(op, user, name)] = sent.as_slice() else {
        panic!("{effects:?}");
    };
    let (op, user, name) = (*op, user.as_str(), name.as_str());
    assert_eq!((user, name), ("a", "Coop no sofá"));
    let view = dialog(&mut machine);
    let new = view.new_collection.expect("kept while creating");
    assert!(new.creating && !new.can_create);
    assert!(creates(&effects).is_empty(), "no entry is written");

    let effects = machine.handle(Input::CollectionCreated { op, result: Ok(()) });
    let (list_op, _) = list_ops(&effects)[0].clone();
    assert!(creates(&effects).is_empty());
    machine.handle(Input::CollectionsListed {
        op: list_op,
        result: Ok(vec![
            collection("backlog", "Backlog", None),
            collection("coop", "Coop no sofá", None),
        ]),
    });

    let view = dialog(&mut machine);
    assert!(view.new_collection.is_none(), "the field closes");
    let CollectionsView::Loaded(list) = view.collections else {
        panic!("loaded")
    };
    let picked: Vec<&str> = list
        .iter()
        .filter(|collection| collection.picked)
        .map(|collection| collection.id.as_str())
        .collect();
    assert_eq!(picked, ["backlog", "coop"], "the new collection is checked");
    assert_eq!(
        (view.status, view.applied_status),
        (EntryStatus::Finished, EntryStatus::Finished),
        "the chosen status is unchanged"
    );
    assert!(
        machine.account_entries.is_empty(),
        "creating a collection never writes a library entry"
    );
    assert_eq!(machine.membership(7), Membership::NotInLibrary);
    assert!(machine.last_add.is_none(), "nothing was added yet");
}

#[test]
fn two_same_name_collections_pick_the_new_one() {
    let mut machine = library();
    open_with(
        &mut machine,
        &["620"],
        vec![collection("coop-old", "Coop", None)],
    );
    machine.handle(Input::StartNewCollection);
    machine.handle(Input::SetNewCollectionName("Coop".to_owned()));
    let effects = machine.handle(Input::CreateCollection);
    let (op, _, _) = collection_creates(&effects)[0].clone();
    let effects = machine.handle(Input::CollectionCreated { op, result: Ok(()) });
    let (list_op, _) = list_ops(&effects)[0].clone();
    machine.handle(Input::CollectionsListed {
        op: list_op,
        result: Ok(vec![
            collection("coop-old", "Coop", None),
            collection("coop-new", "Coop", None),
        ]),
    });

    let CollectionsView::Loaded(list) = dialog(&mut machine).collections else {
        panic!("loaded")
    };
    let picked: Vec<&str> = list
        .iter()
        .filter(|collection| collection.picked)
        .map(|collection| collection.id.as_str())
        .collect();
    assert_eq!(
        picked,
        ["coop-new"],
        "the id that appeared is picked, not the older homonym"
    );
}

#[test]
fn the_newest_same_name_is_used_when_no_new_id_appears() {
    let mut machine = library();
    open_with(
        &mut machine,
        &["620"],
        vec![
            collection("coop-a", "Coop", None),
            collection("coop-b", "Coop", None),
        ],
    );
    machine.handle(Input::StartNewCollection);
    machine.handle(Input::SetNewCollectionName("Coop".to_owned()));
    let effects = machine.handle(Input::CreateCollection);
    let (op, _, _) = collection_creates(&effects)[0].clone();
    let effects = machine.handle(Input::CollectionCreated { op, result: Ok(()) });
    let (list_op, _) = list_ops(&effects)[0].clone();
    // The re-list shows no id that was not there before: the fallback picks the newest homonym.
    machine.handle(Input::CollectionsListed {
        op: list_op,
        result: Ok(vec![
            collection("coop-a", "Coop", None),
            collection("coop-b", "Coop", None),
        ]),
    });
    let CollectionsView::Loaded(list) = dialog(&mut machine).collections else {
        panic!("loaded")
    };
    let picked: Vec<&str> = list
        .iter()
        .filter(|collection| collection.picked)
        .map(|collection| collection.id.as_str())
        .collect();
    assert_eq!(picked, ["coop-b"]);
}

#[test]
fn a_failed_create_keeps_the_name_and_every_choice() {
    for (result, expected) in [
        (CreateCollectionError::Failed, NewCollectionError::Failed),
        (
            CreateCollectionError::Rejected,
            NewCollectionError::Rejected,
        ),
        (
            CreateCollectionError::LimitReached,
            NewCollectionError::LimitReached,
        ),
    ] {
        let mut machine = library();
        open_with(
            &mut machine,
            &["620"],
            vec![
                collection("backlog", "Backlog", None),
                collection("coop", "Coop", None),
            ],
        );
        machine.handle(Input::SetAddStatus(EntryStatus::Finished));
        machine.handle(Input::ToggleAddCollection("coop".to_owned()));
        machine.handle(Input::StartNewCollection);
        machine.handle(Input::SetNewCollectionName("Coop no sofá".to_owned()));
        let effects = machine.handle(Input::CreateCollection);
        let (op, _, _) = collection_creates(&effects)[0].clone();
        assert!(
            machine
                .handle(Input::CollectionCreated {
                    op,
                    result: Err(result)
                })
                .is_empty(),
            "{result:?}"
        );

        let view = dialog(&mut machine);
        let new = view.new_collection.expect("the field stays open");
        assert_eq!(new.name, "Coop no sofá", "{result:?}: the name is kept");
        assert_eq!(new.error, Some(expected), "{result:?}");
        assert!(
            !new.creating && new.can_create,
            "{result:?}: retry is possible"
        );
        assert_eq!(view.status, EntryStatus::Finished);
        let CollectionsView::Loaded(list) = view.collections else {
            panic!("the choices are kept")
        };
        let picked: Vec<&str> = list
            .iter()
            .filter(|collection| collection.picked)
            .map(|collection| collection.id.as_str())
            .collect();
        assert_eq!(picked, ["coop"], "{result:?}");
        assert!(machine.last_add.is_none());
    }
}

#[test]
fn an_offline_session_reports_it_without_sending_the_create() {
    let mut machine = library();
    open_with(&mut machine, &["620"], vec![]);
    machine.handle(Input::SetAccess {
        access: Access::Offline,
        now: NOW,
    });
    machine.handle(Input::StartNewCollection);
    machine.handle(Input::SetNewCollectionName("Coop".to_owned()));
    assert!(collection_creates(&machine.handle(Input::CreateCollection)).is_empty());
    let view = dialog(&mut machine);
    let new = view.new_collection.expect("the field stays open");
    assert_eq!(new.error, Some(NewCollectionError::Offline));
    assert_eq!(new.name, "Coop");
}

#[test]
fn a_blank_name_cannot_be_created() {
    let mut machine = library();
    open_with(&mut machine, &["620"], vec![]);
    machine.handle(Input::StartNewCollection);
    machine.handle(Input::SetNewCollectionName("   ".to_owned()));
    let view = dialog(&mut machine);
    let new = view.new_collection.expect("the field is open");
    assert!(!new.can_create);
    assert!(collection_creates(&machine.handle(Input::CreateCollection)).is_empty());
    // The initial blank state is disabled too.
    machine.handle(Input::CancelNewCollection);
    machine.handle(Input::StartNewCollection);
    assert!(!dialog(&mut machine).new_collection.unwrap().can_create);
}

#[test]
fn cancelling_the_new_collection_hides_it_and_sends_nothing() {
    let mut machine = library();
    open_with(&mut machine, &["620"], vec![]);
    machine.handle(Input::StartNewCollection);
    machine.handle(Input::SetNewCollectionName("Coop".to_owned()));
    assert!(machine.handle(Input::CancelNewCollection).is_empty());
    assert!(dialog(&mut machine).new_collection.is_none());
    assert!(collection_creates(&machine.handle(Input::CreateCollection)).is_empty());
}

#[test]
fn a_create_that_cannot_be_relisted_keeps_the_name_for_a_retry() {
    let mut machine = library();
    open_with(
        &mut machine,
        &["620"],
        vec![collection("coop", "Coop", None)],
    );
    machine.handle(Input::StartNewCollection);
    machine.handle(Input::SetNewCollectionName("Novo".to_owned()));
    let effects = machine.handle(Input::CreateCollection);
    let (op, _, _) = collection_creates(&effects)[0].clone();
    let effects = machine.handle(Input::CollectionCreated { op, result: Ok(()) });
    let (list_op, _) = list_ops(&effects)[0].clone();
    machine.handle(Input::CollectionsListed {
        op: list_op,
        result: Err(ApiError::Failed),
    });
    let view = dialog(&mut machine);
    let new = view.new_collection.expect("the name is kept");
    assert_eq!(
        (new.name.as_str(), new.error),
        ("Novo", Some(NewCollectionError::Failed))
    );
    assert!(new.creating, "the re-list is still owed");

    let effects = machine.handle(Input::RetryCollections);
    let (retry, _) = list_ops(&effects)[0].clone();
    machine.handle(Input::CollectionsListed {
        op: retry,
        result: Ok(vec![
            collection("coop", "Coop", None),
            collection("novo", "Novo", None),
        ]),
    });
    let view = dialog(&mut machine);
    assert!(view.new_collection.is_none());
    let CollectionsView::Loaded(list) = view.collections else {
        panic!("loaded")
    };
    assert!(
        list.iter()
            .any(|collection| collection.id == "novo" && collection.picked)
    );
}

#[test]
fn a_drm_free_custom_cover_wins_in_the_add_dialog_header() {
    let mut machine = library();
    machine.games.get_mut(&7).unwrap().cover_file = Some("7.jpg".into());
    machine.local_covers.insert(
        "steam:620".into(),
        "local-local-1-0123456789abcdef.webp".into(),
    );
    let effects = select(&mut machine, "620");
    answer_read(&mut machine, &effects, Ok(None));
    let effects = machine.handle(Input::OpenAdd);
    let (op, _) = list_ops(&effects)[0].clone();
    machine.handle(Input::CollectionsListed {
        op,
        result: Ok(Vec::new()),
    });
    assert_eq!(
        dialog(&mut machine).targets[0].cover_file.as_deref(),
        Some("local-local-1-0123456789abcdef.webp")
    );

    machine.local_covers.clear();
    machine.handle(Input::ToggleAddCollection("noop".into()));
    assert_eq!(
        dialog(&mut machine).targets[0].cover_file.as_deref(),
        Some("7.jpg")
    );
}
