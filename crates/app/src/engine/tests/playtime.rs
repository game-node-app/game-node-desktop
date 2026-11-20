//! Engine integration with synthetic Steam files, scripted processes and mock totals.
use super::*;
use gamenode_core::playtime::PlaytimeSource;
use gamenode_core::ports::ProcessInfo;
use std::time::UNIX_EPOCH;
const ROOT: &str = "/home/me/.steam/steam";
const CONFIG: &str = "/home/me/.steam/steam/userdata/1/config/localconfig.vdf";
async fn eventually(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

/// Waits until no snapshot has been published for a short quiet window, so background work from
/// an earlier step cannot be mistaken for the step under test.
async fn settled(h: &mut Harness) {
    loop {
        h.snapshots.borrow_and_update();
        tokio::time::sleep(Duration::from_millis(50)).await;
        if !h.snapshots.has_changed().unwrap() {
            return;
        }
    }
}
fn set_counter(h: &Harness, minutes: i64) {
    set_counter_with_last(h, minutes, 100);
}
fn set_counter_with_last(h: &Harness, minutes: i64, last_played: i64) {
    let text = format!(
        r#""UserLocalConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "apps" {{ "620" {{ "Playtime" "{minutes}" "LastPlayed" "{last_played}" }} }} }} }} }} }}"#
    );
    let mut files = h.files.inner.lock().unwrap();
    *files = files.clone().with_file(CONFIG, &text);
}
fn set_counters(h: &Harness, entries: &[(&str, i64, i64)]) {
    let apps = entries
        .iter()
        .map(|(id, minutes, last_played)| {
            format!(r#""{id}" {{ "Playtime" "{minutes}" "LastPlayed" "{last_played}" }}"#)
        })
        .collect::<Vec<_>>()
        .join(" ");
    let text = format!(
        r#""UserLocalConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "apps" {{ {apps} }} }} }} }} }}"#
    );
    let mut files = h.files.inner.lock().unwrap();
    *files = files.clone().with_file(CONFIG, &text);
}
/// Adds a second installed Steam app next to Portal 2.
fn add_second_install(h: &Harness) {
    let mut files = h.files.inner.lock().unwrap();
    *files = files
        .clone()
        .with_file(
            &format!("{ROOT}/steamapps/appmanifest_621.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"621\"\n\t\"name\"\t\t\"Portal 2: Second App\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"installdir\"\t\t\"Portal 2 Second\"\n}\n",
        )
        .with_dir(&format!("{ROOT}/steamapps/common/Portal 2 Second"));
}
async fn ready() -> Harness {
    ready_with_source(PlaytimeSource::Store).await
}
async fn ready_with_source(source: PlaytimeSource) -> Harness {
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![ExternalMatch {
        uid: "620".into(),
        game: GameSummary {
            id: 7,
            name: "Portal 2".into(),
            release_year: None,
            cover_url: None,
            platforms: Vec::new(),
        },
    }]));
    let mut h = harness(api, FakeCreds::with(None));
    h.engine.shared.ports.playtime.save_source(source).unwrap();
    set_counter(&h, 754);
    h.signed_out().await;
    h.sign_in("test-token").await;
    h.until("matched installed game", |s| {
        s.library
            .as_ref()
            .is_some_and(|l| !l.scanning && l.rows.iter().any(|r| r.status.game_id() == Some(7)))
    })
    .await;
    if source == PlaytimeSource::Store {
        eventually(|| {
            h.engine
                .shared
                .ports
                .playtime
                .totals("u1")
                .unwrap()
                .iter()
                .any(|t| t.total_seconds == 45240)
        })
        .await;
    }
    h
}
fn process() -> ProcessInfo {
    ProcessInfo {
        pid: 99,
        created_at: UNIX_EPOCH + Duration::from_secs(super::super::unix_now() as u64 - 30),
        exe_path: format!("{ROOT}/steamapps/common/Portal 2/game.exe"),
        parent: None,
    }
}
#[tokio::test(flavor = "multi_thread")]
async fn explanation_and_sync_gate_imports_while_scanning_and_matching_work() {
    let h = ready().await;
    assert!(h.api.playtimes.lock().unwrap().is_empty());
    h.engine.set_sync(false);
    h.engine.dismiss_sync_explanation();
    h.engine.rescan();
    eventually(|| !h.engine.snapshot().library.unwrap().scanning).await;
    assert!(h.api.playtimes.lock().unwrap().is_empty());
    assert!(!h.api.lookups.lock().unwrap().is_empty());
    h.engine.set_sync(true);
    eventually(|| h.api.playtimes.lock().unwrap().len() == 1).await;
    assert_eq!(h.api.playtimes.lock().unwrap()[0].total_seconds, 45240);
    h.engine.rescan();
    eventually(|| !h.engine.snapshot().library.unwrap().scanning).await;
    // FIFO barrier ensures accepted state is durable before another submit pass.
    h.engine.shutdown();
    assert_eq!(h.api.playtimes.lock().unwrap().len(), 1);
}
#[tokio::test(flavor = "multi_thread")]
async fn internal_records_sessions_and_source_switch_preserves_histories() {
    let mut h = ready().await;
    h.engine.set_playtime_source(PlaytimeSource::Internal);
    h.engine.dismiss_sync_explanation();
    let process = process();
    h.processes.start(process.clone());
    h.engine.maintenance();
    h.until("running internally", |s| {
        s.tracking.running.contains("steam:620")
    })
    .await;
    let snapshot = h.engine.snapshot();
    let started = snapshot
        .tracking
        .session_started_at
        .get("steam:620")
        .copied()
        .expect("the snapshot carries the execution start");
    let row = snapshot
        .library
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .find(|row| row.key == "steam:620")
        .unwrap();
    assert_eq!(row.session_started_at, Some(started));
    assert!(row.running);
    // A refresh builds a new snapshot; the same execution keeps its original clock.
    h.engine.maintenance();
    assert_eq!(
        h.engine
            .snapshot()
            .tracking
            .session_started_at
            .get("steam:620"),
        Some(&started)
    );
    eventually(|| h.processes.watch_count() > 0).await;
    assert!(h.api.playtimes.lock().unwrap().is_empty());
    eventually(|| {
        !h.engine
            .shared
            .ports
            .playtime
            .sessions("u1")
            .unwrap()
            .is_empty()
    })
    .await;
    let rows = h.engine.shared.ports.playtime.sessions("u1").unwrap();
    assert!(rows[0].ended_at.is_none());
    h.processes.exit(
        &process,
        UNIX_EPOCH + Duration::from_secs(super::super::unix_now() as u64 + 5),
    );
    h.until("internal exit", |s| s.tracking.running.is_empty())
        .await;
    eventually(|| {
        h.engine.shared.ports.playtime.sessions("u1").unwrap()[0]
            .ended_at
            .is_some()
    })
    .await;
    h.engine.set_playtime_source(PlaytimeSource::Store);
    eventually(|| h.api.playtimes.lock().unwrap().len() == 1).await;
    assert_eq!(
        h.engine.shared.ports.playtime.sessions("u1").unwrap().len(),
        1
    );
    assert_eq!(
        h.engine.shared.ports.playtime.totals("u1").unwrap()[0].total_seconds,
        45240
    );
    h.engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn disabling_plugin_playtime_preserves_measured_history_and_activation() {
    use crate::plugin_host::profile::{PluginProfiles, PluginStateLayout};
    use gamenode_core::playtime::Session;
    use std::sync::Arc;

    let h = ready_with_source(PlaytimeSource::Internal).await;
    let state_dir = tempfile::tempdir().unwrap();
    let profiles = Arc::new(PluginProfiles::default());
    let layout = PluginStateLayout::new(state_dir.path().join("plugins"));
    profiles.set_state_layout(layout.clone());
    h.engine.set_plugin_profiles(profiles.clone());

    let profile = profiles.current().unwrap();
    layout
        .open_activation(&profile)
        .unwrap()
        .set_enabled("org.example.emulator", true)
        .unwrap();
    let existing = Session {
        id: "previous-runtime-session".into(),
        account_id: "u1".into(),
        entry_key: "steam:620".into(),
        game_id: Some(7),
        source: "runtime:org.example.emulator".into(),
        platform_id: 1,
        started_at: 100,
        checkpoint_at: 200,
        ended_at: None,
    };
    let store = h.engine.shared.ports.playtime.clone();
    store.open_session(&existing).unwrap();
    store
        .close_session("u1", &existing.id, 200, Some(7))
        .unwrap();
    let history_before = store.sessions("u1").unwrap();
    assert_eq!(history_before.len(), 1);
    assert_eq!(history_before[0].ended_at, Some(200));

    h.engine
        .set_plugin_playtime_accounting("org.example.emulator".into(), false)
        .await
        .unwrap();

    assert!(
        !profiles
            .is_playtime_accounting_enabled("org.example.emulator")
            .unwrap()
    );
    assert!(
        layout
            .open_activation(&profile)
            .unwrap()
            .is_enabled("org.example.emulator")
    );
    assert_eq!(store.sessions("u1").unwrap(), history_before);
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_reports_account_for_confirmed_pauses_and_disable_fences_active_session() {
    use crate::plugin_host::{
        grants::PluginIdentity,
        namespace::ExternalNamespaceResolver,
        profile::{PluginProfiles, PluginStateLayout},
    };
    use gamenode_core::library::{ApprovedFileReference, RuntimeContentBinding};
    use gamenode_plugin_protocol::proto::{
        RuntimeSessionOutcome, RuntimeSessionReport, RuntimeSessionTransition,
    };
    use std::sync::Arc;

    let h = ready_with_source(PlaytimeSource::Internal).await;
    let state_dir = tempfile::tempdir().unwrap();
    let profiles = Arc::new(PluginProfiles::default());
    let layout = PluginStateLayout::new(state_dir.path().join("plugins"));
    profiles.set_state_layout(layout.clone());
    h.engine.set_plugin_profiles(profiles.clone());

    let plugin_id = "org.example.emulator";
    let profile = profiles.current().unwrap();
    layout
        .open_activation(&profile)
        .unwrap()
        .set_enabled(plugin_id, true)
        .unwrap();
    let identity = PluginIdentity {
        plugin_id: plugin_id.into(),
        publisher_fingerprint: "ab".repeat(32),
        profile_id: profile.profile_id().to_owned(),
    };
    let namespace = ExternalNamespaceResolver::new(identity, None, profile.epoch()).unwrap();
    let owner_key = namespace.library_recipe_owner_key();
    h.store.runtime_bindings.lock().unwrap().insert(
        (owner_key.clone(), 7, "steam:620".into()),
        vec![RuntimeContentBinding {
            game_id: 7,
            installation_id: "steam:620".into(),
            content_id: "rom:portal".into(),
            content: ApprovedFileReference {
                approval_id: "approved-content".into(),
                relative_path: vec!["portal.rom".into()],
            },
            content_kind: "fixture-rom".into(),
            runtime_id: "runtime:emulator".into(),
            supported_platforms: vec![std::env::consts::OS.into()],
            revision: 1,
        }],
    );

    let now = super::super::unix_now();
    let started_at = now - 5;
    let paused_at = now - 4;
    let resumed_at = now - 3;
    let checkpoint_at = now - 2;
    let received_at = Instant::now();
    let report = |sequence, observed_at, transition| RuntimeSessionReport {
        correlation_id: format!("runtime-report-{sequence}"),
        game_id: "7".into(),
        installation_id: "steam:620".into(),
        content_id: "rom:portal".into(),
        runtime_id: "runtime:emulator".into(),
        runtime_instance_id: "instance:one".into(),
        reporter_session_id: "extension-session".into(),
        sequence,
        observed_at,
        transition: transition as i32,
        source_observation_id: format!("observation-{sequence}"),
        runtime_context_id: String::new(),
    };
    let store = h.engine.shared.ports.playtime.clone();

    let started = h
        .engine
        .report_runtime_session(
            plugin_id.into(),
            owner_key.clone(),
            report(1, started_at, RuntimeSessionTransition::Started),
            false,
            None,
            received_at,
        )
        .await;
    assert_eq!(started.outcome, RuntimeSessionOutcome::Accepted as i32);
    eventually(|| store.sessions("u1").unwrap().len() == 1).await;

    let paused = h
        .engine
        .report_runtime_session(
            plugin_id.into(),
            owner_key.clone(),
            report(2, paused_at, RuntimeSessionTransition::Paused),
            false,
            None,
            received_at,
        )
        .await;
    assert_eq!(paused.outcome, RuntimeSessionOutcome::Accepted as i32);
    eventually(|| {
        store
            .sessions("u1")
            .unwrap()
            .iter()
            .any(|session| session.ended_at == Some(paused_at))
    })
    .await;

    let resumed = h
        .engine
        .report_runtime_session(
            plugin_id.into(),
            owner_key.clone(),
            report(3, resumed_at, RuntimeSessionTransition::Resumed),
            false,
            None,
            received_at,
        )
        .await;
    assert_eq!(resumed.outcome, RuntimeSessionOutcome::Accepted as i32);
    let checkpoint = h
        .engine
        .report_runtime_session(
            plugin_id.into(),
            owner_key.clone(),
            report(4, checkpoint_at, RuntimeSessionTransition::Identified),
            false,
            None,
            received_at,
        )
        .await;
    assert_eq!(checkpoint.outcome, RuntimeSessionOutcome::Accepted as i32);

    let sessions_before_disable = store.sessions("u1").unwrap();
    assert_eq!(sessions_before_disable.len(), 2);
    assert_eq!(sessions_before_disable[0].ended_at, Some(paused_at));
    assert!(sessions_before_disable[1].ended_at.is_none());
    h.engine
        .set_plugin_playtime_accounting(plugin_id.into(), false)
        .await
        .unwrap();
    eventually(|| {
        store
            .sessions("u1")
            .unwrap()
            .iter()
            .any(|session| session.ended_at == Some(checkpoint_at))
    })
    .await;
    let sessions_after_disable = store.sessions("u1").unwrap();
    assert_eq!(sessions_after_disable.len(), 2);

    let after_disable = h
        .engine
        .report_runtime_session(
            plugin_id.into(),
            owner_key,
            report(5, now - 1, RuntimeSessionTransition::Ended),
            false,
            None,
            received_at,
        )
        .await;
    assert_eq!(
        after_disable.outcome,
        RuntimeSessionOutcome::RejectedUnavailable as i32
    );
    assert_eq!(store.sessions("u1").unwrap(), sessions_after_disable);
    assert!(
        layout
            .open_activation(&profile)
            .unwrap()
            .is_enabled(plugin_id)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn quit_flushes_a_closed_session_and_signout_preserves_its_account() {
    for signout in [false, true] {
        let mut h = ready_with_source(PlaytimeSource::Internal).await;
        let p = process();
        h.processes.start(p);
        h.engine.maintenance();
        h.until("running", |s| !s.tracking.running.is_empty()).await;
        if signout {
            h.engine.dispatch(Input::SignOut);
            let tracking = h.engine.snapshot().tracking;
            assert!(tracking.running.is_empty());
            assert!(
                tracking.session_started_at.is_empty(),
                "signing out clears the live session clock with the tracked account"
            );
        }
        h.engine.shutdown();
        let rows = h.engine.shared.ports.playtime.sessions("u1").unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows.iter().all(|r| r.ended_at.is_some()));
        assert!(
            h.engine
                .shared
                .ports
                .playtime
                .sessions("another")
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn idle_maintenance_does_not_publish_but_a_running_change_does() {
    let mut h = ready().await;
    let now = super::super::unix_now();
    // The first tick arms the periodic timers (scan every 30 minutes, process listing every 5
    // seconds) and starts those sub-steps once.
    h.engine.maintenance_at(now);
    eventually(|| !h.engine.snapshot().library.unwrap().scanning).await;
    settled(&mut h).await;

    let publishes = h.publishes.clone();
    let before = publishes.load(std::sync::atomic::Ordering::SeqCst);

    // Nothing changed: an idle tick must not publish a rebuilt snapshot.
    h.engine.maintenance_at(now + 1);
    assert_eq!(
        publishes.load(std::sync::atomic::Ordering::SeqCst),
        before,
        "an idle maintenance tick published a snapshot"
    );

    // A game that starts is a real change: the next listing publishes it.
    h.processes.start(process());
    h.engine.maintenance_at(now + 5);
    h.until("running", |s| s.tracking.running.contains("steam:620"))
        .await;
    assert!(
        publishes.load(std::sync::atomic::Ordering::SeqCst) > before,
        "a running set change did not publish"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_publishes_the_execution_start_for_a_counter_game_without_a_session() {
    let mut h = ready().await;
    let process = process();
    let started = process
        .created_at
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    h.processes.start(process);
    h.engine.maintenance();
    h.until("running counter game", |s| {
        s.tracking.running.contains("steam:620")
    })
    .await;
    let snapshot = h.engine.snapshot();
    assert_eq!(
        snapshot.tracking.session_started_at.get("steam:620"),
        Some(&started),
        "counter-backed entries keep a live session clock without a durable session"
    );
    assert!(
        h.engine
            .shared
            .ports
            .playtime
            .sessions("u1")
            .unwrap()
            .is_empty(),
        "the store counter never opens a measured session"
    );
    let row = snapshot
        .library
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .find(|row| row.key == "steam:620")
        .unwrap();
    assert_eq!(row.session_started_at, Some(started));
    h.engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn periodic_scans_use_thirty_minutes_and_window_show_requests_a_scan() {
    let h = ready().await;
    let now = super::super::unix_now();
    h.engine.maintenance_at(now);
    eventually(|| !h.engine.snapshot().library.unwrap().scanning).await;
    let scans = h
        .files
        .library_reads
        .load(std::sync::atomic::Ordering::SeqCst);
    h.engine.maintenance_at(now + 1799);
    assert_eq!(
        h.files
            .library_reads
            .load(std::sync::atomic::Ordering::SeqCst),
        scans
    );
    h.engine.maintenance_at(now + 1800);
    eventually(|| {
        h.files
            .library_reads
            .load(std::sync::atomic::Ordering::SeqCst)
            > scans
    })
    .await;
    eventually(|| !h.engine.snapshot().library.unwrap().scanning).await;
    let scans = h
        .files
        .library_reads
        .load(std::sync::atomic::Ordering::SeqCst);
    h.engine.window_shown();
    eventually(|| {
        h.files
            .library_reads
            .load(std::sync::atomic::Ordering::SeqCst)
            > scans
    })
    .await;
    h.engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn newer_counter_read_survives_an_older_submission_completing() {
    let h = ready().await;
    *h.api.playtime_delay.lock().unwrap() = Duration::from_millis(100);
    h.engine.dismiss_sync_explanation();
    eventually(|| h.api.playtimes.lock().unwrap().len() == 1).await;
    set_counter(&h, 755);
    h.engine.rescan();
    eventually(|| h.engine.shared.ports.playtime.totals("u1").unwrap()[0].total_seconds == 45300)
        .await;
    eventually(|| h.api.playtimes.lock().unwrap().len() == 2).await;
    eventually(|| {
        h.engine.shared.ports.playtime.totals("u1").unwrap()[0].accepted_seconds == Some(45300)
    })
    .await;
    h.engine.shutdown();
    assert_eq!(
        h.engine.shared.ports.playtime.totals("u1").unwrap()[0].total_seconds,
        45300
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_last_played_is_unknown_and_the_total_is_still_submitted() {
    let mut h = ready().await;
    h.engine.set_sync(true);
    h.engine.dismiss_sync_explanation();
    eventually(|| h.api.playtimes.lock().unwrap().len() == 1).await;
    assert_eq!(
        h.api.playtimes.lock().unwrap()[0].last_played_at,
        Some(100),
        "the known date is submitted first"
    );

    // LastPlayed 0 means Steam has never recorded a date; it must not become 1970.
    set_counter_with_last(&h, 760, 0);
    h.engine.rescan();
    eventually(|| {
        h.engine
            .shared
            .ports
            .playtime
            .totals("u1")
            .unwrap()
            .iter()
            .any(|t| {
                t.entry_key == "steam:620"
                    && t.total_seconds == 45_600
                    && t.last_played_at.is_none()
            })
    })
    .await;
    eventually(|| h.api.playtimes.lock().unwrap().len() == 2).await;
    assert_eq!(
        h.api.playtimes.lock().unwrap()[1].last_played_at,
        None,
        "an absent counter date is submitted as unknown, not as a date"
    );
    assert_eq!(h.api.playtimes.lock().unwrap()[1].total_seconds, 45_600);
    h.until("unknown last played shown", |s| {
        s.library.as_ref().is_some_and(|library| {
            library.rows.iter().any(|row| {
                row.status.game_id() == Some(7)
                    && row.playtime.seconds == Some(45_600)
                    && row.playtime.last_played_at.is_none()
            })
        })
    })
    .await;
    h.engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn two_entries_of_one_game_submit_one_summed_total_and_do_not_ping_pong() {
    let mut h = ready().await;
    add_second_install(&h);
    set_counters(&h, &[("620", 754, 100), ("621", 30, 200)]);
    *h.api.lookup_result.lock().unwrap() = Some(Ok(vec![
        ExternalMatch {
            uid: "620".into(),
            game: GameSummary {
                id: 7,
                name: "Portal 2".into(),
                release_year: None,
                cover_url: None,
                platforms: Vec::new(),
            },
        },
        ExternalMatch {
            uid: "621".into(),
            game: GameSummary {
                id: 7,
                name: "Portal 2".into(),
                release_year: None,
                cover_url: None,
                platforms: Vec::new(),
            },
        },
    ]));

    h.engine.rescan();
    h.until("both entries matched to one game", |s| {
        s.library.as_ref().is_some_and(|library| {
            library
                .rows
                .iter()
                .filter(|row| row.status.game_id() == Some(7))
                .count()
                == 2
        })
    })
    .await;
    eventually(|| {
        let totals = h.engine.shared.ports.playtime.totals("u1").unwrap();
        totals
            .iter()
            .any(|t| t.entry_key == "steam:620" && t.total_seconds == 45_240)
            && totals
                .iter()
                .any(|t| t.entry_key == "steam:621" && t.total_seconds == 1_800)
    })
    .await;

    h.engine.set_sync(true);
    h.engine.dismiss_sync_explanation();
    eventually(|| h.api.playtimes.lock().unwrap().len() == 1).await;
    let submitted = h.api.playtimes.lock().unwrap()[0].clone();
    assert_eq!(submitted.game_id, 7);
    assert_eq!(
        submitted.total_seconds, 47_040,
        "one summed total, not two competing totals"
    );
    assert_eq!(
        submitted.last_played_at,
        Some(200),
        "the latest known date of the group is submitted"
    );

    eventually(|| {
        h.engine
            .shared
            .ports
            .playtime
            .totals("u1")
            .unwrap()
            .iter()
            .any(|t| {
                t.entry_key == "steam:game:7"
                    && t.accepted_seconds == Some(47_040)
                    && t.accepted_game_id == Some(7)
            })
    })
    .await;

    // The same sum on a later read must not send anything again.
    h.engine.rescan();
    eventually(|| !h.engine.snapshot().library.unwrap().scanning).await;
    settled(&mut h).await;
    h.engine.shutdown();
    assert_eq!(
        h.api.playtimes.lock().unwrap().len(),
        1,
        "an unchanged sum must not be submitted again"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn library_displays_imported_time_and_live_internal_time_when_source_changes() {
    let mut h = ready().await;
    h.until("imported time is visible", |s| {
        s.library
            .as_ref()
            .is_some_and(|l| l.rows[0].playtime.seconds == Some(45240))
    })
    .await;
    assert_eq!(
        h.engine.snapshot().library.unwrap().rows[0].playtime.source,
        PlaytimeSource::Store
    );
    h.engine.set_playtime_source(PlaytimeSource::Internal);
    h.until("separate internal history", |s| {
        s.library.as_ref().is_some_and(|l| {
            l.rows[0].playtime.seconds == Some(0)
                && l.rows[0].playtime.source == PlaytimeSource::Internal
        })
    })
    .await;
    let p = process();
    h.processes.start(p);
    h.engine.maintenance();
    h.until("running internally", |s| {
        s.tracking.running.contains("steam:620")
    })
    .await;
    eventually(|| {
        !h.engine
            .shared
            .ports
            .playtime
            .sessions("u1")
            .unwrap()
            .is_empty()
    })
    .await;
    let start = h.engine.shared.ports.playtime.sessions("u1").unwrap()[0].started_at;
    eventually(|| {
        let mut state = h.engine.lock();
        h.engine.update_playtime_view(&mut state, start + 125);
        state.library.view().unwrap().rows[0].playtime.seconds == Some(125)
    })
    .await;
    h.engine.set_playtime_source(PlaytimeSource::Store);
    h.until("imported history restored", |s| {
        s.library.as_ref().is_some_and(|l| {
            l.rows[0].playtime.seconds == Some(45240)
                && l.rows[0].playtime.source == PlaytimeSource::Store
        })
    })
    .await;
    h.engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_now_submits_held_totals_and_records_the_summary() {
    let mut h = ready().await;
    h.engine.dismiss_sync_explanation();
    h.until("the automatic pass ends", |s| !s.sync.running)
        .await;
    // The store reports a newer total that the next pass must submit.
    set_counter(&h, 764);
    let before = h.api.playtimes.lock().unwrap().len();
    *h.api.playtime_delay.lock().unwrap() = Duration::from_millis(100);

    h.engine.sync_now().unwrap();
    assert!(
        h.engine.snapshot().sync.running,
        "the button shows progress until the pass ends"
    );
    let snapshot = h.until("sync now completed", |s| !s.sync.running).await;
    let summary = snapshot
        .sync
        .summary
        .expect("a completed pass has a summary");
    assert_eq!(summary.accepted_games, 1);
    assert_eq!(summary.stores, 1);
    assert_eq!(summary.held, 0);
    assert!(summary.completed_at > 0);
    let submissions = h.api.playtimes.lock().unwrap().clone();
    assert!(submissions.len() > before);
    assert_eq!(submissions.last().unwrap().total_seconds, 764 * 60);
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_now_waits_for_the_rescan_and_counter_import_and_coalesces_a_second_click() {
    let mut h = ready().await;
    h.engine.dismiss_sync_explanation();
    h.until("the automatic pass ends", |s| !s.sync.running)
        .await;

    // The stored total is now stale and no longer accepted, so submitting before the scan would
    // send the old 754 minutes.
    {
        let store = &h.engine.shared.ports.playtime;
        let mut total = store
            .totals("u1")
            .unwrap()
            .into_iter()
            .find(|total| total.entry_key == "steam:620")
            .unwrap();
        total.accepted_game_id = None;
        total.accepted_seconds = None;
        total.rejected_seconds = None;
        store.save_total(&total).unwrap();
    }
    set_counter(&h, 764);
    let before = h.api.playtimes.lock().unwrap().len();

    h.files.gate.close();
    h.engine.sync_now().unwrap();
    assert!(
        h.engine.snapshot().sync.running,
        "the button shows progress while the rescan runs"
    );
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        h.api.playtimes.lock().unwrap().len(),
        before,
        "the pass must not send the pre-scan total"
    );

    h.engine.sync_now().unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        h.api.playtimes.lock().unwrap().len(),
        before,
        "a second click coalesces instead of starting another pass"
    );

    h.files.gate.open();
    let snapshot = h.until("sync now completes", |s| !s.sync.running).await;
    let summary = snapshot
        .sync
        .summary
        .expect("a completed pass has a summary");
    assert_eq!(summary.accepted_games, 1);
    assert_eq!(summary.stores, 1);
    assert_eq!(summary.held, 0);
    let submissions = h.api.playtimes.lock().unwrap().clone();
    assert_eq!(submissions.len(), before + 1, "exactly one requested pass");
    assert_eq!(submissions.last().unwrap().total_seconds, 764 * 60);
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_now_disabled_while_waiting_sends_nothing() {
    let mut h = ready().await;
    h.engine.dismiss_sync_explanation();
    h.until("the automatic pass ends", |s| !s.sync.running)
        .await;
    {
        let store = &h.engine.shared.ports.playtime;
        let mut total = store
            .totals("u1")
            .unwrap()
            .into_iter()
            .find(|total| total.entry_key == "steam:620")
            .unwrap();
        total.accepted_game_id = None;
        total.accepted_seconds = None;
        store.save_total(&total).unwrap();
    }
    set_counter(&h, 764);
    let before = h.api.playtimes.lock().unwrap().len();

    h.files.gate.close();
    h.engine.sync_now().unwrap();
    assert!(h.engine.snapshot().sync.running);
    h.engine.set_sync(false);
    assert!(
        !h.engine.snapshot().sync.running,
        "turning sync off cancels the requested pass"
    );

    h.files.gate.open();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        h.api.playtimes.lock().unwrap().len(),
        before,
        "the scan completion cannot submit after sync was turned off"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_now_account_change_while_waiting_sends_nothing_for_the_old_account() {
    let mut h = ready().await;
    h.engine.dismiss_sync_explanation();
    h.until("the automatic pass ends", |s| !s.sync.running)
        .await;
    {
        let store = &h.engine.shared.ports.playtime;
        let mut total = store
            .totals("u1")
            .unwrap()
            .into_iter()
            .find(|total| total.entry_key == "steam:620")
            .unwrap();
        total.accepted_game_id = None;
        total.accepted_seconds = None;
        store.save_total(&total).unwrap();
    }
    set_counter(&h, 764);
    let before = h.api.playtimes.lock().unwrap().len();

    h.files.gate.close();
    h.engine.sync_now().unwrap();
    assert!(h.engine.snapshot().sync.running);
    h.engine.dispatch(Input::SignOut);
    assert!(!h.engine.snapshot().sync.running);

    h.files.gate.open();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        h.api.playtimes.lock().unwrap().len(),
        before,
        "a scan finishing after sign-out cannot submit the old account's totals"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_total_is_retried_by_the_next_sync_now_and_then_accepted() {
    let mut h = ready().await;
    h.engine.set_sync(false);
    h.api.lookup_result.lock().unwrap().replace(Ok(vec![
        ExternalMatch {
            uid: "620".into(),
            game: GameSummary {
                id: 7,
                name: "Portal 2".into(),
                release_year: None,
                cover_url: None,
                platforms: Vec::new(),
            },
        },
        ExternalMatch {
            uid: "621".into(),
            game: GameSummary {
                id: 8,
                name: "Portal 2: Second App".into(),
                release_year: None,
                cover_url: None,
                platforms: Vec::new(),
            },
        },
    ]));
    add_second_install(&h);
    set_counters(&h, &[("620", 754, 100), ("621", 10, 101)]);
    h.engine.rescan();
    h.until("both installed games are matched", |s| {
        s.library.as_ref().is_some_and(|library| {
            !library.scanning
                && library
                    .rows
                    .iter()
                    .filter(|row| row.status.game_id().is_some())
                    .count()
                    == 2
        })
    })
    .await;
    eventually(|| {
        let totals = h.engine.shared.ports.playtime.totals("u1").unwrap();
        totals
            .iter()
            .any(|total| total.entry_key == "steam:620" && total.total_seconds == 45240)
            && totals
                .iter()
                .any(|total| total.entry_key == "steam:621" && total.total_seconds == 600)
    })
    .await;

    // Enable the pass without letting an automatic submission run first.
    let preferences = gamenode_core::sync::SyncPreferences {
        enabled: true,
        explained: true,
    };
    h.engine
        .shared
        .ports
        .playtime
        .save_sync(preferences)
        .unwrap();
    h.engine.lock().playtime.preferences = preferences;
    *h.api.playtime_reject_seconds.lock().unwrap() = Some(600);

    h.engine.sync_now().unwrap();
    let snapshot = h
        .until("partial pass records one held total", |s| {
            !s.sync.running && s.sync.summary.is_some_and(|summary| summary.held == 1)
        })
        .await;
    let summary = snapshot.sync.summary.unwrap();
    assert_eq!(summary.accepted_games, 1);
    assert_eq!(summary.held, 1);
    assert_eq!(summary.stores, 1);

    // The next explicit opportunity retries the rejected total; the accepted one is not sent
    // again.
    *h.api.playtime_reject_seconds.lock().unwrap() = None;
    h.engine.sync_now().unwrap();
    let snapshot = h
        .until("retry pass accepts both totals", |s| {
            !s.sync.running && s.sync.summary.is_some_and(|summary| summary.held == 0)
        })
        .await;
    let summary = snapshot.sync.summary.unwrap();
    assert_eq!(summary.accepted_games, 2);
    let submissions = h.api.playtimes.lock().unwrap().clone();
    assert_eq!(
        submissions
            .iter()
            .filter(|submission| submission.total_seconds == 600)
            .count(),
        2,
        "the rejected total was sent once, rejected, then retried: {submissions:?}"
    );
    assert_eq!(
        submissions
            .iter()
            .filter(|submission| submission.total_seconds == 45240)
            .count(),
        1,
        "the accepted total keeps its dedup"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_now_is_unavailable_with_sync_off_or_without_a_session() {
    let h = ready().await;
    h.engine.set_sync(false);
    let before = h.engine.snapshot().sync;
    assert_eq!(
        h.engine.sync_now(),
        Err(gamenode_core::sync::SyncNowError::Disabled)
    );
    assert_eq!(h.engine.snapshot().sync, before, "nothing was sent");

    let h = ready().await;
    h.engine.dispatch(Input::SignOut);
    assert_eq!(
        h.engine.sync_now(),
        Err(gamenode_core::sync::SyncNowError::Unverified)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_sync_summary_is_shown_after_start_and_before_any_new_pass() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.engine
        .shared
        .ports
        .playtime
        .save_sync_summary(
            "u1",
            gamenode_core::sync::SyncSummary {
                completed_at: 1_700_000_000,
                accepted_games: 3,
                stores: 2,
                held: 1,
            },
        )
        .unwrap();
    h.signed_out().await;
    h.sign_in("token").await;
    let snapshot = h
        .until("the stored summary is loaded", |s| s.sync.summary.is_some())
        .await;
    assert_eq!(
        snapshot.sync.summary,
        Some(gamenode_core::sync::SyncSummary {
            completed_at: 1_700_000_000,
            accepted_games: 3,
            stores: 2,
            held: 1,
        })
    );
}
