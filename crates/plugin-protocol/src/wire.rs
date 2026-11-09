use crate::{WireError, models::*, proto};

macro_rules! enum_wire {
    ($model:ident, $wire:ident, $($variant:ident),+ $(,)?) => {
        impl From<$model> for proto::$wire {
            fn from(value: $model) -> Self {
                match value { $($model::$variant => Self::$variant),+ }
            }
        }
        impl TryFrom<proto::$wire> for $model {
            type Error = WireError;
            fn try_from(value: proto::$wire) -> Result<Self, Self::Error> {
                match value { $(proto::$wire::$variant => Ok(Self::$variant),)+ _ => Err(WireError) }
            }
        }
    };
}

enum_wire!(
    SignInResult,
    AccountLoginOutcome,
    Connected,
    Cancelled,
    Closed,
    TimedOut,
    Failed,
    Superseded
);

enum_wire!(
    AccountState,
    AccountConnection,
    NotConnected,
    Checking,
    SigningIn,
    Connected,
    Expired,
    Unreachable,
    CredentialError
);
enum_wire!(
    StorageMethod,
    SecretStorage,
    WindowsCredentialManager,
    SecretService,
    PrivateFile
);
enum_wire!(
    CatalogError,
    CatalogFailure,
    Unreachable,
    RateLimited,
    Server,
    Expired,
    Privacy,
    Changed,
    InvalidCookies,
    FamilyUnavailable
);

impl From<CatalogRecord> for proto::CatalogGame {
    fn from(record: CatalogRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
        }
    }
}

impl TryFrom<proto::CatalogGame> for CatalogRecord {
    type Error = WireError;
    fn try_from(record: proto::CatalogGame) -> Result<Self, Self::Error> {
        if record.id.trim().is_empty()
            || record.id.len() > 256
            || record.name.trim().is_empty()
            || record.name.len() > 4096
            || record.id.chars().any(char::is_control)
            || record.name.chars().any(char::is_control)
        {
            return Err(WireError);
        }
        Ok(Self {
            id: record.id,
            name: record.name,
        })
    }
}

impl From<StoreAccount> for proto::AccountIdentity {
    fn from(account: StoreAccount) -> Self {
        Self {
            provider_id: account.provider,
            account_id: account.account_id,
            account_name: account.account_name,
            last_verified_at: account.last_verified_at,
        }
    }
}

impl TryFrom<proto::AccountIdentity> for StoreAccount {
    type Error = WireError;
    fn try_from(account: proto::AccountIdentity) -> Result<Self, Self::Error> {
        if account.provider_id.is_empty()
            || account.provider_id.len() > 128
            || account.account_id.is_empty()
            || account.account_id.len() > 256
            || account.account_id.chars().any(char::is_control)
            || account
                .account_name
                .as_ref()
                .is_some_and(|s| s.len() > 1024 || s.chars().any(char::is_control))
            || account.last_verified_at.is_some_and(|time| time < 0)
        {
            return Err(WireError);
        }
        Ok(Self {
            provider: account.provider_id,
            account_id: account.account_id,
            account_name: account.account_name,
            last_verified_at: account.last_verified_at,
        })
    }
}

impl From<AccountStatus> for proto::AccountStatus {
    fn from(status: AccountStatus) -> Self {
        Self {
            state: proto::AccountConnection::from(status.state) as i32,
            account_id: status.account_id,
            account_name: status.account_name,
            last_checked_at: status.last_checked_at,
            last_synced_at: status.last_synced_at,
            storage: proto::SecretStorage::from(status.storage) as i32,
            persist_failed: status.persist_failed,
            catalog_error: status
                .catalog_error
                .map(|error| proto::CatalogFailure::from(error) as i32),
            family_error: status
                .family_error
                .map(|error| proto::CatalogFailure::from(error) as i32),
        }
    }
}

impl TryFrom<proto::AccountStatus> for AccountStatus {
    type Error = WireError;
    fn try_from(status: proto::AccountStatus) -> Result<Self, Self::Error> {
        if status
            .account_id
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
            || status
                .account_name
                .as_ref()
                .is_some_and(|s| s.len() > 1024 || s.chars().any(char::is_control))
            || [status.last_checked_at, status.last_synced_at]
                .into_iter()
                .flatten()
                .any(|time| time < 0)
        {
            return Err(WireError);
        }
        fn error(value: i32) -> Result<CatalogError, WireError> {
            proto::CatalogFailure::try_from(value)
                .map_err(|_| WireError)?
                .try_into()
        }
        Ok(Self {
            state: proto::AccountConnection::try_from(status.state)
                .map_err(|_| WireError)?
                .try_into()?,
            account_id: status.account_id,
            account_name: status.account_name,
            last_checked_at: status.last_checked_at,
            last_synced_at: status.last_synced_at,
            storage: proto::SecretStorage::try_from(status.storage)
                .map_err(|_| WireError)?
                .try_into()?,
            persist_failed: status.persist_failed,
            catalog_error: status.catalog_error.map(error).transpose()?,
            family_error: status.family_error.map(error).transpose()?,
        })
    }
}
