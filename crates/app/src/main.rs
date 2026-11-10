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

    #[cfg(windows)]
    if let Err(error) = gamenode_platform::autostart::register() {
        tracing::warn!(%error, "could not register login startup");
    }

    let paths = DataPaths::resolve().ok_or("Could not locate your application data directory")?;
    paths.ensure_data_dir()?;
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
    let initial_settings = store.load()?;
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

    let plugin_api = gamenode_app::plugin_host::PluginApi::default();
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
            plugin_endpoint,
            paths.clone(),
            library_store.clone(),
            login_broker,
        ),
    ) {
        Ok(providers) => Some(providers),
        Err(error) => {
            tracing::warn!(%error, "Steam provider is unavailable");
            None
        }
    };

    let app_handle_slot = Arc::new(std::sync::Mutex::new(None::<AppHandle>));
    let slot_clone = app_handle_slot.clone();

    let publisher = commands::SnapshotPublisher::new(move |snapshot| {
        if let Some(app) = slot_clone.lock().unwrap().as_ref() {
            use tauri::Emitter;
            let _ = app.emit("engine:snapshot", &snapshot);
        }
    });

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
            achievements: Arc::new(achievement_store),
            backups: Some(Arc::new(gamenode_platform::backups::Ludusavi::bundled(
                paths.data_dir(),
            )?)),
            local_games: Some(local_game_store),
            installations: Some(Arc::new(
                gamenode_platform::installations::WindowsInstallations,
            )),
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
            processes,
            playtime: Arc::new(store.playtime_store()),
            playtime_api: client.clone(),
            settings: store,
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
        move |snapshot| {
            pub_clone.publish(snapshot);
        },
    );
    if let Some(provider_runtime) = provider_runtime.clone() {
        engine.set_provider_runtime(provider_runtime);
    }

    let mut builder = tauri::Builder::default();
    builder = gamenode_app::protocol::register_asset_protocol(
        builder,
        paths.covers_dir(),
        Some(engine.clone()),
    );

    builder = builder.manage(engine.clone());
    builder = builder.manage(logging.location().clone());
    // Serves `gamenode-asset://localhost/port-art/<catalogId>/<capsule|hero>`; without this state
    // every port artwork request is answered 404.
    builder = builder.manage(gamenode_app::protocol::PortArtState::production(
        Arc::new(gamenode_platform::ports::SnapshotPortArtResolver::new(
            Arc::clone(&port_store),
        )),
        paths.data_dir(),
    )?);
    builder = builder.invoke_handler(tauri::generate_handler![
        commands::get_snapshot,
        commands::get_app_info,
        commands::get_license_notices,
        commands::dispatch_auth,
        commands::set_sync_enabled,
        commands::set_playtime_source,
        commands::dismiss_sync_explanation,
        commands::update_window_geometry,
        commands::quit_app,
        commands::library_input,
        commands::activate_entry,
        commands::open_entry,
        commands::launch_selected,
        commands::dismiss_launch,
        commands::rescan_library,
        commands::remove_entry,
        commands::pick_local_executable,
        commands::add_local_executable,
        commands::edit_local_game,
        commands::repair_local_game,
        commands::remove_local_game,
        commands::backup_local_now,
        commands::set_local_backup_settings,
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
        commands::select_tool_proxy,
        commands::set_tool_anticheat_confirmed,
        commands::set_tool_reset_config,
        commands::install_or_update_tool,
        commands::uninstall_tool,
        commands::open_tool_source,
        commands::clear_tool_cache,
        commands::plugin_inventory,
        commands::provider_sign_in,
        commands::provider_focus_sign_in,
        commands::provider_cancel_sign_in,
        commands::provider_sign_out,
        commands::provider_check,
        commands::achievement_list,
        commands::steam_sign_in,
        commands::steam_focus_sign_in,
        commands::steam_cancel_sign_in,
        commands::steam_sign_out,
        commands::steam_check,
        commands::get_recent_logs,
        commands::open_logs_folder,
        commands::ports_open,
        commands::ports_refresh,
        commands::ports_detail,
        commands::ports_pick_parent,
        commands::ports_pick_supplied,
        commands::ports_install,
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
        let locale = commands::normalize_locale(&commands::system_locale());
        let (open_text, quit_text) = if locale == "pt-BR" {
            ("Abrir o GameNode", "Sair")
        } else {
            ("Open GameNode", "Quit")
        };

        let open_item = MenuItem::with_id(app, "open", open_text, true, None::<&str>)?;
        let quit_item = MenuItem::with_id(app, "quit", quit_text, true, None::<&str>)?;
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
            eng_for_setup.dispatch(Input::CallbackReceived(url));
        }

        // Single-instance IPC listener
        let win_for_ipc = window.clone();
        let eng_for_ipc = eng_for_setup.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(msg) = owner_messages.recv().await {
                if let ipc::Message::AuthCallback { url } = msg {
                    eng_for_ipc.dispatch(Input::CallbackReceived(url));
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
                eng_for_ticker.dispatch(Input::Tick);
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
                eng_for_events.update_settings(move |s| s.window = Some(geom));
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
                eng_for_events.update_settings(move |s| s.window = Some(geom));
            }

            let _ = &api;
            #[cfg(target_os = "windows")]
            {
                api.prevent_close();
                let _ = window.hide();
                if let Some(win) = window.app_handle().get_webview_window(window.label()) {
                    set_webview_visible(&win, false);
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
