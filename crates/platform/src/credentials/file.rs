//! Linux fallback: the session token in a file only the current user can read.
//!
//! Used only when no Secret Service exists. The directory is `0700` and the file `0600`; both
//! must be owned by the current user and must not be symlinks. Writes are atomic: a temporary
//! file created with `O_EXCL | O_NOFOLLOW` is synced and renamed over the target.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gamenode_core::ports::{CredentialError, CredentialStore};
use gamenode_core::{SessionCredential, StorageMethod};
use rustix::fs::OFlags;

const MAX_FILE_LEN: u64 = 8 * 1024;

pub struct FileCredentialStore {
    dir: PathBuf,
    file: PathBuf,
}

impl FileCredentialStore {
    pub fn new(dir: PathBuf, file: PathBuf) -> Self {
        Self { dir, file }
    }

    /// Verifies (creating if absent) the private directory.
    fn ensure_dir(&self, create: bool) -> Result<bool, CredentialError> {
        match fs::symlink_metadata(&self.dir) {
            Ok(meta) => {
                let uid = rustix::process::getuid().as_raw();
                if !meta.is_dir() || meta.uid() != uid {
                    return Err(CredentialError::Failed);
                }
                if meta.mode() & 0o077 != 0 {
                    fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
                        .map_err(|_| CredentialError::Failed)?;
                }
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !create {
                    return Ok(false);
                }
                if let Some(parent) = self.dir.parent() {
                    fs::create_dir_all(parent).map_err(|_| CredentialError::Failed)?;
                }
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&self.dir)
                    .map_err(|_| CredentialError::Failed)?;
                Ok(true)
            }
            Err(_) => Err(CredentialError::Failed),
        }
    }

    fn temp_path(&self) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = self
            .file
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("credential");
        self.dir.join(format!(
            ".{name}.{}.{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }
}

fn open_nofollow(path: &Path, options: &mut OpenOptions) -> std::io::Result<File> {
    options
        .custom_flags(OFlags::NOFOLLOW.bits() as i32)
        .open(path)
}

impl CredentialStore for FileCredentialStore {
    fn method(&self) -> StorageMethod {
        StorageMethod::PrivateFile
    }

    fn load(&self) -> Result<Option<SessionCredential>, CredentialError> {
        if !self.ensure_dir(false)? {
            return Ok(None);
        }
        let mut file = match open_nofollow(&self.file, OpenOptions::new().read(true)) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(CredentialError::Failed), // includes a symlink (ELOOP)
        };
        let meta = file.metadata().map_err(|_| CredentialError::Failed)?;
        if !meta.is_file()
            || meta.uid() != rustix::process::getuid().as_raw()
            || meta.len() > MAX_FILE_LEN
        {
            return Err(CredentialError::Failed);
        }
        if meta.mode() & 0o077 != 0 {
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| CredentialError::Failed)?;
        }
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|_| CredentialError::Failed)?;
        Ok(SessionCredential::new(text.trim_end_matches(['\r', '\n'])))
    }

    fn save(&self, credential: &SessionCredential) -> Result<(), CredentialError> {
        self.ensure_dir(true)?;
        let temp = self.temp_path();
        let result = (|| -> std::io::Result<()> {
            let mut file = open_nofollow(
                &temp,
                OpenOptions::new().write(true).create_new(true).mode(0o600),
            )?;
            file.write_all(credential.expose().as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &self.file)?;
            // Persist the rename itself.
            File::open(&self.dir)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
            return Err(CredentialError::Failed);
        }
        Ok(())
    }

    fn delete(&self) -> Result<(), CredentialError> {
        match fs::remove_file(&self.file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(CredentialError::Failed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{Absent, BackendError, CredentialSlot, KeyringBackend, select_linux};
    use crate::paths::DataPaths;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn store(dir: &TempDir) -> FileCredentialStore {
        let paths = DataPaths::at(dir.path().join("data"));
        FileCredentialStore::new(paths.private_dir(), paths.credential_file())
    }

    fn cred(v: &str) -> SessionCredential {
        SessionCredential::new(v).unwrap()
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().mode() & 0o777
    }

    #[test]
    fn creates_a_private_directory_and_file_and_round_trips() {
        let dir = TempDir::new().unwrap();
        let store = store(&dir);
        assert_eq!(store.load(), Ok(None));
        assert!(!store.dir.exists(), "reading does not create anything");

        store.save(&cred("tok.en+/=")).unwrap();
        assert_eq!(mode(&store.dir), 0o700);
        assert_eq!(mode(&store.file), 0o600);
        assert_eq!(store.load().unwrap().unwrap().expose(), "tok.en+/=");
        assert_eq!(store.method(), StorageMethod::PrivateFile);
        // No temporary file is left behind.
        let leftovers: Vec<_> = fs::read_dir(&store.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["session-token"]);
    }

    #[test]
    fn overwrite_replaces_atomically_and_delete_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let store = store(&dir);
        store.save(&cred("one")).unwrap();
        store.save(&cred("two")).unwrap();
        assert_eq!(store.load().unwrap().unwrap().expose(), "two");
        store.delete().unwrap();
        assert_eq!(store.load(), Ok(None));
        store.delete().unwrap();
    }

    #[test]
    fn too_open_permissions_are_tightened() {
        let dir = TempDir::new().unwrap();
        let store = store(&dir);
        store.save(&cred("tok")).unwrap();
        fs::set_permissions(&store.dir, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&store.file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.load().unwrap().is_some());
        assert_eq!((mode(&store.dir), mode(&store.file)), (0o700, 0o600));
    }

    #[test]
    fn slots_use_separate_files_and_temporary_names() {
        let dir = TempDir::new().unwrap();
        let paths = DataPaths::at(dir.path().join("data"));
        let session = FileCredentialStore::new(
            paths.private_dir(),
            CredentialSlot::Session.private_file(&paths),
        );
        let steam = FileCredentialStore::new(
            paths.private_dir(),
            CredentialSlot::Steam.private_file(&paths),
        );
        assert_eq!(session.file, paths.credential_file());
        assert_eq!(steam.file, paths.steam_credential_file());
        assert!(
            session
                .temp_path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".session-token.")
        );
        assert!(
            steam
                .temp_path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".steam-token."),
            "the Steam temp file is named after steam-token, not session-token"
        );

        session.save(&cred("gamenode")).unwrap();
        steam.save(&cred("steamLoginSecure=abc")).unwrap();
        assert_eq!(session.load().unwrap().unwrap().expose(), "gamenode");
        assert_eq!(
            steam.load().unwrap().unwrap().expose(),
            "steamLoginSecure=abc"
        );
        assert_eq!(mode(&paths.credential_file()), 0o600);
        assert_eq!(mode(&paths.steam_credential_file()), 0o600);

        // Deleting the Steam slot leaves the GameNode session alone.
        steam.delete().unwrap();
        assert_eq!(session.load().unwrap().unwrap().expose(), "gamenode");
        assert!(!paths.steam_credential_file().exists());
    }

    #[test]
    fn a_symlinked_steam_credential_is_refused() {
        let dir = TempDir::new().unwrap();
        let paths = DataPaths::at(dir.path().join("data"));
        let store = FileCredentialStore::new(
            paths.private_dir(),
            CredentialSlot::Steam.private_file(&paths),
        );
        paths.ensure_data_dir().unwrap();
        fs::create_dir(paths.private_dir()).unwrap();
        fs::set_permissions(paths.private_dir(), fs::Permissions::from_mode(0o700)).unwrap();
        let target = dir.path().join("elsewhere");
        fs::write(&target, "victim").unwrap();
        std::os::unix::fs::symlink(&target, paths.steam_credential_file()).unwrap();

        assert_eq!(store.load(), Err(CredentialError::Failed));
        store.save(&cred("fresh")).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "victim");
        assert_eq!(store.load().unwrap().unwrap().expose(), "fresh");
    }

    #[test]
    fn symlinks_are_never_followed() {
        let dir = TempDir::new().unwrap();
        let store = store(&dir);
        store.save(&cred("tok")).unwrap();

        // A symlinked credential file is refused, and saving replaces the link, not its target.
        let target = dir.path().join("elsewhere");
        fs::write(&target, "victim").unwrap();
        fs::remove_file(&store.file).unwrap();
        std::os::unix::fs::symlink(&target, &store.file).unwrap();
        assert_eq!(store.load(), Err(CredentialError::Failed));
        store.save(&cred("fresh")).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "victim");
        assert_eq!(store.load().unwrap().unwrap().expose(), "fresh");

        // A symlinked private directory is refused.
        let real = dir.path().join("real-dir");
        fs::create_dir(&real).unwrap();
        let other = TempDir::new().unwrap();
        let linked = FileCredentialStore::new(
            other.path().join("private"),
            other.path().join("private/session-token"),
        );
        std::os::unix::fs::symlink(&real, &linked.dir).unwrap();
        assert_eq!(linked.save(&cred("x")), Err(CredentialError::Failed));
        assert_eq!(linked.load(), Err(CredentialError::Failed));
        assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
    }

    #[test]
    fn oversized_files_are_rejected() {
        let dir = TempDir::new().unwrap();
        let store = store(&dir);
        store.save(&cred("tok")).unwrap();
        fs::write(&store.file, vec![b'a'; 9000]).unwrap();
        assert_eq!(store.load(), Err(CredentialError::Failed));
    }

    struct Locked;
    impl KeyringBackend for Locked {
        fn get(&self) -> Result<Option<String>, BackendError> {
            Err(BackendError::Locked)
        }
        fn set(&self, _: &str) -> Result<(), BackendError> {
            Err(BackendError::Locked)
        }
        fn delete(&self) -> Result<(), BackendError> {
            Err(BackendError::Locked)
        }
    }

    #[test]
    fn only_an_absent_service_selects_the_file_and_a_locked_one_does_not() {
        let dir = TempDir::new().unwrap();
        let paths = DataPaths::at(dir.path().join("data"));

        let absent = select_linux(
            || Err(Absent),
            &paths,
            StorageMethod::SecretService,
            CredentialSlot::Session,
        );
        assert_eq!(absent.method(), StorageMethod::PrivateFile);

        let locked = select_linux(
            || Ok(Arc::new(Locked) as Arc<dyn KeyringBackend>),
            &paths,
            StorageMethod::SecretService,
            CredentialSlot::Steam,
        );
        assert_eq!(locked.method(), StorageMethod::SecretService);
        assert_eq!(locked.save(&cred("x")), Err(CredentialError::Locked));
        assert!(
            !paths.private_dir().exists(),
            "a locked service never creates the fallback file"
        );
    }
}
