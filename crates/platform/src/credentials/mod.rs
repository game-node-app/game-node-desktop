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

use std::sync::Arc;

use gamenode_core::ports::CredentialStore;

#[cfg(unix)]
pub use file::FileCredentialStore;
pub use keyring::{BackendError, KeyringBackend, KeyringCredentialStore};

use crate::paths::DataPaths;

/// Stable keyring service name.
pub const KEYRING_SERVICE: &str = "app.gamenode.desktop";
/// Stable keyring account name.
pub const KEYRING_ACCOUNT: &str = "session";

/// Picks the credential store for this OS and machine.
///
/// On Linux, a Secret Service that cannot be reached at all means "absent" and selects the
/// private-file fallback; everything else (locked, denied) is an error from the keyring store.
#[cfg(target_os = "linux")]
pub fn open_store(paths: &DataPaths) -> Arc<dyn CredentialStore> {
    use gamenode_core::StorageMethod;
    use zbus_secret_service_keyring_store::Store;

    select_linux(
        || {
            let store = Store::new().map_err(|_| Absent)?;
            keyring::os_backend(store, KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|_| Absent)
        },
        paths,
        StorageMethod::SecretService,
    )
}

/// The Linux selection rule, separated from the real D-Bus probe so it can be tested.
#[cfg(unix)]
pub(crate) fn select_linux(
    probe: impl FnOnce() -> Result<Arc<dyn KeyringBackend>, Absent>,
    paths: &DataPaths,
    method: gamenode_core::StorageMethod,
) -> Arc<dyn CredentialStore> {
    match probe() {
        Ok(backend) => Arc::new(KeyringCredentialStore::new(backend, method)),
        Err(Absent) => {
            tracing::info!("no Secret Service found; using the private credential file");
            Arc::new(FileCredentialStore::new(
                paths.private_dir(),
                paths.credential_file(),
            ))
        }
    }
}

/// No Secret Service implementation could be reached.
#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Absent;

#[cfg(windows)]
pub fn open_store(_paths: &DataPaths) -> Arc<dyn CredentialStore> {
    use gamenode_core::StorageMethod;

    match windows_native_keyring_store::Store::new() {
        Ok(store) => match keyring::os_backend(store, KEYRING_SERVICE, KEYRING_ACCOUNT) {
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
