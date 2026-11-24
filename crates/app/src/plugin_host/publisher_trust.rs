//! User-approved publisher key pinning for third-party plugins.
//!
//! A valid package signature proves only that the package was signed by the
//! presented key. It does not identify the publisher as official or trusted.
//! The first key and every replacement key require an explicit user decision.
//!
//! This store performs synchronous filesystem work; callers must use a worker
//! thread when opening or updating it from async/UI code.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use thiserror::Error;

const FILE_NAME: &str = "publisher-keys.json";
const MAX_STORE_BYTES: u64 = 1024 * 1024;
/// How many replaced fingerprints one plugin's trust entry remembers for a reviewed data
/// migration. The oldest are dropped first.
const MAX_PRIOR_FINGERPRINTS: usize = 8;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Error)]
pub enum TrustStoreError {
    #[error("publisher identity requires explicit user confirmation")]
    ConfirmationRequired,
    #[error("publisher key changed; explicit re-trust is required")]
    KeyChanged,
    #[error("publisher key fingerprint does not match the confirmation")]
    FingerprintMismatch,
    #[error("plugin identity is invalid")]
    InvalidPluginId,
    #[error("Ed25519 public key must contain exactly 32 bytes")]
    InvalidPublicKey,
    #[error("publisher trust store is invalid: {0}")]
    InvalidStore(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyStatus {
    Untrusted {
        presented_fingerprint: String,
    },
    Trusted {
        fingerprint: String,
    },
    Changed {
        pinned_fingerprint: String,
        presented_fingerprint: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoreFile {
    schema: u32,
    publishers: BTreeMap<String, PinnedPublisher>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PinnedPublisher {
    fingerprint: String,
    /// Fingerprints this plugin was pinned to before the current one. Re-trust records the key it
    /// replaced so the host can offer a reviewed data migration from the old identity. Schema 1
    /// entries have no history.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    prior_fingerprints: Vec<String>,
}

/// Durable map from external plugin IDs to user-pinned Ed25519 key fingerprints.
pub struct PublisherTrustStore {
    path: PathBuf,
    publishers: BTreeMap<String, PinnedPublisher>,
}

impl PublisherTrustStore {
    /// Opens `<private_dir>/publisher-keys.json`, creating an empty store if absent.
    /// The directory and file are restricted to the current user on Unix.
    pub fn open(private_dir: impl AsRef<Path>) -> Result<Self, TrustStoreError> {
        let directory = private_dir.as_ref();
        ensure_private_dir(directory)?;
        let path = directory.join(FILE_NAME);
        let publishers = match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_file() => {
                if meta.len() > MAX_STORE_BYTES {
                    return Err(TrustStoreError::InvalidStore(
                        "file exceeds the size limit".into(),
                    ));
                }
                ensure_private_file(&path)?;
                let file = File::open(&path)?;
                let mut bytes = Vec::with_capacity(meta.len() as usize);
                file.take(MAX_STORE_BYTES + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_STORE_BYTES {
                    return Err(TrustStoreError::InvalidStore(
                        "file exceeds the size limit".into(),
                    ));
                }
                let document: StoreFile = serde_json::from_slice(&bytes)
                    .map_err(|error| TrustStoreError::InvalidStore(error.to_string()))?;
                if !(1..=2).contains(&document.schema) {
                    return Err(TrustStoreError::InvalidStore(
                        "unsupported schema version".into(),
                    ));
                }
                for (plugin_id, publisher) in &document.publishers {
                    validate_plugin_id(plugin_id)?;
                    if !is_fingerprint(&publisher.fingerprint)
                        || publisher
                            .prior_fingerprints
                            .iter()
                            .any(|prior| !is_fingerprint(prior))
                    {
                        return Err(TrustStoreError::InvalidStore(
                            "malformed publisher fingerprint".into(),
                        ));
                    }
                }
                document.publishers
            }
            Ok(_) => {
                return Err(TrustStoreError::InvalidStore(
                    "store path is not a regular file".into(),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self { path, publishers })
    }

    /// Computes the user-visible status for a candidate signing key.
    pub fn inspect(
        &self,
        plugin_id: &str,
        public_key: &[u8],
    ) -> Result<KeyStatus, TrustStoreError> {
        validate_plugin_id(plugin_id)?;
        let presented_fingerprint = fingerprint(public_key)?;
        Ok(match self.publishers.get(plugin_id) {
            None => KeyStatus::Untrusted {
                presented_fingerprint,
            },
            Some(pinned) if pinned.fingerprint == presented_fingerprint => KeyStatus::Trusted {
                fingerprint: presented_fingerprint,
            },
            Some(pinned) => KeyStatus::Changed {
                pinned_fingerprint: pinned.fingerprint.clone(),
                presented_fingerprint,
            },
        })
    }

    /// Pins a first key only when the user confirms the exact displayed fingerprint.
    pub fn confirm_first_key(
        &mut self,
        plugin_id: &str,
        public_key: &[u8],
        confirmed_fingerprint: &str,
    ) -> Result<String, TrustStoreError> {
        validate_plugin_id(plugin_id)?;
        let candidate = fingerprint(public_key)?;
        if self.publishers.contains_key(plugin_id) {
            return Err(TrustStoreError::KeyChanged);
        }
        if !constant_time_eq(candidate.as_bytes(), confirmed_fingerprint.as_bytes()) {
            return Err(TrustStoreError::FingerprintMismatch);
        }
        let mut next = self.publishers.clone();
        next.insert(
            plugin_id.to_owned(),
            PinnedPublisher {
                fingerprint: candidate.clone(),
                prior_fingerprints: Vec::new(),
            },
        );
        self.persist(&next)?;
        self.publishers = next;
        Ok(candidate)
    }

    /// Replaces a pinned key only after explicit confirmation of its new fingerprint. The key it
    /// replaces is remembered for a reviewed data migration, never trusted automatically.
    pub fn retrust_key(
        &mut self,
        plugin_id: &str,
        public_key: &[u8],
        confirmed_new_fingerprint: &str,
    ) -> Result<String, TrustStoreError> {
        validate_plugin_id(plugin_id)?;
        let candidate = fingerprint(public_key)?;
        let Some(previous) = self.publishers.get(plugin_id) else {
            return Err(TrustStoreError::ConfirmationRequired);
        };
        if !constant_time_eq(candidate.as_bytes(), confirmed_new_fingerprint.as_bytes()) {
            return Err(TrustStoreError::FingerprintMismatch);
        }
        let mut next = self.publishers.clone();
        next.insert(
            plugin_id.to_owned(),
            PinnedPublisher {
                fingerprint: candidate.clone(),
                prior_fingerprints: remember_prior(previous, &candidate),
            },
        );
        self.persist(&next)?;
        self.publishers = next;
        Ok(candidate)
    }

    /// Pins one key for several plugin ids in a single write. A bundle is one distribution unit
    /// with one publisher, so one fingerprint confirmation covers every component; a failure
    /// leaves no component pinned. Existing pins are replaced the same way `retrust_key` replaces
    /// one, because the review already showed the change and the user confirmed the new value.
    pub fn confirm_publishers(
        &mut self,
        plugin_ids: &[String],
        public_key: &[u8],
        confirmed_fingerprint: &str,
    ) -> Result<String, TrustStoreError> {
        if plugin_ids.is_empty() {
            return Err(TrustStoreError::InvalidPluginId);
        }
        let candidate = fingerprint(public_key)?;
        if !constant_time_eq(candidate.as_bytes(), confirmed_fingerprint.as_bytes()) {
            return Err(TrustStoreError::FingerprintMismatch);
        }
        let mut next = self.publishers.clone();
        for plugin_id in plugin_ids {
            validate_plugin_id(plugin_id)?;
            let prior_fingerprints = next
                .get(plugin_id)
                .map(|previous| remember_prior(previous, &candidate))
                .unwrap_or_default();
            next.insert(
                plugin_id.clone(),
                PinnedPublisher {
                    fingerprint: candidate.clone(),
                    prior_fingerprints,
                },
            );
        }
        self.persist(&next)?;
        self.publishers = next;
        Ok(candidate)
    }

    /// Fingerprints the plugin was pinned to before its current key, oldest first. The host uses
    /// this only to offer a reviewed data migration; a prior key is never trusted automatically.
    pub fn prior_fingerprints(&self, plugin_id: &str) -> Vec<String> {
        self.publishers
            .get(plugin_id)
            .map(|publisher| publisher.prior_fingerprints.clone())
            .unwrap_or_default()
    }

    /// Returns whether a candidate key is exactly the key the user approved.
    pub fn is_pinned(&self, plugin_id: &str, public_key: &[u8]) -> Result<bool, TrustStoreError> {
        Ok(matches!(
            self.inspect(plugin_id, public_key)?,
            KeyStatus::Trusted { .. }
        ))
    }

    fn persist(
        &self,
        publishers: &BTreeMap<String, PinnedPublisher>,
    ) -> Result<(), TrustStoreError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| TrustStoreError::InvalidStore("store has no parent directory".into()))?;
        ensure_private_dir(parent)?;
        let data = serde_json::to_vec(&StoreFile {
            schema: 2,
            publishers: publishers.clone(),
        })
        .map_err(|error| TrustStoreError::InvalidStore(error.to_string()))?;
        if data.len() as u64 > MAX_STORE_BYTES {
            return Err(TrustStoreError::InvalidStore(
                "serialized store exceeds the size limit".into(),
            ));
        }
        let temp = parent.join(format!(
            ".publisher-keys-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        let write_result = (|| -> io::Result<()> {
            file.write_all(&data)?;
            file.sync_all()?;
            atomic_replace(&temp, &self.path)?;
            sync_dir(parent)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        write_result?;
        ensure_private_file(&self.path)?;
        Ok(())
    }
}

fn remember_prior(previous: &PinnedPublisher, new_fingerprint: &str) -> Vec<String> {
    let mut prior = previous.prior_fingerprints.clone();
    if !prior.contains(&previous.fingerprint) {
        prior.push(previous.fingerprint.clone());
    }
    prior.retain(|fingerprint| fingerprint != new_fingerprint);
    if prior.len() > MAX_PRIOR_FINGERPRINTS {
        let excess = prior.len() - MAX_PRIOR_FINGERPRINTS;
        prior.drain(0..excess);
    }
    prior
}

fn fingerprint(public_key: &[u8]) -> Result<String, TrustStoreError> {
    if public_key.len() != 32 {
        return Err(TrustStoreError::InvalidPublicKey);
    }
    let digest = Sha256::digest(public_key);
    let mut output = String::with_capacity(71);
    output.push_str("SHA256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing into a String cannot fail");
    }
    Ok(output)
}

/// Returns the canonical, user-visible fingerprint for a publisher's Ed25519 public key.
pub fn key_fingerprint(public_key: &[u8]) -> String {
    fingerprint(public_key).unwrap_or_default()
}

fn is_fingerprint(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("SHA256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_plugin_id(plugin_id: &str) -> Result<(), TrustStoreError> {
    if plugin_id.is_empty()
        || plugin_id.len() > 256
        || !plugin_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
    {
        return Err(TrustStoreError::InvalidPluginId);
    }
    Ok(())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

#[cfg(unix)]
fn ensure_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe trust directory",
        )),
        Ok(meta) => {
            if meta.permissions().mode() & 0o077 != 0 {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new().mode(0o700).create(path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn ensure_private_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe trust directory",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(path),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn ensure_private_file(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe trust file",
        ));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_file(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe trust file",
        ));
    }
    Ok(())
}

fn sync_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn atomic_replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let from = from
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let to = to
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: both paths are NUL-terminated UTF-16 buffers valid for this call.
        let result = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        fs::rename(from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn first_key_requires_exact_user_confirmation_and_survives_reopen() {
        let directory = tempdir().unwrap();
        let key = [7; 32];
        let mut store = PublisherTrustStore::open(directory.path()).unwrap();
        let fingerprint = match store.inspect("org.example.plugin", &key).unwrap() {
            KeyStatus::Untrusted {
                presented_fingerprint,
            } => presented_fingerprint,
            other => panic!("unexpected status: {other:?}"),
        };
        assert!(matches!(
            store.confirm_first_key("org.example.plugin", &key, "wrong"),
            Err(TrustStoreError::FingerprintMismatch)
        ));
        store
            .confirm_first_key("org.example.plugin", &key, &fingerprint)
            .unwrap();
        drop(store);
        let store = PublisherTrustStore::open(directory.path()).unwrap();
        assert!(store.is_pinned("org.example.plugin", &key).unwrap());
        assert!(matches!(
            store.inspect("org.example.plugin", &[8; 32]).unwrap(),
            KeyStatus::Changed { .. }
        ));
    }

    #[test]
    fn key_rotation_requires_explicit_retrust_and_exact_new_fingerprint() {
        let directory = tempdir().unwrap();
        let old = [1; 32];
        let new = [2; 32];
        let mut store = PublisherTrustStore::open(directory.path()).unwrap();
        let old_fingerprint = match store.inspect("org.example.plugin", &old).unwrap() {
            KeyStatus::Untrusted {
                presented_fingerprint,
            } => presented_fingerprint,
            _ => unreachable!(),
        };
        store
            .confirm_first_key("org.example.plugin", &old, &old_fingerprint)
            .unwrap();
        assert!(matches!(
            store.confirm_first_key("org.example.plugin", &new, "any"),
            Err(TrustStoreError::KeyChanged)
        ));
        assert!(matches!(
            store.retrust_key("org.example.plugin", &new, "wrong"),
            Err(TrustStoreError::FingerprintMismatch)
        ));
        let new_fingerprint = match store.inspect("org.example.plugin", &new).unwrap() {
            KeyStatus::Changed {
                presented_fingerprint,
                ..
            } => presented_fingerprint,
            _ => unreachable!(),
        };
        store
            .retrust_key("org.example.plugin", &new, &new_fingerprint)
            .unwrap();
        assert!(store.is_pinned("org.example.plugin", &new).unwrap());
        assert!(!store.is_pinned("org.example.plugin", &old).unwrap());
        assert_eq!(
            store.prior_fingerprints("org.example.plugin"),
            vec![old_fingerprint.clone()]
        );
        drop(store);

        // The replaced key is remembered across reopening so a reviewed migration can be offered.
        let store = PublisherTrustStore::open(directory.path()).unwrap();
        assert_eq!(
            store.prior_fingerprints("org.example.plugin"),
            vec![old_fingerprint.clone()]
        );
        assert!(store.prior_fingerprints("org.example.unknown").is_empty());
        assert_eq!(
            store.inspect("org.example.plugin", &new).unwrap(),
            KeyStatus::Trusted {
                fingerprint: new_fingerprint,
            }
        );
    }

    #[test]
    fn a_schema_one_store_still_loads_without_migration_history() {
        let directory = tempdir().unwrap();
        let key = [3; 32];
        let fingerprint = key_fingerprint(&key);
        fs::write(
            directory.path().join(FILE_NAME),
            format!(r#"{{"schema":1,"publishers":{{"org.example.plugin":{{"fingerprint":"{fingerprint}"}}}}}}"#),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                directory.path().join(FILE_NAME),
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
        let store = PublisherTrustStore::open(directory.path()).unwrap();
        assert!(store.is_pinned("org.example.plugin", &key).unwrap());
        assert!(store.prior_fingerprints("org.example.plugin").is_empty());
    }

    #[test]
    fn repeated_retrust_keeps_a_bounded_oldest_first_history() {
        let directory = tempdir().unwrap();
        let mut store = PublisherTrustStore::open(directory.path()).unwrap();
        let first = [1; 32];
        let first_fingerprint = key_fingerprint(&first);
        store
            .confirm_first_key("org.example.plugin", &first, &first_fingerprint)
            .unwrap();
        let mut previous = first;
        let mut expected = Vec::new();
        for byte in 2..=11u8 {
            let next = [byte; 32];
            store
                .retrust_key("org.example.plugin", &next, &key_fingerprint(&next))
                .unwrap();
            expected.push(key_fingerprint(&previous));
            previous = next;
        }
        let history = store.prior_fingerprints("org.example.plugin");
        assert_eq!(history.len(), MAX_PRIOR_FINGERPRINTS);
        assert_eq!(history, expected[expected.len() - MAX_PRIOR_FINGERPRINTS..]);
        assert!(!history.contains(&key_fingerprint(&previous)));
    }

    #[test]
    fn refuses_symlink_and_malformed_stores() {
        let directory = tempdir().unwrap();
        let store_path = directory.path().join(FILE_NAME);
        fs::write(&store_path, br#"{"schema":9,"publishers":{}}"#).unwrap();
        assert!(matches!(
            PublisherTrustStore::open(directory.path()),
            Err(TrustStoreError::InvalidStore(_))
        ));
        fs::remove_file(&store_path).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(directory.path().join("missing"), &store_path).unwrap();
            assert!(PublisherTrustStore::open(directory.path()).is_err());
        }
    }
}
