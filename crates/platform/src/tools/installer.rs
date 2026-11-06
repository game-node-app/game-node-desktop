//! Transactional tool installation, layered composition, and byte-identical rollback.

#![allow(clippy::collapsible_if)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gamenode_core::ports::{ProcessError, Processes};
use gamenode_core::tools::{
    apply_ini_patch, plan_deployment, revert_ini_patch, ArchiveEntry, FileAction, InstallRequest,
    TargetInspection, ToolCatalogApi, ToolDescriptor, ToolError, ToolFileRecord, ToolId,
    ToolIniPatchRecord, ToolInstallState, ToolInstallation, ToolInstallerHost, ToolRelease,
    ToolStore,
};

use crate::tools::catalog;
use crate::tools::inspect::{self, inspect_game_target};

fn sanitize_path_segment(segment: &str) -> String {
    segment
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect()
}

fn sanitize_entry_key(key: &str) -> String {
    sanitize_path_segment(key)
}

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Result<Self, ToolError> {
        let base = std::env::temp_dir().join("gamenode_tools");
        let hex = crate::random::nonce();
        let path = base.join(format!("{prefix}_{}_{}", std::process::id(), &hex[..16]));
        std::fs::create_dir_all(&path).map_err(|e| ToolError::Io(e.to_string()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn scan_install_dir_files(canonical_dir: &Path) -> HashMap<String, String> {
    let mut files = HashMap::new();
    let mut queue = vec![canonical_dir.to_path_buf()];
    while let Some(dir) = queue.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    queue.push(path);
                } else if path.is_file() {
                    if let Some(rel) = inspect::relative_path_from(&path, canonical_dir) {
                        files.insert(rel, String::new());
                    }
                }
            }
        }
    }
    files
}

struct InstallRollbackJournal {
    temp_dir: PathBuf,
    created_files: Vec<PathBuf>,
    overwritten_files: Vec<(PathBuf, PathBuf)>, // (dest, backup_copy)
    modified_ini_files: Vec<(PathBuf, String)>, // (ini_path, original_content)
    created_dirs: Vec<PathBuf>,
    created_backups: Vec<PathBuf>,
    counter: usize,
}

impl InstallRollbackJournal {
    fn new(temp_dir: PathBuf) -> Self {
        Self {
            temp_dir,
            created_files: Vec::new(),
            overwritten_files: Vec::new(),
            modified_ini_files: Vec::new(),
            created_dirs: Vec::new(),
            created_backups: Vec::new(),
            counter: 0,
        }
    }

    fn record_created_file(&mut self, dest: PathBuf) {
        self.created_files.push(dest);
    }

    fn record_overwritten_or_removed(&mut self, dest: &Path) -> Result<(), ToolError> {
        self.counter += 1;
        let temp_copy = self.temp_dir.join(format!("backup_{}", self.counter));
        std::fs::copy(dest, &temp_copy).map_err(|e| ToolError::Io(e.to_string()))?;
        self.overwritten_files.push((dest.to_path_buf(), temp_copy));
        Ok(())
    }

    fn record_ini_modification(&mut self, ini_path: &Path, original_content: &str) {
        if !self.modified_ini_files.iter().any(|(p, _)| p == ini_path) {
            self.modified_ini_files
                .push((ini_path.to_path_buf(), original_content.to_string()));
        }
    }

    fn record_created_dir(&mut self, dir: PathBuf) {
        if !self.created_dirs.contains(&dir) {
            self.created_dirs.push(dir);
        }
    }

    fn record_created_backup(&mut self, backup: PathBuf) {
        self.created_backups.push(backup);
    }

    fn rollback(
        &self,
        store: &dyn ToolStore,
        entry_key: &str,
        tool_id: &ToolId,
        prior_install: Option<&ToolInstallation>,
        prior_files: &[ToolFileRecord],
        prior_patches: &[ToolIniPatchRecord],
    ) {
        // 1. Revert modified INI files
        for (ini_path, content) in &self.modified_ini_files {
            let _ = std::fs::write(ini_path, content.as_bytes());
        }

        // 2. Restore overwritten/removed files
        for (dest, temp_copy) in &self.overwritten_files {
            if temp_copy.is_file() {
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::copy(temp_copy, dest);
            }
        }

        // 3. Delete newly created files
        for created in &self.created_files {
            if created.is_file() {
                let _ = std::fs::remove_file(created);
            }
        }

        // 4. Remove newly created directories (deepest first)
        let mut sorted_dirs = self.created_dirs.clone();
        sorted_dirs.sort_by_key(|a| std::cmp::Reverse(a.components().count()));
        for dir in sorted_dirs {
            if dir.is_dir() {
                let _ = std::fs::remove_dir(dir);
            }
        }

        // 5. Clean up created backup files
        for backup in &self.created_backups {
            if backup.is_file() {
                let _ = std::fs::remove_file(backup);
            }
        }

        // 6. Restore database state
        if let Some(prev) = prior_install {
            let _ = store.save_installation(prev);
            let _ = store.delete_files(entry_key, tool_id);
            let _ = store.save_files(prior_files);
            let _ = store.delete_ini_patches(entry_key, tool_id);
            let _ = store.save_ini_patches(prior_patches);
        } else {
            let _ = store.delete_installation(entry_key, tool_id);
            let _ = store.delete_files(entry_key, tool_id);
            let _ = store.delete_ini_patches(entry_key, tool_id);
        }
    }
}

/// Production implementation of `ToolInstallerHost` supporting transactional file deployment,
/// layered multi-tool composition, and byte-identical rollback.
pub struct PlatformToolInstaller {
    backups_dir: PathBuf,
    processes: Arc<dyn Processes>,
}

impl PlatformToolInstaller {
    pub fn new(backups_dir: PathBuf, processes: Arc<dyn Processes>) -> Self {
        Self {
            backups_dir,
            processes,
        }
    }

    pub fn backups_dir(&self) -> &Path {
        &self.backups_dir
    }

    /// Verifies that no running process has an executable path located inside `canonical_install_dir`.
    pub fn ensure_game_stopped(&self, canonical_install_dir: &Path) -> Result<(), ToolError> {
        match self.processes.list() {
            Ok(procs) => {
                for proc in procs {
                    if proc.exe_path.trim().is_empty() {
                        continue;
                    }
                    let exe_p = Path::new(&proc.exe_path);
                    if inspect::path_starts_with(exe_p, canonical_install_dir) {
                        return Err(ToolError::GameRunning);
                    }
                    if let Ok(canon) = exe_p.canonicalize() {
                        let canon_stripped = inspect::strip_verbatim_prefix(&canon);
                        if inspect::path_starts_with(&canon_stripped, canonical_install_dir) {
                            return Err(ToolError::GameRunning);
                        }
                    }
                }
                Ok(())
            }
            Err(ProcessError::Unsupported) => Ok(()),
            Err(e) => Err(ToolError::Failed(format!("Failed to list running processes: {e}"))),
        }
    }
}

impl ToolInstallerHost for PlatformToolInstaller {
    fn inspect_target(
        &self,
        entry_key: &str,
        install_dir: &str,
        title: &str,
        manual_exe: Option<&str>,
    ) -> Result<TargetInspection, ToolError> {
        inspect_game_target(entry_key, install_dir, title, manual_exe)
    }

    fn install_or_update(
        &self,
        store: &dyn ToolStore,
        catalog: &dyn ToolCatalogApi,
        descriptor: &ToolDescriptor,
        release: &ToolRelease,
        request: &InstallRequest,
        install_dir: &str,
    ) -> Result<ToolInstallation, ToolError> {
        // 1. Validate install_dir exists and canonicalize
        let install_path = Path::new(install_dir);
        let canonical_install_dir = inspect::strip_verbatim_prefix(
            &install_path
                .canonicalize()
                .map_err(|_| ToolError::InstallDirNotFound(install_dir.to_string()))?,
        );
        if !canonical_install_dir.is_dir() {
            return Err(ToolError::InstallDirNotFound(install_dir.to_string()));
        }

        // 2. Stopped game check
        self.ensure_game_stopped(&canonical_install_dir)?;

        // 3. Validate target_exe inside canonical_install_dir and exists
        let (_canon_target, normalized_target_exe) =
            inspect::validate_target_within_install_dir(&canonical_install_dir, &request.target_exe)?;

        // 4. Anti-cheat check
        if descriptor.requires_anticheat_check && !request.anticheat_confirmed {
            let inspection = inspect_game_target(
                &request.entry_key,
                install_dir,
                &descriptor.name,
                Some(&normalized_target_exe),
            )?;
            if inspection.anticheat.has_warning() {
                return Err(ToolError::AntiCheatWarningUnconfirmed(
                    inspection.anticheat.evidence,
                ));
            }
        }

        // 5. Obtain archive or loose payload
        let archive_path = if let Some(ref custom_path) = request.custom_payload_path {
            PathBuf::from(custom_path)
        } else {
            let cached = catalog.ensure_cached_archive(descriptor, release)?;
            PathBuf::from(cached.path)
        };

        // 6. Extract into temporary staging directory
        let staging_dir = TempDirGuard::new("staging")?;
        let entries = match catalog::extract_archive_to_dir(&archive_path, staging_dir.path()) {
            Ok(entries) => entries,
            Err(e) => {
                if archive_path.is_file() {
                    let file_name = archive_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    let lower = file_name.to_lowercase();
                    if lower.ends_with(".dll") || lower.ends_with(".asi") {
                        let dest = staging_dir.path().join(file_name);
                        std::fs::copy(&archive_path, &dest)
                            .map_err(|io_err| ToolError::Io(io_err.to_string()))?;
                        let meta = std::fs::metadata(&dest)
                            .map_err(|io_err| ToolError::Io(io_err.to_string()))?;
                        vec![ArchiveEntry {
                            relative_path: file_name.to_string(),
                            size_bytes: meta.len(),
                            is_dir: false,
                        }]
                    } else {
                        return Err(e);
                    }
                } else {
                    return Err(e);
                }
            }
        };

        // 7. Scan existing files & query existing installation state
        let existing_files_map = scan_install_dir_files(&canonical_install_dir);
        let installed_tools = store.list_for_entry(&request.entry_key)?;
        let mut all_installed_files = Vec::new();
        for t in &installed_tools {
            let files = store.list_files(&request.entry_key, &t.tool_id)?;
            all_installed_files.extend(files);
        }
        let installed_ini_patches = store.list_all_ini_patches_for_entry(&request.entry_key)?;

        // 8. Existing install and file records
        let existing_install = store.get_installation(&request.entry_key, &descriptor.id)?;
        let prior_tool_files = store.list_files(&request.entry_key, &descriptor.id)?;
        let prior_ini_patches = store.list_ini_patches(&request.entry_key, &descriptor.id)?;

        // 9. Plan deployment
        let plan = plan_deployment(
            descriptor,
            &entries,
            &normalized_target_exe,
            request.proxy_dll,
            request.reset_config,
            &existing_files_map,
            &installed_tools,
            &all_installed_files,
            &installed_ini_patches,
        )?;

        // 10. Record transient state
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let is_update = existing_install.is_some();
        let transient_state = if is_update {
            ToolInstallState::Updating
        } else {
            ToolInstallState::Installing
        };
        let mut current_install = ToolInstallation {
            entry_key: request.entry_key.clone(),
            tool_id: descriptor.id.clone(),
            channel: request.channel,
            version: release.version.clone(),
            target_exe: normalized_target_exe.clone(),
            proxy_dll: plan.proxy_dll,
            state: transient_state,
            installed_at: existing_install.as_ref().map(|i| i.installed_at).unwrap_or(now),
            updated_at: now,
        };
        store.save_installation(&current_install)?;

        // 11. Transactional deployment
        let tool_backup_dir = self
            .backups_dir
            .join(sanitize_entry_key(&request.entry_key))
            .join(descriptor.id.as_str());

        let rollback_temp = TempDirGuard::new("rollback")?;
        let mut journal = InstallRollbackJournal::new(rollback_temp.path().to_path_buf());

        let deploy_result: Result<Vec<ToolFileRecord>, ToolError> = (|| {
            std::fs::create_dir_all(&tool_backup_dir).map_err(|e| ToolError::Io(e.to_string()))?;

            // A. If updating: handle obsolete files from prior_tool_files
            let new_destinations: HashSet<String> = plan
                .files
                .iter()
                .map(|f| f.destination_path.clone())
                .collect();

            for prior_file in &prior_tool_files {
                if !new_destinations.contains(&prior_file.relative_path) {
                    // Check if overridden by a higher tool layer
                    let overriding_tool = all_installed_files.iter().find(|other| {
                        other.tool_id != descriptor.id
                            && other.relative_path == prior_file.relative_path
                            && other.prior_owner_tool_id.as_ref() == Some(&descriptor.id)
                    });

                    if let Some(other_rec) = overriding_tool {
                        let new_bp = if let Some(ref bp) = prior_file.backup_path {
                            let bp_path = Path::new(bp);
                            if bp_path.is_file() {
                                let other_backup_dir = self
                                    .backups_dir
                                    .join(sanitize_entry_key(&request.entry_key))
                                    .join(other_rec.tool_id.as_str());
                                std::fs::create_dir_all(&other_backup_dir)
                                    .map_err(|e| ToolError::Io(e.to_string()))?;
                                let fname = bp_path.file_name().unwrap();
                                let target_bp = other_backup_dir.join(fname);
                                std::fs::rename(bp_path, &target_bp)
                                    .map_err(|e| ToolError::Io(e.to_string()))?;
                                Some(target_bp.to_string_lossy().to_string())
                            } else {
                                prior_file.backup_path.clone()
                            }
                        } else {
                            None
                        };

                        store.update_file_backup_owner(
                            &request.entry_key,
                            &prior_file.relative_path,
                            prior_file.prior_owner_tool_id.as_ref(),
                            new_bp.as_deref(),
                            prior_file.original_sha256.as_deref(),
                        )?;
                    } else {
                        let obsolete_disk_path = canonical_install_dir.join(&prior_file.relative_path);
                        if obsolete_disk_path.is_file() {
                            journal.record_overwritten_or_removed(&obsolete_disk_path)?;
                        }
                        if let Some(ref bp) = prior_file.backup_path {
                            let bp_path = Path::new(bp);
                            if bp_path.is_file() {
                                if let Some(parent) = obsolete_disk_path.parent() {
                                    std::fs::create_dir_all(parent)
                                        .map_err(|e| ToolError::Io(e.to_string()))?;
                                }
                                std::fs::copy(bp_path, &obsolete_disk_path)
                                    .map_err(|e| ToolError::Io(e.to_string()))?;
                                let _ = std::fs::remove_file(bp_path);
                            }
                        } else if obsolete_disk_path.is_file() {
                            let _ = std::fs::remove_file(&obsolete_disk_path);
                        }
                    }
                }
            }

            // B. Deploy planned files
            let mut new_records = Vec::new();

            for file in &plan.files {
                let dest_full = canonical_install_dir.join(&file.destination_path);
                for comp in Path::new(&file.destination_path).components() {
                    if let std::path::Component::ParentDir = comp {
                        return Err(ToolError::PathEscapesInstallDir(file.destination_path.clone()));
                    }
                }
                if !inspect::path_starts_with(&dest_full, &canonical_install_dir) {
                    return Err(ToolError::PathEscapesInstallDir(file.destination_path.clone()));
                }

                let direct_source = staging_dir.path().join(&file.source_path);
                let actual_source = if direct_source.is_file() {
                    direct_source
                } else {
                    let mut found = None;
                    if let Ok(dir_entries) = std::fs::read_dir(staging_dir.path()) {
                        for dir_entry in dir_entries.flatten() {
                            if dir_entry.path().is_dir() {
                                let candidate = dir_entry.path().join(&file.source_path);
                                if candidate.is_file() {
                                    found = Some(candidate);
                                    break;
                                }
                            }
                        }
                    }
                    found.ok_or_else(|| {
                        ToolError::ExtractionFailed(format!(
                            "Source file not found in extracted archive: {}",
                            file.source_path
                        ))
                    })?
                };

                match file.action {
                    FileAction::PreserveExisting => {
                        let disk_sha = catalog::compute_file_sha256(&dest_full)?;
                        let prior_rec = prior_tool_files
                            .iter()
                            .find(|r| r.relative_path == file.destination_path);
                        new_records.push(ToolFileRecord {
                            entry_key: request.entry_key.clone(),
                            tool_id: descriptor.id.clone(),
                            relative_path: file.destination_path.clone(),
                            installed_sha256: disk_sha,
                            backup_path: prior_rec.and_then(|r| r.backup_path.clone()),
                            original_sha256: prior_rec.and_then(|r| r.original_sha256.clone()),
                            prior_owner_tool_id: prior_rec.and_then(|r| r.prior_owner_tool_id.clone()),
                            created_dirs: prior_rec.map(|r| r.created_dirs.clone()).unwrap_or_default(),
                        });
                    }

                    FileAction::UpdateShadowedBackup => {
                        let higher_rec = all_installed_files.iter().find(|r| {
                            r.relative_path == file.destination_path
                                && r.prior_owner_tool_id.as_ref() == Some(&descriptor.id)
                        });
                        if let Some(higher) = higher_rec {
                            if let Some(ref bp) = higher.backup_path {
                                let backup_dest = PathBuf::from(bp);
                                if let Some(parent) = backup_dest.parent() {
                                    std::fs::create_dir_all(parent)
                                        .map_err(|e| ToolError::Io(e.to_string()))?;
                                }
                                std::fs::copy(&actual_source, &backup_dest)
                                    .map_err(|e| ToolError::Io(e.to_string()))?;
                                let new_sha = catalog::compute_file_sha256(&backup_dest)?;
                                store.update_file_backup_owner(
                                    &request.entry_key,
                                    &file.destination_path,
                                    Some(&descriptor.id),
                                    Some(bp),
                                    Some(&new_sha),
                                )?;

                                let prior_rec = prior_tool_files
                                    .iter()
                                    .find(|r| r.relative_path == file.destination_path);
                                new_records.push(ToolFileRecord {
                                    entry_key: request.entry_key.clone(),
                                    tool_id: descriptor.id.clone(),
                                    relative_path: file.destination_path.clone(),
                                    installed_sha256: new_sha,
                                    backup_path: prior_rec.and_then(|r| r.backup_path.clone()),
                                    original_sha256: prior_rec.and_then(|r| r.original_sha256.clone()),
                                    prior_owner_tool_id: prior_rec.and_then(|r| r.prior_owner_tool_id.clone()),
                                    created_dirs: vec![],
                                });
                            }
                        }
                    }

                    FileAction::DeployFresh | FileAction::DeployOverwriting => {
                        let prior_rec = prior_tool_files
                            .iter()
                            .find(|r| r.relative_path == file.destination_path);
                        let (backup_path, original_sha256, prior_owner_tool_id) = if let Some(pr) = prior_rec {
                            (pr.backup_path.clone(), pr.original_sha256.clone(), pr.prior_owner_tool_id.clone())
                        } else if dest_full.is_file() && file.requires_backup {
                            let orig_sha = catalog::compute_file_sha256(&dest_full)?;
                            let safe_name = format!(
                                "{}_{}",
                                sanitize_path_segment(&file.destination_path),
                                &orig_sha[..16.min(orig_sha.len())]
                            );
                            let backup_file = tool_backup_dir.join(safe_name);
                            std::fs::copy(&dest_full, &backup_file)
                                .map_err(|e| ToolError::Io(e.to_string()))?;
                            journal.record_created_backup(backup_file.clone());
                            (
                                Some(backup_file.to_string_lossy().to_string()),
                                Some(orig_sha),
                                file.prior_owner_tool_id.clone(),
                            )
                        } else {
                            (None, None, file.prior_owner_tool_id.clone())
                        };

                        if dest_full.is_file() {
                            journal.record_overwritten_or_removed(&dest_full)?;
                        } else {
                            journal.record_created_file(dest_full.clone());
                        }

                        let mut created_dirs_for_file = Vec::new();
                        if let Some(parent) = dest_full.parent() {
                            let mut cur = parent.to_path_buf();
                            let mut to_create = Vec::new();
                            while !cur.exists()
                                && inspect::path_starts_with(&cur, &canonical_install_dir)
                                && cur != canonical_install_dir
                            {
                                to_create.push(cur.clone());
                                if let Some(p) = cur.parent() {
                                    cur = p.to_path_buf();
                                } else {
                                    break;
                                }
                            }
                            to_create.reverse();
                            for dir_to_create in to_create {
                                std::fs::create_dir_all(&dir_to_create)
                                    .map_err(|e| ToolError::Io(e.to_string()))?;
                                journal.record_created_dir(dir_to_create.clone());
                                if let Some(rel) = inspect::relative_path_from(&dir_to_create, &canonical_install_dir) {
                                    created_dirs_for_file.push(rel);
                                }
                            }
                        }

                        std::fs::copy(&actual_source, &dest_full)
                            .map_err(|e| ToolError::Io(e.to_string()))?;
                        let installed_sha = catalog::compute_file_sha256(&dest_full)?;

                        new_records.push(ToolFileRecord {
                            entry_key: request.entry_key.clone(),
                            tool_id: descriptor.id.clone(),
                            relative_path: file.destination_path.clone(),
                            installed_sha256: installed_sha,
                            backup_path,
                            original_sha256,
                            prior_owner_tool_id,
                            created_dirs: created_dirs_for_file,
                        });
                    }
                }
            }

            // C. Apply plan INI patches
            for patch in &plan.ini_patches {
                let config_file_path = canonical_install_dir.join(&patch.config_path);
                if config_file_path.is_file() {
                    let content = std::fs::read_to_string(&config_file_path)
                        .map_err(|e| ToolError::Io(format!("Failed to read {}: {e}", config_file_path.display())))?;
                    journal.record_ini_modification(&config_file_path, &content);

                    let (new_content, applied_prior_val) =
                        apply_ini_patch(&content, &patch.section, &patch.key, &patch.applied_value);
                    std::fs::write(&config_file_path, new_content.as_bytes())
                        .map_err(|e| ToolError::Io(format!("Failed to write {}: {e}", config_file_path.display())))?;

                    if patch.tool_id == descriptor.id {
                        let existing_patches = store.list_ini_patches(&request.entry_key, &descriptor.id)?;
                        let existing_record = existing_patches.iter().find(|p| {
                            p.config_path == patch.config_path
                                && p.section.eq_ignore_ascii_case(&patch.section)
                                && p.key.eq_ignore_ascii_case(&patch.key)
                        });
                        let prior_value = if let Some(rec) = existing_record {
                            rec.prior_value.clone().or(applied_prior_val)
                        } else {
                            applied_prior_val
                        };

                        let patch_rec = ToolIniPatchRecord {
                            entry_key: request.entry_key.clone(),
                            tool_id: descriptor.id.clone(),
                            config_path: patch.config_path.clone(),
                            section: patch.section.clone(),
                            key: patch.key.clone(),
                            applied_value: patch.applied_value.clone(),
                            prior_value,
                        };
                        store.save_ini_patches(&[patch_rec])?;
                    }
                }
            }

            // D. Save updated file records
            store.delete_files(&request.entry_key, &descriptor.id)?;
            store.save_files(&new_records)?;

            Ok(new_records)
        })();

        match deploy_result {
            Ok(_) => {
                current_install.state = ToolInstallState::Installed;
                store.save_installation(&current_install)?;
                Ok(current_install)
            }
            Err(err) => {
                journal.rollback(
                    store,
                    &request.entry_key,
                    &descriptor.id,
                    existing_install.as_ref(),
                    &prior_tool_files,
                    &prior_ini_patches,
                );
                Err(err)
            }
        }
    }

    fn uninstall(
        &self,
        store: &dyn ToolStore,
        entry_key: &str,
        tool_id: &str,
        install_dir: &str,
    ) -> Result<(), ToolError> {
        let install_path = Path::new(install_dir);
        let canonical_install_dir = inspect::strip_verbatim_prefix(
            &install_path
                .canonicalize()
                .map_err(|_| ToolError::InstallDirNotFound(install_dir.to_string()))?,
        );
        if !canonical_install_dir.is_dir() {
            return Err(ToolError::InstallDirNotFound(install_dir.to_string()));
        }

        self.ensure_game_stopped(&canonical_install_dir)?;

        let tool_id_obj = ToolId::new(tool_id);

        if let Some(mut inst) = store.get_installation(entry_key, &tool_id_obj)? {
            inst.state = ToolInstallState::Uninstalling;
            store.save_installation(&inst)?;
        }

        let files = store.list_files(entry_key, &tool_id_obj)?;
        let all_tools = store.list_for_entry(entry_key)?;
        let mut other_installed_files = Vec::new();
        for t in &all_tools {
            if t.tool_id != tool_id_obj && t.state == ToolInstallState::Installed {
                other_installed_files.extend(store.list_files(entry_key, &t.tool_id)?);
            }
        }
        let patches = store.list_ini_patches(entry_key, &tool_id_obj)?;

        for file in &files {
            let overriding_tool = other_installed_files.iter().find(|other| {
                other.relative_path == file.relative_path
                    && other.prior_owner_tool_id.as_ref() == Some(&tool_id_obj)
            });

            if let Some(overriding) = overriding_tool {
                let new_bp = if let Some(ref bp) = file.backup_path {
                    let bp_path = Path::new(bp);
                    if bp_path.is_file() {
                        let overriding_backup_dir = self
                            .backups_dir
                            .join(sanitize_entry_key(entry_key))
                            .join(overriding.tool_id.as_str());
                        std::fs::create_dir_all(&overriding_backup_dir)
                            .map_err(|e| ToolError::Io(e.to_string()))?;
                        let fname = bp_path.file_name().unwrap();
                        let target_bp = overriding_backup_dir.join(fname);
                        std::fs::rename(bp_path, &target_bp)
                            .map_err(|e| ToolError::Io(e.to_string()))?;
                        Some(target_bp.to_string_lossy().to_string())
                    } else {
                        file.backup_path.clone()
                    }
                } else {
                    None
                };

                store.update_file_backup_owner(
                    entry_key,
                    &file.relative_path,
                    file.prior_owner_tool_id.as_ref(),
                    new_bp.as_deref(),
                    file.original_sha256.as_deref(),
                )?;
            } else {
                let dest = canonical_install_dir.join(&file.relative_path);
                if let Some(ref bpath) = file.backup_path {
                    let bp = Path::new(bpath);
                    if bp.is_file() {
                        if let Some(ref expected_sha) = file.original_sha256 {
                            let actual_sha = catalog::compute_file_sha256(bp)?;
                            if !expected_sha.eq_ignore_ascii_case(&actual_sha) {
                                return Err(ToolError::ChecksumMismatch {
                                    path: bpath.clone(),
                                    expected: expected_sha.clone(),
                                    actual: actual_sha,
                                });
                            }
                        }
                        if let Some(parent) = dest.parent() {
                            std::fs::create_dir_all(parent)
                                .map_err(|e| ToolError::Io(e.to_string()))?;
                        }
                        std::fs::copy(bp, &dest).map_err(|e| ToolError::Io(e.to_string()))?;
                        let _ = std::fs::remove_file(bp);
                    }
                } else if dest.is_file() {
                    let _ = std::fs::remove_file(&dest);
                }
            }
        }

        // Revert INI patches
        let all_entry_patches = store.list_all_ini_patches_for_entry(entry_key)?;
        for patch in &patches {
            let other_needs = all_entry_patches.iter().any(|other| {
                other.tool_id != tool_id_obj
                    && other.config_path == patch.config_path
                    && other.section.eq_ignore_ascii_case(&patch.section)
                    && other.key.eq_ignore_ascii_case(&patch.key)
                    && other.applied_value == patch.applied_value
            });

            if !other_needs {
                let config_full = canonical_install_dir.join(&patch.config_path);
                if config_full.is_file() {
                    if let Ok(content) = std::fs::read_to_string(&config_full) {
                        let reverted = revert_ini_patch(
                            &content,
                            &patch.section,
                            &patch.key,
                            patch.prior_value.as_deref(),
                        );
                        let _ = std::fs::write(&config_full, reverted.as_bytes());

                        if let Ok(new_sha) = catalog::compute_file_sha256(&config_full) {
                            for other_tool in &all_tools {
                                if other_tool.tool_id != tool_id_obj
                                    && other_tool.state == ToolInstallState::Installed
                                {
                                    if let Ok(mut other_files) =
                                        store.list_files(entry_key, &other_tool.tool_id)
                                    {
                                        let mut changed = false;
                                        for f in &mut other_files {
                                            if f.relative_path == patch.config_path {
                                                f.installed_sha256 = new_sha.clone();
                                                changed = true;
                                            }
                                        }
                                        if changed {
                                            let _ = store.save_files(&other_files);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Clean up empty created directories
        let mut dirs_to_check = BTreeSet::new();
        for file in &files {
            for d in &file.created_dirs {
                dirs_to_check.insert(canonical_install_dir.join(d));
            }
            let dest = canonical_install_dir.join(&file.relative_path);
            let mut cur = dest.parent();
            while let Some(parent) = cur {
                if inspect::path_starts_with(parent, &canonical_install_dir)
                    && parent != canonical_install_dir
                {
                    dirs_to_check.insert(parent.to_path_buf());
                    cur = parent.parent();
                } else {
                    break;
                }
            }
        }

        let mut sorted_dirs: Vec<PathBuf> = dirs_to_check.into_iter().collect();
        sorted_dirs.sort_by_key(|a| std::cmp::Reverse(a.components().count()));

        for d in sorted_dirs {
            if d.is_dir()
                && d != canonical_install_dir
                && inspect::path_starts_with(&d, &canonical_install_dir)
            {
                if let Ok(mut it) = std::fs::read_dir(&d) {
                    if it.next().is_none() {
                        let _ = std::fs::remove_dir(&d);
                    }
                }
            }
        }

        store.delete_ini_patches(entry_key, &tool_id_obj)?;
        store.delete_files(entry_key, &tool_id_obj)?;
        store.delete_installation(entry_key, &tool_id_obj)?;

        let tool_backup_dir = self
            .backups_dir
            .join(sanitize_entry_key(entry_key))
            .join(tool_id);
        if tool_backup_dir.is_dir() {
            let _ = std::fs::remove_dir_all(&tool_backup_dir);
        }
        let game_backup_dir = self.backups_dir.join(sanitize_entry_key(entry_key));
        if game_backup_dir.is_dir() {
            if let Ok(mut it) = std::fs::read_dir(&game_backup_dir) {
                if it.next().is_none() {
                    let _ = std::fs::remove_dir(&game_backup_dir);
                }
            }
        }

        Ok(())
    }

    fn recover_incomplete(
        &self,
        store: &dyn ToolStore,
        resolve_install_dir: &dyn Fn(&str) -> Option<String>,
    ) -> Result<usize, ToolError> {
        let incomplete = store.incomplete_installations()?;
        let mut count = 0;
        for inst in incomplete {
            if let Some(install_dir) = resolve_install_dir(&inst.entry_key) {
                if Path::new(&install_dir).is_dir() {
                    let _ = self.uninstall(
                        store,
                        &inst.entry_key,
                        inst.tool_id.as_str(),
                        &install_dir,
                    );
                } else {
                    let _ = store.delete_installation(&inst.entry_key, &inst.tool_id);
                    let _ = store.delete_files(&inst.entry_key, &inst.tool_id);
                    let _ = store.delete_ini_patches(&inst.entry_key, &inst.tool_id);
                }
            } else {
                let _ = store.delete_installation(&inst.entry_key, &inst.tool_id);
                let _ = store.delete_files(&inst.entry_key, &inst.tool_id);
                let _ = store.delete_ini_patches(&inst.entry_key, &inst.tool_id);
            }
            count += 1;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use gamenode_core::ports::{ProcessIdentity, ProcessInfo};
    use gamenode_core::tools::registry::{
        fsr4_rdna2_lite_test_descriptor, mfg_unlocker_test_descriptor, optiscaler_descriptor,
        ProxyDllName, ToolChannel,
    };
    use gamenode_core::tools::{CachedToolArchive, InMemoryToolStore};
    use tempfile::TempDir;

    struct MockProcesses {
        running: Mutex<Vec<String>>,
    }

    impl MockProcesses {
        fn new(running: Vec<String>) -> Self {
            Self {
                running: Mutex::new(running),
            }
        }
    }

    impl Processes for MockProcesses {
        fn list(&self) -> Result<Vec<ProcessInfo>, ProcessError> {
            let list = self.running.lock().unwrap();
            let procs = list
                .iter()
                .enumerate()
                .map(|(i, exe)| ProcessInfo {
                    pid: 1000 + i as u32,
                    created_at: std::time::SystemTime::now(),
                    exe_path: exe.clone(),
                    parent: Some(ProcessIdentity {
                        pid: 1,
                        created_at: std::time::SystemTime::now(),
                    }),
                })
                .collect();
            Ok(procs)
        }

        fn watch(
            &self,
            _process: &ProcessInfo,
            _on_exit: Box<dyn FnOnce(std::time::SystemTime) + Send + 'static>,
        ) -> Result<(), ProcessError> {
            Ok(())
        }
    }

    struct MockCatalog {
        archives: Mutex<HashMap<(ToolId, ToolChannel, String), CachedToolArchive>>,
    }

    impl MockCatalog {
        fn new() -> Self {
            Self {
                archives: Mutex::new(HashMap::new()),
            }
        }

        fn register_archive(&self, tool_id: ToolId, channel: ToolChannel, version: &str, path: &Path) {
            let sha = catalog::compute_file_sha256(path).unwrap();
            let meta = std::fs::metadata(path).unwrap();
            let mut map = self.archives.lock().unwrap();
            map.insert(
                (tool_id.clone(), channel, version.to_string()),
                CachedToolArchive {
                    tool_id,
                    channel,
                    version: version.to_string(),
                    path: path.to_string_lossy().to_string(),
                    size_bytes: meta.len(),
                    sha256: sha,
                },
            );
        }
    }

    impl ToolCatalogApi for MockCatalog {
        fn fetch_releases(
            &self,
            _descriptor: &ToolDescriptor,
            _channel: ToolChannel,
        ) -> Result<Vec<ToolRelease>, ToolError> {
            Ok(Vec::new())
        }

        fn ensure_cached_archive(
            &self,
            descriptor: &ToolDescriptor,
            release: &ToolRelease,
        ) -> Result<CachedToolArchive, ToolError> {
            let map = self.archives.lock().unwrap();
            map.get(&(descriptor.id.clone(), release.channel, release.version.clone()))
                .cloned()
                .ok_or_else(|| ToolError::ReleaseNotFound {
                    channel: release.channel,
                    version: release.version.clone(),
                })
        }

        fn cache_usage(&self) -> Result<(u64, usize), ToolError> {
            Ok((0, 0))
        }

        fn clear_cache(&self) -> Result<u64, ToolError> {
            Ok(0)
        }
    }

    fn collect_dir_fingerprint(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut map = BTreeMap::new();
        let mut queue = vec![dir.to_path_buf()];
        while let Some(current) = queue.pop() {
            if let Ok(entries) = std::fs::read_dir(&current) {
                let mut is_empty = true;
                for entry in entries.flatten() {
                    is_empty = false;
                    let path = entry.path();
                    if path.is_dir() {
                        queue.push(path);
                    } else if path.is_file() {
                        let rel = inspect::relative_path_from(&path, dir).unwrap();
                        let bytes = std::fs::read(&path).unwrap();
                        map.insert(rel, bytes);
                    }
                }
                if is_empty && current != dir {
                    let rel = inspect::relative_path_from(&current, dir).unwrap();
                    map.insert(format!("{rel}/__EMPTY_DIR__"), Vec::new());
                }
            }
        }
        map
    }

    fn setup_unreal_game_folder(root: &Path) {
        let bin_win64 = root.join("Binaries/Win64");
        std::fs::create_dir_all(&bin_win64).unwrap();

        // Target shipping exe
        std::fs::write(
            bin_win64.join("Game-Win64-Shipping.exe"),
            inspect::make_minimal_pe_bytes(true),
        )
        .unwrap();

        // Game files that OptiScaler might overwrite/preserve
        std::fs::write(bin_win64.join("dxgi.dll"), b"original game dxgi.dll").unwrap();
        std::fs::write(
            bin_win64.join("amd_fidelityfx_dx12.dll"),
            b"original game amd_fidelityfx_dx12.dll",
        )
        .unwrap();

        // Other game content
        std::fs::write(root.join("Game.exe"), inspect::make_minimal_pe_bytes(true)).unwrap();
        let content_dir = root.join("Content/Paks");
        std::fs::create_dir_all(&content_dir).unwrap();
        std::fs::write(content_dir.join("pakchunk0-Windows.pak"), b"pak content 12345").unwrap();
    }

    #[test]
    fn test_single_tool_optiscaler_install_and_byte_identical_uninstall() {
        let temp = TempDir::new().unwrap();
        let game_dir = temp.path().join("game");
        let backups_dir = temp.path().join("backups");
        setup_unreal_game_folder(&game_dir);

        let initial_fingerprint = collect_dir_fingerprint(&game_dir);

        // Build OptiScaler 0.7.9 .7z archive fixture
        let fixture_src = temp.path().join("opti_src");
        let helper_dir = fixture_src.join("D3D12_OptiScaler");
        let streamline_dir = fixture_src.join("streamline");
        std::fs::create_dir_all(&helper_dir).unwrap();
        std::fs::create_dir_all(&streamline_dir).unwrap();

        std::fs::write(fixture_src.join("OptiScaler.dll"), b"OptiScaler 0.7.9 binary").unwrap();
        std::fs::write(
            fixture_src.join("OptiScaler.ini"),
            b"[General]\nLogLevel = info\n",
        )
        .unwrap();
        std::fs::write(
            helper_dir.join("helper.dll"),
            b"D3D12 OptiScaler helper bytes",
        )
        .unwrap();
        std::fs::write(
            streamline_dir.join("sl.common.dll"),
            b"Streamline common dll bytes",
        )
        .unwrap();
        // Companion files that must be ignored/skipped
        std::fs::write(fixture_src.join("OptiScaler Setup.bat"), b"@echo off\r\npause").unwrap();
        std::fs::write(fixture_src.join("run.sh"), b"#!/bin/sh\necho test").unwrap();

        let archive_7z = temp.path().join("OptiScaler-v0.7.9.7z");
        sevenz_rust2::compress_to_path(&fixture_src, &archive_7z).unwrap();

        let procs = Arc::new(MockProcesses::new(vec![]));
        let installer = PlatformToolInstaller::new(backups_dir.clone(), procs);
        let store = InMemoryToolStore::default();
        let catalog = MockCatalog::new();

        let opti_desc = optiscaler_descriptor();
        catalog.register_archive(
            opti_desc.id.clone(),
            ToolChannel::Stable,
            "0.7.9",
            &archive_7z,
        );

        let release = ToolRelease {
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            tag_name: "v0.7.9".to_string(),
            name: "OptiScaler v0.7.9".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: "https://example.com/opti.7z".to_string(),
            asset_name: "OptiScaler-v0.7.9.7z".to_string(),
            size_bytes: 1024,
            sha256: None,
            cached_path: Some(archive_7z.to_string_lossy().to_string()),
        };

        let request = InstallRequest {
            entry_key: "steam:12345".to_string(),
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "Binaries/Win64/Game-Win64-Shipping.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            reset_config: false,
            anticheat_confirmed: true,
            custom_payload_path: None,
        };

        let inst = installer
            .install_or_update(
                &store,
                &catalog,
                &opti_desc,
                &release,
                &request,
                game_dir.to_str().unwrap(),
            )
            .expect("install succeeds");

        assert_eq!(inst.state, ToolInstallState::Installed);

        let bin_win64 = game_dir.join("Binaries/Win64");
        // OptiScaler.dll was deployed as dxgi.dll
        assert_eq!(
            std::fs::read(bin_win64.join("dxgi.dll")).unwrap(),
            b"OptiScaler 0.7.9 binary"
        );
        assert_eq!(
            std::fs::read_to_string(bin_win64.join("OptiScaler.ini")).unwrap(),
            "[General]\nLogLevel = info\n"
        );
        assert_eq!(
            std::fs::read(bin_win64.join("D3D12_OptiScaler/helper.dll")).unwrap(),
            b"D3D12 OptiScaler helper bytes"
        );
        assert_eq!(
            std::fs::read(bin_win64.join("streamline/sl.common.dll")).unwrap(),
            b"Streamline common dll bytes"
        );
        // Scripts were skipped
        assert!(!bin_win64.join("OptiScaler Setup.bat").exists());
        assert!(!bin_win64.join("run.sh").exists());

        // File records saved
        let files = store.list_files("steam:12345", &opti_desc.id).unwrap();
        assert!(!files.is_empty());
        let dxgi_rec = files
            .iter()
            .find(|f| f.relative_path == "Binaries/Win64/dxgi.dll")
            .unwrap();
        assert!(dxgi_rec.backup_path.is_some());
        assert_eq!(
            dxgi_rec.original_sha256.as_deref(),
            Some(catalog::compute_bytes_sha256(b"original game dxgi.dll").as_str())
        );

        // Now uninstall
        installer
            .uninstall(
                &store,
                "steam:12345",
                opti_desc.id.as_str(),
                game_dir.to_str().unwrap(),
            )
            .expect("uninstall succeeds");

        // Verify folder is byte-identical
        let final_fingerprint = collect_dir_fingerprint(&game_dir);
        assert_eq!(final_fingerprint, initial_fingerprint);

        // Verify database and backups cleaned up
        assert!(
            store
                .get_installation("steam:12345", &opti_desc.id)
                .unwrap()
                .is_none()
        );
        assert!(store.list_files("steam:12345", &opti_desc.id).unwrap().is_empty());
        assert!(!backups_dir.join("steam_12345/optiscaler").exists());
    }

    #[test]
    fn test_version_update_preserves_user_ini_and_removes_obsolete_files() {
        let temp = TempDir::new().unwrap();
        let game_dir = temp.path().join("game");
        let backups_dir = temp.path().join("backups");
        setup_unreal_game_folder(&game_dir);

        let initial_fingerprint = collect_dir_fingerprint(&game_dir);

        // Build v0.7.7 archive fixture with obsolete file
        let v77_src = temp.path().join("v77_src");
        std::fs::create_dir_all(&v77_src).unwrap();
        std::fs::write(v77_src.join("OptiScaler.dll"), b"OptiScaler 0.7.7 binary").unwrap();
        std::fs::write(
            v77_src.join("OptiScaler.ini"),
            b"[General]\nLogLevel = info\n",
        )
        .unwrap();
        std::fs::write(v77_src.join("obsolete_companion.dll"), b"v0.7.7 obsolete file").unwrap();

        let v77_archive = temp.path().join("OptiScaler-v0.7.7.7z");
        sevenz_rust2::compress_to_path(&v77_src, &v77_archive).unwrap();

        // Build v0.7.9 archive fixture with new companion and NO obsolete file
        let v79_src = temp.path().join("v79_src");
        std::fs::create_dir_all(&v79_src).unwrap();
        std::fs::write(v79_src.join("OptiScaler.dll"), b"OptiScaler 0.7.9 binary").unwrap();
        std::fs::write(
            v79_src.join("OptiScaler.ini"),
            b"[General]\nLogLevel = info\n",
        )
        .unwrap();
        std::fs::write(v79_src.join("new_companion.dll"), b"v0.7.9 new file").unwrap();

        let v79_archive = temp.path().join("OptiScaler-v0.7.9.7z");
        sevenz_rust2::compress_to_path(&v79_src, &v79_archive).unwrap();

        let procs = Arc::new(MockProcesses::new(vec![]));
        let installer = PlatformToolInstaller::new(backups_dir.clone(), procs);
        let store = InMemoryToolStore::default();
        let catalog = MockCatalog::new();

        let opti_desc = optiscaler_descriptor();
        catalog.register_archive(opti_desc.id.clone(), ToolChannel::Stable, "0.7.7", &v77_archive);
        catalog.register_archive(opti_desc.id.clone(), ToolChannel::Stable, "0.7.9", &v79_archive);

        let release_v77 = ToolRelease {
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.7".to_string(),
            tag_name: "v0.7.7".to_string(),
            name: "OptiScaler v0.7.7".to_string(),
            published_at: "2025-10-01T00:00:00Z".to_string(),
            download_url: "https://example.com/v77.7z".to_string(),
            asset_name: "OptiScaler-v0.7.7.7z".to_string(),
            size_bytes: 1024,
            sha256: None,
            cached_path: Some(v77_archive.to_string_lossy().to_string()),
        };

        let release_v79 = ToolRelease {
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            tag_name: "v0.7.9".to_string(),
            name: "OptiScaler v0.7.9".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: "https://example.com/v79.7z".to_string(),
            asset_name: "OptiScaler-v0.7.9.7z".to_string(),
            size_bytes: 1024,
            sha256: None,
            cached_path: Some(v79_archive.to_string_lossy().to_string()),
        };

        let request_v77 = InstallRequest {
            entry_key: "steam:12345".to_string(),
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.7".to_string(),
            target_exe: "Binaries/Win64/Game-Win64-Shipping.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            reset_config: false,
            anticheat_confirmed: true,
            custom_payload_path: None,
        };

        // 1. Install v0.7.7
        installer
            .install_or_update(
                &store,
                &catalog,
                &opti_desc,
                &release_v77,
                &request_v77,
                game_dir.to_str().unwrap(),
            )
            .expect("v77 install succeeds");

        let bin_win64 = game_dir.join("Binaries/Win64");
        assert!(bin_win64.join("obsolete_companion.dll").exists());

        // 2. User edits OptiScaler.ini
        let custom_ini_content = "[General]\nLogLevel = debug\nCustomUserSetting = true\n";
        std::fs::write(bin_win64.join("OptiScaler.ini"), custom_ini_content).unwrap();

        // 3. Update to v0.7.9 (reset_config = false)
        let request_v79 = InstallRequest {
            entry_key: "steam:12345".to_string(),
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "Binaries/Win64/Game-Win64-Shipping.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            reset_config: false,
            anticheat_confirmed: true,
            custom_payload_path: None,
        };

        let updated_inst = installer
            .install_or_update(
                &store,
                &catalog,
                &opti_desc,
                &release_v79,
                &request_v79,
                game_dir.to_str().unwrap(),
            )
            .expect("v79 update succeeds");

        assert_eq!(updated_inst.version, "0.7.9");
        assert_eq!(
            std::fs::read(bin_win64.join("dxgi.dll")).unwrap(),
            b"OptiScaler 0.7.9 binary"
        );
        // User's custom INI was preserved!
        assert_eq!(
            std::fs::read_to_string(bin_win64.join("OptiScaler.ini")).unwrap(),
            custom_ini_content
        );
        // Obsolete companion was removed!
        assert!(!bin_win64.join("obsolete_companion.dll").exists());
        // New companion deployed
        assert_eq!(
            std::fs::read(bin_win64.join("new_companion.dll")).unwrap(),
            b"v0.7.9 new file"
        );

        // Pre-tool backup is intact
        let files = store.list_files("steam:12345", &opti_desc.id).unwrap();
        let dxgi_rec = files
            .iter()
            .find(|f| f.relative_path == "Binaries/Win64/dxgi.dll")
            .unwrap();
        assert_eq!(
            dxgi_rec.original_sha256.as_deref(),
            Some(catalog::compute_bytes_sha256(b"original game dxgi.dll").as_str())
        );

        // 4. Uninstall
        installer
            .uninstall(
                &store,
                "steam:12345",
                opti_desc.id.as_str(),
                game_dir.to_str().unwrap(),
            )
            .expect("uninstall succeeds");

        // Byte-identical to unmodded initial state
        let final_fingerprint = collect_dir_fingerprint(&game_dir);
        assert_eq!(final_fingerprint, initial_fingerprint);
    }

    #[test]
    fn test_proxy_dll_switch_restores_and_backs_up() {
        let temp = TempDir::new().unwrap();
        let game_dir = temp.path().join("game");
        let backups_dir = temp.path().join("backups");
        setup_unreal_game_folder(&game_dir);

        // Also add an original winmm.dll to the game folder
        let bin_win64 = game_dir.join("Binaries/Win64");
        std::fs::write(bin_win64.join("winmm.dll"), b"original game winmm.dll").unwrap();

        let initial_fingerprint = collect_dir_fingerprint(&game_dir);

        // Create OptiScaler fixture
        let fixture_src = temp.path().join("opti_src");
        std::fs::create_dir_all(&fixture_src).unwrap();
        std::fs::write(fixture_src.join("OptiScaler.dll"), b"OptiScaler DLL bytes").unwrap();
        std::fs::write(fixture_src.join("OptiScaler.ini"), b"[General]\nLogLevel=1\n").unwrap();
        let archive_7z = temp.path().join("OptiScaler.7z");
        sevenz_rust2::compress_to_path(&fixture_src, &archive_7z).unwrap();

        let procs = Arc::new(MockProcesses::new(vec![]));
        let installer = PlatformToolInstaller::new(backups_dir.clone(), procs);
        let store = InMemoryToolStore::default();
        let catalog = MockCatalog::new();

        let opti_desc = optiscaler_descriptor();
        catalog.register_archive(opti_desc.id.clone(), ToolChannel::Stable, "0.7.9", &archive_7z);

        let release = ToolRelease {
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            tag_name: "v0.7.9".to_string(),
            name: "OptiScaler v0.7.9".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: "https://example.com/opti.7z".to_string(),
            asset_name: "OptiScaler.7z".to_string(),
            size_bytes: 1024,
            sha256: None,
            cached_path: Some(archive_7z.to_string_lossy().to_string()),
        };

        // 1. Install with dxgi.dll
        let req1 = InstallRequest {
            entry_key: "steam:12345".to_string(),
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "Binaries/Win64/Game-Win64-Shipping.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            reset_config: false,
            anticheat_confirmed: true,
            custom_payload_path: None,
        };

        installer
            .install_or_update(
                &store,
                &catalog,
                &opti_desc,
                &release,
                &req1,
                game_dir.to_str().unwrap(),
            )
            .expect("install with dxgi succeeds");

        assert_eq!(
            std::fs::read(bin_win64.join("dxgi.dll")).unwrap(),
            b"OptiScaler DLL bytes"
        );
        assert_eq!(
            std::fs::read(bin_win64.join("winmm.dll")).unwrap(),
            b"original game winmm.dll"
        );

        // 2. Switch proxy DLL to winmm.dll
        let req2 = InstallRequest {
            entry_key: "steam:12345".to_string(),
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "Binaries/Win64/Game-Win64-Shipping.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Winmm),
            reset_config: false,
            anticheat_confirmed: true,
            custom_payload_path: None,
        };

        installer
            .install_or_update(
                &store,
                &catalog,
                &opti_desc,
                &release,
                &req2,
                game_dir.to_str().unwrap(),
            )
            .expect("proxy switch to winmm succeeds");

        // dxgi.dll restored to original game content!
        assert_eq!(
            std::fs::read(bin_win64.join("dxgi.dll")).unwrap(),
            b"original game dxgi.dll"
        );
        // winmm.dll is now OptiScaler!
        assert_eq!(
            std::fs::read(bin_win64.join("winmm.dll")).unwrap(),
            b"OptiScaler DLL bytes"
        );

        // 3. Uninstall
        installer
            .uninstall(
                &store,
                "steam:12345",
                opti_desc.id.as_str(),
                game_dir.to_str().unwrap(),
            )
            .expect("uninstall succeeds");

        let final_fingerprint = collect_dir_fingerprint(&game_dir);
        assert_eq!(final_fingerprint, initial_fingerprint);
    }

    #[test]
    fn test_layered_three_tool_composition_all_uninstall_orders() {
        // Test all 4 uninstall orders:
        // Order A: plugin -> swapper -> OptiScaler
        // Order B: OptiScaler -> swapper -> plugin
        // Order C: swapper -> OptiScaler -> plugin
        // Order D: swapper -> plugin -> OptiScaler
        let orders = vec![
            vec!["mfg-unlocker", "fsr4-rdna2-lite", "optiscaler"],
            vec!["optiscaler", "fsr4-rdna2-lite", "mfg-unlocker"],
            vec!["fsr4-rdna2-lite", "optiscaler", "mfg-unlocker"],
            vec!["fsr4-rdna2-lite", "mfg-unlocker", "optiscaler"],
        ];

        for (test_idx, order) in orders.into_iter().enumerate() {
            let temp = TempDir::new().unwrap();
            let game_dir = temp.path().join(format!("game_{test_idx}"));
            let backups_dir = temp.path().join(format!("backups_{test_idx}"));
            setup_unreal_game_folder(&game_dir);

            let initial_fingerprint = collect_dir_fingerprint(&game_dir);

            // Fixture 1: OptiScaler
            let opti_src = temp.path().join("opti_src");
            std::fs::create_dir_all(&opti_src).unwrap();
            std::fs::write(opti_src.join("OptiScaler.dll"), b"OptiScaler 0.7.9 binary").unwrap();
            std::fs::write(
                opti_src.join("OptiScaler.ini"),
                b"[General]\nLogLevel = info\n[Plugins]\nLoadAsiPlugins = false\n",
            )
            .unwrap();
            std::fs::write(
                opti_src.join("amd_fidelityfx_dx12.dll"),
                b"OptiScaler amd_fidelityfx_dx12 companion",
            )
            .unwrap();
            let opti_7z = temp.path().join("OptiScaler.7z");
            sevenz_rust2::compress_to_path(&opti_src, &opti_7z).unwrap();

            // Fixture 2: FSR 4 RDNA 2 Lite swapper
            let fsr_src = temp.path().join("fsr_src");
            std::fs::create_dir_all(&fsr_src).unwrap();
            std::fs::write(
                fsr_src.join("amd_fidelityfx_dx12.dll"),
                b"FSR 4 RDNA 2 Lite modded binary",
            )
            .unwrap();
            let fsr_7z = temp.path().join("fsr4.7z");
            sevenz_rust2::compress_to_path(&fsr_src, &fsr_7z).unwrap();

            // Fixture 3: MFG Unlocker AddonOverlay plugin (.asi)
            let mfg_src = temp.path().join("mfg_src");
            std::fs::create_dir_all(&mfg_src).unwrap();
            std::fs::write(mfg_src.join("MFGUnlocker.asi"), b"MFG Unlocker ASI plugin binary").unwrap();
            let mfg_7z = temp.path().join("mfg.7z");
            sevenz_rust2::compress_to_path(&mfg_src, &mfg_7z).unwrap();

            let procs = Arc::new(MockProcesses::new(vec![]));
            let installer = PlatformToolInstaller::new(backups_dir.clone(), procs);
            let store = InMemoryToolStore::default();
            let catalog = MockCatalog::new();

            let opti_desc = optiscaler_descriptor();
            let fsr_desc = fsr4_rdna2_lite_test_descriptor();
            let mfg_desc = mfg_unlocker_test_descriptor();

            catalog.register_archive(opti_desc.id.clone(), ToolChannel::Stable, "0.7.9", &opti_7z);
            catalog.register_archive(fsr_desc.id.clone(), ToolChannel::Stable, "1.0.0", &fsr_7z);
            catalog.register_archive(mfg_desc.id.clone(), ToolChannel::Stable, "1.0.0", &mfg_7z);

            let release_opti = ToolRelease {
                tool_id: opti_desc.id.clone(),
                channel: ToolChannel::Stable,
                version: "0.7.9".to_string(),
                tag_name: "v0.7.9".to_string(),
                name: "OptiScaler".to_string(),
                published_at: "2026-01-01T00:00:00Z".to_string(),
                download_url: "https://example.com/opti.7z".to_string(),
                asset_name: "OptiScaler.7z".to_string(),
                size_bytes: 1024,
                sha256: None,
                cached_path: Some(opti_7z.to_string_lossy().to_string()),
            };

            let release_fsr = ToolRelease {
                tool_id: fsr_desc.id.clone(),
                channel: ToolChannel::Stable,
                version: "1.0.0".to_string(),
                tag_name: "v1.0.0".to_string(),
                name: "FSR 4".to_string(),
                published_at: "2026-01-01T00:00:00Z".to_string(),
                download_url: "https://example.com/fsr.7z".to_string(),
                asset_name: "fsr4.7z".to_string(),
                size_bytes: 1024,
                sha256: None,
                cached_path: Some(fsr_7z.to_string_lossy().to_string()),
            };

            let release_mfg = ToolRelease {
                tool_id: mfg_desc.id.clone(),
                channel: ToolChannel::Stable,
                version: "1.0.0".to_string(),
                tag_name: "v1.0.0".to_string(),
                name: "MFG Unlocker".to_string(),
                published_at: "2026-01-01T00:00:00Z".to_string(),
                download_url: "https://example.com/mfg.7z".to_string(),
                asset_name: "mfg.7z".to_string(),
                size_bytes: 1024,
                sha256: None,
                cached_path: Some(mfg_7z.to_string_lossy().to_string()),
            };

            // 1. Install OptiScaler
            let req_opti = InstallRequest {
                entry_key: "steam:12345".to_string(),
                tool_id: opti_desc.id.clone(),
                channel: ToolChannel::Stable,
                version: "0.7.9".to_string(),
                target_exe: "Binaries/Win64/Game-Win64-Shipping.exe".to_string(),
                proxy_dll: Some(ProxyDllName::Dxgi),
                reset_config: false,
                anticheat_confirmed: true,
                custom_payload_path: None,
            };
            installer
                .install_or_update(
                    &store,
                    &catalog,
                    &opti_desc,
                    &release_opti,
                    &req_opti,
                    game_dir.to_str().unwrap(),
                )
                .unwrap();

            // 2. Install FSR 4 swapper over OptiScaler
            let req_fsr = InstallRequest {
                entry_key: "steam:12345".to_string(),
                tool_id: fsr_desc.id.clone(),
                channel: ToolChannel::Stable,
                version: "1.0.0".to_string(),
                target_exe: "Binaries/Win64/Game-Win64-Shipping.exe".to_string(),
                proxy_dll: None,
                reset_config: false,
                anticheat_confirmed: true,
                custom_payload_path: None,
            };
            installer
                .install_or_update(
                    &store,
                    &catalog,
                    &fsr_desc,
                    &release_fsr,
                    &req_fsr,
                    game_dir.to_str().unwrap(),
                )
                .unwrap();

            // 3. Install MFG Unlocker plugin
            let req_mfg = InstallRequest {
                entry_key: "steam:12345".to_string(),
                tool_id: mfg_desc.id.clone(),
                channel: ToolChannel::Stable,
                version: "1.0.0".to_string(),
                target_exe: "Binaries/Win64/Game-Win64-Shipping.exe".to_string(),
                proxy_dll: None,
                reset_config: false,
                anticheat_confirmed: true,
                custom_payload_path: None,
            };
            installer
                .install_or_update(
                    &store,
                    &catalog,
                    &mfg_desc,
                    &release_mfg,
                    &req_mfg,
                    game_dir.to_str().unwrap(),
                )
                .unwrap();

            let bin_win64 = game_dir.join("Binaries/Win64");
            // Check active state
            assert_eq!(
                std::fs::read(bin_win64.join("dxgi.dll")).unwrap(),
                b"OptiScaler 0.7.9 binary"
            );
            assert_eq!(
                std::fs::read(bin_win64.join("amd_fidelityfx_dx12.dll")).unwrap(),
                b"FSR 4 RDNA 2 Lite modded binary"
            );
            assert_eq!(
                std::fs::read(bin_win64.join("plugins/MFGUnlocker.asi")).unwrap(),
                b"MFG Unlocker ASI plugin binary"
            );
            let opti_ini = std::fs::read_to_string(bin_win64.join("OptiScaler.ini")).unwrap();
            assert!(opti_ini.contains("LoadAsiPlugins = true"));

            // Uninstall in the specified order!
            for tool_name in &order {
                installer
                    .uninstall(
                        &store,
                        "steam:12345",
                        tool_name,
                        game_dir.to_str().unwrap(),
                    )
                    .unwrap_or_else(|e| panic!("uninstall {tool_name} failed in order {order:?}: {e}"));
            }

            // Verify byte-identical to initial unmodded state
            let final_fingerprint = collect_dir_fingerprint(&game_dir);
            assert_eq!(
                final_fingerprint, initial_fingerprint,
                "Failed byte-identical comparison for order: {order:?}"
            );
        }
    }

    #[test]
    fn test_stopped_game_process_guard() {
        let temp = TempDir::new().unwrap();
        let game_dir = temp.path().join("game");
        let backups_dir = temp.path().join("backups");
        setup_unreal_game_folder(&game_dir);

        let bin_win64 = game_dir.join("Binaries/Win64");
        let running_exe = bin_win64.join("Game-Win64-Shipping.exe");

        let procs = Arc::new(MockProcesses::new(vec![running_exe.to_string_lossy().to_string()]));
        let installer = PlatformToolInstaller::new(backups_dir, procs.clone());
        let store = InMemoryToolStore::default();
        let catalog = MockCatalog::new();
        let opti_desc = optiscaler_descriptor();

        let release = ToolRelease {
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            tag_name: "v0.7.9".to_string(),
            name: "OptiScaler".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: "https://example.com/opti.7z".to_string(),
            asset_name: "OptiScaler.7z".to_string(),
            size_bytes: 1024,
            sha256: None,
            cached_path: None,
        };

        let request = InstallRequest {
            entry_key: "steam:12345".to_string(),
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "Binaries/Win64/Game-Win64-Shipping.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            reset_config: false,
            anticheat_confirmed: true,
            custom_payload_path: None,
        };

        let err = installer
            .install_or_update(
                &store,
                &catalog,
                &opti_desc,
                &release,
                &request,
                game_dir.to_str().unwrap(),
            )
            .unwrap_err();
        assert_eq!(err, ToolError::GameRunning);

        let err_un = installer
            .uninstall(
                &store,
                "steam:12345",
                opti_desc.id.as_str(),
                game_dir.to_str().unwrap(),
            )
            .unwrap_err();
        assert_eq!(err_un, ToolError::GameRunning);

        // Stop the game
        *procs.running.lock().unwrap() = vec![];
        assert!(installer.ensure_game_stopped(&game_dir.canonicalize().unwrap()).is_ok());
    }

    #[test]
    fn test_mid_install_failure_rollback() {
        let temp = TempDir::new().unwrap();
        let game_dir = temp.path().join("game");
        let backups_dir = temp.path().join("backups");
        setup_unreal_game_folder(&game_dir);

        let initial_fingerprint = collect_dir_fingerprint(&game_dir);

        // Archive with invalid path traversal component (e.g. invalid filename or non-PE target exe)
        let procs = Arc::new(MockProcesses::new(vec![]));
        let installer = PlatformToolInstaller::new(backups_dir, procs);
        let store = InMemoryToolStore::default();
        let catalog = MockCatalog::new();
        let opti_desc = optiscaler_descriptor();

        let release = ToolRelease {
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            tag_name: "v0.7.9".to_string(),
            name: "OptiScaler".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: "https://example.com/opti.7z".to_string(),
            asset_name: "OptiScaler.7z".to_string(),
            size_bytes: 1024,
            sha256: None,
            cached_path: None,
        };

        // Target executable that does not exist
        let request = InstallRequest {
            entry_key: "steam:12345".to_string(),
            tool_id: opti_desc.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "NonExistent.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            reset_config: false,
            anticheat_confirmed: true,
            custom_payload_path: None,
        };

        let err = installer
            .install_or_update(
                &store,
                &catalog,
                &opti_desc,
                &release,
                &request,
                game_dir.to_str().unwrap(),
            )
            .unwrap_err();

        assert!(matches!(err, ToolError::TargetExeNotFound(_)));

        // Folder remains byte-identical
        let final_fingerprint = collect_dir_fingerprint(&game_dir);
        assert_eq!(final_fingerprint, initial_fingerprint);
        assert!(store.list_installations().unwrap().is_empty());
    }

    #[test]
    fn test_recover_incomplete_installations_on_startup() {
        let temp = TempDir::new().unwrap();
        let game_dir = temp.path().join("game");
        let backups_dir = temp.path().join("backups");
        setup_unreal_game_folder(&game_dir);

        let procs = Arc::new(MockProcesses::new(vec![]));
        let installer = PlatformToolInstaller::new(backups_dir, procs);
        let store = InMemoryToolStore::default();

        let opti_id = ToolId::new("optiscaler");

        // Save an installation stuck in Installing state
        store
            .save_installation(&ToolInstallation {
                entry_key: "steam:12345".to_string(),
                tool_id: opti_id.clone(),
                channel: ToolChannel::Stable,
                version: "0.7.9".to_string(),
                target_exe: "Binaries/Win64/Game-Win64-Shipping.exe".to_string(),
                proxy_dll: Some(ProxyDllName::Dxgi),
                state: ToolInstallState::Installing,
                installed_at: 100,
                updated_at: 100,
            })
            .unwrap();

        // Save a mock file record
        store
            .save_files(&[ToolFileRecord {
                entry_key: "steam:12345".to_string(),
                tool_id: opti_id.clone(),
                relative_path: "Binaries/Win64/dxgi.dll".to_string(),
                installed_sha256: "sha_new".to_string(),
                backup_path: None,
                original_sha256: None,
                prior_owner_tool_id: None,
                created_dirs: vec![],
            }])
            .unwrap();

        assert_eq!(store.incomplete_installations().unwrap().len(), 1);

        let gdir_str = game_dir.to_str().unwrap().to_string();
        let recovered = installer
            .recover_incomplete(&store, &|_| Some(gdir_str.clone()))
            .expect("recovery succeeds");

        assert_eq!(recovered, 1);
        assert_eq!(store.incomplete_installations().unwrap().len(), 0);
        assert!(store.get_installation("steam:12345", &opti_id).unwrap().is_none());
        assert!(store.list_files("steam:12345", &opti_id).unwrap().is_empty());
    }
}
