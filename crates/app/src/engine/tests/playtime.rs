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
    let text = format!(
        r#""UserLocalConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "apps" {{ "620" {{ "Playtime" "{minutes}" "LastPlayed" "100" }} }} }} }} }} }}"#
    );
    let mut files = h.files.inner.lock().unwrap();
    *files = files.clone().with_file(CONFIG, &text);
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
async fn counter_update_after_exit_submits_once_without_measured_sessions() {
    let mut h = ready().await;
    h.engine.dismiss_sync_explanation();
    eventually(|| h.api.playtimes.lock().unwrap().len() == 1).await;
    let p = process();
    h.processes.start(p.clone());
    h.engine.maintenance();
    h.until("running counter game", |s| {
        s.tracking.running.contains("steam:620")
    })
    .await;
    eventually(|| h.processes.watch_count() > 0).await;
    set_counter(&h, 755);
    h.processes.exit(
        &p,
        UNIX_EPOCH + Duration::from_secs(super::super::unix_now() as u64),
    );
    eventually(|| h.api.playtimes.lock().unwrap().len() == 2).await;
    assert!(
        h.engine
            .shared
            .ports
            .playtime
            .sessions("u1")
            .unwrap()
            .is_empty()
    );
    h.engine.maintenance_at(super::super::unix_now() + 61);
    h.engine.shutdown();
    assert_eq!(h.api.playtimes.lock().unwrap().len(), 2);
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
