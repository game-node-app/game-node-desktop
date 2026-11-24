//! Scalable per-game tools engine methods: target inspection, release fetching,
//! transactional installation, update, rollback, anti-cheat guards, and cache management.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use gamenode_core::drm_free::LocalError;
use gamenode_core::tools::{
    GameToolView, InstallRequest, ProxyDllName, ToolCacheEntryView, ToolCatalogItemView,
    ToolChannel, ToolError, ToolId, ToolModalView, ToolOperationErrorCode, ToolOperationStatus,
    ToolRelease, ToolStore, builtin_tools, evaluate_anticheat, find_tool,
};

use super::Engine;

const RELEASE_CACHE_MAX_AGE: Duration = Duration::from_secs(3600);
/// How often the automatic tool update check runs while the application is open.
const TOOL_AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Opens the native executable picker for a game's install folder. Tests inject a fake so the
/// accepted, cancelled and failed paths do not need a real dialog.
pub trait ToolTargetPicker: Send + Sync {
    fn pick(&self, install_dir: &str) -> Result<Option<String>, String>;
}

/// One successful release-list fetch, kept with its identity so a worker can publish it without
/// overwriting a modal that changed meanwhile.
struct FetchedReleases {
    tool_id: ToolId,
    channel: ToolChannel,
    releases: Vec<ToolRelease>,
}

/// Identity of the open tools view at the moment a background fetch starts. The result is applied
/// only when the modal still shows the same game, tool and channel.
#[derive(Clone)]
struct ToolModalGuard {
    entry_key: String,
    tool_id: ToolId,
    channel: ToolChannel,
}

pub(super) struct PluginToolRequest {
    id: String,
    entry_key: String,
    tool_id: String,
    review: gamenode_core::tools::ToolPluginRequest,
    check: crate::plugin_host::jobs::Fence,
    done: tokio::sync::oneshot::Sender<Result<(), tonic::Status>>,
    started: bool,
}

struct ToolWait {
    engine: Engine,
    id: String,
}
impl Drop for ToolWait {
    fn drop(&mut self) {
        self.engine.cancel_plugin_tool(&self.id);
    }
}

impl Engine {
    pub(super) fn plugin_tools_list(
        &self,
        entry_key: &str,
    ) -> Result<Vec<gamenode_plugin_protocol::proto::ToolRecord>, tonic::Status> {
        let state = self.lock();
        if state.library.entry(entry_key).is_none()
            && !state
                .local
                .registrations
                .iter()
                .any(|r| r.key() == entry_key && !r.removed)
        {
            return Err(tonic::Status::not_found("game not found"));
        }
        drop(state);
        let installed = self
            .shared
            .ports
            .tools
            .as_ref()
            .map(|s| s.list_for_entry(entry_key))
            .transpose()
            .map_err(|_| tonic::Status::unavailable("tools state unavailable"))?
            .unwrap_or_default();
        Ok(builtin_tools()
            .into_iter()
            .map(|d| {
                let current = installed.iter().find(|i| i.tool_id == d.id);
                gamenode_plugin_protocol::proto::ToolRecord {
                    tool_id: d.id.as_str().into(),
                    name: d.name,
                    version: current.map(|i| i.version.clone()).unwrap_or_default(),
                    availability: if !cfg!(windows)
                        || self.shared.ports.tool_installer.is_none()
                        || self.shared.ports.tool_catalog.is_none()
                        || self.shared.ports.tools.is_none()
                    {
                        "unavailable"
                    } else if current.is_some_and(|i| {
                        i.state == gamenode_core::tools::ToolInstallState::Installed
                    }) {
                        "installed"
                    } else if current.is_some() {
                        "busy"
                    } else {
                        "available"
                    }
                    .into(),
                }
            })
            .collect())
    }

    pub(super) async fn request_plugin_tool(
        &self,
        entry_key: String,
        tool_id: String,
        review: gamenode_core::tools::ToolPluginRequest,
        check: crate::plugin_host::jobs::Fence,
    ) -> Result<(), tonic::Status> {
        if !cfg!(windows) {
            return Err(tonic::Status::unavailable("tools are Windows-only"));
        }
        self.review_plugin_tool(entry_key, tool_id, review, check)
            .await
    }

    /// Platform-independent review coordinator; callers enforce platform availability.
    pub(super) async fn review_plugin_tool(
        &self,
        entry_key: String,
        tool_id: String,
        review: gamenode_core::tools::ToolPluginRequest,
        check: crate::plugin_host::jobs::Fence,
    ) -> Result<(), tonic::Status> {
        if self.shared.ports.tool_installer.is_none()
            || self.shared.ports.tools.is_none()
            || self.shared.ports.tool_catalog.is_none()
        {
            return Err(tonic::Status::unavailable("tools ports unavailable"));
        }
        if find_tool(&tool_id).is_none() {
            return Err(tonic::Status::not_found("tool not found"));
        }
        check()?;
        let id = gamenode_platform::random::session_id();
        let (done, mut result) = tokio::sync::oneshot::channel();
        {
            let mut state = self.lock();
            if state.tools.modal.is_some() || state.plugin_tool_request.is_some() {
                return Err(tonic::Status::resource_exhausted(
                    "tools window is already in use",
                ));
            }
            state.plugin_tool_request = Some(PluginToolRequest {
                id: id.clone(),
                entry_key: entry_key.clone(),
                tool_id: tool_id.clone(),
                review,
                check: check.clone(),
                done,
                started: false,
            });
        }
        let _wait = ToolWait {
            engine: self.clone(),
            id: id.clone(),
        };
        let engine = self.clone();
        let request_id = id.clone();
        tokio::task::spawn_blocking(move || {
            engine.open_tools_impl(Some(entry_key), ToolId::new(tool_id), Some(&request_id))
        })
        .await
        .map_err(|_| tonic::Status::unavailable("tools review unavailable"))?;
        if self.lock().tools.modal.is_none() {
            return Err(tonic::Status::failed_precondition(
                "tools target unavailable",
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            tokio::select! {
                answer = &mut result => return answer.unwrap_or_else(|_| Err(tonic::Status::cancelled("tools review closed"))),
                () = tokio::time::sleep(Duration::from_millis(100)) => {
                    let started = self.lock().plugin_tool_request.as_ref().is_some_and(|r| r.id == id && r.started);
                    if !started {
                        check()?;
                        if Instant::now() >= deadline { return Err(tonic::Status::deadline_exceeded("tools review timed out")); }
                    }
                }
            }
        }
    }

    fn cancel_plugin_tool(&self, id: &str) {
        let mut state = self.lock();
        if state
            .plugin_tool_request
            .as_ref()
            .is_some_and(|r| r.id == id && !r.started)
        {
            state.plugin_tool_request.take();
            state.tools.modal = None;
            self.publish(&mut state);
        }
    }

    fn plugin_tool_operation_allowed(&self, remove: bool) -> bool {
        let state = self.lock();
        state
            .plugin_tool_request
            .as_ref()
            .is_none_or(|r| !r.started && r.review.remove == remove && (r.check)().is_ok())
    }

    fn begin_plugin_tool(
        &self,
        remove: bool,
    ) -> Result<Option<crate::plugin_host::jobs::Fence>, ()> {
        let mut state = self.lock();
        let Some(request) = state.plugin_tool_request.as_ref() else {
            return Ok(None);
        };
        if request.started
            || request.review.remove != remove
            || !state.tools.modal.as_ref().is_some_and(|m| {
                m.entry_key == request.entry_key && m.active_tool_id.as_str() == request.tool_id
            })
        {
            return Err(());
        }
        if (request.check)().is_err() {
            return Err(());
        }
        let request = state.plugin_tool_request.as_mut().unwrap();
        request.started = true;
        Ok(Some(request.check.clone()))
    }

    /// Dispatches startup recovery of incomplete tool jobs and initializes installed tools
    /// and catalog state.
    pub fn recover_and_load_tools(&self) {
        let engine = self.clone();
        self.shared.storage.submit(move || {
            let (store, installer) = (
                engine.shared.ports.tools.clone(),
                engine.shared.ports.tool_installer.clone(),
            );
            let Some(store) = store else {
                return;
            };

            // 1. Recover incomplete installations
            if let Some(installer) = &installer {
                let resolve = |key: &str| -> Option<String> {
                    let state = engine.lock();
                    if let Some(entry) = state.library.entry(key)
                        && !entry.install_dir.is_empty()
                    {
                        return Some(entry.install_dir.clone());
                    }
                    if let Some(id) = key.strip_prefix("manual:")
                        && let Some(reg) = state
                            .local
                            .registrations
                            .iter()
                            .find(|r| r.id == id && !r.removed)
                    {
                        return Some(reg.root.clone());
                    }
                    state
                        .local
                        .registrations
                        .iter()
                        .find(|r| r.key() == key && !r.removed)
                        .map(|r| r.root.clone())
                };
                if let Err(error) = installer.recover_incomplete(&*store, &resolve) {
                    tracing::warn!(%error, "failed to recover incomplete tool installations");
                }
            }

            // 2. Load all installed tools
            let installations = store.list_installations().unwrap_or_else(|error| {
                tracing::warn!(%error, "could not list tool installations");
                Vec::new()
            });

            let mut installed_by_entry: BTreeMap<String, Vec<GameToolView>> = BTreeMap::new();
            for inst in installations {
                let descriptor = find_tool(inst.tool_id.as_str());
                let name = descriptor
                    .as_ref()
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| inst.tool_id.as_str().to_string());
                let summary = descriptor
                    .as_ref()
                    .map(|d| d.summary.clone())
                    .unwrap_or_default();
                let view = GameToolView {
                    tool_id: inst.tool_id,
                    name,
                    channel: inst.channel,
                    version: inst.version,
                    proxy_dll: inst.proxy_dll.map(|p| p.as_str().to_string()),
                    target_exe: inst.target_exe,
                    state: inst.state,
                    summary,
                };
                installed_by_entry
                    .entry(inst.entry_key)
                    .or_default()
                    .push(view);
            }

            // 3. Catalog items from built-in registry
            let catalog_items = builtin_catalog_items();

            // 4. Cache statistics. The tool total is the sum of the listed archives; achievement
            // icons have their own accounting and are not added to it.
            let cache_entries = tool_cache_entries(&*store);
            let total_cache_bytes = cache_entries.iter().map(|entry| entry.bytes).sum();
            let total_cache_archives = cache_entries.len();
            let achievement_icon_cache_bytes =
                gamenode_platform::achievement_icons::achievement_icon_usage(
                    &engine.shared.ports.covers_dir,
                );

            let mut state = engine.lock();
            state.tools.installed_by_entry = installed_by_entry;
            state.tools.catalog = catalog_items;
            state.tools.total_cache_bytes = total_cache_bytes;
            state.tools.total_cache_archives = total_cache_archives;
            state.tools.cache_entries = cache_entries;
            state.tools.achievement_icon_cache_bytes = achievement_icon_cache_bytes;
            engine.publish(&mut state);
        });
    }

    /// Schedules the automatic tool update check when due. The preference is read from the live
    /// state, so turning it off stops future checks immediately. At most one check runs; it only
    /// fetches release metadata and never downloads an archive.
    pub(crate) fn tool_auto_check_at(&self, now: Instant) {
        if !self.preferences().tool_auto_check {
            return;
        }
        if self
            .shared
            .tool_auto_check_running
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let due = {
            let mut last = self
                .shared
                .tool_auto_check_last
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let due = last
                .is_none_or(|last| now.saturating_duration_since(last) >= TOOL_AUTO_CHECK_INTERVAL);
            if due {
                *last = Some(now);
            }
            due
        };
        if !due {
            self.shared
                .tool_auto_check_running
                .store(false, std::sync::atomic::Ordering::Release);
            return;
        }

        let modal_guard = self.tool_modal_guard();
        let catalog = self.shared.ports.tool_catalog.clone();
        let Some(catalog) = catalog else {
            self.shared
                .tool_auto_check_running
                .store(false, std::sync::atomic::Ordering::Release);
            return;
        };
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let targets = engine.tool_auto_check_targets();
            let mut fetched = Vec::new();
            for (tool_id, channel) in targets {
                let Some(descriptor) = find_tool(tool_id.as_str()) else {
                    continue;
                };
                if let Ok(releases) = catalog.fetch_releases(&descriptor, channel) {
                    fetched.push(FetchedReleases {
                        tool_id,
                        channel,
                        releases,
                    });
                }
            }
            // Publish the fetched lists to the open modal (when it still shows the same game,
            // tool and channel) and refresh the cache view in the same snapshot. This is the
            // automatic check's only path: it never downloads an archive.
            engine.apply_fetched_releases(fetched, modal_guard.as_ref());
            engine
                .shared
                .tool_auto_check_running
                .store(false, std::sync::atomic::Ordering::Release);
        });
    }

    /// The tool/channel pairs the automatic check refreshes: every built-in tool's stable channel,
    /// plus the channel of each installed version so an installed Nightly stays current.
    #[cfg(test)]
    pub(crate) fn tool_auto_check_is_running(&self) -> bool {
        self.shared
            .tool_auto_check_running
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// The tool/channel pairs the automatic check refreshes: every built-in tool's stable channel,
    /// plus the channel of each installed version so an installed Nightly stays current.
    fn tool_auto_check_targets(&self) -> Vec<(ToolId, ToolChannel)> {
        let mut targets = std::collections::BTreeSet::new();
        for descriptor in builtin_tools() {
            targets.insert((descriptor.id, ToolChannel::Stable));
        }
        if let Some(store) = &self.shared.ports.tools
            && let Ok(installations) = store.list_installations()
        {
            for installation in installations {
                targets.insert((installation.tool_id, installation.channel));
            }
        }
        targets.into_iter().collect()
    }

    /// Captures the open tools view so a background fetch only applies to the modal it started
    /// for. A closed modal, another game, another tool or another channel makes the result stale
    /// for the visible view (the cache metadata is still refreshed).
    fn tool_modal_guard(&self) -> Option<ToolModalGuard> {
        self.lock()
            .tools
            .modal
            .as_ref()
            .map(|modal| ToolModalGuard {
                entry_key: modal.entry_key.clone(),
                tool_id: modal.active_tool_id.clone(),
                channel: modal.selected_channel,
            })
    }

    /// Persists fetched release lists, refreshes the tools cache view and publishes the result.
    /// The open modal is updated only when `guard` still matches its game, tool and channel; its
    /// current version selection is kept when the refreshed list still offers that version.
    fn apply_fetched_releases(
        &self,
        mut fetched: Vec<FetchedReleases>,
        guard: Option<&ToolModalGuard>,
    ) {
        if let Some(store) = &self.shared.ports.tools {
            for item in &mut fetched {
                if let Err(error) = store.save_releases(&item.tool_id, item.channel, &item.releases)
                {
                    tracing::warn!(%error, "could not persist fetched tool releases");
                }
                // `save_releases` keeps cached versions upstream no longer lists. Reading the
                // merged rows back keeps those versions available in the open picker too, so a
                // selection does not jump when a cached archive disappears from the API list.
                // Rows whose archive was deleted outside the application are left out.
                if let Ok(stored) = store.load_releases(&item.tool_id, item.channel) {
                    let merged: Vec<ToolRelease> = stored
                        .into_iter()
                        .filter_map(|mut release| {
                            let missing = release
                                .cached_path
                                .as_deref()
                                .is_some_and(|path| !Path::new(path).is_file());
                            if missing {
                                if !item
                                    .releases
                                    .iter()
                                    .any(|offered| offered.version == release.version)
                                {
                                    return None;
                                }
                                release.cached_path = None;
                            }
                            Some(release)
                        })
                        .collect();
                    if !merged.is_empty() {
                        item.releases = merged;
                    }
                }
            }
        }

        let cache_entries = self
            .shared
            .ports
            .tools
            .as_ref()
            .map(|store| tool_cache_entries(store.as_ref()))
            .unwrap_or_default();
        let total_cache_bytes = cache_entries.iter().map(|entry| entry.bytes).sum();
        let total_cache_archives = cache_entries.len();
        let achievement_icon_cache_bytes =
            gamenode_platform::achievement_icons::achievement_icon_usage(
                &self.shared.ports.covers_dir,
            );

        let mut state = self.lock();
        for item in &fetched {
            state
                .tool_last_fetch
                .insert((item.tool_id.clone(), item.channel), Instant::now());
        }
        state.tools.total_cache_bytes = total_cache_bytes;
        state.tools.total_cache_archives = total_cache_archives;
        state.tools.cache_entries = cache_entries;
        state.tools.achievement_icon_cache_bytes = achievement_icon_cache_bytes;

        if let Some(guard) = guard
            && let Some(item) = fetched
                .iter()
                .find(|item| item.tool_id == guard.tool_id && item.channel == guard.channel)
            && let Some(modal) = &mut state.tools.modal
            && modal.entry_key == guard.entry_key
            && modal.active_tool_id == guard.tool_id
            && modal.selected_channel == guard.channel
        {
            modal.available_versions = item.releases.clone();
            if modal.selected_version.as_ref().is_none_or(|version| {
                !modal
                    .available_versions
                    .iter()
                    .any(|release| &release.version == version)
            }) {
                modal.selected_version =
                    modal.available_versions.first().map(|r| r.version.clone());
            }
        }
        self.publish(&mut state);
    }

    /// Opens the tools modal for the currently selected game in the library.
    pub fn open_tools_for_selected(&self) {
        let key = {
            let mut state = self.lock();
            state.library.view().and_then(|v| {
                if let [key] = v.selected.as_slice() {
                    Some(key.clone())
                } else {
                    None
                }
            })
        };
        self.open_tools(key);
    }

    /// Opens the tools modal for the specified `entry_key`.
    pub fn open_tools(&self, entry_key: Option<String>) {
        if self.lock().plugin_tool_request.is_some() {
            return;
        }
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            if engine.lock().plugin_tool_request.is_none() {
                engine.open_tools_impl(entry_key, ToolId::new("optiscaler"), None);
            }
        });
    }

    fn open_tools_impl(
        &self,
        entry_key: Option<String>,
        active_tool_id: ToolId,
        request_id: Option<&str>,
    ) {
        let Some(entry_key) = entry_key else {
            return;
        };

        let (title, install_dir, manual_exe) = {
            let state = self.lock();
            let manual = state
                .local
                .registrations
                .iter()
                .find(|r| r.key() == entry_key && !r.removed);
            if let Some(reg) = manual {
                (
                    reg.title.clone(),
                    reg.root.clone(),
                    Some(reg.executable.clone()),
                )
            } else if let Some(entry) = state.library.entry(&entry_key) {
                let title = entry.name.clone().unwrap_or_else(|| entry.store_id.clone());
                (title, entry.install_dir.clone(), None)
            } else {
                return;
            }
        };

        if install_dir.trim().is_empty() {
            return;
        }

        let mut state = self.lock();
        if match request_id {
            Some(id) => !state
                .plugin_tool_request
                .as_ref()
                .is_some_and(|r| r.id == id),
            None => state.plugin_tool_request.is_some(),
        } {
            return;
        }
        let existing_tools = state
            .tools
            .installed_by_entry
            .get(&entry_key)
            .cloned()
            .unwrap_or_default();

        let installed_tool = existing_tools.iter().find(|t| t.tool_id == active_tool_id);

        let channel = installed_tool
            .map(|t| t.channel)
            .unwrap_or(state.settings.preferences.tool_default_channel);

        let proxy = installed_tool
            .and_then(|t| {
                t.proxy_dll
                    .as_deref()
                    .and_then(ProxyDllName::parse_flexible)
            })
            .unwrap_or(ProxyDllName::Dxgi);

        let cached_releases = if let Some(store) = &self.shared.ports.tools {
            store
                .load_releases(&active_tool_id, channel)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let selected_version = installed_tool
            .map(|t| t.version.clone())
            .or_else(|| cached_releases.first().map(|r| r.version.clone()));

        let plugin_request = state.plugin_tool_request.as_ref().map(|r| r.review.clone());
        state.tools.modal = Some(ToolModalView {
            plugin_request,
            entry_key: entry_key.clone(),
            title: title.clone(),
            install_dir: install_dir.clone(),
            inspection: None,
            tools: existing_tools,
            active_tool_id: active_tool_id.clone(),
            selected_channel: channel,
            available_versions: cached_releases.clone(),
            selected_version,
            selected_proxy: proxy,
            anticheat_confirmed: false,
            operation: None,
        });

        state.tool_reset_config = false;
        state.tool_selected_target_exe = None;
        self.publish(&mut state);

        // Background target inspection and anti-cheat checks
        let (installer, catalog) = (
            self.shared.ports.tool_installer.clone(),
            self.shared.ports.tool_catalog.clone(),
        );

        let engine = self.clone();
        let k = entry_key.clone();
        let dir = install_dir.clone();
        let tit = title.clone();
        let m_exe = manual_exe.clone();
        self.shared.runtime.spawn_blocking(move || {
            if let Some(installer) = installer {
                match installer.inspect_target(&k, &dir, &tit, m_exe.as_deref()) {
                    Ok(mut inspection) => {
                        let extra = evaluate_anticheat(&k, &tit, []);
                        for s in extra.detected {
                            if !inspection.anticheat.detected.contains(&s) {
                                inspection.anticheat.detected.push(s);
                            }
                        }
                        for e in extra.evidence {
                            if !inspection.anticheat.evidence.contains(&e) {
                                inspection.anticheat.evidence.push(e);
                            }
                        }

                        let mut state = engine.lock();
                        let modal_matches =
                            state.tools.modal.as_ref().is_some_and(|m| m.entry_key == k);
                        if modal_matches {
                            if let Some(sel) = &state.tool_selected_target_exe {
                                inspection.selected_exe = Some(sel.clone());
                            } else if inspection.selected_exe.is_some() {
                                state.tool_selected_target_exe = inspection.selected_exe.clone();
                            }
                            if let Some(modal) = &mut state.tools.modal {
                                modal.inspection = Some(inspection);
                            }
                            engine.publish(&mut state);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(%err, "failed to inspect game target for tools");
                    }
                }
            }
        });

        // Background release catalog refresh if cached releases are empty or older than 1 hour
        let needs_fetch = cached_releases.is_empty()
            || state
                .tool_last_fetch
                .get(&(active_tool_id.clone(), channel))
                .map(|t| t.elapsed() > RELEASE_CACHE_MAX_AGE)
                .unwrap_or(true);

        if needs_fetch {
            let descriptor = find_tool(active_tool_id.as_str());
            if let (Some(catalog), Some(descriptor)) = (catalog, descriptor) {
                let engine = self.clone();
                let guard = ToolModalGuard {
                    entry_key: entry_key.clone(),
                    tool_id: active_tool_id.clone(),
                    channel,
                };
                self.shared.runtime.spawn_blocking(move || {
                    if let Ok(releases) = catalog.fetch_releases(&descriptor, channel) {
                        engine.apply_fetched_releases(
                            vec![FetchedReleases {
                                tool_id: active_tool_id,
                                channel,
                                releases,
                            }],
                            Some(&guard),
                        );
                    }
                });
            }
        }
    }

    /// Closes the tools modal dialog unless an installation/uninstallation is actively progressing.
    pub fn close_tools(&self) {
        let mut state = self.lock();
        if let Some(modal) = &state.tools.modal
            && matches!(
                modal.operation,
                Some(
                    ToolOperationStatus::Installing { .. }
                        | ToolOperationStatus::Updating { .. }
                        | ToolOperationStatus::Uninstalling
                )
            )
        {
            return;
        }
        state.tools.modal = None;
        if let Some(request) = state.plugin_tool_request.take() {
            let _ = request
                .done
                .send(Err(tonic::Status::cancelled("tools review declined")));
        }
        self.publish(&mut state);
    }

    /// Switches the active release channel (`Stable` or `Nightly`).
    pub fn select_tool_channel(&self, channel: ToolChannel) {
        let mut state = self.lock();
        let Some(modal) = &mut state.tools.modal else {
            return;
        };
        if modal.selected_channel == channel {
            return;
        }
        modal.selected_channel = channel;
        let tool_id = modal.active_tool_id.clone();
        let entry_key = modal.entry_key.clone();

        let cached = if let Some(store) = &self.shared.ports.tools {
            store.load_releases(&tool_id, channel).unwrap_or_default()
        } else {
            Vec::new()
        };
        modal.available_versions = cached.clone();
        modal.selected_version = modal.available_versions.first().map(|r| r.version.clone());
        self.publish(&mut state);

        let needs_fetch = cached.is_empty()
            || state
                .tool_last_fetch
                .get(&(tool_id.clone(), channel))
                .map(|t| t.elapsed() > RELEASE_CACHE_MAX_AGE)
                .unwrap_or(true);

        if needs_fetch {
            let descriptor = find_tool(tool_id.as_str());
            let catalog = self.shared.ports.tool_catalog.clone();
            if let (Some(catalog), Some(descriptor)) = (catalog, descriptor) {
                let engine = self.clone();
                let guard = ToolModalGuard {
                    entry_key,
                    tool_id: tool_id.clone(),
                    channel,
                };
                self.shared.runtime.spawn_blocking(move || {
                    if let Ok(releases) = catalog.fetch_releases(&descriptor, channel) {
                        engine.apply_fetched_releases(
                            vec![FetchedReleases {
                                tool_id,
                                channel,
                                releases,
                            }],
                            Some(&guard),
                        );
                    }
                });
            }
        }
    }

    /// Forces a fresh query to GitHub for releases of the current channel.
    pub fn refresh_tool_versions(&self) {
        let (tool_id, channel, entry_key) = {
            let state = self.lock();
            let Some(modal) = &state.tools.modal else {
                return;
            };
            (
                modal.active_tool_id.clone(),
                modal.selected_channel,
                modal.entry_key.clone(),
            )
        };

        let descriptor = find_tool(tool_id.as_str());
        let catalog = self.shared.ports.tool_catalog.clone();
        if let (Some(catalog), Some(descriptor)) = (catalog, descriptor) {
            let engine = self.clone();
            let guard = ToolModalGuard {
                entry_key,
                tool_id: tool_id.clone(),
                channel,
            };
            self.shared.runtime.spawn_blocking(move || {
                if let Ok(releases) = catalog.fetch_releases(&descriptor, channel) {
                    engine.apply_fetched_releases(
                        vec![FetchedReleases {
                            tool_id,
                            channel,
                            releases,
                        }],
                        Some(&guard),
                    );
                }
            });
        }
    }

    /// Selects a specific release version in the picker.
    pub fn select_tool_version(&self, version: String) {
        let mut state = self.lock();
        if let Some(modal) = &mut state.tools.modal {
            modal.selected_version = Some(version);
            self.publish(&mut state);
        }
    }

    /// Updates the selected target executable for deployment.
    pub fn select_tool_target_exe(&self, relative_exe: String) {
        let mut state = self.lock();
        state.tool_selected_target_exe = Some(relative_exe.clone());
        if let Some(modal) = &mut state.tools.modal {
            if let Some(inspection) = &mut modal.inspection {
                inspection.selected_exe = Some(relative_exe);
            }
            self.publish(&mut state);
        }
    }

    /// Opens the executable picker for the open game's install folder on a worker, then feeds the
    /// chosen file through [`Self::select_tool_target_exe`] after the existing containment rules.
    /// Cancelling, or a pick outside the folder, leaves the current target untouched.
    pub async fn pick_tool_target_exe(&self) {
        let Some((entry_key, install_dir)) = ({
            let state = self.lock();
            state
                .tools
                .modal
                .as_ref()
                .map(|modal| (modal.entry_key.clone(), modal.install_dir.clone()))
        }) else {
            return;
        };
        let picker = self.shared.ports.tool_picker.clone();
        let folder = install_dir.clone();
        let picked = self
            .shared
            .runtime
            .spawn_blocking(move || {
                let raw_pick = match picker {
                    Some(picker) => picker
                        .pick(&folder)
                        .map_err(|message| (ToolOperationErrorCode::PickerFailed, message)),
                    None => gamenode_platform::installations::pick_executable_in(&folder).map_err(
                        |error| match error {
                            LocalError::Unsupported => (
                                ToolOperationErrorCode::PickerUnsupported,
                                "the executable picker is unsupported on this platform".to_string(),
                            ),
                            other => (ToolOperationErrorCode::PickerFailed, other.to_string()),
                        },
                    ),
                }?;
                let Some(path) = raw_pick else {
                    return Ok(None);
                };
                gamenode_platform::tools::inspect::validate_target_within_install_dir(
                    Path::new(&folder),
                    &path,
                )
                .map(|(_, relative_exe)| Some(relative_exe))
                .map_err(|error| {
                    let code = match &error {
                        ToolError::PathEscapesInstallDir(_) => {
                            ToolOperationErrorCode::TargetOutsideInstallDir
                        }
                        ToolError::TargetExeNotFound(_) => {
                            ToolOperationErrorCode::TargetExeNotFound
                        }
                        ToolError::InstallDirNotFound(_) => {
                            ToolOperationErrorCode::InstallDirNotFound
                        }
                        _ => ToolOperationErrorCode::PickerFailed,
                    };
                    (code, error.to_string())
                })
            })
            .await;
        match picked {
            Ok(Ok(Some(relative_exe))) => {
                let still_open = {
                    let state = self.lock();
                    state
                        .tools
                        .modal
                        .as_ref()
                        .is_some_and(|modal| modal.entry_key == entry_key)
                };
                if !still_open {
                    return;
                }
                self.select_tool_target_exe(relative_exe);
                // A previous picker failure is resolved by a successful choice.
                let mut state = self.lock();
                if let Some(modal) = &mut state.tools.modal
                    && modal.entry_key == entry_key
                    && matches!(modal.operation, Some(ToolOperationStatus::Failed { .. }))
                {
                    modal.operation = None;
                    self.publish(&mut state);
                }
            }
            // The user cancelled the dialog: the current target stays.
            Ok(Ok(None)) => {}
            Ok(Err((code, message))) => {
                self.report_tool_target_error(&entry_key, Some(code), message);
            }
            Err(error) => self.report_tool_target_error(
                &entry_key,
                Some(ToolOperationErrorCode::PickerFailed),
                format!("the executable picker stopped: {error}"),
            ),
        }
    }

    fn report_tool_target_error(
        &self,
        entry_key: &str,
        code: Option<ToolOperationErrorCode>,
        message: String,
    ) {
        let mut state = self.lock();
        if let Some(modal) = &mut state.tools.modal
            && modal.entry_key == entry_key
        {
            modal.operation = Some(ToolOperationStatus::Failed { message, code });
            self.publish(&mut state);
        }
    }

    /// Selects the proxy DLL filename (e.g. `dxgi.dll`).
    pub fn select_tool_proxy(&self, proxy: String) {
        if let Some(parsed) = ProxyDllName::parse_flexible(&proxy) {
            let mut state = self.lock();
            if let Some(modal) = &mut state.tools.modal {
                modal.selected_proxy = parsed;
                self.publish(&mut state);
            }
        }
    }

    /// Sets whether the user has confirmed the anti-cheat warning banner.
    pub fn set_tool_anticheat_confirmed(&self, confirmed: bool) {
        let mut state = self.lock();
        if let Some(modal) = &mut state.tools.modal {
            modal.anticheat_confirmed = confirmed;
            self.publish(&mut state);
        }
    }

    /// Sets whether the tool configuration file should be reset on install/update.
    pub fn set_tool_reset_config(&self, reset: bool) {
        let mut state = self.lock();
        state.tool_reset_config = reset;
        self.publish(&mut state);
    }

    /// Executes transactional installation or version update of the active tool.
    pub fn install_or_update_tool(&self, is_update: bool) {
        if !self.plugin_tool_operation_allowed(false) {
            return;
        }
        let (request, release, descriptor, install_dir) = {
            let mut state = self.lock();
            let (
                entry_key,
                active_tool_id,
                channel,
                selected_proxy,
                anticheat_confirmed,
                has_warning,
                target_exe,
                selected_version,
                install_dir,
            ) = {
                let Some(modal) = &state.tools.modal else {
                    return;
                };

                let target_exe = modal
                    .inspection
                    .as_ref()
                    .and_then(|i| i.selected_exe.clone())
                    .or_else(|| state.tool_selected_target_exe.clone());

                let has_warning = modal
                    .inspection
                    .as_ref()
                    .is_some_and(|i| i.anticheat.has_warning());

                (
                    modal.entry_key.clone(),
                    modal.active_tool_id.clone(),
                    modal.selected_channel,
                    modal.selected_proxy,
                    modal.anticheat_confirmed,
                    has_warning,
                    target_exe,
                    modal.selected_version.clone(),
                    modal.install_dir.clone(),
                )
            };

            // Game must not be running
            if state.playtime.tracker.running().contains(&entry_key) {
                if let Some(modal) = &mut state.tools.modal {
                    modal.operation = Some(ToolOperationStatus::Failed {
                        message: "Game is currently running; stop the game before modifying tools"
                            .to_string(),
                        code: None,
                    });
                }
                self.publish(&mut state);
                return;
            }

            // Anti-cheat warning confirmation
            if has_warning && !anticheat_confirmed {
                if let Some(modal) = &mut state.tools.modal {
                    modal.operation = Some(ToolOperationStatus::Failed {
                        message: "Anti-cheat warning must be confirmed before installing"
                            .to_string(),
                        code: None,
                    });
                }
                self.publish(&mut state);
                return;
            }

            // Target executable selected
            let Some(target_exe) = target_exe else {
                if let Some(modal) = &mut state.tools.modal {
                    modal.operation = Some(ToolOperationStatus::Failed {
                        message: "No target executable selected".to_string(),
                        code: None,
                    });
                }
                self.publish(&mut state);
                return;
            };

            // Version release selected
            let Some(selected_version) = selected_version else {
                if let Some(modal) = &mut state.tools.modal {
                    modal.operation = Some(ToolOperationStatus::Failed {
                        message: "No version selected".to_string(),
                        code: None,
                    });
                }
                self.publish(&mut state);
                return;
            };

            let release = state.tools.modal.as_ref().and_then(|m| {
                m.available_versions
                    .iter()
                    .find(|r| r.version == selected_version)
                    .cloned()
            });

            let Some(release) = release else {
                if let Some(modal) = &mut state.tools.modal {
                    modal.operation = Some(ToolOperationStatus::Failed {
                        message: format!("Release not found for version {selected_version}"),
                        code: None,
                    });
                }
                self.publish(&mut state);
                return;
            };

            let descriptor = find_tool(active_tool_id.as_str());
            let Some(descriptor) = descriptor else {
                if let Some(modal) = &mut state.tools.modal {
                    modal.operation = Some(ToolOperationStatus::Failed {
                        message: format!("Tool not found: {active_tool_id}"),
                        code: None,
                    });
                }
                self.publish(&mut state);
                return;
            };

            let reset_config = state.tool_reset_config;

            let request = InstallRequest {
                entry_key,
                tool_id: active_tool_id,
                channel,
                version: release.version.clone(),
                target_exe,
                proxy_dll: Some(selected_proxy),
                reset_config,
                anticheat_confirmed,
                custom_payload_path: None,
            };

            let op = if is_update {
                ToolOperationStatus::Updating {
                    progress_percent: None,
                    message: format!("Updating {} to {}...", descriptor.name, release.version),
                }
            } else {
                ToolOperationStatus::Installing {
                    progress_percent: None,
                    message: format!("Installing {} {}...", descriptor.name, release.version),
                }
            };
            if let Some(modal) = &mut state.tools.modal {
                modal.operation = Some(op);
            }
            self.publish(&mut state);

            (request, release, descriptor, install_dir)
        };

        let (installer, catalog, store) = (
            self.shared.ports.tool_installer.clone(),
            self.shared.ports.tool_catalog.clone(),
            self.shared.ports.tools.clone(),
        );

        let (Some(installer), Some(catalog), Some(store)) = (installer, catalog, store) else {
            let mut state = self.lock();
            if let Some(modal) = &mut state.tools.modal {
                modal.operation = Some(ToolOperationStatus::Failed {
                    message: "Tools ports are not configured".to_string(),
                    code: None,
                });
                self.publish(&mut state);
            }
            return;
        };

        let Ok(fence) = self.begin_plugin_tool(false) else {
            return;
        };
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let check = || {
                fence
                    .as_ref()
                    .map(|f| {
                        f().map_err(|_| {
                            gamenode_core::tools::ToolError::Failed(
                                "tool authorization ended".into(),
                            )
                        })
                    })
                    .unwrap_or(Ok(()))
            };
            let result = if fence.is_some() {
                installer.install_or_update_checked(
                    &*store,
                    &*catalog,
                    &descriptor,
                    &release,
                    &request,
                    &install_dir,
                    &check,
                )
            } else {
                installer.install_or_update(
                    &*store,
                    &*catalog,
                    &descriptor,
                    &release,
                    &request,
                    &install_dir,
                )
            };

            match result {
                Ok(_installation) => {
                    engine.reload_tools_after_mutation(&request.entry_key, Ok(()));
                }
                Err(err) => {
                    engine.reload_tools_after_mutation(&request.entry_key, Err(err.to_string()));
                }
            }
        });
    }

    /// Executes transactional uninstallation of the active tool.
    pub fn uninstall_tool(&self) {
        if !self.plugin_tool_operation_allowed(true) {
            return;
        }
        let (entry_key, tool_id, install_dir) = {
            let mut state = self.lock();
            let Some(modal) = &mut state.tools.modal else {
                return;
            };
            modal.operation = Some(ToolOperationStatus::Uninstalling);
            let entry_key = modal.entry_key.clone();
            let tool_id = modal.active_tool_id.clone();
            let install_dir = modal.install_dir.clone();
            self.publish(&mut state);
            (entry_key, tool_id, install_dir)
        };

        let (installer, store) = (
            self.shared.ports.tool_installer.clone(),
            self.shared.ports.tools.clone(),
        );

        let (Some(installer), Some(store)) = (installer, store) else {
            let mut state = self.lock();
            if let Some(modal) = &mut state.tools.modal {
                modal.operation = Some(ToolOperationStatus::Failed {
                    message: "Tools ports are not configured".to_string(),
                    code: None,
                });
                self.publish(&mut state);
            }
            return;
        };

        let Ok(fence) = self.begin_plugin_tool(true) else {
            return;
        };
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let check = || {
                fence
                    .as_ref()
                    .map(|f| {
                        f().map_err(|_| {
                            gamenode_core::tools::ToolError::Failed(
                                "tool authorization ended".into(),
                            )
                        })
                    })
                    .unwrap_or(Ok(()))
            };
            let result = if fence.is_some() {
                installer.uninstall_checked(
                    &*store,
                    &entry_key,
                    tool_id.as_str(),
                    &install_dir,
                    &check,
                )
            } else {
                installer.uninstall(&*store, &entry_key, tool_id.as_str(), &install_dir)
            };
            match result {
                Ok(()) => {
                    engine.reload_tools_after_mutation(&entry_key, Ok(()));
                }
                Err(err) => {
                    engine.reload_tools_after_mutation(&entry_key, Err(err.to_string()));
                }
            }
        });
    }

    /// Opens the tool's source / repository page in the system browser.
    pub fn open_tool_source(&self, url: String) {
        if let Ok(parsed) = url::Url::parse(&url) {
            let _ = self.shared.ports.browser.open(&parsed);
        }
    }

    /// Clears cached release archives from disk and resets database metadata.
    pub fn clear_tool_cache(&self) {
        let (catalog, store) = (
            self.shared.ports.tool_catalog.clone(),
            self.shared.ports.tools.clone(),
        );
        let engine = self.clone();
        let covers_dir = self.shared.ports.covers_dir.clone();
        self.shared.storage.submit(move || {
            if let Some(catalog) = &catalog {
                let _ = catalog.clear_cache();
            }
            if let Some(store) = &store {
                let _ = store.clear_cached_archives_metadata();
            }
            gamenode_platform::achievement_icons::clear_achievement_icons(&covers_dir);
            let mut state = engine.lock();
            state.tools.total_cache_bytes = 0;
            state.tools.total_cache_archives = 0;
            state.tools.achievement_icon_cache_bytes = 0;
            state.tools.cache_entries.clear();
            if let Some(modal) = &mut state.tools.modal {
                for r in &mut modal.available_versions {
                    r.cached_path = None;
                }
            }
            engine.publish(&mut state);
        });
    }

    /// Helper called after an install, update, or uninstall to refresh installations and cache usage.
    fn reload_tools_after_mutation(&self, entry_key: &str, outcome: Result<(), String>) {
        let store = self.shared.ports.tools.clone();
        let all_installs = store
            .as_ref()
            .and_then(|s| s.list_installations().ok())
            .unwrap_or_default();

        let mut map: BTreeMap<String, Vec<GameToolView>> = BTreeMap::new();
        for inst in all_installs {
            let descriptor = find_tool(inst.tool_id.as_str());
            let name = descriptor
                .as_ref()
                .map(|d| d.name.clone())
                .unwrap_or_else(|| inst.tool_id.as_str().to_string());
            let summary = descriptor
                .as_ref()
                .map(|d| d.summary.clone())
                .unwrap_or_default();
            map.entry(inst.entry_key.clone())
                .or_default()
                .push(GameToolView {
                    tool_id: inst.tool_id,
                    name,
                    channel: inst.channel,
                    version: inst.version,
                    proxy_dll: inst.proxy_dll.map(|p| p.as_str().to_string()),
                    target_exe: inst.target_exe,
                    state: inst.state,
                    summary,
                });
        }

        let cache_entries = store
            .as_ref()
            .map(|store| tool_cache_entries(store.as_ref()))
            .unwrap_or_default();
        let total_cache_bytes = cache_entries.iter().map(|entry| entry.bytes).sum();
        let total_cache_archives = cache_entries.len();
        let achievement_icon_cache_bytes =
            gamenode_platform::achievement_icons::achievement_icon_usage(
                &self.shared.ports.covers_dir,
            );

        let new_modal_tools = map.get(entry_key).cloned().unwrap_or_default();

        let mut state = self.lock();
        state.tools.installed_by_entry = map;
        state.tools.total_cache_bytes = total_cache_bytes;
        state.tools.total_cache_archives = total_cache_archives;
        state.tools.cache_entries = cache_entries;
        state.tools.achievement_icon_cache_bytes = achievement_icon_cache_bytes;

        if state
            .plugin_tool_request
            .as_ref()
            .is_some_and(|r| r.entry_key == entry_key && r.started)
        {
            let request = state.plugin_tool_request.take().unwrap();
            let answer = outcome.as_ref().map(|_| ()).map_err(|_| {
                tonic::Status::failed_precondition(
                    "tool operation failed; inspect Core tool status",
                )
            });
            let _ = request.done.send(answer);
            // The plugin request has ended. Keep errors visible, without leaving a live action.
            if let Some(modal) = &mut state.tools.modal {
                modal.plugin_request = None;
            }
        }
        if let Some(modal) = &mut state.tools.modal
            && modal.entry_key == entry_key
        {
            modal.tools = new_modal_tools;

            match outcome {
                Ok(()) => {
                    modal.operation = None;
                }
                Err(err_msg) => {
                    modal.operation = Some(ToolOperationStatus::Failed {
                        message: err_msg,
                        code: None,
                    });
                }
            }
        }
        self.publish(&mut state);
    }
}

/// Builds the Settings cache listing from the persisted release rows whose archive is present,
/// marking a version in use when a game installation references the same tool, version and
/// channel.
fn tool_cache_entries(store: &dyn ToolStore) -> Vec<ToolCacheEntryView> {
    let releases = store.list_cached_releases().unwrap_or_else(|error| {
        tracing::warn!(%error, "could not list cached tool releases");
        Vec::new()
    });
    let installations = store.list_installations().unwrap_or_else(|error| {
        tracing::warn!(%error, "could not list tool installations for the cache view");
        Vec::new()
    });
    releases
        .into_iter()
        .filter(|release| {
            release
                .cached_path
                .as_deref()
                .is_some_and(|path| Path::new(path).is_file())
        })
        .map(|release| {
            let in_use = installations.iter().any(|installation| {
                installation.tool_id == release.tool_id
                    && installation.version == release.version
                    && installation.channel == release.channel
            });
            ToolCacheEntryView {
                tool_id: release.tool_id,
                version: release.version,
                channel: release.channel,
                bytes: release.size_bytes,
                sha256_verified: release
                    .sha256
                    .as_deref()
                    .is_some_and(|digest| !digest.trim().is_empty()),
                in_use,
            }
        })
        .collect()
}

/// Helper converting built-in tool descriptors to catalog item views.
pub(crate) fn builtin_catalog_items() -> Vec<ToolCatalogItemView> {
    builtin_tools()
        .iter()
        .map(|d| ToolCatalogItemView {
            id: d.id.clone(),
            name: d.name.clone(),
            summary: d.summary.clone(),
            license: d.license.clone(),
            source_url: d.source_url.clone(),
        })
        .collect()
}
