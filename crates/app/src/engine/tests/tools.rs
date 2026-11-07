use super::*;
use gamenode_core::drm_free::Registration;
use gamenode_core::tools::{
    CachedToolArchive, InMemoryToolStore, MockToolCatalogApi, MockToolInstallerHost, ToolChannel,
    ToolOperationStatus, ToolRelease, optiscaler_descriptor,
};

async fn eventually(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

struct ToolsHarness {
    harness: Harness,
    #[allow(dead_code)]
    store: Arc<InMemoryToolStore>,
    #[allow(dead_code)]
    catalog: Arc<MockToolCatalogApi>,
    #[allow(dead_code)]
    installer: Arc<MockToolInstallerHost>,
}

fn tools_harness() -> ToolsHarness {
    let api = FakeApi::new();
    let creds = FakeCreds::with(None);
    let files = Arc::new(SlowFiles::steam_with_portal_2());
    let uri = Arc::new(FakeUri::default());
    let covers = tempfile::tempdir().unwrap();
    let playtime_db = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let processes = Arc::new(gamenode_core::tracker::scripted::ScriptedProcesses::default());

    let store = Arc::new(InMemoryToolStore::default());
    let catalog = Arc::new(MockToolCatalogApi::default());
    let installer = Arc::new(MockToolInstallerHost::default());

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
        cached_path: Some("/cache/opti.7z".to_string()),
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
        path: "/cache/opti.7z".to_string(),
        size_bytes: 5_000_000,
        sha256: "sha_test".to_string(),
    }];

    let ports = Ports {
        backups: None,
        local_games: None,
        installations: None,
        tools: Some(store.clone()),
        tool_catalog: Some(catalog.clone()),
        tool_installer: Some(installer.clone()),
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
        steam: None,
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
        library: None,
        launch: None,
        tools: Default::default(),
        store_accounts: Vec::new(),
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
    };

    ToolsHarness {
        harness,
        store,
        catalog,
        installer,
    }
}

fn add_test_game(h: &ToolsHarness, id: &str, title: &str) {
    let reg = Registration {
        id: id.to_string(),
        title: title.to_string(),
        executable: format!("C:/Games/{title}/bin/x64/Game.exe"),
        file_identity: format!("ident_{id}"),
        root: format!("C:/Games/{title}"),
        working_directory: format!("C:/Games/{title}"),
        arguments: vec![],
        gameplay: vec![],
        launchers: vec![],
        revision: 1,
        removed: false,
        rediscovery_allowed: false,
        transferred_from: None,
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
async fn clear_tool_cache_resets_bytes_and_archives() {
    let th = tools_harness();
    add_test_game(&th, "game1", "Cyberpunk 2077");

    th.harness.engine.recover_and_load_tools();
    eventually(|| th.harness.engine.snapshot().tools.total_cache_bytes > 0).await;

    let snapshot = th.harness.engine.snapshot();
    assert_eq!(snapshot.tools.total_cache_bytes, 5_000_000);
    assert_eq!(snapshot.tools.total_cache_archives, 1);

    th.harness.engine.clear_tool_cache();
    eventually(|| th.harness.engine.snapshot().tools.total_cache_bytes == 0).await;

    let snapshot = th.harness.engine.snapshot();
    assert_eq!(snapshot.tools.total_cache_bytes, 0);
    assert_eq!(snapshot.tools.total_cache_archives, 0);
}
