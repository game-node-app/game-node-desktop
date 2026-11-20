use super::*;
use gamenode_core::drm_free::Registration;
use gamenode_core::tools::{
    CachedToolArchive, InMemoryToolStore, MockToolCatalogApi, MockToolInstallerHost, ProxyDllName,
    ToolChannel, ToolInstallState, ToolInstallation, ToolOperationErrorCode, ToolOperationStatus,
    ToolRelease, ToolStore, optiscaler_descriptor,
};
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;

async fn eventually(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[derive(Default)]
struct FakeToolPicker {
    picks: Mutex<VecDeque<Result<Option<String>, String>>>,
}

impl FakeToolPicker {
    fn push(&self, pick: Result<Option<String>, String>) {
        self.picks.lock().unwrap().push_back(pick);
    }
}

impl crate::engine::tools::ToolTargetPicker for FakeToolPicker {
    fn pick(&self, _install_dir: &str) -> Result<Option<String>, String> {
        self.picks.lock().unwrap().pop_front().unwrap_or(Ok(None))
    }
}

struct ToolsHarness {
    harness: Harness,
    #[allow(dead_code)]
    store: Arc<InMemoryToolStore>,
    #[allow(dead_code)]
    catalog: Arc<MockToolCatalogApi>,
    #[allow(dead_code)]
    installer: Arc<MockToolInstallerHost>,
    picker: Arc<FakeToolPicker>,
    #[allow(dead_code)]
    cache: tempfile::TempDir,
}

fn tools_harness() -> ToolsHarness {
    let api = FakeApi::new();
    let creds = FakeCreds::with(None);
    let files = Arc::new(SlowFiles::steam_with_portal_2());
    let uri = Arc::new(FakeUri::default());
    let covers = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let cache_file = cache.path().join("opti.7z");
    std::fs::write(&cache_file, b"stable cached archive").unwrap();
    let cache_path = cache_file.to_string_lossy().into_owned();
    let playtime_db = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let processes = Arc::new(gamenode_core::tracker::scripted::ScriptedProcesses::default());

    let store = Arc::new(InMemoryToolStore::default());
    let catalog = Arc::new(MockToolCatalogApi::default());
    let installer = Arc::new(MockToolInstallerHost::default());
    let picker = Arc::new(FakeToolPicker::default());

    let opti = optiscaler_descriptor();
    let stable_release = ToolRelease {
        tool_id: opti.id.clone(),
        channel: ToolChannel::Stable,
        version: "0.7.9".to_string(),
        tag_name: "v0.7.9".to_string(),
        name: "OptiScaler v0.7.9".to_string(),
        published_at: "2026-01-01T00:00:00Z".to_string(),
        download_url: "https://example.com/opti.7z".to_string(),
        asset_name: "opti.7z".to_string(),
        size_bytes: 5_000_000,
        sha256: Some("sha_test".to_string()),
        cached_path: Some(cache_path.clone()),
    };
    let nightly_release = ToolRelease {
        tool_id: opti.id.clone(),
        channel: ToolChannel::Nightly,
        version: "0.8.0-pre1".to_string(),
        tag_name: "nightly-2026-02-01".to_string(),
        name: "OptiScaler Nightly 2026-02-01".to_string(),
        published_at: "2026-02-01T00:00:00Z".to_string(),
        download_url: "https://example.com/opti-nightly.7z".to_string(),
        asset_name: "opti-nightly.7z".to_string(),
        size_bytes: 5_500_000,
        sha256: Some("sha_nightly".to_string()),
        cached_path: None,
    };

    *catalog.releases.lock().unwrap() = vec![stable_release.clone(), nightly_release];
    *catalog.cached_archives.lock().unwrap() = vec![CachedToolArchive {
        tool_id: opti.id,
        channel: ToolChannel::Stable,
        version: "0.7.9".to_string(),
        path: cache_path,
        size_bytes: 5_000_000,
        sha256: "sha_test".to_string(),
    }];

    let ports = Ports {
        discovery: None,
        achievements: super::test_achievement_store(),
        backups: None,
        local_games: None,
        installations: None,
        tools: Some(store.clone()),
        tool_catalog: Some(catalog.clone()),
        tool_installer: Some(installer.clone()),
        tool_picker: Some(picker.clone()),
        processes: processes.clone(),
        playtime: Arc::new(playtime_db.playtime_store()),
        playtime_api: api.clone(),
        settings: Arc::new(FakeSettings::default()),
        credentials: creds.clone(),
        api: api.clone(),
        browser: Arc::new(FakeBrowser::default()),
        library: Arc::new(FakeLibrary::default()),
        games: api.clone(),
        files: files.clone(),
        roots: Arc::new(FixedRoots::with_home("/home/me")),
        uri: uri.clone(),
        pc_ports: None,
        port_store: None,
        port_progress: Arc::new(crate::engine::PortProgressPublisher::disabled(
            tokio::runtime::Handle::current(),
        )),
        covers_dir: covers.path().to_owned(),
        platform: Platform::Linux,
    };

    let mut n = 0;
    let nonce: gamenode_core::auth::NonceSource = Box::new(move || {
        n += 1;
        format!("nonce{n}")
    });

    let (tx, rx) = watch::channel(Snapshot {
        local_games: Default::default(),
        playtime_error: false,
        tracking: Default::default(),
        sync: Default::default(),
        playtime_source: Default::default(),
        auth: AuthView::Starting,
        notice: None,
        storage: StorageMethod::SecretService,
        settings: Settings::default(),
        preferences: Default::default(),
        library: None,
        launch: None,
        add_notice: None,
        tools: Default::default(),
        store_accounts: Vec::new(),
        ports: Default::default(),
    });

    let providers = crate::providers::load_bundled(Platform::Linux).providers;
    let engine = Engine::new(
        ports,
        tokio::runtime::Handle::current(),
        Settings::default(),
        LibraryMachine::new(providers, Default::default()),
        nonce,
        move |s| {
            let _ = tx.send(s);
        },
    );

    let harness = Harness {
        processes,
        engine,
        snapshots: rx,
        publishes: Arc::new(AtomicUsize::new(0)),
        api,
        creds,
        browser: Arc::new(FakeBrowser::default()),
        settings: Arc::new(FakeSettings::default()),
        store: Arc::new(FakeLibrary::default()),
        files,
        uri,
        covers,
        achievements: super::test_achievement_store(),
        // Tools tests never read achievements; this probe is not wired to the engine.
        achievement_probe: Arc::default(),
    };

    ToolsHarness {
        harness,
        store,
        catalog,
        installer,
        picker,
        cache,
    }
}

fn add_test_game(h: &ToolsHarness, id: &str, title: &str) {
    add_test_game_at(
        h,
        id,
        title,
        &format!("C:/Games/{title}"),
        &format!("C:/Games/{title}/bin/x64/Game.exe"),
    );
}

fn add_test_game_at(h: &ToolsHarness, id: &str, title: &str, root: &str, executable: &str) {
    let reg = Registration {
        id: id.to_string(),
        title: title.to_string(),
        executable: executable.to_string(),
        file_identity: format!("ident_{id}"),
        root: root.to_string(),
        working_directory: root.to_string(),
        arguments: vec![],
        gameplay: vec![],
        launchers: vec![],
        revision: 1,
        removed: false,
        rediscovery_allowed: false,
        transferred_from: None,
        custom_cover: None,
        available: true,
    };
    h.harness.engine.lock().local.registrations.push(reg);
}

#[tokio::test(flavor = "multi_thread")]
async fn open_tools_modal_inspects_target_and_loads_versions() {
    let th = tools_harness();
    add_test_game(&th, "game1", "Cyberpunk 2077");

    th.harness
        .engine
        .open_tools(Some("manual:game1".to_string()));

    // Wait for background target inspection and release catalog fetch
    eventually(|| {
        let snapshot = th.harness.engine.snapshot();
        if let Some(modal) = &snapshot.tools.modal {
            modal.inspection.is_some() && !modal.available_versions.is_empty()
        } else {
            false
        }
    })
    .await;

    let snapshot = th.harness.engine.snapshot();
    let modal = snapshot.tools.modal.as_ref().unwrap();
    assert_eq!(modal.entry_key, "manual:game1");
    assert_eq!(modal.title, "Cyberpunk 2077");
    assert_eq!(modal.selected_channel, ToolChannel::Stable);
    assert_eq!(modal.available_versions[0].version, "0.7.9");
    assert_eq!(modal.selected_version.as_deref(), Some("0.7.9"));
    assert_eq!(
        modal.inspection.as_ref().unwrap().selected_exe.as_deref(),
        Some("C:/Games/Cyberpunk 2077/bin/x64/Game.exe")
    );

    // Switch to Nightly channel and verify release update
    th.harness.engine.select_tool_channel(ToolChannel::Nightly);
    eventually(|| {
        let snapshot = th.harness.engine.snapshot();
        if let Some(modal) = &snapshot.tools.modal {
            modal.selected_channel == ToolChannel::Nightly
                && modal
                    .available_versions
                    .iter()
                    .any(|r| r.version == "0.8.0-pre1")
        } else {
            false
        }
    })
    .await;

    let snapshot = th.harness.engine.snapshot();
    let modal = snapshot.tools.modal.as_ref().unwrap();
    assert_eq!(modal.selected_version.as_deref(), Some("0.8.0-pre1"));

    // Close tools modal
    th.harness.engine.close_tools();
    assert!(th.harness.engine.snapshot().tools.modal.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn browse_picks_an_executable_inside_the_install_folder_and_validates_it() {
    let th = tools_harness();
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_path_buf();
    std::fs::create_dir_all(root.join("bin/x64")).unwrap();
    let initial = root.join("bin/x64/Game.exe");
    std::fs::write(&initial, b"game").unwrap();
    let picked = root.join("bin/x64/Picked.exe");
    std::fs::write(&picked, b"picked").unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside = outside_dir.path().join("Outside.exe");
    std::fs::write(&outside, b"outside").unwrap();
    let root_str = root.to_string_lossy().into_owned();
    add_test_game_at(
        &th,
        "game1",
        "Picker Game",
        &root_str,
        &initial.to_string_lossy(),
    );

    th.harness
        .engine
        .open_tools(Some("manual:game1".to_string()));
    eventually(|| {
        th.harness
            .engine
            .snapshot()
            .tools
            .modal
            .as_ref()
            .is_some_and(|modal| modal.inspection.is_some())
    })
    .await;

    // The chosen file is validated against the install folder and selected through the same
    // pipeline the candidate list uses.
    th.picker
        .push(Ok(Some(picked.to_string_lossy().into_owned())));
    th.harness.engine.pick_tool_target_exe().await;
    let modal = th.harness.engine.snapshot().tools.modal.unwrap();
    assert_eq!(
        modal.inspection.as_ref().unwrap().selected_exe.as_deref(),
        Some("bin/x64/Picked.exe")
    );
    assert!(modal.operation.is_none(), "{:?}", modal.operation);

    // Cancelling the picker keeps the current target.
    th.picker.push(Ok(None));
    th.harness.engine.pick_tool_target_exe().await;
    let modal = th.harness.engine.snapshot().tools.modal.unwrap();
    assert_eq!(
        modal.inspection.as_ref().unwrap().selected_exe.as_deref(),
        Some("bin/x64/Picked.exe")
    );

    // A file outside the install folder is refused by the existing containment rule.
    th.picker
        .push(Ok(Some(outside.to_string_lossy().into_owned())));
    th.harness.engine.pick_tool_target_exe().await;
    let modal = th.harness.engine.snapshot().tools.modal.unwrap();
    assert_eq!(
        modal.inspection.as_ref().unwrap().selected_exe.as_deref(),
        Some("bin/x64/Picked.exe")
    );
    assert!(matches!(
        modal.operation,
        Some(ToolOperationStatus::Failed {
            code: Some(ToolOperationErrorCode::TargetOutsideInstallDir),
            ..
        })
    ));

    // A later successful pick clears that picker error.
    let other = root.join("bin/x64/Other.exe");
    std::fs::write(&other, b"other").unwrap();
    th.picker
        .push(Ok(Some(other.to_string_lossy().into_owned())));
    th.harness.engine.pick_tool_target_exe().await;
    let modal = th.harness.engine.snapshot().tools.modal.unwrap();
    assert_eq!(
        modal.inspection.as_ref().unwrap().selected_exe.as_deref(),
        Some("bin/x64/Other.exe")
    );
    assert!(modal.operation.is_none());

    // A picker failure keeps the target and reports the failure in the modal.
    th.picker.push(Err("the dialog could not open".to_string()));
    th.harness.engine.pick_tool_target_exe().await;
    let modal = th.harness.engine.snapshot().tools.modal.unwrap();
    assert_eq!(
        modal.inspection.as_ref().unwrap().selected_exe.as_deref(),
        Some("bin/x64/Other.exe")
    );
    assert!(matches!(
        modal.operation,
        Some(ToolOperationStatus::Failed {
            ref message,
            code: Some(ToolOperationErrorCode::PickerFailed),
        }) if message.contains("the dialog could not open")
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn install_update_and_uninstall_optiscaler_lifecycle() {
    let th = tools_harness();
    add_test_game(&th, "game1", "Cyberpunk 2077");

    th.harness
        .engine
        .open_tools(Some("manual:game1".to_string()));
    eventually(|| {
        let snapshot = th.harness.engine.snapshot();
        snapshot
            .tools
            .modal
            .as_ref()
            .is_some_and(|m| m.inspection.is_some() && !m.available_versions.is_empty())
    })
    .await;

    // 1. Install OptiScaler Stable v0.7.9
    th.harness.engine.select_tool_proxy("dxgi.dll".to_string());
    th.harness.engine.install_or_update_tool(false);

    eventually(|| {
        let snapshot = th.harness.engine.snapshot();
        snapshot
            .tools
            .installed_by_entry
            .get("manual:game1")
            .is_some_and(|list| !list.is_empty())
            && snapshot
                .tools
                .modal
                .as_ref()
                .is_some_and(|m| m.operation.is_none())
    })
    .await;

    let snapshot = th.harness.engine.snapshot();
    assert_eq!(
        snapshot.tools.summary_for_entry("manual:game1"),
        "OptiScaler 0.7.9 · dxgi.dll"
    );

    // 2. Update OptiScaler to Nightly 0.8.0-pre1
    th.harness.engine.select_tool_channel(ToolChannel::Nightly);
    eventually(|| {
        let snapshot = th.harness.engine.snapshot();
        snapshot.tools.modal.as_ref().is_some_and(|m| {
            m.selected_channel == ToolChannel::Nightly
                && m.selected_version.as_deref() == Some("0.8.0-pre1")
        })
    })
    .await;

    th.harness.engine.install_or_update_tool(true);
    eventually(|| {
        let snapshot = th.harness.engine.snapshot();
        snapshot
            .tools
            .installed_by_entry
            .get("manual:game1")
            .is_some_and(|list| list.iter().any(|t| t.version == "0.8.0-pre1"))
            && snapshot
                .tools
                .modal
                .as_ref()
                .is_some_and(|m| m.operation.is_none())
    })
    .await;

    let snapshot = th.harness.engine.snapshot();
    assert_eq!(
        snapshot.tools.summary_for_entry("manual:game1"),
        "OptiScaler 0.8.0-pre1 · dxgi.dll"
    );

    // 3. Uninstall OptiScaler
    th.harness.engine.uninstall_tool();
    eventually(|| {
        let snapshot = th.harness.engine.snapshot();
        snapshot
            .tools
            .installed_by_entry
            .get("manual:game1")
            .is_none_or(|list| list.is_empty())
            && snapshot
                .tools
                .modal
                .as_ref()
                .is_some_and(|m| m.operation.is_none())
    })
    .await;

    let snapshot = th.harness.engine.snapshot();
    assert_eq!(snapshot.tools.summary_for_entry("manual:game1"), "");
}

#[tokio::test(flavor = "multi_thread")]
async fn anticheat_detection_blocks_install_until_confirmed() {
    let th = tools_harness();
    // Use a title on the anti-cheat blocklist
    add_test_game(&th, "game_r6", "Tom Clancy's Rainbow Six Siege");

    th.harness
        .engine
        .open_tools(Some("manual:game_r6".to_string()));
    eventually(|| {
        let snapshot = th.harness.engine.snapshot();
        snapshot
            .tools
            .modal
            .as_ref()
            .is_some_and(|m| m.inspection.is_some() && !m.available_versions.is_empty())
    })
    .await;

    let snapshot = th.harness.engine.snapshot();
    let modal = snapshot.tools.modal.as_ref().unwrap();
    assert!(modal.inspection.as_ref().unwrap().anticheat.has_warning());
    assert!(!modal.anticheat_confirmed);

    // Attempting install without confirmation must be blocked with an error status
    th.harness.engine.install_or_update_tool(false);
    let snapshot = th.harness.engine.snapshot();
    let modal = snapshot.tools.modal.as_ref().unwrap();
    assert!(matches!(
        modal.operation,
        Some(ToolOperationStatus::Failed { .. })
    ));
    assert!(
        snapshot
            .tools
            .installed_by_entry
            .get("manual:game_r6")
            .is_none_or(|l| l.is_empty())
    );

    // Confirm anti-cheat and install again
    th.harness.engine.set_tool_anticheat_confirmed(true);
    let snapshot = th.harness.engine.snapshot();
    assert!(snapshot.tools.modal.as_ref().unwrap().anticheat_confirmed);

    th.harness.engine.install_or_update_tool(false);
    eventually(|| {
        let snapshot = th.harness.engine.snapshot();
        snapshot
            .tools
            .installed_by_entry
            .get("manual:game_r6")
            .is_some_and(|list| !list.is_empty())
    })
    .await;

    let snapshot = th.harness.engine.snapshot();
    assert_eq!(
        snapshot.tools.summary_for_entry("manual:game_r6"),
        "OptiScaler 0.7.9 · dxgi.dll"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cache_entries_list_cached_versions_and_mark_the_installed_one_in_use() {
    let th = tools_harness();
    let opti_id = optiscaler_descriptor().id;
    let stable_file = th.cache.path().join("opti-0.7.9.7z");
    let nightly_file = th.cache.path().join("opti-nightly.7z");
    std::fs::write(&stable_file, b"stable archive").unwrap();
    std::fs::write(&nightly_file, b"nightly archive").unwrap();
    let stable_path = stable_file.to_string_lossy().into_owned();
    let nightly_path = nightly_file.to_string_lossy().into_owned();
    {
        let mut releases = th.store.releases.lock().unwrap();
        releases.extend([
            ToolRelease {
                tool_id: opti_id.clone(),
                channel: ToolChannel::Stable,
                version: "0.7.9".to_string(),
                tag_name: "v0.7.9".to_string(),
                name: "OptiScaler v0.7.9".to_string(),
                published_at: "2026-01-01T00:00:00Z".to_string(),
                download_url: "https://example.com/opti-0.7.9.7z".to_string(),
                asset_name: "opti-0.7.9.7z".to_string(),
                size_bytes: 5_000_000,
                sha256: Some("sha_stable".to_string()),
                cached_path: Some(stable_path),
            },
            ToolRelease {
                tool_id: opti_id.clone(),
                channel: ToolChannel::Stable,
                version: "0.7.3".to_string(),
                tag_name: "v0.7.3".to_string(),
                name: "OptiScaler v0.7.3".to_string(),
                published_at: "2025-12-01T00:00:00Z".to_string(),
                download_url: "https://example.com/opti-0.7.3.7z".to_string(),
                asset_name: "opti-0.7.3.7z".to_string(),
                size_bytes: 4_500_000,
                sha256: Some("sha_old".to_string()),
                cached_path: None,
            },
            ToolRelease {
                tool_id: opti_id.clone(),
                channel: ToolChannel::Nightly,
                version: "0.8.0-pre1".to_string(),
                tag_name: "nightly-2026-02-01".to_string(),
                name: "OptiScaler Nightly 2026-02-01".to_string(),
                published_at: "2026-02-01T00:00:00Z".to_string(),
                download_url: "https://example.com/opti-nightly.7z".to_string(),
                asset_name: "opti-nightly.7z".to_string(),
                size_bytes: 5_500_000,
                sha256: None,
                cached_path: Some(nightly_path),
            },
        ]);
    }
    th.store
        .installations
        .lock()
        .unwrap()
        .push(ToolInstallation {
            entry_key: "manual:game1".to_string(),
            tool_id: opti_id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "Game.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            state: ToolInstallState::Installed,
            installed_at: 0,
            updated_at: 0,
        });

    th.harness.engine.recover_and_load_tools();
    eventually(|| th.harness.engine.snapshot().tools.cache_entries.len() == 2).await;

    let entries = th.harness.engine.snapshot().tools.cache_entries;
    let stable = entries
        .iter()
        .find(|entry| entry.version == "0.7.9")
        .expect("the installed stable version is cached");
    assert_eq!(stable.channel, ToolChannel::Stable);
    assert_eq!(stable.bytes, 5_000_000);
    assert!(stable.sha256_verified);
    assert!(stable.in_use);

    let nightly = entries
        .iter()
        .find(|entry| entry.version == "0.8.0-pre1")
        .expect("the cached nightly is listed");
    assert_eq!(nightly.channel, ToolChannel::Nightly);
    assert!(!nightly.sha256_verified, "no digest was published");
    assert!(!nightly.in_use);
    assert!(
        entries.iter().all(|entry| entry.version != "0.7.3"),
        "an archive that is not cached is not listed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_tool_checks_run_at_start_and_at_most_once_a_day() {
    let th = tools_harness();
    let now = Instant::now();
    th.harness.engine.tool_auto_check_at(now);
    eventually(|| {
        th.catalog
            .fetch_calls
            .lock()
            .unwrap()
            .iter()
            .any(|(tool, channel)| tool.as_str() == "optiscaler" && *channel == ToolChannel::Stable)
    })
    .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while th.harness.engine.tool_auto_check_is_running() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let first = th.catalog.fetch_calls.lock().unwrap().len();

    // Less than a day later no new release fetch runs.
    th.harness
        .engine
        .tool_auto_check_at(now + Duration::from_secs(23 * 60 * 60));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(th.catalog.fetch_calls.lock().unwrap().len(), first);

    // A day later the check runs again.
    th.harness
        .engine
        .tool_auto_check_at(now + Duration::from_secs(24 * 60 * 60));
    eventually(|| th.catalog.fetch_calls.lock().unwrap().len() > first).await;
    assert!(
        th.catalog.download_calls.lock().unwrap().is_empty(),
        "automatic checks only refresh release metadata; they never download"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_tool_checks_off_never_fetch_releases_in_the_background() {
    let th = tools_harness();
    th.harness
        .engine
        .set_app_preference(AppPreferencePatch::ToolAutoCheck(false))
        .unwrap();
    th.harness.engine.tool_auto_check_at(Instant::now());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        th.catalog.fetch_calls.lock().unwrap().is_empty(),
        "automatic checks off must not fetch release lists"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_check_publishes_a_newer_stable_release_to_the_open_tool_view() {
    let th = tools_harness();
    add_test_game(&th, "game1", "Cyberpunk 2077");
    let opti_id = optiscaler_descriptor().id;
    th.store
        .installations
        .lock()
        .unwrap()
        .push(ToolInstallation {
            entry_key: "manual:game1".to_string(),
            tool_id: opti_id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "bin/x64/Game.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            state: ToolInstallState::Installed,
            installed_at: 0,
            updated_at: 0,
        });
    th.harness.engine.recover_and_load_tools();
    eventually(|| {
        th.harness
            .engine
            .snapshot()
            .tools
            .installed_by_entry
            .get("manual:game1")
            .is_some_and(|tools| tools.iter().any(|tool| tool.version == "0.7.9"))
    })
    .await;

    th.harness
        .engine
        .open_tools(Some("manual:game1".to_string()));
    eventually(|| {
        th.harness
            .engine
            .snapshot()
            .tools
            .modal
            .as_ref()
            .is_some_and(|modal| {
                modal.entry_key == "manual:game1"
                    && modal.selected_channel == ToolChannel::Stable
                    && modal
                        .available_versions
                        .iter()
                        .any(|release| release.version == "0.7.9")
            })
    })
    .await;
    let modal = th.harness.engine.snapshot().tools.modal.unwrap();
    assert_eq!(modal.selected_version.as_deref(), Some("0.7.9"));

    // Removing a downloaded archive must leave an upstream release available to download again.
    let cached_path = modal
        .available_versions
        .iter()
        .find(|release| release.version == "0.7.9")
        .and_then(|release| release.cached_path.as_ref())
        .unwrap();
    std::fs::remove_file(cached_path).unwrap();

    // Upstream publishes 0.7.10 after the modal is already open. The automatic check must publish
    // it into that view without downloading anything or waiting for a manual refresh.
    {
        let opti = optiscaler_descriptor();
        let mut releases = th.catalog.releases.lock().unwrap();
        releases.retain(|release| {
            !(release.tool_id == opti.id && release.channel == ToolChannel::Stable)
        });
        releases.push(ToolRelease {
            tool_id: opti.id,
            channel: ToolChannel::Stable,
            version: "0.7.10".to_string(),
            tag_name: "v0.7.10".to_string(),
            name: "OptiScaler v0.7.10".to_string(),
            published_at: "2026-03-01T00:00:00Z".to_string(),
            download_url: "https://example.com/opti-0.7.10.7z".to_string(),
            asset_name: "opti-0.7.10.7z".to_string(),
            size_bytes: 5_200_000,
            sha256: Some("sha_new".to_string()),
            cached_path: None,
        });
        releases.push(ToolRelease {
            tool_id: opti_id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            tag_name: "v0.7.9".to_string(),
            name: "OptiScaler v0.7.9".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: "https://example.com/opti.7z".to_string(),
            asset_name: "opti.7z".to_string(),
            size_bytes: 5_000_000,
            sha256: Some("sha_test".to_string()),
            cached_path: Some(
                th.cache
                    .path()
                    .join("opti.7z")
                    .to_string_lossy()
                    .into_owned(),
            ),
        });
    }
    let before = Instant::now();
    th.harness.engine.tool_auto_check_at(Instant::now());
    eventually(|| {
        th.harness
            .engine
            .snapshot()
            .tools
            .modal
            .as_ref()
            .is_some_and(|modal| {
                modal
                    .available_versions
                    .first()
                    .is_some_and(|release| release.version == "0.7.10")
            })
    })
    .await;

    let snapshot = th.harness.engine.snapshot();
    let modal = snapshot.tools.modal.as_ref().unwrap();
    assert_eq!(modal.entry_key, "manual:game1");
    assert_eq!(modal.selected_channel, ToolChannel::Stable);
    assert_eq!(
        modal.selected_version.as_deref(),
        Some("0.7.9"),
        "the installed version stays selected while the update is offered"
    );
    assert!(
        modal
            .available_versions
            .iter()
            .find(|release| release.version == "0.7.9")
            .unwrap()
            .cached_path
            .is_none(),
        "a deleted archive can be downloaded again"
    );
    assert!(
        snapshot
            .tools
            .installed_by_entry
            .get("manual:game1")
            .is_some_and(|tools| tools.iter().any(|tool| tool.version == "0.7.9"))
    );
    assert!(
        th.catalog.download_calls.lock().unwrap().is_empty(),
        "automatic checks must never download an archive"
    );
    let last_fetch = th
        .harness
        .engine
        .lock()
        .tool_last_fetch
        .get(&(opti_id, ToolChannel::Stable))
        .copied();
    assert!(
        last_fetch.is_some_and(|fetched| fetched >= before),
        "the automatic fetch updates the release freshness marker"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_default_channel_preselects_the_install_channel() {
    let th = tools_harness();
    th.harness
        .engine
        .set_app_preference(AppPreferencePatch::ToolDefaultChannel(ToolChannel::Nightly))
        .unwrap();
    add_test_game(&th, "game1", "Cyberpunk 2077");

    th.harness
        .engine
        .open_tools(Some("manual:game1".to_string()));
    let modal = th.harness.engine.snapshot().tools.modal.unwrap();
    assert_eq!(modal.selected_channel, ToolChannel::Nightly);

    eventually(|| {
        th.harness
            .engine
            .snapshot()
            .tools
            .modal
            .as_ref()
            .is_some_and(|modal| {
                modal
                    .available_versions
                    .iter()
                    .any(|release| release.version == "0.8.0-pre1")
            })
    })
    .await;
    let modal = th.harness.engine.snapshot().tools.modal.unwrap();
    assert_eq!(modal.selected_channel, ToolChannel::Nightly);
    assert_eq!(modal.selected_version.as_deref(), Some("0.8.0-pre1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn cache_listing_matches_physical_archives_and_marks_in_use_by_channel() {
    let th = tools_harness();
    let opti_id = optiscaler_descriptor().id;
    let stable_file = th.cache.path().join("stable-0.7.9.7z");
    let old_file = th.cache.path().join("stable-0.7.3.7z");
    let nightly_file = th.cache.path().join("nightly-0.7.9.7z");
    std::fs::write(&stable_file, vec![1u8; 128]).unwrap();
    std::fs::write(&old_file, vec![2u8; 64]).unwrap();
    std::fs::write(&nightly_file, vec![3u8; 32]).unwrap();
    let icon_dir =
        gamenode_platform::achievement_icons::achievement_icon_dir(th.harness.covers.path());
    std::fs::create_dir_all(&icon_dir).unwrap();
    std::fs::write(icon_dir.join("icon.png"), vec![4u8; 7]).unwrap();

    {
        let mut releases = th.store.releases.lock().unwrap();
        releases.extend([
            ToolRelease {
                tool_id: opti_id.clone(),
                channel: ToolChannel::Stable,
                version: "0.7.9".to_string(),
                tag_name: "v0.7.9".to_string(),
                name: "OptiScaler v0.7.9".to_string(),
                published_at: "2026-01-01T00:00:00Z".to_string(),
                download_url: "https://example.com/0.7.9.7z".to_string(),
                asset_name: "0.7.9.7z".to_string(),
                size_bytes: 128,
                sha256: Some("sha_stable".to_string()),
                cached_path: Some(stable_file.to_string_lossy().into_owned()),
            },
            ToolRelease {
                tool_id: opti_id.clone(),
                channel: ToolChannel::Stable,
                version: "0.7.3".to_string(),
                tag_name: "v0.7.3".to_string(),
                name: "OptiScaler v0.7.3".to_string(),
                published_at: "2025-12-01T00:00:00Z".to_string(),
                download_url: "https://example.com/0.7.3.7z".to_string(),
                asset_name: "0.7.3.7z".to_string(),
                size_bytes: 64,
                sha256: Some("sha_old".to_string()),
                cached_path: Some(old_file.to_string_lossy().into_owned()),
            },
            ToolRelease {
                tool_id: opti_id.clone(),
                channel: ToolChannel::Nightly,
                version: "0.7.9".to_string(),
                tag_name: "nightly-2026-02-01".to_string(),
                name: "OptiScaler Nightly".to_string(),
                published_at: "2026-02-01T00:00:00Z".to_string(),
                download_url: "https://example.com/nightly.7z".to_string(),
                asset_name: "nightly.7z".to_string(),
                size_bytes: 32,
                sha256: None,
                cached_path: Some(nightly_file.to_string_lossy().into_owned()),
            },
        ]);
    }
    th.store
        .installations
        .lock()
        .unwrap()
        .push(ToolInstallation {
            entry_key: "manual:game1".to_string(),
            tool_id: opti_id,
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "Game.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            state: ToolInstallState::Installed,
            installed_at: 0,
            updated_at: 0,
        });

    th.harness.engine.recover_and_load_tools();
    eventually(|| th.harness.engine.snapshot().tools.cache_entries.len() == 3).await;

    let snapshot = th.harness.engine.snapshot();
    let tools = &snapshot.tools;
    assert_eq!(tools.total_cache_bytes, 128 + 64 + 32);
    assert_eq!(
        tools.total_cache_bytes,
        tools
            .cache_entries
            .iter()
            .map(|entry| entry.bytes)
            .sum::<u64>(),
        "the cache total is exactly the sum of the listed archives"
    );
    assert_eq!(tools.total_cache_archives, 3);
    assert_eq!(tools.achievement_icon_cache_bytes, 7);
    let stable = tools
        .cache_entries
        .iter()
        .find(|entry| entry.channel == ToolChannel::Stable && entry.version == "0.7.9")
        .unwrap();
    assert!(stable.in_use);
    let nightly = tools
        .cache_entries
        .iter()
        .find(|entry| entry.channel == ToolChannel::Nightly && entry.version == "0.7.9")
        .unwrap();
    assert!(
        !nightly.in_use,
        "a same-version nightly is not in use just because a stable install matches"
    );

    // A file removed outside the application disappears from the listing, and the total and count
    // follow it. Achievement icons stay separately accounted.
    std::fs::remove_file(&old_file).unwrap();
    th.harness.engine.recover_and_load_tools();
    eventually(|| th.harness.engine.snapshot().tools.cache_entries.len() == 2).await;
    let tools = th.harness.engine.snapshot().tools;
    assert_eq!(tools.total_cache_bytes, 128 + 32);
    assert_eq!(tools.total_cache_archives, 2);
    assert_eq!(tools.achievement_icon_cache_bytes, 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cached_version_dropped_by_upstream_stays_listed_after_an_automatic_refresh() {
    let th = tools_harness();
    let opti_id = optiscaler_descriptor().id;
    let old_file = th.cache.path().join("old-0.7.3.7z");
    std::fs::write(&old_file, vec![1u8; 64]).unwrap();
    th.store.releases.lock().unwrap().push(ToolRelease {
        tool_id: opti_id.clone(),
        channel: ToolChannel::Stable,
        version: "0.7.3".to_string(),
        tag_name: "v0.7.3".to_string(),
        name: "OptiScaler v0.7.3".to_string(),
        published_at: "2025-12-01T00:00:00Z".to_string(),
        download_url: "https://example.com/0.7.3.7z".to_string(),
        asset_name: "0.7.3.7z".to_string(),
        size_bytes: 64,
        sha256: Some("sha_old".to_string()),
        cached_path: Some(old_file.to_string_lossy().into_owned()),
    });
    th.harness.engine.recover_and_load_tools();
    eventually(|| th.harness.engine.snapshot().tools.cache_entries.len() == 1).await;

    // Upstream no longer lists 0.7.3. The automatic refresh replaces the release list but keeps
    // the local archive row, so Settings still shows the cached version until the cache is cleared.
    {
        let opti = optiscaler_descriptor();
        let mut releases = th.catalog.releases.lock().unwrap();
        releases.retain(|release| {
            !(release.tool_id == opti.id && release.channel == ToolChannel::Stable)
        });
        releases.push(ToolRelease {
            tool_id: opti.id,
            channel: ToolChannel::Stable,
            version: "0.7.10".to_string(),
            tag_name: "v0.7.10".to_string(),
            name: "OptiScaler v0.7.10".to_string(),
            published_at: "2026-03-01T00:00:00Z".to_string(),
            download_url: "https://example.com/0.7.10.7z".to_string(),
            asset_name: "0.7.10.7z".to_string(),
            size_bytes: 256,
            sha256: Some("sha_new".to_string()),
            cached_path: None,
        });
    }
    th.harness.engine.tool_auto_check_at(Instant::now());
    eventually(|| {
        th.harness
            .engine
            .lock()
            .tool_last_fetch
            .contains_key(&(optiscaler_descriptor().id, ToolChannel::Stable))
    })
    .await;
    eventually(|| {
        th.store
            .load_releases(&optiscaler_descriptor().id, ToolChannel::Stable)
            .unwrap()
            .iter()
            .any(|release| release.version == "0.7.10")
    })
    .await;

    let tools = th.harness.engine.snapshot().tools;
    assert_eq!(tools.cache_entries.len(), 1);
    assert_eq!(tools.cache_entries[0].version, "0.7.3");
    assert_eq!(tools.total_cache_bytes, 64);
    assert_eq!(tools.total_cache_archives, 1);
    assert!(
        th.catalog.download_calls.lock().unwrap().is_empty(),
        "refreshing release metadata never downloads an archive"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn clear_tool_cache_resets_bytes_and_archives() {
    let th = tools_harness();
    add_test_game(&th, "game1", "Cyberpunk 2077");
    let opti_id = optiscaler_descriptor().id;
    let stable_file = th.cache.path().join("opti.7z");
    th.store.releases.lock().unwrap().push(ToolRelease {
        tool_id: opti_id,
        channel: ToolChannel::Stable,
        version: "0.7.9".to_string(),
        tag_name: "v0.7.9".to_string(),
        name: "OptiScaler v0.7.9".to_string(),
        published_at: "2026-01-01T00:00:00Z".to_string(),
        download_url: "https://example.com/opti.7z".to_string(),
        asset_name: "opti.7z".to_string(),
        size_bytes: 5_000_000,
        sha256: Some("sha_test".to_string()),
        cached_path: Some(stable_file.to_string_lossy().into_owned()),
    });

    let icon_dir =
        gamenode_platform::achievement_icons::achievement_icon_dir(th.harness.covers.path());
    std::fs::create_dir_all(&icon_dir).unwrap();
    std::fs::write(icon_dir.join(format!("{}.img", "a".repeat(64))), b"icon").unwrap();
    th.harness.engine.recover_and_load_tools();
    eventually(|| th.harness.engine.snapshot().tools.total_cache_bytes > 0).await;

    let snapshot = th.harness.engine.snapshot();
    assert_eq!(snapshot.tools.total_cache_bytes, 5_000_000);
    assert_eq!(snapshot.tools.total_cache_archives, 1);
    assert_eq!(snapshot.tools.achievement_icon_cache_bytes, 4);

    th.harness.engine.clear_tool_cache();
    eventually(|| th.harness.engine.snapshot().tools.total_cache_bytes == 0).await;

    let snapshot = th.harness.engine.snapshot();
    assert_eq!(snapshot.tools.total_cache_bytes, 0);
    assert_eq!(snapshot.tools.total_cache_archives, 0);
    assert_eq!(snapshot.tools.achievement_icon_cache_bytes, 0);
    assert!(snapshot.tools.cache_entries.is_empty());
    assert!(
        !icon_dir.join(format!("{}.img", "a".repeat(64))).exists(),
        "clearing the settings cache removes achievement icons"
    );
}

fn plugin_review(remove: bool) -> gamenode_core::tools::ToolPluginRequest {
    gamenode_core::tools::ToolPluginRequest {
        plugin_id: "fixture.plugin".into(),
        publisher_fingerprint: "publisher-fingerprint".into(),
        remove,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_tool_review_waits_for_user_selection_and_returns_the_real_operation_result() {
    let th = tools_harness();
    add_test_game(&th, "plugin", "Fixture");
    let engine = th.harness.engine.clone();
    let review = tokio::spawn(async move {
        engine
            .review_plugin_tool(
                "manual:plugin".into(),
                "optiscaler".into(),
                plugin_review(false),
                Arc::new(|| Ok(())),
            )
            .await
    });
    eventually(|| {
        th.harness
            .engine
            .snapshot()
            .tools
            .modal
            .as_ref()
            .is_some_and(|m| m.inspection.is_some() && !m.available_versions.is_empty())
    })
    .await;
    let modal = th.harness.engine.snapshot().tools.modal.unwrap();
    assert_eq!(
        modal.plugin_request.as_ref().unwrap().plugin_id,
        "fixture.plugin"
    );
    assert!(th.store.list_installations().unwrap().is_empty());
    th.harness.engine.uninstall_tool(); // An apply request never authorizes removal.
    assert!(
        th.harness
            .engine
            .snapshot()
            .tools
            .modal
            .unwrap()
            .operation
            .is_none()
    );
    th.harness.engine.set_tool_anticheat_confirmed(true);
    th.harness.engine.install_or_update_tool(false);
    review.await.unwrap().unwrap();
    assert_eq!(th.store.list_installations().unwrap().len(), 1);
    th.harness.engine.close_tools();
    let engine = th.harness.engine.clone();
    let removal = tokio::spawn(async move {
        engine
            .review_plugin_tool(
                "manual:plugin".into(),
                "optiscaler".into(),
                plugin_review(true),
                Arc::new(|| Ok(())),
            )
            .await
    });
    eventually(|| {
        th.harness
            .engine
            .snapshot()
            .tools
            .modal
            .as_ref()
            .is_some_and(|m| m.plugin_request.as_ref().is_some_and(|r| r.remove))
    })
    .await;
    th.harness.engine.install_or_update_tool(true); // Removal never authorizes installation.
    assert!(
        th.harness
            .engine
            .snapshot()
            .tools
            .modal
            .unwrap()
            .operation
            .is_none()
    );
    th.harness.engine.uninstall_tool();
    removal.await.unwrap().unwrap();
    assert!(th.store.list_installations().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_or_revoked_plugin_tool_review_never_installs_and_another_request_cannot_replace_it()
 {
    for revoked in [false, true] {
        let th = tools_harness();
        add_test_game(&th, "plugin", "Fixture");
        let allowed = Arc::new(AtomicBool::new(true));
        let flag = allowed.clone();
        let engine = th.harness.engine.clone();
        let review = tokio::spawn(async move {
            engine
                .review_plugin_tool(
                    "manual:plugin".into(),
                    "optiscaler".into(),
                    plugin_review(false),
                    Arc::new(move || {
                        if flag.load(Ordering::SeqCst) {
                            Ok(())
                        } else {
                            Err(tonic::Status::permission_denied("revoked"))
                        }
                    }),
                )
                .await
        });
        eventually(|| th.harness.engine.snapshot().tools.modal.is_some()).await;
        let other = th
            .harness
            .engine
            .review_plugin_tool(
                "manual:plugin".into(),
                "optiscaler".into(),
                plugin_review(true),
                Arc::new(|| Ok(())),
            )
            .await
            .unwrap_err();
        assert_eq!(other.code(), tonic::Code::ResourceExhausted);
        if revoked {
            allowed.store(false, Ordering::SeqCst);
        } else {
            th.harness.engine.close_tools();
        }
        assert!(review.await.unwrap().is_err());
        eventually(|| th.harness.engine.snapshot().tools.modal.is_none()).await;
        assert!(th.store.list_installations().unwrap().is_empty());
    }
}
