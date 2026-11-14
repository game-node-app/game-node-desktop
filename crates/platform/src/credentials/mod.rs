//! Session and provider credential storage.
//!
//! * Windows: Credential Manager. No file fallback.
//! * Linux: Secret Service. Only when no Secret Service exists is a private file used
//!   (`file` module); a locked or failing service is reported, never bypassed.
//!
//! Each provider's plugin sees one logical `session` secret. The host maps it to that provider's
//! physical slot, derived from the provider id unless trusted descriptor data names an alias.

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

/// Existing physical names for a provider credential.
///
/// Only trusted descriptor data supplies an alias. The Steam descriptor keeps its historical
/// `steam` keyring account and `steam-token` private file, so no credential migration is needed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialAlias {
    pub keyring_account: String,
    pub private_file: String,
}

impl CredentialAlias {
    /// A plain keyring account and file name that does not collide with the GameNode session.
    pub fn is_valid(&self) -> bool {
        plain_name(&self.keyring_account)
            && plain_name(&self.private_file)
            && self.keyring_account != KEYRING_ACCOUNT
            && self.private_file != crate::paths::CREDENTIAL_FILE
    }
}

fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// Which stored secret a credential store is for.
///
/// The GameNode session and provider credentials are separate secrets with separate keyring
/// entries and separate Linux private files. Deleting one never touches another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialSlot {
    /// The GameNode account session token.
    Session,
    /// A provider's own credential, derived from its provider id or descriptor alias.
    Provider {
        keyring_account: String,
        private_file: String,
    },
}

impl CredentialSlot {
    /// The logical slot for a provider. The default keyring account is `provider.<provider id>`
    /// and the Linux private file is `provider-<provider id>.secret`.
    pub fn provider(provider_id: &str, alias: Option<CredentialAlias>) -> Self {
        match alias {
            Some(alias) => Self::Provider {
                keyring_account: alias.keyring_account,
                private_file: alias.private_file,
            },
            None => Self::Provider {
                keyring_account: format!("provider.{provider_id}"),
                private_file: format!("provider-{provider_id}.secret"),
            },
        }
    }

    /// The keyring account name used by Windows Credential Manager and Secret Service.
    pub fn keyring_account(&self) -> &str {
        match self {
            Self::Session => KEYRING_ACCOUNT,
            Self::Provider {
                keyring_account, ..
            } => keyring_account,
        }
    }

    /// The Linux private-file fallback path for this slot.
    pub fn private_file(&self, paths: &DataPaths) -> PathBuf {
        match self {
            Self::Session => paths.credential_file(),
            Self::Provider { private_file, .. } => paths.private_dir().join(private_file),
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

    let keyring_account = slot.keyring_account().to_owned();
    select_linux(
        || {
            let store = Store::new().map_err(|_| Absent)?;
            keyring::os_backend(store, KEYRING_SERVICE, &keyring_account).map_err(|_| Absent)
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

    fn alias(keyring_account: &str, private_file: &str) -> CredentialAlias {
        CredentialAlias {
            keyring_account: keyring_account.into(),
            private_file: private_file.into(),
        }
    }

    #[test]
    fn provider_slots_use_aliases_or_default_names() {
        let dir = TempDir::new().unwrap();
        let paths = DataPaths::at(dir.path().join("data"));

        let steam = CredentialSlot::provider("steam", Some(alias("steam", "steam-token")));
        assert_eq!(steam.keyring_account(), "steam");
        assert_eq!(
            steam.private_file(&paths),
            paths.private_dir().join("steam-token")
        );

        let epic = CredentialSlot::provider("epic", None);
        assert_eq!(epic.keyring_account(), "provider.epic");
        assert_eq!(
            epic.private_file(&paths),
            paths.private_dir().join("provider-epic.secret")
        );
        assert_ne!(epic.private_file(&paths), paths.credential_file());
        assert_ne!(
            epic.keyring_account(),
            CredentialSlot::Session.keyring_account()
        );
    }

    #[test]
    fn aliases_must_be_plain_names_and_never_the_session_slot() {
        assert!(alias("steam", "steam-token").is_valid());
        assert!(alias("provider.epic", "provider-epic.secret").is_valid());
        for invalid in [
            alias("", "token"),
            alias("session", "steam-token"),
            alias("steam", "session-token"),
            alias("../escape", "token"),
            alias("steam", "dir/token"),
            alias("steam", ".."),
            alias("steam", "token\n"),
        ] {
            assert!(!invalid.is_valid(), "{invalid:?} must be rejected");
        }
    }

    #[test]
    fn the_file_fallback_keeps_slots_independent() {
        let dir = TempDir::new().unwrap();
        let paths = DataPaths::at(dir.path().join("data"));
        let session = select_linux(
            || Err(Absent),
            &paths,
            StorageMethod::PrivateFile,
            CredentialSlot::Session,
        );
        let provider = select_linux(
            || Err(Absent),
            &paths,
            StorageMethod::PrivateFile,
            CredentialSlot::provider("steam", Some(alias("steam", "steam-token"))),
        );
        session.save(&cred("gamenode-session")).unwrap();
        provider.save(&cred("steamLoginSecure=cookie")).unwrap();

        // Deleting one slot does not touch the other.
        provider.delete().unwrap();
        assert_eq!(
            session.load().unwrap().unwrap().expose(),
            "gamenode-session"
        );
        assert_eq!(provider.load().unwrap(), None);
        assert!(paths.credential_file().exists());
        assert!(!paths.private_dir().join("steam-token").exists());
    }
}
