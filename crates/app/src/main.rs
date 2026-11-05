#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::io;
use std::process::Command;
#[cfg(target_os = "linux")]
use std::process::Stdio;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use gamenode_app::AppWindow;
use gamenode_app::engine::{Engine, Ports};
use gamenode_core::geometry::{self, Display};
use gamenode_core::library::LibraryStore;
use gamenode_core::ports::SettingsStore;
use gamenode_core::{AuthView, Input, RendererPreference, Snapshot, StorageMethod, WindowGeometry};
use gamenode_platform::{
    browser::SystemBrowser, credentials, db::SqliteSettingsStore, display, ipc, paths::DataPaths,
    random,
};
use slint::ComponentHandle;

const SOFTWARE_FALLBACK_ARG: &str = "--gamenode-internal-software-fallback";
const GPU_STARTUP_ERROR_ARG_PREFIX: &str = "--gamenode-internal-gpu-error=";
const FALLBACK_OWNER_WAIT: Duration = Duration::from_secs(5);
const BOTH_RENDERERS_ERROR: &str = "GameNode could not initialize either renderer.\nSkia error: {gpu}\nSoftware error: {software}\nYour saved settings and session were preserved. Update your graphics driver or contact support with these details.";
const SOFTWARE_RENDERER_ERROR: &str = "GameNode could not initialize software rendering.\nError: {software}\nYour saved settings and session were preserved. Update your graphics driver or contact support with these details.";
const FALLBACK_RESTART_ERROR: &str = "Skia failed to initialize: {gpu}\nGameNode could not restart with software rendering: {error}\nYour saved settings and session were preserved. Restart after updating your graphics driver.";

fn render(ui: &AppWindow, snapshot: Snapshot) {
    ui.set_playtime_error(snapshot.playtime_error);
    ui.set_sync_enabled(snapshot.sync.enabled);
    ui.set_store_playtime(
        snapshot.playtime_source == gamenode_core::playtime::PlaytimeSource::Store,
    );
    ui.set_sync_explanation(
        !snapshot.sync.explained && matches!(snapshot.auth, AuthView::SignedIn { .. }),
    );
    ui.set_background_supported(cfg!(windows));
    gamenode_app::library_ui::render(ui, &snapshot);
    ui.set_signed_in(matches!(snapshot.auth, AuthView::SignedIn { .. }));
    ui.set_busy(matches!(
        snapshot.auth,
        AuthView::Pending { .. } | AuthView::Verifying | AuthView::Starting | AuthView::SigningOut
    ));
    ui.set_can_retry(matches!(snapshot.auth, AuthView::RestoreFailed));
    ui.set_identity(match &snapshot.auth {
        AuthView::SignedIn { user } => user.display_name().into(),
        _ => "".into(),
    });
    ui.set_software(snapshot.settings.renderer == RendererPreference::Software);
    ui.set_storage(
        match snapshot.storage {
            StorageMethod::WindowsCredentialManager => {
                "Session storage: Windows Credential Manager"
            }
            StorageMethod::SecretService => "Session storage: Secret Service",
            StorageMethod::PrivateFile => {
                "Session storage: a file readable only by your user account"
            }
        }
        .into(),
    );
    ui.set_status(if let Some(notice) = snapshot.notice {
        match notice {
            gamenode_core::Notice::BrowserLaunchFailed => "Could not open your browser. Please try again.",
            gamenode_core::Notice::SignInUnavailable => "Could not start sign-in. Please try again.",
            gamenode_core::Notice::ProviderRejected => "Sign-in was rejected. Please try again.",
            gamenode_core::Notice::AttemptExpired => "Sign-in expired. Please try again.",
            gamenode_core::Notice::SignInCancelled => "Sign-in cancelled.",
            gamenode_core::Notice::SignInRejected => "The session was rejected. Please sign in again.",
            gamenode_core::Notice::ConnectionProblem => "Could not connect. Your stored session is preserved; retry when online.",
            gamenode_core::Notice::SessionRejected => "Your stored session expired. Please sign in again.",
            gamenode_core::Notice::CredentialUnreadable => "Could not read your session from credential storage.",
            gamenode_core::Notice::PersistFailed => "Could not save your session. Sign-in will not survive restart.",
            gamenode_core::Notice::SignOutNotConfirmed => "Signed out locally. Server sign-out could not be confirmed.",
            gamenode_core::Notice::CredentialDeleteFailed => "Could not remove the saved credential. Check credential storage before restarting.",
        }
    } else if matches!(snapshot.auth, AuthView::Pending { .. }) { "Complete sign-in in your browser." } else { "" }.into());
}

fn show_window(
    ui: &AppWindow,
    engine: &Engine,
    errors: &std::sync::Mutex<Option<slint::PlatformError>>,
) {
    engine.window_shown();
    if let Err(error) = ui.show() {
        *errors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
        let _ = slint::quit_event_loop();
    }
}

fn restart_with_software(
    owner: ipc::OwnerGuard,
    runtime: tokio::runtime::Runtime,
    gpu_error: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let child = std::env::current_exe().and_then(|executable| {
        Command::new(executable)
            .args(
                std::env::args_os()
                    .skip(1)
                    .filter(|argument| !is_internal_startup_argument(argument)),
            )
            .arg(SOFTWARE_FALLBACK_ARG)
            .arg(format!("{GPU_STARTUP_ERROR_ARG_PREFIX}{gpu_error}"))
            .spawn()
    });

    // The child waits briefly for this process to release single-instance ownership.
    drop(owner);
    runtime.shutdown_timeout(Duration::from_secs(1));

    match child {
        Ok(_child) => Ok(()),
        Err(error) => {
            let detail = localized_catalog_message(
                FALLBACK_RESTART_ERROR,
                &[("gpu", gpu_error), ("error", &error.to_string())],
            );
            show_renderer_error(&detail);
            Err(io::Error::other(detail).into())
        }
    }
}

fn report_renderer_failure(
    error: impl std::fmt::Display,
    gpu_error: Option<&str>,
    owner: ipc::OwnerGuard,
    runtime: tokio::runtime::Runtime,
) -> Result<(), Box<dyn std::error::Error>> {
    let software_error = error.to_string();
    let detail = if let Some(gpu_error) = gpu_error {
        localized_catalog_message(
            BOTH_RENDERERS_ERROR,
            &[("gpu", gpu_error), ("software", &software_error)],
        )
    } else {
        localized_catalog_message(SOFTWARE_RENDERER_ERROR, &[("software", &software_error)])
    };
    drop(owner);
    runtime.shutdown_timeout(Duration::from_secs(1));
    show_renderer_error(&detail);
    Err(io::Error::other(detail).into())
}

fn renderer_startup_failure(
    renderer: &str,
    fallback_process: bool,
    error: impl std::fmt::Display,
    gpu_error: Option<&str>,
    owner: ipc::OwnerGuard,
    runtime: tokio::runtime::Runtime,
) -> Result<(), Box<dyn std::error::Error>> {
    let error = error.to_string();
    if should_retry_with_software(renderer, fallback_process) {
        return restart_with_software(owner, runtime, &error);
    }
    report_renderer_failure(error, gpu_error, owner, runtime)
}

fn renderer_for_startup(preference: RendererPreference, fallback_process: bool) -> &'static str {
    if preference == RendererPreference::Software || fallback_process {
        "software"
    } else {
        "skia"
    }
}

fn automatic_fallback_active(preference: RendererPreference, fallback_process: bool) -> bool {
    fallback_process && preference != RendererPreference::Software
}

fn should_retry_with_software(renderer: &str, fallback_process: bool) -> bool {
    renderer == "skia" && !fallback_process
}

struct StartupArguments {
    background: bool,
    fallback_process: bool,
    incoming: ipc::Message,
    gpu_error: Option<String>,
}

fn parse_startup_arguments(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> StartupArguments {
    let arguments: Vec<std::ffi::OsString> = arguments.into_iter().collect();
    let fallback_process = arguments
        .iter()
        .any(|argument| argument == std::ffi::OsStr::new(SOFTWARE_FALLBACK_ARG));
    let gpu_error = arguments.iter().find_map(|argument| {
        argument
            .to_str()
            .and_then(|value| value.strip_prefix(GPU_STARTUP_ERROR_ARG_PREFIX))
            .map(str::to_owned)
    });
    let incoming = arguments
        .iter()
        .find(|argument| !is_internal_startup_argument(argument))
        .and_then(|argument| argument.to_str())
        .map(|url| ipc::Message::AuthCallback {
            url: url.to_owned(),
        })
        .unwrap_or(ipc::Message::Activate);
    StartupArguments {
        background: arguments.iter().any(|arg| arg == "--background"),
        fallback_process,
        incoming,
        gpu_error,
    }
}

fn is_internal_startup_argument(argument: &std::ffi::OsStr) -> bool {
    argument == "--background"
        || argument == SOFTWARE_FALLBACK_ARG
        || argument
            .to_str()
            .is_some_and(|value| value.starts_with(GPU_STARTUP_ERROR_ARG_PREFIX))
}

fn localized_catalog_message(template: &str, values: &[(&str, &str)]) -> String {
    catalog_message_for_locale(template, values, &system_locale())
}

fn catalog_message_for_locale(template: &str, values: &[(&str, &str)], locale: &str) -> String {
    let catalog = if locale.to_ascii_lowercase().starts_with("pt") {
        include_str!("../../../i18n/pt_BR/LC_MESSAGES/gamenode-app.po")
    } else {
        include_str!("../../../i18n/en/LC_MESSAGES/gamenode-app.po")
    };
    let translated = catalog.split("\n\n").find_map(|entry| {
        let id = entry
            .lines()
            .find_map(|line| line.strip_prefix("msgid "))
            .and_then(|value| serde_json::from_str::<String>(value).ok());
        (id.as_deref() == Some(template))
            .then(|| {
                entry
                    .lines()
                    .find_map(|line| line.strip_prefix("msgstr "))
                    .and_then(|value| serde_json::from_str::<String>(value).ok())
            })
            .flatten()
    });
    let mut message = translated.unwrap_or_else(|| template.to_owned());
    for (key, value) in values {
        message = message.replace(&format!("{{{key}}}"), value);
    }
    message
}

fn system_locale() -> String {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Globalization::{GetUserDefaultLocaleName, MAX_LOCALE_NAME};
        let mut locale = [0u16; MAX_LOCALE_NAME as usize];
        // The Windows API writes a NUL-terminated locale name to this fixed-size buffer.
        let written = unsafe { GetUserDefaultLocaleName(locale.as_mut_ptr(), locale.len() as i32) };
        if written > 0 {
            return String::from_utf16_lossy(&locale[..written as usize]);
        }
        String::new()
    }
    #[cfg(not(target_os = "windows"))]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
            .unwrap_or_default()
    }
}

fn capture_window_geometry(
    window: &slint::Window,
    mut geometry: WindowGeometry,
    can_position: bool,
) -> WindowGeometry {
    let scale = window.scale_factor().max(f32::EPSILON);
    geometry.maximized = window.is_maximized();
    if !geometry.maximized {
        let size = window.size();
        geometry.width = (size.width as f32 / scale).round().max(1.0) as u32;
        geometry.height = (size.height as f32 / scale).round().max(1.0) as u32;
        if can_position {
            let position = window.position();
            geometry.x = Some((position.x as f32 / scale).round() as i32);
            geometry.y = Some((position.y as f32 / scale).round() as i32);
        }
    }
    geometry
}

fn logical_displays(physical: Vec<Display>, scale: f32) -> Vec<Display> {
    let scale = scale.max(f32::EPSILON);
    physical
        .into_iter()
        .map(|display| Display {
            x: (display.x as f32 / scale).round() as i32,
            y: (display.y as f32 / scale).round() as i32,
            width: (display.width as f32 / scale).round().max(1.0) as u32,
            height: (display.height as f32 / scale).round().max(1.0) as u32,
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn show_renderer_error(message: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    let title: Vec<u16> = std::ffi::OsStr::new("GameNode Desktop")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let text: Vec<u16> = std::ffi::OsStr::new(message)
        .encode_wide()
        .chain(Some(0))
        .collect();
    // The process is a Windows GUI application, so use a native dialog for startup errors.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(target_os = "linux")]
fn show_renderer_error(message: &str) {
    for (program, args) in [
        (
            "zenity",
            vec!["--error", "--title=GameNode Desktop", "--text", message],
        ),
        (
            "kdialog",
            vec!["--error", message, "--title", "GameNode Desktop"],
        ),
        (
            "xmessage",
            vec!["-center", "-title", "GameNode Desktop", message],
        ),
        (
            "notify-send",
            vec!["--urgency=critical", "GameNode Desktop", message],
        ),
    ] {
        if Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return;
        }
    }
    eprintln!("{message}");
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn show_renderer_error(message: &str) {
    eprintln!("{message}");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Diagnostics (invalid provider manifests, skipped scan files) go to stderr; `RUST_LOG` tunes it.
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
    let fallback_process = startup.fallback_process;
    let incoming = startup.incoming;
    let gpu_error = startup.gpu_error;
    let endpoint = ipc::Endpoint::for_current_user()?;
    let owner = match ipc::acquire(&endpoint, runtime.handle())? {
        ipc::Instance::Owner(owner) => owner,
        ipc::Instance::Existing(existing) => {
            let fallback_owner = if fallback_process {
                let deadline = Instant::now() + FALLBACK_OWNER_WAIT;
                loop {
                    match ipc::acquire(&endpoint, runtime.handle())? {
                        ipc::Instance::Owner(owner) => break Some(owner),
                        ipc::Instance::Existing(_) if Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(50));
                        }
                        ipc::Instance::Existing(_) => break None,
                    }
                }
            } else {
                None
            };
            if let Some(owner) = fallback_owner {
                owner
            } else {
                ipc::forward_launch(&existing, &incoming, startup.background, ipc::FORWARD_WAIT)?;
                return Ok(());
            }
        }
    };
    let mut owner = owner.serve(runtime.handle());
    #[cfg(windows)]
    if let Err(error) = gamenode_platform::autostart::register() {
        tracing::warn!(%error,"could not register login startup");
    }
    let paths = DataPaths::resolve().ok_or("Could not locate your application data directory")?;
    paths.ensure_data_dir()?;
    let store = Arc::new(SqliteSettingsStore::open(&paths.database_file())?);
    let settings = store.load()?;
    // Bundled provider manifests are validated once at startup; invalid ones are logged and skipped.
    let platform = gamenode_core::provider::Platform::current()
        .ok_or("GameNode Desktop supports Windows and Linux only")?;
    let providers = gamenode_app::providers::load_bundled(platform).providers;
    let library_store = Arc::new(store.library_store());
    let layout = library_store.load_layout().unwrap_or_else(|error| {
        tracing::warn!(%error, "could not read the saved library layout; using the default");
        Default::default()
    });
    let library = gamenode_core::library::LibraryMachine::new(providers, layout);
    let renderer = renderer_for_startup(settings.renderer, fallback_process);
    if let Err(error) = slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name(renderer.into())
        .select()
    {
        return renderer_startup_failure(
            renderer,
            fallback_process,
            error,
            gpu_error.as_deref(),
            owner,
            runtime,
        );
    }
    let locale = system_locale();
    let _ = slint::select_bundled_translation(if locale.to_ascii_lowercase().starts_with("pt") {
        "pt_BR"
    } else {
        ""
    });
    let ui = match AppWindow::new() {
        Ok(ui) => ui,
        Err(error) => {
            return renderer_startup_failure(
                renderer,
                fallback_process,
                error,
                gpu_error.as_deref(),
                owner,
                runtime,
            );
        }
    };
    let show_errors = Arc::new(std::sync::Mutex::new(None));
    let first_frame_rendered = Arc::new(AtomicBool::new(false));
    let rendered = first_frame_rendered.clone();
    if let Err(error) = ui.window().set_rendering_notifier(move |state, _| {
        if matches!(state, slint::RenderingState::AfterRendering) {
            rendered.store(true, Ordering::Release);
        }
    }) {
        return renderer_startup_failure(
            renderer,
            fallback_process,
            error,
            gpu_error.as_deref(),
            owner,
            runtime,
        );
    }
    ui.set_version(env!("GAMENODE_BUILD_VERSION").into());
    // Bound each text layout while retaining every character of the bundled notices.
    let notices = concat!(
        include_str!("../../../LICENSE"),
        "\n\n",
        include_str!("../../../licenses/README.md"),
        "\n\n",
        include_str!("../../../licenses/Roboto-OFL-1.1.txt"),
        "\n\n",
        include_str!("../../../licenses/THIRD-PARTY-NOTICES.txt")
    );
    let characters: Vec<char> = notices.chars().collect();
    let pages: Vec<slint::SharedString> = characters
        .chunks(2400)
        .map(|chunk| chunk.iter().collect::<String>().into())
        .collect();
    ui.set_license_page_count(pages.len() as i32);
    ui.set_licenses(pages[0].clone());
    let weak = ui.as_weak();
    ui.on_license_page_changed(move |page| {
        if let Some(ui) = weak.upgrade()
            && let Some(text) = pages.get(page as usize)
        {
            ui.set_license_page(page);
            ui.set_licenses(text.clone());
        }
    });
    ui.set_active_software(renderer == "software");
    ui.set_fallback_renderer(automatic_fallback_active(
        settings.renderer,
        fallback_process,
    ));
    let can_position = display::can_position_windows();
    let displays = logical_displays(display::available_displays(), ui.window().scale_factor());
    let window_plan = geometry::plan_window(settings.window, &displays, can_position);
    ui.window().set_size(slint::LogicalSize::new(
        window_plan.size.0 as f32,
        window_plan.size.1 as f32,
    ));
    if let Some((x, y)) = window_plan.position {
        ui.window()
            .set_position(slint::LogicalPosition::new(x as f32, y as f32));
    }
    ui.window().set_maximized(window_plan.maximized);
    let initial_geometry = settings.window.unwrap_or(WindowGeometry {
        x: window_plan.position.map(|position| position.0),
        y: window_plan.position.map(|position| position.1),
        width: window_plan.size.0,
        height: window_plan.size.1,
        maximized: window_plan.maximized,
    });
    let current_geometry = Arc::new(std::sync::Mutex::new(initial_geometry));
    let last_saved_geometry = Arc::new(std::sync::Mutex::new(settings.window));
    let credentials = {
        let paths = paths.clone();
        std::thread::spawn(move || credentials::open_store(&paths))
            .join()
            .map_err(|_| "Credential storage initialization failed")?
    };
    let weak = ui.as_weak();
    let client = Arc::new(gamenode_api::GameNodeClient::new(
        gamenode_api::server_url_from_env()?,
    )?);
    let engine = Engine::new(
        Ports {
            backups: Some(Arc::new(gamenode_platform::backups::Ludusavi::bundled(paths.data_dir())?)),
            local_games: Some(Arc::new(store.local_game_store())),
            installations: Some(Arc::new(
                gamenode_platform::installations::WindowsInstallations,
            )),
            processes: Arc::new(gamenode_platform::processes::HostProcesses::new()),
            playtime: Arc::new(store.playtime_store()),
            playtime_api: client.clone(),
            settings: store,
            credentials,
            api: client.clone(),
            browser: Arc::new(SystemBrowser),
            library: library_store,
            games: client,
            files: Arc::new(gamenode_platform::host::StdHostFiles),
            roots: Arc::new(gamenode_platform::host::SystemRoots::new()),
            uri: Arc::new(gamenode_platform::uri::SystemUriOpener),
            covers_dir: paths.covers_dir(),
            platform,
        },
        runtime.handle().clone(),
        settings,
        library,
        Box::new(random::nonce),
        move |snapshot| {
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    render(&ui, snapshot);
                }
            });
        },
    );
    gamenode_app::library_ui::install(&ui, &engine, paths.covers_dir());
    let e = engine.clone();
    ui.on_sign_in(move |index| {
        if let Some(provider) = gamenode_core::Provider::ALL.get(index as usize) {
            e.dispatch(Input::StartSignIn(*provider));
        }
    });
    let e = engine.clone();
    ui.on_cancel(move || e.dispatch(Input::CancelSignIn));
    let e = engine.clone();
    ui.on_retry(move || e.dispatch(Input::RetryRestore));
    let e = engine.clone();
    ui.on_sign_out(move || e.dispatch(Input::SignOut));
    let e = engine.clone();
    ui.on_renderer_changed(move |software| {
        e.update_settings(|s| {
            s.renderer = if software {
                RendererPreference::Software
            } else {
                RendererPreference::Gpu
            }
        })
    });
    let e = engine.clone();
    ui.on_sync_changed(move |enabled| e.set_sync(enabled));
    let e = engine.clone();
    ui.on_source_changed(move |store| {
        e.set_playtime_source(if store {
            gamenode_core::playtime::PlaytimeSource::Store
        } else {
            gamenode_core::playtime::PlaytimeSource::Internal
        })
    });
    let e = engine.clone();
    ui.on_sync_dismissed(move || e.dismiss_sync_explanation());
    let e = engine.clone();
    ui.on_quit(move || {
        e.shutdown();
        let _ = slint::quit_event_loop();
    });
    let weak = ui.as_weak();
    let e = engine.clone();
    let (placeholder_sender, placeholder_receiver) = tokio::sync::mpsc::channel(1);
    drop(placeholder_sender);
    let mut owner_messages = std::mem::replace(&mut owner.messages, placeholder_receiver);
    let ipc_errors = show_errors.clone();
    let ipc_task = runtime.spawn(async move {
        while let Some(message) = owner_messages.recv().await {
            if let ipc::Message::AuthCallback { url } = message {
                e.dispatch(Input::CallbackReceived(url));
            }
            // Another launch is the user coming back: pick up games installed meanwhile.
            let engine = e.clone();
            let errors = ipc_errors.clone();
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    show_window(&ui, &engine, &errors);
                }
            });
        }
    });
    let e = engine.clone();
    let timer = slint::Timer::default();
    let weak = ui.as_weak();
    let ready = first_frame_rendered.clone();
    let current = current_geometry.clone();
    let last_saved = last_saved_geometry.clone();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(1),
        move || {
            e.dispatch(Input::Tick);
            e.maintenance();
            if !ready.load(Ordering::Acquire) {
                return;
            }
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let geometry = {
                let mut stored = current
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *stored = capture_window_geometry(ui.window(), *stored, can_position);
                *stored
            };
            let mut last_saved = last_saved
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *last_saved != Some(geometry) {
                *last_saved = Some(geometry);
                e.update_settings(|settings| settings.window = Some(geometry));
            }
        },
    );
    engine.start();
    if let ipc::Message::AuthCallback { url } = incoming {
        engine.dispatch(Input::CallbackReceived(url));
    }
    #[cfg(windows)]
    let tray = {
        let tray = gamenode_app::Tray::new()?;
        let weak = ui.as_weak();
        let e = engine.clone();
        let errors = show_errors.clone();
        tray.on_open(move || {
            if let Some(ui) = weak.upgrade() {
                show_window(&ui, &e, &errors);
            }
        });
        let e = engine.clone();
        tray.on_quit(move || {
            e.shutdown();
            let _ = slint::quit_event_loop();
        });
        tray
    };
    #[cfg(windows)]
    {
        let e = engine.clone();
        let current = current_geometry.clone();
        let weak = ui.as_weak();
        ui.window().on_close_requested(move || {
            if let Some(ui) = weak.upgrade() {
                let geometry = capture_window_geometry(
                    ui.window(),
                    *current
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                    can_position,
                );
                e.update_settings(|s| s.window = Some(geometry));
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
    #[cfg(windows)]
    let tray_timer = {
        let weak = tray.as_weak();
        let e = engine.clone();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(1),
            move || {
                if let Some(tray) = weak.upgrade() {
                    let snapshot = e.snapshot();
                    let title = &snapshot.tracking.title;
                    tray.set_playing_title(title.as_str().into());
                }
            },
        );
        timer
    };
    let result = if cfg!(windows) {
        if !startup.background {
            engine.window_shown();
            ui.show().and_then(|_| slint::run_event_loop_until_quit())
        } else {
            slint::run_event_loop_until_quit()
        }
    } else {
        ui.run()
    };
    let result = show_errors
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .map_or(result, Err);
    engine.shutdown();
    #[cfg(windows)]
    {
        tray_timer.stop();
        drop(tray);
    }
    ipc_task.abort();
    timer.stop();
    drop(engine);
    if let Err(error) = result {
        if !first_frame_rendered.load(Ordering::Acquire) {
            if should_retry_with_software(renderer, fallback_process) {
                return restart_with_software(owner, runtime, &error.to_string());
            }
            return report_renderer_failure(error, gpu_error.as_deref(), owner, runtime);
        }
        drop(owner);
        runtime.shutdown_timeout(Duration::from_secs(1));
        return Err(error.into());
    }
    drop(owner);
    runtime.shutdown_timeout(Duration::from_secs(1));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_preference_selects_skia_by_default() {
        assert_eq!(renderer_for_startup(RendererPreference::Gpu, false), "skia");
        assert!(!automatic_fallback_active(RendererPreference::Gpu, false));
    }

    #[test]
    fn persisted_software_preference_is_applied_on_restart() {
        assert_eq!(
            renderer_for_startup(RendererPreference::Software, false),
            "software"
        );
        assert!(!automatic_fallback_active(
            RendererPreference::Software,
            false
        ));
    }

    #[test]
    fn automatic_fallback_keeps_the_saved_gpu_preference_separate() {
        let saved_preference = RendererPreference::Gpu;
        assert_eq!(renderer_for_startup(saved_preference, true), "software");
        assert!(automatic_fallback_active(saved_preference, true));
        assert_eq!(saved_preference, RendererPreference::Gpu);
    }

    #[test]
    fn renderer_retry_is_bounded_to_one_software_attempt() {
        assert!(should_retry_with_software("skia", false));
        assert!(!should_retry_with_software("software", false));
        assert!(!should_retry_with_software("skia", true));
    }

    #[test]
    fn fallback_restart_argument_does_not_replace_or_delay_a_callback() {
        let startup = parse_startup_arguments([
            "app.gamenode://home?cookie=one%20two".into(),
            SOFTWARE_FALLBACK_ARG.into(),
            format!("{GPU_STARTUP_ERROR_ARG_PREFIX}device unavailable").into(),
        ]);
        assert!(startup.fallback_process);
        assert_eq!(startup.gpu_error.as_deref(), Some("device unavailable"));
        assert_eq!(
            startup.incoming,
            ipc::Message::AuthCallback {
                url: "app.gamenode://home?cookie=one%20two".into()
            }
        );
    }

    #[test]
    fn background_is_an_internal_argument_and_is_dropped_on_renderer_restart() {
        let startup = parse_startup_arguments(["--background".into()]);
        assert!(startup.background);
        assert_eq!(startup.incoming, ipc::Message::Activate);
        let args: Vec<std::ffi::OsString> =
            ["--background", SOFTWARE_FALLBACK_ARG, "app.gamenode://home"]
                .map(Into::into)
                .into();
        let retained: Vec<_> = args
            .into_iter()
            .filter(|arg| !is_internal_startup_argument(arg))
            .collect();
        assert_eq!(
            retained,
            vec![std::ffi::OsString::from("app.gamenode://home")]
        );
    }

    #[test]
    fn fallback_restart_argument_alone_is_only_activation() {
        let startup = parse_startup_arguments([SOFTWARE_FALLBACK_ARG.into()]);
        assert!(startup.fallback_process);
        assert_eq!(startup.incoming, ipc::Message::Activate);
    }

    #[test]
    fn startup_error_messages_use_pt_br_and_fall_back_to_english() {
        let portuguese = catalog_message_for_locale(
            SOFTWARE_RENDERER_ERROR,
            &[("software", "initialization failed")],
            "pt_BR",
        );
        assert!(portuguese.contains("renderização por software"));
        assert!(portuguese.contains("initialization failed"));

        let english = catalog_message_for_locale(
            SOFTWARE_RENDERER_ERROR,
            &[("software", "initialization failed")],
            "fr_FR",
        );
        assert!(english.contains("could not initialize software rendering"));
        assert!(english.contains("initialization failed"));
    }
}
