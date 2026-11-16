use super::*;
use crate::library::add_dialog::tests::{
    NOW, answer_read, collection, creates, dialog, entry, library, open_with, reads, select,
};
use crate::library::{Access, AddMode, AddTarget, CollectionsView, Input, Reason};

fn confirm(machine: &mut LibraryMachine) -> Vec<Effect> {
    machine.handle(Input::ConfirmAdd)
}

/// Answers the one read in `effects`.
fn read_says(
    machine: &mut LibraryMachine,
    effects: &[Effect],
    result: Result<Option<LibraryEntry>, ApiError>,
) -> Vec<Effect> {
    assert_eq!(reads(effects).len(), 1, "one game at a time: {effects:?}");
    answer_read(machine, effects, result)
}

fn created(
    machine: &mut LibraryMachine,
    effects: &[Effect],
    result: Result<(), ApiError>,
) -> Vec<Effect> {
    let [Effect::CreateEntry { op, game_id, .. }] = effects else {
        panic!("expected one create: {effects:?}");
    };
    machine.handle(Input::EntryCreated {
        op: *op,
        game_id: *game_id,
        result,
    })
}

/// The entry a create's read-back returns.
fn created_entry(game_id: u64, status: EntryStatus) -> LibraryEntry {
    LibraryEntry {
        id: format!("entry-{game_id}"),
        status: Some(status),
        collection_ids: Vec::new(),
        platform_ids: vec![6],
        finished_at: None,
    }
}

/// Answers the create and the read-back that follows it.
fn create_and_read_back(
    machine: &mut LibraryMachine,
    effects: &[Effect],
    status: EntryStatus,
) -> Vec<Effect> {
    let next = created(machine, effects, Ok(()));
    let [(op, game_id)] = reads(&next)[..] else {
        panic!("the create is read back: {next:?}");
    };
    machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Ok(Some(created_entry(game_id, status))),
    })
}

fn summary(machine: &mut LibraryMachine) -> AddSummaryView {
    dialog(machine).summary.expect("every game was handled")
}

#[test]
fn a_game_added_elsewhere_before_confirm_is_not_sent_again() {
    let mut machine = library();
    open_with(&mut machine, &["620"], vec![]);
    // Selected as absent; meanwhile it was added from the website.
    let effects = confirm(&mut machine);
    assert_eq!(
        reads(&effects),
        [(reads(&effects)[0].0, 7)],
        "checked again"
    );
    let effects = read_says(
        &mut machine,
        &effects,
        Ok(Some(entry(Some(EntryStatus::Finished)))),
    );
    assert!(
        creates(&effects).is_empty() && effects.is_empty(),
        "{effects:?}"
    );

    let summary = summary(&mut machine);
    assert_eq!(
        summary
            .items
            .iter()
            .map(|item| item.state)
            .collect::<Vec<_>>(),
        [AddItemState::AlreadyPresent]
    );
    assert_eq!(summary.items[0].status, Some(EntryStatus::Finished));
    assert!(!summary.undoable, "an existing entry is never undoable");
    assert!(
        machine
            .last_add
            .as_ref()
            .is_some_and(|summary| summary.created.is_empty())
    );
    assert_eq!(
        machine.membership(7),
        Membership::InLibrary(Some(EntryStatus::Finished)),
        "what was learned is kept"
    );
}

#[test]
fn a_mixed_selection_adds_one_game_at_a_time_and_summarizes() {
    let mut machine = library();
    open_with(
        &mut machine,
        &["620", "570", "500", "1145360", "504230"],
        vec![collection("backlog", "Backlog", Some(EntryStatus::Planned))],
    );
    machine.handle(Input::ToggleAddCollection("backlog".to_owned()));

    // Display order: Celeste, Dota 2 (unmatched, skipped), Hades, Left 4 Dead, then Portal 2
    // (already in the library).
    let targets: Vec<String> = dialog(&mut machine)
        .targets
        .iter()
        .map(|target| target.title.clone())
        .collect();
    assert_eq!(
        targets,
        ["Celeste", "Dota 2", "Hades", "Left 4 Dead", "Portal 2"]
    );

    let mut sent_creates = Vec::new();
    let mut effects = confirm(&mut machine);
    for expected_game in [10, 9, 8] {
        assert_eq!(reads(&effects)[0].1, expected_game);
        let progress_before = dialog(&mut machine).progress.unwrap();
        assert_eq!(progress_before.total, 5);
        let next = read_says(&mut machine, &effects, Ok(None));
        let [
            Effect::CreateEntry {
                game_id,
                status,
                collection_ids,
                ..
            },
        ] = next.as_slice()
        else {
            panic!("expected one create: {next:?}");
        };
        assert_eq!(*game_id, expected_game);
        assert_eq!(
            *status,
            EntryStatus::Planned,
            "the collection's default applies"
        );
        assert_eq!(collection_ids, &["backlog".to_owned()]);
        sent_creates.push(*game_id);
        effects = create_and_read_back(&mut machine, &next, EntryStatus::Planned);
    }
    // Portal 2 is checked and found; Dota 2 was skipped without any request.
    assert_eq!(reads(&effects)[0].1, 7);
    let effects = read_says(
        &mut machine,
        &effects,
        Ok(Some(entry(Some(EntryStatus::Playing)))),
    );
    assert!(effects.is_empty());

    assert_eq!(sent_creates, [10, 9, 8]);
    let summary = summary(&mut machine);
    assert_eq!(
        (
            summary.added,
            summary.already_present,
            summary.skipped,
            summary.failed
        ),
        (3, 1, 1, 0)
    );
    let titles: Vec<&str> = summary
        .items
        .iter()
        .map(|item| item.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["Celeste", "Dota 2", "Hades", "Left 4 Dead", "Portal 2"]
    );
    assert!(summary.undoable);
    let created = &machine.last_add.as_ref().unwrap().created;
    assert_eq!(
        created
            .iter()
            .map(|entry| (entry.game_id, entry.entry_id.as_str()))
            .collect::<Vec<_>>(),
        [(10, "entry-10"), (9, "entry-9"), (8, "entry-8")],
        "only the entries this add created carry an id"
    );
    assert_eq!(
        machine.membership(9),
        Membership::InLibrary(Some(EntryStatus::Planned))
    );

    let view = dialog(&mut machine);
    assert_eq!(
        view.progress,
        Some(AddProgress { done: 5, total: 5 }),
        "the finished run shows its final progress"
    );
    assert!(!view.can_confirm);
    machine.handle(Input::CloseAdd);
    let view = machine.view().unwrap();
    assert!(view.add_dialog.is_none());
    assert_eq!(
        view.add_summary.expect("kept after close").run_id,
        summary.run_id,
        "the notification still has its result"
    );
}

/// The window reads flat item states and the counts; the summary must serialize exactly that
/// shape (an externally tagged enum object would render every item as failed).
#[test]
fn the_summary_serializes_flat_states_and_their_counts() {
    let mut machine = library();
    open_with(&mut machine, &["620", "570"], vec![]);
    let effects = confirm(&mut machine);
    let next = read_says(&mut machine, &effects, Ok(None));
    create_and_read_back(&mut machine, &next, EntryStatus::Playing);

    let summary = summary(&mut machine);
    let run = summary.run_id;
    assert_eq!(
        serde_json::to_value(&summary).unwrap(),
        serde_json::json!({
            "runId": run,
            "undoable": true,
            "added": 1,
            "alreadyPresent": 0,
            "skipped": 1,
            "failed": 0,
            "items": [
                {
                    "target": { "row": "steam:570" },
                    "entryKey": "steam:570",
                    "title": "Dota 2",
                    "state": "skipped",
                    "status": null,
                    "error": null
                },
                {
                    "target": { "row": "steam:620" },
                    "entryKey": "steam:620",
                    "title": "Portal 2",
                    "state": "added",
                    "status": "playing",
                    "error": null
                }
            ]
        })
    );
}

#[test]
fn a_catalog_game_target_is_added_by_game_id_and_kept_for_undo() {
    let mut machine = library();
    let effects = machine.open_add_for_game(7);
    let (op, _) = match &effects[0] {
        Effect::ListCollections { op, user_id } => (*op, user_id.clone()),
        other => panic!("{other:?}"),
    };
    machine.handle(Input::CollectionsListed {
        op,
        result: Ok(vec![]),
    });
    let view = dialog(&mut machine);
    assert_eq!(view.mode, AddMode::Catalog);
    assert_eq!(view.targets.len(), 1);
    assert_eq!(view.targets[0].target, AddTarget::Game(7));
    assert!(view.targets[0].entry_key.is_none());
    assert!(view.targets[0].matched);
    assert_eq!(view.targets[0].title, "Portal 2");

    let effects = confirm(&mut machine);
    assert_eq!(reads(&effects), [(reads(&effects)[0].0, 7)]);
    let next = read_says(&mut machine, &effects, Ok(None));
    let effects = create_and_read_back(&mut machine, &next, EntryStatus::Playing);
    assert!(effects.is_empty(), "{effects:?}");

    let summary = summary(&mut machine);
    assert_eq!(
        (summary.added, summary.items[0].target.clone()),
        (1, AddTarget::Game(7))
    );
    assert_eq!(
        machine.last_add.as_ref().unwrap().created,
        [CreatedEntry {
            game_id: 7,
            entry_id: "entry-7".to_owned()
        }]
    );
}

#[test]
fn a_failed_game_does_not_stop_the_others() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620"], vec![]);
    let effects = confirm(&mut machine);
    assert_eq!(reads(&effects)[0].1, 8);
    let next = read_says(&mut machine, &effects, Ok(None));
    let effects = created(&mut machine, &next, Err(ApiError::Failed));
    assert_eq!(reads(&effects)[0].1, 7, "the next game is still handled");
    let next = read_says(&mut machine, &effects, Ok(None));
    create_and_read_back(&mut machine, &next, EntryStatus::Playing);

    let summary = summary(&mut machine);
    assert_eq!((summary.added, summary.failed), (1, 1));
    assert_eq!(summary.items[0].state, AddItemState::Failed);
    assert_eq!(
        machine.last_add.as_ref().unwrap().created,
        [CreatedEntry {
            game_id: 7,
            entry_id: "entry-7".to_owned()
        }]
    );
}

#[test]
fn a_read_that_fails_sends_no_create() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620"], vec![]);
    let effects = confirm(&mut machine);
    let effects = read_says(&mut machine, &effects, Err(ApiError::Failed));
    assert!(creates(&effects).is_empty());
    let next = read_says(&mut machine, &effects, Ok(None));
    create_and_read_back(&mut machine, &next, EntryStatus::Playing);
    assert_eq!(summary(&mut machine).failed, 1);
}

#[test]
fn an_unreadable_read_back_keeps_the_add_but_offers_no_undo() {
    let mut machine = library();
    open_with(&mut machine, &["620"], vec![]);
    let effects = confirm(&mut machine);
    let next = read_says(&mut machine, &effects, Ok(None));
    let next = created(&mut machine, &next, Ok(()));
    let [(op, game_id)] = reads(&next)[..] else {
        panic!("the create is read back: {next:?}");
    };
    machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Err(ApiError::Failed),
    });

    let summary = summary(&mut machine);
    assert_eq!(summary.added, 1);
    assert!(!summary.undoable);
    assert_eq!(
        machine.membership(7),
        Membership::InLibrary(Some(EntryStatus::Playing)),
        "the create succeeded even though its id could not be read"
    );
}

#[test]
fn a_failed_single_add_keeps_the_dialog_and_choices_for_a_retry() {
    let mut machine = library();
    open_with(
        &mut machine,
        &["620"],
        vec![collection("c", "C", None), collection("d", "D", None)],
    );
    machine.handle(Input::SetAddStatus(EntryStatus::Finished));
    machine.handle(Input::ToggleAddCollection("d".to_owned()));

    let effects = confirm(&mut machine);
    let next = read_says(&mut machine, &effects, Ok(None));
    assert!(creates(&created(&mut machine, &next, Err(ApiError::Failed))).is_empty());

    let view = dialog(&mut machine);
    assert_eq!(view.error, Some(ApiError::Failed));
    assert!(view.summary.is_none() && view.progress.is_none());
    assert_eq!(view.status, EntryStatus::Finished);
    let CollectionsView::Loaded(list) = &view.collections else {
        panic!("collections kept");
    };
    assert_eq!(
        list.iter()
            .filter(|c| c.picked)
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["d"]
    );
    assert!(view.can_confirm, "the user can try again");

    // Retry: the standing is read again, so a game that got in meanwhile is not overwritten.
    let effects = confirm(&mut machine);
    let next = read_says(&mut machine, &effects, Ok(None));
    let [
        Effect::CreateEntry {
            status,
            collection_ids,
            ..
        },
    ] = next.as_slice()
    else {
        panic!("{next:?}");
    };
    assert_eq!(
        (*status, collection_ids.as_slice()),
        (EntryStatus::Finished, ["d".to_owned()].as_slice())
    );
    create_and_read_back(&mut machine, &next, EntryStatus::Finished);
    assert_eq!(summary(&mut machine).added, 1);
    assert_eq!(dialog(&mut machine).error, None);
}

#[test]
fn an_edit_success_never_enters_the_undo_list() {
    let mut machine = library();
    let selected = select(&mut machine, "620");
    let existing = LibraryEntry {
        id: "entry-7".into(),
        status: Some(EntryStatus::Finished),
        collection_ids: vec!["backlog".into()],
        platform_ids: vec![6],
        finished_at: Some("2026-01-01T00:00:00Z".into()),
    };
    answer_read(&mut machine, &selected, Ok(Some(existing.clone())));
    let effects = machine.handle(Input::OpenAdd);
    let (op, _) = match &effects[0] {
        Effect::ListCollections { op, user_id } => (*op, user_id.clone()),
        other => panic!("{other:?}"),
    };
    machine.handle(Input::CollectionsListed {
        op,
        result: Ok(vec![collection("backlog", "Backlog", None)]),
    });
    let effects = confirm(&mut machine);
    let [(read_op, game_id)] = reads(&effects)[..] else {
        panic!("{effects:?}");
    };
    let effects = machine.handle(Input::EntryRead {
        op: read_op,
        game_id,
        result: Ok(Some(existing)),
    });
    let [Effect::UpdateEntry { op, game_id, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    machine.handle(Input::EntryUpdated {
        op: *op,
        game_id: *game_id,
        result: Ok(()),
    });

    let view = machine.view().unwrap();
    let summary = view.add_summary.expect("the edit completed");
    assert_eq!(summary.added, 1);
    assert!(!summary.undoable, "an edit is never undone by Undo add");
    assert!(machine.last_add.as_ref().unwrap().created.is_empty());
}

#[test]
fn a_rejected_session_ends_the_run() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620", "504230"], vec![]);
    let effects = confirm(&mut machine);
    let effects = read_says(&mut machine, &effects, Err(ApiError::Rejected));
    assert!(effects.is_empty(), "no more requests: {effects:?}");
    let summary = summary(&mut machine);
    assert_eq!(summary.failed, 3);
}

#[test]
fn closing_a_single_game_run_abandons_it_and_ignores_the_answer() {
    let mut machine = library();
    open_with(&mut machine, &["620"], vec![]);
    let effects = confirm(&mut machine);
    let (op, game_id) = reads(&effects)[0];
    let closed = machine.handle(Input::CloseAdd);
    assert!(
        closed
            .iter()
            .all(|e| !matches!(e, Effect::CreateEntry { .. }))
    );

    // The answer to the read in flight arrives: nothing is created.
    let late = machine.handle(Input::EntryRead {
        op,
        game_id,
        result: Ok(None),
    });
    assert!(creates(&late).is_empty() && late.is_empty(), "{late:?}");
}

#[test]
fn a_late_standing_read_does_not_undo_a_successful_add() {
    let mut machine = library();
    let selected = select(&mut machine, "620");
    answer_read(&mut machine, &selected, Ok(None));
    // A second read of the same game is out when the add runs.
    machine.handle(Input::RefreshMembership);
    machine.membership_focus = None;
    machine.membership.insert(7, Membership::NotInLibrary);
    let stale = {
        let op = machine.next_op();
        machine.membership_reads.insert(7, op);
        (op, 7)
    };

    let dialog_effects = machine.handle(Input::OpenAdd);
    let (list_op, _) = match &dialog_effects[0] {
        Effect::ListCollections { op, user_id } => (*op, user_id.clone()),
        other => panic!("{other:?}"),
    };
    machine.handle(Input::CollectionsListed {
        op: list_op,
        result: Ok(vec![]),
    });
    let effects = confirm(&mut machine);
    let next = read_says(&mut machine, &effects, Ok(None));
    create_and_read_back(&mut machine, &next, EntryStatus::Playing);

    machine.handle(Input::EntryRead {
        op: stale.0,
        game_id: stale.1,
        result: Ok(None),
    });
    assert_eq!(
        machine.membership(7),
        Membership::InLibrary(Some(EntryStatus::Playing))
    );
}

#[test]
fn nothing_is_sent_while_the_session_is_not_usable() {
    let mut machine = library();
    open_with(&mut machine, &["620"], vec![]);
    machine.handle(Input::SetAccess {
        access: Access::Unverified,
        now: NOW,
    });
    assert!(confirm(&mut machine).is_empty());
    assert_eq!(
        dialog(&mut machine).blocked,
        Some(Reason::NeedsVerifiedSession)
    );
}

#[test]
fn confirming_only_unmatched_games_sends_nothing() {
    let mut machine = library();
    select(&mut machine, "570");
    assert!(machine.handle(Input::OpenAdd).is_empty());
    // Add is not even offered for a single unmatched entry, so the dialog never opens.
    assert!(machine.view().unwrap().add_dialog.is_none());
    assert!(confirm(&mut machine).is_empty());
}

/// Nothing but a confirmed Add dialog may ever produce a create request.
#[test]
fn only_confirming_the_dialog_can_create_an_entry() {
    use crate::game_api::{ExternalMatch, GameSummary};
    use crate::library::machine::tests::{account, loaded, record};
    use crate::library::store::LibraryCache;
    use crate::library::{Direction, SelectMode, entry_key};

    let mut all: Vec<Effect> = Vec::new();
    let (mut machine, effects) = loaded(false, LibraryCache::default());
    all.extend(effects);
    let mut step = |machine: &mut LibraryMachine, input: Input| {
        let effects = machine.handle(input);
        all.extend(effects.iter().cloned());
        effects
    };

    // Sign-in, a verified session, a scan and the lookup it triggers.
    let load = step(&mut machine, Input::AccountChanged(Some(account("b"))));
    let Some(Effect::LoadCache { op, .. }) = load.first() else {
        panic!("{load:?}");
    };
    let scan = step(
        &mut machine,
        Input::Loaded {
            op: *op,
            result: Ok(LibraryCache::default()),
        },
    );
    step(
        &mut machine,
        Input::SetAccess {
            access: Access::Verified,
            now: 1,
        },
    );
    let op = crate::library::machine::tests::scan_ops(&scan)[0].0;
    let lookups = step(
        &mut machine,
        Input::ProviderScanned {
            op,
            provider: "steam".to_owned(),
            result: Ok(vec![
                record("620", "Portal 2", "/g"),
                record("570", "Dota 2", "/g"),
            ]),
            now: 2,
        },
    );
    for effect in &lookups {
        if let Effect::Lookup { op, category, uids } = effect {
            step(
                &mut machine,
                Input::LookupDone {
                    op: *op,
                    category: *category,
                    result: Ok(vec![ExternalMatch {
                        uid: uids[0].clone(),
                        game: GameSummary {
                            id: 7,
                            name: "Portal 2".to_owned(),
                            release_year: None,
                            cover_url: None,
                            platforms: Vec::new(),
                        },
                    }]),
                    now: 3,
                },
            );
        }
    }

    // Selecting, moving, matching, clearing, and opening, editing and closing the Add dialog.
    for key in [entry_key("steam", "620"), entry_key("steam", "570")] {
        let reads = step(
            &mut machine,
            Input::Select {
                key,
                mode: SelectMode::Replace,
            },
        );
        for effect in reads {
            if let Effect::ReadEntry { op, game_id } = effect {
                step(
                    &mut machine,
                    Input::EntryRead {
                        op,
                        game_id,
                        result: Ok(None),
                    },
                );
            }
        }
        step(
            &mut machine,
            Input::Move {
                direction: Direction::Down,
                extend: false,
            },
        );
        let opened = step(&mut machine, Input::OpenMatch);
        for effect in opened {
            if let Effect::SearchGames { op, .. } = effect {
                step(
                    &mut machine,
                    Input::MatchSearched {
                        op,
                        result: Ok(vec![]),
                    },
                );
            }
        }
        step(&mut machine, Input::MarkNoGame { now: 4 });
        step(&mut machine, Input::ClearMatch);
        step(&mut machine, Input::PickMatch { game_id: 7, now: 5 });
        step(&mut machine, Input::CloseMatch);
        let add = step(&mut machine, Input::OpenAdd);
        for effect in add {
            if let Effect::ListCollections { op, .. } = effect {
                step(
                    &mut machine,
                    Input::CollectionsListed {
                        op,
                        result: Ok(vec![]),
                    },
                );
            }
        }
        step(&mut machine, Input::SetAddStatus(EntryStatus::Dropped));
        step(&mut machine, Input::CloseAdd);
    }
    step(&mut machine, Input::AccountChanged(None));

    assert!(
        all.iter()
            .any(|effect| matches!(effect, Effect::ReadEntry { .. })),
        "the flow did exercise reads"
    );
    assert!(creates(&all).is_empty(), "{:?}", creates(&all));
}

fn states(machine: &mut LibraryMachine) -> Vec<AddItemState> {
    dialog(machine)
        .items
        .iter()
        .map(|item| item.state)
        .collect()
}

#[test]
fn a_batch_lists_waiting_adding_skipped_already_and_added() {
    let mut machine = library();
    open_with(&mut machine, &["500", "570", "620"], vec![]);
    // Display order: Dota 2 (unmatched), Left 4 Dead, Portal 2.
    let effects = confirm(&mut machine);
    let view = dialog(&mut machine);
    assert_eq!(view.progress, Some(AddProgress { done: 1, total: 3 }));
    assert_eq!(
        states(&mut machine),
        [
            AddItemState::Skipped,
            AddItemState::Adding,
            AddItemState::Waiting
        ]
    );

    let next = read_says(
        &mut machine,
        &effects,
        Ok(Some(entry(Some(EntryStatus::Finished)))),
    );
    assert_eq!(reads(&next)[0].1, 7, "the next matched game is handled");
    assert_eq!(
        states(&mut machine),
        [
            AddItemState::Skipped,
            AddItemState::AlreadyPresent,
            AddItemState::Adding
        ]
    );

    let next = read_says(&mut machine, &next, Ok(None));
    create_and_read_back(&mut machine, &next, EntryStatus::Playing);
    let view = dialog(&mut machine);
    assert_eq!(
        view.items.iter().map(|item| item.state).collect::<Vec<_>>(),
        [
            AddItemState::Skipped,
            AddItemState::AlreadyPresent,
            AddItemState::Added
        ]
    );
    assert_eq!(
        (view.summary.unwrap().added, view.progress.unwrap().done),
        (1, 3)
    );
}

#[test]
fn hiding_a_batch_keeps_issuing_creates() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620"], vec![]);
    let effects = confirm(&mut machine);
    assert_eq!(reads(&effects)[0].1, 8);

    assert!(machine.handle(Input::HideAdd).is_empty());
    assert!(machine.view().unwrap().add_dialog.is_none());
    assert!(machine.add_run_active());

    let next = read_says(&mut machine, &effects, Ok(None));
    assert_eq!(
        creates(&next).len(),
        1,
        "the hidden run still creates the next game"
    );
    assert!(machine.add_run_active());
}

#[test]
fn a_hidden_batch_is_summarized_once_and_ignores_a_late_answer() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620"], vec![]);
    let effects = confirm(&mut machine);
    let in_flight = reads(&effects)[0];
    machine.handle(Input::HideAdd);

    let next = read_says(&mut machine, &effects, Ok(None));
    let effects = create_and_read_back(&mut machine, &next, EntryStatus::Playing);
    assert_eq!(reads(&effects)[0].1, 7);
    let next = read_says(&mut machine, &effects, Ok(None));
    create_and_read_back(&mut machine, &next, EntryStatus::Playing);

    let run = machine.last_add.as_ref().expect("the run finished").run;
    assert!(machine.add_runs.is_none());
    assert_eq!(
        machine.view().unwrap().add_summary.unwrap().run_id,
        run,
        "the notification has the summary after the dialog hid"
    );
    // The answer to the first read arrives again: the finished run is not resumed or re-raised.
    let late = machine.handle(Input::EntryRead {
        op: in_flight.0,
        game_id: in_flight.1,
        result: Ok(None),
    });
    assert!(late.is_empty() && creates(&late).is_empty(), "{late:?}");
    assert_eq!(machine.last_add.as_ref().unwrap().run, run);
}

#[test]
fn a_new_add_reopens_the_running_progress() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620"], vec![]);
    confirm(&mut machine);
    machine.handle(Input::HideAdd);
    let in_flight = machine.add_runs.as_ref().unwrap().in_flight_game();

    let effects = machine.handle(Input::OpenAdd);
    assert!(effects.is_empty(), "reopening sends nothing: {effects:?}");
    let view = dialog(&mut machine);
    assert_eq!(view.progress, Some(AddProgress { done: 0, total: 2 }));
    assert_eq!(view.targets.len(), 2);
    assert_eq!(view.items.len(), 2);
    assert!(!view.can_confirm, "a running run cannot be confirmed again");
    assert_eq!(
        machine.add_runs.as_ref().unwrap().in_flight_game(),
        in_flight,
        "the same run is shown, not a new one"
    );

    // Hide again: the run keeps going.
    machine.handle(Input::HideAdd);
    assert!(machine.add_run_active());
    assert!(machine.view().unwrap().add_dialog.is_none());
}

#[test]
fn only_one_add_run_can_be_active() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620"], vec![]);
    let first = confirm(&mut machine);
    assert!(confirm(&mut machine).is_empty(), "the run is already going");

    machine.handle(Input::HideAdd);
    assert!(machine.handle(Input::ConfirmAdd).is_empty());
    assert!(machine.handle(Input::OpenAdd).is_empty());
    assert_eq!(dialog(&mut machine).progress.unwrap().total, 2);
    assert!(confirm(&mut machine).is_empty());

    // The original request is still the one that answers.
    let next = read_says(&mut machine, &first, Ok(None));
    assert_eq!(creates(&next).len(), 1);
}

#[test]
fn sign_out_stops_new_effects_and_counts_the_rest() {
    let mut machine = library();
    open_with(&mut machine, &["500", "570", "620"], vec![]);
    let effects = confirm(&mut machine);
    let in_flight = reads(&effects)[0];
    machine.handle(Input::HideAdd);
    // Dota 2 was skipped; Left 4 Dead was being read; Portal 2 was waiting.
    assert!(machine.add_run_active());

    let effects = machine.handle(Input::SetAccess {
        access: Access::Unverified,
        now: NOW,
    });
    assert!(
        effects.iter().all(|effect| !matches!(
            effect,
            Effect::CreateEntry { .. } | Effect::ReadEntry { .. }
        )),
        "no further request is sent: {effects:?}"
    );
    assert!(machine.add_runs.is_none());
    let summary = machine
        .view()
        .unwrap()
        .add_summary
        .expect("the run finished");
    assert_eq!(
        (
            summary.added,
            summary.already_present,
            summary.skipped,
            summary.failed
        ),
        (0, 0, 1, 2),
        "the games that were not handled count as not added"
    );
    let notice = machine
        .add_notice()
        .expect("the stopped batch is reported outside the view");
    assert!(notice.batch);
    assert_eq!(notice.account_id.as_deref(), Some("a"));
    assert_eq!(
        (
            notice.summary.added,
            notice.summary.skipped,
            notice.summary.failed
        ),
        (0, 1, 2)
    );

    // The answer to the read that was in flight is ignored.
    let late = machine.handle(Input::EntryRead {
        op: in_flight.0,
        game_id: in_flight.1,
        result: Ok(None),
    });
    assert!(late.is_empty() && creates(&late).is_empty(), "{late:?}");
}

#[test]
fn a_different_account_stops_the_hidden_run() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620"], vec![]);
    let effects = confirm(&mut machine);
    let in_flight = reads(&effects)[0];
    machine.handle(Input::HideAdd);
    machine.handle(Input::AccountChanged(None));
    assert!(machine.add_runs.is_none());
    assert!(machine.view().is_none(), "the previous view is gone");

    let notice = machine
        .add_notice()
        .expect("the previous account's outcome survives");
    assert!(notice.batch);
    assert_eq!(notice.account_id.as_deref(), Some("a"));
    assert_eq!(
        (notice.summary.added, notice.summary.failed),
        (0, 2),
        "the read in flight and the waiting game count as not added"
    );
    assert_eq!(
        notice.summary.run_id,
        machine.last_add_notice.as_ref().unwrap().summary.run_id
    );

    let late = machine.handle(Input::EntryRead {
        op: in_flight.0,
        game_id: in_flight.1,
        result: Ok(None),
    });
    assert!(late.is_empty() && creates(&late).is_empty(), "{late:?}");
    assert_eq!(
        machine.add_notice().unwrap().summary,
        notice.summary,
        "a stale callback cannot change the reported counts"
    );

    // The same notice keeps its identity when the next account loads; nothing is re-run.
    machine.handle(Input::AccountChanged(Some(crate::library::LastAccount {
        id: "b".to_owned(),
        name: "User b".to_owned(),
    })));
    let switched = machine.add_notice().expect("kept across the switch");
    assert_eq!(switched.account_id.as_deref(), Some("a"));
    assert_eq!(switched.summary.run_id, notice.summary.run_id);
    assert_eq!(machine.account().unwrap().id, "b");
}

#[test]
fn an_all_failed_batch_still_publishes_a_notice() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620"], vec![]);
    let effects = confirm(&mut machine);
    let effects = read_says(&mut machine, &effects, Err(ApiError::Rejected));
    assert!(effects.is_empty(), "no further request: {effects:?}");
    let notice = machine.add_notice().expect("an all-failed batch notifies");
    assert!(notice.batch);
    assert_eq!(
        (
            notice.summary.added,
            notice.summary.already_present,
            notice.summary.skipped,
            notice.summary.failed
        ),
        (0, 0, 0, 2)
    );
    assert!(
        machine.last_add.is_some(),
        "the dialog keeps its summary too"
    );
}

#[test]
fn hiding_a_single_game_run_still_abandons_it() {
    let mut machine = library();
    open_with(&mut machine, &["620"], vec![]);
    confirm(&mut machine);
    machine.handle(Input::HideAdd);
    assert!(machine.add_runs.is_none());
    assert!(machine.last_add.is_none());
}
