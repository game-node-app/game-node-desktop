use super::*;
use gamenode_core::drm_free::{
    Candidate, Evidence, InstallationHost, LocalError, LocalGameStore, Registration, WatchRoot,
};
use gamenode_core::playtime::Total;
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
            custom_cover: None,
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
    h.engine.remove_local_game(registration.id.clone(), false);
    h.until("removal persisted", |s| {
        !s.local_games.busy && s.local_games.registrations[0].removed
    })
    .await;
    assert!(moved.is_file());
    // Re-adding a previously removed executable reactivates the same registration without duplicate tombstones.
    h.engine
        .add_local_executable(Some(moved.to_string_lossy().into_owned()));
    h.until("reactivation persisted", |s| {
        !s.local_games.busy && !s.local_games.registrations[0].removed
    })
    .await;
    let reactivated = store.registrations().unwrap();
    assert_eq!(reactivated.len(), 1);
    assert_eq!(reactivated[0].id, registration.id);
    assert!(!reactivated[0].removed);
    assert_eq!(reactivated[0].revision, 4);
    h.engine.remove_local_game(registration.id, false);
    h.until("second removal persisted", |s| {
        !s.local_games.busy && s.local_games.registrations[0].removed
    })
    .await;
    h.engine.shutdown();
}

const CLAIM_STEAM_ROOT: &str = "/home/me/.steam/steam/steamapps/common/Portal 2";
const CLAIM_CUSTOM_COVER: &str = "local-claim-0123456789abcdef.webp";

/// A host whose inspected registration lives under the scanned Steam install folder, so adding
/// it produces a tracking conflict without writing to the real filesystem.
struct ClaimHost;

impl InstallationHost for ClaimHost {
    fn inspect(&self, _path: &str) -> Result<Registration, LocalError> {
        Ok(Registration {
            id: "claim-portal".into(),
            title: "Portal 2".into(),
            executable: format!("{CLAIM_STEAM_ROOT}/portal2.exe"),
            file_identity: "claim-portal".into(),
            root: CLAIM_STEAM_ROOT.into(),
            working_directory: CLAIM_STEAM_ROOT.into(),
            arguments: vec![],
            gameplay: vec![],
            launchers: vec![],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            custom_cover: Some(CLAIM_CUSTOM_COVER.into()),
            available: true,
        })
    }

    fn launch(&self, _: &Registration) -> Result<(), LocalError> {
        Ok(())
    }

    fn evidence(&self, _: &Registration) -> Result<Vec<Evidence>, LocalError> {
        Ok(vec![])
    }
}

fn portal_2_match() -> gamenode_core::game_api::ExternalMatch {
    gamenode_core::game_api::ExternalMatch {
        uid: "620".into(),
        game: GameSummary {
            id: 7,
            name: "Portal 2".into(),
            release_year: Some(2011),
            cover_url: Some("//images.igdb.com/igdb/image/upload/t_thumb/co1abc.jpg".into()),
            platforms: Vec::new(),
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claim_carries_both_entries_and_cancel_keeps_store_tracking() {
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    let mut h = harness(api, FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
        shared.ports.installations = Some(Arc::new(ClaimHost));
    }
    h.engine
        .shared
        .ports
        .playtime
        .save_total(&Total {
            account_id: "u1".into(),
            entry_key: "steam:620".into(),
            total_seconds: 15 * 3600,
            last_played_at: None,
            accepted_game_id: None,
            accepted_seconds: None,
            rejected_seconds: None,
        })
        .unwrap();
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("the Steam game, its cover and playtime", |s| {
        s.library.as_ref().is_some_and(|library| {
            library.rows.first().is_some_and(|row| {
                row.key == "steam:620"
                    && row.cover_file.as_deref() == Some("7.jpg")
                    && row.playtime.seconds == Some(15 * 3600)
            })
        })
    })
    .await;

    h.engine.add_local_executable(Some("IGNORED".to_owned()));
    let snapshot = h
        .until("the tracking conflict", |s| {
            s.local_games.pending_claim.is_some() && !s.local_games.busy
        })
        .await;
    let claim = snapshot.local_games.pending_claim.clone().unwrap();
    assert_eq!(claim.store_key, "steam:620");
    assert_eq!(claim.store_entry.title, "Portal 2");
    assert_eq!(claim.store_entry.provider, "steam");
    assert_eq!(claim.store_entry.cover_file.as_deref(), Some("7.jpg"));
    assert_eq!(claim.store_entry.playtime_seconds, Some(15 * 3600));
    assert_eq!(claim.registration.title, "Portal 2");
    assert_eq!(claim.registration.root, CLAIM_STEAM_ROOT);
    assert_eq!(
        claim.registration.cover_file.as_deref(),
        Some(CLAIM_CUSTOM_COVER),
        "the custom cover wins over the matched GameNode cover"
    );
    assert!(
        store.registrations().unwrap().is_empty(),
        "nothing is saved while the conflict waits for a choice"
    );

    h.engine.cancel_local_claim();
    let snapshot = h
        .until("the conflict canceled", |s| {
            s.local_games.pending_claim.is_none()
        })
        .await;
    assert!(store.registrations().unwrap().is_empty());
    let library = snapshot.library.unwrap();
    assert_eq!(library.rows.len(), 1);
    assert_eq!(library.rows[0].key, "steam:620");

    // Using the existing item keeps tracking with the store entry and saves nothing.
    h.engine.add_local_executable(Some("IGNORED".to_owned()));
    h.until("the conflict again", |s| {
        s.local_games.pending_claim.is_some() && !s.local_games.busy
    })
    .await;
    h.engine.resolve_local_claim(false);
    let snapshot = h
        .until("the store entry chosen", |s| {
            s.local_games.pending_claim.is_none()
                && s.library
                    .as_ref()
                    .is_some_and(|library| library.selected.contains(&"steam:620".to_owned()))
        })
        .await;
    assert!(store.registrations().unwrap().is_empty());
    assert_eq!(snapshot.library.unwrap().rows.len(), 1);

    // Moving tracking saves the unsaved registration exactly once, still under the store root.
    h.engine.add_local_executable(Some("IGNORED".to_owned()));
    h.until("the conflict once more", |s| {
        s.local_games.pending_claim.is_some() && !s.local_games.busy
    })
    .await;
    h.engine.resolve_local_claim(true);
    h.until("tracking moved", |s| {
        !s.local_games.busy && s.local_games.registrations.len() == 1
    })
    .await;
    let registrations = store.registrations().unwrap();
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0].id, "claim-portal");
    assert_eq!(
        registrations[0].transferred_from.as_deref(),
        Some("steam:620")
    );
    assert_eq!(
        registrations[0].custom_cover.as_deref(),
        Some(CLAIM_CUSTOM_COVER)
    );
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
            platforms: Vec::new(),
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
    h.until("manual game is available in the library", |s| {
        s.library
            .as_ref()
            .is_some_and(|lib| lib.rows.iter().any(|row| row.key == key))
    })
    .await;
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
    // Signing in re-runs enrichment for the new account. Inferences are stored per
    // account, so wait for u2's own record instead of assuming the write has not
    // happened yet, and prove u1's record was not replaced.
    h.until("u2 enrichment is scoped to u2", |s| {
        matches!(s.auth, AuthView::SignedIn { ref user } if user.id == "u2")
            && s.local_games.inferences.iter().any(|i| i.account == "u2")
    })
    .await;
    let inferences_u2 = store.inferences("u2").unwrap();
    assert!(!inferences_u2.is_empty());
    assert!(inferences_u2.iter().all(|i| i.account == "u2"));
    assert_eq!(store.inferences("u1").unwrap(), inferences_u1);

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

#[tokio::test]
async fn bootstrap_gameplay_learning_without_launcher_classification() {
    gameplay_learning(false).await;
}

async fn gameplay_learning(classified_launcher: bool) {
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
    eventually(|| h.engine.lock().playtime.ready).await;

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
    if classified_launcher {
        reg.launchers = vec![launcher_canonical.to_string_lossy().into_owned()];
        reg.gameplay.clear();
    }
    h.engine.edit_local_game(reg);
    h.until("edit saved", |s| {
        s.local_games.registrations.first().is_some_and(|r| {
            if classified_launcher {
                r.gameplay.is_empty() && !r.launchers.is_empty()
            } else {
                !r.gameplay.is_empty() && r.launchers.is_empty()
            }
        })
    })
    .await;
    eventually(|| !h.engine.lock().playtime.listing).await;

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
    if classified_launcher {
        h.processes.start(launcher_proc.clone());
    }

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

    eventually(|| !h.engine.lock().playtime.listing).await;
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
    eventually(|| !h.engine.lock().playtime.listing).await;
    // The asynchronous exit callback can race the first listing. A second periodic listing
    // observes the persistent launcher after the settling window in either ordering.
    h.engine.maintenance_at(now + 35);
    eventually(|| !h.engine.lock().playtime.listing).await;
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
    h.engine.maintenance_at(now + 55);
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
    h.until("manual row available", |s| {
        s.library
            .as_ref()
            .is_some_and(|v| v.rows.iter().any(|r| r.key == key))
    })
    .await;
    eventually(|| h.engine.lock().playtime.ready).await;
    eventually(|| !h.engine.lock().playtime.listing).await;
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
    h2.until(
        "signed in, library scanned and local games loaded on restart",
        |s| {
            matches!(s.auth, AuthView::SignedIn { .. })
                && s.library
                    .as_ref()
                    .is_some_and(|l| !l.scanning && l.rows.iter().any(|r| r.key == key))
                && s.local_games.registrations.iter().any(|r| r.key() == key)
        },
    )
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
    let launcher_exe = root.join("Launcher.exe");
    let game_exe = root.join("game_x64.exe");
    std::fs::write(&launcher_exe, b"launcher").unwrap();
    std::fs::write(&game_exe, b"game").unwrap();
    let launcher_canonical = std::fs::canonicalize(&launcher_exe).unwrap();
    let game_canonical = std::fs::canonicalize(&game_exe).unwrap();

    let watch = WatchRoot {
        id: "w1".into(),
        path: root.to_string_lossy().into_owned(),
        enabled: true,
        ..Default::default()
    };
    store.save_watch(&watch).unwrap();

    let candidate = Candidate {
        fingerprint: "fp1".into(),
        watch: "w1".into(),
        root: root.to_string_lossy().into_owned(),
        title: "Discovered Game".into(),
        targets: vec![
            launcher_canonical.to_string_lossy().into_owned(),
            game_canonical.to_string_lossy().into_owned(),
        ],
        selected_target: None,
        evidence: vec![],
        ignored: false,
        incomplete: false,
        suggestion: None,
        likely_non_game: false,
    };
    let mut registered = host.inspect(game_exe.to_str().unwrap()).unwrap();
    registered.root = root.join("nested").to_string_lossy().into_owned();
    assert!(crate::engine::drm_free::candidate_matches_registration(
        &candidate,
        &registered
    ));
    store.save_candidate(&candidate).unwrap();

    // 1. Rescanning watched folders auto-selects the preferred target and registers the game,
    // recording its title in `recently_added`.
    h.engine.rescan_local_folders();
    h.until("candidate auto-registered into library", |s| {
        s.local_games.discovery_completed
            && !s.local_games.discovery_scanning
            && s.local_games.registrations.iter().any(|r| !r.removed)
            && s.local_games.recently_added == vec!["Discovered Game".to_string()]
            && s.library.as_ref().is_some_and(|l| {
                l.rows
                    .iter()
                    .any(|r| r.provider == "manual" && r.title == "Discovered Game")
            })
    })
    .await;
    let active_reg = store
        .registrations()
        .unwrap()
        .into_iter()
        .find(|r| !r.removed)
        .unwrap();
    assert_eq!(active_reg.executable, game_canonical.to_string_lossy());
    assert_eq!(
        store.candidates().unwrap()[0].selected_target.as_deref(),
        Some(game_canonical.to_string_lossy().as_ref())
    );

    // 2. Dismissing the discovery banner clears `recently_added`
    h.engine.dismiss_local_discovered();
    h.until("discovered banner dismissed", |s| {
        s.local_games.recently_added.is_empty()
    })
    .await;

    // 3a. Removing with allow_rediscovery = true removes from library immediately but re-imports on next rescan with the same id
    h.engine.remove_local_game(active_reg.id.clone(), true);
    h.until("game removed with rediscovery allowed", |s| {
        !s.local_games.busy
            && s.local_games
                .registrations
                .iter()
                .all(|r| r.removed && r.rediscovery_allowed)
            && s.library
                .as_ref()
                .is_some_and(|l| !l.rows.iter().any(|r| r.provider == "manual"))
    })
    .await;
    assert!(!store.candidates().unwrap()[0].ignored);

    h.engine.rescan_local_folders();
    h.until("game re-imported on rescan with same id", |s| {
        s.local_games.discovery_completed
            && !s.local_games.discovery_scanning
            && s.local_games.registrations.iter().any(|r| !r.removed)
            && s.local_games.recently_added == vec!["Discovered Game".to_string()]
    })
    .await;
    assert_eq!(store.registrations().unwrap().len(), 1);
    assert_eq!(store.registrations().unwrap()[0].id, active_reg.id);
    h.engine.dismiss_local_discovered();
    h.until("discovered banner dismissed again", |s| {
        s.local_games.recently_added.is_empty()
    })
    .await;

    // 3b. Removing with allow_rediscovery = false suppresses automatic re-addition on subsequent rescans
    h.engine.remove_local_game(active_reg.id.clone(), false);
    h.until("game removed from library", |s| {
        !s.local_games.busy
            && s.local_games
                .registrations
                .iter()
                .all(|r| r.removed && !r.rediscovery_allowed)
            && s.library
                .as_ref()
                .is_some_and(|l| !l.rows.iter().any(|r| r.provider == "manual"))
    })
    .await;
    assert!(store.candidates().unwrap()[0].ignored);

    h.engine.rescan_local_folders();
    h.until("rescan completed while suppressed", |s| {
        s.local_games.discovery_completed && !s.local_games.discovery_scanning
    })
    .await;
    assert!(
        store.registrations().unwrap().iter().all(|r| r.removed),
        "removed game must not be re-added on rescan before reset"
    );
    assert!(h.engine.snapshot().local_games.recently_added.is_empty());

    // 4. Resetting ignored discoveries re-enables rediscovery and auto-registers on rescan
    h.engine.reset_local_ignores();
    h.until("game re-added after reset_local_ignores", |s| {
        !s.local_games.busy
            && !s.local_games.discovery_scanning
            && s.local_games.registrations.iter().any(|r| !r.removed)
            && s.local_games.recently_added == vec!["Discovered Game".to_string()]
            && s.library.as_ref().is_some_and(|l| {
                l.rows
                    .iter()
                    .any(|r| r.provider == "manual" && r.title == "Discovered Game")
            })
    })
    .await;
    assert_eq!(store.registrations().unwrap().len(), 1);
    assert!(!store.registrations().unwrap()[0].removed);

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

    // 6. Cancelling a restore preview clears `restore`
    {
        let mut state = h.engine.lock();
        state.local.restore = Some(gamenode_core::drm_free::RestorePlan {
            request: gamenode_core::drm_free::BackupRequest {
                job_id: None,
                profile: gamenode_core::drm_free::SaveProfile {
                    id: "p1".into(),
                    installation: active_reg.id.clone(),
                    revision: 1,
                    title: "Discovered Game".into(),
                    manifest_hash: "h".into(),
                    definition: String::new(),
                    files: vec![],
                    registry: vec![],
                    support: gamenode_core::drm_free::SaveSupport::Supported,
                    automatic: None,
                    retention: None,
                },
                installation: active_reg,
                destination: "/tmp/backups".into(),
                operation: gamenode_core::drm_free::BackupOperation::Restore {
                    backup: "backup-1".into(),
                },
            },
            targets: vec![r"C:\Saves\slot1.dat".into()],
        });
        h.engine.publish(&mut state);
    }
    assert!(h.engine.snapshot().local_games.restore.is_some());
    h.engine.cancel_local_restore();
    assert!(h.engine.snapshot().local_games.restore.is_none());

    h.engine.shutdown();
}

#[cfg(not(windows))]
#[tokio::test(flavor = "multi_thread")]
async fn the_last_watched_folder_check_is_saved_and_reloaded_on_start() {
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
            ..Default::default()
        })
        .unwrap();
    Arc::get_mut(&mut h.engine.shared)
        .unwrap()
        .ports
        .local_games = Some(store.clone());

    h.engine.rescan_local_folders();
    h.until("a completed check with a timestamp", |s| {
        s.local_games.discovery_completed
            && !s.local_games.discovery_scanning
            && s.local_games.discovery_last_checked_at.is_some()
    })
    .await;
    let writes = h.settings.discovery_writes.lock().unwrap().len();
    assert!(writes >= 1);

    // Every completed check writes the setting again.
    h.engine.rescan_local_folders();
    eventually(|| h.settings.discovery_writes.lock().unwrap().len() > writes).await;
    let latest = *h.settings.discovery_last_checked_at.lock().unwrap();

    // A fresh engine over the same settings reads the timestamp at startup.
    let mut restarted = harness(FakeApi::new(), FakeCreds::with(None));
    {
        let shared = Arc::get_mut(&mut restarted.engine.shared).unwrap();
        shared.ports.local_games = Some(store);
        shared.ports.settings = h.settings.clone();
    }
    restarted.engine.start();
    let snapshot = restarted
        .until("the saved check reloaded", |s| {
            s.local_games.discovery_last_checked_at.is_some()
        })
        .await;
    assert_eq!(snapshot.local_games.discovery_last_checked_at, latest);
}

#[cfg(not(windows))]
#[tokio::test(flavor = "multi_thread")]
async fn discovery_failure_remains_visible_after_refresh() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    let fixture = tempfile::tempdir().unwrap();
    store
        .save_watch(&WatchRoot {
            id: "watch".into(),
            path: fixture.path().to_string_lossy().into_owned(),
            enabled: true,
            ..Default::default()
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
            ..Default::default()
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

#[derive(Default)]
struct LibraryRootScanner {
    calls: Mutex<Vec<String>>,
}
impl gamenode_core::drm_free::DiscoveryHost for LibraryRootScanner {
    fn discover(
        &self,
        root: &WatchRoot,
        _cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<gamenode_core::drm_free::DiscoveryReport, LocalError> {
        self.calls.lock().unwrap().push(root.path.clone());
        Ok(Default::default())
    }
}
fn plugin_scan_root(
    path: &std::path::Path,
    id: &str,
) -> crate::plugin_host::library::PluginScanRoot {
    let approved =
        gamenode_platform::plugin_files::approve_root(path, &Default::default()).unwrap();
    let volume = gamenode_platform::volume::volume_info(&approved.path).unwrap();
    crate::plugin_host::library::PluginScanRoot {
        root: gamenode_platform::plugin_storage::PluginLibraryRoot {
            id: id.into(),
            name: "Root".into(),
            path: approved.path.to_str().unwrap().into(),
            volume: approved.identity.volume,
            node: approved.identity.node,
            volume_token: volume.volume_identity_token,
            declared: false,
            watching: true,
        },
        valid: Arc::new(|| Ok(())),
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn plugin_roots_deduplicate_aliases_and_overlap_preserve_legacy_watches_and_skip_unavailable_volumes()
 {
    use crate::plugin_host::library::LibraryHost;
    let base = tempfile::tempdir().unwrap();
    let games = base.path().join("games");
    let child = games.join("child");
    let offline = base.path().join("offline");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir(&offline).unwrap();
    std::fs::write(offline.join("save"), b"keep").unwrap();
    let alias = base.path().join("alias");
    std::os::unix::fs::symlink(&games, &alias).unwrap();
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    store
        .save_watch(&WatchRoot {
            id: "legacy".into(),
            path: alias.to_str().unwrap().into(),
            enabled: true,
            ..Default::default()
        })
        .unwrap();
    let scanner = Arc::new(LibraryRootScanner::default());
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let ports = &mut Arc::get_mut(&mut h.engine.shared).unwrap().ports;
    ports.local_games = Some(store.clone());
    ports.discovery = Some(scanner.clone());
    let host = super::super::PluginLibraryHost {
        shared: Arc::downgrade(&h.engine.shared),
    };
    let unavailable = plugin_scan_root(&offline, "offline");
    let away = offline.with_extension("away");
    std::fs::rename(&offline, &away).unwrap();
    host.set_plugin_roots(
        "plugin/account".into(),
        vec![
            plugin_scan_root(&games, "same"),
            plugin_scan_root(&child, "child"),
            unavailable,
        ],
    )
    .unwrap();
    eventually(|| {
        !h.engine.snapshot().local_games.discovery_scanning
            && scanner.calls.lock().unwrap().len() == 1
    })
    .await;
    assert_eq!(scanner.calls.lock().unwrap()[0], games.to_str().unwrap());
    assert!(!offline.exists());
    assert_eq!(std::fs::read(away.join("save")).unwrap(), b"keep");
    host.set_plugin_roots("plugin/account".into(), vec![])
        .unwrap();
    eventually(|| {
        !h.engine.snapshot().local_games.discovery_scanning
            && scanner.calls.lock().unwrap().len() == 2
    })
    .await;
    assert!(store.watches().unwrap()[0].enabled);
    assert_eq!(store.watches().unwrap()[0].id, "legacy");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_plugin_root_never_reaches_the_core_scanner() {
    use crate::plugin_host::library::LibraryHost;
    let folder = tempfile::tempdir().unwrap();
    let mut root = plugin_scan_root(folder.path(), "revoked");
    root.valid = Arc::new(|| Err(tonic::Status::permission_denied("revoked")));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let scanner = Arc::new(LibraryRootScanner::default());
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let ports = &mut Arc::get_mut(&mut h.engine.shared).unwrap().ports;
    ports.local_games = Some(Arc::new(database.local_game_store()));
    ports.discovery = Some(scanner.clone());
    let host = super::super::PluginLibraryHost {
        shared: Arc::downgrade(&h.engine.shared),
    };
    host.set_plugin_roots("plugin/account".into(), vec![root])
        .unwrap();
    eventually(|| {
        h.engine.snapshot().local_games.discovery_completed
            && !h.engine.snapshot().local_games.discovery_scanning
    })
    .await;
    assert!(scanner.calls.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn discovery_defaults_exclude_likely_non_games_and_keep_manual_choices() {
    let api = FakeApi::new();
    let creds = FakeCreds::with(None);
    let mut h = harness(api, creds);
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
    let game_dir = root.join("Moss & Machine");
    let installer_dir = root.join("Instaladores");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::create_dir_all(&installer_dir).unwrap();
    let game_exe = std::fs::canonicalize({
        let path = game_dir.join("MossMachine.exe");
        std::fs::write(&path, b"game").unwrap();
        path
    })
    .unwrap();
    let other_exe = std::fs::canonicalize({
        let path = game_dir.join("MossMachine64.exe");
        std::fs::write(&path, b"game").unwrap();
        path
    })
    .unwrap();
    let redist = std::fs::canonicalize({
        let path = installer_dir.join("vc_redist.x64.exe");
        std::fs::write(&path, b"tool").unwrap();
        path
    })
    .unwrap();
    let watch = WatchRoot {
        id: "w1".into(),
        path: root.to_string_lossy().into_owned(),
        enabled: true,
        ..Default::default()
    };
    store.save_watch(&watch).unwrap();

    let game = Candidate {
        fingerprint: "game".into(),
        watch: "w1".into(),
        root: game_dir.to_string_lossy().into_owned(),
        title: "Moss & Machine".into(),
        targets: vec![
            game_exe.to_string_lossy().into_owned(),
            other_exe.to_string_lossy().into_owned(),
        ],
        selected_target: Some(game_exe.to_string_lossy().into_owned()),
        evidence: vec![Evidence {
            kind: gamenode_core::drm_free::EvidenceKind::Product,
            value: "Moss & Machine".into(),
            strong: true,
        }],
        ignored: false,
        incomplete: false,
        suggestion: None,
        likely_non_game: false,
    };
    let non_game = Candidate {
        fingerprint: "redist".into(),
        watch: "w1".into(),
        root: installer_dir.to_string_lossy().into_owned(),
        title: "setup_redist".into(),
        targets: vec![redist.to_string_lossy().into_owned()],
        selected_target: Some(redist.to_string_lossy().into_owned()),
        evidence: vec![],
        ignored: false,
        incomplete: false,
        suggestion: None,
        likely_non_game: false,
    };
    store.save_candidate(&game).unwrap();
    store.save_candidate(&non_game).unwrap();

    h.engine.refresh_local_games();
    h.until("review defaults", |snapshot| {
        snapshot.local_games.candidates.len() == 2
            && snapshot
                .local_games
                .review_selected
                .contains(&"game".to_string())
            && !snapshot
                .local_games
                .review_selected
                .contains(&"redist".to_string())
    })
    .await;
    let snapshot = h.engine.snapshot();
    assert!(
        snapshot
            .local_games
            .candidates
            .iter()
            .find(|candidate| candidate.fingerprint == "redist")
            .unwrap()
            .likely_non_game
    );
    // A scan hook never auto-adds a likely non-game; it stays in the review.
    assert!(
        h.engine
            .auto_register_candidates(vec![non_game.clone()])
            .unwrap()
            .is_empty()
    );
    assert!(store.registrations().unwrap().is_empty());

    // A manual uncheck survives later snapshots.
    h.engine.select_local_candidate("game".into());
    h.until("manual uncheck", |snapshot| {
        !snapshot
            .local_games
            .review_selected
            .contains(&"game".to_string())
    })
    .await;
    h.engine
        .set_local_candidate_executable("game".into(), other_exe.to_string_lossy().into_owned());
    h.until("local work finished", |snapshot| !snapshot.local_games.busy)
        .await;
    assert!(
        !h.engine
            .snapshot()
            .local_games
            .review_selected
            .contains(&"game".to_string())
    );

    // A candidate that appears later starts selected without redefining the manual choice.
    let later = Candidate {
        fingerprint: "later".into(),
        watch: "w1".into(),
        root: game_dir.to_string_lossy().into_owned(),
        title: "Frostbyte Arcade".into(),
        targets: vec![other_exe.to_string_lossy().into_owned()],
        selected_target: Some(other_exe.to_string_lossy().into_owned()),
        evidence: vec![Evidence {
            kind: gamenode_core::drm_free::EvidenceKind::Product,
            value: "Frostbyte Arcade".into(),
            strong: true,
        }],
        ignored: false,
        incomplete: false,
        suggestion: None,
        likely_non_game: false,
    };
    store.save_candidate(&later).unwrap();
    h.engine.refresh_local_games();
    h.until("new candidate default", |snapshot| {
        snapshot
            .local_games
            .candidates
            .iter()
            .any(|candidate| candidate.fingerprint == "later")
            && snapshot
                .local_games
                .review_selected
                .contains(&"later".to_string())
            && !snapshot
                .local_games
                .review_selected
                .contains(&"game".to_string())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn discovery_suggestions_are_queued_once_per_title_and_added_as_automatic_matches() {
    let api = FakeApi::new();
    let creds = FakeCreds::with(None);
    let mut h = harness(api.clone(), creds);
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
    *api.search_result.lock().unwrap() = Some(Ok(gamenode_core::game_api::SearchPage {
        games: vec![GameSummary {
            id: 42,
            name: "Moss & Machine".into(),
            release_year: Some(2023),
            cover_url: None,
            platforms: vec![],
        }],
        complete: Some(true),
    }));

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_path_buf();
    let watch = WatchRoot {
        id: "w1".into(),
        path: root.to_string_lossy().into_owned(),
        enabled: true,
        ..Default::default()
    };
    store.save_watch(&watch).unwrap();
    for (fingerprint, title) in [
        ("moss-a", "Moss & Machine"),
        ("moss-b", "Moss & Machine"),
        ("frost", "Frostbyte Arcade"),
    ] {
        let directory = root.join(fingerprint);
        std::fs::create_dir_all(&directory).unwrap();
        let exe = std::fs::canonicalize({
            let path = directory.join(format!("{fingerprint}.exe"));
            std::fs::write(&path, b"game").unwrap();
            path
        })
        .unwrap();
        let candidate = Candidate {
            fingerprint: fingerprint.into(),
            watch: "w1".into(),
            root: directory.to_string_lossy().into_owned(),
            title: title.into(),
            targets: vec![exe.to_string_lossy().into_owned()],
            selected_target: Some(exe.to_string_lossy().into_owned()),
            evidence: vec![Evidence {
                kind: gamenode_core::drm_free::EvidenceKind::Product,
                value: title.into(),
                strong: true,
            }],
            ignored: false,
            incomplete: false,
            suggestion: None,
            likely_non_game: false,
        };
        store.save_candidate(&candidate).unwrap();
    }

    h.engine.refresh_local_games();
    h.until("suggestions applied", |snapshot| {
        snapshot
            .local_games
            .candidates
            .iter()
            .filter(|candidate| candidate.title == "Moss & Machine")
            .all(|candidate| candidate.suggestion.is_some())
    })
    .await;
    eventually(|| {
        store
            .candidates()
            .unwrap()
            .iter()
            .filter(|candidate| candidate.title == "Moss & Machine")
            .all(|candidate| candidate.suggestion.is_some())
    })
    .await;
    // The worker processes one request at a time; wait for the second title before counting.
    eventually(|| {
        api.searches
            .lock()
            .unwrap()
            .iter()
            .any(|query| query == "Frostbyte Arcade")
    })
    .await;
    let searches = api.searches.lock().unwrap().clone();
    assert_eq!(
        searches
            .iter()
            .filter(|query| *query == "Moss & Machine")
            .count(),
        1,
        "searches={searches:?}"
    );
    assert_eq!(
        searches
            .iter()
            .filter(|query| *query == "Frostbyte Arcade")
            .count(),
        1,
        "searches={searches:?}"
    );
    // A later refresh reuses the fingerprint cache and makes no new request.
    let searches_before = api.searches.lock().unwrap().len();
    h.engine.refresh_local_games();
    h.until("suggestions reloaded", |snapshot| {
        snapshot
            .local_games
            .candidates
            .iter()
            .filter(|candidate| candidate.title == "Moss & Machine")
            .all(|candidate| candidate.suggestion.is_some())
    })
    .await;
    assert_eq!(api.searches.lock().unwrap().len(), searches_before);
    let snapshot = h.engine.snapshot();
    let moss = snapshot
        .local_games
        .candidates
        .iter()
        .find(|candidate| candidate.fingerprint == "moss-a")
        .unwrap();
    assert!(
        moss.suggestion.is_some(),
        "state={:?} store={:?}",
        snapshot.local_games.candidates,
        store.candidates()
    );
    assert_eq!(moss.suggestion.as_ref().unwrap().game_id, 42);
    assert!(
        snapshot
            .local_games
            .candidates
            .iter()
            .find(|candidate| candidate.fingerprint == "frost")
            .unwrap()
            .suggestion
            .is_none()
    );

    // Add only the first Moss candidate and check the automatic match it records.
    h.engine.select_local_candidate("moss-b".into());
    h.engine.select_local_candidate("frost".into());
    h.until("only one candidate selected", |snapshot| {
        snapshot.local_games.review_selected == vec!["moss-a".to_string()]
    })
    .await;
    h.engine.review_selected_local_candidates(true, false);
    h.until("candidate added", |snapshot| {
        !snapshot.local_games.busy
            && snapshot
                .local_games
                .registrations
                .iter()
                .any(|registration| !registration.removed)
    })
    .await;
    let registration = store
        .registrations()
        .unwrap()
        .into_iter()
        .find(|registration| !registration.removed)
        .unwrap();
    let key = registration.key();
    h.until("automatic match row", |snapshot| {
        snapshot.library.as_ref().is_some_and(|library| {
            library.rows.iter().any(|row| {
                row.key == key
                    && matches!(
                        row.status,
                        gamenode_core::library::MatchStatus::Matched {
                            source: gamenode_core::library::MatchSource::Automatic,
                            ..
                        }
                    )
            })
        })
    })
    .await;
    assert!(h.engine.lock().library.chosen_match(&key).is_none());
    let account = h.engine.lock().library.account().unwrap().id.clone();
    assert!(store.inferences(&account).unwrap().iter().any(|inference| {
        inference.installation == registration.id && inference.game_id == Some(42)
    }));
}

#[tokio::test(flavor = "multi_thread")]
async fn discovery_suggestions_search_every_pending_title_from_a_bounded_queue() {
    const CANDIDATES: usize = 105;

    let api = FakeApi::new();
    let creds = FakeCreds::with(None);
    let mut h = harness(api.clone(), creds);
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
    // One request every couple of milliseconds, so the queue and in-flight bounds can be seen.
    *api.delay.lock().unwrap() = Duration::from_millis(1);

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_path_buf();
    let watch = WatchRoot {
        id: "w1".into(),
        path: root.to_string_lossy().into_owned(),
        enabled: true,
        ..Default::default()
    };
    store.save_watch(&watch).unwrap();
    for index in 0..CANDIDATES {
        let fingerprint = format!("queued-{index}");
        let title = format!("Queue Fixture {index}");
        let directory = root.join(&fingerprint);
        std::fs::create_dir_all(&directory).unwrap();
        let exe = std::fs::canonicalize({
            let path = directory.join(format!("{fingerprint}.exe"));
            std::fs::write(&path, b"game").unwrap();
            path
        })
        .unwrap();
        let candidate = Candidate {
            fingerprint: fingerprint.clone(),
            watch: "w1".into(),
            root: directory.to_string_lossy().into_owned(),
            title: title.clone(),
            targets: vec![exe.to_string_lossy().into_owned()],
            selected_target: Some(exe.to_string_lossy().into_owned()),
            evidence: vec![Evidence {
                kind: gamenode_core::drm_free::EvidenceKind::Product,
                value: title,
                strong: true,
            }],
            ignored: false,
            incomplete: false,
            suggestion: None,
            likely_non_game: false,
        };
        store.save_candidate(&candidate).unwrap();
    }

    // One refresh is the only trigger: the bounded queue refills itself as it drains.
    h.engine.refresh_local_games();
    assert!(
        h.engine.lock().local_suggestion_queue.len() <= 100,
        "the first batch stays within the bound"
    );

    let mut max_queue = 0;
    let mut max_in_flight = 0;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            {
                let state = h.engine.lock();
                max_queue = max_queue.max(state.local_suggestion_queue.len());
                max_in_flight = max_in_flight.max(state.local_suggestion_in_flight.len());
            }
            if api.searches.lock().unwrap().len() >= CANDIDATES {
                let state = h.engine.lock();
                if state.local_suggestion_in_flight.is_empty()
                    && !state.local_suggestion_running
                    && state.local_suggestion_queue.is_empty()
                {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("every queued title is searched without another refresh");

    assert!(max_queue <= 100, "queue stayed bounded: {max_queue}");
    assert!(max_in_flight <= 1, "one request at a time: {max_in_flight}");
    let searches = api.searches.lock().unwrap().clone();
    assert_eq!(searches.len(), CANDIDATES);
    let unique: std::collections::BTreeSet<&String> = searches.iter().collect();
    assert_eq!(
        unique.len(),
        CANDIDATES,
        "each fingerprint is searched once"
    );
    for index in 0..CANDIDATES {
        assert!(
            searches.contains(&format!("Queue Fixture {index}")),
            "missing title {index}"
        );
    }

    let state = h.engine.lock();
    assert!(state.local_suggestion_queue.is_empty());
    assert!(state.local_suggestion_queued.is_empty());
    assert!(state.local_suggestion_in_flight.is_empty());
    assert!(
        state.local_suggestion_searched.len() >= CANDIDATES,
        "every candidate was answered"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn discovery_suggestions_cache_an_offline_answer_and_do_not_repeat_it() {
    let api = FakeApi::new();
    let creds = FakeCreds::with(None);
    let mut h = harness(api.clone(), creds);
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
    *api.search_result.lock().unwrap() = Some(Err(ApiError::Failed));

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_path_buf();
    let watch = WatchRoot {
        id: "w1".into(),
        path: root.to_string_lossy().into_owned(),
        enabled: true,
        ..Default::default()
    };
    store.save_watch(&watch).unwrap();
    for (fingerprint, title) in [
        ("offline-a", "Offline Alpha"),
        ("offline-b", "Offline Beta"),
    ] {
        let directory = root.join(fingerprint);
        std::fs::create_dir_all(&directory).unwrap();
        let exe = std::fs::canonicalize({
            let path = directory.join(format!("{fingerprint}.exe"));
            std::fs::write(&path, b"game").unwrap();
            path
        })
        .unwrap();
        let candidate = Candidate {
            fingerprint: fingerprint.into(),
            watch: "w1".into(),
            root: directory.to_string_lossy().into_owned(),
            title: title.into(),
            targets: vec![exe.to_string_lossy().into_owned()],
            selected_target: Some(exe.to_string_lossy().into_owned()),
            evidence: vec![Evidence {
                kind: gamenode_core::drm_free::EvidenceKind::Product,
                value: title.into(),
                strong: true,
            }],
            ignored: false,
            incomplete: false,
            suggestion: None,
            likely_non_game: false,
        };
        store.save_candidate(&candidate).unwrap();
    }

    h.engine.refresh_local_games();
    eventually(|| api.searches.lock().unwrap().len() >= 2).await;
    let searches = api.searches.lock().unwrap().clone();
    assert_eq!(searches.len(), 2);
    {
        let state = h.engine.lock();
        assert_eq!(state.local_suggestion_searched.len(), 2);
        assert!(state.local_suggestion_titles.values().all(Option::is_none));
    }

    // The failed answer is cached: a later refresh sends nothing again.
    h.engine.refresh_local_games();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(api.searches.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn discovery_suggestions_make_no_requests_without_a_verified_session() {
    let api = FakeApi::new();
    let creds = FakeCreds::with(None);
    let mut h = harness(api.clone(), creds);
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
    let fixture = tempfile::tempdir().unwrap();
    let exe = std::fs::canonicalize({
        let path = fixture.path().join("offline.exe");
        std::fs::write(&path, b"game").unwrap();
        path
    })
    .unwrap();
    let watch = WatchRoot {
        id: "w1".into(),
        path: fixture.path().to_string_lossy().into_owned(),
        enabled: true,
        ..Default::default()
    };
    store.save_watch(&watch).unwrap();
    store
        .save_candidate(&Candidate {
            fingerprint: "offline".into(),
            watch: "w1".into(),
            root: fixture.path().to_string_lossy().into_owned(),
            title: "Offline Game".into(),
            targets: vec![exe.to_string_lossy().into_owned()],
            selected_target: Some(exe.to_string_lossy().into_owned()),
            evidence: vec![Evidence {
                kind: gamenode_core::drm_free::EvidenceKind::Product,
                value: "Offline Game".into(),
                strong: true,
            }],
            ignored: false,
            incomplete: false,
            suggestion: None,
            likely_non_game: false,
        })
        .unwrap();
    h.engine.refresh_local_games();
    h.until("offline candidate loaded", |snapshot| {
        snapshot.local_games.candidates.len() == 1
    })
    .await;
    assert!(api.searches.lock().unwrap().is_empty());

    // The review still adds the candidate with no request; it starts selected by default.
    h.until("offline candidate selected", |snapshot| {
        snapshot.local_games.review_selected == vec!["offline".to_string()]
    })
    .await;
    h.engine.review_selected_local_candidates(true, false);
    h.until("offline candidate added", |snapshot| {
        !snapshot.local_games.busy
            && snapshot
                .local_games
                .registrations
                .iter()
                .any(|registration| !registration.removed)
    })
    .await;
    assert!(api.searches.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn ignoring_discoveries_deletes_only_rediscoverable_candidates() {
    let api = FakeApi::new();
    let creds = FakeCreds::with(None);
    let mut h = harness(api, creds);
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
    let watch = WatchRoot {
        id: "w1".into(),
        path: fixture.path().to_string_lossy().into_owned(),
        enabled: true,
        ..Default::default()
    };
    store.save_watch(&watch).unwrap();
    let mut candidates = Vec::new();
    for (fingerprint, title) in [
        ("rediscover", "Moss & Machine"),
        ("keep", "Frostbyte Arcade"),
    ] {
        let directory = fixture.path().join(fingerprint);
        std::fs::create_dir_all(&directory).unwrap();
        let exe = std::fs::canonicalize({
            let path = directory.join(format!("{fingerprint}.exe"));
            std::fs::write(&path, b"game").unwrap();
            path
        })
        .unwrap();
        let candidate = Candidate {
            fingerprint: fingerprint.into(),
            watch: "w1".into(),
            root: directory.to_string_lossy().into_owned(),
            title: title.into(),
            targets: vec![exe.to_string_lossy().into_owned()],
            selected_target: Some(exe.to_string_lossy().into_owned()),
            evidence: vec![Evidence {
                kind: gamenode_core::drm_free::EvidenceKind::Product,
                value: title.into(),
                strong: true,
            }],
            ignored: false,
            incomplete: false,
            suggestion: None,
            likely_non_game: false,
        };
        store.save_candidate(&candidate).unwrap();
        candidates.push(candidate);
    }
    h.engine.refresh_local_games();
    h.until("both candidates selected", |snapshot| {
        snapshot.local_games.candidates.len() == 2
            && snapshot.local_games.review_selected.len() == 2
    })
    .await;

    // Ignore the first with rediscovery: its record is forgotten.
    h.engine.select_local_candidate("keep".into());
    h.until("only the rediscoverable candidate selected", |snapshot| {
        snapshot.local_games.review_selected == vec!["rediscover".to_string()]
    })
    .await;
    h.engine.review_selected_local_candidates(false, true);
    h.until("rediscoverable candidate ignored", |snapshot| {
        !snapshot.local_games.busy
            && !snapshot
                .local_games
                .candidates
                .iter()
                .any(|candidate| candidate.fingerprint == "rediscover")
    })
    .await;
    assert!(
        store
            .candidates()
            .unwrap()
            .iter()
            .all(|candidate| candidate.fingerprint != "rediscover")
    );

    // Ignore the second permanently: the record stays, marked ignored.
    h.engine.select_local_candidate("keep".into());
    h.until("permanent candidate selected", |snapshot| {
        snapshot.local_games.review_selected == vec!["keep".to_string()]
    })
    .await;
    h.engine.review_selected_local_candidates(false, false);
    h.until("permanent candidate ignored", |snapshot| {
        !snapshot.local_games.busy
            && snapshot
                .local_games
                .candidates
                .iter()
                .any(|candidate| candidate.fingerprint == "keep" && candidate.ignored)
    })
    .await;
    assert!(
        store
            .candidates()
            .unwrap()
            .iter()
            .any(|candidate| candidate.fingerprint == "keep" && candidate.ignored)
    );

    // A later scan finds the same installation again; the permanently ignored one is not
    // re-proposed, while the forgotten one comes back and starts selected.
    let rediscover = candidates
        .iter()
        .find(|candidate| candidate.fingerprint == "rediscover")
        .unwrap()
        .clone();
    store.save_candidate(&rediscover).unwrap();
    h.engine.rescan_local_folders();
    h.until("rediscoverable candidate proposed again", |snapshot| {
        snapshot.local_games.discovery_completed
            && !snapshot.local_games.discovery_scanning
            && snapshot
                .local_games
                .candidates
                .iter()
                .any(|candidate| candidate.fingerprint == "rediscover" && !candidate.ignored)
            && snapshot
                .local_games
                .review_selected
                .contains(&"rediscover".to_string())
    })
    .await;
    let saved = store.candidates().unwrap();
    assert!(
        saved
            .iter()
            .any(|candidate| candidate.fingerprint == "rediscover" && !candidate.ignored)
    );
    assert!(
        saved
            .iter()
            .any(|candidate| candidate.fingerprint == "keep" && candidate.ignored)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn discovery_suggestions_refill_across_100_boundary_persists_shared_title_matches() {
    const TOTAL: usize = 105;

    let api = FakeApi::new();
    *api.search_result.lock().unwrap() = Some(Ok(gamenode_core::game_api::SearchPage {
        games: vec![GameSummary {
            id: 901,
            name: "Boundary Match".into(),
            release_year: None,
            cover_url: Some("//images.igdb.com/igdb/image/upload/t_thumb/co901.jpg".into()),
            platforms: Vec::new(),
        }],
        complete: Some(true),
    }));
    let creds = FakeCreds::with(None);
    let mut h = harness(api.clone(), creds);
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
    store
        .save_watch(&WatchRoot {
            id: "w1".into(),
            path: root.to_string_lossy().into_owned(),
            enabled: true,
            ..Default::default()
        })
        .unwrap();

    // Candidates 102 and 104 are beyond the 100-item initial queue limit and share one title.
    for index in 0..TOTAL {
        let fingerprint = format!("bound-{index}");
        let title = if index == 102 || index == 104 {
            "Boundary Match".to_string()
        } else {
            format!("Unmatched Fixture {index}")
        };
        let directory = root.join(&fingerprint);
        std::fs::create_dir_all(&directory).unwrap();
        let exe = std::fs::canonicalize({
            let path = directory.join(format!("{fingerprint}.exe"));
            std::fs::write(&path, b"game").unwrap();
            path
        })
        .unwrap();
        store
            .save_candidate(&Candidate {
                fingerprint,
                watch: "w1".into(),
                root: directory.to_string_lossy().into_owned(),
                title: title.clone(),
                targets: vec![exe.to_string_lossy().into_owned()],
                selected_target: Some(exe.to_string_lossy().into_owned()),
                evidence: vec![Evidence {
                    kind: gamenode_core::drm_free::EvidenceKind::Product,
                    value: title,
                    strong: true,
                }],
                ignored: false,
                incomplete: false,
                suggestion: None,
                likely_non_game: false,
            })
            .unwrap();
    }

    h.engine.refresh_local_games();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let done = {
                let state = h.engine.lock();
                !state.local_suggestion_running
                    && state.local_suggestion_queue.is_empty()
                    && state.local_suggestion_in_flight.is_empty()
                    && state.local_suggestion_searched.len() >= TOTAL
            };
            let persisted = store.candidates().unwrap();
            let both_saved = ["bound-102", "bound-104"].iter().all(|fp| {
                persisted
                    .iter()
                    .find(|c| c.fingerprint == *fp)
                    .and_then(|c| c.suggestion.as_ref())
                    .is_some_and(|s| s.game_id == 901)
            });
            if done && both_saved {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("refill searches beyond 100 and persists shared-title matches");

    let searches = api.searches.lock().unwrap().clone();
    assert_eq!(
        searches.iter().filter(|q| *q == "Boundary Match").count(),
        1,
        "shared title beyond the 100-item boundary is searched only once"
    );
    assert_eq!(searches.len(), TOTAL - 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn discovery_suggestions_stop_and_clear_queue_when_session_is_rejected() {
    let api = FakeApi::new();
    *api.search_result.lock().unwrap() = Some(Err(ApiError::Rejected));
    let creds = FakeCreds::with(None);
    let mut h = harness(api.clone(), creds);
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
    store
        .save_watch(&WatchRoot {
            id: "w1".into(),
            path: root.to_string_lossy().into_owned(),
            enabled: true,
            ..Default::default()
        })
        .unwrap();
    for (fingerprint, title) in [("rej-1", "Rejected One"), ("rej-2", "Rejected Two")] {
        let directory = root.join(fingerprint);
        std::fs::create_dir_all(&directory).unwrap();
        let exe = std::fs::canonicalize({
            let path = directory.join(format!("{fingerprint}.exe"));
            std::fs::write(&path, b"game").unwrap();
            path
        })
        .unwrap();
        store
            .save_candidate(&Candidate {
                fingerprint: fingerprint.into(),
                watch: "w1".into(),
                root: directory.to_string_lossy().into_owned(),
                title: title.into(),
                targets: vec![exe.to_string_lossy().into_owned()],
                selected_target: Some(exe.to_string_lossy().into_owned()),
                evidence: vec![Evidence {
                    kind: gamenode_core::drm_free::EvidenceKind::Product,
                    value: title.into(),
                    strong: true,
                }],
                ignored: false,
                incomplete: false,
                suggestion: None,
                likely_non_game: false,
            })
            .unwrap();
    }

    h.engine.refresh_local_games();
    eventually(|| {
        let state = h.engine.lock();
        !api.searches.lock().unwrap().is_empty()
            && !state.local_suggestion_running
            && state.local_suggestion_queue.is_empty()
            && state.local_suggestion_queued.is_empty()
            && state.local_suggestion_in_flight.is_empty()
    })
    .await;

    assert_eq!(api.searches.lock().unwrap().len(), 1);
    let state = h.engine.lock();
    assert!(state.local_suggestion_titles.is_empty());
    assert!(state.local_suggestion_searched.is_empty());
}

#[derive(Default)]
struct ScriptedBackups {
    runs: Mutex<
        Vec<Result<gamenode_core::drm_free::BackupReport, gamenode_core::drm_free::LocalError>>,
    >,
    history: AtomicUsize,
    requests: Mutex<Vec<gamenode_core::drm_free::BackupRequest>>,
}

impl gamenode_core::drm_free::BackupEngine for ScriptedBackups {
    fn execute(
        &self,
        request: &gamenode_core::drm_free::BackupRequest,
        _: &std::sync::atomic::AtomicBool,
    ) -> Result<gamenode_core::drm_free::BackupReport, gamenode_core::drm_free::LocalError> {
        use gamenode_core::drm_free::{BackupOperation, BackupReport, JobState};
        self.requests.lock().unwrap().push(request.clone());
        match &request.operation {
            BackupOperation::Backup => self.runs.lock().unwrap().remove(0),
            BackupOperation::History => {
                let index = self.history.fetch_add(1, Ordering::SeqCst) + 1;
                let title = request.profile.title.clone();
                Ok(BackupReport {
                    state: JobState::Successful,
                    json: serde_json::json!({
                        "games": {
                            title: {
                                "backups": [{
                                    "name": format!("backup-{index}"),
                                    "when": "2026-11-15 14:20:00"
                                }]
                            }
                        }
                    })
                    .to_string(),
                })
            }
            BackupOperation::RestorePreview { .. } => Ok(BackupReport {
                state: JobState::Successful,
                json: serde_json::json!({"overall": {"totalBytes": 7_100_000}}).to_string(),
            }),
            _ => Ok(BackupReport {
                state: JobState::Successful,
                json: "{}".into(),
            }),
        }
    }
}

fn backup_fixture() -> (
    gamenode_core::drm_free::Registration,
    gamenode_core::drm_free::SaveProfile,
) {
    use gamenode_core::drm_free::{Registration, SaveProfile, SaveSupport};
    (
        Registration {
            id: "game".into(),
            title: "Fixture".into(),
            executable: "/games/fixture/fixture.exe".into(),
            file_identity: "fixture".into(),
            root: "/games/fixture".into(),
            working_directory: "/games/fixture".into(),
            arguments: vec![],
            gameplay: vec![],
            launchers: vec![],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            custom_cover: None,
            available: true,
        },
        SaveProfile {
            id: "profile".into(),
            installation: "game".into(),
            revision: 1,
            title: "Fixture".into(),
            manifest_hash: "hash".into(),
            definition: "{}".into(),
            files: vec![],
            registry: vec![],
            support: SaveSupport::Supported,
            automatic: None,
            retention: None,
        },
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn backup_origins_are_recorded_only_for_successful_changed_runs() {
    use gamenode_core::drm_free::{
        BackupJob, BackupReason, BackupReport, BackupSettings, JobState, LocalError,
    };
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    let backups = Arc::new(ScriptedBackups::default());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
        shared.ports.backups = Some(backups.clone());
    }
    let (registration, profile) = backup_fixture();
    store.save_registration(&registration).unwrap();
    store.save_profile(&profile).unwrap();

    let job = |id: &str, period: Option<&str>, seconds: Option<u32>| BackupJob {
        id: id.into(),
        installation: "game".into(),
        profile: "profile".into(),
        revision: 1,
        play_period: period.map(str::to_owned),
        destination: "/backups".into(),
        state: JobState::Queued,
        attempts: 0,
        ready_at: 0,
        backup_id: None,
        error: None,
        play_seconds: seconds,
    };

    // 1. Automatic after-session success records the session length.
    backups.runs.lock().unwrap().push(Ok(BackupReport {
        state: JobState::Successful,
        json: "{}".into(),
    }));
    let automatic = job("automatic", Some("session"), Some(42 * 60));
    store.save_job(&automatic).unwrap();
    {
        let mut state = h.engine.lock();
        state.local.registrations = vec![registration.clone()];
        state.local.profiles = vec![profile.clone()];
        state.local.jobs = vec![automatic.clone()];
        state.local.settings = BackupSettings {
            automatic: true,
            destination: "/backups".into(),
            retention: 10,
        };
        h.engine.drive_local_backups(&mut state, 100);
    }
    eventually(|| {
        store
            .jobs()
            .unwrap()
            .iter()
            .find(|job| job.id == "automatic")
            .is_some_and(|job| job.state == JobState::Successful)
    })
    .await;
    eventually(|| !h.engine.lock().local_backup_busy).await;
    let meta = store.backup_version_meta("game").unwrap();
    assert_eq!(meta.len(), 1);
    assert_eq!(meta[0].reason, BackupReason::AfterSession);
    assert_eq!(meta[0].play_seconds, Some(42 * 60));
    assert_eq!(
        backups
            .requests
            .lock()
            .unwrap()
            .iter()
            .rfind(|request| {
                request.operation == gamenode_core::drm_free::BackupOperation::Backup
            })
            .unwrap()
            .profile
            .retention,
        Some(10),
        "the helper limit follows the settings default"
    );

    // 2. Backup now records manual origin without a session length; the configured
    // retention becomes the helper limit.
    backups.runs.lock().unwrap().push(Ok(BackupReport {
        state: JobState::Successful,
        json: "{}".into(),
    }));
    let manual = job("manual", None, None);
    store.save_job(&manual).unwrap();
    {
        let mut state = h.engine.lock();
        state.local.jobs = store.jobs().unwrap();
        state.local.settings.retention = 50;
        h.engine.drive_local_backups(&mut state, 200);
    }
    eventually(|| store.backup_version_meta("game").unwrap().len() == 2).await;
    eventually(|| !h.engine.lock().local_backup_busy).await;
    let meta = store.backup_version_meta("game").unwrap();
    assert_eq!(meta[1].reason, BackupReason::Manual);
    assert_eq!(meta[1].play_seconds, None);
    assert_eq!(
        backups
            .requests
            .lock()
            .unwrap()
            .iter()
            .rfind(|request| {
                request.operation == gamenode_core::drm_free::BackupOperation::Backup
            })
            .unwrap()
            .profile
            .retention,
        Some(50),
        "the helper limit follows the configured setting"
    );

    // 3. Unchanged data adds no version and no origin row.
    backups.runs.lock().unwrap().push(Ok(BackupReport {
        state: JobState::Unchanged,
        json: "{}".into(),
    }));
    let unchanged = job("unchanged", Some("session-2"), Some(60));
    store.save_job(&unchanged).unwrap();
    {
        let mut state = h.engine.lock();
        state.local.jobs = store.jobs().unwrap();
        h.engine.drive_local_backups(&mut state, 300);
    }
    eventually(|| {
        store
            .jobs()
            .unwrap()
            .iter()
            .find(|job| job.id == "unchanged")
            .is_some_and(|job| job.state == JobState::Unchanged)
    })
    .await;
    eventually(|| !h.engine.lock().local_backup_busy).await;
    assert_eq!(store.backup_version_meta("game").unwrap().len(), 2);

    // 4. A failed run records no origin and stays queued for retry.
    backups
        .runs
        .lock()
        .unwrap()
        .push(Err(LocalError::Failed("disk full".into())));
    let failed = job("failed", Some("session-3"), Some(120));
    store.save_job(&failed).unwrap();
    {
        let mut state = h.engine.lock();
        state.local.jobs = store.jobs().unwrap();
        h.engine.drive_local_backups(&mut state, 400);
    }
    eventually(|| {
        store
            .jobs()
            .unwrap()
            .iter()
            .find(|job| job.id == "failed")
            .is_some_and(|job| job.state == JobState::Queued && job.attempts == 1)
    })
    .await;
    assert_eq!(store.backup_version_meta("game").unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_is_validated_and_isolated_per_installation() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
    }
    let (registration, mut profile) = backup_fixture();
    store.save_registration(&registration).unwrap();
    store.save_profile(&profile).unwrap();
    let mut other = profile.clone();
    other.id = "other-profile".into();
    other.installation = "other".into();
    profile.retention = Some(20);
    store.save_profile(&profile).unwrap();
    store.save_profile(&other).unwrap();

    h.engine
        .set_local_backup_retention("manual:game".into(), 50);
    eventually(|| {
        store
            .profiles()
            .unwrap()
            .iter()
            .find(|profile| profile.id == "profile")
            .is_some_and(|profile| profile.retention == Some(50))
    })
    .await;
    assert_eq!(
        store
            .profiles()
            .unwrap()
            .iter()
            .find(|profile| profile.id == "other-profile")
            .unwrap()
            .retention,
        None
    );

    h.engine.set_local_backup_retention("manual:game".into(), 7);
    eventually(|| h.engine.snapshot().local_games.error.is_some()).await;
    assert_eq!(
        store
            .profiles()
            .unwrap()
            .iter()
            .find(|profile| profile.id == "profile")
            .unwrap()
            .retention,
        Some(50)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn local_custom_cover_commits_survives_a_match_and_cleans_up_on_replace_cancel_and_clear() {
    use gamenode_core::library::store::GameRecord;

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

    let fixture = tempfile::tempdir().unwrap();
    let executable = fixture.path().join("game.exe");
    std::fs::write(&executable, b"fixture").unwrap();
    h.engine
        .add_local_executable(Some(executable.to_string_lossy().into_owned()));
    h.until("local game added", |s| {
        s.local_games.registrations.len() == 1 && !s.local_games.busy
    })
    .await;
    let registration = store.registrations().unwrap().remove(0);
    let key = registration.key();
    let id = registration.id.clone();
    let cover = |hash: &str| format!("local-{id}-{hash}.webp");

    let first = cover("aaaaaaaaaaaaaaaa");
    let second = cover("bbbbbbbbbbbbbbbb");
    let pending = cover("cccccccccccccccc");
    for file in [&first, &second, &pending] {
        std::fs::write(h.covers.path().join(file), b"image").unwrap();
    }

    let mut value = registration.clone();
    value.custom_cover = Some(first.clone());
    h.engine.edit_local_game(value);
    h.until("custom cover committed", |s| {
        !s.local_games.busy
            && s.local_games.registrations[0].custom_cover.as_deref() == Some(&first)
    })
    .await;

    // The library row shows the custom cover, and a later match keeps it.
    h.until("custom cover in the library row", |s| {
        s.library.as_ref().is_some_and(|library| {
            library
                .rows
                .iter()
                .any(|row| row.key == key && row.cover_file.as_deref() == Some(&first))
        })
    })
    .await;
    {
        let mut state = h.engine.lock();
        state.library.set_local_inference(
            key.clone(),
            Some(10),
            Some(GameRecord {
                game_id: 10,
                name: "Matched".into(),
                release_year: Some(2020),
                cover_url: None,
                cover_file: Some("10.jpg".into()),
                cover_failed_at: None,
                platforms: vec![],
            }),
        );
    }
    let row = h
        .engine
        .snapshot()
        .library
        .unwrap()
        .rows
        .iter()
        .find(|row| row.key == key)
        .cloned()
        .unwrap();
    assert_eq!(row.cover_file.as_deref(), Some(first.as_str()));
    assert_eq!(row.game_cover_file.as_deref(), Some("10.jpg"));

    // Replacing the cover deletes the previously committed file.
    let mut value = store.registrations().unwrap().remove(0);
    value.custom_cover = Some(second.clone());
    h.engine.edit_local_game(value);
    h.until("replacement committed", |s| {
        !s.local_games.busy
            && s.local_games.registrations[0].custom_cover.as_deref() == Some(&second)
    })
    .await;
    assert!(!h.covers.path().join(&first).exists());
    assert!(h.covers.path().join(&second).exists());

    // Cancelling an editor deletes the pending file but keeps the committed one.
    h.engine
        .lock()
        .local_cover_pending
        .insert(id.clone(), pending.clone());
    h.engine.discard_local_cover(key.clone());
    assert!(!h.covers.path().join(&pending).exists());
    assert!(h.covers.path().join(&second).exists());

    // Clearing returns to the matched GameNode cover and deletes the custom file.
    let mut value = store.registrations().unwrap().remove(0);
    value.custom_cover = None;
    h.engine.edit_local_game(value);
    h.until("cover cleared", |s| {
        !s.local_games.busy && s.local_games.registrations[0].custom_cover.is_none()
    })
    .await;
    assert!(!h.covers.path().join(&second).exists());
    {
        // The in-memory match this test set is not persisted; re-apply it while checking that the
        // cleared registration falls back to the GameNode cover.
        let mut state = h.engine.lock();
        state.library.set_local_inference(
            key.clone(),
            Some(10),
            Some(GameRecord {
                game_id: 10,
                name: "Matched".into(),
                release_year: Some(2020),
                cover_url: None,
                cover_file: Some("10.jpg".into()),
                cover_failed_at: None,
                platforms: vec![],
            }),
        );
    }
    let row = h
        .engine
        .snapshot()
        .library
        .unwrap()
        .rows
        .iter()
        .find(|row| row.key == key)
        .cloned()
        .unwrap();
    assert_eq!(row.cover_file.as_deref(), Some("10.jpg"));
    assert_eq!(row.game_cover_file.as_deref(), Some("10.jpg"));

    h.engine.shutdown();
}

fn local_discovery_harness() -> (Harness, Arc<dyn LocalGameStore>) {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store: Arc<dyn LocalGameStore> = Arc::new(database.local_game_store());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
        shared.ports.installations = Some(Arc::new(Host::default()));
        shared.ports.platform = Platform::Windows;
    }
    (h, store)
}

#[tokio::test(flavor = "multi_thread")]
async fn discovery_preferences_gate_the_startup_reconcile_and_a_later_reconcile_runs() {
    use gamenode_core::settings::AppPreferencePatch;
    let (mut h, store) = local_discovery_harness();
    let fixture = tempfile::tempdir().unwrap();
    store
        .save_watch(&WatchRoot {
            id: "w".into(),
            path: fixture.path().to_string_lossy().into_owned(),
            enabled: true,
            ..Default::default()
        })
        .unwrap();
    h.engine
        .set_app_preference(AppPreferencePatch::DiscoveryOnStart(false))
        .unwrap();
    h.engine
        .set_app_preference(AppPreferencePatch::DiscoveryRealtime(false))
        .unwrap();
    h.signed_out().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let snapshot = h.engine.snapshot();
    assert_eq!(
        snapshot.local_games.discovery_last_checked_at, None,
        "no watched folder is traversed at start"
    );
    assert!(!snapshot.local_games.discovery_completed || snapshot.local_games.watches.is_empty());

    // Turning real-time watching back on reconciles missed changes once.
    h.engine
        .set_app_preference(AppPreferencePatch::DiscoveryRealtime(true))
        .unwrap();
    h.until("the resume reconcile completes", |s| {
        s.local_games.discovery_last_checked_at.is_some()
            && s.local_games
                .watches
                .iter()
                .any(|watch| watch.last_checked_at.is_some())
    })
    .await;
    let watch = &h.engine.snapshot().local_games.watches[0];
    assert!(watch.last_checked_at.is_some());
    assert_eq!(watch.state, gamenode_core::drm_free::WatchState::Idle);
}

#[tokio::test(flavor = "multi_thread")]
async fn check_at_startup_reconciles_once_with_real_time_watching_off() {
    use gamenode_core::settings::AppPreferencePatch;
    let (mut h, store) = local_discovery_harness();
    let fixture = tempfile::tempdir().unwrap();
    store
        .save_watch(&WatchRoot {
            id: "w".into(),
            path: fixture.path().to_string_lossy().into_owned(),
            enabled: true,
            ..Default::default()
        })
        .unwrap();
    h.engine
        .set_app_preference(AppPreferencePatch::DiscoveryRealtime(false))
        .unwrap();
    h.signed_out().await;
    h.until("the startup reconcile completes", |s| {
        s.local_games.discovery_last_checked_at.is_some()
            && !s.local_games.discovery_scanning
            && s.local_games
                .watches
                .iter()
                .any(|watch| watch.last_checked_at.is_some())
    })
    .await;
    assert!(
        h.engine.snapshot().local_games.watches[0]
            .last_checked_at
            .is_some()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn watched_folders_report_their_count_last_check_and_availability() {
    use gamenode_core::settings::AppPreferencePatch;
    let (mut h, store) = local_discovery_harness();
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("Game");
    std::fs::create_dir_all(&root).unwrap();
    store
        .save_watch(&WatchRoot {
            id: "w".into(),
            path: fixture.path().to_string_lossy().into_owned(),
            enabled: true,
            ..Default::default()
        })
        .unwrap();
    let missing = fixture.path().join("gone");
    store
        .save_watch(&WatchRoot {
            id: "missing".into(),
            path: missing.to_string_lossy().into_owned(),
            enabled: true,
            ..Default::default()
        })
        .unwrap();
    store
        .save_registration(&Registration {
            id: "game".into(),
            title: "Fixture".into(),
            executable: root.join("game.exe").to_string_lossy().into_owned(),
            file_identity: "fixture".into(),
            root: root.to_string_lossy().into_owned(),
            working_directory: root.to_string_lossy().into_owned(),
            arguments: vec![],
            gameplay: vec![],
            launchers: vec![],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            custom_cover: None,
            available: true,
        })
        .unwrap();
    h.engine
        .set_app_preference(AppPreferencePatch::DiscoveryOnStart(false))
        .unwrap();
    h.engine
        .set_app_preference(AppPreferencePatch::DiscoveryRealtime(false))
        .unwrap();
    h.signed_out().await;

    let snapshot = h
        .until("the watched folders are reported", |s| {
            s.local_games.watches.len() == 2
        })
        .await;
    let checked = snapshot
        .local_games
        .watches
        .iter()
        .find(|watch| watch.id == "w")
        .unwrap();
    assert_eq!(checked.entry_count, 1);
    assert_eq!(checked.state, gamenode_core::drm_free::WatchState::Idle);
    assert_eq!(checked.last_checked_at, None);
    let missing = snapshot
        .local_games
        .watches
        .iter()
        .find(|watch| watch.id == "missing")
        .unwrap();
    assert_eq!(
        missing.state,
        gamenode_core::drm_free::WatchState::Unavailable
    );
    assert!(missing.unavailable_since.is_some());

    // An unavailable folder is not traversed and keeps its state.
    let before = missing.unavailable_since;
    h.engine.check_local_watch("missing".into());
    let snapshot = h.engine.snapshot();
    let missing = snapshot
        .local_games
        .watches
        .iter()
        .find(|watch| watch.id == "missing")
        .unwrap();
    assert_eq!(
        missing.state,
        gamenode_core::drm_free::WatchState::Unavailable
    );
    assert_eq!(missing.last_checked_at, None);
    assert_eq!(missing.unavailable_since, before);

    // One folder check reports its completion time and keeps the count.
    h.engine.check_local_watch("w".into());
    let snapshot = h
        .until("the single folder check completes", |s| {
            s.local_games
                .watches
                .iter()
                .find(|watch| watch.id == "w")
                .is_some_and(|watch| {
                    watch.last_checked_at.is_some()
                        && watch.state == gamenode_core::drm_free::WatchState::Idle
                })
        })
        .await;
    let checked = snapshot
        .local_games
        .watches
        .iter()
        .find(|watch| watch.id == "w")
        .unwrap();
    assert_eq!(checked.entry_count, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn backup_settings_save_the_default_retention_with_the_other_fields() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
    }
    h.engine
        .set_local_backup_settings(false, "/backups".into(), 20)
        .await
        .unwrap();
    let settings = store.backup_settings().unwrap();
    assert!(!settings.automatic);
    assert_eq!(settings.destination, "/backups");
    assert_eq!(settings.retention, 20);

    // An unsupported count is refused and leaves every saved field alone.
    assert_eq!(
        h.engine
            .set_local_backup_settings(true, "/other".into(), 7)
            .await,
        Err(gamenode_core::drm_free::LocalError::InvalidTarget)
    );
    let settings = store.backup_settings().unwrap();
    assert!(!settings.automatic);
    assert_eq!(settings.destination, "/backups");
    assert_eq!(settings.retention, 20);
}

#[tokio::test(flavor = "multi_thread")]
async fn backup_settings_failures_keep_the_previous_values_and_report_to_the_caller() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
    }
    // A relative destination is not usable; the save is reported as failed rather than silently
    // accepted after the command returns.
    assert!(
        h.engine
            .set_local_backup_settings(false, "relative/backups".into(), 20)
            .await
            .is_err()
    );
    let settings = store.backup_settings().unwrap();
    assert!(settings.automatic);
    assert_eq!(settings.retention, 10, "all three fields stayed unchanged");
    assert_ne!(settings.destination, "relative/backups");

    // A second request while one local operation is busy is refused, not dropped.
    h.engine.lock().local.busy = true;
    assert_eq!(
        h.engine
            .set_local_backup_settings(true, "/backups".into(), 20)
            .await,
        Err(gamenode_core::drm_free::LocalError::Busy)
    );
    h.engine.lock().local.busy = false;
}

#[tokio::test(flavor = "multi_thread")]
async fn backup_usage_counts_versions_and_bytes_per_game_largest_first() {
    use gamenode_core::drm_free::{BackupSettings, Registration};
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let database = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let store = Arc::new(database.local_game_store());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(store.clone());
    }
    let fixture = tempfile::tempdir().unwrap();
    let destination = fixture.path().join("backups");
    let version = |installation: &str, profile: &str, backup: &str, bytes: usize| {
        let directory = destination
            .join("manual")
            .join(installation)
            .join(profile)
            .join(backup)
            .join("files");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("save.dat"), vec![0u8; bytes]).unwrap();
    };
    version("brasa", "brasa-profile", "2026-11-15 10-00-00", 100);
    version("brasa", "brasa-profile", "2026-11-14 10-00-00", 50);
    version(
        "drift",
        "drift-profile_recovery_mock",
        "2026-11-10 10-00-00",
        30,
    );
    store
        .save_backup_settings(&BackupSettings {
            automatic: true,
            destination: destination.to_string_lossy().into_owned(),
            retention: 10,
        })
        .unwrap();
    for (id, title, removed) in [("brasa", "Brasa", false), ("drift", "Drift", true)] {
        let root = fixture.path().join(id);
        std::fs::create_dir_all(&root).unwrap();
        store
            .save_registration(&Registration {
                id: id.into(),
                title: title.into(),
                executable: root.join("game.exe").to_string_lossy().into_owned(),
                file_identity: id.into(),
                root: root.to_string_lossy().into_owned(),
                working_directory: root.to_string_lossy().into_owned(),
                arguments: vec![],
                gameplay: vec![],
                launchers: vec![],
                revision: 1,
                removed,
                rediscovery_allowed: false,
                transferred_from: None,
                custom_cover: None,
                available: true,
            })
            .unwrap();
    }

    let usage = h.engine.local_backup_usage().unwrap();
    assert_eq!(usage.total_bytes, 180);
    assert_eq!(usage.total_versions, 3);
    assert_eq!(
        usage
            .games
            .iter()
            .map(|game| (
                game.entry_key.as_str(),
                game.title.as_str(),
                game.bytes,
                game.versions
            ))
            .collect::<Vec<_>>(),
        [
            ("manual:brasa", "Brasa", 150, 2),
            ("manual:drift", "Drift", 30, 1),
        ],
        "largest first, removed games keep their last known title"
    );
}
