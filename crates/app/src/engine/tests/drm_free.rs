use super::*;
use gamenode_core::drm_free::{
    Candidate, Evidence, InstallationHost, LocalError, LocalGameStore, Registration, WatchRoot,
};
use gamenode_core::ports::{PlaytimeStore, ProcessIdentity, ProcessInfo};
use std::sync::Condvar;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

async fn eventually(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[derive(Default)]
struct Host {
    blocked: Mutex<bool>,
    release: Condvar,
    sequence: AtomicUsize,
}
impl InstallationHost for Host {
    fn inspect(&self, path: &str) -> Result<Registration, LocalError> {
        let mut blocked = self.blocked.lock().unwrap();
        while *blocked {
            blocked = self.release.wait(blocked).unwrap();
        }
        let path = std::fs::canonicalize(path).map_err(|_| LocalError::InvalidTarget)?;
        let root = path.parent().unwrap().to_string_lossy().into_owned();
        Ok(Registration {
            id: format!("fixture-{}", self.sequence.fetch_add(1, Ordering::SeqCst)),
            title: "Fixture".into(),
            executable: path.to_string_lossy().into_owned(),
            file_identity: path.to_string_lossy().into_owned(),
            root: root.clone(),
            working_directory: root,
            arguments: vec![],
            gameplay: vec![path.to_string_lossy().into_owned()],
            launchers: vec![],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            available: true,
        })
    }
    fn launch(&self, _: &Registration) -> Result<(), LocalError> {
        Ok(())
    }
    fn evidence(&self, reg: &Registration) -> Result<Vec<Evidence>, LocalError> {
        Ok(vec![Evidence {
            kind: gamenode_core::drm_free::EvidenceKind::Product,
            value: reg.title.clone(),
            strong: true,
        }])
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn local_add_cancel_duplicates_repair_and_remove_are_durable_and_do_not_block() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    let host = Arc::new(Host::default());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
        shared.ports.installations = Some(host.clone());
        shared.state.get_mut().unwrap().library = LibraryMachine::new(
            crate::providers::load_bundled(Platform::Windows).providers,
            Default::default(),
        );
    }
    h.signed_out().await;
    h.sign_in("token").await;
    h.engine.add_local_executable(None);
    assert!(store.registrations().unwrap().is_empty());
    let fixture = tempfile::tempdir().unwrap();
    let original = fixture.path().join("game.exe");
    std::fs::write(&original, b"fixture").unwrap();
    struct Unblock(Arc<Host>);
    impl Drop for Unblock {
        fn drop(&mut self) {
            *self.0.blocked.lock().unwrap() = false;
            self.0.release.notify_all();
        }
    }
    *host.blocked.lock().unwrap() = true;
    let _unblock = Unblock(host.clone());
    let started = Instant::now();
    h.engine
        .add_local_executable(Some(original.to_string_lossy().into_owned()));
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(h.engine.snapshot().local_games.busy);
    assert!(store.registrations().unwrap().is_empty());
    *host.blocked.lock().unwrap() = false;
    host.release.notify_all();
    h.until("local add persisted", |s| {
        s.local_games.registrations.len() == 1 && !s.local_games.busy
    })
    .await;
    let registration = store.registrations().unwrap().remove(0);
    h.engine
        .add_local_executable(Some(original.to_string_lossy().into_owned()));
    h.until("duplicate resolved", |s| !s.local_games.busy).await;
    assert_eq!(store.registrations().unwrap().len(), 1);
    let moved = fixture.path().join("moved.exe");
    std::fs::rename(&original, &moved).unwrap();
    h.engine.repair_local_game(
        registration.id.clone(),
        Some(moved.to_string_lossy().into_owned()),
    );
    h.until("repair persisted", |s| {
        !s.local_games.busy && s.local_games.registrations[0].revision == 2
    })
    .await;
    assert_eq!(store.registrations().unwrap()[0].id, registration.id);
    h.engine.remove_local_game(registration.id);
    h.until("removal persisted", |s| {
        !s.local_games.busy && s.local_games.registrations[0].removed
    })
    .await;
    assert!(moved.is_file());
    h.engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn local_evidence_enrichment_overrides_retries_and_account_switching() {
    let api = FakeApi::new();
    *api.search_result.lock().unwrap() = Some(Ok(gamenode_core::game_api::SearchPage {
        games: vec![GameSummary {
            id: 10,
            name: "Fixture".into(),
            release_year: Some(2020),
            cover_url: None,
        }],
        complete: Some(true),
    }));
    let mut h = harness(api.clone(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    let host = Arc::new(Host::default());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
        shared.ports.installations = Some(host.clone());
        shared.state.get_mut().unwrap().library = LibraryMachine::new(
            crate::providers::load_bundled(Platform::Windows).providers,
            Default::default(),
        );
    }
    h.signed_out().await;
    h.sign_in("token").await;

    let fixture = tempfile::tempdir().unwrap();
    let original = fixture.path().join("game.exe");
    std::fs::write(&original, b"fixture").unwrap();

    h.engine
        .add_local_executable(Some(original.to_string_lossy().into_owned()));
    h.until("local game added", |s| {
        s.local_games.registrations.len() == 1 && !s.local_games.busy
    })
    .await;

    // Background enrichment runs:
    h.until("local game enriched with catalog match", |s| {
        s.local_games
            .inferences
            .iter()
            .any(|i| i.game_id == Some(10))
    })
    .await;

    let inferences_u1 = store.inferences("u1").unwrap();
    assert_eq!(inferences_u1.len(), 1);
    assert_eq!(inferences_u1[0].game_id, Some(10));

    // Verify chosen match overrides local inference
    let key = format!("manual:{}", inferences_u1[0].installation);
    h.engine
        .library_input(gamenode_core::library::Input::Select {
            key: key.clone(),
            mode: gamenode_core::library::SelectMode::Replace,
        });
    h.engine
        .library_input(gamenode_core::library::Input::OpenMatch);
    h.until("match dialog open with results", |s| {
        s.library
            .as_ref()
            .and_then(|lib| lib.match_dialog.as_ref())
            .is_some_and(|d| !d.searching && !d.results.is_empty())
    })
    .await;
    h.engine
        .library_input(gamenode_core::library::Input::PickMatch {
            game_id: 10,
            now: 1,
        });
    h.until("chosen match applied", |s| {
        s.library.as_ref().is_some_and(|lib| {
            lib.rows.iter().any(|r| {
                r.key == key
                    && matches!(
                        r.status,
                        gamenode_core::library::MatchStatus::Matched {
                            game_id: 10,
                            source: gamenode_core::library::matching::MatchSource::Chosen,
                        }
                    )
            })
        })
    })
    .await;

    // Verify no-match override
    h.engine
        .library_input(gamenode_core::library::Input::OpenMatch);
    h.until("match dialog open again", |s| {
        s.library
            .as_ref()
            .and_then(|lib| lib.match_dialog.as_ref())
            .is_some()
    })
    .await;
    h.engine
        .library_input(gamenode_core::library::Input::MarkNoGame { now: 2 });
    h.until("no-match override applied", |s| {
        s.library.as_ref().is_some_and(|lib| {
            lib.rows.iter().any(|r| {
                r.key == key && matches!(r.status, gamenode_core::library::MatchStatus::NoGame)
            })
        })
    })
    .await;

    // Clear match so automatic match applies again
    h.engine
        .library_input(gamenode_core::library::Input::OpenMatch);
    h.until("match dialog open for clear", |s| {
        s.library
            .as_ref()
            .and_then(|lib| lib.match_dialog.as_ref())
            .is_some()
    })
    .await;
    h.engine
        .library_input(gamenode_core::library::Input::ClearMatch);
    h.until("automatic match restored", |s| {
        s.library.as_ref().is_some_and(|lib| {
            lib.rows.iter().any(|r| {
                r.key == key
                    && matches!(
                        r.status,
                        gamenode_core::library::MatchStatus::Matched {
                            game_id: 10,
                            source: gamenode_core::library::matching::MatchSource::Automatic,
                        }
                    )
            })
        })
    })
    .await;

    // Verify account switching: sign out clears inferences, sign in as u2 has empty inferences
    h.engine.dispatch(Input::SignOut);
    h.until("signed out and inferences cleared", |s| {
        s.auth == AuthView::SignedOut && s.local_games.inferences.is_empty()
    })
    .await;

    // Switch to user u2
    *api.session.lock().unwrap() = Some(Ok(User {
        id: "u2".into(),
        name: "Bob".into(),
        email: None,
        image: None,
    }));
    h.sign_in("token2").await;
    h.until(
        "signed in as u2",
        |s| matches!(s.auth, AuthView::SignedIn { ref user } if user.id == "u2"),
    )
    .await;
    assert!(store.inferences("u2").unwrap().is_empty());

    // Switch back to u1: inference for u1 is restored
    h.engine.dispatch(Input::SignOut);
    h.until("signed out again", |s| s.auth == AuthView::SignedOut)
        .await;
    *api.session.lock().unwrap() = Some(Ok(ana()));
    h.sign_in("token").await;
    h.until("signed in as u1 with restored inference", |s| {
        s.local_games
            .inferences
            .iter()
            .any(|i| i.game_id == Some(10))
    })
    .await;

    // Verify lookup failure does not crash or corrupt state
    *api.search_result.lock().unwrap() = Some(Err(ApiError::Failed));
    let original2 = fixture.path().join("game2.exe");
    std::fs::write(&original2, b"fixture2").unwrap();
    h.engine
        .add_local_executable(Some(original2.to_string_lossy().into_owned()));
    h.until("second game added", |s| {
        s.local_games.registrations.len() == 2 && !s.local_games.busy
    })
    .await;

    // It remains in library, inferences for u1 still has the previous one intact
    assert_eq!(store.inferences("u1").unwrap().len(), 1);

    h.engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn launcher_gameplay_learning_persistence_and_external_tracking() {
    let api = FakeApi::new();
    let mut h = harness(api.clone(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    let host = Arc::new(Host::default());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
        shared.ports.installations = Some(host.clone());
        shared.state.get_mut().unwrap().library = LibraryMachine::new(
            crate::providers::load_bundled(Platform::Windows).providers,
            Default::default(),
        );
    }
    h.signed_out().await;
    h.sign_in("token").await;

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_path_buf();
    let launcher_path = root.join("launcher.exe");
    let game_path = root.join("bin").join("game.exe");
    let outside_path = fixture
        .path()
        .parent()
        .unwrap()
        .join("other")
        .join("outside.exe");
    let uninstall_path = root.join("uninstall.exe");

    std::fs::create_dir_all(game_path.parent().unwrap()).unwrap();
    std::fs::write(&launcher_path, b"launcher").unwrap();
    std::fs::write(&game_path, b"game").unwrap();

    let launcher_canonical = std::fs::canonicalize(&launcher_path).unwrap();
    let game_canonical = std::fs::canonicalize(&game_path).unwrap();

    // Add local executable as launcher
    h.engine
        .add_local_executable(Some(launcher_canonical.to_string_lossy().into_owned()));
    h.until("local game added", |s| {
        s.local_games.registrations.len() == 1 && !s.local_games.busy
    })
    .await;

    let mut reg = store.registrations().unwrap().remove(0);
    let key = reg.key();
    // Configure it as having a launcher role and no initial gameplay executable
    reg.launchers = vec![launcher_canonical.to_string_lossy().into_owned()];
    reg.gameplay.clear();
    h.engine.edit_local_game(reg);
    h.until("edit saved", |s| {
        s.local_games
            .registrations
            .first()
            .is_some_and(|r| r.gameplay.is_empty() && !r.launchers.is_empty())
    })
    .await;

    // 1. Process listing includes an outside-root executable and an uninstaller.
    // They must not be learned or tracked.
    let start_time = super::super::unix_now();
    let outside_proc = ProcessInfo {
        pid: 101,
        created_at: UNIX_EPOCH + Duration::from_secs((start_time - 10) as u64),
        exe_path: outside_path.to_string_lossy().into_owned(),
        parent: None,
    };
    let uninst_proc = ProcessInfo {
        pid: 102,
        created_at: UNIX_EPOCH + Duration::from_secs((start_time - 10) as u64),
        exe_path: uninstall_path.to_string_lossy().into_owned(),
        parent: None,
    };
    let launcher_proc = ProcessInfo {
        pid: 103,
        created_at: UNIX_EPOCH + Duration::from_secs((start_time - 5) as u64),
        exe_path: launcher_canonical.to_string_lossy().into_owned(),
        parent: None,
    };
    h.processes.start(outside_proc.clone());
    h.processes.start(uninst_proc.clone());
    h.processes.start(launcher_proc.clone());

    h.engine.maintenance_at(start_time);
    eventually(|| !h.engine.lock().playtime.listing).await;
    // Only launcher is running; not counted as gameplay
    assert!(!h.engine.snapshot().tracking.running.contains(&key));

    // 2. Child game executable starts
    let game_proc = ProcessInfo {
        pid: 104,
        created_at: UNIX_EPOCH + Duration::from_secs(start_time as u64),
        exe_path: game_canonical.to_string_lossy().into_owned(),
        parent: Some(ProcessIdentity {
            pid: 103,
            created_at: UNIX_EPOCH + Duration::from_secs((start_time - 5) as u64),
        }),
    };
    h.processes.start(game_proc.clone());

    // Trigger maintenance to learn and track
    let now = start_time + 10;
    h.engine.maintenance_at(now);

    // Should learn game.exe and track it
    h.until("child game tracked", |s| s.tracking.running.contains(&key))
        .await;

    // Verify registration was updated in store with learned gameplay executable
    let updated_reg = store.registrations().unwrap().remove(0);
    assert!(
        updated_reg
            .gameplay
            .iter()
            .any(|p| p == &game_canonical.to_string_lossy())
    );

    // 3. Game process exits. Launcher persists.
    eventually(|| !h.engine.lock().playtime.listing).await;
    h.processes.exit(
        &game_proc,
        UNIX_EPOCH + Duration::from_secs((now + 5) as u64),
    );
    // After 10s settling delay has elapsed (e.g. now + 20):
    h.engine.maintenance_at(now + 20);
    h.until("gameplay stopped despite persistent launcher", |s| {
        !s.tracking.running.contains(&key)
    })
    .await;

    // Verify no sessions were sent via HTTP (sessions remain local with platform 6)
    assert!(api.playtimes.lock().unwrap().is_empty());
    eventually(|| {
        h.engine
            .shared
            .ports
            .playtime
            .sessions("u1")
            .unwrap()
            .first()
            .is_some_and(|s| s.ended_at.is_some())
    })
    .await;
    let sessions = h.engine.shared.ports.playtime.sessions("u1").unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].platform_id, 6);
    assert_eq!(sessions[0].source, "manual");
    assert!(sessions[0].ended_at.is_some());

    // 4. Externally started learned executable without launcher
    eventually(|| !h.engine.lock().playtime.listing).await;
    let game_proc2 = ProcessInfo {
        pid: 204,
        created_at: UNIX_EPOCH + Duration::from_secs((now + 30) as u64),
        exe_path: game_canonical.to_string_lossy().into_owned(),
        parent: None,
    };
    h.processes.start(game_proc2.clone());
    h.engine.maintenance_at(now + 35);
    h.until("externally started learned game tracked", |s| {
        s.tracking.running.contains(&key)
    })
    .await;

    h.engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn drm_free_session_tracking_restart_recovery_and_no_http_submission() {
    let api = FakeApi::new();
    let creds = FakeCreds::with(None);
    let mut h = harness(api.clone(), creds.clone());
    let db_path = tempfile::tempdir().unwrap();
    let db_file = db_path.path().join("settings.db");
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(&db_file).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    let host = Arc::new(Host::default());
    let playtime_store = Arc::new(database.playtime_store());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
        shared.ports.installations = Some(host.clone());
        shared.ports.playtime = playtime_store.clone();
        shared.state.get_mut().unwrap().library = LibraryMachine::new(
            crate::providers::load_bundled(Platform::Windows).providers,
            Default::default(),
        );
    }
    h.signed_out().await;
    h.sign_in("token").await;

    let fixture = tempfile::tempdir().unwrap();
    let game_path = fixture.path().join("game.exe");
    std::fs::write(&game_path, b"game").unwrap();
    let game_canonical = std::fs::canonicalize(&game_path).unwrap();

    h.engine
        .add_local_executable(Some(game_canonical.to_string_lossy().into_owned()));
    h.until("game added", |s| {
        s.local_games.registrations.len() == 1 && !s.local_games.busy
    })
    .await;

    let key = store.registrations().unwrap()[0].key();
    eventually(|| h.engine.lock().playtime.ready).await;
    let now = super::super::unix_now() + 10;
    let game_proc = ProcessInfo {
        pid: 300,
        created_at: UNIX_EPOCH + Duration::from_secs((now - 20) as u64),
        exe_path: game_canonical.to_string_lossy().into_owned(),
        parent: None,
    };
    h.processes.start(game_proc.clone());

    // Tracking while hidden: maintenance_at called by repeating timer
    h.engine.maintenance_at(now);
    h.until("game running while hidden", |s| {
        s.tracking.running.contains(&key)
    })
    .await;

    // Advance 60s for checkpoint
    h.engine.maintenance_at(now + 65);
    eventually(|| {
        playtime_store
            .sessions("u1")
            .unwrap()
            .first()
            .is_some_and(|s| s.checkpoint_at >= now + 65)
    })
    .await;

    let sessions_before_crash = playtime_store.sessions("u1").unwrap();
    assert_eq!(sessions_before_crash.len(), 1);
    assert_eq!(sessions_before_crash[0].ended_at, None);
    assert_eq!(sessions_before_crash[0].platform_id, 6);
    assert_eq!(sessions_before_crash[0].source, "manual");

    // Simulate an ungraceful crash: database.recover() recovers the session at checkpoint
    playtime_store.recover().unwrap();
    let recovered = playtime_store.sessions("u1").unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(
        recovered[0].ended_at,
        Some(sessions_before_crash[0].checkpoint_at)
    );

    // Restart engine using the recovered database while the game is STILL running
    let mut h2 = harness(api.clone(), creds.clone());
    {
        let shared = Arc::get_mut(&mut h2.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
        shared.ports.installations = Some(host.clone());
        shared.ports.playtime = playtime_store.clone();
        shared.ports.processes = h.processes.clone();
        shared.ports.library = h.engine.shared.ports.library.clone();
        shared.state.get_mut().unwrap().library = LibraryMachine::new(
            crate::providers::load_bundled(Platform::Windows).providers,
            Default::default(),
        );
    }
    h2.engine.start();
    h2.until("signed in and library scanned on restart", |s| {
        matches!(s.auth, AuthView::SignedIn { .. })
            && s.library
                .as_ref()
                .is_some_and(|l| !l.scanning && l.rows.iter().any(|r| r.key == key))
    })
    .await;

    eventually(|| h2.engine.lock().playtime.ready).await;

    // Now drive maintenance on the restarted engine
    h2.engine.maintenance_at(now + 80);
    h2.until("restarted engine tracking still-running game", |s| {
        s.tracking.running.contains(&key)
    })
    .await;

    // Prior session remains recovered at checkpoint, new session starts from floor without premature backup
    eventually(|| playtime_store.sessions("u1").unwrap().len() == 2).await;
    let sessions_after_restart = playtime_store.sessions("u1").unwrap();
    assert_eq!(
        sessions_after_restart[0].ended_at,
        Some(sessions_before_crash[0].checkpoint_at)
    );
    assert_eq!(sessions_after_restart[1].ended_at, None);
    assert_eq!(
        sessions_after_restart[1].started_at,
        sessions_before_crash[0].checkpoint_at
    );

    // Verify no sessions were ever submitted via HTTP
    assert!(api.playtimes.lock().unwrap().is_empty());

    h.engine.shutdown();
    h2.engine.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn reviewed_candidate_lifecycle_ignores_approval_and_watch_removal() {
    let api = FakeApi::new();
    let creds = FakeCreds::with(None);
    let mut h = harness(api.clone(), creds.clone());
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    let host = Arc::new(Host::default());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
        shared.ports.installations = Some(host.clone());
        shared.state.get_mut().unwrap().library = LibraryMachine::new(
            crate::providers::load_bundled(Platform::Windows).providers,
            Default::default(),
        );
    }
    h.signed_out().await;
    h.sign_in("token").await;

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_path_buf();
    let game_exe = root.join("game.exe");
    std::fs::write(&game_exe, b"game").unwrap();
    let game_canonical = std::fs::canonicalize(&game_exe).unwrap();

    let watch = WatchRoot {
        id: "w1".into(),
        path: root.to_string_lossy().into_owned(),
        enabled: true,
    };
    store.save_watch(&watch).unwrap();

    let candidate = Candidate {
        fingerprint: "fp1".into(),
        watch: "w1".into(),
        root: root.to_string_lossy().into_owned(),
        title: "Discovered Game".into(),
        targets: vec![game_canonical.to_string_lossy().into_owned()],
        selected_target: Some(game_canonical.to_string_lossy().into_owned()),
        evidence: vec![],
        ignored: false,
        incomplete: false,
    };
    let mut registered = host.inspect(game_exe.to_str().unwrap()).unwrap();
    registered.root = root.join("nested").to_string_lossy().into_owned();
    assert!(crate::engine::drm_free::candidate_matches_registration(
        &candidate,
        &registered
    ));
    store.save_candidate(&candidate).unwrap();

    // Reload local view in engine
    h.engine.refresh_local_games();
    h.until("candidate visible in review", |s| {
        s.local_games.candidates.len() == 1
    })
    .await;

    // 1. Candidate has NO effect on library or tracker before approval
    assert_eq!(store.registrations().unwrap().len(), 0);
    assert!(
        !h.engine
            .snapshot()
            .library
            .unwrap()
            .rows
            .iter()
            .any(|r| r.provider == "manual")
    );
    assert!(h.engine.snapshot().tracking.running.is_empty());

    // 2. Select candidate and ignore it
    h.engine.select_local_candidate("fp1".into());
    h.until("candidate selected", |s| {
        s.local_games.review_selected.contains(&"fp1".to_string())
    })
    .await;

    h.engine.review_selected_local_candidates(false);
    h.until("candidate ignored", |s| {
        !s.local_games.busy
            && s.local_games
                .candidates
                .iter()
                .any(|c| c.fingerprint == "fp1" && c.ignored)
    })
    .await;
    assert!(store.candidates().unwrap()[0].ignored);

    // 3. Reset ignores restores candidate to review
    h.engine.reset_local_ignores();
    h.until("candidate restored after reset ignores", |s| {
        !s.local_games.busy
            && s.local_games
                .candidates
                .iter()
                .any(|c| c.fingerprint == "fp1" && !c.ignored)
    })
    .await;
    assert!(!store.candidates().unwrap()[0].ignored);

    // 4. Select and approve (add) candidate
    h.engine.select_local_candidate("fp1".into());
    h.engine.review_selected_local_candidates(true);
    h.until("candidate approved into library", |s| {
        s.local_games.candidates.is_empty()
            && s.local_games.registrations.len() == 1
            && s.library.as_ref().is_some_and(|l| {
                l.rows
                    .iter()
                    .any(|r| r.provider == "manual" && r.title == "Discovered Game")
            })
    })
    .await;
    assert_eq!(store.registrations().unwrap().len(), 1);

    // 5. Remove watch root disables watch while preserving registration in library and store
    h.engine.remove_local_watch("w1".into());
    h.until("watch removed", |s| {
        s.local_games.watches.iter().all(|w| !w.enabled)
    })
    .await;
    assert!(!store.watches().unwrap()[0].enabled);
    assert_eq!(store.registrations().unwrap().len(), 1);
    assert!(
        h.engine
            .snapshot()
            .library
            .unwrap()
            .rows
            .iter()
            .any(|r| r.provider == "manual")
    );

    h.engine.shutdown();
}

#[cfg(not(windows))]
#[tokio::test(flavor = "multi_thread")]
async fn discovery_failure_remains_visible_after_refresh() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    store
        .save_watch(&WatchRoot {
            id: "watch".into(),
            path: "/games".into(),
            enabled: true,
        })
        .unwrap();
    Arc::get_mut(&mut h.engine.shared)
        .unwrap()
        .ports
        .local_games = Some(store);
    h.engine.rescan_local_folders();
    h.until("discovery failure", |s| {
        s.local_games.discovery_completed && !s.local_games.discovery_scanning
    })
    .await;
    assert_eq!(
        h.engine.snapshot().local_games.discovery_error.as_deref(),
        Some("this operation is unavailable on this platform")
    );
    h.engine.refresh_local_games();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(h.engine.snapshot().local_games.discovery_error.is_some());
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires GAMENODE_DISCOVERY_ROOT and the packaged manifest next to the test executable"]
async fn windows_watched_folder_produces_review_candidates() {
    let root = std::env::var("GAMENODE_DISCOVERY_ROOT").unwrap();
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    store
        .save_watch(&WatchRoot {
            id: "watch".into(),
            path: root,
            enabled: true,
        })
        .unwrap();
    Arc::get_mut(&mut h.engine.shared)
        .unwrap()
        .ports
        .local_games = Some(store);
    h.engine.rescan_local_folders();
    assert!(h.engine.snapshot().local_games.discovery_scanning);
    tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            let snapshot = h.engine.snapshot();
            if snapshot.local_games.discovery_completed
                && !snapshot.local_games.discovery_scanning
                && !snapshot.local_games.candidates.is_empty()
            {
                assert!(snapshot.local_games.discovery_error.is_none());
                break;
            }
            if let Some(error) = snapshot.local_games.discovery_error {
                panic!("discovery failed: {error}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
}
