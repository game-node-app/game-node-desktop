#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::Duration;

use gamenode_app::commands;
use gamenode_app::engine::pc_ports::{
    InstallDeps, PORTS_PROGRESS_EVENT, PortProgressPublisher, PortsService, PortsServiceConfig,
};
use gamenode_app::engine::{Engine, Ports};
use gamenode_app::plugin_host::runtime::ProviderRuntime;
use gamenode_core::geometry;
use gamenode_core::library::LibraryStore;
use gamenode_core::ports::PortStore;
use gamenode_core::ports::SettingsStore;
use gamenode_core::{Input, WindowGeometry};
use gamenode_platform::{
    browser::SystemBrowser, credentials, db::SqliteSettingsStore, display, ipc, paths::DataPaths,
    random,
};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// The longest the post-event-loop shutdown may take before ending the process anyway.
const SHUTDOWN_WATCHDOG: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupArguments {
    pub background: bool,
    pub incoming: ipc::Message,
}

pub fn parse_startup_arguments(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> StartupArguments {
    let arguments: Vec<std::ffi::OsString> = arguments.into_iter().collect();
    let background = arguments.iter().any(|arg| arg == "--background");
    let incoming = arguments
        .iter()
        .find(|arg| *arg != "--background")
        .and_then(|arg| arg.to_str())
        .map(|url| ipc::Message::AuthCallback {
            url: url.to_owned(),
        })
        .unwrap_or(ipc::Message::Activate);
    StartupArguments {
        background,
        incoming,
    }
}

pub fn format_tray_tooltip(running_title: &str) -> String {
    let trimmed = running_title.trim();
    if trimmed.is_empty() {
        "GameNode".to_string()
    } else {
        format!("GameNode — {trimmed}")
    }
}

/// Rewrites the current user's autostart value once at start: on with the minimized flag when
/// "Start with Windows" is on, removed otherwise. The registry work runs on its own thread before
/// the window exists, and a non-installed build is a no-op (`register` returns `Ok(false)`).
#[cfg(windows)]
fn apply_autostart(preferences: &gamenode_core::AppPreferences) {
    let preferences = preferences.clone();
    let worker = std::thread::Builder::new()
        .name("gamenode-autostart".into())
        .spawn(move || {
            if preferences.start_with_windows {
                gamenode_platform::autostart::register(preferences.start_minimized).map(drop)
            } else {
                gamenode_platform::autostart::unregister().map(drop)
            }
        });
    match worker {
        Ok(worker) => match worker.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "could not update login startup"),
            Err(_) => tracing::warn!("the login startup worker panicked"),
        },
        Err(error) => tracing::warn!(%error, "could not start the login startup worker"),
    }
}

/// Shows or hides the WebView2 controller with its window. `window.hide()` alone leaves the
/// webview rendering in the background; `SetIsVisible(false)` suspends it. On newer runtimes the
/// memory target follows the visibility, and older runtimes without `ICoreWebView2_19` still get
/// the visibility change.
#[cfg(target_os = "windows")]
fn set_webview_visible(window: &tauri::WebviewWindow, visible: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_19,
    };
    use windows_core::Interface;

    let result = window.with_webview(move |webview| {
        let controller = webview.controller();
        // SAFETY: `with_webview` runs this closure on the UI thread, as WebView2 requires.
        unsafe {
            let _ = controller.SetIsVisible(visible);
            let Ok(core) = controller.CoreWebView2() else {
                return;
            };
            let Ok(core) = core.cast::<ICoreWebView2_19>() else {
                return;
            };
            let level = if visible {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            };
            let _ = core.SetMemoryUsageTargetLevel(level);
        }
    });
    if let Err(error) = result {
        tracing::warn!(%error, visible, "could not update the webview visibility");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    if let Some(result) =
        gamenode_app::plugin_host::launch_supervisor::run_linux_helper_if_requested()
    {
        return result;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let startup = parse_startup_arguments(std::env::args_os().skip(1));
    let incoming = startup.incoming.clone();

    let endpoint = ipc::Endpoint::for_current_user()?;
    let owner = match ipc::acquire(&endpoint, runtime.handle())? {
        ipc::Instance::Owner(owner) => owner,
        ipc::Instance::Existing(existing) => {
            ipc::forward_launch(&existing, &incoming, startup.background, ipc::FORWARD_WAIT)?;
            return Ok(());
        }
    };
    let mut owner = owner.serve(runtime.handle());

    let paths = DataPaths::resolve().ok_or("Could not locate your application data directory")?;
    paths.ensure_data_dir()?;
    let plugin_updates = gamenode_app::plugin_host::updates::PluginUpdateService::open(&paths)?;
    let logging = gamenode_app::logging::init(&paths);
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        data_dir = %paths.data_dir().display(),
        logs_dir = %logging.location().directory.display(),
        "GameNode Desktop starting"
    );
    let store = Arc::new(SqliteSettingsStore::open(&paths.database_file())?);
    plugin_updates.set_bundle_store(store.bundle_store());
    let initial_settings = store.load()?;
    // The lifecycle preferences decide the login value before any window exists.
    #[cfg(windows)]
    apply_autostart(&initial_settings.preferences);
    // One durable port store, shared by the catalog worker, the install runner and the
    // `port-art` route. It lives on the same SQLite connection as the rest of the app.
    let port_store: Arc<dyn PortStore> = Arc::new(store.ports_store());
    let local_game_store = Arc::new(store.local_game_store());

    let platform = gamenode_core::provider::Platform::current()
        .ok_or("GameNode Desktop supports Windows and Linux only")?;
    let providers = gamenode_app::providers::load_bundled_with_plugins(
        platform,
        &gamenode_app::plugin_host::backends(platform),
    )
    .providers;
    let library_store = Arc::new(store.library_store());
    let layout = library_store.load_layout().unwrap_or_else(|error| {
        tracing::warn!(%error, "could not read the saved library layout; using the default");
        Default::default()
    });
    // "Open in" replaces the remembered scope once, before the window exists. Later scope changes
    // persist through the normal layout path.
    let layout = initial_settings.preferences.apply_start_scope(layout);
    let library = gamenode_core::library::LibraryMachine::new(providers, layout);

    let displays = display::available_displays();
    let can_position = display::can_position_windows();
    let plan = geometry::plan_window(initial_settings.window, &displays, can_position);

    let credentials = {
        let paths = paths.clone();
        std::thread::spawn(move || {
            credentials::open_store(&paths, credentials::CredentialSlot::Session)
        })
        .join()
        .map_err(|_| "Credential storage initialization failed")?
    };

    let client = Arc::new(gamenode_api::GameNodeClient::new(
        gamenode_api::server_url_from_env()?,
    )?);
    let processes: Arc<dyn gamenode_core::ports::Processes> =
        Arc::new(gamenode_platform::processes::HostProcesses::new());
    // One coordinator instance is shared by supervised runtime launches and backup/restore jobs.
    let custom_launch_leases =
        Arc::new(gamenode_app::plugin_host::resource_leases::ResourceLeases::default());

    let plugin_api = gamenode_app::plugin_host::PluginApi {
        shared_policies: Arc::new(
            gamenode_app::plugin_host::sharing_policy::SharedSharingPolicies::open(
                paths.plugin_data_dir().join("shared-data-policies"),
            )?,
        ),
        storage: Arc::new(gamenode_app::plugin_host::storage::PluginStorage::new(
            paths.plugin_data_dir(),
        )),
        shared_pool: Arc::new(
            gamenode_app::plugin_host::shared_pool::DurableSharedPool::new(paths.data_dir()),
        ),
        // Nothing under the application data folder is reachable through file handles. The
        // folder-approval dialog is connected with the settings UI (`FileAccess::set_approver`).
        files: Arc::new(gamenode_app::plugin_host::files::FileAccess::new(
            gamenode_platform::plugin_files::Reserved::new(&[paths.data_dir().to_path_buf()]),
        )),
        leases: custom_launch_leases.clone(),
        processes: Some(processes.clone()),
        ..Default::default()
    };
    plugin_api
        .set_state_layout(gamenode_app::plugin_host::profile::PluginStateLayout::managed(&paths));
    #[cfg(target_os = "linux")]
    let plugin_api_server = match gamenode_app::plugin_host::PluginApiServer::start(
        runtime.handle(),
        endpoint.dir().join("plugins.sock"),
        plugin_api.clone(),
    ) {
        Ok(server) => Some(server),
        Err(error) => {
            tracing::warn!(%error, "plugin services are unavailable");
            None
        }
    };
    #[cfg(windows)]
    let plugin_api_server = match gamenode_app::plugin_host::PluginApiServer::start_windows(
        runtime.handle(),
        plugin_api.clone(),
    ) {
        Ok(server) => Some(server),
        Err(error) => {
            tracing::warn!(%error, "plugin services are unavailable");
            None
        }
    };
    let plugin_endpoint = plugin_api_server
        .as_ref()
        .map(|server| server.endpoint().to_path_buf());
    let login_windows = Arc::new(gamenode_app::plugin_host::login::TauriLoginWindows::new(
        Arc::new(SystemBrowser),
    ));
    let login_broker =
        match runtime.block_on(gamenode_app::plugin_host::login::LoginBroker::recover(
            paths.private_dir(),
            login_windows.clone(),
            gamenode_app::plugin_host::legacy_login_profiles(),
        )) {
            Ok(broker) => Some(broker),
            Err(error) => {
                tracing::warn!(%error, "plugin login services are unavailable");
                None
            }
        };
    let provider_runtime = match runtime.block_on(
        gamenode_app::plugin_host::runtime::HostedProviders::bundled(
            plugin_api.providers.clone(),
            plugin_api.intents.clone(),
            plugin_endpoint,
            paths.clone(),
            library_store.clone(),
            login_broker,
        ),
    ) {
        Ok(providers) => Some(providers),
        Err(error) => {
            tracing::warn!(%error, "bundled providers are unavailable");
            None
        }
    };
    if let Some(provider_runtime) = provider_runtime.as_ref() {
        provider_runtime
            .lifecycle()
            .set_private_storage(plugin_api.storage.clone());
        plugin_updates.set_lifecycle(provider_runtime.lifecycle());
    }

    let app_handle_slot = Arc::new(std::sync::Mutex::new(None::<AppHandle>));
    let slot_clone = app_handle_slot.clone();

    let publisher = commands::SnapshotPublisher::new(move |snapshot| {
        if let Some(app) = slot_clone.lock().unwrap().as_ref() {
            use tauri::Emitter;
            let _ = app.emit("engine:snapshot", &snapshot);
        }
    });

    // A plugin's notification leaves on its own event and is shown by the window as plain text.
    // It is an event, not state: with no window listening it is dropped.
    struct NoticeSink(Arc<std::sync::Mutex<Option<AppHandle>>>);
    impl gamenode_app::plugin_host::notifications::NotificationSink for NoticeSink {
        fn deliver(&self, notice: gamenode_app::plugin_host::notifications::PluginNotice) {
            if let Some(app) = self.0.lock().unwrap().as_ref() {
                use tauri::Emitter;
                let _ = app.emit(gamenode_app::plugin_host::notifications::EVENT, &notice);
            }
        }
    }
    plugin_api
        .notifications
        .set_sink(Arc::new(NoticeSink(app_handle_slot.clone())));

    // Folder approvals are collected by the host, correlated through one-use ids, and answered
    // by the shared React window. No plugin-suggested path is trusted until the user selects it
    // and the host validates/canonicalizes it in the resource handler.
    let root_approval_slot = app_handle_slot.clone();
    let root_approvals = Arc::new(gamenode_app::plugin_host::files::RootApprovalBridge::new(
        Arc::new(move |signal| {
            use gamenode_app::plugin_host::files::{
                ROOT_CLOSED_EVENT, ROOT_CONFIRM_EVENT, ROOT_SELECT_EVENT, RootApprovalSignal,
            };
            use tauri::Emitter;
            let guard = root_approval_slot.lock().unwrap();
            let Some(app) = guard.as_ref() else {
                return false;
            };
            match signal {
                RootApprovalSignal::Select(prompt) => app.emit(ROOT_SELECT_EVENT, &prompt).is_ok(),
                RootApprovalSignal::Confirm(prompt) => {
                    app.emit(ROOT_CONFIRM_EVENT, &prompt).is_ok()
                }
                RootApprovalSignal::Closed(id) => app.emit(ROOT_CLOSED_EVENT, id).is_ok(),
            }
        }),
    ));
    plugin_api.files.set_approver(root_approvals.clone());

    // A plugin's request to launch or install a game is confirmed in the window: the bridge shows
    // it with an event and takes the answer from a command. With no window listening, a request
    // that needs the user fails instead of waiting.
    let confirmation_slot = app_handle_slot.clone();
    let confirmations = Arc::new(
        gamenode_app::plugin_host::confirmations::ConfirmationBridge::new(Arc::new(
            move |signal| {
                use gamenode_app::plugin_host::confirmations::{CLOSED_EVENT, EVENT, Signal};
                use tauri::Emitter;
                let guard = confirmation_slot.lock().unwrap();
                let Some(app) = guard.as_ref() else {
                    return false;
                };
                match signal {
                    Signal::Open(confirmation) => app.emit(EVENT, confirmation.as_ref()).is_ok(),
                    Signal::Closed(id) => app.emit(CLOSED_EVENT, &id).is_ok(),
                }
            },
        )),
    );
    plugin_api.intents.set_prompter(confirmations.clone());
    let plugin_approvals = Arc::new(
        gamenode_app::plugin_host::confirmations::PluginApprovals::new(
            plugin_api.storage.clone(),
            plugin_api.providers.profiles(),
        ),
    );

    // PC ports progress leaves on its own event; the engine coalesces ticks before they get here.
    let progress_app_slot = app_handle_slot.clone();
    let port_progress = Arc::new(PortProgressPublisher::new(
        runtime.handle().clone(),
        move |progress| {
            if let Some(app) = progress_app_slot.lock().unwrap().as_ref() {
                use tauri::Emitter;
                let _ = app.emit(PORTS_PROGRESS_EVENT, &progress);
            }
        },
    ));

    // The service loads the overlay, recovers interrupted installs and starts its workers on a
    // dedicated thread; construction returns immediately.
    let pc_ports = if gamenode_core::ports::PortPlatform::current().supports_installs() {
        let data_dir = paths.data_dir().to_owned();
        Some(PortsService::start(PortsServiceConfig {
            store: Arc::clone(&port_store),
            local_games: local_game_store.clone(),
            catalog: Arc::new(gamenode_platform::ports::QuiverClient::new()?),
            overlay_fetcher: Arc::new(
                gamenode_platform::ports::overlay_source::HttpOverlayFetcher::new(),
            ),
            clock: Arc::new(gamenode_platform::ports::SystemClock),
            data_dir: data_dir.clone(),
            platform: gamenode_core::ports::PortPlatform::current(),
            install: Some(InstallDeps {
                fetcher: Arc::new(gamenode_platform::download::HttpDownloadFetcher::new()?),
                url_policy: Arc::new(gamenode_platform::download::HttpsDownloadPolicy),
                user_roots: gamenode_platform::ports::UserDataRoots::host()?,
                identity: Arc::new(gamenode_platform::ports::SystemExecutableIdentity),
                packages: Arc::new(gamenode_platform::ports::PinnedExtractor::new(data_dir)?),
            }),
        }))
    } else {
        None
    };

    let pub_clone = publisher.clone();
    let achievement_store = store.achievement_store();
    let engine = Engine::new(
        Ports {
            discovery: None,
            achievements: Arc::new(achievement_store),
            backups: Some(Arc::new(gamenode_platform::backups::Ludusavi::bundled(
                paths.data_dir(),
            )?)),
            local_games: Some(local_game_store),
            installations: Some(Arc::new({
                #[cfg(target_os = "windows")]
                {
                    gamenode_platform::installations::WindowsInstallations
                }
                #[cfg(target_os = "linux")]
                {
                    gamenode_platform::installations::LinuxInstallations
                }
            })),
            tools: Some(Arc::new(store.tool_store())),
            tool_catalog: Some(Arc::new(
                gamenode_platform::tools::catalog::GitHubToolCatalog::new(paths.tools_cache_dir())?,
            )),
            tool_installer: Some(Arc::new(
                gamenode_platform::tools::PlatformToolInstaller::new(
                    paths.tool_backups_dir(),
                    processes.clone(),
                ),
            )),
            tool_picker: None,
            processes,
            playtime: Arc::new(store.playtime_store()),
            playtime_api: client.clone(),
            settings: store.clone(),
            credentials,
            api: client.clone(),
            browser: Arc::new(SystemBrowser),
            library: library_store.clone(),
            games: client,
            files: Arc::new(gamenode_platform::host::StdHostFiles),
            roots: Arc::new(gamenode_platform::host::SystemRoots::new()),
            uri: Arc::new(gamenode_platform::uri::SystemUriOpener),
            pc_ports,
            port_store: Some(Arc::clone(&port_store)),
            port_progress,
            covers_dir: paths.covers_dir(),
            platform,
        },
        runtime.handle().clone(),
        initial_settings,
        library,
        Box::new(random::nonce),
        custom_launch_leases.clone(),
        move |snapshot| {
            pub_clone.publish(snapshot);
        },
    );
    if let Some(provider_runtime) = provider_runtime.clone() {
        engine.set_provider_runtime(provider_runtime);
    }
    // HostedProviders owns the single profile lifecycle observer: it fences sessions, reaps old
    // process trees, then restores only the new account's enabled plugins.
    engine.set_plugin_profiles(plugin_api.providers.profiles());
    plugin_updates.set_profiles(plugin_api.providers.profiles());
    // Plugins read the library through a host-owned projection the engine keeps current.
    engine.set_library_feed(plugin_api.library.clone());
    let folder_approvals = Arc::new(
        gamenode_app::plugin_host::file_approvals::PluginFolderApprovals::new(
            plugin_api.storage.clone(),
            plugin_api.providers.profiles(),
            plugin_api.files.clone(),
            plugin_api.library.clone(),
            root_approvals.clone(),
        ),
    );
    let shared_resources = Arc::new(
        gamenode_app::plugin_host::shared_resources::PluginSharedResources::new(
            plugin_api.storage.clone(),
            plugin_api.providers.profiles(),
            plugin_api.files.clone(),
            plugin_api.library.clone(),
        ),
    );
    if let Some(server) = plugin_api_server.as_ref() {
        engine.set_selected_launch_api(server.resource_api());
    }

    let mut builder = tauri::Builder::default();
    builder = builder.plugin(tauri_plugin_dialog::init());
    builder = gamenode_app::protocol::register_asset_protocol(
        builder,
        paths.covers_dir(),
        Some(engine.clone()),
    );

    builder = builder.manage(engine.clone());
    builder = builder.manage(store.clone());
    builder = builder.manage(plugin_api.providers.clone());
    builder = builder.manage(plugin_api.shared_policies.clone());
    builder = builder.manage(plugin_api.data_broker.clone());
    if let Some(provider_runtime) = provider_runtime.as_ref() {
        builder = builder.manage(provider_runtime.lifecycle());
    }
    builder = builder.manage(confirmations);
    builder = builder.manage(root_approvals);
    builder = builder.manage(folder_approvals);
    builder = builder.manage(shared_resources);
    builder = builder.manage(plugin_approvals);
    builder = builder.manage(plugin_updates.clone());
    builder = builder.manage(logging.location().clone());
    // Serves `gamenode-asset://localhost/port-art/<catalogId>/<capsule|hero>`; without this state
    // every port artwork request is answered 404.
    builder = builder.manage(gamenode_app::protocol::PortArtState::production(
        Arc::new(gamenode_platform::ports::SnapshotPortArtResolver::new(
            Arc::clone(&port_store),
        )),
        paths.data_dir(),
    )?);
    let periodic_updates = plugin_updates.clone();
    let background_runtime = runtime.handle().clone();
    builder = builder.setup(move |app| {
        let app = app.handle().clone();
        let updates = periodic_updates.clone();
        background_runtime.spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
            loop {
                interval.tick().await;
                if !updates.preferences().periodic_checks {
                    continue;
                }
                match updates.check_updates().await {
                    Ok(available) if !available.is_empty() => {
                        use tauri::Emitter;
                        if let Err(error) = app.emit("plugin:update-available", &available) {
                            tracing::warn!(%error, "could not notify the UI about plugin updates");
                        }
                    }
                    Ok(_) => {}
                    Err(error) => tracing::debug!(%error, "periodic plugin update check failed"),
                }
            }
        });
        Ok(())
    });
    builder = builder.invoke_handler(tauri::generate_handler![
        commands::get_snapshot,
        commands::get_app_info,
        commands::get_license_notices,
        commands::dispatch_auth,
        commands::set_sync_enabled,
        commands::sync_now,
        commands::set_playtime_source,
        commands::set_app_preference,
        commands::dismiss_sync_explanation,
        commands::update_window_geometry,
        commands::quit_app,
        commands::library_input,
        commands::activate_entry,
        commands::open_entry,
        commands::review_detected_target,
        commands::launch_recipe_choices,
        commands::select_launch_recipe,
        commands::use_detected_default,
        commands::repair_detected_target,
        commands::launch_selected,
        commands::start_extension_action,
        commands::available_plugin_extensions,
        commands::dismiss_launch,
        commands::rescan_library,
        commands::open_gamenode_page,
        commands::open_gamenode_link,
        commands::sign_in_cover_wall,
        commands::remove_entry,
        commands::pick_local_executable,
        commands::pick_local_cover,
        commands::discard_local_cover,
        commands::add_local_executable,
        commands::edit_local_game,
        commands::repair_local_game,
        commands::remove_local_game,
        commands::backup_local_now,
        commands::set_local_backup_settings,
        commands::local_backup_usage,
        commands::set_local_backup_retention,
        commands::pick_save_destination,
        commands::set_local_profile_automatic,
        commands::configure_local_saves,
        commands::pick_local_saves,
        commands::choose_local_save_profile,
        commands::watch_local_folder,
        commands::pick_local_watch_folder,
        commands::remove_local_watch,
        commands::set_local_candidate_executable,
        commands::rescan_local_folders,
        commands::check_local_watch,
        commands::select_local_candidate,
        commands::review_selected_local_candidates,
        commands::reset_local_ignores,
        commands::load_local_history,
        commands::preview_local_restore,
        commands::confirm_local_restore,
        commands::cancel_local_restore,
        commands::resolve_local_claim,
        commands::cancel_local_claim,
        commands::dismiss_local_discovered,
        commands::open_tools_for_selected,
        commands::open_tools,
        commands::close_tools,
        commands::select_tool_channel,
        commands::refresh_tool_versions,
        commands::select_tool_version,
        commands::select_tool_target_exe,
        commands::pick_tool_target_exe,
        commands::select_tool_proxy,
        commands::set_tool_anticheat_confirmed,
        commands::set_tool_reset_config,
        commands::install_or_update_tool,
        commands::uninstall_tool,
        commands::open_tool_source,
        commands::clear_tool_cache,
        commands::plugin_inventory,
        commands::plugin_import_review,
        commands::plugin_import_confirm_publisher,
        commands::plugin_import_install,
        commands::plugin_import_license_text,
        commands::plugin_bundle_import_review,
        commands::plugin_bundle_import_confirm_publisher,
        commands::plugin_bundle_import_install,
        commands::plugin_bundle_permissions_replace,
        commands::plugin_bundle_update_check,
        commands::plugin_bundle_update_available,
        commands::plugin_bundle_update_review,
        commands::plugin_bundle_update_approve,
        commands::plugin_bundle_update_decline,
        commands::plugin_bundle_rollback_available,
        commands::plugin_bundle_rollback_review,
        commands::plugin_bundle_rollback_approve,
        commands::plugin_permission_snapshot,
        commands::plugin_permissions_replace,
        commands::plugin_shared_data_snapshot,
        commands::plugin_shared_data_replace,
        commands::plugin_durable_usage,
        commands::plugin_durable_delete,
        commands::plugin_durable_reuse,
        commands::plugin_private_reuse,
        commands::plugin_storage_cleanup_review,
        commands::plugin_storage_cleanup_apply,
        gamenode_app::plugin_host::migration::plugin_publisher_migration_preview,
        gamenode_app::plugin_host::migration::plugin_publisher_migration_apply,
        gamenode_app::plugin_host::migration::plugin_publisher_migration_cancel,
        gamenode_app::plugin_host::migration::plugin_publisher_migration_rollback,
        commands::plugin_enable,
        commands::plugin_disable,
        commands::plugin_remove,
        commands::plugin_bundle_uninstall,
        commands::plugin_configuration_state,
        commands::plugin_configuration_modes,
        commands::plugin_configuration_open,
        commands::plugin_settings_describe,
        commands::plugin_settings_submit,
        commands::plugin_settings_cancel,
        commands::plugin_playtime_inventory,
        commands::plugin_playtime_set_accounting,
        commands::plugin_update_preferences,
        commands::plugin_update_available,
        commands::plugin_update_set_periodic_checks,
        commands::plugin_update_check,
        commands::plugin_update_review,
        commands::plugin_rollback_available,
        commands::plugin_rollback_review,
        commands::plugin_update_retrust_publisher,
        commands::plugin_rollback_approve,
        commands::plugin_update_approve,
        commands::plugin_update_decline,
        commands::plugin_confirmation_answer,
        commands::plugin_folder_selection_answer,
        commands::plugin_folder_approval_answer,
        commands::plugin_folder_approvals,
        commands::plugin_folder_revoke,
        commands::plugin_folder_change,
        commands::plugin_library_root_set_watching,
        commands::plugin_shared_resource_reviews,
        commands::plugin_shared_resource_accept,
        commands::plugin_shared_resource_revoke,
        commands::plugin_remembered_confirmations,
        commands::plugin_revoke_confirmation,
        commands::provider_sign_in,
        commands::provider_focus_sign_in,
        commands::provider_cancel_sign_in,
        commands::provider_sign_out,
        commands::provider_check,
        commands::plugin_stop,
        commands::custom_launch_ids,
        commands::custom_launch_stop,
        commands::plugin_start,
        commands::achievement_list,
        commands::open_achievements_page,
        commands::get_recent_logs,
        commands::open_logs_folder,
        commands::system_info_text,
        commands::ports_open,
        commands::ports_refresh,
        commands::ports_detail,
        commands::ports_pick_parent,
        commands::ports_pick_supplied,
        commands::ports_install,
        commands::ports_dismiss_plugin_setup,
        commands::ports_cancel,
        commands::ports_mark_seen,
        commands::ports_set_preferences,
        commands::ports_pick_executable,
        commands::ports_tools_cache,
        commands::ports_clear_cache,
        commands::open_install_folder,
    ]);

    let eng_for_setup = engine.clone();
    let eng_for_events = engine.clone();
    let setup_startup = startup.clone();
    let setup_plan = plan;
    let app_handle_slot_setup = app_handle_slot.clone();

    let (placeholder_sender, placeholder_receiver) = tokio::sync::mpsc::channel(1);
    drop(placeholder_sender);
    let mut owner_messages = std::mem::replace(&mut owner.messages, placeholder_receiver);

    builder = builder.setup(move |app| {
        *app_handle_slot_setup.lock().unwrap() = Some(app.handle().clone());
        login_windows.attach(app.handle().clone());

        let mut win_builder =
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("GameNode")
                .inner_size(setup_plan.size.0 as f64, setup_plan.size.1 as f64)
                .min_inner_size(960.0, 600.0)
                .decorations(false)
                .maximized(setup_plan.maximized)
                .visible(!setup_startup.background)
                .additional_browser_args(
                    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --enable-scaling-clipped-images",
                );

        if let Some((x, y)) = setup_plan.position {
            win_builder = win_builder.position(x as f64, y as f64);
        }

        let window = win_builder.build()?;

        if !setup_startup.background {
            eng_for_setup.window_shown();
        }

        // Setup system tray
        let locale = commands::effective_locale(
            eng_for_setup.preferences().locale,
            &commands::system_locale(),
        );
        let (open_text, quit_text) = commands::tray_labels(locale);

        let open_item = MenuItem::with_id(app, "open", open_text, true, None::<&str>)?;
        let quit_item = MenuItem::with_id(app, "quit", quit_text, true, None::<&str>)?;
        app.manage(commands::TrayLabels::new(open_item.clone(), quit_item.clone()));
        let tray_menu = Menu::with_items(app, &[&open_item, &quit_item])?;

        let mut tray_builder = TrayIconBuilder::with_id("main-tray")
            .menu(&tray_menu)
            .tooltip("GameNode")
            .show_menu_on_left_click(false);

        if let Some(icon) = app.default_window_icon() {
            tray_builder = tray_builder.icon(icon.clone());
        }

        let eng_for_tray = eng_for_setup.clone();
        let tray_icon = tray_builder
            .on_menu_event(move |app_handle, event| match event.id.as_ref() {
                "open" => {
                    if let Some(win) = app_handle.get_webview_window("main") {
                        let _ = win.unminimize();
                        let _ = win.show();
                        #[cfg(target_os = "windows")]
                        set_webview_visible(&win, true);
                        let _ = win.set_focus();
                        eng_for_tray.window_shown();
                    }
                }
                "quit" => {
                    app_handle.exit(0);
                }
                _ => {}
            })
            .on_tray_icon_event(move |tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    let app_handle = tray.app_handle();
                    if let Some(win) = app_handle.get_webview_window("main") {
                        let _ = win.unminimize();
                        let _ = win.show();
                        #[cfg(target_os = "windows")]
                        set_webview_visible(&win, true);
                        let _ = win.set_focus();
                    }
                }
            })
            .build(app)?;

        eng_for_setup.start();
        if let ipc::Message::AuthCallback { url } = incoming {
            let engine = eng_for_setup.clone();
            tauri::async_runtime::spawn(async move {
                engine.dispatch_on_worker(Input::CallbackReceived(url)).await;
            });
        }

        // Single-instance IPC listener
        let win_for_ipc = window.clone();
        let eng_for_ipc = eng_for_setup.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(msg) = owner_messages.recv().await {
                if let ipc::Message::AuthCallback { url } = msg {
                    eng_for_ipc.dispatch_on_worker(Input::CallbackReceived(url)).await;
                }
                let _ = win_for_ipc.unminimize();
                let _ = win_for_ipc.show();
                #[cfg(target_os = "windows")]
                set_webview_visible(&win_for_ipc, true);
                let _ = win_for_ipc.set_focus();
                eng_for_ipc.window_shown();
            }
        });

        // Maintenance ticker (1 sec). `tokio::time::interval` needs a Tokio reactor, which the
        // setup closure (main thread) does not have, so the interval is created inside the task.
        let eng_for_ticker = eng_for_setup.clone();
        let mut last_tooltip = String::new();

        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                eng_for_ticker.dispatch_on_worker(Input::Tick).await;
                eng_for_ticker.maintenance();

                let title = eng_for_ticker.running_title().unwrap_or_default();
                let new_tooltip = format_tray_tooltip(&title);
                if new_tooltip != last_tooltip {
                    let _ = tray_icon.set_tooltip(Some(&new_tooltip));
                    last_tooltip = new_tooltip;
                }
            }
        });

        Ok(())
    });

    builder = builder.on_window_event(move |window, event| match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            if let (Ok(is_max), Ok(size)) = (window.is_maximized(), window.inner_size()) {
                let scale = window.scale_factor().unwrap_or(1.0).max(f64::EPSILON);
                let width = (size.width as f64 / scale).round().max(1.0) as u32;
                let height = (size.height as f64 / scale).round().max(1.0) as u32;
                let (x, y) = if can_position && !is_max {
                    if let Ok(pos) = window.outer_position() {
                        (
                            Some((pos.x as f64 / scale).round() as i32),
                            Some((pos.y as f64 / scale).round() as i32),
                        )
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };
                let geom = WindowGeometry {
                    x,
                    y,
                    width,
                    height,
                    maximized: is_max,
                };
                eng_for_events.update_window_geometry(geom);
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            if let (Ok(is_max), Ok(size)) = (window.is_maximized(), window.inner_size()) {
                let scale = window.scale_factor().unwrap_or(1.0).max(f64::EPSILON);
                let width = (size.width as f64 / scale).round().max(1.0) as u32;
                let height = (size.height as f64 / scale).round().max(1.0) as u32;
                let (x, y) = if can_position && !is_max {
                    if let Ok(pos) = window.outer_position() {
                        (
                            Some((pos.x as f64 / scale).round() as i32),
                            Some((pos.y as f64 / scale).round() as i32),
                        )
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };
                let geom = WindowGeometry {
                    x,
                    y,
                    width,
                    height,
                    maximized: is_max,
                };
                eng_for_events.update_window_geometry(geom);
            }

            let _ = &api;
            #[cfg(target_os = "windows")]
            match commands::close_action(eng_for_events.preferences().close_to_tray) {
                commands::CloseAction::HideToTray => {
                    api.prevent_close();
                    let _ = window.hide();
                    if let Some(win) = window.app_handle().get_webview_window(window.label()) {
                        set_webview_visible(&win, false);
                    }
                }
                commands::CloseAction::Quit => {
                    api.prevent_close();
                    let engine = eng_for_events.clone();
                    let app = window.app_handle().clone();
                    // Ending sessions and flushing storage blocks; keep it off the UI thread,
                    // then end the event loop like the Quit path does.
                    tauri::async_runtime::spawn(async move {
                        let _ = tokio::task::spawn_blocking(move || engine.shutdown()).await;
                        app.exit(0);
                    });
                }
            }
        }
        _ => {}
    });

    let app = builder.build(tauri::generate_context!())?;
    let exit_code = app.run_return(|_app_handle, _event| {});

    // Quit must always end the process. If a provider or plugin task wedges shutdown, the
    // watchdog ends it after the bounded shutdown windows instead of leaving a background
    // process (and a ghost tray icon) behind.
    if let Err(error) = std::thread::Builder::new()
        .name("gamenode-exit-watchdog".into())
        .spawn(move || {
            std::thread::sleep(SHUTDOWN_WATCHDOG);
            std::process::exit(exit_code);
        })
    {
        tracing::warn!(%error, "could not start the exit watchdog");
    }

    // The engine flushes storage and may wait; keep it off the main thread. Call the runtime
    // method: the free `tokio::task::spawn_blocking` uses `Handle::current()`, which panics
    // here because the Tauri event loop has already returned and no runtime is entered.
    let shutdown = runtime.spawn_blocking(move || engine.shutdown());
    if let Err(error) = runtime.block_on(shutdown) {
        tracing::error!(%error, "engine shutdown failed");
    }

    if let Some(provider_runtime) = provider_runtime {
        runtime.block_on(provider_runtime.shutdown());
    }
    if let Some(server) = plugin_api_server {
        runtime.block_on(server.shutdown());
    }

    runtime.shutdown_timeout(Duration::from_secs(1));
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_startup_arguments_defaults_to_foreground_activate() {
        let args = parse_startup_arguments(Vec::<std::ffi::OsString>::new());
        assert!(!args.background);
        assert_eq!(args.incoming, ipc::Message::Activate);
    }

    #[test]
    fn parse_startup_arguments_recognizes_background_flag() {
        let args = parse_startup_arguments([std::ffi::OsString::from("--background")]);
        assert!(args.background);
        assert_eq!(args.incoming, ipc::Message::Activate);
    }

    #[test]
    fn parse_startup_arguments_extracts_auth_callback_url() {
        let args = parse_startup_arguments([
            std::ffi::OsString::from("--background"),
            std::ffi::OsString::from("app.gamenode://auth/callback?code=123"),
        ]);
        assert!(args.background);
        assert_eq!(
            args.incoming,
            ipc::Message::AuthCallback {
                url: "app.gamenode://auth/callback?code=123".to_string(),
            }
        );
    }

    #[test]
    fn test_tray_tooltip_formatting() {
        assert_eq!(format_tray_tooltip(""), "GameNode");
        assert_eq!(format_tray_tooltip("   "), "GameNode");
        assert_eq!(format_tray_tooltip("Hades"), "GameNode — Hades");
        assert_eq!(
            format_tray_tooltip("Cyberpunk 2077"),
            "GameNode — Cyberpunk 2077"
        );
    }

    #[test]
    fn test_locale_normalization_in_main() {
        assert_eq!(commands::normalize_locale("en"), "en");
        assert_eq!(commands::normalize_locale("en-US"), "en");
        assert_eq!(commands::normalize_locale("pt"), "pt-BR");
        assert_eq!(commands::normalize_locale("pt-BR"), "pt-BR");
        assert_eq!(commands::normalize_locale("pt_BR"), "pt-BR");
        assert_eq!(commands::normalize_locale("fr"), "en");
    }
}
