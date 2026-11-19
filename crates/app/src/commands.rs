use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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
    engine.dispatch_on_worker(input).await;
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

/// Approves the currently displayed detected target after worker-side host validation.
#[tauri::command]
pub async fn review_detected_target(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
    expected_revision: u64,
) -> Result<(), String> {
    engine
        .review_detected_target(entry_key, expected_revision)
        .await
}

/// Lists the active profile's custom recipes for the selected matched game.
#[tauri::command]
pub async fn launch_recipe_choices(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
) -> Result<crate::engine::LaunchRecipeChoices, String> {
    engine.launch_recipe_choices(entry_key).await
}

/// Makes one authenticated plugin recipe the explicit launch choice for this library entry.
#[tauri::command]
pub async fn select_launch_recipe(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
    plugin_id: String,
    publisher_fingerprint: String,
    recipe_id: String,
    recipe_revision: u64,
    expected_target_revision: u64,
) -> Result<(), String> {
    engine
        .select_launch_recipe(
            entry_key,
            plugin_id,
            publisher_fingerprint,
            recipe_id,
            recipe_revision,
            expected_target_revision,
        )
        .await
}

/// Explicitly returns this library entry to its detected default launch target.
#[tauri::command]
pub async fn use_detected_default(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
    expected_target_revision: u64,
) -> Result<(), String> {
    engine
        .use_detected_default(entry_key, expected_target_revision)
        .await
}

/// Opens a native picker to repair a missing detected executable. Selecting a file is explicit
/// approval for the replacement target; validation and persistence run on a worker.
#[tauri::command]
pub async fn repair_detected_target(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
    expected_revision: u64,
) -> Result<(), String> {
    engine
        .repair_detected_target(entry_key, expected_revision)
        .await
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
pub async fn start_extension_action(
    engine: tauri::State<'_, Engine>,
    entry_key: String,
    plugin_id: String,
    operation: String,
) -> Result<(), String> {
    let operation = match operation.as_str() {
        "play" => crate::plugin_host::intents::Operation::Launch,
        "install" => crate::plugin_host::intents::Operation::Install,
        _ => return Err("unsupported extension action".to_owned()),
    };
    engine
        .start_extension_action(&entry_key, &plugin_id, operation)
        .await
        .map_err(|status| status.message().to_owned())
}

#[tauri::command]
pub async fn available_plugin_extensions(
    engine: tauri::State<'_, Engine>,
    operation: String,
) -> Result<Vec<crate::plugin_host::PluginInfo>, String> {
    let operation = match operation.as_str() {
        "play" => crate::plugin_host::intents::Operation::Launch,
        "install" => crate::plugin_host::intents::Operation::Install,
        _ => return Err("unsupported extension action".to_owned()),
    };
    engine
        .available_extensions(operation)
        .await
        .map_err(|status| status.message().to_owned())
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
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
) -> Result<Vec<crate::plugin_host::PluginInfo>, String> {
    let engine = engine.inner().clone();
    let (mut inventory, runtime_inventory) = tokio::task::spawn_blocking(move || {
        let bundled = crate::plugin_host::inventory(|id| {
            let (_, connected) = engine.provider_health(id);
            (engine.provider_availability(id), connected)
        });
        (bundled, engine.managed_plugin_inventory())
    })
    .await
    .map_err(|_| "plugin inventory worker failed".to_owned())?;
    let mut managed = updates.managed_inventory().await?;
    for plugin in &mut managed {
        if let Some(runtime) = runtime_inventory
            .iter()
            .find(|runtime| runtime.plugin_id == plugin.plugin_id)
        {
            plugin.status.clone_from(&runtime.status);
            plugin.session.clone_from(&runtime.session);
            if runtime.enabled.is_some() {
                plugin.enabled = runtime.enabled;
            }
            if runtime.runtime_available.is_some() {
                plugin.runtime_available = runtime.runtime_available;
            }
        }
    }
    inventory.extend(managed);
    inventory.sort_by(|left, right| left.plugin_id.cmp(&right.plugin_id));
    Ok(inventory)
}

#[tauri::command]
pub async fn plugin_import_review(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    archive_path: String,
) -> Result<crate::plugin_host::updates::PluginImportReview, String> {
    updates.import_review(archive_path.into()).await
}

#[tauri::command]
pub async fn plugin_import_confirm_publisher(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    token: String,
    confirmed_fingerprint: String,
) -> Result<crate::plugin_host::updates::PluginImportReview, String> {
    updates
        .import_confirm_publisher(&token, &confirmed_fingerprint)
        .await
}

#[tauri::command]
pub async fn plugin_import_install(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    token: String,
    native_code_consent: bool,
) -> Result<String, String> {
    updates.import_install(&token, native_code_consent).await
}

#[tauri::command]
pub async fn plugin_import_license_text(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    token: String,
    file: String,
) -> Result<String, String> {
    updates.import_license_text(&token, &file).await
}

#[tauri::command]
pub async fn plugin_permission_snapshot(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    plugin_id: String,
) -> Result<crate::plugin_host::runtime::PluginPermissionSnapshot, String> {
    updates
        .permission_snapshot(plugin_id)
        .await
        .map_err(|error| error.message().to_owned())
}

#[tauri::command]
pub async fn plugin_permissions_replace(
    lifecycle: tauri::State<'_, Arc<crate::plugin_host::runtime::ThirdPartyPluginLifecycle>>,
    plugin_id: String,
    expected_profile_id: String,
    expected_revision: u64,
    decisions: Vec<crate::plugin_host::grants::PermissionDecision>,
) -> Result<crate::plugin_host::runtime::PluginPermissionSnapshot, String> {
    lifecycle
        .replace_permissions(plugin_id, expected_profile_id, expected_revision, decisions)
        .await
        .map_err(|error| error.message().to_owned())
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedDataCatalogRecord {
    pub source_plugin_id: String,
    pub source_publisher_fingerprint: String,
    pub dataset_id: String,
    pub class: String,
    pub category: Option<String>,
    pub record_id: String,
    pub fields: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedDataSourcePolicy {
    pub source_plugin_id: String,
    pub source_publisher_fingerprint: String,
    pub dataset_id: String,
    pub class: String,
    pub fields: Vec<String>,
    pub history: bool,
    pub live: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSharedDataSnapshot {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub profile_id: String,
    pub policy_revision: u64,
    pub common_categories: Vec<String>,
    pub selected_record_ids: Vec<String>,
    pub catalog: Vec<SharedDataCatalogRecord>,
    pub source_policies: Vec<SharedDataSourcePolicy>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharedDataSourcePolicyDecision {
    pub source_plugin_id: String,
    pub source_publisher_fingerprint: String,
    pub dataset_id: String,
    pub class: String,
    pub fields: BTreeSet<String>,
    pub history: bool,
    pub live: bool,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginSharedDataPolicyUpdate {
    pub profile_id: String,
    pub expected_publisher_fingerprint: String,
    pub expected_revision: u64,
    pub common_categories: Vec<String>,
    pub selected_record_ids: Vec<String>,
    pub source_policies: Vec<SharedDataSourcePolicyDecision>,
}

fn common_data_category_name(
    category: gamenode_plugin_protocol::proto::CommonDataCategory,
) -> String {
    use gamenode_plugin_protocol::proto::CommonDataCategory as C;
    match category {
        C::GamePaths => "gamePaths",
        C::GamePerformanceHistory => "gamePerformanceHistory",
        C::RuntimeCatalog => "runtimeCatalog",
        C::ContentAvailability => "contentAvailability",
        C::LibraryRoots => "libraryRoots",
        C::SaveProfiles => "saveProfiles",
        C::Unspecified => "unspecified",
    }
    .to_owned()
}

fn common_data_category(
    value: &str,
) -> Result<gamenode_plugin_protocol::proto::CommonDataCategory, String> {
    use gamenode_plugin_protocol::proto::CommonDataCategory as C;
    match value {
        "gamePaths" => Ok(C::GamePaths),
        "gamePerformanceHistory" => Ok(C::GamePerformanceHistory),
        "runtimeCatalog" => Ok(C::RuntimeCatalog),
        "contentAvailability" => Ok(C::ContentAvailability),
        "libraryRoots" => Ok(C::LibraryRoots),
        "saveProfiles" => Ok(C::SaveProfiles),
        _ => Err("unknown shared-data category".into()),
    }
}

fn sharing_class_name(class: crate::plugin_host::sharing_policy::SharingClass) -> String {
    match class {
        crate::plugin_host::sharing_policy::SharingClass::Common => "common",
        crate::plugin_host::sharing_policy::SharingClass::Sensitive => "sensitive",
        crate::plugin_host::sharing_policy::SharingClass::Custom => "custom",
    }
    .to_owned()
}

fn sharing_class(value: &str) -> Result<crate::plugin_host::sharing_policy::SharingClass, String> {
    use crate::plugin_host::sharing_policy::SharingClass as C;
    match value {
        "sensitive" => Ok(C::Sensitive),
        "custom" => Ok(C::Custom),
        _ => Err("only sensitive and custom datasets use source-recipient review".into()),
    }
}

fn unix_timestamp() -> Result<i64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "system clock is before Unix epoch".to_owned())?
        .as_secs()
        .try_into()
        .map_err(|_| "system time is outside supported range".to_owned())
}

#[tauri::command]
pub async fn plugin_shared_data_snapshot(
    lifecycle: tauri::State<'_, Arc<crate::plugin_host::runtime::ThirdPartyPluginLifecycle>>,
    policies: tauri::State<'_, Arc<crate::plugin_host::sharing_policy::SharedSharingPolicies>>,
    broker: tauri::State<'_, Arc<crate::plugin_host::shared_data::DataBroker>>,
    plugin_id: String,
    expected_profile_id: String,
) -> Result<PluginSharedDataSnapshot, String> {
    let identity = lifecycle
        .sharing_identity(plugin_id, &expected_profile_id)
        .await
        .map_err(|status| status.message().to_owned())?;
    let policy_snapshot = read_shared_data_policy(policies.inner().clone(), &identity).await?;
    let catalog = shared_data_catalog(broker.inner().clone(), &identity.profile_id).await?;
    Ok(plugin_shared_data_snapshot_for(
        identity,
        policy_snapshot,
        catalog,
    ))
}

async fn read_shared_data_policy(
    policies: Arc<crate::plugin_host::sharing_policy::SharedSharingPolicies>,
    identity: &crate::plugin_host::grants::PluginIdentity,
) -> Result<crate::plugin_host::sharing_policy::RecipientSharingSnapshot, String> {
    let identity = identity.clone();
    tokio::task::spawn_blocking(move || policies.recipient_snapshot(&identity))
        .await
        .map_err(|_| "shared-data permission worker failed".to_owned())?
        .map_err(|error| error.to_string())
}

async fn shared_data_catalog(
    broker: Arc<crate::plugin_host::shared_data::DataBroker>,
    profile_id: &str,
) -> Result<Vec<crate::plugin_host::shared_data::SharedDataCatalogRecord>, String> {
    let now = unix_timestamp()?;
    let profile_id = profile_id.to_owned();
    tokio::task::spawn_blocking(move || {
        broker
            .sharing_catalog(now)
            .into_iter()
            .filter(|record| record.owner.profile_id == profile_id)
            .collect()
    })
    .await
    .map_err(|_| "shared-data catalog worker failed".to_owned())
}

fn plugin_shared_data_snapshot_for(
    identity: crate::plugin_host::grants::PluginIdentity,
    policy_snapshot: crate::plugin_host::sharing_policy::RecipientSharingSnapshot,
    catalog: Vec<crate::plugin_host::shared_data::SharedDataCatalogRecord>,
) -> PluginSharedDataSnapshot {
    let catalog = catalog
        .into_iter()
        .map(|record| SharedDataCatalogRecord {
            source_plugin_id: record.owner.plugin_id,
            source_publisher_fingerprint: record.owner.publisher_fingerprint,
            dataset_id: record.dataset_id,
            class: sharing_class_name(record.class),
            category: record.category.map(common_data_category_name),
            record_id: record.record_id,
            fields: record.fields.into_iter().collect(),
        })
        .collect();
    let source_policies = policy_snapshot
        .source_recipient_policies
        .into_iter()
        .map(|policy| SharedDataSourcePolicy {
            source_plugin_id: policy.source().plugin_id.clone(),
            source_publisher_fingerprint: policy.source().publisher_fingerprint.clone(),
            dataset_id: policy.dataset_id().to_owned(),
            class: sharing_class_name(policy.class()),
            fields: policy.fields().iter().cloned().collect(),
            history: policy.allows_history(),
            live: policy.allows_live(),
        })
        .collect();
    PluginSharedDataSnapshot {
        plugin_id: identity.plugin_id,
        publisher_fingerprint: identity.publisher_fingerprint,
        profile_id: identity.profile_id,
        policy_revision: policy_snapshot.revision,
        common_categories: policy_snapshot
            .categories
            .into_iter()
            .map(common_data_category_name)
            .collect(),
        selected_record_ids: policy_snapshot.selected_record_ids.into_iter().collect(),
        catalog,
        source_policies,
    }
}

#[tauri::command]
pub async fn plugin_shared_data_replace(
    lifecycle: tauri::State<'_, Arc<crate::plugin_host::runtime::ThirdPartyPluginLifecycle>>,
    policies: tauri::State<'_, Arc<crate::plugin_host::sharing_policy::SharedSharingPolicies>>,
    broker: tauri::State<'_, Arc<crate::plugin_host::shared_data::DataBroker>>,
    plugin_id: String,
    update: PluginSharedDataPolicyUpdate,
) -> Result<PluginSharedDataSnapshot, String> {
    let observation = lifecycle
        .observe_sharing_identity(plugin_id, &update.profile_id)
        .await
        .map_err(|status| status.message().to_owned())?;
    let identity = &observation.identity;
    if identity.publisher_fingerprint != update.expected_publisher_fingerprint {
        return Err("plugin publisher changed; reload and review shared-data permissions".into());
    }
    let expected_revision = update.expected_revision;
    let existing = read_shared_data_policy(policies.inner().clone(), identity).await?;
    if existing.revision != expected_revision {
        return Err("shared-data permissions changed; reload and try again".into());
    }
    let catalog = shared_data_catalog(broker.inner().clone(), &identity.profile_id).await?;
    let categories = update
        .common_categories
        .iter()
        .map(|category| common_data_category(category))
        .collect::<Result<Vec<_>, _>>()?;
    let grant = crate::plugin_host::sharing_policy::ActivationReadGrant::new(
        categories,
        update.selected_record_ids.clone(),
    )
    .map_err(|error| error.to_string())?;
    let available_common_records = catalog
        .iter()
        .filter(|record| record.class == crate::plugin_host::sharing_policy::SharingClass::Common)
        .map(|record| record.record_id.as_str())
        .collect::<BTreeSet<_>>();
    if update.selected_record_ids.iter().any(|record_id| {
        !available_common_records.contains(record_id.as_str())
            && !existing.selected_record_ids.contains(record_id)
    }) {
        return Err("selected record is not present in the host sharing catalog".into());
    }
    let mut source_policies = Vec::new();
    let mut required_sources = Vec::new();
    for decision in update.source_policies {
        let class = sharing_class(&decision.class)?;
        let source = crate::plugin_host::grants::PluginIdentity {
            plugin_id: decision.source_plugin_id,
            publisher_fingerprint: decision.source_publisher_fingerprint,
            profile_id: identity.profile_id.clone(),
        };
        let existing_policy = existing.source_recipient_policies.iter().find(|policy| {
            policy.source() == &source
                && policy.dataset_id() == decision.dataset_id
                && policy.class() == class
        });
        if decision.fields.is_empty() {
            if decision.history || decision.live {
                return Err("history or live delivery requires at least one approved field".into());
            }
            continue;
        }
        let available_fields = catalog
            .iter()
            .filter(|record| {
                record.owner == source
                    && record.dataset_id == decision.dataset_id
                    && record.class == class
            })
            .flat_map(|record| record.fields.iter().cloned())
            .collect::<BTreeSet<_>>();
        let within_previous_grant = existing_policy.is_some_and(|previous| {
            decision.fields.is_subset(previous.fields())
                && (!decision.history || previous.allows_history())
                && (!decision.live || previous.allows_live())
        });
        let source_is_current = !available_fields.is_empty()
            && lifecycle
                .sharing_identity(source.plugin_id.clone(), &identity.profile_id)
                .await
                .is_ok_and(|current| current == source);
        if source_is_current {
            if !decision.fields.is_subset(&available_fields) {
                return Err("sharing policy names fields not published by this dataset".into());
            }
            if !within_previous_grant && !required_sources.contains(&source) {
                required_sources.push(source.clone());
            }
        } else if !within_previous_grant {
            return Err(if available_fields.is_empty() {
                "new sharing grants require a current verified source"
            } else {
                "source identity is no longer verified; reload and review shared-data permissions"
            }
            .into());
        }
        source_policies.push(
            crate::plugin_host::sharing_policy::SourceRecipientPolicy::new(
                source,
                identity.clone(),
                class,
                decision.dataset_id,
                decision.fields,
                decision.history,
                decision.live,
            )
            .map_err(|error| error.to_string())?,
        );
    }
    let identity = identity.clone();
    let saved = lifecycle
        .replace_sharing_policy(
            observation,
            policies.inner().clone(),
            expected_revision,
            grant,
            source_policies,
            required_sources,
        )
        .await
        .map_err(|status| status.message().to_owned())?;
    Ok(plugin_shared_data_snapshot_for(identity, saved, catalog))
}

#[tauri::command]
pub async fn plugin_enable(
    lifecycle: tauri::State<'_, Arc<crate::plugin_host::runtime::ThirdPartyPluginLifecycle>>,
    plugin_id: String,
) -> Result<(), String> {
    lifecycle
        .enable(plugin_id)
        .await
        .map_err(|error| error.message().to_owned())
}

#[tauri::command]
pub async fn plugin_disable(
    lifecycle: tauri::State<'_, Arc<crate::plugin_host::runtime::ThirdPartyPluginLifecycle>>,
    plugin_id: String,
) -> Result<(), String> {
    lifecycle
        .disable(plugin_id)
        .await
        .map_err(|error| error.message().to_owned())
}

#[tauri::command]
pub async fn plugin_remove(
    lifecycle: tauri::State<'_, Arc<crate::plugin_host::runtime::ThirdPartyPluginLifecycle>>,
    plugin_id: String,
) -> Result<(), String> {
    lifecycle
        .remove(plugin_id)
        .await
        .map_err(|error| error.message().to_owned())
}

#[tauri::command]
pub async fn plugin_configuration_state(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    plugin_id: String,
) -> Result<crate::plugin_host::runtime::PluginConfigurationState, String> {
    updates
        .configuration_state(plugin_id)
        .await
        .map_err(|error| error.message().to_owned())
}

static NEXT_SETTINGS_FORM: AtomicU64 = AtomicU64::new(1);

#[tauri::command]
pub async fn plugin_configuration_modes(
    providers: tauri::State<'_, crate::plugin_host::provider::ProviderApi>,
    provider_id: String,
) -> Result<crate::plugin_host::provider::ConfigurationModes, String> {
    providers
        .configuration_modes(&provider_id)
        .map_err(|status| status.message().to_owned())
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfigurationStatus {
    pub status: &'static str,
    pub window_id: Option<String>,
    pub message: Option<String>,
}

#[tauri::command]
pub async fn plugin_configuration_open(
    providers: tauri::State<'_, crate::plugin_host::provider::ProviderApi>,
    provider_id: String,
    focus: bool,
) -> Result<PluginConfigurationStatus, String> {
    use gamenode_plugin_protocol::proto::ProviderOperation;
    let operation = if focus {
        ProviderOperation::ConfigurationFocus
    } else {
        ProviderOperation::ConfigurationOpen
    };
    Ok(match providers.configure(&provider_id, operation).await {
        Ok(completion) => match completion.result.configuration {
            Some(result) if result.accepted => PluginConfigurationStatus {
                status: if focus { "focused" } else { "open" },
                window_id: Some(result.window_id),
                message: None,
            },
            Some(result) => PluginConfigurationStatus {
                status: "failed",
                window_id: None,
                message: Some(result.failure),
            },
            None => PluginConfigurationStatus {
                status: "failed",
                window_id: None,
                message: Some("plugin returned no configuration result".into()),
            },
        },
        Err(status) => {
            let code = status.code();
            PluginConfigurationStatus {
                status: match code {
                    tonic::Code::DeadlineExceeded => "timedOut",
                    tonic::Code::PermissionDenied => "denied",
                    tonic::Code::Unavailable | tonic::Code::FailedPrecondition => "unavailable",
                    _ => "failed",
                },
                window_id: None,
                message: Some(status.message().to_owned()),
            }
        }
    })
}

#[tauri::command]
pub async fn plugin_settings_describe(
    providers: tauri::State<'_, crate::plugin_host::provider::ProviderApi>,
    provider_id: String,
) -> Result<gamenode_plugin_protocol::settings::Reply, String> {
    use gamenode_plugin_protocol::settings::{Action, Request};
    let handle = format!(
        "settings-form-{}",
        NEXT_SETTINGS_FORM.fetch_add(1, Ordering::Relaxed)
    );
    let context = providers
        .settings_scope(&provider_id, handle)
        .map_err(|status| status.message().to_owned())?;
    complete_plugin_settings(
        &providers,
        Request {
            context,
            action: Action::DescribeRead,
        },
    )
    .await
}

#[tauri::command]
pub async fn plugin_settings_submit(
    providers: tauri::State<'_, crate::plugin_host::provider::ProviderApi>,
    context: gamenode_plugin_protocol::settings::Context,
    schema_revision: String,
    configuration_revision: String,
    values: gamenode_plugin_protocol::settings::Values,
) -> Result<gamenode_plugin_protocol::settings::Reply, String> {
    use gamenode_plugin_protocol::settings::{Action, Request};
    complete_plugin_settings(
        &providers,
        Request {
            context,
            action: Action::Submit {
                schema_revision,
                configuration_revision,
                values,
            },
        },
    )
    .await
}

#[tauri::command]
pub async fn plugin_settings_cancel(
    providers: tauri::State<'_, crate::plugin_host::provider::ProviderApi>,
    context: gamenode_plugin_protocol::settings::Context,
) -> Result<gamenode_plugin_protocol::settings::Reply, String> {
    use gamenode_plugin_protocol::settings::{Action, Request};
    complete_plugin_settings(
        &providers,
        Request {
            context,
            action: Action::Cancel,
        },
    )
    .await
}

async fn complete_plugin_settings(
    providers: &crate::plugin_host::provider::ProviderApi,
    request: gamenode_plugin_protocol::settings::Request,
) -> Result<gamenode_plugin_protocol::settings::Reply, String> {
    request
        .validate()
        .map_err(|_| "invalid plugin settings request".to_owned())?;
    let completion = providers
        .execute_settings(request.clone())
        .await
        .map_err(|status| status.message().to_owned())?;
    let reply: gamenode_plugin_protocol::settings::Reply = completion
        .result
        .settings
        .ok_or_else(|| "plugin returned no settings response".to_owned())?
        .try_into()
        .map_err(|_| "plugin returned an invalid settings response".to_owned())?;
    reply
        .validate_for(&request)
        .map_err(|_| "plugin settings response did not match the request".to_owned())?;
    Ok(reply)
}

#[tauri::command]
pub async fn plugin_playtime_inventory(
    engine: tauri::State<'_, Engine>,
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
) -> Result<Vec<crate::plugin_host::updates::PluginPlaytimeInfo>, String> {
    let engine = engine.inner().clone();
    let mut plugins = updates.playtime_inventory().await?;
    for plugin in &mut plugins {
        plugin.accounting_enabled = engine
            .plugin_playtime_accounting_enabled(plugin.plugin_id.clone())
            .await?;
    }
    Ok(plugins)
}

#[tauri::command]
pub async fn plugin_playtime_set_accounting(
    engine: tauri::State<'_, Engine>,
    plugin_id: String,
    enabled: bool,
) -> Result<(), String> {
    engine
        .inner()
        .set_plugin_playtime_accounting(plugin_id, enabled)
        .await
}

#[tauri::command]
pub async fn plugin_update_preferences(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
) -> Result<crate::plugin_host::updates::PluginUpdatePreferences, String> {
    Ok(updates.preferences())
}

#[tauri::command]
pub async fn plugin_update_available(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
) -> Result<Vec<crate::plugin_host::updates::AvailablePluginUpdate>, String> {
    Ok(updates.available_updates())
}

#[tauri::command]
pub async fn plugin_update_set_periodic_checks(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    enabled: bool,
) -> Result<crate::plugin_host::updates::PluginUpdatePreferences, String> {
    // Persisting this preference performs filesystem I/O. Keep it off Tauri's async command
    // executor so a slow disk cannot block IPC handling or the UI runtime.
    let updates = updates.inner().clone();
    tokio::task::spawn_blocking(move || updates.set_periodic_checks(enabled))
        .await
        .map_err(|_| "plugin update preferences worker failed".to_owned())?
}

#[tauri::command]
pub async fn plugin_update_check(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
) -> Result<Vec<crate::plugin_host::updates::AvailablePluginUpdate>, String> {
    updates.check_updates().await
}

#[tauri::command]
pub async fn plugin_update_review(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    plugin_id: String,
    version: String,
) -> Result<crate::plugin_host::updates::UpdateReview, String> {
    updates.review(&plugin_id, &version).await
}

#[tauri::command]
pub async fn plugin_rollback_available(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
) -> Result<Vec<crate::plugin_host::updates::AvailablePluginRollback>, String> {
    updates.available_rollbacks().await
}

#[tauri::command]
pub async fn plugin_rollback_review(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    plugin_id: String,
    version: String,
) -> Result<crate::plugin_host::updates::UpdateReview, String> {
    updates.review_rollback(&plugin_id, &version).await
}

#[tauri::command]
pub async fn plugin_update_retrust_publisher(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    plugin_id: String,
    version: String,
    fingerprint: String,
) -> Result<(), String> {
    updates
        .confirm_publisher_retrust(&plugin_id, &version, &fingerprint)
        .await
}

#[tauri::command]
pub async fn plugin_rollback_approve(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    plugin_id: String,
    version: String,
    confirmed: bool,
) -> Result<String, String> {
    updates
        .approve_rollback(&plugin_id, &version, confirmed)
        .await
}

#[tauri::command]
pub async fn plugin_update_approve(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    plugin_id: String,
    version: String,
) -> Result<String, String> {
    updates.approve(&plugin_id, &version).await
}

#[tauri::command]
pub async fn plugin_update_decline(
    updates: tauri::State<'_, crate::plugin_host::updates::PluginUpdateService>,
    plugin_id: String,
) -> Result<(), String> {
    updates.decline(&plugin_id).await;
    Ok(())
}

/// Delivers the user's answer to a plugin's confirmation. A late, repeated or unknown answer is
/// ignored: only a confirmation that is still waiting takes one.
#[tauri::command]
pub async fn plugin_confirmation_answer(
    bridge: tauri::State<'_, Arc<crate::plugin_host::confirmations::ConfirmationBridge>>,
    id: String,
    approved: bool,
    remember: bool,
) -> Result<(), String> {
    bridge.answer(&id, approved, remember);
    Ok(())
}

/// Answers a pending folder-selection prompt. Empty paths and declines cancel the approval.
#[tauri::command]
pub async fn plugin_folder_selection_answer(
    bridge: tauri::State<'_, Arc<crate::plugin_host::files::RootApprovalBridge>>,
    id: String,
    selected_root: Option<String>,
) -> Result<(), String> {
    if selected_root
        .as_ref()
        .is_some_and(|path| path.is_empty() || path.len() > 32 * 1024 || path.contains('\0'))
    {
        return Err("invalid folder selection".into());
    }
    bridge.answer_selection(&id, selected_root);
    Ok(())
}

/// Answers a pending folder-scope confirmation. A late or repeated response is ignored.
#[tauri::command]
pub async fn plugin_folder_approval_answer(
    bridge: tauri::State<'_, Arc<crate::plugin_host::files::RootApprovalBridge>>,
    id: String,
    approved: bool,
) -> Result<(), String> {
    bridge.answer_confirmation(&id, approved);
    Ok(())
}

/// Lists file and logical-library root approvals in one plugin's active account profile.
#[tauri::command]
pub async fn plugin_folder_approvals(
    approvals: tauri::State<'_, Arc<crate::plugin_host::file_approvals::PluginFolderApprovals>>,
    plugin_id: String,
    publisher_fingerprint: String,
) -> Result<Vec<crate::plugin_host::file_approvals::PluginFolderApproval>, String> {
    approvals.list(&plugin_id, &publisher_fingerprint).await
}

/// Revokes a folder approval or a standing logical-library root approval.
#[tauri::command]
pub async fn plugin_folder_revoke(
    approvals: tauri::State<'_, Arc<crate::plugin_host::file_approvals::PluginFolderApprovals>>,
    plugin_id: String,
    publisher_fingerprint: String,
    id: String,
) -> Result<bool, String> {
    approvals
        .revoke(&plugin_id, &publisher_fingerprint, &id)
        .await
}

/// Opens the folder chooser and replaces an existing file approval with the newly confirmed root.
#[tauri::command]
pub async fn plugin_folder_change(
    approvals: tauri::State<'_, Arc<crate::plugin_host::file_approvals::PluginFolderApprovals>>,
    plugin_id: String,
    publisher_fingerprint: String,
    id: String,
) -> Result<(), String> {
    approvals
        .change_root(&plugin_id, &publisher_fingerprint, &id)
        .await
}

/// Turns Core discovery on or off for a standing logical-library root.
#[tauri::command]
pub async fn plugin_library_root_set_watching(
    approvals: tauri::State<'_, Arc<crate::plugin_host::file_approvals::PluginFolderApprovals>>,
    plugin_id: String,
    publisher_fingerprint: String,
    id: String,
    watching: bool,
) -> Result<bool, String> {
    approvals
        .set_watching(&plugin_id, &publisher_fingerprint, &id, watching)
        .await
}

/// What the user allowed one plugin to start without asking, in the active profile.
#[tauri::command]
pub async fn plugin_remembered_confirmations(
    approvals: tauri::State<'_, Arc<crate::plugin_host::confirmations::PluginApprovals>>,
    plugin_id: String,
    publisher_fingerprint: String,
) -> Result<Vec<crate::plugin_host::confirmations::RememberedConfirmation>, String> {
    approvals.list(&plugin_id, &publisher_fingerprint).await
}

/// Forgets one remembered confirmation, so the next request asks the user again.
#[tauri::command]
pub async fn plugin_revoke_confirmation(
    approvals: tauri::State<'_, Arc<crate::plugin_host::confirmations::PluginApprovals>>,
    plugin_id: String,
    publisher_fingerprint: String,
    id: String,
) -> Result<bool, String> {
    approvals
        .revoke(&plugin_id, &publisher_fingerprint, &id)
        .await
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

/// The full achievement list, read from the local cache with no GameNode request.
#[tauri::command]
pub async fn achievement_list(
    engine: tauri::State<'_, Engine>,
    key: String,
    filter: gamenode_core::achievements::AchievementFilter,
) -> Result<Option<gamenode_core::achievements::AchievementListView>, String> {
    engine.achievement_list(key, filter).await
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
pub async fn ports_dismiss_plugin_setup(
    engine: tauri::State<'_, Engine>,
    request_id: String,
) -> Result<(), String> {
    engine.dismiss_plugin_port(request_id);
    Ok(())
}

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
