//! Credential store on top of an OS keyring.

use std::sync::Arc;

use gamenode_core::ports::{CredentialError, CredentialStore};
use gamenode_core::{SessionCredential, StorageMethod};

/// Why a keyring operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendError {
    /// The service exists but is locked or refused access.
    Locked,
    Failed,
}

/// The few operations a keyring provides for one fixed entry.
pub trait KeyringBackend: Send + Sync {
    fn get(&self) -> Result<Option<String>, BackendError>;
    fn set(&self, value: &str) -> Result<(), BackendError>;
    /// Removing an entry that does not exist succeeds.
    fn delete(&self) -> Result<(), BackendError>;
}

pub struct KeyringCredentialStore {
    backend: Arc<dyn KeyringBackend>,
    method: StorageMethod,
}

impl KeyringCredentialStore {
    pub fn new(backend: Arc<dyn KeyringBackend>, method: StorageMethod) -> Self {
        Self { backend, method }
    }
}

fn map(error: BackendError) -> CredentialError {
    match error {
        BackendError::Locked => CredentialError::Locked,
        BackendError::Failed => CredentialError::Failed,
    }
}

impl CredentialStore for KeyringCredentialStore {
    fn method(&self) -> StorageMethod {
        self.method
    }

    fn load(&self) -> Result<Option<SessionCredential>, CredentialError> {
        Ok(self
            .backend
            .get()
            .map_err(map)?
            .and_then(SessionCredential::new))
    }

    fn save(&self, credential: &SessionCredential) -> Result<(), CredentialError> {
        self.backend.set(credential.expose()).map_err(map)
    }

    fn delete(&self) -> Result<(), CredentialError> {
        self.backend.delete().map_err(map)
    }
}

/// Used on Windows when Credential Manager cannot be opened: every operation fails, and there is
/// deliberately no file fallback.
#[cfg(windows)]
pub(crate) struct Unavailable(pub StorageMethod);

#[cfg(windows)]
impl CredentialStore for Unavailable {
    fn method(&self) -> StorageMethod {
        self.0
    }
    fn load(&self) -> Result<Option<SessionCredential>, CredentialError> {
        Err(CredentialError::Failed)
    }
    fn save(&self, _: &SessionCredential) -> Result<(), CredentialError> {
        Err(CredentialError::Failed)
    }
    fn delete(&self) -> Result<(), CredentialError> {
        Err(CredentialError::Failed)
    }
}

/// The real OS keyring entry, through `keyring-core`.
struct OsKeyring(keyring_core::Entry);

fn classify(error: &keyring_core::Error) -> BackendError {
    match error {
        keyring_core::Error::NoStorageAccess(_) => BackendError::Locked,
        _ => BackendError::Failed,
    }
}

impl KeyringBackend for OsKeyring {
    fn get(&self) -> Result<Option<String>, BackendError> {
        match self.0.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(classify(&error)),
        }
    }

    fn set(&self, value: &str) -> Result<(), BackendError> {
        self.0.set_password(value).map_err(|e| classify(&e))
    }

    fn delete(&self) -> Result<(), BackendError> {
        match self.0.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(classify(&error)),
        }
    }
}

/// Builds the backend for `service`/`account` from a keyring-core credential store.
pub(crate) fn os_backend(
    store: Arc<impl keyring_core::api::CredentialStoreApi + 'static>,
    service: &str,
    account: &str,
) -> Result<Arc<dyn KeyringBackend>, BackendError> {
    let entry = store
        .build(service, account, None)
        .map_err(|e| classify(&e))?;
    Ok(Arc::new(OsKeyring(entry)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory keyring with injectable failures.
    #[derive(Default)]
    struct Fake {
        value: Mutex<Option<String>>,
        fail_get: Mutex<Option<BackendError>>,
        fail_set: Mutex<Option<BackendError>>,
        fail_delete: Mutex<Option<BackendError>>,
    }

    impl KeyringBackend for Fake {
        fn get(&self) -> Result<Option<String>, BackendError> {
            if let Some(e) = *self.fail_get.lock().unwrap() {
                return Err(e);
            }
            Ok(self.value.lock().unwrap().clone())
        }
        fn set(&self, value: &str) -> Result<(), BackendError> {
            if let Some(e) = *self.fail_set.lock().unwrap() {
                return Err(e);
            }
            *self.value.lock().unwrap() = Some(value.to_owned());
            Ok(())
        }
        fn delete(&self) -> Result<(), BackendError> {
            if let Some(e) = *self.fail_delete.lock().unwrap() {
                return Err(e);
            }
            self.value.lock().unwrap().take();
            Ok(())
        }
    }

    fn store() -> (Arc<Fake>, KeyringCredentialStore) {
        let fake = Arc::new(Fake::default());
        (
            fake.clone(),
            KeyringCredentialStore::new(fake, StorageMethod::SecretService),
        )
    }

    fn cred(v: &str) -> SessionCredential {
        SessionCredential::new(v).unwrap()
    }

    #[test]
    fn saves_loads_and_deletes() {
        let (_, store) = store();
        assert_eq!(store.load(), Ok(None));
        store.save(&cred("abc")).unwrap();
        assert_eq!(store.load().unwrap().unwrap().expose(), "abc");
        store.save(&cred("def")).unwrap();
        assert_eq!(store.load().unwrap().unwrap().expose(), "def");
        store.delete().unwrap();
        assert_eq!(store.load(), Ok(None));
        store.delete().unwrap(); // already gone
        assert_eq!(store.method(), StorageMethod::SecretService);
    }

    #[test]
    fn a_locked_service_is_reported_not_hidden() {
        let (fake, store) = store();
        *fake.fail_get.lock().unwrap() = Some(BackendError::Locked);
        *fake.fail_set.lock().unwrap() = Some(BackendError::Locked);
        assert_eq!(store.load().unwrap_err(), CredentialError::Locked);
        assert_eq!(store.save(&cred("x")).unwrap_err(), CredentialError::Locked);
        assert!(fake.value.lock().unwrap().is_none(), "nothing was written");
    }

    #[test]
    fn failed_writes_and_deletions_surface_and_keep_the_old_value() {
        let (fake, store) = store();
        store.save(&cred("old")).unwrap();
        *fake.fail_set.lock().unwrap() = Some(BackendError::Failed);
        assert_eq!(
            store.save(&cred("new")).unwrap_err(),
            CredentialError::Failed
        );
        *fake.fail_delete.lock().unwrap() = Some(BackendError::Failed);
        assert_eq!(store.delete().unwrap_err(), CredentialError::Failed);
        assert_eq!(fake.value.lock().unwrap().as_deref(), Some("old"));
    }

    #[test]
    fn an_empty_stored_value_is_treated_as_no_credential() {
        let (fake, store) = store();
        *fake.value.lock().unwrap() = Some(String::new());
        assert_eq!(store.load(), Ok(None));
    }

    #[test]
    fn keyring_core_errors_are_classified() {
        use keyring_core::Error;
        let platform = || keyring_core::error::PlatformError::from("boom");
        assert_eq!(
            classify(&Error::NoStorageAccess(platform())),
            BackendError::Locked
        );
        assert_eq!(
            classify(&Error::PlatformFailure(platform())),
            BackendError::Failed
        );
        assert_eq!(classify(&Error::NoEntry), BackendError::Failed);
    }

    #[test]
    fn the_os_backend_round_trips_through_keyring_core_with_the_mock_store() {
        let store = keyring_core::mock::Store::new().unwrap();
        let backend = os_backend(store, "svc", "acct").unwrap();
        let store = KeyringCredentialStore::new(backend, StorageMethod::SecretService);
        assert_eq!(store.load(), Ok(None));
        store.save(&cred("tok")).unwrap();
        assert_eq!(store.load().unwrap().unwrap().expose(), "tok");
        store.delete().unwrap();
        assert_eq!(store.load(), Ok(None));
        store.delete().unwrap();
    }
}
