//! Session credential storage.
//!
//! * Windows: Credential Manager. No file fallback.
//! * Linux: Secret Service. Only when no Secret Service exists is a private file used
//!   (`file` module); a locked or failing service is reported, never bypassed.
//!
//! The credential is stored under the stable service/account key below.

#[cfg(unix)]
mod file;
mod keyring;

use std::path::PathBuf;
use std::sync::Arc;

use gamenode_core::ports::CredentialStore;

#[cfg(unix)]
pub use file::FileCredentialStore;
pub use keyring::{BackendError, KeyringBackend, KeyringCredentialStore};

use crate::paths::DataPaths;

/// Stable keyring service name.
pub const KEYRING_SERVICE: &str = "app.gamenode.desktop";
/// Stable keyring account name for the GameNode session.
pub const KEYRING_ACCOUNT: &str = "session";
/// Stable keyring account name for the Steam session cookie.
pub const KEYRING_ACCOUNT_STEAM: &str = "steam";

/// Which stored secret a credential store is for.
///
/// The GameNode session and the Steam store session are separate credentials with separate
/// keyring entries and separate Linux private files. Deleting one never touches the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialSlot {
    /// The GameNode account session token.
    Session,
    /// The Steam browser session cookie header.
    Steam,
}

impl CredentialSlot {
    /// The keyring account name used by Windows Credential Manager and Secret Service.
    pub const fn keyring_account(self) -> &'static str {
        match self {
            Self::Session => KEYRING_ACCOUNT,
            Self::Steam => KEYRING_ACCOUNT_STEAM,
        }
    }

    /// The Linux private-file fallback path for this slot.
    pub fn private_file(self, paths: &DataPaths) -> PathBuf {
        match self {
            Self::Session => paths.credential_file(),
            Self::Steam => paths.steam_credential_file(),
        }
    }
}

/// Picks the credential store for this OS, machine and credential slot.
///
/// On Linux, a Secret Service that cannot be reached at all means "absent" and selects the
/// private-file fallback; everything else (locked, denied) is an error from the keyring store.
#[cfg(target_os = "linux")]
pub fn open_store(paths: &DataPaths, slot: CredentialSlot) -> Arc<dyn CredentialStore> {
    use gamenode_core::StorageMethod;
    use zbus_secret_service_keyring_store::Store;

    select_linux(
        || {
            let store = Store::new().map_err(|_| Absent)?;
            keyring::os_backend(store, KEYRING_SERVICE, slot.keyring_account()).map_err(|_| Absent)
        },
        paths,
        StorageMethod::SecretService,
        slot,
    )
}

/// The Linux selection rule, separated from the real D-Bus probe so it can be tested.
#[cfg(unix)]
pub(crate) fn select_linux(
    probe: impl FnOnce() -> Result<Arc<dyn KeyringBackend>, Absent>,
    paths: &DataPaths,
    method: gamenode_core::StorageMethod,
    slot: CredentialSlot,
) -> Arc<dyn CredentialStore> {
    match probe() {
        Ok(backend) => Arc::new(KeyringCredentialStore::new(backend, method)),
        Err(Absent) => {
            tracing::info!(
                slot = ?slot,
                "no Secret Service found; using the private credential file"
            );
            Arc::new(FileCredentialStore::new(
                paths.private_dir(),
                slot.private_file(paths),
            ))
        }
    }
}

/// No Secret Service implementation could be reached.
#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Absent;

#[cfg(windows)]
pub fn open_store(_paths: &DataPaths, slot: CredentialSlot) -> Arc<dyn CredentialStore> {
    use gamenode_core::StorageMethod;

    match windows_native_keyring_store::Store::new() {
        Ok(store) => match keyring::os_backend(store, KEYRING_SERVICE, slot.keyring_account()) {
            Ok(backend) => Arc::new(KeyringCredentialStore::new(
                backend,
                StorageMethod::WindowsCredentialManager,
            )),
            Err(_) => Arc::new(keyring::Unavailable(
                StorageMethod::WindowsCredentialManager,
            )),
        },
        Err(_) => Arc::new(keyring::Unavailable(
            StorageMethod::WindowsCredentialManager,
        )),
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("GameNode Desktop supports Windows and Linux only");

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use gamenode_core::StorageMethod;
    use tempfile::TempDir;

    fn cred(value: &str) -> gamenode_core::SessionCredential {
        gamenode_core::SessionCredential::new(value).unwrap()
    }

    #[test]
    fn slots_name_their_keyring_accounts_and_private_files() {
        assert_eq!(CredentialSlot::Session.keyring_account(), "session");
        assert_eq!(CredentialSlot::Steam.keyring_account(), "steam");
        assert_ne!(
            CredentialSlot::Session.keyring_account(),
            CredentialSlot::Steam.keyring_account()
        );

        let dir = TempDir::new().unwrap();
        let paths = DataPaths::at(dir.path().join("data"));
        assert_eq!(
            CredentialSlot::Session.private_file(&paths),
            paths.credential_file()
        );
        assert_eq!(
            CredentialSlot::Steam.private_file(&paths),
            paths.steam_credential_file()
        );
        assert_ne!(
            CredentialSlot::Session.private_file(&paths),
            CredentialSlot::Steam.private_file(&paths)
        );
    }

    #[test]
    fn the_file_fallback_keeps_both_slots_independent() {
        let dir = TempDir::new().unwrap();
        let paths = DataPaths::at(dir.path().join("data"));
        let session = select_linux(
            || Err(Absent),
            &paths,
            StorageMethod::PrivateFile,
            CredentialSlot::Session,
        );
        let steam = select_linux(
            || Err(Absent),
            &paths,
            StorageMethod::PrivateFile,
            CredentialSlot::Steam,
        );
        session.save(&cred("gamenode-session")).unwrap();
        steam.save(&cred("steamLoginSecure=cookie")).unwrap();

        // Deleting one slot does not touch the other.
        steam.delete().unwrap();
        assert_eq!(
            session.load().unwrap().unwrap().expose(),
            "gamenode-session"
        );
        assert_eq!(steam.load().unwrap(), None);
        assert!(paths.credential_file().exists());
        assert!(!paths.steam_credential_file().exists());
    }
}
