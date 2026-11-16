use super::*;
use crate::game_api::ExternalMatch;
use crate::library::machine::tests::{loaded, record, stored};
use crate::library::matching::MatchSource;
use crate::library::store::{AutoMatch, GameRecord, LibraryCache};
use crate::library::{Access, Input, SelectMode, entry_key};

const NOW: i64 = 5_000;

fn game(id: u64, name: &str) -> GameSummary {
    GameSummary {
        id,
        name: name.to_owned(),
        release_year: Some(2011),
        cover_url: Some(format!(
            "//images.igdb.com/igdb/image/upload/t_thumb/{id}.jpg"
        )),
        platforms: Vec::new(),
    }
}

fn record_of(id: u64, name: &str) -> GameRecord {
    GameRecord {
        game_id: id,
        name: name.to_owned(),
        release_year: Some(2011),
        cover_url: None,
        cover_file: None,
        cover_failed_at: None,
        platforms: Vec::new(),
    }
}

/// A verified library with Portal 2 (620, automatically matched to game 7), Dota 2 (570,
/// unknown to GameNode) and Half-Life (70, ambiguous between games 20 and 21), Portal 2 selected.
fn library() -> LibraryMachine {
    let mut cache = LibraryCache::default();
    for (id, name) in [("620", "Portal 2"), ("570", "Dota 2"), ("70", "Half-Life")] {
        cache.entries.push(stored("steam", id, name, true));
    }
    for (uid, ids) in [("620", vec![7]), ("570", vec![]), ("70", vec![20, 21])] {
        cache.auto_matches.push(AutoMatch {
            category: 1,
            uid: uid.to_owned(),
            game_ids: ids,
            checked_at: NOW,
        });
    }
    cache.games = vec![
        record_of(7, "Portal 2"),
        record_of(20, "Half-Life"),
        record_of(21, "Half-Life: Source"),
    ];
    let (mut machine, _) = loaded(false, cache);
    let op = machine.scan.as_ref().unwrap().op;
    machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![
            record("620", "Portal 2", "/g"),
            record("570", "Dota 2", "/g"),
            record("70", "Half-Life", "/g"),
        ]),
        now: NOW,
    });
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    machine
}

fn select(machine: &mut LibraryMachine, id: &str) {
    machine.handle(Input::Select {
        key: entry_key("steam", id),
        mode: SelectMode::Replace,
    });
}

fn open(machine: &mut LibraryMachine, id: &str) -> Vec<Effect> {
    select(machine, id);
    machine.handle(Input::OpenMatch)
}

fn searches(effects: &[Effect]) -> Vec<(u64, String)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::SearchGames { op, query } => Some((*op, query.clone())),
            _ => None,
        })
        .collect()
}

fn fetches(effects: &[Effect]) -> Vec<u64> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::FetchCover { game_id, .. } => Some(*game_id),
            _ => None,
        })
        .collect()
}

/// The effects without detail image loads and membership reads, which are not what these
/// tests are about.
fn without_reads(effects: Vec<Effect>) -> Vec<Effect> {
    effects
        .into_iter()
        .filter(|effect| {
            !matches!(
                effect,
                Effect::ReadEntry { .. } | Effect::FetchArtwork { .. }
            )
        })
        .collect()
}

fn dialog(machine: &mut LibraryMachine) -> MatchDialogView {
    machine
        .view()
        .unwrap()
        .match_dialog
        .expect("the dialog is open")
}

fn status(machine: &LibraryMachine, id: &str) -> MatchStatus {
    machine.match_status(machine.entry(&entry_key("steam", id)).unwrap())
}

#[test]
fn opening_prefills_the_detected_title_and_searches_it() {
    let mut machine = library();
    let effects = open(&mut machine, "570");
    let sent = searches(&effects);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "Dota 2");

    let view = dialog(&mut machine);
    assert_eq!(view.query, "Dota 2");
    assert_eq!(view.detected_name, "Dota 2");
    assert_eq!(view.provider_name, "Steam");
    assert!(view.searching && view.results.is_empty() && view.error.is_none());
    assert_eq!(view.current, MatchStatus::Unmatched);
    assert!(!view.can_clear);
}

#[test]
fn search_results_show_title_year_and_cover_and_can_be_picked() {
    let mut machine = library();
    let (op, _) = searches(&open(&mut machine, "570")).remove(0);
    let effects = machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(40, "Dota 2"), game(41, "Dota Underlords")]),
    });
    assert_eq!(
        fetches(&effects),
        [40, 41],
        "the results' covers are downloaded for the dialog"
    );
    let view = dialog(&mut machine);
    assert!(!view.searching && !view.no_results);
    assert_eq!(view.results.len(), 2);
    assert_eq!(view.results[0].name, "Dota 2");
    assert_eq!(view.results[0].release_year, Some(2011));
    assert!(view.results[0].cover_url.is_some());
    assert!(
        view.results[0].cover_file.is_none(),
        "the cover is not on disk yet"
    );

    machine.handle(Input::CoverDone {
        game_id: 40,
        result: Ok("40.jpg".to_owned()),
        now: NOW,
    });
    let view = dialog(&mut machine);
    assert_eq!(view.results[0].cover_file.as_deref(), Some("40.jpg"));
    assert!(view.results[1].cover_file.is_none());

    let effects = without_reads(machine.handle(Input::PickMatch {
        game_id: 40,
        now: NOW + 1,
    }));
    assert_eq!(
        status(&machine, "570"),
        MatchStatus::Matched {
            game_id: 40,
            source: MatchSource::Chosen
        }
    );
    // The game is stored before the choice that refers to it.
    assert!(
        matches!(
            effects.as_slice(),
            [
                Effect::PersistMatches { games, matches },
                Effect::PersistChoice { account_id, choice },
                rest @ ..,
            ] if games[0].game_id == 40 && matches.is_empty()
                && account_id == "a" && choice.game_id == Some(40) && choice.chosen_at == NOW + 1
                && rest.iter().all(|e| matches!(e, Effect::FetchCover { game_id, .. } if *game_id != 40))
        ),
        "{effects:?}"
    );
    assert!(machine.view().unwrap().match_dialog.is_none(), "closes");
    assert_eq!(machine.game(40).unwrap().name, "Dota 2");
}

#[test]
fn the_query_can_be_edited_and_searched_again() {
    let mut machine = library();
    open(&mut machine, "570");
    machine.handle(Input::SetMatchQuery(
        "  Defense of the Ancients ".to_owned(),
    ));
    assert_eq!(dialog(&mut machine).query, "  Defense of the Ancients ");
    let effects = machine.handle(Input::SearchMatch);
    assert_eq!(searches(&effects)[0].1, "Defense of the Ancients");
}

#[test]
fn an_answer_to_an_earlier_search_is_ignored() {
    let mut machine = library();
    let (first, _) = searches(&open(&mut machine, "570")).remove(0);
    machine.handle(Input::SetMatchQuery("dota".to_owned()));
    let (second, _) = searches(&machine.handle(Input::SearchMatch)).remove(0);

    machine.handle(Input::MatchSearched {
        op: first,
        result: Ok(vec![game(1, "Old")]),
    });
    assert!(
        dialog(&mut machine).searching,
        "still waiting for the second"
    );
    machine.handle(Input::MatchSearched {
        op: second,
        result: Ok(vec![game(2, "New")]),
    });
    assert_eq!(dialog(&mut machine).results[0].game_id, 2);
}

#[test]
fn a_blank_query_sends_nothing() {
    let mut machine = library();
    open(&mut machine, "570");
    machine.handle(Input::SetMatchQuery("   ".to_owned()));
    assert!(searches(&machine.handle(Input::SearchMatch)).is_empty());
    assert!(!dialog(&mut machine).searching);
}

#[test]
fn an_empty_search_says_so() {
    let mut machine = library();
    let (op, _) = searches(&open(&mut machine, "570")).remove(0);
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![]),
    });
    assert!(dialog(&mut machine).no_results);
}

#[test]
fn ambiguous_candidates_come_first_and_are_not_repeated_in_the_results() {
    let mut machine = library();
    assert_eq!(
        status(&machine, "70"),
        MatchStatus::Ambiguous {
            candidates: vec![20, 21]
        }
    );
    let (op, query) = searches(&open(&mut machine, "70")).remove(0);
    assert_eq!(query, "Half-Life");
    let view = dialog(&mut machine);
    let names: Vec<_> = view.candidates.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Half-Life", "Half-Life: Source"]);

    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(20, "Half-Life"), game(22, "Half-Life 2")]),
    });
    let view = dialog(&mut machine);
    assert_eq!(view.candidates.len(), 2);
    assert_eq!(
        view.results.iter().map(|r| r.game_id).collect::<Vec<_>>(),
        [22]
    );

    // A candidate is picked without being stored again: it is already known.
    let effects = without_reads(machine.handle(Input::PickMatch {
        game_id: 21,
        now: NOW,
    }));
    assert!(matches!(effects.as_slice(), [Effect::PersistChoice { .. }]));
    assert_eq!(status(&machine, "70").game_id(), Some(21));
}

#[test]
fn replacing_an_automatic_match_uses_the_chosen_game_from_then_on() {
    let mut machine = library();
    let (op, _) = searches(&open(&mut machine, "620")).remove(0);
    let view = dialog(&mut machine);
    assert_eq!(
        view.current,
        MatchStatus::Matched {
            game_id: 7,
            source: MatchSource::Automatic
        }
    );
    assert!(
        !view.can_clear,
        "an automatic match is not a choice to forget"
    );

    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(7, "Portal 2"), game(8, "Portal 2 Mod")]),
    });
    assert!(dialog(&mut machine).results.iter().any(|r| r.current));
    machine.handle(Input::PickMatch {
        game_id: 8,
        now: NOW,
    });
    assert_eq!(
        status(&machine, "620"),
        MatchStatus::Matched {
            game_id: 8,
            source: MatchSource::Chosen
        }
    );

    // The detail pane tells chosen from automatic.
    let detail = machine.view().unwrap().detail.unwrap();
    assert_eq!(
        detail.row.status,
        MatchStatus::Matched {
            game_id: 8,
            source: MatchSource::Chosen
        }
    );
}

#[test]
fn the_choice_survives_a_later_lookup_for_the_same_id() {
    let mut machine = library();
    let (op, _) = searches(&open(&mut machine, "620")).remove(0);
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(8, "Portal 2 Mod")]),
    });
    machine.handle(Input::PickMatch {
        game_id: 8,
        now: NOW,
    });

    let later = NOW + 100_000;
    let effects = machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: later,
    });
    let ask = effects
        .iter()
        .find_map(|e| match e {
            Effect::Lookup { op, category, .. } => Some((*op, *category)),
            _ => None,
        })
        .expect("stale ids are asked again");
    machine.handle(Input::LookupDone {
        op: ask.0,
        category: ask.1,
        result: Ok(vec![ExternalMatch {
            uid: "620".to_owned(),
            game: game(99, "Something Else"),
        }]),
        now: later,
    });
    assert_eq!(status(&machine, "620").game_id(), Some(8));
}

#[test]
fn marking_no_game_is_sticky_and_leaves_the_entry_playable() {
    let mut machine = library();
    open(&mut machine, "620");
    let effects = machine.handle(Input::MarkNoGame { now: NOW });
    assert!(matches!(
        effects.as_slice(),
        [Effect::PersistChoice { choice, .. }] if choice.game_id.is_none()
    ));
    assert_eq!(status(&machine, "620"), MatchStatus::NoGame);
    let view = machine.view().unwrap();
    assert!(view.match_dialog.is_none());
    assert!(view.actions.play.is_available());
    assert!(view.actions.change_match, "it can be changed back");
}

#[test]
fn marking_no_game_without_the_dialog_marks_the_selected_entry_and_persists_it() {
    let mut machine = library();
    select(&mut machine, "570");
    let effects = machine.handle(Input::MarkEntryNoGame {
        key: entry_key("steam", "570"),
        now: NOW,
    });
    assert!(matches!(
        without_reads(effects).as_slice(),
        [Effect::PersistChoice { account_id, choice }]
            if account_id == "a"
                && choice.entry_key == entry_key("steam", "570")
                && choice.game_id.is_none()
                && choice.chosen_at == NOW
    ));
    assert_eq!(status(&machine, "570"), MatchStatus::NoGame);
    // The other entries keep their own status.
    assert_eq!(
        status(&machine, "620"),
        MatchStatus::Matched {
            game_id: 7,
            source: MatchSource::Automatic
        }
    );
    assert_eq!(
        status(&machine, "70"),
        MatchStatus::Ambiguous {
            candidates: vec![20, 21]
        }
    );
    let view = machine.view().unwrap();
    assert!(view.match_dialog.is_none());
    assert!(view.actions.play.is_available());
    assert!(view.actions.change_match, "it can be changed back");
}

#[test]
fn the_direct_no_game_action_ignores_another_selection_or_a_missing_entry() {
    let mut machine = library();
    select(&mut machine, "620");
    let effects = machine.handle(Input::MarkEntryNoGame {
        key: entry_key("steam", "570"),
        now: NOW,
    });
    assert!(without_reads(effects).is_empty());
    assert_eq!(status(&machine, "570"), MatchStatus::Unmatched);
    assert_eq!(
        status(&machine, "620"),
        MatchStatus::Matched {
            game_id: 7,
            source: MatchSource::Automatic
        }
    );

    let effects = machine.handle(Input::MarkEntryNoGame {
        key: entry_key("steam", "999"),
        now: NOW,
    });
    assert!(without_reads(effects).is_empty());
    assert_eq!(status(&machine, "620").game_id(), Some(7));
}

#[test]
fn the_direct_no_game_action_refuses_an_entry_that_is_already_decided() {
    for id in ["620", "70"] {
        let mut machine = library();
        let before = status(&machine, id);
        select(&mut machine, id);
        let effects = machine.handle(Input::MarkEntryNoGame {
            key: entry_key("steam", id),
            now: NOW,
        });
        assert!(without_reads(effects).is_empty(), "{id}");
        assert_eq!(status(&machine, id), before, "{id}");
    }
}

#[test]
fn the_direct_no_game_action_is_dropped_when_the_account_changes() {
    let mut machine = library();
    select(&mut machine, "570");
    machine.handle(Input::AccountChanged(None));
    let effects = machine.handle(Input::MarkEntryNoGame {
        key: entry_key("steam", "570"),
        now: NOW,
    });
    assert!(effects.is_empty());
    assert!(machine.view().is_none());
    assert!(machine.chosen_match(&entry_key("steam", "570")).is_none());
}

#[test]
fn clearing_forgets_only_the_users_choice() {
    let mut machine = library();
    open(&mut machine, "620");
    machine.handle(Input::MarkNoGame { now: NOW });

    open(&mut machine, "620");
    let view = dialog(&mut machine);
    assert!(view.can_clear);
    assert_eq!(view.current, MatchStatus::NoGame);

    let effects = without_reads(machine.handle(Input::ClearMatch));
    assert_eq!(
        effects,
        [Effect::ForgetChoice {
            account_id: "a".to_owned(),
            entry_key: entry_key("steam", "620")
        }]
    );
    // The automatic match applies again.
    assert_eq!(
        status(&machine, "620"),
        MatchStatus::Matched {
            game_id: 7,
            source: MatchSource::Automatic
        }
    );
    assert!(machine.view().unwrap().match_dialog.is_none());
}

#[test]
fn a_failed_search_shows_an_error_and_leaves_the_entry_alone() {
    for (error, failure) in [
        (ApiError::Failed, SearchFailure::Failed),
        (ApiError::Rejected, SearchFailure::Rejected),
    ] {
        let mut machine = library();
        let (op, _) = searches(&open(&mut machine, "70")).remove(0);
        let before = status(&machine, "70");

        let effects = machine.handle(Input::MatchSearched {
            op,
            result: Err(error),
        });
        assert!(effects.is_empty());
        let view = dialog(&mut machine);
        assert_eq!(view.error, Some(failure));
        assert!(!view.searching && view.results.is_empty());
        assert_eq!(view.candidates.len(), 2, "known candidates stay");
        assert_eq!(status(&machine, "70"), before);

        // The user can try again.
        assert_eq!(searches(&machine.handle(Input::SearchMatch)).len(), 1);
    }
}

#[test]
fn closing_changes_nothing() {
    let mut machine = library();
    let (op, _) = searches(&open(&mut machine, "70")).remove(0);
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(30, "Other")]),
    });
    let before = status(&machine, "70");
    assert!(machine.handle(Input::CloseMatch).is_empty());
    assert!(machine.view().unwrap().match_dialog.is_none());
    assert_eq!(status(&machine, "70"), before);
    assert!(machine.chosen_match(&entry_key("steam", "70")).is_none());
}

#[test]
fn a_game_the_dialog_did_not_offer_cannot_be_picked() {
    let mut machine = library();
    open(&mut machine, "570");
    let effects = machine.handle(Input::PickMatch {
        game_id: 12345,
        now: NOW,
    });
    assert!(effects.is_empty());
    assert_eq!(status(&machine, "570"), MatchStatus::Unmatched);
    assert!(machine.view().unwrap().match_dialog.is_some(), "stays open");
}

#[test]
fn picking_the_current_choice_again_writes_nothing() {
    let mut machine = library();
    let (op, _) = searches(&open(&mut machine, "570")).remove(0);
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(40, "Dota 2")]),
    });
    machine.handle(Input::PickMatch {
        game_id: 40,
        now: NOW,
    });

    let (op, _) = searches(&open(&mut machine, "570")).remove(0);
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(40, "Dota 2")]),
    });
    let effects = machine.handle(Input::PickMatch {
        game_id: 40,
        now: NOW + 9,
    });
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::PersistChoice { .. }))
    );
}

#[test]
fn matching_is_unavailable_without_a_verified_session() {
    for (access, reason) in [
        (Access::Unverified, Reason::NeedsVerifiedSession),
        (Access::Offline, Reason::Offline),
    ] {
        let mut machine = library();
        machine.handle(Input::SetAccess { access, now: NOW });
        select(&mut machine, "570");
        assert_eq!(
            machine.view().unwrap().actions.match_game.reason(),
            Some(reason)
        );
        assert!(machine.handle(Input::OpenMatch).is_empty());
        assert!(machine.view().unwrap().match_dialog.is_none());
    }
}

#[test]
fn losing_the_session_while_open_blocks_searching_but_not_local_choices() {
    let mut machine = library();
    open(&mut machine, "570");
    machine.handle(Input::SetAccess {
        access: Access::Offline,
        now: NOW,
    });
    assert!(searches(&machine.handle(Input::SearchMatch)).is_empty());
    let view = dialog(&mut machine);
    assert_eq!(view.error, Some(SearchFailure::Blocked(Reason::Offline)));
    assert_eq!(view.search_blocked, Some(Reason::Offline));

    machine.handle(Input::MarkNoGame { now: NOW });
    assert_eq!(status(&machine, "570"), MatchStatus::NoGame);
}

#[test]
fn match_needs_exactly_one_selected_game() {
    let mut machine = library();
    machine.handle(Input::Select {
        key: entry_key("steam", "620"),
        mode: SelectMode::Replace,
    });
    machine.handle(Input::Select {
        key: entry_key("steam", "570"),
        mode: SelectMode::Toggle,
    });
    assert!(machine.handle(Input::OpenMatch).is_empty());
    assert!(machine.view().unwrap().match_dialog.is_none());
}

#[test]
fn changing_the_account_closes_the_dialog() {
    let mut machine = library();
    open(&mut machine, "570");
    machine.handle(Input::AccountChanged(None));
    assert!(machine.view().is_none());
    assert!(machine.dialog.is_none());
}

/// A verified library whose entry was detected with a year and has two server candidates: the
/// exact game (2024) and a similarly named one from another year (2019). The candidates arrive in
/// reverse confidence order, and the exact one carries platforms.
fn year_library() -> LibraryMachine {
    let mut cache = LibraryCache::default();
    cache
        .entries
        .push(stored("steam", "900", "Hollow Mountain 2024", true));
    cache.auto_matches.push(AutoMatch {
        category: 1,
        uid: "900".to_owned(),
        game_ids: vec![101, 100],
        checked_at: NOW,
    });
    cache.games = vec![
        GameRecord {
            game_id: 100,
            name: "Hollow Mountain".to_owned(),
            release_year: Some(2024),
            cover_url: None,
            cover_file: None,
            cover_failed_at: None,
            platforms: vec!["PC".to_owned(), "Switch".to_owned()],
        },
        GameRecord {
            game_id: 101,
            name: "Hollow Hearth".to_owned(),
            release_year: Some(2019),
            cover_url: None,
            cover_file: None,
            cover_failed_at: None,
            platforms: vec!["PC".to_owned()],
        },
    ];
    let (mut machine, _) = loaded(false, cache);
    let op = machine.scan.as_ref().unwrap().op;
    machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![record("900", "Hollow Mountain 2024", "/g")]),
        now: NOW,
    });
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    machine
}

#[test]
fn opening_selects_the_current_match_before_a_search_answer() {
    let mut machine = library();
    open(&mut machine, "620");
    let view = dialog(&mut machine);
    let selected = view.selected.expect("the current match is preselected");
    assert_eq!(selected.game_id, 7);
    assert!(selected.current);
    assert!(
        !selected.reasons.contains(&MatchReason::ChosenByYou),
        "an automatic match is not a choice by the user"
    );
}

#[test]
fn opening_without_a_current_match_selects_the_most_confident_candidate() {
    let mut machine = library();
    open(&mut machine, "70");
    let view = dialog(&mut machine);
    assert_eq!(
        view.candidates
            .iter()
            .map(|candidate| candidate.game_id)
            .collect::<Vec<_>>(),
        [20, 21],
        "candidates are ordered by confidence"
    );
    assert_eq!(
        view.selected.as_ref().map(|selected| selected.game_id),
        Some(20),
        "the exact title is the best option"
    );
    assert!(
        view.selected
            .as_ref()
            .is_some_and(|selected| !selected.reasons.contains(&MatchReason::SameYear)),
        "no year was detected in Half-Life's name or path"
    );
}

#[test]
fn candidates_are_ordered_by_confidence_and_expose_platforms() {
    let mut machine = year_library();
    open(&mut machine, "900");
    let view = dialog(&mut machine);
    assert_eq!(
        view.candidates
            .iter()
            .map(|candidate| candidate.game_id)
            .collect::<Vec<_>>(),
        [100, 101],
        "the server's order is replaced by confidence order"
    );
    assert_eq!(
        view.selected.as_ref().map(|selected| selected.game_id),
        Some(100)
    );
    assert_eq!(view.candidates[0].platforms, ["PC", "Switch"]);
    assert!(view.candidates[0].confidence > view.candidates[1].confidence);
}

#[test]
fn reasons_appear_only_under_their_conditions() {
    let mut machine = year_library();
    open(&mut machine, "900");

    let exact = dialog(&mut machine).candidates.remove(0);
    assert!(
        matches!(
            exact.reasons.first(),
            Some(MatchReason::TitleSimilarity { percent }) if *percent >= 80
        ),
        "{:?}",
        exact.reasons
    );
    assert!(exact.reasons.contains(&MatchReason::SameYear));
    assert!(exact.reasons.contains(&MatchReason::StoreIdRecognized));
    assert!(
        !exact
            .reasons
            .iter()
            .any(|reason| matches!(reason, MatchReason::DifferentYear { .. }))
    );
    assert!(!exact.reasons.contains(&MatchReason::ChosenByYou));

    let other = dialog(&mut machine).candidates.remove(1);
    assert!(
        other
            .reasons
            .contains(&MatchReason::DifferentYear { year: 2019 }),
        "{:?}",
        other.reasons
    );
    assert!(!other.reasons.contains(&MatchReason::SameYear));
    assert!(
        matches!(
            other.reasons.first(),
            Some(MatchReason::TitleSimilarity { percent }) if *percent < 80
        ),
        "a weak title match is a warning reason"
    );
    assert!(other.reasons.contains(&MatchReason::StoreIdRecognized));

    // Search results did not come from the store id, so they never say it was recognized.
    let (op, _) = searches(&machine.handle(Input::SearchMatch)).remove(0);
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(300, "Something Else")]),
    });
    machine.handle(Input::SelectMatch { game_id: 300 });
    let selected = dialog(&mut machine).selected.unwrap();
    assert!(!selected.reasons.contains(&MatchReason::StoreIdRecognized));
}

#[test]
fn a_detected_year_absent_from_the_entry_adds_no_year_reason() {
    let mut machine = library();
    let (op, _) = searches(&open(&mut machine, "570")).remove(0);
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(40, "Dota 2")]),
    });
    machine.handle(Input::SelectMatch { game_id: 40 });
    let selected = dialog(&mut machine).selected.unwrap();
    assert!(
        !selected.reasons.iter().any(|reason| matches!(
            reason,
            MatchReason::SameYear | MatchReason::DifferentYear { .. }
        )),
        "no year was detected in Dota 2's name or path"
    );
}

#[test]
fn choosing_a_match_and_reopening_shows_chosen_by_you() {
    let mut machine = year_library();
    open(&mut machine, "900");
    machine.handle(Input::SelectMatch { game_id: 100 });
    machine.handle(Input::ConfirmMatch { now: NOW + 1 });
    assert_eq!(status(&machine, "900").game_id(), Some(100));

    open(&mut machine, "900");
    let selected = dialog(&mut machine).selected.unwrap();
    assert_eq!(selected.game_id, 100);
    assert!(selected.current);
    assert!(selected.reasons.contains(&MatchReason::ChosenByYou));
}

#[test]
fn selecting_an_option_does_not_change_the_entry() {
    let mut machine = year_library();
    open(&mut machine, "900");
    let effects = machine.handle(Input::SelectMatch { game_id: 101 });
    assert!(effects.is_empty());
    assert_eq!(status(&machine, "900").game_id(), None);
    assert!(machine.view().unwrap().match_dialog.is_some());
    assert!(machine.chosen_match(&entry_key("steam", "900")).is_none());
}

#[test]
fn confirming_without_a_selection_does_nothing() {
    let mut machine = library();
    open(&mut machine, "570");
    assert!(dialog(&mut machine).selected.is_none());
    assert!(machine.handle(Input::ConfirmMatch { now: NOW }).is_empty());
    assert_eq!(status(&machine, "570"), MatchStatus::Unmatched);
    assert!(
        machine.view().unwrap().match_dialog.is_some(),
        "the dialog stays open"
    );
}

#[test]
fn confirming_matches_the_selected_option() {
    let mut machine = library();
    let (op, _) = searches(&open(&mut machine, "570")).remove(0);
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(40, "Dota 2"), game(41, "Dota Underlords")]),
    });
    // The best option is preselected.
    assert_eq!(
        dialog(&mut machine).selected.as_ref().map(|o| o.game_id),
        Some(40)
    );
    machine.handle(Input::SelectMatch { game_id: 41 });
    let effects = without_reads(machine.handle(Input::ConfirmMatch { now: NOW + 1 }));
    assert_eq!(
        status(&machine, "570"),
        MatchStatus::Matched {
            game_id: 41,
            source: MatchSource::Chosen
        }
    );
    assert!(matches!(
        effects.as_slice(),
        [
            Effect::PersistMatches { games, .. },
            Effect::PersistChoice { choice, .. },
            ..
        ] if games[0].game_id == 41 && choice.game_id == Some(41)
    ));
    assert!(machine.view().unwrap().match_dialog.is_none());
}

#[test]
fn a_users_selection_survives_a_later_search_answer() {
    let mut machine = library();
    let (op, _) = searches(&open(&mut machine, "570")).remove(0);
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(40, "Dota 2"), game(41, "Dota Underlords")]),
    });
    machine.handle(Input::SelectMatch { game_id: 41 });
    assert_eq!(
        dialog(&mut machine).selected.as_ref().map(|o| o.game_id),
        Some(41)
    );

    let (op, _) = searches(&machine.handle(Input::SearchMatch)).remove(0);
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(42, "Dota 2 Beta"), game(43, "Dota 2 Test")]),
    });
    let view = dialog(&mut machine);
    assert_eq!(
        view.selected.as_ref().map(|selected| selected.game_id),
        Some(41),
        "the best new result did not replace the user's choice"
    );
    assert_eq!(view.selected.unwrap().name, "Dota Underlords");
}

#[test]
fn a_selection_the_dialog_does_not_offer_is_ignored() {
    let mut machine = library();
    open(&mut machine, "570");
    assert!(
        machine
            .handle(Input::SelectMatch { game_id: 12345 })
            .is_empty()
    );
    assert!(dialog(&mut machine).selected.is_none());
}
