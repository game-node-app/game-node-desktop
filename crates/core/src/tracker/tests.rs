use super::*;
use std::time::Duration;

fn entry(key: &str, path: &str) -> Entry {
    Entry {
        key: key.into(),
        install_dir: path.into(),
        source: "steam".into(),
        counter: false,
        game_id: None,
        title: key.into(),
    }
}
fn process(pid: u32, created: i64, exe: &str) -> ProcessInfo {
    ProcessInfo {
        pid,
        created_at: UNIX_EPOCH + Duration::from_secs(created as u64),
        exe_path: exe.into(),
    }
}
fn apply(tracker: &mut Tracker, input: Input) -> Vec<Effect> {
    let mut n = 0;
    tracker.update(input, || {
        n += 1;
        format!("session-{n}")
    })
}
fn setup(counter: bool) -> Tracker {
    let mut tracker = Tracker::new(false);
    let mut game = entry("steam:1", "/games/one");
    game.counter = counter;
    apply(
        &mut tracker,
        Input::Library {
            account: Some("a".into()),
            entries: vec![game],
            at: 100,
        },
    );
    tracker
}
fn listed(tracker: &mut Tracker, at: i64, processes: Vec<ProcessInfo>) -> Vec<Effect> {
    apply(tracker, Input::Listed { at, processes })
}
#[test]
fn folders_respect_normalization_case_boundaries_and_depth() {
    let entries = BTreeMap::from([
        ("a".into(), entry("a", "C:/Games/Foo/")),
        ("b".into(), entry("b", "C:/Games/Foo/mod")),
    ]);
    for (exe, windows, expected) in [
        ("c:/games/foo/game.exe", true, Some("a")),
        ("C:/Games//Foo/mod/x.exe", true, Some("b")),
        ("C:/Games/FooBar/x.exe", true, None),
        ("C:/Games/Foo", true, None),
        ("c:/games/foo/x.exe", false, None),
    ] {
        assert_eq!(
            match_entry(&entries, exe, windows).map(|e| e.key.as_str()),
            expected
        );
    }
    let entries = BTreeMap::from([
        ("a".into(), entry("a", "/games/Foo/")),
        ("b".into(), entry("b", "/games/Foo/mod")),
    ]);
    assert_eq!(
        match_entry(&entries, "/games//Foo/mod/x", false)
            .unwrap()
            .key,
        "b"
    );
    assert!(match_entry(&entries, "/games/foo/x", false).is_none());
    assert!(match_entry(&entries, "/games/FooBar/x", false).is_none());
}
#[test]
fn helpers_keep_one_session_and_delayed_exits_use_actual_end() {
    let mut tracker = setup(false);
    let main = process(1, 80, "/games/one/main");
    let helper = process(2, 90, "/games/one/helper");
    let effects = listed(&mut tracker, 100, vec![helper.clone(), main.clone()]);
    assert!(
        matches!(&effects[0], Effect::OpenSession(row) if row.started_at == 80 && row.account_id == "a")
    );
    assert_eq!(
        effects
            .iter()
            .filter(|e| matches!(e, Effect::Watch(_)))
            .count(),
        2
    );
    assert!(listed(&mut tracker, 110, vec![main.clone(), helper.clone()]).is_empty());
    assert!(
        apply(
            &mut tracker,
            Input::Exited {
                process: main,
                at: 120
            }
        )
        .is_empty()
    );
    assert_eq!(tracker.running().len(), 1);
    let end = apply(
        &mut tracker,
        Input::Exited {
            process: helper,
            at: 130,
        },
    );
    assert!(matches!(
        &end[0],
        Effect::CloseSession { ended_at: 130, .. }
    ));
    assert!(tracker.running().is_empty());
}
#[test]
fn missing_process_falls_back_to_listing_and_pid_reuse_is_separate() {
    let mut tracker = setup(false);
    let old = process(1, 80, "/games/one/main");
    listed(&mut tracker, 100, vec![old.clone()]);
    let effects = listed(&mut tracker, 120, vec![process(1, 110, "/elsewhere/app")]);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::CloseSession { ended_at: 120, .. }))
    );
    assert!(tracker.running().is_empty());
    let new = process(1, 140, "/games/one/main");
    listed(&mut tracker, 150, vec![new]);
    assert!(
        apply(
            &mut tracker,
            Input::Exited {
                process: old,
                at: 120
            }
        )
        .is_empty()
    );
    assert_eq!(tracker.running().len(), 1);
}
#[test]
fn counter_games_emit_running_and_one_read_without_sessions() {
    let mut tracker = setup(true);
    let game = process(1, 80, "/games/one/main");
    assert_eq!(
        listed(&mut tracker, 100, vec![game.clone()]),
        vec![Effect::Watch(game.clone())]
    );
    assert_eq!(tracker.running().len(), 1);
    assert!(apply(&mut tracker, Input::Tick { now: 200 }).is_empty());
    assert_eq!(
        apply(
            &mut tracker,
            Input::Exited {
                process: game.clone(),
                at: 210
            }
        ),
        vec![Effect::ReadCounter {
            key: "steam:1".into()
        }]
    );
    assert!(
        apply(
            &mut tracker,
            Input::Exited {
                process: game,
                at: 210
            }
        )
        .is_empty()
    );
}
#[test]
fn checkpoints_split_fifty_hours_contiguously_with_distinct_ids() {
    let mut tracker = setup(false);
    let mut id = 0;
    let mut next = || {
        id += 1;
        format!("uuid-{id}")
    };
    tracker.update(
        Input::Listed {
            at: 100,
            processes: vec![process(1, 80, "/games/one/main")],
        },
        &mut next,
    );
    assert!(
        tracker
            .update(Input::Tick { now: 139 }, &mut next)
            .is_empty()
    );
    assert!(matches!(
        &tracker.update(Input::Tick { now: 140 }, &mut next)[0],
        Effect::Checkpoint { at: 140, .. }
    ));
    let effects = tracker.update(Input::Stop { at: 80 + 50 * 3600 }, &mut next);
    let closes: Vec<_> = effects
        .iter()
        .filter_map(|e| {
            if let Effect::CloseSession { id, ended_at, .. } = e {
                Some((id.as_str(), *ended_at))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        closes,
        [
            ("uuid-1", 80 + DAY),
            ("uuid-2", 80 + DAY * 2),
            ("uuid-3", 80 + 50 * 3600)
        ]
    );
    let opens: Vec<_> = effects
        .iter()
        .filter_map(|e| {
            if let Effect::OpenSession(s) = e {
                Some(s.started_at)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(opens, [80 + DAY, 80 + DAY * 2]);
}
#[test]
fn signout_and_account_switch_close_the_previous_account_and_stop_tracking() {
    for account in [None, Some("b".into())] {
        let mut tracker = setup(false);
        let game = process(1, 80, "/games/one/main");
        listed(&mut tracker, 100, vec![game.clone()]);
        let effects = apply(
            &mut tracker,
            Input::Library {
                account: account.clone(),
                entries: vec![entry("steam:1", "/games/one")],
                at: 120,
            },
        );
        assert!(matches!(
            &effects[0],
            Effect::CloseSession { ended_at: 120, .. }
        ));
        assert!(tracker.running().is_empty());
        let effects = listed(&mut tracker, 130, vec![game]);
        if account.is_none() {
            assert!(effects.is_empty());
        } else {
            assert!(matches!(&effects[0], Effect::OpenSession(row) if row.account_id == "b"));
        }
    }
}
#[test]
fn cached_unverified_accounts_track_and_match_is_captured_at_close() {
    let mut tracker = setup(false); // verification deliberately is not a tracker input
    let game = process(1, 80, "/games/one/main");
    listed(&mut tracker, 100, vec![game.clone()]);
    let mut matched = entry("steam:1", "/games/one");
    matched.game_id = Some(620);
    assert!(
        apply(
            &mut tracker,
            Input::Library {
                account: Some("a".into()),
                entries: vec![matched],
                at: 110
            }
        )
        .is_empty()
    );
    assert!(matches!(
        &apply(
            &mut tracker,
            Input::Exited {
                process: game,
                at: 120
            }
        )[0],
        Effect::CloseSession {
            game_id: Some(620),
            ..
        }
    ));
}

#[test]
fn switching_from_store_to_internal_starts_at_switch_and_no_counter_games_stay_internal() {
    use crate::playtime::PlaytimeSource;
    for (preference, supported, uses_counter) in [
        (PlaytimeSource::Store, true, true),
        (PlaytimeSource::Internal, true, false),
        (PlaytimeSource::Store, false, false),
        (PlaytimeSource::Internal, false, false),
    ] {
        assert_eq!(preference.uses_counter(supported), uses_counter);
    }
    let mut tracker = setup(true);
    let p = process(1, 80, "/games/one/main");
    listed(&mut tracker, 100, vec![p.clone()]);
    let effects = apply(
        &mut tracker,
        Input::Library {
            account: Some("a".into()),
            entries: vec![entry("steam:1", "/games/one")],
            at: 120,
        },
    );
    assert_eq!(
        effects,
        vec![Effect::ReadCounter {
            key: "steam:1".into()
        }]
    );
    let effects = listed(&mut tracker, 125, vec![p]);
    assert!(matches!(&effects[0],Effect::OpenSession(row) if row.started_at==120));
}
