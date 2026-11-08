#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::Duration;

use gamenode_app::commands;
use gamenode_core::geometry;
use gamenode_core::{Input, WindowGeometry};
use gamenode_platform::{display, ipc};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init();

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

    #[cfg(target_os = "linux")]
    let plugin_api = gamenode_app::plugin_host::PluginApi::default();
    #[cfg(target_os = "linux")]
    let plugin_api_server = gamenode_app::plugin_host::PluginApiServer::start(
        runtime.handle(),
        endpoint.dir().join("plugins.sock"),
        plugin_api.clone(),
    )?;
    #[cfg(target_os = "linux")]
    let plugin_manager = gamenode_app::plugin_host::PluginManager::bundled(
        plugin_api,
        endpoint.dir().join("plugins.sock"),
    )?;

    #[cfg(windows)]
    if let Err(error) = gamenode_platform::autostart::register() {
        tracing::warn!(%error, "could not register login startup");
    }

    let app_handle_slot = Arc::new(std::sync::Mutex::new(None::<AppHandle>));
    let slot_clone = app_handle_slot.clone();

    let publisher = commands::SnapshotPublisher::new(move |snapshot| {
        if let Some(app) = slot_clone.lock().unwrap().as_ref() {
            use tauri::Emitter;
            let _ = app.emit("engine:snapshot", &snapshot);
        }
    });

    let application = gamenode_app::bootstrap::build(
        move |snapshot| publisher.publish(snapshot),
        runtime.handle().clone(),
    )?;
    let paths = application.paths;
    let engine = application.engine;
    #[cfg(target_os = "linux")]
    engine.set_plugin_manager(plugin_manager);
    let displays = display::available_displays();
    let can_position = display::can_position_windows();
    let plan = geometry::plan_window(application.initial_settings.window, &displays, can_position);

    let mut builder = tauri::Builder::default();
    builder = gamenode_app::protocol::register_asset_protocol(
        builder,
        paths.covers_dir(),
        Some(engine.clone()),
    );

    builder = builder.manage(engine.clone());
    builder = builder.invoke_handler(tauri::generate_handler![
        commands::get_snapshot,
        commands::get_app_info,
        commands::get_plugins,
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

        let mut win_builder =
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("GameNode")
                .inner_size(setup_plan.size.0 as f64, setup_plan.size.1 as f64)
                .min_inner_size(960.0, 600.0)
                .maximized(setup_plan.maximized)
                .visible(!setup_startup.background);

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
                        let _ = win.set_focus();
                        eng_for_tray.window_shown();
                    }
                }
                "quit" => {
                    eng_for_tray.shutdown();
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

                let title = eng_for_ticker.snapshot().tracking.title;
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
            }
            #[cfg(not(target_os = "windows"))]
            {
                eng_for_events.shutdown();
            }
        }
        _ => {}
    });

    let app = builder.build(tauri::generate_context!())?;
    app.run(|_app_handle, _event| {});

    #[cfg(target_os = "linux")]
    engine.shutdown_plugins();
    #[cfg(target_os = "linux")]
    runtime.block_on(plugin_api_server.shutdown());

    runtime.shutdown_timeout(Duration::from_secs(1));
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
