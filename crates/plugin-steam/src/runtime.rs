//! Persistent provider implementation. All application access uses the public
//! SDK; native file/registry access stays on the plugin's blocking workers.

use crate::{
    account::{SteamAccount, SteamAccountPorts, SystemClock},
    bridge::{LoginAttempt, SdkLogin, SdkStorage},
    discovery::{Manifest, Platform, ScanInput, StdFiles, SystemRoots},
    http::SteamClient,
};
use gamenode_plugin_protocol::{models, proto::*};
use gamenode_plugin_sdk::{Error, HostClient, ProviderPlugin};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
struct Contexts {
    epoch: u64,
    operations: Vec<std::sync::Weak<AtomicBool>>,
}

pub struct SteamPlugin {
    client: Arc<dyn crate::steam::SteamWebApi>,
    account: Mutex<Option<Arc<SteamAccount>>>,
    login: LoginAttempt,
    contexts: Mutex<Contexts>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl SteamPlugin {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            client: Arc::new(SteamClient::new().map_err(|_| Error::Unavailable)?),
            account: Mutex::new(None),
            login: LoginAttempt::default(),
            contexts: Mutex::new(Contexts::default()),
        })
    }

    fn validity(&self, epoch: u64) -> Arc<AtomicBool> {
        let mut contexts = lock(&self.contexts);
        let valid = Arc::new(AtomicBool::new(epoch >= contexts.epoch));
        if epoch > contexts.epoch {
            for old in contexts.operations.drain(..).filter_map(|w| w.upgrade()) {
                old.store(false, Ordering::Release);
            }
            contexts.epoch = epoch;
        }
        contexts.operations.retain(|w| w.strong_count() > 0);
        contexts.operations.push(Arc::downgrade(&valid));
        valid
    }

    fn account(
        &self,
        host: HostClient,
        method: models::StorageMethod,
        valid: Arc<AtomicBool>,
    ) -> Arc<SteamAccount> {
        let storage = Arc::new(SdkStorage::new(host.clone(), method, valid.clone()));
        let ports = SteamAccountPorts {
            credentials: storage.clone(),
            accounts: storage,
            client: self.client.clone(),
            login: Arc::new(SdkLogin::new(host, self.login.clone(), valid.clone())),
            clock: Arc::new(SystemClock),
        };
        let mut base = lock(&self.account);
        if let Some(base) = base.as_ref() {
            Arc::new(base.for_operation(ports, valid))
        } else {
            let account = Arc::new(SteamAccount::without_observer(ports.clone()));
            let scoped = Arc::new(account.for_operation(ports, valid));
            *base = Some(account);
            scoped
        }
    }
}

fn success() -> OperationResult {
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

async fn category(
    host: &HostClient,
    category: CatalogCategory,
    value: Option<Result<Vec<models::CatalogRecord>, models::CatalogError>>,
    excluded: &BTreeSet<String>,
) -> Result<Option<CatalogCompletion>, Error> {
    Ok(match value {
        None => None,
        Some(Err(error)) => Some(CatalogCompletion {
            category: category as i32,
            outcome: Some(catalog_completion::Outcome::Failed(
                CatalogFailure::from(error) as i32,
            )),
        }),
        Some(Ok(mut records)) => {
            records.retain(|r| !excluded.contains(&r.id));
            Some(host.catalog_chunks(category, records).await?)
        }
    })
}

#[async_trait::async_trait]
impl ProviderPlugin for SteamPlugin {
    async fn handle(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error> {
        let kind = ProviderOperation::try_from(command.operation).map_err(|_| Error::Protocol)?;
        let valid = if matches!(
            kind,
            ProviderOperation::Discover | ProviderOperation::ReadCounter
        ) {
            Arc::new(AtomicBool::new(true))
        } else {
            self.validity(host.context().account_epoch)
        };
        if !valid.load(Ordering::Acquire) {
            return Ok(superseded());
        }
        let context = host.provider_context().await?;
        let manifest: Manifest =
            toml::from_str(&context.manifest_toml).map_err(|_| Error::Protocol)?;
        if context.provider_id != "steam" || manifest.id != "steam" {
            return Err(Error::Denied);
        }
        let platform = match context.platform.as_str() {
            "linux" => Platform::Linux,
            "windows" => Platform::Windows,
            _ => return Err(Error::Protocol),
        };
        if kind == ProviderOperation::Discover {
            return tokio::task::spawn_blocking(move || {
                manifest
                    .scan(platform, &StdFiles, &SystemRoots)
                    .map(|records| OperationResult {
                        discovery: Some(DiscoveryResult {
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
                        }),
                        ..success()
                    })
                    .map_err(|_| Error::Unavailable)
            })
            .await
            .map_err(|_| Error::Unavailable)?;
        }
        if kind == ProviderOperation::ReadCounter {
            return tokio::task::spawn_blocking(move || {
                let roots = manifest.resolve_roots(platform, &SystemRoots);
                let totals = crate::discovery::counter::read(&ScanInput {
                    roots: &roots,
                    files: &StdFiles,
                });
                OperationResult {
                    counters: totals
                        .into_iter()
                        .map(|(id, value)| CounterObservation {
                            id,
                            seconds: value.seconds,
                            last_played: value.last_played,
                        })
                        .collect(),
                    ..success()
                }
            })
            .await
            .map_err(|_| Error::Unavailable);
        }
        if !valid.load(Ordering::Acquire) {
            return Ok(superseded());
        }
        let method = SecretStorage::try_from(context.secret_storage)
            .map_err(|_| Error::Protocol)?
            .try_into()?;
        let account = self.account(host.clone(), method, valid.clone());
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
        // Do not use `?` until the observer has been dropped and its queue
        // drained, including failures while streaming catalog chunks.
        let result = async {
            Ok(match kind {
                ProviderOperation::Initialize => {
                    host.declare_actions(ActionDeclaration {
                        play_uri: manifest.launch.map(|a| a.uri),
                        install_uri: manifest.install.map(|a| a.uri),
                    })
                    .await?;
                    OperationResult {
                        status: Some(account.startup().await.into()),
                        ..success()
                    }
                }
                ProviderOperation::SignIn => {
                    let result = account.sign_in().await;
                    let disposition = match result.result {
                        models::SignInResult::Connected => OperationDisposition::Completed,
                        models::SignInResult::Cancelled | models::SignInResult::Closed => {
                            OperationDisposition::Cancelled
                        }
                        models::SignInResult::Superseded => OperationDisposition::Superseded,
                        _ => OperationDisposition::Failed,
                    };
                    OperationResult {
                        disposition: disposition as i32,
                        status: Some(result.status.into()),
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
                        status: Some(account.status().into()),
                        focused: Some(focused),
                        ..success()
                    }
                }
                ProviderOperation::CancelSignIn | ProviderOperation::Cancel => {
                    let current = account.clone();
                    let status = tokio::task::spawn_blocking(move || current.cancel_sign_in())
                        .await
                        .map_err(|_| Error::Unavailable)?;
                    OperationResult {
                        status: Some(status.into()),
                        ..success()
                    }
                }
                ProviderOperation::SignOut => {
                    let result = account.sign_out().await;
                    OperationResult {
                        signed_out: result.signed_out,
                        status: Some(result.status.into()),
                        ..success()
                    }
                }
                ProviderOperation::Check => {
                    let result = account.check().await;
                    OperationResult {
                        status: Some(result.status.into()),
                        account_changed: result.account_changed,
                        ..if result.superseded {
                            superseded()
                        } else {
                            success()
                        }
                    }
                }
                ProviderOperation::SyncCatalog => {
                    let result = account.sync().await;
                    if result.superseded {
                        return Ok(superseded());
                    }
                    let excluded: BTreeSet<_> = manifest.exclude_ids.into_iter().collect();
                    let mut catalogs = vec![];
                    if let Some(value) =
                        category(&host, CatalogCategory::Owned, result.owned, &excluded).await?
                    {
                        catalogs.push(value);
                    }
                    if let Some(value) =
                        category(&host, CatalogCategory::Family, result.family, &excluded).await?
                    {
                        catalogs.push(value);
                    }
                    OperationResult {
                        status: Some(result.status.into()),
                        account_changed: result.account_changed,
                        observed_at: result.now,
                        catalogs,
                        ..success()
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
    }
}

#[cfg(all(test, unix))]
mod tests;
