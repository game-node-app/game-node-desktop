//! Scalable per-game tools engine methods: target inspection, release fetching,
//! transactional installation, update, rollback, anti-cheat guards, and cache management.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use gamenode_core::tools::{
    builtin_tools, evaluate_anticheat, find_tool, GameToolView, InstallRequest, ProxyDllName,
    ToolCatalogItemView, ToolChannel, ToolId, ToolModalView, ToolOperationStatus,
};

use super::Engine;

const RELEASE_CACHE_MAX_AGE: Duration = Duration::from_secs(3600);

impl Engine {
    /// Dispatches startup recovery of incomplete tool jobs and initializes installed tools
    /// and catalog state.
    pub fn recover_and_load_tools(&self) {
        let engine = self.clone();
        self.shared.storage.submit(move || {
            let (store, catalog, installer) = (
                engine.shared.ports.tools.clone(),
                engine.shared.ports.tool_catalog.clone(),
                engine.shared.ports.tool_installer.clone(),
            );
            let Some(store) = store else {
                return;
            };

            // 1. Recover incomplete installations
            if let Some(installer) = &installer {
                let resolve = |key: &str| -> Option<String> {
                    let state = engine.lock();
                    if let Some(entry) = state.library.entry(key) {
                        if !entry.install_dir.is_empty() {
                            return Some(entry.install_dir.clone());
                        }
                    }
                    if let Some(id) = key.strip_prefix("manual:") {
                        if let Some(reg) = state.local.registrations.iter().find(|r| r.id == id && !r.removed) {
                            return Some(reg.root.clone());
                        }
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
                let summary = descriptor.as_ref().map(|d| d.summary.clone()).unwrap_or_default();
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
                installed_by_entry.entry(inst.entry_key).or_default().push(view);
            }

            // 3. Catalog items from built-in registry
            let catalog_items = builtin_catalog_items();

            // 4. Cache statistics
            let (bytes, archives) = if let Some(catalog) = &catalog {
                catalog.cache_usage().unwrap_or((0, 0))
            } else {
                (0, 0)
            };

            let mut state = engine.lock();
            state.tools.installed_by_entry = installed_by_entry;
            state.tools.catalog = catalog_items;
            state.tools.total_cache_bytes = bytes;
            state.tools.total_cache_archives = archives;
            engine.publish(&mut state);
        });
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
                (reg.title.clone(), reg.root.clone(), Some(reg.executable.clone()))
            } else if let Some(entry) = state.library.entry(&entry_key) {
                let title = entry
                    .name
                    .clone()
                    .unwrap_or_else(|| entry.store_id.clone());
                (title, entry.install_dir.clone(), None)
            } else {
                return;
            }
        };

        if install_dir.trim().is_empty() {
            return;
        }

        let mut state = self.lock();
        let existing_tools = state
            .tools
            .installed_by_entry
            .get(&entry_key)
            .cloned()
            .unwrap_or_default();

        let active_tool_id = ToolId::new("optiscaler");
        let installed_tool = existing_tools.iter().find(|t| t.tool_id == active_tool_id);

        let channel = installed_tool
            .map(|t| t.channel)
            .unwrap_or(ToolChannel::Stable);

        let proxy = installed_tool
            .and_then(|t| t.proxy_dll.as_deref().and_then(ProxyDllName::parse_flexible))
            .unwrap_or(ProxyDllName::Dxgi);

        let cached_releases = if let Some(store) = &self.shared.ports.tools {
            store.load_releases(&active_tool_id, channel).unwrap_or_default()
        } else {
            Vec::new()
        };

        let selected_version = installed_tool
            .map(|t| t.version.clone())
            .or_else(|| cached_releases.first().map(|r| r.version.clone()));

        state.tools.modal = Some(ToolModalView {
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
        let (installer, catalog, store) = (
            self.shared.ports.tool_installer.clone(),
            self.shared.ports.tool_catalog.clone(),
            self.shared.ports.tools.clone(),
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
                        let modal_matches = state
                            .tools
                            .modal
                            .as_ref()
                            .is_some_and(|m| m.entry_key == k);
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
            if let (Some(catalog), Some(store), Some(descriptor)) = (catalog, store, descriptor) {
                let engine = self.clone();
                let k = entry_key.clone();
                let tool_id = active_tool_id.clone();
                self.shared.runtime.spawn_blocking(move || {
                    if let Ok(releases) = catalog.fetch_releases(&descriptor, channel) {
                        let _ = store.save_releases(&tool_id, channel, &releases);
                        let (bytes, archives) = catalog.cache_usage().unwrap_or((0, 0));
                        let mut state = engine.lock();
                        state.tool_last_fetch.insert((tool_id.clone(), channel), Instant::now());
                        state.tools.total_cache_bytes = bytes;
                        state.tools.total_cache_archives = archives;

                        if let Some(modal) = &mut state.tools.modal {
                            if modal.entry_key == k
                                && modal.active_tool_id == tool_id
                                && modal.selected_channel == channel
                            {
                                modal.available_versions = releases;
                                if modal.selected_version.is_none()
                                    || !modal
                                        .available_versions
                                        .iter()
                                        .any(|r| Some(&r.version) == modal.selected_version.as_ref())
                                {
                                    modal.selected_version =
                                        modal.available_versions.first().map(|r| r.version.clone());
                                }
                                engine.publish(&mut state);
                            }
                        }
                    }
                });
            }
        }
    }

    /// Closes the tools modal dialog unless an installation/uninstallation is actively progressing.
    pub fn close_tools(&self) {
        let mut state = self.lock();
        if let Some(modal) = &state.tools.modal {
            if matches!(
                modal.operation,
                Some(
                    ToolOperationStatus::Installing { .. }
                        | ToolOperationStatus::Updating { .. }
                        | ToolOperationStatus::Uninstalling
                )
            ) {
                return;
            }
        }
        state.tools.modal = None;
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
            let (catalog, store) = (
                self.shared.ports.tool_catalog.clone(),
                self.shared.ports.tools.clone(),
            );
            if let (Some(catalog), Some(store), Some(descriptor)) = (catalog, store, descriptor) {
                let engine = self.clone();
                self.shared.runtime.spawn_blocking(move || {
                    if let Ok(releases) = catalog.fetch_releases(&descriptor, channel) {
                        let _ = store.save_releases(&tool_id, channel, &releases);
                        let (bytes, archives) = catalog.cache_usage().unwrap_or((0, 0));
                        let mut state = engine.lock();
                        state.tool_last_fetch.insert((tool_id.clone(), channel), Instant::now());
                        state.tools.total_cache_bytes = bytes;
                        state.tools.total_cache_archives = archives;

                        if let Some(modal) = &mut state.tools.modal {
                            if modal.entry_key == entry_key
                                && modal.active_tool_id == tool_id
                                && modal.selected_channel == channel
                            {
                                modal.available_versions = releases;
                                if modal.selected_version.is_none()
                                    || !modal
                                        .available_versions
                                        .iter()
                                        .any(|r| Some(&r.version) == modal.selected_version.as_ref())
                                {
                                    modal.selected_version =
                                        modal.available_versions.first().map(|r| r.version.clone());
                                }
                                engine.publish(&mut state);
                            }
                        }
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
        let (catalog, store) = (
            self.shared.ports.tool_catalog.clone(),
            self.shared.ports.tools.clone(),
        );
        if let (Some(catalog), Some(store), Some(descriptor)) = (catalog, store, descriptor) {
            let engine = self.clone();
            self.shared.runtime.spawn_blocking(move || {
                if let Ok(releases) = catalog.fetch_releases(&descriptor, channel) {
                    let _ = store.save_releases(&tool_id, channel, &releases);
                    let (bytes, archives) = catalog.cache_usage().unwrap_or((0, 0));
                    let mut state = engine.lock();
                    state.tool_last_fetch.insert((tool_id.clone(), channel), Instant::now());
                    state.tools.total_cache_bytes = bytes;
                    state.tools.total_cache_archives = archives;

                    if let Some(modal) = &mut state.tools.modal {
                        if modal.entry_key == entry_key
                            && modal.active_tool_id == tool_id
                            && modal.selected_channel == channel
                        {
                            modal.available_versions = releases;
                            if modal.selected_version.is_none()
                                || !modal
                                    .available_versions
                                    .iter()
                                    .any(|r| Some(&r.version) == modal.selected_version.as_ref())
                            {
                                modal.selected_version =
                                    modal.available_versions.first().map(|r| r.version.clone());
                            }
                            engine.publish(&mut state);
                        }
                    }
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
        let (request, release, descriptor, install_dir) = {
            let mut state = self.lock();
            let (entry_key, active_tool_id, channel, selected_proxy, anticheat_confirmed, has_warning, target_exe, selected_version, install_dir) = {
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
                    });
                }
                self.publish(&mut state);
                return;
            }

            // Anti-cheat warning confirmation
            if has_warning && !anticheat_confirmed {
                if let Some(modal) = &mut state.tools.modal {
                    modal.operation = Some(ToolOperationStatus::Failed {
                        message: "Anti-cheat warning must be confirmed before installing".to_string(),
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
                });
                self.publish(&mut state);
            }
            return;
        };

        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let result = installer.install_or_update(
                &*store,
                &*catalog,
                &descriptor,
                &release,
                &request,
                &install_dir,
            );

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
                });
                self.publish(&mut state);
            }
            return;
        };

        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let result = installer.uninstall(&*store, &entry_key, tool_id.as_str(), &install_dir);
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
        self.shared.storage.submit(move || {
            if let Some(catalog) = &catalog {
                let _ = catalog.clear_cache();
            }
            if let Some(store) = &store {
                let _ = store.clear_cached_archives_metadata();
            }
            let mut state = engine.lock();
            state.tools.total_cache_bytes = 0;
            state.tools.total_cache_archives = 0;
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
        let catalog = self.shared.ports.tool_catalog.clone();
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
            let summary = descriptor.as_ref().map(|d| d.summary.clone()).unwrap_or_default();
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

        let (bytes, archives) = catalog
            .as_ref()
            .and_then(|c| c.cache_usage().ok())
            .unwrap_or((0, 0));

        let new_modal_tools = map.get(entry_key).cloned().unwrap_or_default();

        let mut state = self.lock();
        state.tools.installed_by_entry = map;
        state.tools.total_cache_bytes = bytes;
        state.tools.total_cache_archives = archives;

        if let Some(modal) = &mut state.tools.modal {
            if modal.entry_key == entry_key {
                modal.tools = new_modal_tools;

                match outcome {
                    Ok(()) => {
                        modal.operation = None;
                    }
                    Err(err_msg) => {
                        modal.operation = Some(ToolOperationStatus::Failed { message: err_msg });
                    }
                }
            }
        }
        self.publish(&mut state);
    }
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
