//! Pure deployment planning, archive layout normalization, slot conflict detection,
//! and layered tool composition.

#![allow(clippy::collapsible_if)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

use super::registry::{
    AddonMode, InstallStrategy, ProxyDllName, SwapperMode, ToolDescriptor, ToolId,
};
use super::{ToolFileRecord, ToolIniPatchRecord, ToolInstallState, ToolInstallation};

/// An entry found inside an extracted tool release archive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub relative_path: String,
    pub size_bytes: u64,
    pub is_dir: bool,
}

/// Action to perform for a planned file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileAction {
    /// Newly added file that did not exist previously.
    DeployFresh,
    /// Overwrites an existing file on disk; must back up prior file first.
    DeployOverwriting,
    /// Preserves existing file (e.g. user-edited INI); do not overwrite.
    PreserveExisting,
    /// Destination is currently overridden by a higher-layer swapper; update the shadowed backup instead of the disk file.
    UpdateShadowedBackup,
}

/// A planned file mutation in the game folder or backup ledger.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedFile {
    pub source_path: String,
    pub destination_path: String,
    pub action: FileAction,
    pub prior_owner_tool_id: Option<ToolId>,
    pub requires_backup: bool,
}

/// Complete planned deployment for a tool on a game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedDeployment {
    pub tool_id: ToolId,
    pub target_exe: String,
    pub target_dir: String,
    pub proxy_dll: Option<ProxyDllName>,
    pub files: Vec<PlannedFile>,
    pub ini_patches: Vec<ToolIniPatchRecord>,
    pub created_dirs: Vec<String>,
}

/// Errors occurring during deployment planning.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("Main DLL candidate not found in archive")]
    MainDllNotFound,
    #[error("Proxy DLL slot conflict: {conflicting_tool_id} is already installed at {proxy_dll}")]
    SlotConflict {
        conflicting_tool_id: ToolId,
        proxy_dll: ProxyDllName,
        suggested_fallbacks: Vec<ProxyDllName>,
    },
    #[error("Required host tool {host_tool_id} is not installed")]
    HostToolRequired { host_tool_id: ToolId },
    #[error("Target DLLs {target_dll_names:?} not found in game folder or host tool")]
    TargetDllNotFound { target_dll_names: Vec<String> },
    #[error("No payload DLL found in archive for swapper or addon")]
    PayloadNotFound,
    #[error("Unsupported proxy DLL {0:?} for tool")]
    UnsupportedProxy(ProxyDllName),
    #[error("Planning failed: {0}")]
    Other(String),
}

/// Returns the parent directory containing the target executable, normalized with forward slashes.
pub fn target_dir_of(target_exe: &str) -> String {
    let normalized = target_exe.replace('\\', "/");
    let trimmed = normalized.trim_matches('/');
    if let Some((dir, _)) = trimmed.rsplit_once('/') {
        dir.to_string()
    } else {
        String::new()
    }
}

/// Joins a target directory and relative file path cleanly with `/`.
pub fn join_rel_path(dir: &str, file: &str) -> String {
    let clean_dir = dir.replace('\\', "/").trim_matches('/').to_string();
    let clean_file = file.replace('\\', "/").trim_matches('/').to_string();
    if clean_dir.is_empty() {
        clean_file
    } else if clean_file.is_empty() {
        clean_dir
    } else {
        format!("{clean_dir}/{clean_file}")
    }
}

/// Strips a single common top-level directory wrapper from archive entries if all files
/// sit under one root directory (e.g. `OptiScaler-v0.7.9/...`).
pub fn strip_common_top_level_wrapper(entries: &[ArchiveEntry]) -> Vec<ArchiveEntry> {
    if entries.is_empty() {
        return Vec::new();
    }

    let mut first_components = HashSet::new();
    for entry in entries {
        let norm = entry.relative_path.replace('\\', "/");
        let clean = norm.trim_matches('/');
        if clean.is_empty() {
            continue;
        }
        if let Some((first, _)) = clean.split_once('/') {
            first_components.insert(first.to_string());
        } else if entry.is_dir {
            first_components.insert(clean.to_string());
        } else {
            // Root file exists; no single wrapper directory
            return entries.to_vec();
        }
    }

    if first_components.len() == 1 {
        let wrapper = first_components.into_iter().next().unwrap();
        let prefix = format!("{wrapper}/");
        let mut stripped = Vec::new();

        for entry in entries {
            let norm = entry.relative_path.replace('\\', "/");
            let clean = norm.trim_matches('/');
            if clean == wrapper && entry.is_dir {
                // Skip the root wrapper directory entry itself
                continue;
            }
            if let Some(sub) = clean.strip_prefix(&prefix) {
                if !sub.is_empty() {
                    stripped.push(ArchiveEntry {
                        relative_path: sub.to_string(),
                        size_bytes: entry.size_bytes,
                        is_dir: entry.is_dir,
                    });
                }
            } else {
                stripped.push(entry.clone());
            }
        }
        stripped
    } else {
        entries.to_vec()
    }
}

/// Pure deployment planner for a tool.
#[allow(clippy::too_many_arguments)]
pub fn plan_deployment(
    descriptor: &ToolDescriptor,
    raw_archive_entries: &[ArchiveEntry],
    target_exe: &str,
    selected_proxy: Option<ProxyDllName>,
    reset_config: bool,
    existing_files_in_game: &HashMap<String, String>, // path -> sha256
    installed_tools: &[ToolInstallation],
    installed_files: &[ToolFileRecord],
    installed_ini_patches: &[ToolIniPatchRecord],
) -> Result<PlannedDeployment, PlanError> {
    let target_dir = target_dir_of(target_exe);
    let stripped_entries = strip_common_top_level_wrapper(raw_archive_entries);

    // Set of paths currently installed or active on disk
    let disk_files: HashSet<String> = existing_files_in_game
        .keys()
        .map(|k| k.replace('\\', "/").trim_matches('/').to_string())
        .collect();

    // Map destination path -> tool that currently overrides it
    let mut overriding_tools: HashMap<String, ToolId> = HashMap::new();
    for rec in installed_files {
        let norm = rec
            .relative_path
            .replace('\\', "/")
            .trim_matches('/')
            .to_string();
        overriding_tools.insert(norm, rec.tool_id.clone());
    }

    match &descriptor.strategy {
        InstallStrategy::ProxyDllInjector {
            main_dll_candidates,
            supported_proxies,
            default_proxy,
            preserved_configs,
            ignored_extensions,
            ..
        } => {
            let proxy = selected_proxy.unwrap_or(*default_proxy);
            if !supported_proxies.contains(&proxy) {
                return Err(PlanError::UnsupportedProxy(proxy));
            }

            let proxy_filename = proxy.as_str();
            let proxy_destination = join_rel_path(&target_dir, proxy_filename);

            // Cross-tool slot conflict detection: check if another installed tool occupies this proxy DLL
            for installed in installed_tools {
                if installed.tool_id != descriptor.id
                    && installed.state == ToolInstallState::Installed
                    && installed.proxy_dll == Some(proxy)
                    && target_dir_of(&installed.target_exe) == target_dir
                {
                    // Find suggested available fallback proxies
                    let occupied_proxies: HashSet<ProxyDllName> = installed_tools
                        .iter()
                        .filter(|t| {
                            t.tool_id != descriptor.id
                                && t.state == ToolInstallState::Installed
                                && target_dir_of(&t.target_exe) == target_dir
                        })
                        .filter_map(|t| t.proxy_dll)
                        .collect();

                    let fallbacks: Vec<ProxyDllName> = supported_proxies
                        .iter()
                        .copied()
                        .filter(|p| !occupied_proxies.contains(p))
                        .collect();

                    return Err(PlanError::SlotConflict {
                        conflicting_tool_id: installed.tool_id.clone(),
                        proxy_dll: proxy,
                        suggested_fallbacks: fallbacks,
                    });
                }
            }

            // Locate main DLL candidate in archive
            let main_dll_entry = main_dll_candidates
                .iter()
                .find_map(|candidate| {
                    stripped_entries
                        .iter()
                        .find(|e| !e.is_dir && e.relative_path.eq_ignore_ascii_case(candidate))
                })
                .ok_or(PlanError::MainDllNotFound)?;

            let mut planned_files = Vec::new();
            let mut created_dirs = BTreeSet::new();

            // 1. Plan proxy DLL injection
            let proxy_exists = disk_files.contains(&proxy_destination);
            let proxy_overridden = overriding_tools.get(&proxy_destination);

            let proxy_action = if let Some(higher_tool) = proxy_overridden {
                if higher_tool != &descriptor.id {
                    FileAction::UpdateShadowedBackup
                } else if proxy_exists {
                    FileAction::DeployOverwriting
                } else {
                    FileAction::DeployFresh
                }
            } else if proxy_exists {
                FileAction::DeployOverwriting
            } else {
                FileAction::DeployFresh
            };

            planned_files.push(PlannedFile {
                source_path: main_dll_entry.relative_path.clone(),
                destination_path: proxy_destination.clone(),
                action: proxy_action,
                prior_owner_tool_id: None,
                requires_backup: proxy_exists && proxy_action != FileAction::UpdateShadowedBackup,
            });

            // 2. Plan companion files
            for entry in &stripped_entries {
                if entry.is_dir {
                    continue;
                }
                // Skip the main DLL entry already mapped to proxy DLL
                if entry
                    .relative_path
                    .eq_ignore_ascii_case(&main_dll_entry.relative_path)
                {
                    continue;
                }

                // Check ignored extensions (*.bat, *.cmd, *.sh, *.ps1)
                let lower_path = entry.relative_path.to_lowercase();
                if ignored_extensions
                    .iter()
                    .any(|ext| lower_path.ends_with(ext))
                {
                    continue;
                }

                let destination = join_rel_path(&target_dir, &entry.relative_path);
                let exists_on_disk = disk_files.contains(&destination);

                // Check if directory is created
                if let Some((d, _)) = destination.rsplit_once('/') {
                    if !d.is_empty() && !disk_files.iter().any(|f| f.starts_with(d)) {
                        created_dirs.insert(d.to_string());
                    }
                }

                // Check if file is a preserved config
                let file_name = entry
                    .relative_path
                    .rsplit_once('/')
                    .map(|(_, f)| f)
                    .unwrap_or(&entry.relative_path);

                let is_preserved_config = preserved_configs
                    .iter()
                    .any(|cfg| cfg.eq_ignore_ascii_case(file_name));

                let action = if is_preserved_config && exists_on_disk && !reset_config {
                    FileAction::PreserveExisting
                } else if let Some(higher_tool) = overriding_tools.get(&destination) {
                    if higher_tool != &descriptor.id {
                        FileAction::UpdateShadowedBackup
                    } else if exists_on_disk {
                        FileAction::DeployOverwriting
                    } else {
                        FileAction::DeployFresh
                    }
                } else if exists_on_disk {
                    FileAction::DeployOverwriting
                } else {
                    FileAction::DeployFresh
                };

                let requires_backup = exists_on_disk
                    && action != FileAction::PreserveExisting
                    && action != FileAction::UpdateShadowedBackup;

                planned_files.push(PlannedFile {
                    source_path: entry.relative_path.clone(),
                    destination_path: destination,
                    action,
                    prior_owner_tool_id: None,
                    requires_backup,
                });
            }

            // 3. Keep active AddonOverlay INI patches applied on OptiScaler.ini across updates
            let mut ini_patches = Vec::new();
            for patch in installed_ini_patches {
                if patch.config_path.ends_with("OptiScaler.ini") {
                    ini_patches.push(patch.clone());
                }
            }

            Ok(PlannedDeployment {
                tool_id: descriptor.id.clone(),
                target_exe: target_exe.to_string(),
                target_dir,
                proxy_dll: Some(proxy),
                files: planned_files,
                ini_patches,
                created_dirs: created_dirs.into_iter().collect(),
            })
        }

        InstallStrategy::SiblingLibrarySwapper {
            mode,
            target_dll_names,
        } => {
            // Find payload DLL from stripped archive
            let payload_entry = stripped_entries
                .iter()
                .find(|e| !e.is_dir && e.relative_path.to_lowercase().ends_with(".dll"))
                .ok_or(PlanError::PayloadNotFound)?;

            let mut planned_files = Vec::new();

            match mode {
                SwapperMode::NativeInstallDir => {
                    // Match existing target DLLs anywhere in game install dir
                    let mut matched = false;
                    for target_name in target_dll_names {
                        for existing_path in &disk_files {
                            let file_name = existing_path
                                .rsplit_once('/')
                                .map(|(_, f)| f)
                                .unwrap_or(existing_path);

                            if file_name.eq_ignore_ascii_case(target_name) {
                                matched = true;
                                planned_files.push(PlannedFile {
                                    source_path: payload_entry.relative_path.clone(),
                                    destination_path: existing_path.clone(),
                                    action: FileAction::DeployOverwriting,
                                    prior_owner_tool_id: None,
                                    requires_backup: true,
                                });
                            }
                        }
                    }

                    if !matched {
                        return Err(PlanError::TargetDllNotFound {
                            target_dll_names: target_dll_names.clone(),
                        });
                    }
                }

                SwapperMode::CompanionOrNative { host_tool_ids } => {
                    let mut matched = false;

                    // First check if a host tool is installed and has companion DLLs
                    for host_id in host_tool_ids {
                        if let Some(host_inst) = installed_tools.iter().find(|t| {
                            &t.tool_id == host_id && t.state == ToolInstallState::Installed
                        }) {
                            let host_dir = target_dir_of(&host_inst.target_exe);
                            for target_name in target_dll_names {
                                let companion_dest = join_rel_path(&host_dir, target_name);
                                let exists_in_host = installed_files.iter().any(|f| {
                                    &f.tool_id == host_id
                                        && f.relative_path.eq_ignore_ascii_case(&companion_dest)
                                }) || disk_files.contains(&companion_dest);

                                if exists_in_host {
                                    matched = true;
                                    planned_files.push(PlannedFile {
                                        source_path: payload_entry.relative_path.clone(),
                                        destination_path: companion_dest,
                                        action: FileAction::DeployOverwriting,
                                        prior_owner_tool_id: Some(host_id.clone()),
                                        requires_backup: true,
                                    });
                                }
                            }
                        }
                    }

                    // If not found in host tool, fall back to native game files
                    if !matched {
                        for target_name in target_dll_names {
                            for existing_path in &disk_files {
                                let file_name = existing_path
                                    .rsplit_once('/')
                                    .map(|(_, f)| f)
                                    .unwrap_or(existing_path);

                                if file_name.eq_ignore_ascii_case(target_name) {
                                    matched = true;
                                    planned_files.push(PlannedFile {
                                        source_path: payload_entry.relative_path.clone(),
                                        destination_path: existing_path.clone(),
                                        action: FileAction::DeployOverwriting,
                                        prior_owner_tool_id: None,
                                        requires_backup: true,
                                    });
                                }
                            }
                        }
                    }

                    if !matched {
                        return Err(PlanError::TargetDllNotFound {
                            target_dll_names: target_dll_names.clone(),
                        });
                    }
                }
            }

            Ok(PlannedDeployment {
                tool_id: descriptor.id.clone(),
                target_exe: target_exe.to_string(),
                target_dir,
                proxy_dll: None,
                files: planned_files,
                ini_patches: Vec::new(),
                created_dirs: Vec::new(),
            })
        }

        InstallStrategy::AddonOverlay { mode } => match mode {
            AddonMode::HostPlugin {
                host_tool_id,
                plugin_dir,
                extension,
                ini_patches,
            } => {
                let host_inst = installed_tools
                    .iter()
                    .find(|t| &t.tool_id == host_tool_id && t.state == ToolInstallState::Installed)
                    .ok_or_else(|| PlanError::HostToolRequired {
                        host_tool_id: host_tool_id.clone(),
                    })?;

                let host_dir = target_dir_of(&host_inst.target_exe);

                // Find payload binary (.dll or .asi)
                let payload_entry = stripped_entries
                    .iter()
                    .find(|e| {
                        if e.is_dir {
                            return false;
                        }
                        let lower = e.relative_path.to_lowercase();
                        lower.ends_with(".dll") || lower.ends_with(".asi")
                    })
                    .ok_or(PlanError::PayloadNotFound)?;

                let filename = payload_entry
                    .relative_path
                    .rsplit_once('/')
                    .map(|(_, f)| f)
                    .unwrap_or(&payload_entry.relative_path);

                let stem = filename
                    .strip_suffix(".dll")
                    .or_else(|| filename.strip_suffix(".asi"))
                    .unwrap_or(filename);

                let target_filename = format!("{stem}.{extension}");
                let plugin_folder = join_rel_path(&host_dir, plugin_dir);
                let destination = join_rel_path(&plugin_folder, &target_filename);

                let exists = disk_files.contains(&destination);
                let planned_file = PlannedFile {
                    source_path: payload_entry.relative_path.clone(),
                    destination_path: destination,
                    action: if exists {
                        FileAction::DeployOverwriting
                    } else {
                        FileAction::DeployFresh
                    },
                    prior_owner_tool_id: None,
                    requires_backup: exists,
                };

                // Prepare INI patches targeting host config file
                let mut planned_patches = Vec::new();
                for patch in ini_patches {
                    let config_path = join_rel_path(&host_dir, &patch.config_file);
                    planned_patches.push(ToolIniPatchRecord {
                        entry_key: host_inst.entry_key.clone(),
                        tool_id: descriptor.id.clone(),
                        config_path,
                        section: patch.section.clone(),
                        key: patch.key.clone(),
                        applied_value: patch.value.clone(),
                        prior_value: None, // Resolved dynamically at application time
                    });
                }

                Ok(PlannedDeployment {
                    tool_id: descriptor.id.clone(),
                    target_exe: target_exe.to_string(),
                    target_dir: host_dir,
                    proxy_dll: None,
                    files: vec![planned_file],
                    ini_patches: planned_patches,
                    created_dirs: vec![plugin_folder],
                })
            }

            AddonMode::StandaloneFallback { injector } => {
                let mut fallback_descriptor = descriptor.clone();
                fallback_descriptor.strategy = *injector.clone();
                plan_deployment(
                    &fallback_descriptor,
                    raw_archive_entries,
                    target_exe,
                    selected_proxy,
                    reset_config,
                    existing_files_in_game,
                    installed_tools,
                    installed_files,
                    installed_ini_patches,
                )
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::registry::{
        ToolChannel, fsr4_rdna2_lite_test_descriptor, mfg_unlocker_test_descriptor,
        optiscaler_descriptor,
    };

    #[test]
    fn test_target_dir_of() {
        assert_eq!(
            target_dir_of("Binaries/Win64/Game-Win64-Shipping.exe"),
            "Binaries/Win64"
        );
        assert_eq!(target_dir_of("Game.exe"), "");
        assert_eq!(target_dir_of("bin\\x64\\Cyberpunk2077.exe"), "bin/x64");
    }

    #[test]
    fn test_strip_common_top_level_wrapper() {
        let entries = vec![
            ArchiveEntry {
                relative_path: "OptiScaler-v0.7.9/OptiScaler.dll".to_string(),
                size_bytes: 1000,
                is_dir: false,
            },
            ArchiveEntry {
                relative_path: "OptiScaler-v0.7.9/OptiScaler.ini".to_string(),
                size_bytes: 500,
                is_dir: false,
            },
            ArchiveEntry {
                relative_path: "OptiScaler-v0.7.9/D3D12_OptiScaler/mod.dll".to_string(),
                size_bytes: 2000,
                is_dir: false,
            },
        ];

        let stripped = strip_common_top_level_wrapper(&entries);
        assert_eq!(stripped.len(), 3);
        assert_eq!(stripped[0].relative_path, "OptiScaler.dll");
        assert_eq!(stripped[1].relative_path, "OptiScaler.ini");
        assert_eq!(stripped[2].relative_path, "D3D12_OptiScaler/mod.dll");
    }

    #[test]
    fn test_plan_optiscaler_fresh_install() {
        let opti = optiscaler_descriptor();
        let entries = vec![
            ArchiveEntry {
                relative_path: "OptiScaler.dll".to_string(),
                size_bytes: 1024,
                is_dir: false,
            },
            ArchiveEntry {
                relative_path: "OptiScaler.ini".to_string(),
                size_bytes: 512,
                is_dir: false,
            },
            ArchiveEntry {
                relative_path: "fakenvapi.dll".to_string(),
                size_bytes: 2048,
                is_dir: false,
            },
            ArchiveEntry {
                relative_path: "OptiScaler Setup.bat".to_string(),
                size_bytes: 128,
                is_dir: false,
            },
            ArchiveEntry {
                relative_path: "D3D12_OptiScaler/helper.dll".to_string(),
                size_bytes: 4096,
                is_dir: false,
            },
        ];

        let existing = HashMap::new();
        let plan = plan_deployment(
            &opti,
            &entries,
            "Binaries/Win64/Game-Win64-Shipping.exe",
            Some(ProxyDllName::Dxgi),
            false,
            &existing,
            &[],
            &[],
            &[],
        )
        .expect("plan succeeded");

        assert_eq!(plan.target_dir, "Binaries/Win64");
        assert_eq!(plan.proxy_dll, Some(ProxyDllName::Dxgi));

        // Proxy DLL mapped to dxgi.dll
        let proxy_file = plan
            .files
            .iter()
            .find(|f| f.destination_path == "Binaries/Win64/dxgi.dll")
            .unwrap();
        assert_eq!(proxy_file.source_path, "OptiScaler.dll");
        assert_eq!(proxy_file.action, FileAction::DeployFresh);

        // Scripts filtered out
        assert!(!plan.files.iter().any(|f| f.source_path.ends_with(".bat")));

        // Companion files mapped
        assert!(
            plan.files
                .iter()
                .any(|f| f.destination_path == "Binaries/Win64/fakenvapi.dll")
        );
        assert!(
            plan.files
                .iter()
                .any(|f| f.destination_path == "Binaries/Win64/D3D12_OptiScaler/helper.dll")
        );
        assert!(
            plan.created_dirs
                .contains(&"Binaries/Win64/D3D12_OptiScaler".to_string())
        );
    }

    #[test]
    fn test_plan_optiscaler_preserves_ini_on_update() {
        let opti = optiscaler_descriptor();
        let entries = vec![
            ArchiveEntry {
                relative_path: "OptiScaler.dll".to_string(),
                size_bytes: 1024,
                is_dir: false,
            },
            ArchiveEntry {
                relative_path: "OptiScaler.ini".to_string(),
                size_bytes: 512,
                is_dir: false,
            },
        ];

        let mut existing = HashMap::new();
        existing.insert(
            "Binaries/Win64/OptiScaler.ini".to_string(),
            "hash123".to_string(),
        );

        let plan = plan_deployment(
            &opti,
            &entries,
            "Binaries/Win64/Game.exe",
            Some(ProxyDllName::Dxgi),
            false, // reset_config = false
            &existing,
            &[],
            &[],
            &[],
        )
        .expect("plan succeeded");

        let ini_file = plan
            .files
            .iter()
            .find(|f| f.destination_path == "Binaries/Win64/OptiScaler.ini")
            .unwrap();
        assert_eq!(ini_file.action, FileAction::PreserveExisting);
    }

    #[test]
    fn test_plan_slot_conflict_detected() {
        let opti = optiscaler_descriptor();
        let entries = vec![ArchiveEntry {
            relative_path: "OptiScaler.dll".to_string(),
            size_bytes: 1024,
            is_dir: false,
        }];

        let existing = HashMap::new();
        let installed = vec![ToolInstallation {
            entry_key: "steam:123".to_string(),
            tool_id: ToolId::new("aeonsr"),
            channel: ToolChannel::Stable,
            version: "1.0.0".to_string(),
            target_exe: "Binaries/Win64/Game.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            state: ToolInstallState::Installed,
            installed_at: 100,
            updated_at: 100,
        }];

        let err = plan_deployment(
            &opti,
            &entries,
            "Binaries/Win64/Game.exe",
            Some(ProxyDllName::Dxgi),
            false,
            &existing,
            &installed,
            &[],
            &[],
        )
        .expect_err("should conflict");

        match err {
            PlanError::SlotConflict {
                conflicting_tool_id,
                proxy_dll,
                suggested_fallbacks,
            } => {
                assert_eq!(conflicting_tool_id.as_str(), "aeonsr");
                assert_eq!(proxy_dll, ProxyDllName::Dxgi);
                assert!(!suggested_fallbacks.contains(&ProxyDllName::Dxgi));
                assert!(suggested_fallbacks.contains(&ProxyDllName::Winmm));
            }
            _ => panic!("Expected SlotConflict, got {:?}", err),
        }
    }

    #[test]
    fn test_plan_swapper_layering_over_optiscaler() {
        let swapper = fsr4_rdna2_lite_test_descriptor();
        let entries = vec![ArchiveEntry {
            relative_path: "amd_fidelityfx_dx12.dll".to_string(),
            size_bytes: 2048,
            is_dir: false,
        }];

        let mut existing = HashMap::new();
        existing.insert(
            "Binaries/Win64/amd_fidelityfx_dx12.dll".to_string(),
            "opti_hash".to_string(),
        );

        let installed = vec![ToolInstallation {
            entry_key: "steam:123".to_string(),
            tool_id: ToolId::new("optiscaler"),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "Binaries/Win64/Game.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            state: ToolInstallState::Installed,
            installed_at: 100,
            updated_at: 100,
        }];

        let installed_files = vec![ToolFileRecord {
            entry_key: "steam:123".to_string(),
            tool_id: ToolId::new("optiscaler"),
            relative_path: "Binaries/Win64/amd_fidelityfx_dx12.dll".to_string(),
            installed_sha256: "opti_hash".to_string(),
            backup_path: None,
            original_sha256: None,
            prior_owner_tool_id: None,
            created_dirs: vec![],
        }];

        let plan = plan_deployment(
            &swapper,
            &entries,
            "Binaries/Win64/Game.exe",
            None,
            false,
            &existing,
            &installed,
            &installed_files,
            &[],
        )
        .expect("plan succeeded");

        assert_eq!(plan.files.len(), 1);
        let file = &plan.files[0];
        assert_eq!(
            file.destination_path,
            "Binaries/Win64/amd_fidelityfx_dx12.dll"
        );
        assert_eq!(file.prior_owner_tool_id, Some(ToolId::new("optiscaler")));
        assert!(file.requires_backup);
    }

    #[test]
    fn test_plan_addon_overlay_host_plugin() {
        let addon = mfg_unlocker_test_descriptor();
        let entries = vec![ArchiveEntry {
            relative_path: "MFGUnlocker.dll".to_string(),
            size_bytes: 4096,
            is_dir: false,
        }];

        let installed = vec![ToolInstallation {
            entry_key: "steam:123".to_string(),
            tool_id: ToolId::new("optiscaler"),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "Binaries/Win64/Game.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            state: ToolInstallState::Installed,
            installed_at: 100,
            updated_at: 100,
        }];

        let plan = plan_deployment(
            &addon,
            &entries,
            "Binaries/Win64/Game.exe",
            None,
            false,
            &HashMap::new(),
            &installed,
            &[],
            &[],
        )
        .expect("plan succeeded");

        assert_eq!(plan.files.len(), 1);
        assert_eq!(
            plan.files[0].destination_path,
            "Binaries/Win64/plugins/MFGUnlocker.asi"
        );
        assert_eq!(plan.created_dirs, vec!["Binaries/Win64/plugins"]);

        assert_eq!(plan.ini_patches.len(), 1);
        assert_eq!(
            plan.ini_patches[0].config_path,
            "Binaries/Win64/OptiScaler.ini"
        );
        assert_eq!(plan.ini_patches[0].section, "Plugins");
        assert_eq!(plan.ini_patches[0].key, "LoadAsiPlugins");
        assert_eq!(plan.ini_patches[0].applied_value, "true");
    }
}
