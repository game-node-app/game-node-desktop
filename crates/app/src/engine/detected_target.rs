//! User approval, repair and launch of provider-detected game targets.

use super::{Engine, unix_now};
use gamenode_core::drm_free::{InstallationHost, LocalError, Registration, resolve_install_path};
use gamenode_core::library::{
    DetectedTarget, Input as LibraryInput, LaunchError, StoredEntry, TargetApproval, TargetSpec,
};
use std::path::Path;

impl Engine {
    /// Approves the currently reported target after host validation. File I/O runs on a worker.
    pub async fn review_detected_target(
        &self,
        entry_key: String,
        expected_revision: u64,
    ) -> Result<(), String> {
        let engine = self.clone();
        tokio::task::spawn_blocking(move || {
            engine.review_target_blocking(entry_key, expected_revision, None)
        })
        .await
        .map_err(|_| "target review worker failed".to_owned())?
    }

    /// Lets the user locate a replacement executable, validates it under the install root and
    /// records the explicit selection as approval for the new target revision.
    pub async fn repair_detected_target(
        &self,
        entry_key: String,
        expected_revision: u64,
    ) -> Result<(), String> {
        let engine = self.clone();
        tokio::task::spawn_blocking(move || {
            engine.review_target_blocking(entry_key, expected_revision, Some(()))
        })
        .await
        .map_err(|_| "target repair worker failed".to_owned())?
    }

    fn review_target_blocking(
        &self,
        entry_key: String,
        expected_revision: u64,
        repair: Option<()>,
    ) -> Result<(), String> {
        let entry = {
            let state = self.lock();
            state
                .library
                .entry(&entry_key)
                .cloned()
                .ok_or_else(|| "library entry no longer exists".to_owned())?
        };
        let current = entry
            .target
            .as_ref()
            .filter(|target| target.revision == expected_revision)
            .ok_or_else(|| "detected target changed; review the latest version".to_owned())?;

        let replacement = if let Some(()) = repair {
            let TargetSpec::Executable { root, .. } = &current.spec else {
                return Err("only executable targets can be repaired".into());
            };
            let Some(selected) = gamenode_platform::installations::pick_executable_in(root)
                .map_err(|_| "could not open the executable picker".to_owned())?
            else {
                return Ok(());
            };
            let host = self
                .shared
                .ports
                .installations
                .as_deref()
                .ok_or_else(|| "executable launching is unavailable".to_owned())?;
            let (registration, identity) =
                inspect_target(host, &current.spec, Some(&selected)).map_err(target_error)?;
            let TargetSpec::Executable { arguments, .. } = &current.spec else {
                unreachable!("checked above")
            };
            let relative = Path::new(&registration.executable)
                .strip_prefix(Path::new(&registration.root))
                .map_err(|_| "the selected executable must stay inside the game folder".to_owned())?
                .to_string_lossy()
                .replace('\\', "/");
            let spec = TargetSpec::Executable {
                root: registration.root.clone(),
                path: relative,
                arguments: arguments.clone(),
                working_directory: String::new(),
            };
            if !spec.validate_shape() {
                return Err("the selected executable is not a valid game target".into());
            }
            current.repaired(spec, identity, unix_now())
        } else {
            if !current.available {
                return Err(
                    "the detected target is unavailable; locate its executable to repair it".into(),
                );
            }
            let identity = match &current.spec {
                TargetSpec::Uri { .. } => None,
                TargetSpec::Executable { .. } => {
                    let host = self
                        .shared
                        .ports
                        .installations
                        .as_deref()
                        .ok_or_else(|| "executable launching is unavailable".to_owned())?;
                    match inspect_target(host, &current.spec, None) {
                        Ok((_, identity)) => Some(identity),
                        Err(LocalError::InvalidTarget) => {
                            self.mark_detected_target_unavailable(&entry.key, expected_revision);
                            return Err("the detected executable is missing or invalid; locate it to repair the target".into());
                        }
                        Err(error) => return Err(target_error(error)),
                    }
                }
            };
            let mut approved = current.clone();
            approved.approved = true;
            approved.available = true;
            approved.identity = identity;
            approved
        };

        self.save_review(&entry, expected_revision, replacement)
    }

    fn save_review(
        &self,
        entry: &StoredEntry,
        expected_revision: u64,
        target: DetectedTarget,
    ) -> Result<(), String> {
        match self
            .shared
            .ports
            .library
            .save_target_review(&entry.key, expected_revision, &target)
            .map_err(|_| "could not save the target approval".to_owned())?
        {
            TargetApproval::Applied { .. } => {
                self.library_input(LibraryInput::TargetReviewed {
                    entry_key: entry.key.clone(),
                    expected_revision,
                    target,
                });
                Ok(())
            }
            TargetApproval::Conflict { .. } => {
                Err("detected target changed; review the latest version".into())
            }
            TargetApproval::Missing => Err("detected target no longer exists".into()),
        }
    }

    fn mark_detected_target_unavailable(&self, entry_key: &str, expected_revision: u64) {
        let entry = {
            let state = self.lock();
            state.library.entry(entry_key).cloned()
        };
        let Some(entry) = entry else { return };
        let Some(current) = entry
            .target
            .as_ref()
            .filter(|target| target.revision == expected_revision)
        else {
            return;
        };
        let target = DetectedTarget::missing(current, unix_now());
        let _ = self.save_review(&entry, expected_revision, target);
    }
}

pub(super) fn launch(
    engine: &Engine,
    entry: &StoredEntry,
    target: &DetectedTarget,
) -> Result<(), LaunchError> {
    if !target.available {
        return Err(LaunchError::Incomplete);
    }
    if !target.approved {
        return Err(LaunchError::Incomplete);
    }
    match &target.spec {
        TargetSpec::Uri { uri } => {
            let url = url::Url::parse(uri).map_err(|_| LaunchError::Failed)?;
            engine
                .shared
                .ports
                .uri
                .open(&url)
                .map_err(|error| match error {
                    gamenode_core::ports::UriError::Unsupported => LaunchError::Unsupported,
                    gamenode_core::ports::UriError::NoHandler => LaunchError::NoHandler,
                    gamenode_core::ports::UriError::Failed => LaunchError::Failed,
                })
        }
        TargetSpec::Executable { .. } => {
            let Some(host) = engine.shared.ports.installations.as_deref() else {
                return Err(LaunchError::Unsupported);
            };
            let (registration, identity) = match inspect_target(host, &target.spec, None) {
                Ok(value) => value,
                Err(LocalError::Unsupported) => return Err(LaunchError::Unsupported),
                Err(LocalError::InvalidTarget) => {
                    engine.mark_detected_target_unavailable(&entry.key, target.revision);
                    return Err(LaunchError::Incomplete);
                }
                Err(_) => return Err(LaunchError::Failed),
            };
            if target.identity.as_deref() != Some(identity.as_str()) {
                engine.mark_detected_target_unavailable(&entry.key, target.revision);
                return Err(LaunchError::Incomplete);
            }
            host.launch(&registration).map_err(|error| match error {
                LocalError::Unsupported => LaunchError::Unsupported,
                LocalError::InvalidTarget => {
                    engine.mark_detected_target_unavailable(&entry.key, target.revision);
                    LaunchError::Incomplete
                }
                _ => LaunchError::Failed,
            })
        }
    }
}

fn inspect_target(
    host: &dyn InstallationHost,
    spec: &TargetSpec,
    selected_path: Option<&str>,
) -> Result<(Registration, String), LocalError> {
    let TargetSpec::Executable {
        root,
        path,
        arguments,
        working_directory,
    } = spec
    else {
        return Err(LocalError::InvalidTarget);
    };
    if !spec.validate_shape() {
        return Err(LocalError::InvalidTarget);
    }
    let executable = selected_path
        .map(str::to_owned)
        .or_else(|| resolve_install_path(root, path))
        .ok_or(LocalError::InvalidTarget)?;
    let mut registration = host.inspect(&executable)?;
    let identity = registration.file_identity.clone();
    registration.root = root.clone();
    registration.working_directory = if selected_path.is_some() || working_directory.is_empty() {
        root.clone()
    } else {
        resolve_install_path(root, working_directory).ok_or(LocalError::InvalidTarget)?
    };
    registration.arguments = arguments.clone();
    registration.gameplay = vec![registration.executable.clone()];
    registration.launchers.clear();
    let registration = host.validate_registration(&registration)?;
    Ok((registration, identity))
}

fn target_error(error: LocalError) -> String {
    match error {
        LocalError::Unsupported => "executable targets are unavailable on this platform".into(),
        LocalError::InvalidTarget => {
            "the executable is missing, invalid, or outside the game's install folder".into()
        }
        LocalError::Cancelled => "target review was cancelled".into(),
        LocalError::Failed(_) => "the executable could not be validated".into(),
    }
}
