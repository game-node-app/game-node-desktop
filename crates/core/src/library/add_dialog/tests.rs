use super::*;
use crate::game_api::LibraryEntry;
use crate::library::machine::tests::{loaded, record, stored};
use crate::library::store::{AutoMatch, GameRecord, LibraryCache};
use crate::library::{Access, Input, Membership, SelectMode, entry_key};

pub(crate) const NOW: i64 = 5_000;

fn game(id: u64, name: &str) -> GameRecord {
    GameRecord {
        game_id: id,
        name: name.to_owned(),
        release_year: None,
        cover_url: None,
        cover_file: None,
        cover_failed_at: None,
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
    LibraryEntry { status }
}

pub(crate) fn collection(id: &str, name: &str, default: Option<EntryStatus>) -> Collection {
    Collection {
        id: id.to_owned(),
        name: name.to_owned(),
        default_status: default,
    }
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
fn a_game_already_in_the_library_cannot_be_added_again() {
    let mut machine = library();
    let effects = select(&mut machine, "620");
    answer_read(
        &mut machine,
        &effects,
        Ok(Some(entry(Some(EntryStatus::Finished)))),
    );

    let view = machine.view().unwrap();
    assert_eq!(
        view.detail.unwrap().membership,
        Membership::InLibrary(Some(EntryStatus::Finished))
    );
    assert_eq!(
        view.actions.add_to_gamenode,
        crate::library::Availability::Disabled(Reason::AlreadyInLibrary(Some(
            EntryStatus::Finished
        )))
    );
    assert!(machine.handle(Input::OpenAdd).is_empty());
    assert!(machine.view().unwrap().add_dialog.is_none());
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
