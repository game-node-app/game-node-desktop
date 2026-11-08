use serde::{Deserialize, Serialize};

use crate::auth::{AuthMachine, AuthState, Stage, User};
use crate::callback::Provider;
use crate::credential::StorageMethod;
use crate::library::{CatalogError, LaunchStatus, LibraryView};
use crate::notice::Notice;
use crate::settings::Settings;

pub use crate::auth::Input;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AuthView {
    /// Startup, before the stored session has been read.
    Starting,
    SignedOut,
    /// A sign-in is in progress. `awaiting_browser` is true once the browser was opened.
    Pending {
        provider: Provider,
        awaiting_browser: bool,
    },
    /// Validating a session with GameNode.
    Verifying,
    SignedIn {
        user: User,
    },
    /// A stored session exists but GameNode was unreachable.
    RestoreFailed,
    SigningOut,
}

/// A complete, immutable view of the application for the UI. Never contains credentials.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub local_games: crate::drm_free::LocalView,
    pub playtime_error: bool,
    pub tracking: TrackingView,
    pub sync: crate::sync::SyncPreferences,
    pub playtime_source: crate::playtime::PlaytimeSource,
    pub auth: AuthView,
    pub notice: Option<Notice>,
    pub storage: StorageMethod,
    pub settings: Settings,
    /// The game library, present while an account's library is open.
    pub library: Option<LibraryView>,
    /// The latest Play or Install, while it is opening or after it failed.
    pub launch: Option<LaunchStatus>,
    /// Scalable game tools management and modal view.
    pub tools: crate::tools::ToolsView,
    /// One entry per store provider that declares a catalog, with no secret fields.
    pub store_accounts: Vec<StoreAccountView>,
    /// PC ports catalog, provenance and install jobs. Always present; empty until opened.
    pub ports: crate::ports::PortsView,
}

impl Snapshot {
    pub fn new(
        machine: &AuthMachine,
        storage: StorageMethod,
        settings: Settings,
        library: Option<LibraryView>,
        launch: Option<LaunchStatus>,
    ) -> Self {
        let auth = match machine.state() {
            AuthState::Starting => AuthView::Starting,
            AuthState::SignedOut => AuthView::SignedOut,
            AuthState::Pending { provider, stage } => AuthView::Pending {
                provider: *provider,
                awaiting_browser: *stage == Stage::AwaitingBrowser,
            },
            AuthState::Verifying(_) => AuthView::Verifying,
            AuthState::SignedIn(user) => AuthView::SignedIn { user: user.clone() },
            AuthState::RestoreFailed => AuthView::RestoreFailed,
            AuthState::SigningOut => AuthView::SigningOut,
        };
        Self {
            local_games: Default::default(),
            playtime_error: false,
            tracking: TrackingView::default(),
            sync: Default::default(),
            playtime_source: Default::default(),
            auth,
            notice: machine.notice(),
            storage,
            settings,
            library,
            launch,
            tools: Default::default(),
            store_accounts: Vec::new(),
            ports: crate::ports::PortsView::default(),
        }
    }
}

/// The connection state of one store account, as Stores shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoreAccountState {
    /// No stored credential: sign-in is offered and installed games still work.
    NotConnected,
    /// A session validation or catalog sync is running.
    Checking,
    /// The store's own sign-in window is open.
    SigningIn,
    /// The session was validated; catalog sync may still report its own errors.
    Connected,
    /// The store rejected the session and the dead credential was removed.
    Expired,
    /// The store could not be reached or returned a server error; the credential is kept.
    Unreachable,
    /// The credential store could not be read or is locked.
    CredentialError,
}

/// Everything the Stores card shows about one store account. Never contains a credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreAccountView {
    pub provider: String,
    pub state: StoreAccountState,
    pub account_id: Option<String>,
    pub account_name: Option<String>,
    /// Games the connected account owns.
    pub owned_count: usize,
    /// Games shared through the store's family feature and not owned.
    pub family_count: usize,
    /// Games installed on this PC.
    pub installed_count: usize,
    /// Unix seconds of the last successful session validation.
    pub last_checked_at: Option<i64>,
    /// Unix seconds of the last successful catalog sync in this run. The UI uses it to announce
    /// an imported library after the sign-in flow has actually fetched the counts.
    pub last_synced_at: Option<i64>,
    /// Where the credential is kept; `None` when no account control is available.
    pub storage: Option<StorageMethod>,
    /// A credential or account-row write failed, so a restart may not restore this state.
    pub persist_failed: bool,
    pub catalog_error: Option<CatalogError>,
    pub family_error: Option<CatalogError>,
    pub can_sign_in: bool,
    pub can_cancel_sign_in: bool,
    pub can_focus_sign_in: bool,
    pub can_sign_out: bool,
    pub can_check: bool,
    pub can_remove_credential: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot::new(
            &AuthMachine::new(Box::new(|| "nonce".into())),
            StorageMethod::SecretService,
            Settings::default(),
            None,
            None,
        )
    }

    fn account(state: StoreAccountState) -> StoreAccountView {
        StoreAccountView {
            provider: "steam".to_owned(),
            state,
            account_id: Some("76561198000000001".to_owned()),
            account_name: Some("cassi".to_owned()),
            owned_count: 312,
            family_count: 8,
            installed_count: 18,
            last_checked_at: Some(1_760_000_000),
            last_synced_at: Some(1_760_000_100),
            storage: Some(StorageMethod::PrivateFile),
            persist_failed: true,
            catalog_error: Some(CatalogError::Unreachable),
            family_error: Some(CatalogError::FamilyUnavailable),
            can_sign_in: false,
            can_cancel_sign_in: true,
            can_focus_sign_in: true,
            can_sign_out: true,
            can_check: true,
            can_remove_credential: false,
        }
    }

    #[test]
    fn a_new_snapshot_has_no_store_accounts() {
        assert!(snapshot().store_accounts.is_empty());
    }

    #[test]
    fn store_accounts_serialize_with_the_ui_field_names() {
        let mut snapshot = snapshot();
        snapshot.store_accounts = vec![account(StoreAccountState::CredentialError)];

        let value = serde_json::to_value(&snapshot).expect("a snapshot serializes");
        let accounts = value["storeAccounts"].as_array().expect("storeAccounts");
        assert_eq!(accounts.len(), 1);
        let account = accounts[0].as_object().expect("a store account");
        assert_eq!(account["provider"], "steam");
        assert_eq!(account["state"], "credentialError");
        assert_eq!(account["accountId"], "76561198000000001");
        assert_eq!(account["accountName"], "cassi");
        assert_eq!(account["ownedCount"], 312);
        assert_eq!(account["familyCount"], 8);
        assert_eq!(account["installedCount"], 18);
        assert_eq!(account["lastCheckedAt"], 1_760_000_000);
        assert_eq!(account["lastSyncedAt"], 1_760_000_100);
        assert_eq!(account["storage"], "privateFile");
        assert_eq!(account["persistFailed"], true);
        assert_eq!(account["catalogError"], "unreachable");
        assert_eq!(account["familyError"], "familyUnavailable");
        assert_eq!(account["canSignIn"], false);
        assert_eq!(account["canCancelSignIn"], true);
        assert_eq!(account["canFocusSignIn"], true);
        assert_eq!(account["canSignOut"], true);
        assert_eq!(account["canCheck"], true);
        assert_eq!(account["canRemoveCredential"], false);

        let keys: std::collections::BTreeSet<&str> = account.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "accountId",
                "accountName",
                "canCancelSignIn",
                "canCheck",
                "canFocusSignIn",
                "canRemoveCredential",
                "canSignIn",
                "canSignOut",
                "catalogError",
                "familyCount",
                "familyError",
                "installedCount",
                "lastCheckedAt",
                "lastSyncedAt",
                "ownedCount",
                "persistFailed",
                "provider",
                "state",
                "storage",
            ]
            .into_iter()
            .collect(),
            "no secret or unexpected field may reach the UI"
        );
        assert!(value.get("store_accounts").is_none());
    }

    #[test]
    fn every_store_account_state_uses_the_contract_name() {
        for (state, name) in [
            (StoreAccountState::NotConnected, "notConnected"),
            (StoreAccountState::Checking, "checking"),
            (StoreAccountState::SigningIn, "signingIn"),
            (StoreAccountState::Connected, "connected"),
            (StoreAccountState::Expired, "expired"),
            (StoreAccountState::Unreachable, "unreachable"),
            (StoreAccountState::CredentialError, "credentialError"),
        ] {
            assert_eq!(serde_json::to_value(state).unwrap(), name, "{name}");
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingView {
    pub title: String,
    pub available: bool,
    pub running: std::collections::BTreeSet<String>,
}
