use std::sync::Arc;

use crate::engine::Engine;
use gamenode_core::{AppPreferencePatch, Locale, Snapshot, WindowGeometry};

/// App metadata the React shell needs at startup.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// The interface language the UI should use: the preference, or the normalized OS locale
    /// while the preference follows the system.
    pub locale: String,
    /// The normalized OS locale, kept apart so Settings can show "System language" even when the
    /// effective locale is an explicit choice.
    pub system_locale: String,
    pub version: String,
    /// UTC build date as `YYYY-MM-DD` (`SOURCE_DATE_EPOCH` when set, otherwise compile time).
    pub build_date: String,
    /// A human-readable OS name and version, read once on a worker.
    pub os_version: String,
    /// The build target architecture.
    pub arch: String,
    pub background_supported: bool,
    /// Platform reported to the UI; PC ports is Windows-only in this release.
    pub platform: Option<gamenode_core::ports::PortPlatform>,
    /// Directory of the rotating log files; shown in Settings > About.
    pub log_directory: Option<String>,
}

/// The OS name, read once. The first caller runs on a blocking worker.
fn os_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(gamenode_platform::system::os_version)
}

/// Normalizes raw OS locale strings into supported UI locales ("en" or "pt-BR").
pub fn normalize_locale(raw: &str) -> &'static str {
    let lower = raw.trim().to_ascii_lowercase().replace('_', "-");
    if lower == "pt" || lower.starts_with("pt-") {
        "pt-BR"
    } else {
        "en"
    }
}

/// The effective interface language: an explicit preference wins, while `System` normalizes the
/// OS locale and falls back to English for unsupported languages.
pub fn effective_locale(preference: Locale, system_locale: &str) -> &'static str {
    match preference {
        Locale::System => normalize_locale(system_locale),
        Locale::En => "en",
        Locale::PtBr => "pt-BR",
    }
}

/// The tray menu labels for an effective locale.
pub fn tray_labels(locale: &str) -> (&'static str, &'static str) {
    if locale == "pt-BR" {
        ("Abrir o GameNode", "Sair")
    } else {
        ("Open GameNode", "Quit")
    }
}

/// The tray menu items the host relabels when the language preference changes. Registered as
/// managed state by `main`'s setup; a command only reads it.
pub struct TrayLabels {
    open: tauri::menu::MenuItem<tauri::Wry>,
    quit: tauri::menu::MenuItem<tauri::Wry>,
}

impl TrayLabels {
    pub fn new(
        open: tauri::menu::MenuItem<tauri::Wry>,
        quit: tauri::menu::MenuItem<tauri::Wry>,
    ) -> Self {
        Self { open, quit }
    }

    /// Replaces the labels in the live menu. Tauri dispatches the change to the main thread.
    pub fn apply_locale(&self, locale: &str) {
        let (open, quit) = tray_labels(locale);
        if let Err(error) = self.open.set_text(open) {
            tracing::warn!(%error, "could not relabel the tray's open item");
        }
        if let Err(error) = self.quit.set_text(quit) {
            tracing::warn!(%error, "could not relabel the tray's quit item");
        }
    }
}

/// What closing the main window does on Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAction {
    /// Hide the window and keep tracking; the tray stays.
    HideToTray,
    /// End the application through the Quit path.
    Quit,
}

/// The close-request decision: the tray preference on keeps today's hide behavior; off runs the
/// Quit path instead.
pub fn close_action(close_to_tray: bool) -> CloseAction {
    if close_to_tray {
        CloseAction::HideToTray
    } else {
        CloseAction::Quit
    }
}

/// Detects the system locale from OS settings or environment variables.
pub fn system_locale() -> String {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Globalization::{GetUserDefaultLocaleName, MAX_LOCALE_NAME};
        let mut locale = [0u16; MAX_LOCALE_NAME as usize];
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

/// The shortest time between two snapshots sent to the WebView.
const SNAPSHOT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

#[derive(Default)]
struct PublisherState {
    /// The newest snapshot that has not been sent yet.
    pending: Option<Snapshot>,
    /// Whether a task is already waiting to send `pending`.
    scheduled: bool,
    /// The newest snapshot the WebView has, so an unchanged one is not sent again.
    last_sent: Option<Snapshot>,
    last_sent_at: Option<std::time::Instant>,
}

/// Throttles engine snapshots before they are emitted to the WebView.
///
/// The first snapshot after a quiet period is sent right away. Snapshots that follow within
/// [`SNAPSHOT_INTERVAL`] collapse into one, sent when the interval ends, so a burst (cover
/// downloads, playtime ticks) costs one emission and the newest state is always the last one
/// sent. A snapshot equal to the one already sent is dropped.
#[derive(Clone)]
pub struct SnapshotPublisher {
    state: Arc<std::sync::Mutex<PublisherState>>,
    emitter: Arc<dyn Fn(Snapshot) + Send + Sync>,
}

impl SnapshotPublisher {
    pub fn new(emitter: impl Fn(Snapshot) + Send + Sync + 'static) -> Self {
        Self {
            state: Arc::default(),
            emitter: Arc::new(emitter),
        }
    }

    pub fn from_app_handle(app: tauri::AppHandle) -> Self {
        use tauri::Emitter;
        Self::new(move |snapshot| {
            let _ = app.emit("engine:snapshot", &snapshot);
        })
    }

    /// Hands a snapshot over without blocking: it is sent from a background task.
    pub fn publish(&self, snapshot: Snapshot) {
        let wait = {
            let mut state = lock(&self.state);
            state.pending = Some(snapshot);
            if state.scheduled {
                return;
            }
            state.scheduled = true;
            state.last_sent_at.map_or(std::time::Duration::ZERO, |at| {
                SNAPSHOT_INTERVAL.saturating_sub(at.elapsed())
            })
        };

        let state = self.state.clone();
        let emitter = self.emitter.clone();
        tauri::async_runtime::spawn(async move {
            // Even with no wait, yield so snapshots published back to back share one emission.
            if wait.is_zero() {
                tokio::task::yield_now().await;
            } else {
                tokio::time::sleep(wait).await;
            }
            loop {
                let next = {
                    let mut state = lock(&state);
                    match state.pending.take() {
                        Some(snapshot) if state.last_sent.as_ref() != Some(&snapshot) => {
                            state.last_sent = Some(snapshot.clone());
                            state.last_sent_at = Some(std::time::Instant::now());
                            Some(snapshot)
                        }
                        _ => None,
                    }
                };
                if let Some(snapshot) = next {
                    emitter(snapshot);
                }
                {
                    let mut state = lock(&state);
                    if state.pending.is_none() {
                        state.scheduled = false;
                        break;
                    }
                }
                // Something arrived while sending: send it after the interval.
                tokio::time::sleep(SNAPSHOT_INTERVAL).await;
            }
        });
    }
}

fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Helper to activate or open details for a game entry given its key (or selection fallback).
pub fn activate_entry_by_key(engine: &Engine, key: &str) {
    let target = if !key.is_empty() {
        Some(key.to_owned())
    } else {
        engine.snapshot().library.and_then(|view| {
            view.focused.or_else(|| {
                if let [selected] = view.selected.as_slice() {
                    Some(selected.clone())
                } else {
                    None
                }
            })
        })
    };
    if let Some(target) = target {
        engine.library_input(gamenode_core::library::Input::Select {
            key: target.clone(),
            mode: gamenode_core::library::SelectMode::Replace,
        });
        if let Some(view) = engine.snapshot().library {
            if view.actions.play.is_available() && view.selected.as_slice() == [target.as_str()] {
                engine.open_entry(target, gamenode_core::library::LaunchKind::Play);
            } else {
                engine.library_input(gamenode_core::library::Input::OpenDetails);
            }
        }
    } else {
        engine.library_input(gamenode_core::library::Input::OpenDetails);
    }
}

// ---------------------------------------------------------------------------
// Core & Auth Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_snapshot(engine: tauri::State<'_, Engine>) -> Result<Snapshot, String> {
    Ok(engine.snapshot())
}

#[tauri::command]
pub async fn get_app_info(
    engine: tauri::State<'_, Engine>,
    location: tauri::State<'_, crate::logging::LogLocation>,
) -> Result<AppInfo, String> {
    let system_locale = system_locale();
    let os_version = tokio::task::spawn_blocking(|| os_version().to_owned())
        .await
        .unwrap_or_else(|_| "Unknown".to_owned());
    Ok(AppInfo {
        locale: effective_locale(engine.preferences().locale, &system_locale).to_string(),
        system_locale: normalize_locale(&system_locale).to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        build_date: env!("GAMENODE_BUILD_DATE").to_string(),
        os_version,
        arch: gamenode_platform::system::arch().to_string(),
        background_supported: cfg!(windows),
        platform: Some(gamenode_core::ports::PortPlatform::current()),
        log_directory: Some(location.directory.to_string_lossy().into_owned()),
    })
}

/// Bundled third-party notices. Serialized only when the About panel requests them.
#[tauri::command]
pub async fn get_license_notices() -> Result<String, String> {
    Ok(include_str!("../../../licenses/THIRD-PARTY-NOTICES.txt").to_string())
}

#[tauri::command]
pub async fn dispatch_auth(
    engine: tauri::State<'_, Engine>,
    input: gamenode_core::Input,
) -> Result<(), String> {
    engine.dispatch(input);
    Ok(())
}

#[tauri::command]
pub async fn set_sync_enabled(
    engine: tauri::State<'_, Engine>,
    enabled: bool,
) -> Result<(), String> {
    engine.set_sync(enabled);
    Ok(())
}

/// Settings' "Sync now": rescan and submit every held total under the normal rules. A command
/// sent while a pass runs is ignored; sync off or an unverified session is refused.
#[tauri::command]
pub async fn sync_now(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.sync_now().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_playtime_source(
    engine: tauri::State<'_, Engine>,
    source: gamenode_core::playtime::PlaytimeSource,
) -> Result<(), String> {
    engine.set_playtime_source(source);
    Ok(())
}

/// Applies one application preference (Settings › Geral, Lojas, Jogos DRM-free and Ferramentas).
/// The value is validated, persisted on the storage worker and published in the next snapshot.
/// A lifecycle or language change also re-runs the host side effect: the autostart value and the
/// tray menu labels.
#[tauri::command]
pub async fn set_app_preference(
    tray: tauri::State<'_, TrayLabels>,
    engine: tauri::State<'_, Engine>,
    patch: AppPreferencePatch,
) -> Result<(), String> {
    let locale_changed = matches!(patch, AppPreferencePatch::Locale(_));
    let engine = engine.inner().clone();
    engine
        .commit_app_preference(patch)
        .await
        .map_err(|error| error.to_string())?;
    if locale_changed {
        let preferences = engine.preferences();
        tray.apply_locale(effective_locale(preferences.locale, &system_locale()));
    }
    Ok(())
}

#[tauri::command]
pub async fn dismiss_sync_explanation(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.dismiss_sync_explanation();
    Ok(())
}

#[tauri::command]
pub async fn update_window_geometry(
    engine: tauri::State<'_, Engine>,
    geometry: WindowGeometry,
) -> Result<(), String> {
    engine.update_window_geometry(geometry);
    Ok(())
}

#[tauri::command]
pub async fn quit_app(
    app: tauri::AppHandle,
    engine: tauri::State<'_, Engine>,
) -> Result<(), String> {
    engine.shutdown();
    app.exit(0);
    Ok(())
}

// ---------------------------------------------------------------------------
// Library & Launch Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn library_input(
    engine: tauri::State<'_, Engine>,
    input: gamenode_core::library::Input,
) -> Result<(), String> {
    engine.library_input(input);
    Ok(())
}

#[tauri::command]
pub async fn activate_entry(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    let snapshot = engine.snapshot();
    if let Some(library) = &snapshot.library
        && library.selected.len() == 1
    {
        let key = library.selected[0].clone();
        if library.actions.play.is_available() {
            engine.open_entry(key, gamenode_core::library::LaunchKind::Play);
            return Ok(());
        }
    }
    engine.library_input(gamenode_core::library::Input::OpenDetails);
    Ok(())
}

#[tauri::command]
pub async fn open_entry(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
    kind: gamenode_core::library::LaunchKind,
) -> Result<(), String> {
    engine.open_entry(entry_key, kind);
    Ok(())
}

#[tauri::command]
pub async fn launch_selected(
    engine: tauri::State<'_, Engine>,
    kind: gamenode_core::library::LaunchKind,
) -> Result<(), String> {
    let snapshot = engine.snapshot();
    if let Some(library) = &snapshot.library
        && library.selected.len() == 1
    {
        let key = library.selected[0].clone();
        engine.open_entry(key, kind);
    }
    Ok(())
}

#[tauri::command]
pub async fn dismiss_launch(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.dismiss_launch();
    Ok(())
}

#[tauri::command]
pub async fn rescan_library(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.rescan();
    Ok(())
}

/// Opens a GameNode website page in the default browser (D14). The browser work runs on a
/// blocking worker; a typed error keeps the UI copy localized.
#[tauri::command]
pub async fn open_gamenode_page(
    engine: tauri::State<'_, Engine>,
    page: crate::engine::GameNodePage,
) -> Result<(), crate::engine::GameNodePageError> {
    engine.open_gamenode_page(page).await
}

/// Opens one of the fixed GameNode website links (About footer, Account › Abrir perfil). The UI
/// passes an enum, never a URL; `profile` needs a verified session.
#[tauri::command]
pub async fn open_gamenode_link(
    engine: tauri::State<'_, Engine>,
    kind: crate::engine::GameNodeLink,
) -> Result<(), crate::engine::GameNodePageError> {
    engine.open_gamenode_link(kind).await
}

/// Cached cover file names for the signed-out screen's mural; empty when the cache has none.
#[tauri::command]
pub async fn sign_in_cover_wall(engine: tauri::State<'_, Engine>) -> Result<Vec<String>, String> {
    Ok(engine.inner().clone().sign_in_cover_wall().await)
}

#[tauri::command]
pub async fn remove_entry(
    engine: tauri::State<'_, Engine>,
    key: String,
    allow_rediscovery: bool,
) -> Result<(), String> {
    engine.remove_entry(key, allow_rediscovery);
    Ok(())
}

// ---------------------------------------------------------------------------
// Store Account Commands
// ---------------------------------------------------------------------------

/// Returns trusted packaged plugin metadata. Executable inspection runs off the async worker.
#[tauri::command]
pub async fn plugin_inventory(
    engine: tauri::State<'_, Engine>,
) -> Result<Vec<crate::plugin_host::PluginInfo>, String> {
    let engine = engine.inner().clone();
    tokio::task::spawn_blocking(move || {
        crate::plugin_host::inventory(|id| {
            let (_, connected) = engine.provider_health(id);
            (engine.provider_availability(id), connected)
        })
    })
    .await
    .map_err(|_| "plugin inventory worker failed".to_owned())
}

/// Whether `provider_id` names a provider the host loaded with an account and catalog.
pub(crate) fn catalog_provider_loaded(
    runtime: Option<&Arc<dyn crate::plugin_host::runtime::ProviderRuntime>>,
    provider_id: &str,
) -> bool {
    runtime.is_some_and(|runtime| runtime.accounts().iter().any(|id| id == provider_id))
}

/// Runs one provider-scoped command after the host confirmed the provider is loaded.
async fn run_provider_command<F, Fut>(
    loaded: bool,
    provider_id: &str,
    action: F,
) -> Result<(), String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    if !loaded {
        return Err(format!("unknown provider: {provider_id}"));
    }
    action(provider_id.to_owned()).await;
    Ok(())
}

#[tauri::command]
pub async fn provider_sign_in(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    let engine = engine.inner();
    run_provider_command(
        engine.has_catalog_provider(&provider_id),
        &provider_id,
        |id| async move {
            engine.provider_sign_in(&id).await;
        },
    )
    .await
}

#[tauri::command]
pub async fn provider_focus_sign_in(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    let engine = engine.inner();
    run_provider_command(
        engine.has_catalog_provider(&provider_id),
        &provider_id,
        |id| async move {
            engine.provider_focus_sign_in(&id).await;
        },
    )
    .await
}

#[tauri::command]
pub async fn provider_cancel_sign_in(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    let engine = engine.inner();
    run_provider_command(
        engine.has_catalog_provider(&provider_id),
        &provider_id,
        |id| async move {
            engine.provider_cancel_sign_in(&id).await;
        },
    )
    .await
}

#[tauri::command]
pub async fn provider_sign_out(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    let engine = engine.inner();
    run_provider_command(
        engine.has_catalog_provider(&provider_id),
        &provider_id,
        |id| async move {
            engine.provider_sign_out(&id).await;
        },
    )
    .await
}

#[tauri::command]
pub async fn provider_check(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    let engine = engine.inner();
    run_provider_command(
        engine.has_catalog_provider(&provider_id),
        &provider_id,
        |id| async move {
            engine.provider_check(&id).await;
        },
    )
    .await
}

/// Stops one plugin process for this run (Settings › Plugins › Parar). A pending sign-in is
/// cancelled first and the supervisor's shutdown is bounded.
#[tauri::command]
pub async fn plugin_stop(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    engine.plugin_stop(&provider_id).await
}

/// Starts a stopped plugin with a fresh session and restores its account (Settings › Plugins ›
/// Iniciar).
#[tauri::command]
pub async fn plugin_start(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    engine.plugin_start(&provider_id).await
}

/// The full achievement list, read from the local cache with no GameNode request.
#[tauri::command]
pub async fn achievement_list(
    engine: tauri::State<'_, Engine>,
    key: String,
    filter: gamenode_core::achievements::AchievementFilter,
    sort: gamenode_core::achievements::AchievementSort,
) -> Result<Option<gamenode_core::achievements::AchievementListView>, String> {
    engine.achievement_list(key, filter, sort).await
}

/// Opens the provider's public achievements page for an entry. The host resolves the URL; the UI
/// never passes one. Runs the browser opener on a blocking worker.
#[tauri::command]
pub async fn open_achievements_page(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
) -> Result<(), crate::engine::AchievementPageError> {
    engine.open_achievements_page(entry_key).await
}

// ---------------------------------------------------------------------------
// Diagnostics Commands
// ---------------------------------------------------------------------------

/// The tail of the newest log file, for the Settings screen's report view.
#[tauri::command]
pub async fn get_recent_logs(
    location: tauri::State<'_, crate::logging::LogLocation>,
) -> Result<String, String> {
    let directory = location.directory.clone();
    tokio::task::spawn_blocking(move || crate::logging::read_recent(&directory, 256 * 1024))
        .await
        .map_err(|_| "the log reader stopped".to_owned())?
        .map_err(|error| error.to_string())
}

/// Opens the log directory in the OS file manager.
#[tauri::command]
pub async fn open_logs_folder(
    location: tauri::State<'_, crate::logging::LogLocation>,
) -> Result<(), String> {
    let directory = location.directory.clone();
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&directory)?;
        gamenode_platform::browser::open_directory(&directory)
    })
    .await
    .map_err(|_| "opening the log folder stopped".to_owned())?
    .map_err(|error| error.to_string())
}

/// One detected store as the system information text names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SystemStore {
    pub name: String,
    pub installed_count: usize,
    pub install_root: Option<String>,
}

/// Build, OS and locale facts for the About panel's system information.
#[derive(Clone, Copy)]
pub(crate) struct SystemFacts<'a> {
    pub version: &'a str,
    pub build_date: &'a str,
    pub os_version: &'a str,
    pub arch: &'a str,
    pub locale: &'a str,
    pub home: Option<&'a str>,
}

/// Assembles the About panel's system information. The text carries build and OS facts, the
/// plugins (name, origin, version, status) and detected stores, and never an account name, id,
/// credential or profile path. Every path under the user's profile is replaced with
/// `%USERPROFILE%` (Windows) or `~`.
pub(crate) fn format_system_info(
    facts: &SystemFacts<'_>,
    plugins: &[crate::plugin_host::PluginInfo],
    stores: &[SystemStore],
) -> String {
    let SystemFacts {
        version,
        build_date,
        os_version,
        arch,
        locale,
        home,
    } = *facts;
    let mut text = format!(
        "GameNode Desktop {version}\nBuild {build_date}\n{os_version} {arch}\nLocale: {locale}\n"
    );
    text.push_str("\nPlugins:\n");
    if plugins.is_empty() {
        text.push_str("- none\n");
    } else {
        for plugin in plugins {
            text.push_str(&format!(
                "- {} {} ({}, {})\n",
                plugin.name, plugin.version, plugin.source, plugin.status
            ));
        }
    }
    text.push_str("\nDetected stores:\n");
    if stores.is_empty() {
        text.push_str("- none\n");
    } else {
        for store in stores {
            match &store.install_root {
                Some(root) => text.push_str(&format!(
                    "- {}: {} installed at {root}\n",
                    store.name, store.installed_count
                )),
                None => text.push_str(&format!(
                    "- {}: {} installed\n",
                    store.name, store.installed_count
                )),
            }
        }
    }
    gamenode_platform::system::redact_profile_paths(&text, home)
}

/// The About panel's "Copiar informações do sistema": build, OS and architecture, locale, plugins
/// and detected stores, assembled on a blocking worker and redacted before it reaches the UI.
#[tauri::command]
pub async fn system_info_text(engine: tauri::State<'_, Engine>) -> Result<String, String> {
    let engine = engine.inner().clone();
    tokio::task::spawn_blocking(move || {
        let snapshot = engine.snapshot();
        let plugins = crate::plugin_host::inventory(|id| {
            let (_, connected) = engine.provider_health(id);
            (engine.provider_availability(id), connected)
        });
        let stores = snapshot
            .library
            .as_ref()
            .map(|library| {
                library
                    .providers
                    .iter()
                    .map(|provider| SystemStore {
                        name: provider.name.clone(),
                        installed_count: provider.installed_count,
                        install_root: provider.install_root.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let system_locale = system_locale();
        let locale = effective_locale(snapshot.preferences.locale, &system_locale).to_owned();
        let home = gamenode_platform::system::user_home();
        format_system_info(
            &SystemFacts {
                version: env!("CARGO_PKG_VERSION"),
                build_date: env!("GAMENODE_BUILD_DATE"),
                os_version: os_version(),
                arch: gamenode_platform::system::arch(),
                locale: &locale,
                home: home.as_deref(),
            },
            &plugins,
            &stores,
        )
    })
    .await
    .map_err(|_| "the system information worker stopped".to_owned())
}

// ---------------------------------------------------------------------------
// DRM-Free & Backups Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn pick_local_executable(
    engine: tauri::State<'_, Engine>,
    repair: Option<String>,
) -> Result<(), String> {
    engine.pick_local_executable(repair);
    Ok(())
}

/// Opens the image picker and leaves the converted cover pending for the editor's Save.
#[tauri::command]
pub async fn pick_local_cover(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
) -> Result<Option<String>, String> {
    engine
        .pick_local_cover(entry_key)
        .await
        .map_err(|error| error.code().to_owned())
}

/// Drops an uncommitted cover when the editor is cancelled or replaced.
#[tauri::command]
pub async fn discard_local_cover(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
) -> Result<(), String> {
    let engine = engine.inner().clone();
    tokio::task::spawn_blocking(move || engine.discard_local_cover(entry_key))
        .await
        .map_err(|_| "coverDiscardFailed".to_owned())
}

#[tauri::command]
pub async fn add_local_executable(
    engine: tauri::State<'_, Engine>,
    path: Option<String>,
) -> Result<(), String> {
    engine.add_local_executable(path);
    Ok(())
}

#[tauri::command]
pub async fn edit_local_game(
    engine: tauri::State<'_, Engine>,
    value: gamenode_core::drm_free::Registration,
) -> Result<(), String> {
    engine.edit_local_game(value);
    Ok(())
}

#[tauri::command]
pub async fn repair_local_game(
    engine: tauri::State<'_, Engine>,
    id: String,
    path: Option<String>,
) -> Result<(), String> {
    engine.repair_local_game(id, path);
    Ok(())
}

#[tauri::command]
pub async fn remove_local_game(
    engine: tauri::State<'_, Engine>,
    id: String,
    allow_rediscovery: bool,
) -> Result<(), String> {
    engine.remove_local_game(id, allow_rediscovery);
    Ok(())
}

#[tauri::command]
pub async fn backup_local_now(engine: tauri::State<'_, Engine>, id: String) -> Result<(), String> {
    engine.backup_local_now(id);
    Ok(())
}

#[tauri::command]
pub async fn set_local_backup_settings(
    engine: tauri::State<'_, Engine>,
    automatic: bool,
    destination: String,
    retention: u8,
) -> Result<(), String> {
    if !gamenode_core::drm_free::valid_retention(retention) {
        return Err("unsupported retention value".into());
    }
    engine
        .inner()
        .clone()
        .set_local_backup_settings(automatic, destination, retention)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_local_backup_retention(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
    retention: u8,
) -> Result<(), String> {
    if !gamenode_core::drm_free::valid_retention(retention) {
        return Err("unsupported retention value".into());
    }
    engine.set_local_backup_retention(entry_key, retention);
    Ok(())
}

/// Space used by local backups per game. The directory walk runs on a blocking worker.
#[tauri::command]
pub async fn local_backup_usage(
    engine: tauri::State<'_, Engine>,
) -> Result<gamenode_core::drm_free::BackupUsage, String> {
    let engine = engine.inner().clone();
    tokio::task::spawn_blocking(move || engine.local_backup_usage())
        .await
        .map_err(|_| "the backup usage worker failed".to_owned())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn pick_save_destination(
    _engine: tauri::State<'_, Engine>,
) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(|| gamenode_platform::installations::pick_save_location(true))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_local_profile_automatic(
    engine: tauri::State<'_, Engine>,
    id: String,
    automatic: bool,
) -> Result<(), String> {
    engine.set_local_profile_automatic(id, automatic);
    Ok(())
}

#[tauri::command]
pub async fn configure_local_saves(
    engine: tauri::State<'_, Engine>,
    id: String,
    location: String,
) -> Result<(), String> {
    engine.configure_local_saves(id, location);
    Ok(())
}

#[tauri::command]
pub async fn pick_local_saves(
    engine: tauri::State<'_, Engine>,
    id: String,
    folder: bool,
) -> Result<(), String> {
    engine.pick_local_saves(id, folder);
    Ok(())
}

#[tauri::command]
pub async fn choose_local_save_profile(
    engine: tauri::State<'_, Engine>,
    id: String,
    title: String,
) -> Result<(), String> {
    engine.choose_local_save_profile(id, title);
    Ok(())
}

#[tauri::command]
pub async fn watch_local_folder(
    engine: tauri::State<'_, Engine>,
    path: String,
) -> Result<(), String> {
    engine.watch_local_folder(path);
    Ok(())
}

#[tauri::command]
pub async fn pick_local_watch_folder(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.pick_local_watch_folder();
    Ok(())
}

#[tauri::command]
pub async fn remove_local_watch(
    engine: tauri::State<'_, Engine>,
    id: String,
) -> Result<(), String> {
    engine.remove_local_watch(id);
    Ok(())
}

#[tauri::command]
pub async fn set_local_candidate_executable(
    engine: tauri::State<'_, Engine>,
    fingerprint: String,
    executable: String,
) -> Result<(), String> {
    engine.set_local_candidate_executable(fingerprint, executable);
    Ok(())
}

#[tauri::command]
pub async fn rescan_local_folders(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.rescan_local_folders();
    Ok(())
}

/// Checks one watched folder on the discovery worker. Unavailable folders keep their entries.
#[tauri::command]
pub async fn check_local_watch(engine: tauri::State<'_, Engine>, id: String) -> Result<(), String> {
    engine.check_local_watch(id);
    Ok(())
}

#[tauri::command]
pub async fn select_local_candidate(
    engine: tauri::State<'_, Engine>,
    fingerprint: String,
) -> Result<(), String> {
    engine.select_local_candidate(fingerprint);
    Ok(())
}

#[tauri::command]
pub async fn review_selected_local_candidates(
    engine: tauri::State<'_, Engine>,
    add: bool,
    allow_rediscovery: bool,
) -> Result<(), String> {
    engine.review_selected_local_candidates(add, allow_rediscovery);
    Ok(())
}

#[tauri::command]
pub async fn reset_local_ignores(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.reset_local_ignores();
    Ok(())
}

#[tauri::command]
pub async fn load_local_history(
    engine: tauri::State<'_, Engine>,
    id: String,
) -> Result<(), String> {
    engine.load_local_history(id);
    Ok(())
}

#[tauri::command]
pub async fn preview_local_restore(
    engine: tauri::State<'_, Engine>,
    key: String,
) -> Result<(), String> {
    engine.preview_local_restore(key);
    Ok(())
}

#[tauri::command]
pub async fn confirm_local_restore(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.confirm_local_restore();
    Ok(())
}

#[tauri::command]
pub async fn cancel_local_restore(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.cancel_local_restore();
    Ok(())
}

#[tauri::command]
pub async fn resolve_local_claim(
    engine: tauri::State<'_, Engine>,
    transfer: bool,
) -> Result<(), String> {
    engine.resolve_local_claim(transfer);
    Ok(())
}

#[tauri::command]
pub async fn cancel_local_claim(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.cancel_local_claim();
    Ok(())
}

#[tauri::command]
pub async fn dismiss_local_discovered(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.dismiss_local_discovered();
    Ok(())
}

// ---------------------------------------------------------------------------
// Game Tools (OptiScaler) Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn open_tools_for_selected(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.open_tools_for_selected();
    Ok(())
}

#[tauri::command]
pub async fn open_tools(
    engine: tauri::State<'_, Engine>,
    entry_key: Option<String>,
) -> Result<(), String> {
    engine.open_tools(entry_key);
    Ok(())
}

#[tauri::command]
pub async fn close_tools(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.close_tools();
    Ok(())
}

#[tauri::command]
pub async fn select_tool_channel(
    engine: tauri::State<'_, Engine>,
    channel: gamenode_core::tools::ToolChannel,
) -> Result<(), String> {
    engine.select_tool_channel(channel);
    Ok(())
}

#[tauri::command]
pub async fn refresh_tool_versions(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.refresh_tool_versions();
    Ok(())
}

#[tauri::command]
pub async fn select_tool_version(
    engine: tauri::State<'_, Engine>,
    version: String,
) -> Result<(), String> {
    engine.select_tool_version(version);
    Ok(())
}

#[tauri::command]
pub async fn select_tool_target_exe(
    engine: tauri::State<'_, Engine>,
    relative_exe: String,
) -> Result<(), String> {
    engine.select_tool_target_exe(relative_exe);
    Ok(())
}

/// Opens the native executable picker in the game folder and selects the result on a worker.
#[tauri::command]
pub async fn pick_tool_target_exe(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.pick_tool_target_exe().await;
    Ok(())
}

#[tauri::command]
pub async fn select_tool_proxy(
    engine: tauri::State<'_, Engine>,
    proxy: String,
) -> Result<(), String> {
    engine.select_tool_proxy(proxy);
    Ok(())
}

#[tauri::command]
pub async fn set_tool_anticheat_confirmed(
    engine: tauri::State<'_, Engine>,
    confirmed: bool,
) -> Result<(), String> {
    engine.set_tool_anticheat_confirmed(confirmed);
    Ok(())
}

#[tauri::command]
pub async fn set_tool_reset_config(
    engine: tauri::State<'_, Engine>,
    reset: bool,
) -> Result<(), String> {
    engine.set_tool_reset_config(reset);
    Ok(())
}

#[tauri::command]
pub async fn install_or_update_tool(
    engine: tauri::State<'_, Engine>,
    is_update: bool,
) -> Result<(), String> {
    engine.install_or_update_tool(is_update);
    Ok(())
}

#[tauri::command]
pub async fn uninstall_tool(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.uninstall_tool();
    Ok(())
}

#[tauri::command]
pub async fn open_tool_source(engine: tauri::State<'_, Engine>, url: String) -> Result<(), String> {
    engine.open_tool_source(url);
    Ok(())
}

#[tauri::command]
pub async fn clear_tool_cache(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.clear_tool_cache();
    Ok(())
}

// ---------------------------------------------------------------------------
// PC Ports Commands
// ---------------------------------------------------------------------------

/// A typed PC ports command failure. The UI never parses the message; the code is stable.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "code", content = "message")]
pub enum PortCommandError {
    /// Port installs are Windows-only in this release.
    UnsupportedPlatform,
    /// The workers are still starting.
    NotReady,
    /// The request names something that does not exist or is no longer valid.
    InvalidRequest(String),
    /// The install cannot start for another reason (network, cache, picker).
    Unavailable(String),
    /// The install plan refused the request for a reason the UI words itself.
    InstallBlocked(crate::engine::pc_ports::PortInstallProblem),
    /// A port install is queued, running or waiting for an executable pick; the cache is in use.
    InstallInProgress,
    /// The picker, the local database or another host step failed.
    Failed(String),
}

impl From<crate::engine::pc_ports::PortsError> for PortCommandError {
    fn from(error: crate::engine::pc_ports::PortsError) -> Self {
        use crate::engine::pc_ports::PortsError;
        match error {
            PortsError::Unsupported => Self::UnsupportedPlatform,
            PortsError::NotReady => Self::NotReady,
            PortsError::Invalid(reason) => Self::InvalidRequest(reason),
            PortsError::Unavailable(reason) => Self::Unavailable(reason),
            PortsError::InstallBlocked(problem) => Self::InstallBlocked(problem),
            PortsError::InstallInProgress => Self::InstallInProgress,
            PortsError::Picker(reason) | PortsError::Storage(reason) => Self::Failed(reason),
        }
    }
}

fn require_ports(engine: &Engine) -> Result<(), PortCommandError> {
    if engine.pc_ports_supported() {
        Ok(())
    } else {
        Err(PortCommandError::UnsupportedPlatform)
    }
}

/// Starts the first catalog fetch when PC ports opens. Idempotent and non-blocking.
#[tauri::command]
pub async fn ports_open(engine: tauri::State<'_, Engine>) -> Result<(), PortCommandError> {
    require_ports(&engine)?;
    engine.ports_open();
    Ok(())
}

/// Manual Refresh. The current snapshot stays visible until a validated replacement lands.
#[tauri::command]
pub async fn ports_refresh(engine: tauri::State<'_, Engine>) -> Result<(), PortCommandError> {
    require_ports(&engine)?;
    engine.ports_refresh();
    Ok(())
}

/// Lazy entry detail (releases, assets, withdrawal state).
#[tauri::command]
pub async fn ports_detail(
    engine: tauri::State<'_, Engine>,
    catalog_id: String,
) -> Result<Option<gamenode_core::ports::PortDetailView>, PortCommandError> {
    require_ports(&engine)?;
    Ok(engine.ports_detail(catalog_id).await?)
}

/// Native parent-folder picker. Returns `None` when the user cancels.
#[tauri::command]
pub async fn ports_pick_parent(
    engine: tauri::State<'_, Engine>,
    initial_folder: Option<String>,
) -> Result<Option<String>, PortCommandError> {
    require_ports(&engine)?;
    Ok(engine.ports_pick_parent(initial_folder).await?)
}

/// Native picker plus host validation of one guided-setup slot.
#[tauri::command]
pub async fn ports_pick_supplied(
    engine: tauri::State<'_, Engine>,
    request: crate::engine::pc_ports::PortPickSuppliedRequest,
) -> Result<Option<crate::engine::pc_ports::PortSuppliedPick>, PortCommandError> {
    require_ports(&engine)?;
    Ok(engine.ports_pick_supplied(request).await?)
}

/// Starts a background install. Returns the job id, or `None` when the request was refused.
#[tauri::command]
pub async fn ports_install(
    engine: tauri::State<'_, Engine>,
    request: crate::engine::pc_ports::PortInstallRequest,
) -> Result<Option<String>, PortCommandError> {
    require_ports(&engine)?;
    Ok(engine.ports_install(request).await?)
}

/// Cancels a queued, running or executable-awaiting job.
#[tauri::command]
pub async fn ports_cancel(
    engine: tauri::State<'_, Engine>,
    job_id: String,
) -> Result<(), PortCommandError> {
    require_ports(&engine)?;
    engine.ports_cancel(job_id);
    Ok(())
}

/// Marks the entry's current result seen, dropping it from the sidebar badge.
#[tauri::command]
pub async fn ports_mark_seen(
    engine: tauri::State<'_, Engine>,
    catalog_id: String,
) -> Result<(), PortCommandError> {
    require_ports(&engine)?;
    engine.ports_mark_seen(catalog_id);
    Ok(())
}

/// Persists remembered view, filters and last parent folder.
#[tauri::command]
pub async fn ports_set_preferences(
    engine: tauri::State<'_, Engine>,
    patch: crate::engine::pc_ports::PortPreferencesPatch,
) -> Result<(), PortCommandError> {
    require_ports(&engine)?;
    engine.ports_set_preferences(patch);
    Ok(())
}

/// Follow-up for a completed install whose preferred executable is missing.
#[tauri::command]
pub async fn ports_pick_executable(
    engine: tauri::State<'_, Engine>,
    job_id: String,
) -> Result<Option<String>, PortCommandError> {
    require_ports(&engine)?;
    Ok(engine.ports_pick_executable(job_id).await?)
}

/// The Settings Tools panel's helper-tools and download-cache block.
#[tauri::command]
pub async fn ports_tools_cache(
    engine: tauri::State<'_, Engine>,
) -> Result<gamenode_core::ports::PortToolsCacheView, PortCommandError> {
    require_ports(&engine)?;
    Ok(engine.ports_tools_cache().await?)
}

/// Deletes the cached port downloads and extraction tools. Refused while an install runs.
#[tauri::command]
pub async fn ports_clear_cache(
    engine: tauri::State<'_, Engine>,
) -> Result<gamenode_core::ports::PortToolsCacheView, PortCommandError> {
    require_ports(&engine)?;
    Ok(engine.ports_clear_cache().await?)
}

/// Opens an install folder (or any directory) in the OS file manager.
#[tauri::command]
pub async fn open_install_folder(path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        gamenode_platform::browser::open_directory(std::path::Path::new(&path))
    })
    .await
    .map_err(|_| "the folder could not be opened".to_owned())?
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn snapshot_titled(title: &str) -> Snapshot {
        let mut snapshot = Snapshot::new(
            &gamenode_core::auth::AuthMachine::new(Box::new(|| "nonce".into())),
            gamenode_core::StorageMethod::PrivateFile,
            Default::default(),
            None,
            None,
        );
        snapshot.tracking.title = title.to_owned();
        snapshot
    }

    fn recording_publisher() -> (SnapshotPublisher, Arc<std::sync::Mutex<Vec<String>>>) {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let publisher = SnapshotPublisher::new(move |snapshot| {
            sink.lock().unwrap().push(snapshot.tracking.title);
        });
        (publisher, seen)
    }

    #[tokio::test]
    async fn test_snapshot_coalescing() {
        let (publisher, seen) = recording_publisher();

        // A burst of different snapshots sent without yielding.
        for n in 0..50 {
            publisher.publish(snapshot_titled(&n.to_string()));
        }
        tokio::time::sleep(Duration::from_millis(400)).await;

        let seen = seen.lock().unwrap();
        assert!(
            (1..=3).contains(&seen.len()),
            "expected coalescing, got {} emissions",
            seen.len()
        );
        assert_eq!(seen.last().map(String::as_str), Some("49"), "newest wins");
    }

    #[tokio::test]
    async fn a_snapshot_after_a_quiet_period_is_sent_at_once_and_a_burst_ends_on_the_newest() {
        let (publisher, seen) = recording_publisher();

        publisher.publish(snapshot_titled("first"));
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(
            *seen.lock().unwrap(),
            ["first"],
            "not delayed by the interval"
        );

        publisher.publish(snapshot_titled("second"));
        publisher.publish(snapshot_titled("third"));
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "held back until the interval ends"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(*seen.lock().unwrap(), ["first", "third"]);
    }

    #[tokio::test]
    async fn an_unchanged_snapshot_is_not_sent_again() {
        let (publisher, seen) = recording_publisher();

        publisher.publish(snapshot_titled("same"));
        tokio::time::sleep(Duration::from_millis(150)).await;
        publisher.publish(snapshot_titled("same"));
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(*seen.lock().unwrap(), ["same"]);
    }

    #[test]
    fn test_locale_normalization() {
        assert_eq!(normalize_locale("en"), "en");
        assert_eq!(normalize_locale("en-US"), "en");
        assert_eq!(normalize_locale("en_GB"), "en");
        assert_eq!(normalize_locale("pt"), "pt-BR");
        assert_eq!(normalize_locale("pt-BR"), "pt-BR");
        assert_eq!(normalize_locale("pt_BR"), "pt-BR");
        assert_eq!(normalize_locale("pt-PT"), "pt-BR");
        assert_eq!(normalize_locale(""), "en");
        assert_eq!(normalize_locale("fr-FR"), "en");
    }

    #[test]
    fn the_effective_locale_uses_the_preference_or_the_os_language() {
        assert_eq!(effective_locale(Locale::System, "pt-BR"), "pt-BR");
        assert_eq!(effective_locale(Locale::System, "pt_BR"), "pt-BR");
        assert_eq!(effective_locale(Locale::System, "fr-FR"), "en");
        assert_eq!(effective_locale(Locale::System, ""), "en");
        assert_eq!(effective_locale(Locale::En, "pt-BR"), "en");
        assert_eq!(effective_locale(Locale::PtBr, "en-US"), "pt-BR");
    }

    #[test]
    fn tray_labels_follow_the_effective_locale() {
        assert_eq!(tray_labels("pt-BR"), ("Abrir o GameNode", "Sair"));
        assert_eq!(tray_labels("en"), ("Open GameNode", "Quit"));
    }

    #[test]
    fn the_close_preference_decides_between_the_tray_and_quitting() {
        assert_eq!(close_action(true), CloseAction::HideToTray);
        assert_eq!(close_action(false), CloseAction::Quit);
    }

    #[test]
    fn a_running_install_refuses_the_cache_clear_with_a_typed_error() {
        let error: PortCommandError = crate::engine::pc_ports::PortsError::InstallInProgress.into();
        assert!(matches!(error, PortCommandError::InstallInProgress));
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], "installInProgress");
    }

    #[test]
    fn game_node_page_values_and_errors_use_stable_codes() {
        let game = serde_json::to_value(crate::engine::GameNodePage::Game { id: 42 }).unwrap();
        assert_eq!(game, serde_json::json!({ "game": { "id": 42 } }));
        let library = serde_json::to_value(crate::engine::GameNodePage::Library).unwrap();
        assert_eq!(library, serde_json::json!("library"));

        let no_session = serde_json::to_value(crate::engine::GameNodePageError::NoSession).unwrap();
        assert_eq!(no_session["code"], "noSession");
        let browser = serde_json::to_value(crate::engine::GameNodePageError::Browser).unwrap();
        assert_eq!(browser["code"], "browser");
    }

    #[test]
    fn game_node_link_values_use_the_fixed_enum_spelling() {
        assert_eq!(
            serde_json::to_value(crate::engine::GameNodeLink::Website).unwrap(),
            serde_json::json!("website")
        );
        assert_eq!(
            serde_json::to_value(crate::engine::GameNodeLink::Terms).unwrap(),
            serde_json::json!("terms")
        );
        assert_eq!(
            serde_json::to_value(crate::engine::GameNodeLink::Privacy).unwrap(),
            serde_json::json!("privacy")
        );
        assert_eq!(
            serde_json::to_value(crate::engine::GameNodeLink::ReportIssue).unwrap(),
            serde_json::json!("reportIssue")
        );
        assert_eq!(
            serde_json::to_value(crate::engine::GameNodeLink::Profile).unwrap(),
            serde_json::json!("profile")
        );
    }

    #[test]
    fn the_build_facts_are_baked_in_and_well_formed() {
        let build_date = env!("GAMENODE_BUILD_DATE");
        assert_eq!(build_date.len(), 10, "{build_date}");
        assert!(build_date.as_bytes()[4] == b'-' && build_date.as_bytes()[7] == b'-');
        assert!(build_date[..4].parse::<u32>().is_ok());
        assert!(!gamenode_platform::system::arch().is_empty());
    }

    #[test]
    fn system_information_redacts_profile_paths_and_never_names_the_account() {
        let plugins = vec![crate::plugin_host::PluginInfo {
            provider_id: "steam".to_owned(),
            plugin_id: "org.gamenode.plugin.steam-discovery".to_owned(),
            name: "Steam".to_owned(),
            version: "1.2.3".to_owned(),
            source: "official".to_owned(),
            status: "installed".to_owned(),
            capabilities: vec!["library.discovery".to_owned()],
            session: "active".to_owned(),
            native_access: vec!["files".to_owned()],
        }];
        let stores = vec![SystemStore {
            name: "Steam".to_owned(),
            installed_count: 18,
            install_root: Some("C:\\Users\\cassi\\Steam".to_owned()),
        }];
        let text = format_system_info(
            &SystemFacts {
                version: "0.1.0",
                build_date: "2026-11-16",
                os_version: "Windows 11 Pro (build 26100)",
                arch: "x86_64",
                locale: "pt-BR",
                home: Some("C:\\Users\\cassi"),
            },
            &plugins,
            &stores,
        );
        assert!(text.contains("GameNode Desktop 0.1.0"));
        assert!(text.contains("Build 2026-11-16"));
        assert!(text.contains("Windows 11 Pro (build 26100) x86_64"));
        assert!(text.contains("Locale: pt-BR"));
        assert!(text.contains("Steam 1.2.3 (official, installed)"));
        let placeholder = if cfg!(windows) { "%USERPROFILE%" } else { "~" };
        assert!(text.contains(&format!("Steam: 18 installed at {placeholder}\\Steam")));
        assert!(!text.contains("cassi"));
        assert!(!text.contains("account"));
        assert!(!text.contains("token"));
        assert!(!text.contains("credential"));
    }

    #[test]
    fn achievement_page_errors_use_stable_codes() {
        let unsupported =
            serde_json::to_value(crate::engine::AchievementPageError::Unsupported).unwrap();
        assert_eq!(unsupported["code"], "unsupported");
        let browser = serde_json::to_value(crate::engine::AchievementPageError::Browser).unwrap();
        assert_eq!(browser["code"], "browser");
    }

    /// A provider host with the given loaded catalog providers and no account work.
    struct FakeRuntime {
        providers: Vec<String>,
    }

    #[async_trait::async_trait]
    impl crate::plugin_host::runtime::ProviderRuntime for FakeRuntime {
        fn accounts(&self) -> Vec<String> {
            self.providers.clone()
        }
        fn has_provider(&self, provider: &str) -> bool {
            self.providers.iter().any(|id| id == provider)
        }
        fn status(&self, _: &str) -> Option<gamenode_plugin_protocol::models::AccountStatus> {
            None
        }
        fn available(&self, _: &str) -> bool {
            true
        }
        fn set_observer(&self, _: crate::plugin_host::runtime::Observer) {}
        async fn shutdown(&self) {}
        async fn execute(
            &self,
            provider: &str,
            _: gamenode_plugin_protocol::proto::ProviderOperation,
        ) -> Result<crate::plugin_host::provider::Completion, tonic::Status> {
            Err(tonic::Status::not_found(provider))
        }
    }

    fn fake_runtime(providers: &[&str]) -> Arc<dyn crate::plugin_host::runtime::ProviderRuntime> {
        Arc::new(FakeRuntime {
            providers: providers.iter().map(|id| (*id).to_owned()).collect(),
        })
    }

    #[tokio::test]
    async fn provider_commands_reject_an_unknown_provider_without_touching_the_host() {
        let runtime = fake_runtime(&["steam", "epic"]);
        assert!(catalog_provider_loaded(Some(&runtime), "steam"));
        assert!(catalog_provider_loaded(Some(&runtime), "epic"));
        assert!(!catalog_provider_loaded(Some(&runtime), "gog"));
        assert!(!catalog_provider_loaded(None, "steam"));

        let called = std::sync::Mutex::new(Vec::new());
        let error = run_provider_command(false, "gog", |id| {
            called.lock().unwrap().push(id);
            async {}
        })
        .await
        .unwrap_err();
        assert_eq!(error, "unknown provider: gog");
        assert!(called.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn provider_commands_route_the_named_provider_to_the_action() {
        let called = std::sync::Mutex::new(Vec::new());
        for provider in ["steam", "epic"] {
            run_provider_command(true, provider, |id| {
                called.lock().unwrap().push(id);
                async {}
            })
            .await
            .unwrap();
        }
        assert_eq!(*called.lock().unwrap(), ["steam", "epic"]);
    }
}
