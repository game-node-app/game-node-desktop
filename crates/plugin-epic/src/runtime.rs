//! Persistent provider implementation. All application access uses the public SDK; native file and
//! registry reads stay on the plugin's blocking workers.

use std::{future::Future, pin::Pin, time::Duration};

use gamenode_plugin_protocol::proto::*;
use gamenode_plugin_sdk::{Error, HostClient, ProviderPlugin};

use crate::{
    PROVIDER_ID,
    account::{
        EpicAccount, EpicAccountPorts, EpicAccountStatus, EpicSignInResult, SYNC_BUDGET,
        SystemClock,
    },
    bridge::{LoginAttempt, SdkLogin, SdkStorage},
    discovery::{Manifest, StdFiles, SystemRoots},
    http::EpicClient,
};

type HandlerFuture<'a> = Pin<Box<dyn Future<Output = Result<OperationResult, Error>> + Send + 'a>>;

#[derive(Default)]
struct Contexts {
    epoch: u64,
    operations: Vec<std::sync::Weak<std::sync::atomic::AtomicBool>>,
}

fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct EpicPlugin {
    client: std::sync::Arc<dyn crate::account::EpicApi>,
    account: std::sync::Mutex<Option<std::sync::Arc<EpicAccount>>>,
    login: LoginAttempt,
    contexts: std::sync::Mutex<Contexts>,
    /// Tests accelerate this; production stays inside the host's 300-second budget.
    catalog_budget: Duration,
}

impl EpicPlugin {
    pub fn new() -> Result<Self, Error> {
        Ok(Self::with_client(std::sync::Arc::new(
            EpicClient::new().map_err(|_| Error::Unavailable)?,
        )))
    }

    fn with_client(client: std::sync::Arc<dyn crate::account::EpicApi>) -> Self {
        Self {
            client,
            account: std::sync::Mutex::new(None),
            login: LoginAttempt::default(),
            contexts: std::sync::Mutex::new(Contexts::default()),
            catalog_budget: SYNC_BUDGET,
        }
    }

    fn validity(&self, epoch: u64) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        use std::sync::atomic::{AtomicBool, Ordering};
        let mut contexts = lock(&self.contexts);
        let valid = std::sync::Arc::new(AtomicBool::new(epoch >= contexts.epoch));
        if epoch > contexts.epoch {
            for old in contexts.operations.drain(..).filter_map(|w| w.upgrade()) {
                old.store(false, Ordering::Release);
            }
            contexts.epoch = epoch;
        }
        contexts.operations.retain(|w| w.strong_count() > 0);
        contexts.operations.push(std::sync::Arc::downgrade(&valid));
        valid
    }

    fn account(
        &self,
        host: HostClient,
        method: gamenode_plugin_protocol::models::StorageMethod,
        valid: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> std::sync::Arc<EpicAccount> {
        let storage = std::sync::Arc::new(SdkStorage::new(host.clone(), method, valid.clone()));
        let ports = EpicAccountPorts {
            credentials: storage.clone(),
            accounts: storage,
            api: self.client.clone(),
            login: std::sync::Arc::new(SdkLogin::new(host, self.login.clone(), valid.clone())),
            clock: std::sync::Arc::new(SystemClock),
            catalog_budget: self.catalog_budget,
        };
        let mut base = lock(&self.account);
        if let Some(base) = base.as_ref() {
            std::sync::Arc::new(base.for_operation(ports, valid))
        } else {
            let account = std::sync::Arc::new(EpicAccount::without_observer(ports.clone()));
            let scoped = std::sync::Arc::new(account.for_operation(ports, valid));
            *base = Some(account);
            scoped
        }
    }
}

fn completed() -> OperationResult {
    OperationResult {
        disposition: OperationDisposition::Completed as i32,
        ..Default::default()
    }
}

fn superseded() -> OperationResult {
    OperationResult {
        disposition: OperationDisposition::Superseded as i32,
        ..Default::default()
    }
}

fn status(status: EpicAccountStatus) -> AccountStatus {
    status.into()
}

async fn owned_category(
    host: &HostClient,
    value: Option<
        Result<
            Vec<gamenode_plugin_protocol::models::CatalogRecord>,
            gamenode_plugin_protocol::models::CatalogError,
        >,
    >,
) -> Result<Option<CatalogCompletion>, Error> {
    Ok(match value {
        None => None,
        Some(Err(error)) => Some(CatalogCompletion {
            category: CatalogCategory::Owned as i32,
            outcome: Some(catalog_completion::Outcome::Failed(
                CatalogFailure::from(error) as i32,
            )),
        }),
        Some(Ok(records)) => Some(host.catalog_chunks(CatalogCategory::Owned, records).await?),
    })
}

impl ProviderPlugin for EpicPlugin {
    fn handle<'life0, 'async_trait>(
        &'life0 self,
        command: ProviderCommand,
        host: HostClient,
    ) -> HandlerFuture<'async_trait>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            use std::sync::atomic::Ordering;

            let kind =
                ProviderOperation::try_from(command.operation).map_err(|_| Error::Protocol)?;
            let valid = if matches!(
                kind,
                ProviderOperation::Discover | ProviderOperation::ReadCounter
            ) {
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true))
            } else {
                self.validity(host.context().account_epoch)
            };
            if !valid.load(Ordering::Acquire) {
                return Ok(superseded());
            }
            let context = host.provider_context().await?;
            let manifest: Manifest =
                toml::from_str(&context.manifest_toml).map_err(|_| Error::Protocol)?;
            if context.provider_id != PROVIDER_ID || manifest.id != PROVIDER_ID {
                return Err(Error::Denied);
            }
            if context.platform != "windows" {
                return Err(Error::Denied);
            }
            if kind == ProviderOperation::Discover {
                let installed =
                    tokio::task::spawn_blocking(move || manifest.scan(&StdFiles, &SystemRoots))
                        .await
                        .map_err(|_| Error::Unavailable)?
                        .map_err(|error| {
                            tracing::warn!(error = %error, "Epic installed discovery failed");
                            Error::Unavailable
                        })?;
                return Ok(OperationResult {
                    discovery: Some(DiscoveryResult {
                        provider_id: PROVIDER_ID.into(),
                        records: installed
                            .into_iter()
                            .map(|record| DiscoveryRecord {
                                id: record.id,
                                name: record.name.unwrap_or_default(),
                                install_dir: record.install_dir,
                                fields: record.fields.into_iter().collect(),
                            })
                            .collect(),
                    }),
                    ..completed()
                });
            }
            if !valid.load(Ordering::Acquire) {
                return Ok(superseded());
            }
            let method = SecretStorage::try_from(context.secret_storage)
                .map_err(|_| Error::Protocol)?
                .try_into()?;
            let account = self.account(host.clone(), method, valid.clone());

            // The counter is read-only account data and the host grants it neither secret nor
            // account-publish access. It uses the in-memory access token from the last check.
            if kind == ProviderOperation::ReadCounter {
                let counters = account
                    .counter()
                    .await
                    .into_iter()
                    .map(|total| CounterObservation {
                        // Library entries, display and submissions use the application name;
                        // Epic's ledger artifact id is only a lookup key.
                        id: total.app_name,
                        seconds: total.seconds,
                        // Epic's ledger has no last-played date.
                        last_played: None,
                    })
                    .collect();
                return Ok(OperationResult {
                    counters,
                    ..completed()
                });
            }

            let (sender, mut receiver) = tokio::sync::mpsc::channel(32);
            account.set_observer(move |status| {
                let _ = sender.try_send(status.clone());
            });
            let sink = host.clone();
            let publication = tokio::spawn(async move {
                while let Some(status) = receiver.recv().await {
                    sink.publish_status(status).await?;
                }
                Ok::<_, Error>(())
            });
            // Do not use `?` until the observer has been dropped and its queue drained,
            // including failures while streaming catalog chunks.
            let result = async {
                Ok(match kind {
                    ProviderOperation::Initialize => {
                        host.declare_actions(ActionDeclaration {
                            play_uri: manifest.launch.and_then(|action| action.uri),
                            install_uri: manifest.install.and_then(|action| action.uri),
                        })
                        .await?;
                        OperationResult {
                            status: Some(status(account.startup().await)),
                            ..completed()
                        }
                    }
                    ProviderOperation::SignIn => {
                        let result = account.sign_in().await;
                        let disposition = match result.result {
                            EpicSignInResult::Connected => OperationDisposition::Completed,
                            EpicSignInResult::Cancelled | EpicSignInResult::Closed => {
                                OperationDisposition::Cancelled
                            }
                            EpicSignInResult::Superseded => OperationDisposition::Superseded,
                            _ => OperationDisposition::Failed,
                        };
                        OperationResult {
                            disposition: disposition as i32,
                            status: Some(status(result.status)),
                            account_changed: result.account_changed,
                            login_outcome: Some(AccountLoginOutcome::from(result.result) as i32),
                            ..Default::default()
                        }
                    }
                    ProviderOperation::FocusSignIn => {
                        let current = account.clone();
                        let focused = tokio::task::spawn_blocking(move || current.focus_sign_in())
                            .await
                            .map_err(|_| Error::Unavailable)?;
                        OperationResult {
                            status: Some(status(account.status())),
                            focused: Some(focused),
                            ..completed()
                        }
                    }
                    ProviderOperation::CancelSignIn | ProviderOperation::Cancel => {
                        let current = account.clone();
                        let current_status =
                            tokio::task::spawn_blocking(move || current.cancel_sign_in())
                                .await
                                .map_err(|_| Error::Unavailable)?;
                        OperationResult {
                            status: Some(status(current_status)),
                            ..completed()
                        }
                    }
                    ProviderOperation::SignOut => {
                        let result = account.sign_out().await;
                        OperationResult {
                            signed_out: result.signed_out,
                            status: Some(status(result.status)),
                            ..completed()
                        }
                    }
                    ProviderOperation::Check => {
                        let result = account.check().await;
                        OperationResult {
                            status: Some(status(result.status)),
                            account_changed: result.account_changed,
                            ..if result.superseded {
                                superseded()
                            } else {
                                completed()
                            }
                        }
                    }
                    ProviderOperation::SyncCatalog => {
                        let result = account.sync().await;
                        if result.superseded {
                            return Ok(superseded());
                        }
                        let mut catalogs = vec![];
                        if let Some(value) = owned_category(&host, result.owned).await? {
                            catalogs.push(value);
                        }
                        OperationResult {
                            status: Some(status(result.status)),
                            account_changed: result.account_changed,
                            observed_at: result.now,
                            catalogs,
                            ..completed()
                        }
                    }
                    _ => return Err(Error::Protocol),
                })
            }
            .await;
            drop(account);
            let published = publication.await.map_err(|_| Error::Unavailable)?;
            published?;
            if !valid.load(Ordering::Acquire) {
                return Ok(superseded());
            }
            result
        })
    }
}

// The reference host binds a Unix socket, so these integration-style tests run on Unix only.
#[cfg(all(test, unix))]
mod tests;
