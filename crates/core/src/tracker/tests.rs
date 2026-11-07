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
        parent: None,
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

fn manual_tracker() -> Tracker {
    let mut tracker = Tracker::new(false);
    let mut game = entry("manual:uuid", "/games/one");
    game.source = "manual".into();
    tracker.set_policies(BTreeMap::from([(
        game.key.clone(),
        crate::drm_free::ProcessPolicy::Explicit {
            gameplay: vec!["/games/one/game".into(), "/games/one/actual".into()],
            launchers: vec!["/games/one/launcher".into()],
        },
    )]));
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
#[test]
fn manual_handoff_emits_one_physical_stop_at_actual_exit_and_excludes_helpers() {
    let mut tracker = manual_tracker();
    assert!(
        listed(
            &mut tracker,
            101,
            vec![
                process(1, 100, "/games/one/launcher"),
                process(2, 100, "/games/one/uninstall")
            ]
        )
        .is_empty()
    );
    let game = process(3, 102, "/games/one/game");
    let opened = listed(&mut tracker, 105, vec![game.clone()]);
    assert_eq!(
        opened
            .iter()
            .filter(|e| matches!(e, Effect::OpenSession(_)))
            .count(),
        1
    );
    assert!(
        apply(
            &mut tracker,
            Input::Exited {
                process: game.clone(),
                at: 110
            }
        )
        .is_empty()
    );
    assert!(listed(&mut tracker, 115, vec![]).is_empty());
    let replacement = process(4, 116, "/games/one/actual");
    assert!(
        !listed(&mut tracker, 118, vec![replacement.clone()])
            .iter()
            .any(|e| matches!(e, Effect::OpenSession(_) | Effect::GameStopped { .. }))
    );
    apply(
        &mut tracker,
        Input::Exited {
            process: replacement.clone(),
            at: 125,
        },
    );
    apply(
        &mut tracker,
        Input::Exited {
            process: replacement,
            at: 126,
        },
    );
    let stopped = listed(
        &mut tracker,
        136,
        vec![
            process(1, 100, "/games/one/launcher"),
            process(9, 130, "/games/one/crashreporter"),
        ],
    );
    assert_eq!(
        stopped
            .iter()
            .filter(|e| matches!(
                e,
                Effect::GameStopped {
                    last_gameplay_exit: 125,
                    ..
                }
            ))
            .count(),
        1
    );
    assert!(
        stopped
            .iter()
            .any(|e| matches!(e, Effect::CloseSession { ended_at: 125, .. }))
    );
    assert!(listed(&mut tracker, 140, vec![]).is_empty());
}
#[test]
fn manual_administrative_closures_and_splits_do_not_claim_physical_exit() {
    for close in [
        Input::Stop { at: 110 },
        Input::Library {
            account: None,
            entries: vec![],
            at: 110,
        },
        Input::Tick { now: 100 + DAY + 1 },
    ] {
        let mut tracker = manual_tracker();
        listed(&mut tracker, 105, vec![process(1, 102, "/games/one/game")]);
        let effects = apply(&mut tracker, close);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::GameStopped { .. }))
        );
    }
}
#[test]
fn explicit_manual_ownership_wins_once_and_unknown_outside_root_is_never_adopted() {
    let mut manual = entry("manual:a", "/games/one");
    manual.source = "manual".into();
    let mut other = manual.clone();
    other.key = "manual:b".into();
    let entries = BTreeMap::from([
        (manual.key.clone(), manual),
        (other.key.clone(), other),
        ("store".into(), entry("store", "/games/one/deeper")),
    ]);
    let policies = BTreeMap::from([
        (
            "manual:a".into(),
            crate::drm_free::ProcessPolicy::Explicit {
                gameplay: vec!["/games/one/deeper/game".into()],
                launchers: vec![],
            },
        ),
        (
            "manual:b".into(),
            crate::drm_free::ProcessPolicy::Explicit {
                gameplay: vec!["/games/one/deeper/game".into()],
                launchers: vec![],
            },
        ),
    ]);
    assert_eq!(
        match_policy(&entries, &policies, "/games/one/deeper/game", false)
            .unwrap()
            .key,
        "manual:a"
    );
    assert!(match_policy(&entries, &policies, "/games/one-other/game", false).is_none());
}

#[test]
fn tracker_policy_conflicts_deepest_root_and_one_owner_no_duplicate_playtime() {
    // Deepest-root precedence with two store installs
    let entries = BTreeMap::from([
        ("store:root".into(), entry("store:root", "/games/store")),
        (
            "store:deep".into(),
            entry("store:deep", "/games/store/deeper"),
        ),
    ]);
    let policies = BTreeMap::new();
    assert_eq!(
        match_policy(&entries, &policies, "/games/store/deeper/game.exe", false)
            .unwrap()
            .key,
        "store:deep"
    );
    assert_eq!(
        match_policy(&entries, &policies, "/games/store/other.exe", false)
            .unwrap()
            .key,
        "store:root"
    );

    // Existing store vs manual conflict in shared folder:
    // Store at /games/shared, Manual at /games/shared with explicit gameplay: /games/shared/game.exe
    let mut manual = entry("manual:1", "/games/shared");
    manual.source = "manual".into();
    let entries = BTreeMap::from([
        ("store:1".into(), entry("store:1", "/games/shared")),
        (manual.key.clone(), manual),
    ]);
    let policies = BTreeMap::from([(
        "manual:1".into(),
        crate::drm_free::ProcessPolicy::Explicit {
            gameplay: vec!["/games/shared/game.exe".into()],
            launchers: vec!["/games/shared/launcher.exe".into()],
        },
    )]);
    // Explicit manual gameplay binding wins over store entry in the same folder:
    assert_eq!(
        match_policy(&entries, &policies, "/games/shared/game.exe", false)
            .unwrap()
            .key,
        "manual:1"
    );
    // Launcher or other exe in the shared folder falls back to store entry because manual has no explicit binding:
    assert_eq!(
        match_policy(&entries, &policies, "/games/shared/launcher.exe", false)
            .unwrap()
            .key,
        "store:1"
    );
    assert_eq!(
        match_policy(&entries, &policies, "/games/shared/tool.exe", false)
            .unwrap()
            .key,
        "store:1"
    );

    // In tracker runtime, external manual start emits 1 session, platform 6, and no duplicate playtime
    let mut tracker = Tracker::new(false);
    let mut manual_entry = entry("manual:1", "/games/shared");
    manual_entry.source = "manual".into();
    let entries_vec = vec![manual_entry.clone(), entry("store:1", "/games/shared")];
    tracker.set_policies(policies);
    apply(
        &mut tracker,
        Input::Library {
            account: Some("acc1".into()),
            entries: entries_vec,
            at: 100,
        },
    );
    let p = process(10, 95, "/games/shared/game.exe");
    let effects = listed(&mut tracker, 100, vec![p.clone()]);
    // Exactly one OpenSession effect, with manual:1, platform_id 6, source "manual"
    let open_sessions: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::OpenSession(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(open_sessions.len(), 1);
    assert_eq!(open_sessions[0].entry_key, "manual:1");
    assert_eq!(open_sessions[0].source, "manual");
    assert_eq!(open_sessions[0].platform_id, 6);
    assert_eq!(tracker.running().len(), 1);
    assert!(tracker.running().contains("manual:1"));

    // Subsequent listing while running produces no new sessions (no duplicate playtime)
    let effects2 = listed(&mut tracker, 105, vec![p]);
    assert!(
        effects2
            .iter()
            .all(|e| !matches!(e, Effect::OpenSession(_)))
    );
}

#[test]
fn windows_verbatim_registration_tracks_normal_process_path() {
    for (root, exe) in [
        (
            r"\\?\F:\NonSteamLibrary\Stellar Blade (2025)",
            r"F:\NonSteamLibrary\Stellar Blade (2025)\StellarBlade\SB\Binaries\Win64\SB-Win64-Shipping.exe",
        ),
        (
            r"\\?\UNC\server\games\Stellar Blade",
            r"\\server\games\Stellar Blade\game.exe",
        ),
    ] {
        let mut tracker = Tracker::new(true);
        let mut game = entry("manual:stellar", root);
        game.source = "manual".into();
        tracker.set_policies(BTreeMap::from([(
            game.key.clone(),
            crate::drm_free::ProcessPolicy::Explicit {
                gameplay: vec![exe.into()],
                launchers: vec![],
            },
        )]));
        apply(
            &mut tracker,
            Input::Library {
                account: Some("a".into()),
                entries: vec![game],
                at: 100,
            },
        );
        let effects = listed(&mut tracker, 110, vec![process(1, 105, exe)]);
        assert!(tracker.running().contains("manual:stellar"), "{root}");
        assert!(effects.iter().any(|e| matches!(e, Effect::OpenSession(_))));
        assert!(effects.iter().any(|e| matches!(e, Effect::Watch(_))));
    }
}
