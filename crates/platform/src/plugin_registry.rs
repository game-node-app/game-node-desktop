//! Per-user immutable plugin package versions and crash-safe active pointers.
//!
//! The low-level import helper never validates package signatures or executes installation
//! hooks. `install_signed` is the integrity-checked path for package candidates. All methods
//! perform blocking filesystem work and must run on a worker thread.

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const COMPLETE_FILE: &str = "registry-record.json";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledPluginVersion {
    pub plugin_id: String,
    pub version: String,
    pub package_path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct VersionRecord {
    schema: u32,
    plugin_id: String,
    version: String,
}

/// Persistent per-user package registry. Paths under `versions/` are create-once; activation
/// changes only an atomic pointer and never edits an installed version tree.
#[derive(Clone, Debug)]
pub struct PluginPackageRegistry {
    root: PathBuf,
}

impl PluginPackageRegistry {
    pub fn open(data_paths: &crate::paths::DataPaths) -> io::Result<Self> {
        Self::at(data_paths.plugin_packages_dir())
    }

    pub fn at(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        ensure_owner_dir(&root)?;
        let registry = Self { root };
        registry.recover()?;
        Ok(registry)
    }

    /// Imports an already verified directory as a new immutable version and optionally makes
    /// it active. A failed copy leaves the current active pointer untouched. The source tree is
    /// copied without following symlinks, and no source entry is executed.
    pub fn install_verified(
        &self,
        plugin_id: &str,
        version: &str,
        verified_source: &Path,
        activate: bool,
    ) -> io::Result<InstalledPluginVersion> {
        self.install_with_checkpoint(plugin_id, version, verified_source, activate, |_| Ok(()))
    }

    /// Verify the detached Ed25519 signature and complete payload before import, verify the
    /// copied immutable version again before activation, and leave any previous active version
    /// untouched if either verification fails. The caller must supply the publisher key pinned
    /// by the user's trust decision.
    pub fn install_signed(
        &self,
        plugin_id: &str,
        version: &str,
        package_source: &Path,
        publisher_key: &[u8],
        activate: bool,
    ) -> Result<InstalledPluginVersion, InstallSignedError> {
        crate::plugin_packages::verify_package_directory(package_source, publisher_key)
            .map_err(InstallSignedError::Integrity)?;
        let installed = self.install_verified(plugin_id, version, package_source, false)?;
        crate::plugin_packages::verify_installed_package(&installed.package_path, publisher_key)
            .map_err(InstallSignedError::Integrity)?;
        if activate {
            self.set_active(plugin_id, version)?;
        }
        Ok(installed)
    }

    /// Recheck the active installed version's signed files immediately before launch.
    pub fn verify_active_package(
        &self,
        plugin_id: &str,
        publisher_key: &[u8],
    ) -> Result<Option<InstalledPluginVersion>, InstallSignedError> {
        let Some(installed) = self.active(plugin_id)? else {
            return Ok(None);
        };
        crate::plugin_packages::verify_installed_package(&installed.package_path, publisher_key)
            .map_err(InstallSignedError::Integrity)?;
        Ok(Some(installed))
    }

    fn install_with_checkpoint<F>(
        &self,
        plugin_id: &str,
        version: &str,
        verified_source: &Path,
        activate: bool,
        mut checkpoint: F,
    ) -> io::Result<InstalledPluginVersion>
    where
        F: FnMut(InstallCheckpoint) -> io::Result<()>,
    {
        validate_identity(plugin_id)?;
        validate_identity(version)?;
        let source_meta = fs::symlink_metadata(verified_source)?;
        if !source_meta.is_dir() || source_meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "verified package source must be a real directory",
            ));
        }

        let plugin_key = encode_key(plugin_id);
        let version_key = encode_key(version);
        let plugin_dir = self.root.join("plugins").join(&plugin_key);
        let versions_dir = plugin_dir.join("versions");
        let final_path = versions_dir.join(&version_key);
        ensure_owner_dir(&versions_dir)?;
        sync_dir(&plugin_dir)?;
        sync_dir(&self.root.join("plugins"))?;
        sync_dir(&self.root)?;
        match fs::symlink_metadata(&final_path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "plugin version paths are immutable",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let staging = versions_dir.join(format!(
            ".staging-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        ensure_owner_dir(&staging)?;
        let result = (|| {
            let staged_package = staging.join("package");
            ensure_owner_dir(&staged_package)?;
            copy_tree_no_links(verified_source, &staged_package)?;
            sync_dir(&staged_package)?;
            checkpoint(InstallCheckpoint::PayloadCopied)?;
            let metadata = InstalledPluginVersion {
                plugin_id: plugin_id.into(),
                version: version.into(),
                package_path: final_path.join("package"),
            };
            let record = VersionRecord {
                schema: 1,
                plugin_id: plugin_id.into(),
                version: version.into(),
            };
            let metadata_bytes = serde_json::to_vec(&record).map_err(io::Error::other)?;
            write_new_synced(&staging.join(COMPLETE_FILE), &metadata_bytes)?;
            sync_dir(&staging)?;
            checkpoint(InstallCheckpoint::CompleteMarkerSynced)?;
            // A rename on the same filesystem publishes the fully synced directory atomically.
            fs::rename(&staging, &final_path)?;
            sync_dir(&versions_dir)?;
            checkpoint(InstallCheckpoint::VersionPublished)?;
            if activate {
                self.set_active(plugin_id, version)?;
            }
            Ok(metadata)
        })();
        if result.is_err() {
            // A staging tree is never visible as a version or active entry. Best-effort removal
            // is safe; if recovery is interrupted, open() removes the orphan on next start.
            let _ = remove_tree_no_links(&staging);
        }
        result
    }

    /// Atomically switches the active pointer to a complete immutable version.
    pub fn set_active(&self, plugin_id: &str, version: &str) -> io::Result<()> {
        validate_identity(plugin_id)?;
        validate_identity(version)?;
        let package = self.version_path(plugin_id, version);
        if self.read_complete(&package, plugin_id, version).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "cannot activate an incomplete or missing plugin version",
            ));
        }
        let active_dir = self.root.join("active");
        ensure_owner_dir(&active_dir)?;
        sync_dir(&self.root)?;
        let pointer = active_dir.join(encode_key(plugin_id));
        atomic_replace(&pointer, version.as_bytes())?;
        sync_dir(&active_dir)
    }

    pub fn active(&self, plugin_id: &str) -> io::Result<Option<InstalledPluginVersion>> {
        validate_identity(plugin_id)?;
        let pointer = self.root.join("active").join(encode_key(plugin_id));
        match fs::symlink_metadata(&pointer) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        }
        let version = match fs::read_to_string(pointer) {
            Ok(version) => version,
            Err(error) => return Err(error),
        };
        validate_identity(&version)?;
        Ok(self.read_complete(&self.version_path(plugin_id, &version), plugin_id, &version))
    }

    pub fn versions(&self, plugin_id: &str) -> io::Result<Vec<InstalledPluginVersion>> {
        validate_identity(plugin_id)?;
        let versions_dir = self
            .root
            .join("plugins")
            .join(encode_key(plugin_id))
            .join("versions");
        let mut versions = Vec::new();
        match fs::symlink_metadata(&versions_dir) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "plugin versions path is not a real directory",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(versions),
            Err(error) => return Err(error),
        }
        let entries = match fs::read_dir(versions_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(versions),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir()
                || entry.file_name().to_string_lossy().starts_with(".staging-")
            {
                continue;
            }
            if let Some(metadata) = self.read_complete(&entry.path(), plugin_id, "") {
                versions.push(metadata);
            }
        }
        versions.sort_by(|left, right| left.version.cmp(&right.version));
        Ok(versions)
    }

    /// Removes interrupted staging transactions and invalid active pointers. Valid old active
    /// pointers are never changed by an incomplete update; fully published but unactivated
    /// versions are retained for inspection or a later explicit activation.
    pub fn recover(&self) -> io::Result<()> {
        let plugins = self.root.join("plugins");
        if plugins.exists()
            && fs::symlink_metadata(&plugins)?.is_dir()
            && !fs::symlink_metadata(&plugins)?.file_type().is_symlink()
        {
            for plugin_entry in fs::read_dir(&plugins)? {
                let plugin_entry = plugin_entry?;
                if !plugin_entry.file_type()?.is_dir() {
                    continue;
                }
                let versions_dir = plugin_entry.path().join("versions");
                if !versions_dir.exists() {
                    continue;
                }
                let version_meta = fs::symlink_metadata(&versions_dir)?;
                if !version_meta.is_dir() || version_meta.file_type().is_symlink() {
                    continue;
                }
                for version_entry in fs::read_dir(&versions_dir)? {
                    let version_entry = version_entry?;
                    if version_entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".staging-")
                    {
                        remove_tree_no_links(&version_entry.path())?;
                    } else if version_entry.file_type()?.is_dir()
                        && self.read_record(&version_entry.path()).is_none()
                    {
                        remove_tree_no_links(&version_entry.path())?;
                    }
                }
            }
        }
        let active_dir = self.root.join("active");
        if !active_dir.exists() {
            return Ok(());
        }
        let active_meta = fs::symlink_metadata(&active_dir)?;
        if !active_meta.is_dir() || active_meta.file_type().is_symlink() {
            return Ok(());
        }
        for pointer_entry in fs::read_dir(&active_dir)? {
            let pointer_entry = pointer_entry?;
            if pointer_entry
                .file_name()
                .to_string_lossy()
                .starts_with(".pointer-")
            {
                if pointer_entry.file_type()?.is_file() {
                    fs::remove_file(pointer_entry.path())?;
                }
                continue;
            }
            if !pointer_entry.file_type()?.is_file() {
                if pointer_entry.file_type()?.is_symlink() {
                    fs::remove_file(pointer_entry.path())?;
                }
                continue;
            }
            let plugin_id = decode_key(&pointer_entry.file_name().to_string_lossy());
            let valid = plugin_id.as_deref().is_some_and(|plugin_id| {
                fs::read_to_string(pointer_entry.path())
                    .ok()
                    .and_then(|version| {
                        self.read_complete(
                            &self.version_path(plugin_id, &version),
                            plugin_id,
                            &version,
                        )
                    })
                    .is_some()
            });
            if !valid {
                fs::remove_file(pointer_entry.path())?;
            }
        }
        sync_dir(&self.root)
    }

    fn version_path(&self, plugin_id: &str, version: &str) -> PathBuf {
        self.root
            .join("plugins")
            .join(encode_key(plugin_id))
            .join("versions")
            .join(encode_key(version))
    }

    fn read_complete(
        &self,
        path: &Path,
        expected_plugin: &str,
        expected_version: &str,
    ) -> Option<InstalledPluginVersion> {
        let record = self.read_record(path)?;
        if record.plugin_id != expected_plugin
            || (!expected_version.is_empty() && record.version != expected_version)
        {
            return None;
        }
        Some(record)
    }

    fn read_record(&self, path: &Path) -> Option<InstalledPluginVersion> {
        let metadata = fs::symlink_metadata(path).ok()?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return None;
        }
        let marker = fs::symlink_metadata(path.join(COMPLETE_FILE)).ok()?;
        if !marker.is_file() || marker.file_type().is_symlink() {
            return None;
        }
        let file = File::open(path.join(COMPLETE_FILE)).ok()?;
        let record: VersionRecord = serde_json::from_reader(file).ok()?;
        if record.schema != 1 {
            return None;
        }
        Some(InstalledPluginVersion {
            plugin_id: record.plugin_id,
            version: record.version,
            package_path: {
                let package = path.join("package");
                let payload = fs::symlink_metadata(&package).ok()?;
                if !payload.is_dir() || payload.file_type().is_symlink() {
                    return None;
                }
                package
            },
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallSignedError {
    #[error("plugin package integrity validation failed: {0}")]
    Integrity(#[from] crate::plugin_packages::IntegrityError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstallCheckpoint {
    PayloadCopied,
    CompleteMarkerSynced,
    VersionPublished,
}

fn validate_identity(value: &str) -> io::Result<()> {
    if value.trim().is_empty()
        || value.len() > 256
        || value == "."
        || value == ".."
        || value.chars().any(char::is_control)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid plugin identity",
        ));
    }
    Ok(())
}

fn encode_key(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_key(value: &str) -> Option<String> {
    if value.is_empty() || value.len() % 2 != 0 {
        return None;
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

fn ensure_owner_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "registry path is not a real directory",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                ensure_owner_dir(parent)?;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new().mode(0o700).create(path)?;
            }
            #[cfg(not(unix))]
            fs::create_dir(path)?;
        }
        Err(error) => return Err(error),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(mode & !0o077))?;
        }
    }
    Ok(())
}

fn copy_tree_no_links(source: &Path, destination: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "symlink in verified package tree",
            ));
        }
        if metadata.is_dir() {
            ensure_owner_dir(&to)?;
            copy_tree_no_links(&from, &to)?;
            sync_dir(&to)?;
        } else if metadata.is_file() {
            let mut input = File::open(from)?;
            let mut output = OpenOptions::new().write(true).create_new(true).open(to)?;
            io::copy(&mut input, &mut output)?;
            output.sync_all()?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported filesystem entry in verified package tree",
            ));
        }
    }
    Ok(())
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pointer has no parent"))?;
    ensure_owner_dir(parent)?;
    let temp = parent.join(format!(
        ".pointer-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    write_new_synced(&temp, bytes)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let from = temp
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let to = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: both paths are NUL-terminated UTF-16 buffers valid for the call.
        let result = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result == 0 {
            let error = io::Error::last_os_error();
            let _ = fs::remove_file(temp);
            return Err(error);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if let Err(error) = fs::rename(&temp, path) {
            let _ = fs::remove_file(temp);
            return Err(error);
        }
        Ok(())
    }
}

fn sync_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn remove_tree_no_links(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        return fs::remove_file(path);
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected registry filesystem object",
        ));
    }
    for entry in fs::read_dir(path)? {
        remove_tree_no_links(&entry?.path())?;
    }
    fs::remove_dir(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Component;
    use tempfile::TempDir;

    fn source(temp: &TempDir, name: &str, content: &[u8]) -> PathBuf {
        let path = temp.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::write(path.join("plugin.bin"), content).unwrap();
        path
    }

    fn signed_source(temp: &TempDir, name: &str) -> (PathBuf, Vec<u8>) {
        use ring::signature::KeyPair;
        use sha2::Digest;
        let path = temp.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::write(path.join("plugin.bin"), b"signed bytes").unwrap();
        let files = std::collections::BTreeMap::from([(
            "plugin.bin".to_owned(),
            format!("{:x}", sha2::Sha256::digest(b"signed bytes")),
        )]);
        let raw = serde_json::to_vec(&serde_json::json!({"schema": 1, "files": files})).unwrap();
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[11u8; 32]).unwrap();
        let mut signed = crate::plugin_packages::SIGNATURE_DOMAIN_PREFIX.to_vec();
        signed.extend_from_slice(&raw);
        fs::write(path.join("integrity.json"), raw).unwrap();
        fs::write(path.join("signature.ed25519"), key.sign(&signed).as_ref()).unwrap();
        (path, key.public_key().as_ref().to_vec())
    }

    #[test]
    fn signed_install_checks_copy_and_active_payload_against_pinned_key() {
        let temp = TempDir::new().unwrap();
        let registry = PluginPackageRegistry::at(temp.path().join("registry")).unwrap();
        let (candidate, key) = signed_source(&temp, "signed");
        let installed = registry
            .install_signed("demo", "1", &candidate, &key, true)
            .unwrap();
        assert_eq!(
            registry.verify_active_package("demo", &key).unwrap(),
            Some(installed.clone())
        );
        fs::write(installed.package_path.join("plugin.bin"), b"tampered").unwrap();
        assert!(matches!(
            registry.verify_active_package("demo", &key),
            Err(InstallSignedError::Integrity(
                crate::plugin_packages::IntegrityError::HashMismatch
            ))
        ));
    }

    #[test]
    fn install_publishes_complete_version_then_switches_active_pointer() {
        let temp = TempDir::new().unwrap();
        let registry = PluginPackageRegistry::at(temp.path().join("registry")).unwrap();
        let one = source(&temp, "one", b"one");
        registry
            .install_verified("community.demo", "1.0.0", &one, true)
            .unwrap();
        let active = registry.active("community.demo").unwrap().unwrap();
        assert_eq!(active.version, "1.0.0");
        assert_eq!(
            fs::read(active.package_path.join("plugin.bin")).unwrap(),
            b"one"
        );
        assert!(!active.package_path.join(COMPLETE_FILE).exists());
    }

    #[test]
    fn restart_after_interrupted_copy_keeps_old_active_and_discards_staging() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("registry");
        let registry = PluginPackageRegistry::at(&root).unwrap();
        let one = source(&temp, "one", b"stable");
        registry.install_verified("demo", "1", &one, true).unwrap();
        let two = source(&temp, "two", b"partial candidate");
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = registry.install_with_checkpoint("demo", "2", &two, true, |stage| {
                if stage == InstallCheckpoint::PayloadCopied {
                    panic!("simulated process interruption");
                }
                Ok(())
            });
        }));
        assert!(interrupted.is_err());
        let restarted = PluginPackageRegistry::at(&root).unwrap();
        assert_eq!(restarted.active("demo").unwrap().unwrap().version, "1");
        assert_eq!(
            restarted
                .versions("demo")
                .unwrap()
                .iter()
                .map(|v| v.version.as_str())
                .collect::<Vec<_>>(),
            vec!["1"]
        );
    }

    #[test]
    fn restart_after_publish_before_pointer_swap_retains_old_active_and_new_inactive_version() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("registry");
        let registry = PluginPackageRegistry::at(&root).unwrap();
        let one = source(&temp, "one", b"stable");
        registry.install_verified("demo", "1", &one, true).unwrap();
        let two = source(&temp, "two", b"candidate");
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = registry.install_with_checkpoint("demo", "2", &two, true, |stage| {
                if stage == InstallCheckpoint::VersionPublished {
                    panic!("simulated process interruption");
                }
                Ok(())
            });
        }));
        assert!(interrupted.is_err());
        let restarted = PluginPackageRegistry::at(&root).unwrap();
        assert_eq!(restarted.active("demo").unwrap().unwrap().version, "1");
        assert_eq!(
            restarted
                .versions("demo")
                .unwrap()
                .iter()
                .map(|v| v.version.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "2"]
        );
        restarted.set_active("demo", "2").unwrap();
        assert_eq!(restarted.active("demo").unwrap().unwrap().version, "2");
    }

    #[test]
    fn first_interrupted_install_recovers_to_inactive_state() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("registry");
        let registry = PluginPackageRegistry::at(&root).unwrap();
        let candidate = source(&temp, "candidate", b"candidate");
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = registry
                .install_with_checkpoint("demo", "1", &candidate, true, |_| panic!("crash"));
        }));
        assert!(interrupted.is_err());
        let restarted = PluginPackageRegistry::at(&root).unwrap();
        assert!(restarted.active("demo").unwrap().is_none());
        assert!(restarted.versions("demo").unwrap().is_empty());
    }

    #[test]
    fn refuses_duplicate_version_paths_and_symlink_content() {
        let temp = TempDir::new().unwrap();
        let registry = PluginPackageRegistry::at(temp.path().join("registry")).unwrap();
        let one = source(&temp, "one", b"one");
        registry.install_verified("demo", "1", &one, false).unwrap();
        assert_eq!(
            registry
                .install_verified("demo", "1", &one, true)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(one.join("plugin.bin"), one.join("linked")).unwrap();
            assert!(registry.install_verified("demo", "2", &one, true).is_err());
            assert!(registry.active("demo").unwrap().is_none());
        }
    }

    #[test]
    fn data_paths_expose_persistent_registry_location() {
        let temp = TempDir::new().unwrap();
        let paths = crate::paths::DataPaths::at(temp.path());
        let registry = PluginPackageRegistry::open(&paths).unwrap();
        assert!(registry.root.starts_with(paths.data_dir()));
        assert_eq!(
            paths.plugin_packages_dir(),
            temp.path().join("plugin-packages")
        );
    }

    #[test]
    fn identity_path_encoding_is_traversal_safe_and_reversible() {
        let value = "../plugin\0id".replace('\0', "-zero-");
        let encoded = encode_key(&value);
        assert!(
            !Path::new(&encoded)
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        );
        assert_eq!(decode_key(&encoded).as_deref(), Some(value.as_str()));
        assert!(validate_identity("../plugin").is_ok());
    }
}
