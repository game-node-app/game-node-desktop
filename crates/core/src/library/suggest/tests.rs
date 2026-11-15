use super::*;
use crate::game_api::SearchPage;
use crate::library::machine::tests::{finish_scan, loaded, record, stored};
use crate::library::machine::{Access, Input};
use crate::library::selection::SelectMode;
use crate::library::store::{AutoMatch, ChosenMatch, GameRecord, LibraryCache, StoredEntry};

fn summary(id: u64, name: &str, year: Option<i32>) -> GameSummary {
    GameSummary {
        id,
        name: name.to_owned(),
        release_year: year,
        cover_url: Some(format!(
            "//images.igdb.com/igdb/image/upload/t_thumb/co{id}.jpg"
        )),
    }
}

fn close(left: f32, right: f32) -> bool {
    (left - right).abs() < 0.0001
}

#[test]
fn normalization_strips_separators_and_punctuation_and_collapses_spaces() {
    assert_eq!(normalize_title("Hollow_Knight"), "hollow knight");
    assert_eq!(normalize_title("  Hollow--Knight. "), "hollow knight");
    assert_eq!(normalize_title("Hollow__Knight"), "hollow knight");
    assert_eq!(
        normalize_title("Hollow.Knight - 2017"),
        "hollow knight 2017"
    );
    assert_eq!(normalize_title("***"), "");
}

#[test]
fn exact_underscore_separated_and_partial_titles_score_as_designed() {
    assert!(close(title_score("Hollow Knight", "Hollow Knight"), 1.0));
    assert!(close(title_score("hollow_knight", "Hollow Knight"), 1.0));
    assert!(close(title_score("Hollow-Knight.", "Hollow Knight"), 1.0));
    assert!(title_score("Hollow Knight", "Hollow") > 0.4);
    assert!(title_score("Hollow Knight", "Zzzzz Qqqq") < 0.1);
    assert_eq!(title_score("", ""), 0.0);
}

#[test]
fn confidence_applies_the_title_weight_year_bonus_and_cap() {
    assert!(close(confidence(1.0, false), 0.9));
    assert!(close(confidence(1.0, true), 1.0));
    assert!(close(confidence(0.5, false), 0.45));
    assert!(close(confidence(1.0, true), 1.0));
    assert_eq!(year_in("Hollow_Knight_2017"), Some(2017));
    assert_eq!(year_in("/games/Hollow Knight (2017)/game.exe"), Some(2017));
    assert_eq!(year_in("Portal 2"), None);
    assert_eq!(year_in("12345"), None);
    assert_eq!(
        years_in("/games/2020/Hollow_Knight_2017/game.exe"),
        [2020, 2017]
    );
}

#[test]
fn tones_follow_the_confidence_thresholds() {
    assert_eq!(tone(0.9), SuggestionTone::Success);
    assert_eq!(tone(0.8), SuggestionTone::Success);
    assert_eq!(tone(0.6), SuggestionTone::Warning);
    assert_eq!(tone(0.5), SuggestionTone::Warning);
    assert_eq!(tone(0.4), SuggestionTone::Neutral);
}

#[test]
fn ranking_keeps_the_best_two_at_or_above_forty_percent() {
    let games = [
        summary(1, "Hollow Knight", Some(2017)),
        summary(2, "Hollow Knight", Some(2017)),
        summary(3, "Hollow Knight 2", Some(2022)),
        summary(4, "Totally Different", Some(2010)),
    ];
    let ranked = rank_suggestions("Hollow Knight", &[2017], &games);
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].confidence, 100);
    assert!(ranked[0].confidence >= ranked[1].confidence);
    assert!(ranked.iter().all(|item| item.confidence >= 40));
    assert!(ranked.iter().all(|item| item.game_id != 4));
}

fn machine_with(entries: Vec<StoredEntry>, cache: LibraryCache) -> LibraryMachine {
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, entries.iter().map(record_of).collect(), 10);
    machine
}

fn record_of(entry: &StoredEntry) -> crate::provider::Record {
    record(
        &entry.store_id,
        entry.name.as_deref().unwrap_or(""),
        &entry.install_dir,
    )
}

fn verify(machine: &mut LibraryMachine) {
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: 10,
    });
}

fn select(machine: &mut LibraryMachine, key: &str) -> Vec<Effect> {
    machine.handle(Input::Select {
        key: key.to_owned(),
        mode: SelectMode::Replace,
    })
}

fn suggestion_request(effects: &[Effect]) -> Option<(u64, String, String)> {
    effects.iter().find_map(|effect| match effect {
        Effect::SuggestMatches { op, key, query } => Some((*op, key.clone(), query.clone())),
        _ => None,
    })
}

fn unmatched_machine() -> LibraryMachine {
    let mut entry = stored("steam", "123", "hollow_knight", true);
    entry.install_dir = "/games/Hollow_Knight_2017".to_owned();
    let cache = LibraryCache {
        entries: vec![entry.clone()],
        ..LibraryCache::default()
    };
    let mut machine = machine_with(vec![entry], cache);
    verify(&mut machine);
    machine
}

#[test]
fn selecting_an_unmatched_entry_requests_a_search_with_its_detected_title() {
    let mut machine = unmatched_machine();
    let effects = select(&mut machine, "steam:123");
    let (_, key, query) = suggestion_request(&effects).expect("a suggestion search");
    assert_eq!(key, "steam:123");
    assert_eq!(query, "hollow_knight");

    // Nothing is requested twice while the first answer is pending.
    assert!(suggestion_request(&select(&mut machine, "steam:123")).is_none());
}

#[test]
fn found_suggestions_are_stored_scored_and_shown_in_the_detail() {
    let mut machine = unmatched_machine();
    let effects = select(&mut machine, "steam:123");
    let (op, key, _) = suggestion_request(&effects).unwrap();
    let page = SearchPage {
        games: vec![
            summary(7, "Hollow Knight", Some(2017)),
            summary(8, "Hollow Knight 2", Some(2022)),
            summary(9, "Hollow Knight: Silksong", None),
        ],
        complete: Some(true),
    };
    let effects = machine.handle(Input::SuggestionsFound {
        op,
        key,
        result: Ok(page),
    });
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::PersistMatches { .. }))
    );
    let detail = machine.view().unwrap().detail.unwrap();
    assert_eq!(detail.suggestions.len(), 2);
    assert_eq!(detail.suggestions[0].game_id, 7);
    assert_eq!(detail.suggestions[0].confidence, 100);
    assert_eq!(detail.suggestions[0].tone, SuggestionTone::Success);
    assert_eq!(detail.suggestions[0].year, Some(2017));
    assert_eq!(
        detail.suggestions[0].cover_url.as_deref(),
        Some("//images.igdb.com/igdb/image/upload/t_thumb/co7.jpg")
    );
}

#[test]
fn a_stale_answer_for_another_selection_is_ignored() {
    let first = stored("steam", "123", "hollow_knight", true);
    let second = stored("steam", "456", "celeste", true);
    let cache = LibraryCache {
        entries: vec![first.clone(), second.clone()],
        ..LibraryCache::default()
    };
    let mut machine = machine_with(vec![first.clone(), second], cache);
    verify(&mut machine);
    let effects = select(&mut machine, "steam:123");
    let (op, key, _) = suggestion_request(&effects).unwrap();

    select(&mut machine, "steam:456");
    let stale = machine.handle(Input::SuggestionsFound {
        op,
        key: key.clone(),
        result: Ok(SearchPage {
            games: vec![summary(7, "Hollow Knight", Some(2017))],
            complete: None,
        }),
    });
    assert!(stale.is_empty());
    assert!(
        machine
            .view()
            .unwrap()
            .detail
            .unwrap()
            .suggestions
            .is_empty()
    );

    // Selecting the first entry again asks again: the stale answer was not cached.
    let effects = select(&mut machine, "steam:123");
    assert!(suggestion_request(&effects).is_some());
}

#[test]
fn a_stale_operation_is_ignored() {
    let mut machine = unmatched_machine();
    let effects = select(&mut machine, "steam:123");
    let (op, key, _) = suggestion_request(&effects).unwrap();
    let effects = machine.handle(Input::SuggestionsFound {
        op: op + 1,
        key,
        result: Ok(SearchPage {
            games: vec![summary(7, "Hollow Knight", Some(2017))],
            complete: None,
        }),
    });
    assert!(effects.is_empty());
    assert!(
        machine
            .view()
            .unwrap()
            .detail
            .unwrap()
            .suggestions
            .is_empty()
    );
}

#[test]
fn an_answer_from_a_previous_account_is_ignored() {
    let mut machine = unmatched_machine();
    let effects = select(&mut machine, "steam:123");
    let (op, key, _) = suggestion_request(&effects).unwrap();
    machine.handle(Input::AccountChanged(Some(
        crate::library::machine::tests::account("b"),
    )));
    let effects = machine.handle(Input::SuggestionsFound {
        op,
        key,
        result: Ok(SearchPage {
            games: vec![summary(7, "Hollow Knight", Some(2017))],
            complete: None,
        }),
    });
    assert!(effects.is_empty());
    assert!(machine.suggestions.is_empty());
    assert!(machine.suggestion_requests.is_empty());
}

#[test]
fn using_a_suggestion_persists_the_chosen_match_and_fetches_its_cover() {
    let mut machine = unmatched_machine();
    let effects = select(&mut machine, "steam:123");
    let (op, key, _) = suggestion_request(&effects).unwrap();
    machine.handle(Input::SuggestionsFound {
        op,
        key: key.clone(),
        result: Ok(SearchPage {
            games: vec![summary(7, "Hollow Knight", Some(2017))],
            complete: None,
        }),
    });

    let effects = machine.handle(Input::UseSuggestion {
        key: key.clone(),
        game_id: 7,
        now: 20,
    });
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::PersistChoice { choice, .. } if choice.game_id == Some(7)
    )));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::FetchCover { game_id: 7, .. }))
    );
    assert_eq!(
        machine.chosen_match(&key).and_then(|choice| choice.game_id),
        Some(7)
    );
    let detail = machine.view().unwrap().detail.unwrap();
    assert_eq!(
        detail.row.status,
        MatchStatus::Matched {
            game_id: 7,
            source: crate::library::MatchSource::Chosen,
        }
    );
    assert!(detail.suggestions.is_empty());
}

#[test]
fn marking_no_game_without_the_dialog_clears_the_loaded_and_pending_suggestions() {
    let mut machine = unmatched_machine();
    let effects = select(&mut machine, "steam:123");
    let (op, key, _) = suggestion_request(&effects).unwrap();
    machine.handle(Input::SuggestionsFound {
        op,
        key: key.clone(),
        result: Ok(SearchPage {
            games: vec![summary(7, "Hollow Knight", Some(2017))],
            complete: None,
        }),
    });
    assert!(
        machine
            .suggestions
            .get(&key)
            .is_some_and(|suggestions| !suggestions.is_empty())
    );

    let effects = machine.handle(Input::MarkEntryNoGame {
        key: key.clone(),
        now: 20,
    });
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::PersistChoice { choice, .. } if choice.game_id.is_none()
    )));
    assert!(!machine.suggestions.contains_key(&key));
    assert!(!machine.suggestion_requests.contains_key(&key));
    let detail = machine.view().unwrap().detail.unwrap();
    assert_eq!(detail.row.status, MatchStatus::NoGame);
    assert!(detail.suggestions.is_empty());
}

#[test]
fn a_late_suggestion_answer_after_marking_no_game_directly_changes_nothing() {
    let mut machine = unmatched_machine();
    let effects = select(&mut machine, "steam:123");
    let (op, key, _) = suggestion_request(&effects).unwrap();
    machine.handle(Input::MarkEntryNoGame {
        key: key.clone(),
        now: 20,
    });

    let effects = machine.handle(Input::SuggestionsFound {
        op,
        key: key.clone(),
        result: Ok(SearchPage {
            games: vec![summary(7, "Hollow Knight", Some(2017))],
            complete: None,
        }),
    });
    assert!(effects.is_empty());
    assert!(!machine.suggestions.contains_key(&key));
    let detail = machine.view().unwrap().detail.unwrap();
    assert_eq!(detail.row.status, MatchStatus::NoGame);
    assert!(detail.suggestions.is_empty());
}

#[test]
fn a_game_that_was_not_offered_cannot_be_used() {
    let mut machine = unmatched_machine();
    let effects = select(&mut machine, "steam:123");
    let (op, key, _) = suggestion_request(&effects).unwrap();
    machine.handle(Input::SuggestionsFound {
        op,
        key: key.clone(),
        result: Ok(SearchPage {
            games: vec![summary(7, "Hollow Knight", Some(2017))],
            complete: None,
        }),
    });
    assert!(
        machine
            .handle(Input::UseSuggestion {
                key,
                game_id: 99,
                now: 20,
            })
            .is_empty()
    );
}

#[test]
fn ambiguous_entries_score_their_candidates_without_a_search() {
    let entry = stored("steam", "123", "hollow knight", true);
    let cache = LibraryCache {
        entries: vec![entry.clone()],
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "123".to_owned(),
            game_ids: vec![7, 9],
            checked_at: 1,
        }],
        games: vec![
            GameRecord {
                game_id: 7,
                name: "Hollow Knight".to_owned(),
                release_year: Some(2017),
                cover_url: None,
                cover_file: None,
                cover_failed_at: None,
            },
            GameRecord {
                game_id: 9,
                name: "Hollow Knight 2".to_owned(),
                release_year: Some(2022),
                cover_url: None,
                cover_file: None,
                cover_failed_at: None,
            },
        ],
        ..LibraryCache::default()
    };
    let mut machine = machine_with(vec![entry], cache);
    verify(&mut machine);
    let effects = select(&mut machine, "steam:123");
    assert!(
        suggestion_request(&effects).is_none(),
        "ambiguous candidates are scored, not searched"
    );
    let detail = machine.view().unwrap().detail.unwrap();
    assert_eq!(detail.suggestions.len(), 2);
    assert_eq!(detail.suggestions[0].game_id, 7);
    assert_eq!(detail.suggestions[0].confidence, 90);

    assert!(
        machine
            .handle(Input::UseSuggestion {
                key: "steam:123".to_owned(),
                game_id: 7,
                now: 20,
            })
            .iter()
            .any(|effect| matches!(effect, Effect::PersistChoice { .. }))
    );
}

#[test]
fn a_no_game_entry_gets_no_suggestions() {
    let entry = stored("steam", "123", "hollow knight", true);
    let cache = LibraryCache {
        entries: vec![entry.clone()],
        chosen_matches: vec![ChosenMatch {
            entry_key: "steam:123".to_owned(),
            game_id: None,
            chosen_at: 1,
            source: crate::library::ChoiceSource::User,
        }],
        ..LibraryCache::default()
    };
    let mut machine = machine_with(vec![entry], cache);
    verify(&mut machine);
    let effects = select(&mut machine, "steam:123");
    assert!(suggestion_request(&effects).is_none());
    assert!(
        machine
            .view()
            .unwrap()
            .detail
            .unwrap()
            .suggestions
            .is_empty()
    );
}

#[test]
fn no_suggestions_are_requested_without_a_verified_session() {
    let entry = stored("steam", "123", "hollow_knight", true);
    let cache = LibraryCache {
        entries: vec![entry.clone()],
        ..LibraryCache::default()
    };
    let mut machine = machine_with(vec![entry], cache);
    assert_ne!(machine.access(), Access::Verified);
    let effects = select(&mut machine, "steam:123");
    assert!(suggestion_request(&effects).is_none());
    assert!(
        machine
            .view()
            .unwrap()
            .detail
            .unwrap()
            .suggestions
            .is_empty()
    );
}

#[test]
fn a_failed_suggestion_request_is_not_retried_on_every_input() {
    let mut machine = unmatched_machine();
    let effects = select(&mut machine, "steam:123");
    let (op, key, _) = suggestion_request(&effects).unwrap();
    machine.handle(Input::SuggestionsFound {
        op,
        key,
        result: Err(ApiError::Failed),
    });
    assert!(
        suggestion_request(&select(&mut machine, "steam:123")).is_none(),
        "the empty answer is cached for the selection's lifetime"
    );
    assert!(
        machine
            .view()
            .unwrap()
            .detail
            .unwrap()
            .suggestions
            .is_empty()
    );
}
