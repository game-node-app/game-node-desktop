//! Adapter interfaces implemented outside `core`.
//!
//! Only what the foundation needs: settings persistence, credential storage, the GameNode auth
//! API, and opening the system browser. Game, provider and tracking interfaces arrive with their
//! roadmap steps.

use async_trait::async_trait;
use url::Url;

use crate::auth::{SessionError, SignInFailure, User};
use crate::callback::Provider;
use crate::credential::{SessionCredential, StorageMethod};
use crate::settings::Settings;

/// Failure to read or write local state. Messages are safe to show: no credentials or URLs.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StorageError {
    /// The database was written by a newer version of GameNode Desktop and was left untouched.
    #[error("the local database uses schema {found}, but this version supports up to {supported}")]
    NewerSchema { found: u32, supported: u32 },
    /// A migration failed and was rolled back; the database was left at the previous version.
    #[error("local database migration {version} failed and was rolled back")]
    MigrationFailed { version: u32 },
    #[error("the local database could not be used: {0}")]
    Failed(String),
}

/// Durable, non-secret settings.
pub trait SettingsStore: Send + Sync {
    fn load(&self) -> Result<Settings, StorageError>;
    fn save(&self, settings: &Settings) -> Result<(), StorageError>;
}

/// Why a credential operation failed. Deliberately not "unavailable": absence of a credential
/// service is decided once, when the store is selected, never per operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    /// The credential service exists but is locked or refused access.
    #[error("the credential store is locked or access was denied")]
    Locked,
    #[error("the credential store failed")]
    Failed,
}

/// Where the session credential lives. All calls may block; run them on a blocking worker.
pub trait CredentialStore: Send + Sync {
    fn method(&self) -> StorageMethod;
    fn load(&self) -> Result<Option<SessionCredential>, CredentialError>;
    fn save(&self, credential: &SessionCredential) -> Result<(), CredentialError>;
    /// Deleting a credential that does not exist succeeds.
    fn delete(&self) -> Result<(), CredentialError>;
}

/// The existing GameNode auth endpoints.
#[async_trait]
pub trait AuthApi: Send + Sync {
    /// Starts a provider sign-in (`POST /v1/auth/sign-in/social`) and returns the URL for the
    /// system browser (the authorization proxy URL).
    async fn begin_sign_in(
        &self,
        provider: Provider,
        success_url: &str,
        failure_url: &str,
    ) -> Result<Url, SignInFailure>;

    /// `GET /v1/auth/get-session` with the credential as a bearer token.
    async fn get_session(&self, credential: &SessionCredential) -> Result<User, SessionError>;

    /// `POST /v1/auth/sign-out`. `Err` means invalidation could not be confirmed.
    async fn sign_out(&self, credential: &SessionCredential) -> Result<(), ()>;
}

/// Opens a URL in the user's default browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("could not open the system browser")]
pub struct BrowserError;

/// Opens a URL in the user's default browser.
pub trait BrowserOpener: Send + Sync {
    fn open(&self, url: &Url) -> Result<(), BrowserError>;
}
