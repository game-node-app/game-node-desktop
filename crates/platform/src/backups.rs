//! Isolated, bounded Ludusavi execution. All entry points are worker-only.
use gamenode_core::drm_free::*;
use serde_json::Value;
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub struct Ludusavi {
    pub helper: PathBuf,
    pub private: PathBuf,
    writer: Mutex<()>,
    checked: std::sync::OnceLock<()>,
}
impl Ludusavi {
    pub fn bundled(data: &Path) -> Result<Self, LocalError> {
        let executable = std::env::current_exe().map_err(failed)?;
        Ok(Self {
            helper: executable
                .parent()
                .ok_or(LocalError::InvalidTarget)?
                .join("backup-assets/ludusavi.exe"),
            private: data.join("private/backups"),
            writer: Mutex::new(()),
            checked: std::sync::OnceLock::new(),
        })
    }
    pub fn namespace(request: &BackupRequest) -> Result<PathBuf, LocalError> {
        if !Path::new(&request.destination).is_absolute() {
            return Err(LocalError::InvalidTarget);
        }
        for id in [&request.installation.id, &request.profile.id] {
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            {
                return Err(LocalError::InvalidTarget);
            }
        }
        Ok(Path::new(&request.destination)
            .join("manual")
            .join(&request.installation.id)
            .join(&request.profile.id))
    }
    fn run(
        &self,
        args: &[String],
        cancelled: &AtomicBool,
        stopped: Option<&Registration>,
    ) -> Result<Value, LocalError> {
        if !self.helper.is_absolute() || !self.helper.is_file() {
            return Err(LocalError::Failed(
                "bundled backup helper is unavailable".into(),
            ));
        }
        let mut child = std::process::Command::new(&self.helper)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(failed)?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let overflow = std::sync::Arc::new(AtomicBool::new(false));
        fn reader(mut pipe: impl Read, overflow: &AtomicBool) -> std::io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            let mut block = [0; 8192];
            loop {
                let count = pipe.read(&mut block)?;
                if count == 0 {
                    break;
                }
                if bytes.len() + count > 8 * 1024 * 1024 {
                    overflow.store(true, Ordering::Relaxed);
                    break;
                }
                bytes.extend_from_slice(&block[..count]);
            }
            Ok(bytes)
        }
        let out_flag = overflow.clone();
        let err_flag = overflow.clone();
        let out = std::thread::spawn(move || reader(stdout, &out_flag));
        let err = std::thread::spawn(move || reader(stderr, &err_flag));
        let start = Instant::now();
        let mut next_check = Instant::now();
        let outcome = loop {
            if cancelled.load(Ordering::Relaxed)
                || overflow.load(Ordering::Relaxed)
                || start.elapsed() > Duration::from_secs(300)
            {
                let _ = child.kill();
                let _ = child.wait();
                break Err(if cancelled.load(Ordering::Relaxed) {
                    LocalError::Cancelled
                } else {
                    LocalError::Failed("backup helper exceeded time or output limit".into())
                });
            }
            if let Some(installation) = stopped
                && Instant::now() >= next_check
            {
                next_check = Instant::now() + Duration::from_secs(1);
                if let Err(error) = ensure_stopped(installation) {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(error);
                }
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(failed(error));
                }
            }
        };
        let output = out
            .join()
            .map_err(|_| failed("output reader failed"))?
            .map_err(failed)?;
        let _ = err
            .join()
            .map_err(|_| failed("error reader failed"))?
            .map_err(failed)?;
        let status = outcome?;
        if args == ["--version"] {
            if !status.success() || String::from_utf8_lossy(&output).trim() != "ludusavi 0.31.0" {
                return Err(failed("incompatible backup helper version"));
            }
            return Ok(serde_json::json!({}));
        }
        let value: Value = serde_json::from_slice(&output)
            .map_err(|_| failed("backup helper returned invalid JSON"))?;
        if !status.success() && !has_failure(&value) {
            return Err(failed("backup helper reported failure"));
        }
        Ok(value)
    }
    fn config(&self, request: &BackupRequest) -> Result<PathBuf, LocalError> {
        let path = self
            .private
            .join(&request.installation.id)
            .join(&request.profile.id);
        std::fs::create_dir_all(&path).map_err(failed)?;
        let definition = execution_definition(&request.profile, &request.installation)?;
        let mut manifest = serde_yaml::Mapping::new();
        manifest.insert(
            serde_yaml::Value::String(request.profile.title.clone()),
            definition,
        );
        std::fs::write(
            path.join("manifest.yaml"),
            serde_yaml::to_string(&manifest).map_err(failed)?,
        )
        .map_err(failed)?;
        let config = serde_json::json!({"roots":[],"manifest":{"enable":true,"secondary":[]},"customGames":[],"cloud":{"synchronize":false},"release":{"check":false}});
        std::fs::write(
            path.join("config.yaml"),
            serde_json::to_vec(&config).map_err(failed)?,
        )
        .map_err(failed)?;
        Ok(path)
    }
}
fn ensure_stopped(installation: &Registration) -> Result<(), LocalError> {
    use gamenode_core::ports::Processes;
    let processes = crate::processes::HostProcesses::new()
        .list()
        .map_err(|_| failed("cannot confirm game is stopped"))?;
    let normalized = |p: &str| gamenode_core::provider::path::normalize(p).to_lowercase();
    if processes.iter().any(|p| {
        installation
            .gameplay
            .iter()
            .any(|exe| normalized(exe) == normalized(&p.exe_path))
    }) {
        return Err(failed("game is running"));
    }
    Ok(())
}

fn failed(error: impl std::fmt::Display) -> LocalError {
    LocalError::Failed(error.to_string())
}
pub fn glob_literal(path: &str) -> String {
    path.replace('\\', "/")
        .chars()
        .map(|c| match c {
            '[' => "[[]".into(),
            ']' => "[]]".into(),
            '*' => "[*]".into(),
            '?' => "[?]".into(),
            c => c.to_string(),
        })
        .collect()
}
pub fn execution_definition(
    profile: &SaveProfile,
    installation: &Registration,
) -> Result<serde_yaml::Value, LocalError> {
    let mut definition: serde_yaml::Value =
        serde_yaml::from_str(&profile.definition).map_err(failed)?;
    let map = definition
        .as_mapping_mut()
        .ok_or(LocalError::InvalidTarget)?;
    for field in ["installDir", "launch", "alias"] {
        map.remove(serde_yaml::Value::String(field.into()));
    }
    if let Some(files) = map
        .get_mut(serde_yaml::Value::String("files".into()))
        .and_then(|v| v.as_mapping_mut())
    {
        let mut bound = serde_yaml::Mapping::new();
        for (path, rule) in files.iter() {
            let path = path.as_str().ok_or(LocalError::InvalidTarget)?;
            let game = Path::new(&installation.root)
                .file_name()
                .ok_or(LocalError::InvalidTarget)?
                .to_string_lossy();
            let path = path.replace("<root>/<game>", &glob_literal(&installation.root));
            if path.contains("<root>") {
                return Err(failed(
                    "save rule requires an explicit installation context",
                ));
            }
            let path = path
                .replace("<base>", &glob_literal(&installation.root))
                .replace("<root>", &glob_literal(&installation.root))
                .replace("<game>", &glob_literal(&game));
            bound.insert(serde_yaml::Value::String(path), rule.clone());
        }
        *files = bound;
    }
    Ok(definition)
}
fn has_failure(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            (key == "failed" && value.as_bool() == Some(true))
                || (key == "error" && !value.is_null())
                || has_failure(value)
        }),
        Value::Array(items) => items.iter().any(has_failure),
        _ => false,
    }
}
fn copy_tree(source: &Path, destination: &Path, cancelled: &AtomicBool) -> Result<(), LocalError> {
    std::fs::create_dir_all(destination).map_err(failed)?;
    for entry in std::fs::read_dir(source).map_err(failed)? {
        if cancelled.load(Ordering::Relaxed) {
            return Err(LocalError::Cancelled);
        }
        let entry = entry.map_err(failed)?;
        let kind = entry.file_type().map_err(failed)?;
        if kind.is_symlink() {
            return Err(failed("archive contains a symbolic link"));
        }
        let to = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &to, cancelled)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), to).map_err(failed)?;
        }
    }
    Ok(())
}
fn reconcile(active: &Path) -> Result<(), LocalError> {
    let previous = active.with_extension("previous");
    let stage = active.with_extension("staging");
    let journal = active.with_extension("journal");
    if !journal.exists() {
        return Ok(());
    }
    if !active.exists() {
        if stage.join(".gamenode-verified.json").is_file() {
            std::fs::rename(&stage, active).map_err(failed)?;
        } else if previous.exists() {
            std::fs::rename(&previous, active).map_err(failed)?;
        }
    }
    if active.exists() {
        if previous.exists() {
            std::fs::remove_dir_all(previous).map_err(failed)?;
        }
        if stage.exists() {
            std::fs::remove_dir_all(stage).map_err(failed)?;
        }
        std::fs::remove_file(journal).map_err(failed)?;
    }
    Ok(())
}
impl BackupEngine for Ludusavi {
    fn execute(
        &self,
        request: &BackupRequest,
        cancelled: &AtomicBool,
    ) -> Result<BackupReport, LocalError> {
        let _guard = self
            .writer
            .lock()
            .map_err(|_| failed("backup writer lock failed"))?;
        self.execute_serialized(request, cancelled)
    }
}
impl Ludusavi {
    fn execute_serialized(
        &self,
        request: &BackupRequest,
        cancelled: &AtomicBool,
    ) -> Result<BackupReport, LocalError> {
        #[cfg(not(windows))]
        {
            let _ = (request, cancelled);
            return Err(LocalError::Unsupported);
        }
        #[allow(unreachable_code)]
        {
            if request.profile.title.trim().is_empty() {
                return Err(LocalError::InvalidTarget);
            }
            if self.checked.get().is_none() {
                self.run(&["--version".into()], cancelled, None)?;
                let _ = self.checked.set(());
            }
            if request.profile.title.trim().is_empty() {
                return Err(LocalError::InvalidTarget);
            }
            let active = Self::namespace(request)?;
            reconcile(&active)?;
            if matches!(request.operation, BackupOperation::Backup)
                && let Some(job_id) = &request.job_id
                && let Ok(marker) = std::fs::read(active.join(".gamenode-job.json"))
                && let Ok(marker) = serde_json::from_slice::<Value>(&marker)
                && marker["job"].as_str() == Some(job_id)
            {
                return Ok(BackupReport {
                    state: serde_json::from_value(marker["state"].clone())
                        .unwrap_or(JobState::Successful),
                    json: marker["report"].to_string(),
                });
            }
            let config = self.config(request)?;
            let backup = matches!(request.operation, BackupOperation::Backup);
            if backup {
                let preview_args = vec![
                    "--config".into(),
                    config.to_string_lossy().into_owned(),
                    "--no-manifest-update".into(),
                    "backup".into(),
                    "--preview".into(),
                    "--api".into(),
                    "--no-cloud-sync".into(),
                    "--path".into(),
                    active.to_string_lossy().into_owned(),
                    "--".into(),
                    request.profile.title.clone(),
                ];
                let preview = self.run(&preview_args, cancelled, None)?;
                let files = preview
                    .get("games")
                    .and_then(|g| g.get(&request.profile.title))
                    .and_then(|g| g.get("files"))
                    .and_then(Value::as_object)
                    .map(|m| m.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                validate_destination(
                    Path::new(&request.destination),
                    &files
                        .iter()
                        .map(PathBuf::from)
                        .chain([PathBuf::from(&request.installation.root)])
                        .collect::<Vec<_>>(),
                )?;
                if has_failure(&preview) {
                    return Err(failed("save preview reported partial failure"));
                }
            }

            if let BackupOperation::Restore { backup } = &request.operation {
                ensure_stopped(&request.installation)?;
                let preview_request = BackupRequest {
                    operation: BackupOperation::RestorePreview {
                        backup: backup.clone(),
                    },
                    ..request.clone()
                };
                let preview = self.execute_serialized(&preview_request, cancelled)?;
                if preview.state == JobState::Partial {
                    return Err(failed("restore preview failed"));
                }
                let preview: Value = serde_json::from_str(&preview.json).map_err(failed)?;
                let game = preview
                    .get("games")
                    .and_then(|g| g.get(&request.profile.title))
                    .ok_or_else(|| failed("restore preview omitted game"))?;
                let mut definition = serde_json::json!({"files":{},"registry":{}});
                for kind in ["files", "registry"] {
                    if let Some(targets) = game.get(kind).and_then(Value::as_object) {
                        for target in targets.keys() {
                            let target = if kind == "files" {
                                glob_literal(target)
                            } else {
                                target.clone()
                            };
                            definition[kind]
                                .as_object_mut()
                                .unwrap()
                                .insert(target, serde_json::json!({}));
                        }
                    }
                }
                let mut recovery = request.clone();
                recovery.operation = BackupOperation::Backup;
                recovery.job_id = None;
                recovery.profile.id = format!(
                    "{}_recovery_{}",
                    request.profile.id,
                    crate::random::session_id()
                );
                recovery.profile.definition = definition.to_string();
                let report = self.execute_serialized(&recovery, cancelled)?;
                if !matches!(report.state, JobState::Successful | JobState::Unchanged) {
                    return Err(failed("recovery backup failed; restore aborted"));
                }
                std::fs::create_dir_all(&active).map_err(failed)?;
                std::fs::write(active.join(".gamenode-restore-recovery.json"), serde_json::to_vec(&serde_json::json!({"backup":backup,"recovery":Self::namespace(&recovery)?,"preview":preview})).map_err(failed)?).map_err(failed)?;
                ensure_stopped(&request.installation)?;
            }

            let stage = active.with_extension("staging");
            if backup {
                if stage.exists() {
                    std::fs::remove_dir_all(&stage).map_err(failed)?;
                }
                if active.exists() {
                    copy_tree(&active, &stage, cancelled)?;
                } else {
                    std::fs::create_dir_all(&stage).map_err(failed)?;
                }
            }
            let path = if backup { &stage } else { &active };
            let mut args = vec![
                "--config".into(),
                config.to_string_lossy().into_owned(),
                "--no-manifest-update".into(),
            ];
            args.push(
                match request.operation {
                    BackupOperation::Preview | BackupOperation::Backup => "backup",
                    BackupOperation::History => "backups",
                    _ => "restore",
                }
                .into(),
            );
            args.extend([
                "--api".into(),
                "--path".into(),
                path.to_string_lossy().into_owned(),
            ]);
            if request.operation != BackupOperation::History {
                args.push("--no-cloud-sync".into());
            }
            match &request.operation {
                BackupOperation::Preview | BackupOperation::RestorePreview { .. } => {
                    args.push("--preview".into())
                }
                BackupOperation::Backup => args.extend(
                    ["--force", "--full-limit", "10", "--differential-limit", "0"]
                        .map(str::to_owned),
                ),
                BackupOperation::Restore { .. } => args.push("--force".into()),
                _ => {}
            }
            if let BackupOperation::Restore { backup }
            | BackupOperation::RestorePreview { backup } = &request.operation
            {
                if backup.is_empty() {
                    return Err(LocalError::InvalidTarget);
                }
                args.extend(["--backup".into(), backup.clone()]);
            }
            args.extend(["--".into(), request.profile.title.clone()]);
            let report = self.run(
                &args,
                cancelled,
                if matches!(
                    request.operation,
                    BackupOperation::Backup | BackupOperation::Restore { .. }
                ) {
                    Some(&request.installation)
                } else {
                    None
                },
            )?;
            if !report.get("games").is_some_and(|g| g.is_object()) {
                return Err(failed("backup helper report has no games"));
            }
            let games = report["games"]
                .as_object()
                .ok_or_else(|| failed("backup report omitted games"))?;
            if games.keys().any(|title| title != &request.profile.title) {
                return Err(failed("backup report contains an unexpected game"));
            }
            if matches!(
                request.operation,
                BackupOperation::Restore { .. } | BackupOperation::RestorePreview { .. }
            ) && !games.contains_key(&request.profile.title)
            {
                return Err(failed("selected restore version is unavailable"));
            }
            let state = if has_failure(&report) {
                JobState::Partial
            } else if report
                .pointer("/overall/totalGames")
                .and_then(Value::as_u64)
                == Some(0)
                || report
                    .pointer("/overall/changedGames/same")
                    .and_then(Value::as_u64)
                    == Some(1)
            {
                JobState::Unchanged
            } else {
                JobState::Successful
            };
            if backup && matches!(state, JobState::Successful | JobState::Unchanged) {
                std::fs::write(
                    stage.join(".gamenode-verified.json"),
                    serde_json::to_vec(&report).map_err(failed)?,
                )
                .map_err(failed)?;
                if let Some(job_id) = &request.job_id {
                    std::fs::write(
                        stage.join(".gamenode-job.json"),
                        serde_json::to_vec(
                            &serde_json::json!({"job":job_id,"report":report,"state":state}),
                        )
                        .map_err(failed)?,
                    )
                    .map_err(failed)?;
                }
                let previous = active.with_extension("previous");
                std::fs::write(active.with_extension("journal"), b"verified promotion")
                    .map_err(failed)?;
                if active.exists() {
                    std::fs::rename(&active, &previous).map_err(failed)?;
                }
                std::fs::rename(&stage, &active).map_err(failed)?;
                reconcile(&active)?;
            }
            Ok(BackupReport {
                state,
                json: report.to_string(),
            })
        }
    }
}
/// Resolve existing ancestors too, so a not-yet-created destination cannot hide a junction alias.
pub fn resolved_destination(path: &Path) -> Result<PathBuf, LocalError> {
    if !path.is_absolute() {
        return Err(LocalError::InvalidTarget);
    }
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        match std::fs::canonicalize(ancestor) {
            Ok(mut canonical) => {
                for part in suffix.into_iter().rev() {
                    canonical.push(part);
                }
                return Ok(canonical);
            }
            Err(_) => {
                let name = ancestor.file_name().ok_or(LocalError::InvalidTarget)?;
                if name == ".." {
                    return Err(LocalError::InvalidTarget);
                }
                suffix.push(name.to_os_string());
                ancestor = ancestor.parent().ok_or(LocalError::InvalidTarget)?;
            }
        }
    }
}
pub fn validate_destination(destination: &Path, protected: &[PathBuf]) -> Result<(), LocalError> {
    let destination = resolved_destination(destination)?;
    let normalize = |p: &Path| {
        let value = p.to_string_lossy().replace('\\', "/");
        if cfg!(windows) {
            value.to_lowercase()
        } else {
            value
        }
    };
    let destination = normalize(&destination);
    for source in protected {
        let source = normalize(&resolved_destination(source)?);
        if source == destination
            || source.starts_with(&(destination.trim_end_matches('/').to_owned() + "/"))
            || destination.starts_with(&(source.trim_end_matches('/').to_owned() + "/"))
        {
            return Err(failed(
                "backup destination overlaps a protected save or game folder",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_errors_and_glob_literals_are_preserved() {
        assert!(has_failure(
            &serde_json::json!({"games":{"g":{"files":{"save":{"failed":true}}}}})
        ));
        assert_eq!(glob_literal("C:/a[b]?"), "C:/a[[]b[]][?]");
    }
    #[test]
    fn interrupted_promotion_recovers_verified_stage() {
        let dir = tempfile::tempdir().unwrap();
        let active = dir.path().join("profile");
        std::fs::create_dir(active.with_extension("staging")).unwrap();
        std::fs::write(
            active
                .with_extension("staging")
                .join(".gamenode-verified.json"),
            b"{}",
        )
        .unwrap();
        std::fs::write(active.with_extension("journal"), b"promotion").unwrap();
        reconcile(&active).unwrap();
        assert!(active.is_dir());
        assert!(!active.with_extension("journal").exists());
    }
}

#[cfg(all(test, windows))]
mod adapter_windows_tests {
    use super::*;
    #[test]
    fn bundled_adapter_preserves_history_and_recovers_before_selected_restore() {
        let helper = std::env::var_os("GAMENODE_BACKUP_TEST_HELPER")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap()
                    .join("../../dist/windows/gamenode-desktop/backup-assets/ludusavi.exe")
            });
        let helper = std::fs::canonicalize(helper)
            .expect("prepare bundled helper assets before Windows adapter tests");
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("game");
        std::fs::create_dir(&root).unwrap();
        let live = fixture.path().join("live.sav");
        std::fs::write(&live, b"original").unwrap();
        let mut rules = serde_json::Map::new();
        rules.insert(glob_literal(&live.to_string_lossy()), serde_json::json!({}));
        let installation = Registration {
            id: "installation".into(),
            title: "Fixture".into(),
            executable: root.join("game.exe").to_string_lossy().into_owned(),
            file_identity: "fixture".into(),
            root: root.to_string_lossy().into_owned(),
            working_directory: root.to_string_lossy().into_owned(),
            arguments: vec![],
            gameplay: vec![],
            launchers: vec![],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            available: true,
        };
        let profile = SaveProfile {
            id: "profile".into(),
            installation: installation.id.clone(),
            revision: 1,
            title: "Fixture".into(),
            manifest_hash: "fixture".into(),
            definition: serde_json::json!({"files":rules}).to_string(),
            files: vec![live.to_string_lossy().into_owned()],
            registry: vec![],
            support: SaveSupport::Supported,
            automatic: None,
        };
        let mut request = BackupRequest {
            job_id: Some("job-fixture".into()),
            installation,
            profile,
            destination: fixture
                .path()
                .join("archives")
                .to_string_lossy()
                .into_owned(),
            operation: BackupOperation::Backup,
        };
        let adapter = Ludusavi {
            helper,
            private: fixture.path().join("private"),
            writer: Mutex::new(()),
            checked: std::sync::OnceLock::new(),
        };
        let cancelled = AtomicBool::new(false);
        assert_eq!(
            adapter.execute(&request, &cancelled).unwrap().state,
            JobState::Successful
        );
        std::fs::write(&live, b"uncommitted change after a crash").unwrap();
        assert_eq!(
            adapter.execute(&request, &cancelled).unwrap().state,
            JobState::Successful
        );
        request.operation = BackupOperation::History;
        let report: Value =
            serde_json::from_str(&adapter.execute(&request, &cancelled).unwrap().json).unwrap();
        let id = report["games"]["Fixture"]["backups"][0]["name"]
            .as_str()
            .unwrap()
            .to_owned();
        std::fs::write(&live, b"current state to recover").unwrap();
        request.operation = BackupOperation::Restore { backup: id };
        assert_eq!(
            adapter.execute(&request, &cancelled).unwrap().state,
            JobState::Successful
        );
        assert_eq!(std::fs::read(&live).unwrap(), b"original");
        let namespace = Ludusavi::namespace(&request).unwrap();
        let recovery: Value = serde_json::from_slice(
            &std::fs::read(namespace.join(".gamenode-restore-recovery.json")).unwrap(),
        )
        .unwrap();
        let recovery = PathBuf::from(recovery["recovery"].as_str().unwrap());
        assert!(recovery.join(".gamenode-verified.json").is_file());
    }
}
