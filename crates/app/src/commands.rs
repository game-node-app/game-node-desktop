use std::sync::Arc;

use crate::engine::Engine;
use gamenode_core::{Snapshot, WindowGeometry};

/// Bootstrap payload sent to the React frontend on initial load.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    pub snapshot: Snapshot,
    pub locale: String,
    pub version: String,
    pub license_notices: String,
    pub background_supported: bool,
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

/// Helper that coalesces rapid back-to-back engine snapshots before emitting them to the WebView.
#[derive(Clone)]
pub struct SnapshotPublisher {
    pending: Arc<std::sync::Mutex<(Option<Snapshot>, bool)>>,
    emitter: Arc<dyn Fn(Snapshot) + Send + Sync>,
}

impl SnapshotPublisher {
    pub fn new(emitter: impl Fn(Snapshot) + Send + Sync + 'static) -> Self {
        Self {
            pending: Arc::new(std::sync::Mutex::new((None, false))),
            emitter: Arc::new(emitter),
        }
    }

    pub fn from_app_handle(app: tauri::AppHandle) -> Self {
        use tauri::Emitter;
        Self::new(move |snapshot| {
            let _ = app.emit("engine:snapshot", &snapshot);
        })
    }

    pub fn publish(&self, snapshot: Snapshot) {
        let should_spawn = {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.0 = Some(snapshot);
            if pending.1 {
                false
            } else {
                pending.1 = true;
                true
            }
        };

        if !should_spawn {
            return;
        }

        let pending = self.pending.clone();
        let emitter = self.emitter.clone();

        tauri::async_runtime::spawn(async move {
            tokio::task::yield_now().await;
            loop {
                let snapshot = {
                    let mut lock = pending
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    lock.0.take()
                };

                if let Some(s) = snapshot {
                    emitter(s);
                }

                let mut lock = pending
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if lock.0.is_none() {
                    lock.1 = false;
                    break;
                }
            }
        });
    }
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
pub async fn get_bootstrap_state(
    engine: tauri::State<'_, Engine>,
) -> Result<BootstrapState, String> {
    Ok(BootstrapState {
        snapshot: engine.snapshot(),
        locale: normalize_locale(&system_locale()).to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        license_notices: include_str!("../../../licenses/THIRD-PARTY-NOTICES.txt").to_string(),
        background_supported: cfg!(windows),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn test_snapshot_coalescing() {
        let emit_count = Arc::new(AtomicUsize::new(0));
        let count_clone = emit_count.clone();

        let publisher = SnapshotPublisher::new(move |_snapshot| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        let snapshot = Snapshot::new(
            &gamenode_core::auth::AuthMachine::new(Box::new(|| "nonce".into())),
            gamenode_core::StorageMethod::PrivateFile,
            Default::default(),
            None,
            None,
        );

        // Emit 50 snapshots in rapid succession without yielding
        for _ in 0..50 {
            publisher.publish(snapshot.clone());
        }

        // Wait for tokio tasks to settle
        tokio::time::sleep(Duration::from_millis(50)).await;

        let total = emit_count.load(Ordering::SeqCst);
        assert!(
            (1..10).contains(&total),
            "expected coalescing, got {total} emissions"
        );
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
}
