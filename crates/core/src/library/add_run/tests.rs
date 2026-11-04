use super::*;
use crate::library::add_dialog::tests::{
    NOW, answer_read, collection, creates, dialog, entry, library, open_with, reads, select,
};
use crate::library::{Access, CollectionsView, Input, Reason};

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

fn summary(machine: &mut LibraryMachine) -> AddSummary {
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
            .map(|item| item.outcome)
            .collect::<Vec<_>>(),
        [AddOutcome::AlreadyPresent(Some(EntryStatus::Finished))]
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
        effects = created(&mut machine, &next, Ok(()));
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
            summary.added(),
            summary.already_present(),
            summary.skipped_unmatched(),
            summary.failed()
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
    assert_eq!(
        machine.membership(9),
        Membership::InLibrary(Some(EntryStatus::Planned))
    );

    let view = dialog(&mut machine);
    assert!(view.progress.is_none() && !view.can_confirm);
    machine.handle(Input::CloseAdd);
    assert!(machine.view().unwrap().add_dialog.is_none());
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
    created(&mut machine, &next, Ok(()));

    let summary = summary(&mut machine);
    assert_eq!((summary.added(), summary.failed()), (1, 1));
    assert_eq!(
        summary.items[0].outcome,
        AddOutcome::Failed(ApiError::Failed)
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
    created(&mut machine, &next, Ok(()));
    assert_eq!(summary(&mut machine).failed(), 1);
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
    created(&mut machine, &next, Ok(()));
    assert_eq!(summary(&mut machine).added(), 1);
    assert_eq!(dialog(&mut machine).error, None);
}

#[test]
fn a_rejected_session_ends_the_run() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620", "504230"], vec![]);
    let effects = confirm(&mut machine);
    let effects = read_says(&mut machine, &effects, Err(ApiError::Rejected));
    assert!(effects.is_empty(), "no more requests: {effects:?}");
    let summary = summary(&mut machine);
    assert_eq!(summary.failed(), 3);
}

#[test]
fn closing_during_a_run_stops_it_and_ignores_the_answer() {
    let mut machine = library();
    open_with(&mut machine, &["500", "620"], vec![]);
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
    created(&mut machine, &next, Ok(()));

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
