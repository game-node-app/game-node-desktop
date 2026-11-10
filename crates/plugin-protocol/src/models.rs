//! Public provider-account values. No application-internal types or OS access.

use std::{collections::BTreeMap, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// An opaque provider secret, exposed only to authorized service calls.
///
/// It is deliberately opaque: `Debug` and `Display` never reveal it, so logs and errors cannot
/// leak it by accident. Call [`SessionCredential::expose`] only to send or store it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionCredential(String);

impl SessionCredential {
    /// Wraps a credential value. Returns `None` for an empty value.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty()).then_some(Self(value))
    }

    /// The raw value. Never log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SessionCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionCredential(<redacted>)")
    }
}

/// Where the session credential is kept. Settings shows this; it never carries the credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageMethod {
    /// Windows Credential Manager.
    WindowsCredentialManager,
    /// A Secret Service implementation (GNOME Keyring, KWallet, ...) on Linux.
    SecretService,
    /// Linux only, when no Secret Service exists: a file readable only by the current user.
    PrivateFile,
}

impl StorageMethod {
    /// Whether this is the user-only file fallback that Settings must disclose.
    pub fn is_fallback_file(self) -> bool {
        matches!(self, Self::PrivateFile)
    }
}

/// The categories a catalog sync failure is stored and shown under.
///
/// The strings are stable: the database stores them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogError {
    Unreachable,
    RateLimited,
    Server,
    Expired,
    Privacy,
    Changed,
    InvalidCredential,
    FamilyUnavailable,
}

impl CatalogError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::RateLimited => "rateLimited",
            Self::Server => "server",
            Self::Expired => "expired",
            Self::Privacy => "privacy",
            Self::Changed => "changed",
            Self::InvalidCredential => "invalidCredential",
            Self::FamilyUnavailable => "familyUnavailable",
        }
    }

    /// The category stored under `value`, if it is one this build knows.
    pub fn from_category(value: &str) -> Option<Self> {
        match value {
            "unreachable" => Some(Self::Unreachable),
            "rateLimited" => Some(Self::RateLimited),
            "server" => Some(Self::Server),
            "expired" => Some(Self::Expired),
            "privacy" => Some(Self::Privacy),
            "changed" => Some(Self::Changed),
            // Rows written before the provider-neutral rename still say "invalidCookies".
            "invalidCredential" | "invalidCookies" => Some(Self::InvalidCredential),
            "familyUnavailable" => Some(Self::FamilyUnavailable),
            _ => None,
        }
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CatalogError {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_category(value).ok_or(())
    }
}

/// One game from a provider's owned or family-shared catalog.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRecord {
    pub id: String,
    pub name: String,
    /// Named facts used for matching and action templates. Construction through
    /// [`CatalogRecord::with_field`] keeps them within the wire bounds.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
}

/// The most catalog fields one game may carry.
pub const MAX_CATALOG_FIELDS: usize = 16;
/// The longest catalog field key, in bytes.
pub const MAX_CATALOG_FIELD_KEY: usize = 64;
/// The longest catalog field value, in bytes.
pub const MAX_CATALOG_FIELD_VALUE: usize = 256;

fn bounded_field_text(value: &str, max: usize, allow_empty: bool) -> bool {
    (allow_empty || !value.is_empty()) && value.len() <= max && !value.chars().any(char::is_control)
}

impl CatalogRecord {
    /// A record without fields. Add facts with [`CatalogRecord::with_field`].
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            fields: BTreeMap::new(),
        }
    }

    /// Adds one bounded field, rejecting the whole record when it is invalid instead of
    /// truncating or dropping the value. Replacing an existing key keeps one entry.
    pub fn with_field(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, crate::WireError> {
        let key = key.into();
        let value = value.into();
        if !bounded_field_text(&key, MAX_CATALOG_FIELD_KEY, false)
            || !bounded_field_text(&value, MAX_CATALOG_FIELD_VALUE, true)
            || (!self.fields.contains_key(&key) && self.fields.len() >= MAX_CATALOG_FIELDS)
        {
            return Err(crate::WireError);
        }
        self.fields.insert(key, value);
        Ok(self)
    }

    /// Checks the same bounds the host applies when reading fields from the wire.
    pub fn validate_fields(&self) -> Result<(), crate::WireError> {
        if self.fields.len() > MAX_CATALOG_FIELDS
            || self.fields.iter().any(|(key, value)| {
                !bounded_field_text(key, MAX_CATALOG_FIELD_KEY, false)
                    || !bounded_field_text(value, MAX_CATALOG_FIELD_VALUE, true)
            })
        {
            return Err(crate::WireError);
        }
        Ok(())
    }
}

/// A bounded achievement read: up to [`crate::achievements::MAX_GAMES`] provider game ids and the
/// UI language the store should localize to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementsRequest {
    pub game_ids: Vec<String>,
    pub language: String,
}

/// One achievement definition joined with the connected account's unlock state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub id: String,
    pub name: String,
    pub description: String,
    pub hidden: bool,
    pub icon_url: String,
    pub icon_gray_url: String,
    pub unlocked: bool,
    /// Unix seconds Steam reported. An unlocked achievement without a time has `None`.
    pub unlocked_at: Option<i64>,
}

/// One game's complete achievement result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AchievementOutcome {
    Ready {
        game_id: String,
        achievements: Vec<Achievement>,
    },
    /// The provider confirmed the game has no achievements.
    None { game_id: String },
    Failed {
        game_id: String,
        error: CatalogError,
    },
}

impl AchievementOutcome {
    pub fn game_id(&self) -> &str {
        match self {
            Self::Ready { game_id, .. } | Self::None { game_id } | Self::Failed { game_id, .. } => {
                game_id
            }
        }
    }
}

/// A catalog sync result: `None` means that fact set was not attempted, `Ok` replaces it and
/// `Err` keeps it and records the failure category.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogUpdate {
    pub owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    pub family: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    pub now: i64,
}

/// The non-secret identity of a connected store account.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreAccount {
    pub provider: String,
    pub account_id: String,
    pub account_name: Option<String>,
    pub last_verified_at: Option<i64>,
}

/// The connection states the Stores card shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountState {
    /// No stored credential: sign-in is offered and installed games still work.
    NotConnected,
    /// A session validation or catalog sync is running.
    Checking,
    /// The sign-in window is open.
    SigningIn,
    /// The session was validated; catalog sync may still report its own errors.
    Connected,
    /// The provider rejected the session and the dead credential was removed.
    Expired,
    /// The provider could not be reached or returned a server error; the credential is kept.
    Unreachable,
    /// The credential store could not be read or is locked.
    CredentialError,
}

/// Everything the Stores card needs about the account, with no secret fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub state: AccountState,
    pub account_id: Option<String>,
    pub account_name: Option<String>,
    /// Unix seconds of the last successful session validation.
    pub last_checked_at: Option<i64>,
    /// Unix seconds of the last successful owned-catalog sync in this run. The durable value is
    /// the library's `last_catalog_at`, persisted with `CatalogSynced`.
    pub last_synced_at: Option<i64>,
    pub storage: StorageMethod,
    /// A credential or account-row write failed, so a restart may not restore this state.
    pub persist_failed: bool,
    pub catalog_error: Option<CatalogError>,
    pub family_error: Option<CatalogError>,
}

impl AccountStatus {
    /// Whether the primary action is "Sign in with the provider" / "Sign in again".
    pub const fn can_sign_in(&self) -> bool {
        matches!(
            self.state,
            AccountState::NotConnected
                | AccountState::Expired
                | AccountState::Unreachable
                | AccountState::CredentialError
        )
    }

    /// Whether the sign-in window is open and can be brought to the front.
    pub const fn can_focus_sign_in(&self) -> bool {
        matches!(self.state, AccountState::SigningIn)
    }

    pub const fn can_cancel_sign_in(&self) -> bool {
        matches!(self.state, AccountState::SigningIn)
    }

    /// Whether a stored credential exists, so Sign out is offered.
    pub const fn can_sign_out(&self) -> bool {
        matches!(
            self.state,
            AccountState::Connected
                | AccountState::Expired
                | AccountState::Unreachable
                | AccountState::CredentialError
        )
    }

    /// Whether "Sync now" / "Try again" / "Retry" is offered.
    pub const fn can_check(&self) -> bool {
        matches!(
            self.state,
            AccountState::Connected | AccountState::Unreachable | AccountState::CredentialError
        )
    }

    /// Whether a credential that could not be read can be removed.
    pub const fn can_remove_credential(&self) -> bool {
        matches!(self.state, AccountState::CredentialError)
    }
}

/// The result of a session check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckOutcome {
    pub status: AccountStatus,
    /// The validated account differs from the one on disk, so the engine must clear the catalog
    /// before applying the next result.
    pub account_changed: bool,
    /// The attempt was superseded by a sign-in, sign-out or credential removal; ignore it.
    pub superseded: bool,
}

/// The result of a catalog sync, ready to feed `Input::CatalogSynced`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogOutcome {
    pub status: AccountStatus,
    /// `None` when no catalog was attempted (no account, or the request joined a check).
    pub owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    /// `None` when family sync was not attempted.
    pub family: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    pub now: i64,
    pub account_changed: bool,
    pub superseded: bool,
}

/// How the sign-in attempt ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInResult {
    /// The cookie was validated and the account is connected.
    Connected,
    /// The user pressed Cancel.
    Cancelled,
    /// The user closed the window.
    Closed,
    /// The ten-minute window lifetime elapsed.
    TimedOut,
    /// The window could not open, or the new session could not be validated.
    Failed,
    /// A sign-out or newer attempt superseded this one.
    Superseded,
}

/// The result of a sign-in attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInOutcome {
    pub status: AccountStatus,
    pub result: SignInResult,
    pub account_changed: bool,
}

/// The result of a sign-out or credential removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignOutOutcome {
    pub status: AccountStatus,
    /// `false` when the credential could not be deleted; the account stays connected this run.
    pub signed_out: bool,
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

/// A provider metadata storage failure, with a safe non-secret diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StorageError {
    #[error("provider account storage failed: {0}")]
    Failed(String),
}

/// A plugin's non-secret account metadata service. Calls may block on its SDK worker.
pub trait StoreAccountStore: Send + Sync {
    fn load(&self) -> Result<Option<StoreAccount>, StorageError>;
    fn save(&self, account: &StoreAccount) -> Result<(), StorageError>;
    fn clear(&self) -> Result<(), StorageError>;
}
