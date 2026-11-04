//! Adapter interfaces implemented outside `core`.
//!
//! Settings persistence, credential storage, the GameNode auth API and opening the system browser,
//! plus what provider scans need from the host: read-only file access ([`HostFiles`]) and root
//! resolution ([`RootResolver`]). Library, game API and tracking interfaces sit next to the code
//! that uses them (`library`, `game_api`) or arrive with their roadmap steps.

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

/// Why a provider URI could not be handed to the operating system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UriError {
    /// Nothing on this computer handles the URI's scheme (the store client is not installed).
    #[error("no application handles this kind of link")]
    NoHandler,
    #[error("the link could not be opened")]
    Failed,
}

/// Hands one provider-declared URI (`steam://rungameid/620`) to the OS handler for its scheme.
///
/// Separate from [`BrowserOpener`], which only opens web pages: the two have different rules about
/// which URLs they accept. The URI is passed as a single argument, never through a shell. All calls
/// may block until the OS has accepted the URI; run them on a blocking worker.
pub trait UriOpener: Send + Sync {
    fn open(&self, uri: &Url) -> Result<(), UriError>;
}

/// Read-only file access for provider scanners.
///
/// Paths are plain strings in the host's own style (`C:\Games\Steam` on Windows, `/home/me/.steam`
/// on Linux), so scanners can be tested on any OS with an in-memory double. All calls may block.
pub trait HostFiles: Send + Sync {
    fn read_to_string(&self, path: &str) -> std::io::Result<String>;
    /// File and folder names (not full paths) directly inside `path`.
    fn read_dir(&self, path: &str) -> std::io::Result<Vec<String>>;
    fn is_dir(&self, path: &str) -> bool;
    /// The real path with links resolved; fails if the path does not exist.
    fn canonicalize(&self, path: &str) -> std::io::Result<String>;
}

/// Turns the location hints of a provider manifest into concrete host paths.
pub trait RootResolver: Send + Sync {
    /// Replaces a leading `~` with the user's home folder. `None` if the home folder is unknown.
    fn expand_home(&self, path: &str) -> Option<String>;
    /// A string value under `HKEY_CURRENT_USER\<key>`. Always `None` where there is no registry.
    fn registry_string(&self, key: &str, value: &str) -> Option<String>;
}
