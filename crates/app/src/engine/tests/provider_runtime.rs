//! Mainline Engine fixtures inject a provider port using plugin-owned code.
//! Production hosts use HostedProviders and the authenticated process boundary.
use super::*;
use crate::engine::steam_account::{SteamAccount, SteamAccountState};
use crate::plugin_host::{
    provider::Completion,
    runtime::{Observer, ProviderRuntime},
};
use gamenode_plugin_protocol::{models, proto::*};
use gamenode_plugin_steam::discovery;
use tonic::Status;

pub(crate) struct Runtime {
    account: Option<Arc<SteamAccount>>,
    initialized: tokio::sync::Mutex<bool>,
    files: Arc<dyn gamenode_core::ports::HostFiles>,
    roots: Arc<dyn gamenode_core::ports::RootResolver>,
    platform: Platform,
}

struct Files(Arc<dyn gamenode_core::ports::HostFiles>);
impl discovery::Files for Files {
    fn canonicalize(&self, path: &str) -> std::io::Result<String> {
        self.0.canonicalize(path)
    }
    fn read_dir(&self, path: &str) -> std::io::Result<Vec<String>> {
        self.0.read_dir(path)
    }
    fn read_to_string(&self, path: &str) -> std::io::Result<String> {
        self.0.read_to_string(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        self.0.is_dir(path)
    }
}
struct Roots(Arc<dyn gamenode_core::ports::RootResolver>);
impl discovery::RootResolver for Roots {
    fn expand_home(&self, path: &str) -> Option<String> {
        self.0.expand_home(path)
    }
    fn registry_string(&self, key: &str, value: &str) -> Option<String> {
        self.0.registry_string(key, value)
    }
}

impl Runtime {
    pub(crate) fn new(ports: &Ports) -> Self {
        Self {
            account: ports
                .steam
                .clone()
                .map(|p| Arc::new(SteamAccount::without_observer(p.into_account_ports()))),
            initialized: tokio::sync::Mutex::new(false),
            files: ports.files.clone(),
            roots: ports.roots.clone(),
            platform: ports.platform,
        }
    }
    fn manifest() -> discovery::Manifest {
        toml::from_str(include_str!("../../../../../providers/steam.toml")).unwrap()
    }
    fn platform(&self) -> discovery::Platform {
        match self.platform {
            Platform::Linux => discovery::Platform::Linux,
            Platform::Windows => discovery::Platform::Windows,
        }
    }
    fn status(account: &SteamAccount) -> models::AccountStatus {
        let status = account.status();
        models::AccountStatus {
            state: match status.state {
                SteamAccountState::NotConnected => models::AccountState::NotConnected,
                SteamAccountState::Checking => models::AccountState::Checking,
                SteamAccountState::SigningIn => models::AccountState::SigningIn,
                SteamAccountState::Connected => models::AccountState::Connected,
                SteamAccountState::Expired => models::AccountState::Expired,
                SteamAccountState::Unreachable => models::AccountState::Unreachable,
                SteamAccountState::CredentialError => models::AccountState::CredentialError,
            },
            account_id: status.account_id,
            account_name: status.account_name,
            last_checked_at: status.last_checked_at,
            last_synced_at: status.last_synced_at,
            storage: match status.storage {
                gamenode_core::StorageMethod::PrivateFile => models::StorageMethod::PrivateFile,
                gamenode_core::StorageMethod::SecretService => models::StorageMethod::SecretService,
                gamenode_core::StorageMethod::WindowsCredentialManager => {
                    models::StorageMethod::WindowsCredentialManager
                }
            },
            persist_failed: status.persist_failed,
            catalog_error: status.catalog_error.map(|error| match error {
                gamenode_core::library::CatalogError::Unreachable => {
                    models::CatalogError::Unreachable
                }
                gamenode_core::library::CatalogError::RateLimited => {
                    models::CatalogError::RateLimited
                }
                gamenode_core::library::CatalogError::Server => models::CatalogError::Server,
                gamenode_core::library::CatalogError::Expired => models::CatalogError::Expired,
                gamenode_core::library::CatalogError::Privacy => models::CatalogError::Privacy,
                gamenode_core::library::CatalogError::Changed => models::CatalogError::Changed,
                gamenode_core::library::CatalogError::InvalidCookies => {
                    models::CatalogError::InvalidCookies
                }
                gamenode_core::library::CatalogError::FamilyUnavailable => {
                    models::CatalogError::FamilyUnavailable
                }
            }),
            family_error: status.family_error.map(|error| match error {
                gamenode_core::library::CatalogError::Unreachable => {
                    models::CatalogError::Unreachable
                }
                gamenode_core::library::CatalogError::RateLimited => {
                    models::CatalogError::RateLimited
                }
                gamenode_core::library::CatalogError::Server => models::CatalogError::Server,
                gamenode_core::library::CatalogError::Expired => models::CatalogError::Expired,
                gamenode_core::library::CatalogError::Privacy => models::CatalogError::Privacy,
                gamenode_core::library::CatalogError::Changed => models::CatalogError::Changed,
                gamenode_core::library::CatalogError::InvalidCookies => {
                    models::CatalogError::InvalidCookies
                }
                gamenode_core::library::CatalogError::FamilyUnavailable => {
                    models::CatalogError::FamilyUnavailable
                }
            }),
        }
    }
}

#[async_trait::async_trait]
impl ProviderRuntime for Runtime {
    fn accounts(&self) -> Vec<String> {
        if self.account.is_some() {
            vec!["steam".into()]
        } else {
            vec![]
        }
    }
    fn has_provider(&self, id: &str) -> bool {
        id == "steam"
    }
    fn status(&self, _: &str) -> Option<models::AccountStatus> {
        self.account.as_ref().map(|account| Self::status(account))
    }
    fn available(&self, _: &str) -> bool {
        true
    }
    fn set_observer(&self, observer: Observer) {
        if let Some(account) = &self.account {
            account.set_observer(move |_| observer());
        }
    }
    async fn shutdown(&self) {}
    async fn execute(&self, _: &str, operation: ProviderOperation) -> Result<Completion, Status> {
        let mut result = OperationResult {
            disposition: OperationDisposition::Completed as i32,
            ..Default::default()
        };
        let mut catalog = models::CatalogUpdate::default();
        if matches!(
            operation,
            ProviderOperation::Discover | ProviderOperation::ReadCounter
        ) {
            let files = Files(self.files.clone());
            let roots = Roots(self.roots.clone());
            let platform = self.platform();
            return tokio::task::spawn_blocking(move || {
                let manifest = Self::manifest();
                if operation == ProviderOperation::Discover {
                    let records = manifest
                        .scan(platform, &files, &roots)
                        .map_err(|_| Status::unavailable("fixture scan failed"))?;
                    result.discovery = Some(DiscoveryResult {
                        provider_id: "steam".into(),
                        records: records
                            .into_iter()
                            .map(|r| DiscoveryRecord {
                                id: r.id,
                                name: r.name.unwrap_or_default(),
                                install_dir: r.install_dir,
                                fields: r.fields.into_iter().collect(),
                            })
                            .collect(),
                    });
                } else {
                    let roots = manifest.resolve_roots(platform, &roots);
                    result.counters = discovery::counter::read(&discovery::ScanInput {
                        files: &files,
                        roots: &roots,
                    })
                    .into_iter()
                    .map(|(id, v)| CounterObservation {
                        id,
                        seconds: v.seconds,
                        last_played: v.last_played,
                    })
                    .collect();
                }
                Ok(Completion::fixture(result, catalog))
            })
            .await
            .map_err(|_| Status::unavailable("fixture worker failed"))?;
        }
        let Some(account) = self.account.clone() else {
            return Ok(Completion::fixture(result, catalog));
        };
        {
            let mut initialized = self.initialized.lock().await;
            if !*initialized {
                account.startup().await;
                *initialized = true;
            }
        }
        match operation {
            ProviderOperation::Initialize => {}
            ProviderOperation::Check => {
                let value = account.check().await;
                result.account_changed = value.account_changed;
                if value.superseded {
                    result.disposition = OperationDisposition::Superseded as i32;
                }
            }
            ProviderOperation::SyncCatalog => {
                let value = account.sync().await;
                result.account_changed = value.account_changed;
                if value.superseded {
                    result.disposition = OperationDisposition::Superseded as i32;
                }
                let category = |value: Option<
                    Result<
                        Vec<gamenode_core::library::CatalogRecord>,
                        gamenode_core::library::CatalogError,
                    >,
                >| {
                    value.map(|result| {
                        result
                            .map(|records| {
                                records
                                    .into_iter()
                                    .map(|r| models::CatalogRecord {
                                        id: r.id,
                                        name: r.name,
                                    })
                                    .collect()
                            })
                            .map_err(|error| match error {
                                gamenode_core::library::CatalogError::Unreachable => {
                                    models::CatalogError::Unreachable
                                }
                                gamenode_core::library::CatalogError::RateLimited => {
                                    models::CatalogError::RateLimited
                                }
                                gamenode_core::library::CatalogError::Server => {
                                    models::CatalogError::Server
                                }
                                gamenode_core::library::CatalogError::Expired => {
                                    models::CatalogError::Expired
                                }
                                gamenode_core::library::CatalogError::Privacy => {
                                    models::CatalogError::Privacy
                                }
                                gamenode_core::library::CatalogError::Changed => {
                                    models::CatalogError::Changed
                                }
                                gamenode_core::library::CatalogError::InvalidCookies => {
                                    models::CatalogError::InvalidCookies
                                }
                                gamenode_core::library::CatalogError::FamilyUnavailable => {
                                    models::CatalogError::FamilyUnavailable
                                }
                            })
                    })
                };
                catalog = models::CatalogUpdate {
                    owned: category(value.owned),
                    family: category(value.family),
                    now: value.now,
                };
            }
            ProviderOperation::SignIn => {
                let value = account.sign_in().await;
                result.account_changed = value.account_changed;
                result.login_outcome = Some(match value.result {
                    crate::engine::steam_account::SteamSignInResult::Connected => {
                        AccountLoginOutcome::Connected
                    }
                    crate::engine::steam_account::SteamSignInResult::Cancelled => {
                        AccountLoginOutcome::Cancelled
                    }
                    crate::engine::steam_account::SteamSignInResult::Closed => {
                        AccountLoginOutcome::Closed
                    }
                    crate::engine::steam_account::SteamSignInResult::TimedOut => {
                        AccountLoginOutcome::TimedOut
                    }
                    crate::engine::steam_account::SteamSignInResult::Failed => {
                        AccountLoginOutcome::Failed
                    }
                    crate::engine::steam_account::SteamSignInResult::Superseded => {
                        AccountLoginOutcome::Superseded
                    }
                } as i32);
                if value.result == crate::engine::steam_account::SteamSignInResult::Superseded {
                    result.disposition = OperationDisposition::Superseded as i32;
                }
            }
            ProviderOperation::SignOut => result.signed_out = account.sign_out().await.signed_out,
            ProviderOperation::FocusSignIn => {
                let a = account.clone();
                result.focused = Some(
                    tokio::task::spawn_blocking(move || a.focus_sign_in())
                        .await
                        .unwrap(),
                );
            }
            ProviderOperation::CancelSignIn => {
                let a = account.clone();
                tokio::task::spawn_blocking(move || a.cancel_sign_in())
                    .await
                    .unwrap();
            }
            _ => return Err(Status::unimplemented("fixture operation unsupported")),
        }
        result.status = Some(Self::status(&account).into());
        Ok(Completion::fixture(result, catalog))
    }
}
