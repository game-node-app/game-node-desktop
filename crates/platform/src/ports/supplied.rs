//! Validation of the files a player supplies for a port's guided setup (design D7).
//!
//! A file is read once, with SHA-1 and SHA-256 computed in the same pass. A folder must contain
//! every marker the recipe declares, and its hashes apply to `hash_file` or, when the overlay does
//! not name one, to the first `contains` entry. A slot with no accepted hashes accepts any file.
//!
//! Everything here reads the user's own files, so every function is blocking and must run on a
//! worker thread, never on the UI or main thread. Hashing is cancellable between chunks of at most
//! [`CHUNK_BYTES`] and reports progress as it goes.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use gamenode_core::ports::{
    AcceptedHash, HashAlgorithm, SetupFile, SetupKind, is_safe_relative_path,
};
use sha1::Sha1;
use sha2::{Digest, Sha256};

/// The largest buffer read in one pass, and the granularity at which cancellation is checked.
pub const CHUNK_BYTES: usize = 8 * 1024 * 1024;

/// How far a hash or a copy has got, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkProgress {
    /// Bytes read or written so far.
    pub done: u64,
    /// Bytes the work needs in total, when the source reports a size.
    pub total: u64,
}

/// The SHA-1 and SHA-256 digests of one file, in lowercase hexadecimal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDigests {
    pub sha1: String,
    pub sha256: String,
}

impl FileDigests {
    /// The digest for one algorithm.
    pub fn hex(&self, algorithm: HashAlgorithm) -> &str {
        match algorithm {
            HashAlgorithm::Sha1 => &self.sha1,
            HashAlgorithm::Sha256 => &self.sha256,
        }
    }
}

/// A supplied choice that passed validation and is ready to place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedChoice {
    /// The absolute source the user picked. It is never moved or modified.
    pub path: PathBuf,
    pub kind: SetupKind,
    /// For a folder, the marker file under it that placement confirms in the copy, relative to
    /// `path`; when accepted hashes are given it is also the file they describe.
    pub marker: Option<String>,
    /// Both digests of the file or marker, or `None` when the slot accepts any file.
    pub digests: Option<FileDigests>,
}

/// The result of checking one setup slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    /// An optional slot the user left out. Nothing is placed and nothing is passed.
    Skipped,
    /// A choice the recipe accepts.
    Accepted(CheckedChoice),
}

/// Checks one supplied choice against a slot's markers and accepted versions (design D7).
///
/// `path` is the user's choice for the slot, or `None` when they chose nothing. A required slot
/// without a usable choice fails; an optional slot without one is [`CheckOutcome::Skipped`].
pub fn check_choice(
    path: Option<&Path>,
    slot: &SetupFile,
    progress: &mut dyn FnMut(WorkProgress),
    cancelled: &AtomicBool,
) -> Result<CheckOutcome, SupplyError> {
    let Some(path) = path else {
        return missing_choice(slot);
    };
    if !path.exists() {
        // A choice that vanished between picking and installing is the same as no choice.
        return missing_choice(slot);
    }

    match slot.kind {
        SetupKind::File => {
            if !path.is_file() {
                return Err(SupplyError::WrongKind {
                    slot: slot.id.clone(),
                    label: slot.label.clone(),
                    expected: "file",
                });
            }
            let digests = if slot.accepted.is_empty() {
                None
            } else {
                Some(hash_file(path, progress, cancelled)?)
            };
            accept(
                CheckedChoice {
                    path: path.to_path_buf(),
                    kind: SetupKind::File,
                    marker: None,
                    digests,
                },
                slot,
            )
        }
        SetupKind::Folder => {
            if !path.is_dir() {
                return Err(SupplyError::WrongKind {
                    slot: slot.id.clone(),
                    label: slot.label.clone(),
                    expected: "folder",
                });
            }
            // A player who owns a `.pkg` dump sometimes picks the folder holding the packages
            // in folder mode. Name the package option instead of only the missing marker.
            let package_folder = slot.from_package.is_some() && folder_contains_packages(path);
            for marker in &slot.contains {
                if !is_safe_relative_path(marker) {
                    return Err(SupplyError::UnsafeMarker {
                        slot: slot.id.clone(),
                        path: marker.clone(),
                    });
                }
                if !relative_join(path, marker).is_file() {
                    if package_folder {
                        return Err(SupplyError::PackagesInFolder {
                            slot: slot.id.clone(),
                            folder: path.display().to_string(),
                        });
                    }
                    return Err(SupplyError::MissingMarker {
                        slot: slot.id.clone(),
                        marker: marker.clone(),
                        folder: path.display().to_string(),
                    });
                }
            }

            // The upstream schema does not say which file a folder's hashes describe, so the
            // overlay names it. Without `hash_file`, the first marker stands in (design D7).
            let marker = slot
                .hash_file
                .clone()
                .or_else(|| slot.contains.first().cloned());
            let marker = match marker {
                Some(marker) => {
                    if !is_safe_relative_path(&marker) {
                        return Err(SupplyError::UnsafeMarker {
                            slot: slot.id.clone(),
                            path: marker,
                        });
                    }
                    let file = relative_join(path, &marker);
                    if !file.is_file() {
                        return Err(SupplyError::MissingMarker {
                            slot: slot.id.clone(),
                            marker: marker.clone(),
                            folder: path.display().to_string(),
                        });
                    }
                    Some((marker, file))
                }
                None if slot.accepted.is_empty() => None,
                None => {
                    return Err(SupplyError::NoHashFile {
                        slot: slot.id.clone(),
                        label: slot.label.clone(),
                    });
                }
            };

            let digests = match &marker {
                Some((_, file)) if !slot.accepted.is_empty() => {
                    Some(hash_file(file, progress, cancelled)?)
                }
                _ => None,
            };
            accept(
                CheckedChoice {
                    path: path.to_path_buf(),
                    kind: SetupKind::Folder,
                    marker: marker.map(|(marker, _)| marker),
                    digests,
                },
                slot,
            )
        }
    }
}

/// Reads one file once, computing both digests, and reports progress per chunk.
pub fn hash_file(
    path: &Path,
    progress: &mut dyn FnMut(WorkProgress),
    cancelled: &AtomicBool,
) -> Result<FileDigests, SupplyError> {
    let mut file = File::open(path).map_err(|error| SupplyError::Io {
        path: path.display().to_string(),
        reason: error.to_string(),
    })?;
    let total = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    // Small files need small buffers; large ones are read in cancellation-sized chunks.
    let chunk = usize::try_from(total.clamp(4096, CHUNK_BYTES as u64)).unwrap_or(CHUNK_BYTES);
    let mut buffer = vec![0u8; chunk];
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut done = 0u64;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(SupplyError::Cancelled);
        }
        let read = file.read(&mut buffer).map_err(|error| SupplyError::Io {
            path: path.display().to_string(),
            reason: error.to_string(),
        })?;
        if read == 0 {
            break;
        }
        sha1.update(&buffer[..read]);
        sha256.update(&buffer[..read]);
        done += read as u64;
        progress(WorkProgress { done, total });
    }
    Ok(FileDigests {
        sha1: format!("{:x}", sha1.finalize()),
        sha256: format!("{:x}", sha256.finalize()),
    })
}

/// The accepted versions as a human-readable list, for a rejection message. A hash without a label
/// shows its algorithm and a short digest.
pub fn accepted_versions(accepted: &[AcceptedHash]) -> String {
    accepted
        .iter()
        .map(|hash| match hash.label.as_deref().map(str::trim) {
            Some(label) if !label.is_empty() => label.to_string(),
            _ => {
                let digest = hash.normalized_value();
                let short = digest.get(..12).unwrap_or(&digest);
                format!("{} {short}…", hash.algo.label())
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Joins a recipe-relative path with either separator style. The caller checks that `relative` is
/// safe.
pub(crate) fn relative_join(base: &Path, relative: &str) -> PathBuf {
    relative
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

/// True when the folder directly holds at least one console package. A dump keeps its `.pkg`
/// files at the root of the folder the player picks, and a shallow scan stays cheap for a
/// multi-gigabyte game folder.
fn folder_contains_packages(folder: &Path) -> bool {
    let Ok(entries) = fs::read_dir(folder) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
            && entry
                .path()
                .extension()
                .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("pkg"))
                .unwrap_or(false)
    })
}

fn missing_choice(slot: &SetupFile) -> Result<CheckOutcome, SupplyError> {
    if slot.required {
        Err(SupplyError::MissingChoice {
            slot: slot.id.clone(),
            label: slot.label.clone(),
        })
    } else {
        Ok(CheckOutcome::Skipped)
    }
}

fn accept(choice: CheckedChoice, slot: &SetupFile) -> Result<CheckOutcome, SupplyError> {
    if slot.accepted.is_empty() {
        return Ok(CheckOutcome::Accepted(choice));
    }
    let Some(digests) = &choice.digests else {
        // `check_choice` hashes whenever hashes are listed, so this cannot happen.
        return Ok(CheckOutcome::Accepted(choice));
    };
    let matched = slot.accepted.iter().any(|accepted| {
        accepted.normalized_value() == digests.hex(accepted.algo).to_ascii_lowercase()
    });
    if matched {
        Ok(CheckOutcome::Accepted(choice))
    } else {
        Err(SupplyError::WrongVersion {
            slot: slot.id.clone(),
            label: slot.label.clone(),
            accepted: accepted_versions(&slot.accepted),
        })
    }
}

/// Why a supplied choice cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SupplyError {
    /// A required slot has no usable choice.
    #[error("choose a file or folder for {label}")]
    MissingChoice { slot: String, label: String },
    /// The choice has the wrong kind for the slot.
    #[error("{label} needs a {expected}, not the choice made")]
    WrongKind {
        slot: String,
        label: String,
        expected: &'static str,
    },
    /// A folder does not contain one of its markers.
    #[error("the folder {folder} does not contain {marker}")]
    MissingMarker {
        slot: String,
        marker: String,
        folder: String,
    },
    /// A folder picked in folder mode holds console packages and the slot can extract them.
    #[error("the folder {folder} contains game packages; use the package option instead")]
    PackagesInFolder { slot: String, folder: String },
    /// The recipe gives no file to apply a folder's hashes to.
    #[error(
        "the recipe for {label} lists accepted hashes but no hash_file or contains entry to check them against"
    )]
    NoHashFile { slot: String, label: String },
    /// The file (or folder marker) does not match any accepted version.
    #[error("the choice for {label} does not match an accepted version (accepted: {accepted})")]
    WrongVersion {
        slot: String,
        label: String,
        accepted: String,
    },
    /// A recipe path that could escape the chosen folder.
    #[error("setup file {slot:?} names an unsafe path: {path:?}")]
    UnsafeMarker { slot: String, path: String },
    /// The file could not be read.
    #[error("could not read {path}: {reason}")]
    Io { path: String, reason: String },
    /// The job was cancelled while hashing.
    #[error("the operation was cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use gamenode_core::ports::{FromPackage, SetupFile};

    const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn slot(kind: SetupKind) -> SetupFile {
        SetupFile {
            id: "game".into(),
            label: "Bloodborne (CUSA03173) v1.09".into(),
            kind,
            ..SetupFile::default()
        }
    }

    fn accepted(algorithm: HashAlgorithm, value: &str, label: Option<&str>) -> AcceptedHash {
        AcceptedHash {
            algo: algorithm,
            value: value.into(),
            label: label.map(str::to_string),
        }
    }

    fn no_progress() -> impl FnMut(WorkProgress) {
        |_| {}
    }

    #[test]
    fn one_pass_computes_both_digests() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dump.iso");
        fs::write(&file, b"abc").unwrap();

        let mut reports = Vec::new();
        let digests = hash_file(
            &file,
            &mut |progress| reports.push(progress),
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(digests.sha1, ABC_SHA1);
        assert_eq!(digests.sha256, ABC_SHA256);
        assert_eq!(reports.last().unwrap().done, 3);
        assert_eq!(reports.last().unwrap().total, 3);
    }

    #[test]
    fn a_matching_sha256_or_sha1_accepts_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dump.iso");
        fs::write(&file, b"abc").unwrap();

        let mut by_sha256 = slot(SetupKind::File);
        by_sha256.accepted = vec![accepted(
            HashAlgorithm::Sha256,
            ABC_SHA256,
            Some("CUSA03173 01.09"),
        )];
        let outcome = check_choice(
            Some(&file),
            &by_sha256,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let CheckOutcome::Accepted(choice) = outcome else {
            panic!("the file matches");
        };
        assert_eq!(choice.digests.as_ref().unwrap().sha1, ABC_SHA1);

        let mut by_sha1 = slot(SetupKind::File);
        by_sha1.accepted = vec![accepted(
            HashAlgorithm::Sha1,
            ABC_SHA1,
            Some("CUSA03173 01.00"),
        )];
        assert!(matches!(
            check_choice(
                Some(&file),
                &by_sha1,
                &mut no_progress(),
                &AtomicBool::new(false)
            ),
            Ok(CheckOutcome::Accepted(_))
        ));
    }

    #[test]
    fn a_slot_without_hashes_accepts_anything_without_reading_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dump.iso");
        fs::write(&file, b"abc").unwrap();
        let slot = slot(SetupKind::File);
        let mut hashed = false;
        let outcome = check_choice(
            Some(&file),
            &slot,
            &mut |_| hashed = true,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(matches!(outcome, CheckOutcome::Accepted(_)));
        assert!(!hashed, "a slot without accepted hashes is not read");
    }

    #[test]
    fn a_wrong_version_names_the_accepted_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dump.iso");
        fs::write(&file, b"abc").unwrap();
        let mut slot = slot(SetupKind::File);
        slot.accepted = vec![accepted(
            HashAlgorithm::Sha256,
            "941f887a562aae054fac35af8cc8f27cf075f3d4cc2e029fb5ae2a663aaa5ae7",
            Some("CUSA03173 01.09"),
        )];
        let error = check_choice(
            Some(&file),
            &slot,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(error, SupplyError::WrongVersion { .. }));
        assert!(error.to_string().contains("CUSA03173 01.09"), "{error}");
    }

    #[test]
    fn a_folder_must_hold_every_marker_and_its_hash_file_must_match() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("CUSA03173");
        fs::create_dir_all(folder.join("sce_sys")).unwrap();
        fs::write(folder.join("sce_sys/param.sfo"), b"sfo").unwrap();
        fs::write(folder.join("eboot.bin"), b"abc").unwrap();

        let mut slot = slot(SetupKind::Folder);
        slot.contains = vec!["eboot.bin".into(), "sce_sys/param.sfo".into()];
        slot.hash_file = Some("eboot.bin".into());
        slot.accepted = vec![accepted(
            HashAlgorithm::Sha1,
            ABC_SHA1,
            Some("CUSA03173 01.09"),
        )];
        let outcome = check_choice(
            Some(&folder),
            &slot,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let CheckOutcome::Accepted(choice) = outcome else {
            panic!("the folder matches");
        };
        assert_eq!(choice.kind, SetupKind::Folder);
        assert_eq!(choice.marker.as_deref(), Some("eboot.bin"));

        // Without `hash_file` the first marker is hashed.
        slot.hash_file = None;
        slot.contains = vec!["eboot.bin".into()];
        assert!(matches!(
            check_choice(
                Some(&folder),
                &slot,
                &mut no_progress(),
                &AtomicBool::new(false)
            ),
            Ok(CheckOutcome::Accepted(_))
        ));
    }

    #[test]
    fn a_folder_missing_a_marker_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("CUSA03173");
        fs::create_dir_all(folder.join("sce_sys")).unwrap();
        fs::write(folder.join("eboot.bin"), b"abc").unwrap();
        let mut slot = slot(SetupKind::Folder);
        slot.contains = vec!["eboot.bin".into(), "sce_sys/param.sfo".into()];

        let error = check_choice(
            Some(&folder),
            &slot,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(
            error,
            SupplyError::MissingMarker {
                slot: "game".into(),
                marker: "sce_sys/param.sfo".into(),
                folder: folder.display().to_string(),
            }
        );
    }

    #[test]
    fn a_package_folder_picked_in_folder_mode_names_the_package_option() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Bloodborne dump");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("CUSA03173-base.pkg"), b"package").unwrap();
        fs::write(folder.join("CUSA03173-update.pkg"), b"update").unwrap();
        let mut slot = slot(SetupKind::Folder);
        slot.contains = vec!["eboot.bin".into()];
        slot.from_package = Some(FromPackage {
            extractor: "ps4-pkg".into(),
            title_id: None,
            needs_update: Some("01.09".into()),
        });

        let error = check_choice(
            Some(&folder),
            &slot,
            &mut no_progress(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(
            error,
            SupplyError::PackagesInFolder {
                slot: "game".into(),
                folder: folder.display().to_string(),
            }
        );

        // A slot that cannot extract packages keeps the missing-marker message.
        slot.from_package = None;
        assert!(matches!(
            check_choice(
                Some(&folder),
                &slot,
                &mut no_progress(),
                &AtomicBool::new(false)
            ),
            Err(SupplyError::MissingMarker { .. })
        ));
    }

    #[test]
    fn optional_slots_can_be_skipped_or_absent() {
        let mut optional = slot(SetupKind::File);
        optional.required = false;
        optional.accepted = vec![accepted(HashAlgorithm::Sha256, ABC_SHA256, None)];
        assert_eq!(
            check_choice(None, &optional, &mut no_progress(), &AtomicBool::new(false)),
            Ok(CheckOutcome::Skipped)
        );

        let dir = tempfile::tempdir().unwrap();
        let absent = dir.path().join("gone.iso");
        assert_eq!(
            check_choice(
                Some(&absent),
                &optional,
                &mut no_progress(),
                &AtomicBool::new(false)
            ),
            Ok(CheckOutcome::Skipped)
        );

        let required = slot(SetupKind::File);
        assert!(matches!(
            check_choice(None, &required, &mut no_progress(), &AtomicBool::new(false)),
            Err(SupplyError::MissingChoice { .. })
        ));
    }

    #[test]
    fn cancellation_stops_the_hash() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dump.iso");
        fs::write(&file, vec![7u8; 64 * 1024]).unwrap();
        let mut slot = slot(SetupKind::File);
        slot.accepted = vec![accepted(HashAlgorithm::Sha256, ABC_SHA256, None)];

        assert_eq!(
            hash_file(&file, &mut no_progress(), &AtomicBool::new(true)),
            Err(SupplyError::Cancelled)
        );

        // Cancelling from a progress callback stops at the next chunk.
        let cancelled = AtomicBool::new(false);
        let error = hash_file(
            &file,
            &mut |_| cancelled.store(true, Ordering::Relaxed),
            &cancelled,
        )
        .unwrap_err();
        assert_eq!(error, SupplyError::Cancelled);
    }
}
