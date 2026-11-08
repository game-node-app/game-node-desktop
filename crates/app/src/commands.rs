use std::sync::Arc;

use crate::engine::Engine;
use gamenode_core::{Snapshot, WindowGeometry};

/// App metadata the React shell needs at startup.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub locale: String,
    pub version: String,
    pub background_supported: bool,
    /// Platform reported to the UI; PC ports is Windows-only in this release.
    pub platform: Option<gamenode_core::ports::PortPlatform>,
    /// Directory of the rotating log files; shown in Settings > About.
    pub log_directory: Option<String>,
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
    location: tauri::State<'_, crate::logging::LogLocation>,
) -> Result<AppInfo, String> {
    Ok(AppInfo {
        locale: normalize_locale(&system_locale()).to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
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

#[tauri::command]
pub async fn set_playtime_source(
    engine: tauri::State<'_, Engine>,
    source: gamenode_core::playtime::PlaytimeSource,
) -> Result<(), String> {
    engine.set_playtime_source(source);
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
    engine.update_settings(move |settings| settings.window = Some(geometry));
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
    let status = engine.provider_availability("steam");
    let (_, connected) = engine.provider_health("steam");
    tokio::task::spawn_blocking(move || {
        let mut plugins = crate::plugin_host::inventory(status);
        for plugin in &mut plugins {
            if plugin.provider_id == "steam" {
                plugin.session = if connected { "active" } else { "stopped" }.into();
            }
        }
        plugins
    })
    .await
    .map_err(|_| "plugin inventory worker failed".to_owned())
}

#[tauri::command]
pub async fn provider_sign_in(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    engine.provider_sign_in(&provider_id).await;
    Ok(())
}

#[tauri::command]
pub async fn provider_focus_sign_in(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    engine.provider_focus_sign_in(&provider_id).await;
    Ok(())
}

#[tauri::command]
pub async fn provider_cancel_sign_in(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    engine.provider_cancel_sign_in(&provider_id).await;
    Ok(())
}

#[tauri::command]
pub async fn provider_sign_out(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    engine.provider_sign_out(&provider_id).await;
    Ok(())
}

#[tauri::command]
pub async fn provider_check(
    engine: tauri::State<'_, Engine>,
    provider_id: String,
) -> Result<(), String> {
    engine.provider_check(&provider_id).await;
    Ok(())
}

/// Opens Steam's isolated sign-in window and waits for the attempt to end.
#[tauri::command]
pub async fn steam_sign_in(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.provider_sign_in("steam").await;
    Ok(())
}

/// Brings the open Steam sign-in window to the front.
#[tauri::command]
pub async fn steam_focus_sign_in(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.provider_focus_sign_in("steam").await;
    Ok(())
}

/// Cancels the open Steam sign-in window without saving a credential.
#[tauri::command]
pub async fn steam_cancel_sign_in(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.provider_cancel_sign_in("steam").await;
    Ok(())
}

/// Signs out of Steam, keeping installed games, matches and covers.
#[tauri::command]
pub async fn steam_sign_out(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.provider_sign_out("steam").await;
    Ok(())
}

/// Validates the Steam session and, with an open library, refreshes the catalog.
#[tauri::command]
pub async fn steam_check(engine: tauri::State<'_, Engine>) -> Result<(), String> {
    engine.provider_check("steam").await;
    Ok(())
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
) -> Result<(), String> {
    engine.set_local_backup_settings(automatic, destination);
    Ok(())
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
) -> Result<(), String> {
    engine.review_selected_local_candidates(add);
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
    /// The install cannot start (no verified release, no disk space, confirmation needed).
    Unavailable(String),
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
    fn a_running_install_refuses_the_cache_clear_with_a_typed_error() {
        let error: PortCommandError = crate::engine::pc_ports::PortsError::InstallInProgress.into();
        assert!(matches!(error, PortCommandError::InstallInProgress));
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], "installInProgress");
    }
}
