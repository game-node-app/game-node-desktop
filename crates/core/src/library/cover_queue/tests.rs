use super::*;
use crate::library::machine::tests::{loaded, record, stored};
use crate::library::matching::MatchStatus;
use crate::library::store::{AutoMatch, GameRecord, LibraryCache};
use crate::library::{Input, SelectMode, entry_key};

const NOW: i64 = 10_000;

fn good_url(id: u64) -> Option<String> {
    Some(format!(
        "//images.igdb.com/igdb/image/upload/t_thumb/co{id}.jpg"
    ))
}

fn game(id: u64, cover_url: Option<String>) -> GameRecord {
    GameRecord {
        game_id: id,
        name: format!("Game {id}"),
        release_year: None,
        cover_url,
        cover_file: None,
        cover_failed_at: None,
    }
}

/// An unverified library of Steam games `100..100+n`, each automatically matched to the game
/// with the same number plus 1000 (so entry `100` is game `1100`).
fn library(n: u64, tweak: impl FnOnce(&mut Vec<GameRecord>)) -> LibraryMachine {
    let mut cache = LibraryCache::default();
    let mut games = Vec::new();
    let mut records = Vec::new();
    for i in 0..n {
        let id = (100 + i).to_string();
        cache
            .entries
            .push(stored("steam", &id, &format!("G{id}"), true));
        cache.auto_matches.push(AutoMatch {
            category: 1,
            uid: id.clone(),
            game_ids: vec![1000 + 100 + i],
            checked_at: NOW,
        });
        games.push(game(1100 + i, good_url(1100 + i)));
        records.push(record(&id, &format!("G{id}"), "/g"));
    }
    tweak(&mut games);
    cache.games = games;
    let (mut machine, _) = loaded(false, cache);
    let op = machine.scan.as_ref().unwrap().op;
    machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(records),
        now: NOW,
    });
    machine
}

fn verify(machine: &mut LibraryMachine) -> Vec<Effect> {
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    })
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

fn done(machine: &mut LibraryMachine, game_id: u64, ok: bool, now: i64) -> Vec<Effect> {
    machine.handle(Input::CoverDone {
        game_id,
        result: if ok {
            Ok(format!("{game_id}.jpg"))
        } else {
            Err(())
        },
        now,
    })
}

#[test]
fn only_four_covers_download_at_a_time_and_the_rest_follow_as_they_finish() {
    let mut machine = library(6, |_| {});
    let first = fetches(&verify(&mut machine));
    assert_eq!(first, [1100, 1101, 1102, 1103]);

    let next = done(&mut machine, 1101, true, NOW);
    assert_eq!(fetches(&next), [1104]);
    // A failure frees a slot too.
    let next = done(&mut machine, 1100, false, NOW);
    assert_eq!(fetches(&next), [1105]);
    // Nothing waits any more; finishing the rest starts nothing.
    for id in [1102, 1103, 1104, 1105] {
        assert!(fetches(&done(&mut machine, id, true, NOW)).is_empty());
    }
}

#[test]
fn the_download_url_is_the_checked_large_cover() {
    let mut machine = library(1, |_| {});
    let effects = verify(&mut machine);
    let Effect::FetchCover { url, .. } = &effects[0] else {
        panic!("{effects:?}")
    };
    assert_eq!(
        url.as_url().as_str(),
        "https://images.igdb.com/igdb/image/upload/t_cover_big/co1100.jpg"
    );
}

#[test]
fn nothing_downloads_without_a_verified_session() {
    for access in [Access::Unverified, Access::Offline] {
        let mut machine = library(2, |_| {});
        let effects = machine.handle(Input::SetAccess { access, now: NOW });
        assert!(fetches(&effects).is_empty(), "{access:?}");
    }
}

#[test]
fn a_finished_download_sets_the_cover_and_asks_to_store_it() {
    let mut machine = library(1, |_| {});
    verify(&mut machine);
    let effects = done(&mut machine, 1100, true, NOW + 1);
    assert_eq!(
        effects,
        [Effect::PersistCoverReady {
            game_id: 1100,
            file: "1100.jpg".to_owned()
        }]
    );
    let view = machine.view().unwrap();
    assert_eq!(view.rows[0].cover_file.as_deref(), Some("1100.jpg"));
    // It is not downloaded again.
    assert!(fetches(&verify(&mut machine)).is_empty());
}

fn rescan(machine: &mut LibraryMachine, now: i64) -> Vec<Effect> {
    machine.handle(Input::ScanRequested);
    let op = machine.scan.as_ref().unwrap().op;
    machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![record("100", "G100", "/g")]),
        now,
    })
}

#[test]
fn a_cover_that_failed_a_moment_ago_is_not_retried_by_the_next_scan() {
    let mut machine = library(1, |_| {});
    verify(&mut machine);
    done(&mut machine, 1100, false, NOW + 1);
    assert!(fetches(&rescan(&mut machine, NOW + 60)).is_empty());
    // A failure that keeps happening keeps waiting from its latest attempt.
    assert!(fetches(&rescan(&mut machine, NOW + COVER_RETRY_AFTER_SECS)).is_empty());
    assert_eq!(
        fetches(&rescan(&mut machine, NOW + 1 + COVER_RETRY_AFTER_SECS)),
        [1100]
    );
}

#[test]
fn a_game_is_queued_once_while_it_waits() {
    let mut machine = library(6, |_| {});
    let fetched = fetches(&verify(&mut machine));
    assert_eq!(fetched.len(), MAX_COVER_DOWNLOADS);
    // Planning again while two covers still wait does not queue them twice.
    machine.plan_covers(None);
    machine.plan_covers(None);
    let mut total = fetched;
    for game_id in total.clone() {
        total.extend(fetches(&done(&mut machine, game_id, true, NOW + 1)));
    }
    assert_eq!(total.len(), 6, "each cover is fetched once: {total:?}");
}

#[test]
fn a_failed_download_waits_for_the_next_scan_to_be_retried() {
    let mut machine = library(1, |_| {});
    verify(&mut machine);
    let effects = done(&mut machine, 1100, false, NOW + 1);
    assert_eq!(
        effects,
        [Effect::PersistCoverFailed {
            game_id: 1100,
            at: NOW + 1
        }]
    );
    assert_eq!(machine.game(1100).unwrap().cover_failed_at, Some(NOW + 1));
    assert!(machine.view().unwrap().rows[0].cover_file.is_none());

    // Verifying again or a lookup does not retry it.
    assert!(fetches(&verify(&mut machine)).is_empty());

    // The next scan does, once the retry delay has passed.
    let later = NOW + 1 + COVER_RETRY_AFTER_SECS;
    let effects = rescan(&mut machine, later);
    assert_eq!(fetches(&effects), [1100]);
    let effects = done(&mut machine, 1100, true, later + 1);
    assert_eq!(machine.game(1100).unwrap().cover_failed_at, None);
    assert!(matches!(
        effects.as_slice(),
        [Effect::PersistCoverReady { .. }]
    ));
}

#[test]
fn a_failure_that_persisted_from_an_earlier_run_is_retried_after_a_scan() {
    let mut machine = library(1, |games| games[0].cover_failed_at = Some(1));
    assert!(fetches(&verify(&mut machine)).is_empty(), "not on verify");
    machine.handle(Input::ScanRequested);
    let op = machine.scan.as_ref().unwrap().op;
    let effects = machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![record("100", "G100", "/g")]),
        now: NOW + 1,
    });
    assert_eq!(fetches(&effects), [1100]);
}

#[test]
fn covers_that_cannot_be_trusted_or_do_not_exist_are_never_requested() {
    let mut machine = library(4, |games| {
        games[0].cover_url = None;
        games[1].cover_url =
            Some("http://images.igdb.com/igdb/image/upload/t_thumb/co1.jpg".to_owned());
        games[2].cover_url = Some("https://evil.test/igdb/image/upload/t_thumb/co1.jpg".to_owned());
    });
    assert_eq!(fetches(&verify(&mut machine)), [1103]);
}

#[test]
fn unmatched_games_and_uninstalled_games_get_no_cover() {
    let mut cache = LibraryCache::default();
    cache.entries.push(stored("steam", "1", "Matched", true));
    cache.entries.push(stored("steam", "2", "Unmatched", true));
    cache.entries.push(stored("steam", "3", "Gone", false));
    for (uid, ids) in [("1", vec![11]), ("2", vec![]), ("3", vec![13])] {
        cache.auto_matches.push(AutoMatch {
            category: 1,
            uid: uid.to_owned(),
            game_ids: ids,
            checked_at: NOW,
        });
    }
    cache.games = vec![game(11, good_url(11)), game(13, good_url(13))];
    let (mut machine, _) = loaded(false, cache);
    assert_eq!(fetches(&verify(&mut machine)), [11]);
}

#[test]
fn an_answer_nobody_asked_for_is_ignored() {
    let mut machine = library(1, |_| {});
    verify(&mut machine);
    assert!(done(&mut machine, 4242, true, NOW).is_empty());
    assert!(machine.game(1100).unwrap().cover_file.is_none());
    // And a second answer for the same download does nothing.
    done(&mut machine, 1100, true, NOW);
    assert!(done(&mut machine, 1100, false, NOW).is_empty());
    assert!(machine.game(1100).unwrap().cover_file.is_some());
}

#[test]
fn changing_the_match_points_the_entry_at_the_other_games_cover() {
    let mut machine = library(1, |games| games.push(game(2000, good_url(2000))));
    verify(&mut machine);
    done(&mut machine, 1100, true, NOW);
    let key = entry_key("steam", "100");
    assert_eq!(
        machine.view().unwrap().rows[0].cover_file.as_deref(),
        Some("1100.jpg")
    );

    // The user picks game 2000 from a search; its cover is downloaded to its own file.
    machine.handle(Input::Select {
        key: key.clone(),
        mode: SelectMode::Replace,
    });
    machine.handle(Input::OpenMatch);
    let op = match machine.handle(Input::SearchMatch).as_slice() {
        [Effect::SearchGames { op, .. }] => *op,
        other => panic!("{other:?}"),
    };
    let searched = machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![crate::game_api::GameSummary {
            id: 2000,
            name: "Game 2000".to_owned(),
            release_year: None,
            cover_url: good_url(2000),
        }]),
    });
    assert_eq!(
        fetches(&searched),
        [2000],
        "the search result cover is downloaded before it is picked"
    );
    let effects = machine.handle(Input::PickMatch {
        game_id: 2000,
        now: NOW + 5,
    });
    assert!(
        fetches(&effects).is_empty(),
        "the search result cover was already requested: {effects:?}"
    );
    assert_eq!(
        machine.match_status(machine.entry(&key).unwrap()),
        MatchStatus::Matched {
            game_id: 2000,
            source: crate::library::MatchSource::Chosen
        }
    );
    assert!(
        machine.view().unwrap().rows[0].cover_file.is_none(),
        "no stale cover of the old game is shown meanwhile"
    );
    done(&mut machine, 2000, true, NOW + 6);
    assert_eq!(
        machine.view().unwrap().rows[0].cover_file.as_deref(),
        Some("2000.jpg")
    );
    // The old game's file is untouched.
    assert_eq!(
        machine.game(1100).unwrap().cover_file.as_deref(),
        Some("1100.jpg")
    );
}

#[test]
fn losing_the_account_drops_pending_downloads() {
    let mut machine = library(6, |_| {});
    verify(&mut machine);
    machine.handle(Input::AccountChanged(None));
    assert!(done(&mut machine, 1100, true, NOW).is_empty());
}

#[test]
fn installed_covers_are_queued_before_owned_and_shared_covers() {
    use crate::library::machine::tests::stored_with;

    let mut cache = LibraryCache::default();
    // Key order would start with the owned entry; installed still comes first.
    cache
        .entries
        .push(stored_with("steam", "1", "Owned", false, true, false));
    cache
        .entries
        .push(stored_with("steam", "2", "Installed", true, false, false));
    cache
        .entries
        .push(stored_with("steam", "3", "Shared", false, false, true));
    for (uid, id) in [("1", 11), ("2", 12), ("3", 13)] {
        cache.auto_matches.push(AutoMatch {
            category: 1,
            uid: uid.to_owned(),
            game_ids: vec![id],
            checked_at: NOW,
        });
    }
    cache.games = vec![
        game(11, good_url(11)),
        game(12, good_url(12)),
        game(13, good_url(13)),
    ];
    let (mut machine, _) = loaded(false, cache);
    assert_eq!(fetches(&verify(&mut machine)), [12, 11, 13]);
}
