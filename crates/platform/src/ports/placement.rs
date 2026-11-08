//! Placement of accepted supplied files (design D5, D7).
//!
//! An accepted choice either reaches the port as a verified copy or as an argument/environment
//! value:
//!
//! - a copy goes to the port's own folder (`app`) or to the user's roaming data, settings or local
//!   data folder. The user's original is opened read-only and is never moved or modified;
//! - a copy is written under a temporary name in the destination folder and renamed when complete,
//!   so a crash never leaves a half-written file at the final path;
//! - before the job writes any path, the path is reported to a journal callback (design D5), so
//!   startup recovery can remove it;
//! - a flag or positional slot adds the source path to the launch arguments, and an `env` slot adds
//!   it to the environment map applied at launch. Neither copies anything.
//!
//! Everything here performs blocking I/O and must run on a worker thread. Copies are cancellable
//! between chunks of at most [`CHUNK_BYTES`](crate::ports::supplied::CHUNK_BYTES).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use gamenode_core::ports::{
    CopyRoot, Placement, PlannedSlot, SetupKind, is_safe_relative_path, is_valid_env_name,
};

use crate::ports::supplied::{
    CHUNK_BYTES, CheckedChoice, FileDigests, SupplyError, WorkProgress, hash_file, relative_join,
};
use crate::random::nonce;

/// The absolute folders the four copy roots resolve to for one job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyRoots {
    /// The port's own folder while the job runs: the staging folder. Finalizing renames it into
    /// place, so a copy placed here ends up inside the installed port.
    pub app: PathBuf,
    /// The user's roaming application data (`%APPDATA%`).
    pub data: PathBuf,
    /// The user's settings folder (`%APPDATA%`).
    pub config: PathBuf,
    /// The user's local application data (`%LOCALAPPDATA%`).
    pub local_data: PathBuf,
}

impl CopyRoots {
    /// Resolves the user-data roots from the host and uses `app` for the port's own folder.
    pub fn host(app: impl Into<PathBuf>) -> Result<Self, PlaceError> {
        let roaming = dirs::config_dir().ok_or(PlaceError::UnresolvedRoot { root: "data" })?;
        let local =
            dirs::data_local_dir().ok_or(PlaceError::UnresolvedRoot { root: "localData" })?;
        Ok(Self {
            app: app.into(),
            data: roaming.clone(),
            config: roaming,
            local_data: local,
        })
    }

    /// The absolute folder a [`CopyRoot`] names.
    pub fn path(&self, root: CopyRoot) -> &Path {
        match root {
            CopyRoot::App => &self.app,
            CopyRoot::Data => &self.data,
            CopyRoot::Config => &self.config,
            CopyRoot::LocalData => &self.local_data,
        }
    }
}

/// One accepted slot, paired with the planned placement it belongs to.
#[derive(Clone, Copy, Debug)]
pub struct SlotChoice<'a> {
    pub slot: &'a PlannedSlot,
    pub checked: &'a CheckedChoice,
}

/// What placing one slot produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacedSlot {
    pub id: String,
    /// The user's original path. Never modified.
    pub source: String,
    /// The absolute paths this slot created, in the order they were journaled.
    pub created: Vec<String>,
    /// The arguments this slot contributes, before the launch order is applied.
    pub arguments: Vec<String>,
    /// The environment variables this slot contributes.
    pub environment: BTreeMap<String, String>,
}

/// What placing every accepted slot produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlacementOutcome {
    /// Recipe arguments in launch order: positional paths first, then flags. The user's own
    /// arguments follow these (see [`launch_arguments`]).
    pub arguments: Vec<String>,
    /// Environment variables from `env` slots, applied when the port launches.
    pub environment: BTreeMap<String, String>,
    /// Every path created, in the order it was journaled.
    pub created: Vec<String>,
    /// Per-slot detail, in plan order.
    pub slots: Vec<PlacedSlot>,
}

/// Places every accepted slot: copies what the recipe copies, and collects the arguments and
/// environment values it passes.
///
/// `journal` is called with each path immediately before the job writes it. `cancelled` is checked
/// between copy chunks; `progress` reports bytes copied across all slots.
pub fn place_slots(
    choices: &[SlotChoice<'_>],
    roots: &CopyRoots,
    journal: &mut dyn FnMut(&Path) -> Result<(), PlaceError>,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(WorkProgress),
) -> Result<PlacementOutcome, PlaceError> {
    let mut outcome = PlacementOutcome::default();
    let mut positional = Vec::new();
    let mut flags = Vec::new();

    for choice in choices {
        let path = choice.checked.path.display().to_string();
        // A package slot is extracted while the job runs, but the port must be told the path it
        // will find after Finalizing, not the staging path that disappears then. `PlannedSlot`
        // records that final path for exactly this case (design D5, D8).
        let launch_path = choice.slot.target.clone().unwrap_or_else(|| path.clone());
        let mut placed = PlacedSlot {
            id: choice.slot.id.clone(),
            source: path.clone(),
            created: Vec::new(),
            arguments: Vec::new(),
            environment: BTreeMap::new(),
        };

        match &choice.slot.placement {
            Placement::Copy { root, destination } => {
                if !is_safe_relative_path(destination) {
                    return Err(PlaceError::UnsafeDestination {
                        destination: destination.clone(),
                    });
                }
                let target = relative_join(roots.path(*root), destination);
                let mut created = Vec::new();
                match choice.checked.kind {
                    SetupKind::File => copy_file_verified(
                        &choice.checked.path,
                        &target,
                        choice.checked.digests.as_ref(),
                        journal,
                        &mut created,
                        cancelled,
                        progress,
                    )?,
                    SetupKind::Folder => copy_folder_verified(
                        choice.checked,
                        &target,
                        journal,
                        &mut created,
                        cancelled,
                        progress,
                    )?,
                }
                placed.created = created.clone();
                outcome.created.extend(created);
            }
            Placement::Flag { flag, joined } => {
                let arguments = if *joined {
                    vec![format!("{flag}{launch_path}")]
                } else {
                    vec![flag.clone(), launch_path.clone()]
                };
                placed.arguments = arguments.clone();
                flags.extend(arguments);
            }
            Placement::Positional => {
                placed.arguments = vec![launch_path.clone()];
                positional.push(launch_path.clone());
            }
            Placement::Env { name } => {
                if !is_valid_env_name(name) {
                    return Err(PlaceError::InvalidEnvironment { name: name.clone() });
                }
                placed.environment.insert(name.clone(), launch_path.clone());
                outcome.environment.insert(name.clone(), launch_path);
            }
            Placement::Unused => {}
        }

        outcome.slots.push(placed);
    }

    outcome.arguments = positional;
    outcome.arguments.extend(flags);
    Ok(outcome)
}

/// The launch arguments of a port: the recipe's first, then the ones the user adds, so a user
/// argument never displaces a recipe argument (spec: desktop-drm-free-games).
pub fn launch_arguments(recipe: &[String], user: &[String]) -> Vec<String> {
    let mut arguments = recipe.to_vec();
    arguments.extend(user.iter().cloned());
    arguments
}

fn copy_file_verified(
    source: &Path,
    destination: &Path,
    expected: Option<&FileDigests>,
    journal: &mut dyn FnMut(&Path) -> Result<(), PlaceError>,
    created: &mut Vec<String>,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(WorkProgress),
) -> Result<(), PlaceError> {
    if !source.is_file() {
        return Err(PlaceError::SourceMissing {
            path: source.display().to_string(),
        });
    }
    if destination.exists() {
        return Err(PlaceError::DestinationExists {
            path: destination.display().to_string(),
        });
    }
    ensure_directory(destination.parent(), journal, created)?;
    let temporary = temporary_path(destination);
    record(&temporary, journal, created)?;
    let result = (|| {
        let mut input = File::open(source).map_err(|error| PlaceError::Io {
            path: source.display().to_string(),
            reason: error.to_string(),
        })?;
        let total = input.metadata().map(|meta| meta.len()).unwrap_or(0);
        let mut output = File::create(&temporary).map_err(|error| PlaceError::Io {
            path: temporary.display().to_string(),
            reason: error.to_string(),
        })?;
        let chunk = usize::try_from(total.clamp(4096, CHUNK_BYTES as u64)).unwrap_or(CHUNK_BYTES);
        let mut buffer = vec![0u8; chunk];
        let mut done = 0u64;
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err(PlaceError::Cancelled);
            }
            let read = input.read(&mut buffer).map_err(|error| PlaceError::Io {
                path: source.display().to_string(),
                reason: error.to_string(),
            })?;
            if read == 0 {
                break;
            }
            output
                .write_all(&buffer[..read])
                .map_err(|error| PlaceError::Io {
                    path: temporary.display().to_string(),
                    reason: error.to_string(),
                })?;
            done += read as u64;
            progress(WorkProgress { done, total });
        }
        output.flush().map_err(|error| PlaceError::Io {
            path: temporary.display().to_string(),
            reason: error.to_string(),
        })?;
        drop(output);

        // Re-hash the copy, not the source: that is what proves the bytes on disk are the ones
        // validation accepted (spec: copies are verified after writing).
        if let Some(expected) = expected {
            let actual = hash_file(&temporary, &mut |_| {}, cancelled)?;
            if &actual != expected {
                return Err(PlaceError::CopyMismatch {
                    path: destination.display().to_string(),
                });
            }
        }

        record(destination, journal, created)?;
        std::fs::rename(&temporary, destination).map_err(|error| PlaceError::Io {
            path: destination.display().to_string(),
            reason: error.to_string(),
        })
    })();
    if result.is_err() {
        // The journal also covers this path; removing it now keeps a failed slot clean.
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn copy_folder_verified(
    checked: &CheckedChoice,
    destination: &Path,
    journal: &mut dyn FnMut(&Path) -> Result<(), PlaceError>,
    created: &mut Vec<String>,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(WorkProgress),
) -> Result<(), PlaceError> {
    let source = checked.path.as_path();
    if !source.is_dir() {
        return Err(PlaceError::SourceMissing {
            path: source.display().to_string(),
        });
    }
    if destination.exists() {
        return Err(PlaceError::DestinationExists {
            path: destination.display().to_string(),
        });
    }
    ensure_directory(destination.parent(), journal, created)?;
    let temporary = temporary_path(destination);
    record(&temporary, journal, created)?;
    let result = (|| {
        std::fs::create_dir(&temporary).map_err(|error| PlaceError::Io {
            path: temporary.display().to_string(),
            reason: error.to_string(),
        })?;
        let total = tree_size(source)?;
        let mut done = 0u64;
        let mut buffer = vec![0u8; CHUNK_BYTES];
        copy_tree(
            source,
            &temporary,
            cancelled,
            &mut done,
            total,
            progress,
            &mut buffer,
        )?;

        // The folder's marker is what validation looked at; confirm it in the copy, and compare
        // its digests when the slot listed hashes (design D7).
        if let Some(marker) = &checked.marker {
            let copied = relative_join(&temporary, marker);
            if !copied.is_file() {
                return Err(PlaceError::MissingMarker {
                    path: copied.display().to_string(),
                });
            }
            if let Some(expected) = &checked.digests {
                let actual = hash_file(&copied, &mut |_| {}, cancelled)?;
                if &actual != expected {
                    return Err(PlaceError::CopyMismatch {
                        path: copied.display().to_string(),
                    });
                }
            }
        }

        record(destination, journal, created)?;
        std::fs::rename(&temporary, destination).map_err(|error| PlaceError::Io {
            path: destination.display().to_string(),
            reason: error.to_string(),
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&temporary);
    }
    result
}

fn copy_tree(
    source: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
    done: &mut u64,
    total: u64,
    progress: &mut dyn FnMut(WorkProgress),
    buffer: &mut [u8],
) -> Result<(), PlaceError> {
    let entries = std::fs::read_dir(source).map_err(|error| PlaceError::Io {
        path: source.display().to_string(),
        reason: error.to_string(),
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| PlaceError::Io {
            path: source.display().to_string(),
            reason: error.to_string(),
        })?;
        let file_type = entry.file_type().map_err(|error| PlaceError::Io {
            path: entry.path().display().to_string(),
            reason: error.to_string(),
        })?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if file_type.is_dir() {
            std::fs::create_dir(&to).map_err(|error| PlaceError::Io {
                path: to.display().to_string(),
                reason: error.to_string(),
            })?;
            copy_tree(&from, &to, cancelled, done, total, progress, buffer)?;
        } else if file_type.is_file() {
            let mut input = File::open(&from).map_err(|error| PlaceError::Io {
                path: from.display().to_string(),
                reason: error.to_string(),
            })?;
            let mut output = File::create(&to).map_err(|error| PlaceError::Io {
                path: to.display().to_string(),
                reason: error.to_string(),
            })?;
            loop {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(PlaceError::Cancelled);
                }
                let read = input.read(buffer).map_err(|error| PlaceError::Io {
                    path: from.display().to_string(),
                    reason: error.to_string(),
                })?;
                if read == 0 {
                    break;
                }
                output
                    .write_all(&buffer[..read])
                    .map_err(|error| PlaceError::Io {
                        path: to.display().to_string(),
                        reason: error.to_string(),
                    })?;
                *done += read as u64;
                progress(WorkProgress { done: *done, total });
            }
            output.flush().map_err(|error| PlaceError::Io {
                path: to.display().to_string(),
                reason: error.to_string(),
            })?;
        } else {
            // A link could point anywhere; a supplied dump never needs one.
            return Err(PlaceError::UnsupportedEntry {
                path: from.display().to_string(),
            });
        }
    }
    Ok(())
}

fn tree_size(path: &Path) -> Result<u64, PlaceError> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(path).map_err(|error| PlaceError::Io {
        path: path.display().to_string(),
        reason: error.to_string(),
    })? {
        let entry = entry.map_err(|error| PlaceError::Io {
            path: path.display().to_string(),
            reason: error.to_string(),
        })?;
        let file_type = entry.file_type().map_err(|error| PlaceError::Io {
            path: entry.path().display().to_string(),
            reason: error.to_string(),
        })?;
        if file_type.is_dir() {
            total += tree_size(&entry.path())?;
        } else if file_type.is_file() {
            total += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        } else {
            return Err(PlaceError::UnsupportedEntry {
                path: entry.path().display().to_string(),
            });
        }
    }
    Ok(total)
}

/// Journals every missing ancestor of `path` and then creates it.
fn ensure_directory(
    path: Option<&Path>,
    journal: &mut dyn FnMut(&Path) -> Result<(), PlaceError>,
    created: &mut Vec<String>,
) -> Result<(), PlaceError> {
    let Some(path) = path else {
        return Ok(());
    };
    let mut missing = Vec::new();
    let mut current = Some(path);
    while let Some(directory) = current {
        if directory.as_os_str().is_empty() || directory.exists() {
            break;
        }
        missing.push(directory.to_path_buf());
        current = directory.parent();
    }
    for directory in missing.into_iter().rev() {
        record(&directory, journal, created)?;
        std::fs::create_dir(&directory).map_err(|error| PlaceError::Io {
            path: directory.display().to_string(),
            reason: error.to_string(),
        })?;
    }
    Ok(())
}

/// Reports a path to the journal before it is written and remembers it.
fn record(
    path: &Path,
    journal: &mut dyn FnMut(&Path) -> Result<(), PlaceError>,
    created: &mut Vec<String>,
) -> Result<(), PlaceError> {
    journal(path)?;
    created.push(path.display().to_string());
    Ok(())
}

/// A temporary name in the destination's own folder, so the final rename stays on one volume.
fn temporary_path(destination: &Path) -> PathBuf {
    let nonce = nonce();
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "copy".into());
    destination.with_file_name(format!(".gamenode-tmp-{}-{name}", &nonce[..16]))
}

impl From<SupplyError> for PlaceError {
    fn from(error: SupplyError) -> Self {
        match error {
            SupplyError::Cancelled => PlaceError::Cancelled,
            other => PlaceError::Verify(other.to_string()),
        }
    }
}

/// Why a supplied choice could not be placed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlaceError {
    #[error("the recipe destination {destination:?} is not a safe relative path")]
    UnsafeDestination { destination: String },
    #[error("the destination {path} already exists")]
    DestinationExists { path: String },
    #[error("{path} is no longer a file or folder")]
    SourceMissing { path: String },
    #[error("the destination folder of {path} could not be created: {reason}")]
    Io { path: String, reason: String },
    #[error("the copy of {path} does not match the accepted digest")]
    CopyMismatch { path: String },
    #[error("the copied folder is missing {path}")]
    MissingMarker { path: String },
    #[error("the supplied folder holds a link or special file at {path}")]
    UnsupportedEntry { path: String },
    #[error("the recipe names an invalid environment variable: {name:?}")]
    InvalidEnvironment { name: String },
    #[error("this computer does not report the {root} folder")]
    UnresolvedRoot { root: &'static str },
    #[error("the copy could not be verified: {0}")]
    Verify(String),
    #[error("the operation was cancelled")]
    Cancelled,
}

impl PlaceError {
    /// True when the work stopped because the job was cancelled.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use gamenode_core::ports::{SetupKind, is_valid_env_name};

    use crate::ports::supplied::{CheckedChoice, FileDigests};

    fn digests(path: &Path) -> FileDigests {
        hash_file(path, &mut |_| {}, &AtomicBool::new(false)).expect("test file hashes")
    }

    fn roots(base: &Path) -> CopyRoots {
        CopyRoots {
            app: base.join("app"),
            data: base.join("roaming"),
            config: base.join("settings"),
            local_data: base.join("local"),
        }
    }

    fn planned(id: &str, placement: Placement) -> PlannedSlot {
        PlannedSlot {
            id: id.into(),
            label: id.into(),
            required: true,
            kind: SetupKind::File,
            accepted: Vec::new(),
            placement,
            from_package: None,
            source: None,
            target: None,
        }
    }

    fn checked(path: &Path, kind: SetupKind, marker: Option<&str>) -> CheckedChoice {
        let file = match marker {
            Some(marker) => relative_join(path, marker),
            None => path.to_path_buf(),
        };
        CheckedChoice {
            path: path.to_path_buf(),
            kind,
            marker: marker.map(str::to_string),
            digests: Some(digests(&file)),
        }
    }

    #[test]
    fn copies_reach_every_root_and_leave_the_original_alone() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("dump.iso");
        fs::write(&source, b"disc image").unwrap();
        let roots = roots(dir.path());
        let source_choice = checked(&source, SetupKind::File, None);

        for (root, destination) in [
            (CopyRoot::App, "game/dump.iso"),
            (CopyRoot::Data, "GameNode/dump.iso"),
            (CopyRoot::Config, "GameNode/dump.iso"),
            (CopyRoot::LocalData, "GameNode/dump.iso"),
        ] {
            let slot = planned(
                "iso",
                Placement::Copy {
                    root,
                    destination: destination.into(),
                },
            );
            let outcome = place_slots(
                &[SlotChoice {
                    slot: &slot,
                    checked: &source_choice,
                }],
                &roots,
                &mut |_| Ok(()),
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .unwrap();
            let target = relative_join(roots.path(root), destination);
            assert_eq!(fs::read(&target).unwrap(), b"disc image", "{target:?}");
            assert!(outcome.created.contains(&target.display().to_string()));
        }

        assert_eq!(fs::read(&source).unwrap(), b"disc image");
        assert!(source.exists(), "the user's original stays in place");
    }

    #[test]
    fn a_joined_flag_takes_the_path_in_one_argument() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("dump.iso");
        fs::write(&source, b"disc").unwrap();
        let choice = checked(&source, SetupKind::File, None);
        let slot = planned(
            "iso",
            Placement::Flag {
                flag: "--iso=".into(),
                joined: true,
            },
        );

        let outcome = place_slots(
            &[SlotChoice {
                slot: &slot,
                checked: &choice,
            }],
            &roots(dir.path()),
            &mut |_| Ok(()),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(
            outcome.arguments,
            vec![format!("--iso={}", source.display())]
        );
        assert!(outcome.created.is_empty(), "a flag copies nothing");
    }

    #[test]
    fn positional_paths_come_before_flags() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.iso");
        let second = dir.path().join("second.iso");
        fs::write(&first, b"one").unwrap();
        fs::write(&second, b"two").unwrap();
        let first_choice = checked(&first, SetupKind::File, None);
        let second_choice = checked(&second, SetupKind::File, None);
        let flag_slot = planned(
            "second",
            Placement::Flag {
                flag: "--iso".into(),
                joined: false,
            },
        );
        let positional_slot = planned("first", Placement::Positional);

        let outcome = place_slots(
            &[
                SlotChoice {
                    slot: &flag_slot,
                    checked: &second_choice,
                },
                SlotChoice {
                    slot: &positional_slot,
                    checked: &first_choice,
                },
            ],
            &roots(dir.path()),
            &mut |_| Ok(()),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(
            outcome.arguments,
            vec![
                first.display().to_string(),
                "--iso".into(),
                second.display().to_string()
            ]
        );
    }

    #[test]
    fn a_package_slot_passes_the_final_path_not_the_staging_one() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging/.gamenode-slot-game");
        fs::create_dir_all(&staging).unwrap();
        let choice = CheckedChoice {
            path: staging,
            kind: SetupKind::Folder,
            marker: None,
            digests: None,
        };
        let mut slot = planned(
            "game",
            Placement::Flag {
                flag: "--game=".into(),
                joined: true,
            },
        );
        slot.kind = SetupKind::Folder;
        slot.target = Some("D:/Ports/Bloodborne/CUSA03173".into());

        let outcome = place_slots(
            &[SlotChoice {
                slot: &slot,
                checked: &choice,
            }],
            &roots(dir.path()),
            &mut |_| Ok(()),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(
            outcome.arguments,
            vec!["--game=D:/Ports/Bloodborne/CUSA03173".to_string()]
        );
    }

    #[test]
    fn the_user_arguments_follow_the_recipe_arguments() {
        let recipe = vec!["--iso".to_string(), "D:\\dump.iso".to_string()];
        let user = vec!["--fullscreen".to_string()];
        assert_eq!(
            launch_arguments(&recipe, &user),
            vec![
                "--iso".to_string(),
                "D:\\dump.iso".to_string(),
                "--fullscreen".to_string()
            ]
        );
    }

    #[test]
    fn environment_slots_fill_the_launch_environment() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("dump.iso");
        fs::write(&source, b"disc").unwrap();
        let choice = checked(&source, SetupKind::File, None);
        let slot = planned(
            "iso",
            Placement::Env {
                name: "BLOODBORNE_DUMP".into(),
            },
        );
        assert!(is_valid_env_name("BLOODBORNE_DUMP"));

        let outcome = place_slots(
            &[SlotChoice {
                slot: &slot,
                checked: &choice,
            }],
            &roots(dir.path()),
            &mut |_| Ok(()),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(
            outcome.environment.get("BLOODBORNE_DUMP"),
            Some(&source.display().to_string())
        );
        assert!(outcome.arguments.is_empty());
    }

    #[test]
    fn every_journaled_path_is_reported_before_it_exists() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("dump.iso");
        fs::write(&source, b"disc").unwrap();
        let choice = checked(&source, SetupKind::File, None);
        let slot = planned(
            "iso",
            Placement::Copy {
                root: CopyRoot::Data,
                destination: "GameNode/port/dump.iso".into(),
            },
        );
        let mut journaled = Vec::new();

        let outcome = place_slots(
            &[SlotChoice {
                slot: &slot,
                checked: &choice,
            }],
            &roots(dir.path()),
            &mut |path| {
                assert!(
                    !path.exists(),
                    "{path:?} must be journaled before it is written"
                );
                journaled.push(path.to_path_buf());
                Ok(())
            },
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();

        assert_eq!(journaled.len(), outcome.created.len());
        assert!(
            journaled
                .iter()
                .any(|path| path.to_string_lossy().contains(".gamenode-tmp-")),
            "the copy is written under a temporary name first"
        );
        let target = dir.path().join("roaming/GameNode/port/dump.iso");
        assert!(target.is_file());
        assert_eq!(fs::read(&target).unwrap(), b"disc");
        assert!(
            !target
                .parent()
                .unwrap()
                .read_dir()
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".gamenode-tmp-")),
            "the temporary name is renamed away"
        );
    }

    #[test]
    fn a_folder_is_copied_with_its_markers_and_checked_in_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("CUSA03173");
        fs::create_dir_all(source.join("sce_sys")).unwrap();
        fs::write(source.join("sce_sys/param.sfo"), b"sfo").unwrap();
        fs::write(source.join("eboot.bin"), b"boot").unwrap();
        let choice = checked(&source, SetupKind::Folder, Some("eboot.bin"));
        let slot = planned(
            "game",
            Placement::Copy {
                root: CopyRoot::App,
                destination: "CUSA03173".into(),
            },
        );

        let outcome = place_slots(
            &[SlotChoice {
                slot: &slot,
                checked: &choice,
            }],
            &roots(dir.path()),
            &mut |_| Ok(()),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        let copy = dir.path().join("app/CUSA03173");
        assert_eq!(fs::read(copy.join("eboot.bin")).unwrap(), b"boot");
        assert_eq!(fs::read(copy.join("sce_sys/param.sfo")).unwrap(), b"sfo");
        assert!(
            outcome.slots[0]
                .created
                .contains(&copy.display().to_string()),
            "the final copy is journaled"
        );
        assert_eq!(fs::read(source.join("eboot.bin")).unwrap(), b"boot");
    }

    #[test]
    fn a_copy_that_does_not_match_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("dump.iso");
        fs::write(&source, b"disc").unwrap();
        let mut choice = checked(&source, SetupKind::File, None);
        choice.digests = Some(FileDigests {
            sha1: "0000000000000000000000000000000000000000".into(),
            sha256: "0".repeat(64),
        });
        let slot = planned(
            "iso",
            Placement::Copy {
                root: CopyRoot::App,
                destination: "dump.iso".into(),
            },
        );

        let error = place_slots(
            &[SlotChoice {
                slot: &slot,
                checked: &choice,
            }],
            &roots(dir.path()),
            &mut |_| Ok(()),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, PlaceError::CopyMismatch { .. }));
        assert!(!dir.path().join("app/dump.iso").exists());
        let leftovers: Vec<_> = fs::read_dir(dir.path().join("app"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(leftovers.is_empty(), "no temporary name is left behind");
    }

    #[test]
    fn a_cancelled_copy_writes_nothing_at_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("dump.iso");
        fs::write(&source, vec![1u8; 64 * 1024]).unwrap();
        let choice = checked(&source, SetupKind::File, None);
        let slot = planned(
            "iso",
            Placement::Copy {
                root: CopyRoot::App,
                destination: "dump.iso".into(),
            },
        );

        let error = place_slots(
            &[SlotChoice {
                slot: &slot,
                checked: &choice,
            }],
            &roots(dir.path()),
            &mut |_| Ok(()),
            &AtomicBool::new(true),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.is_cancelled());
        assert!(!dir.path().join("app/dump.iso").exists());
    }
}
