//! Store-neutral dispatch of provider observations through Core policy.
use super::*;
use crate::plugin_host::runtime::ProviderRuntime;
use crate::plugin_host::services::{AccountPersistence, LibraryAccounts};
use gamenode_core::snapshot::StoreAccountState;
use gamenode_plugin_protocol::{
    models,
    proto::{AccountLoginOutcome, OperationDisposition, ProviderOperation},
};

/// This port is called only on provider storage workers. Waiting here preserves
/// the Engine's database queue order without blocking a command or UI callback.
struct EngineAccounts(std::sync::Weak<Shared>);

impl EngineAccounts {
    fn queued<T: Send + 'static>(
        &self,
        work: impl FnOnce(Engine, LibraryAccounts) -> Result<T, tonic::Status> + Send + 'static,
    ) -> Result<T, tonic::Status> {
        let shared = self
            .0
            .upgrade()
            .ok_or_else(|| tonic::Status::unavailable("account host unavailable"))?;
        let engine = Engine {
            shared: shared.clone(),
        };
        let accounts = LibraryAccounts(shared.ports.library.clone());
        let (send, receive) = mpsc::sync_channel(1);
        shared.storage.submit(move || {
            let _ = send.send(work(engine, accounts));
        });
        receive
            .recv()
            .map_err(|_| tonic::Status::unavailable("account storage unavailable"))?
    }
}

impl AccountPersistence for EngineAccounts {
    fn load(&self, provider: &str) -> Result<Option<models::StoreAccount>, tonic::Status> {
        let provider = provider.to_owned();
        self.queued(move |_, accounts| accounts.load(&provider))
    }

    fn save(
        &self,
        account: models::StoreAccount,
        clear_ownership: bool,
    ) -> Result<(), tonic::Status> {
        self.queued(move |engine, accounts| {
            let provider = account.provider.clone();
            let result = accounts.save(account, clear_ownership);
            if clear_ownership {
                engine.clear_store_catalog_memory(&provider);
            }
            result
        })
    }

    fn clear(&self, provider: &str) -> Result<(), tonic::Status> {
        let provider = provider.to_owned();
        self.queued(move |engine, accounts| {
            let result = accounts.clear(&provider);
            engine.clear_store_catalog_memory(&provider);
            result
        })
    }
}

pub(super) fn catalog_error(error: models::CatalogError) -> gamenode_core::library::CatalogError {
    use gamenode_core::library::CatalogError as C;
    use models::CatalogError as P;
    match error {
        P::Unreachable => C::Unreachable,
        P::RateLimited => C::RateLimited,
        P::Server => C::Server,
        P::Expired => C::Expired,
        P::Privacy => C::Privacy,
        P::Changed => C::Changed,
        P::InvalidCookies => C::InvalidCookies,
        P::FamilyUnavailable => C::FamilyUnavailable,
    }
}

fn category(
    value: Option<Result<Vec<models::CatalogRecord>, models::CatalogError>>,
) -> Option<Result<Vec<gamenode_core::library::CatalogRecord>, gamenode_core::library::CatalogError>>
{
    value.map(|result| {
        result
            .map(|records| {
                records
                    .into_iter()
                    .map(|r| gamenode_core::library::CatalogRecord {
                        id: r.id,
                        name: r.name,
                    })
                    .collect()
            })
            .map_err(catalog_error)
    })
}

impl Engine {
    pub fn set_provider_runtime(&self, runtime: Arc<dyn ProviderRuntime>) {
        runtime.set_account_persistence(Arc::new(EngineAccounts(Arc::downgrade(&self.shared))));
        let weak = Arc::downgrade(&self.shared);
        let background = self.shared.runtime.clone();
        runtime.set_observer(Arc::new(move || {
            let weak = weak.clone();
            background.spawn_blocking(move || {
                if let Some(shared) = weak.upgrade() {
                    Engine { shared }.publish_current();
                }
            });
        }));
        *self
            .shared
            .providers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(runtime);
    }
    pub fn provider_health(&self, id: &str) -> (bool, bool) {
        self.provider_runtime()
            .map(|runtime| (runtime.available(id), runtime.connected(id)))
            .unwrap_or((false, false))
    }

    pub fn provider_availability(&self, id: &str) -> &'static str {
        self.provider_runtime()
            .map(|runtime| runtime.availability(id))
            .unwrap_or("unavailable")
    }

    pub(super) fn provider_runtime(&self) -> Option<Arc<dyn ProviderRuntime>> {
        self.shared
            .providers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(super) fn provider_account_views(
        &self,
        library: Option<&gamenode_core::library::LibraryView>,
    ) -> Vec<StoreAccountView> {
        let Some(runtime) = self.provider_runtime() else {
            return vec![];
        };
        runtime
            .accounts()
            .into_iter()
            .filter_map(|provider| {
                let status = runtime.status(&provider)?;
                let available = runtime.available(&provider);
                let count =
                    library.and_then(|view| view.providers.iter().find(|p| p.id == provider));
                let state = match status.state {
                    models::AccountState::NotConnected => StoreAccountState::NotConnected,
                    models::AccountState::Checking => StoreAccountState::Checking,
                    models::AccountState::SigningIn => StoreAccountState::SigningIn,
                    models::AccountState::Connected => StoreAccountState::Connected,
                    models::AccountState::Expired => StoreAccountState::Expired,
                    models::AccountState::Unreachable => StoreAccountState::Unreachable,
                    models::AccountState::CredentialError => StoreAccountState::CredentialError,
                };
                Some(StoreAccountView {
                    provider,
                    state,
                    account_id: status.account_id.clone(),
                    account_name: status.account_name.clone(),
                    owned_count: count.map_or(0, |c| c.owned_count),
                    family_count: count.map_or(0, |c| c.family_count),
                    installed_count: count.map_or(0, |c| c.installed_count),
                    last_checked_at: status.last_checked_at,
                    last_synced_at: status.last_synced_at,
                    storage: Some(match status.storage {
                        models::StorageMethod::PrivateFile => {
                            gamenode_core::StorageMethod::PrivateFile
                        }
                        models::StorageMethod::SecretService => {
                            gamenode_core::StorageMethod::SecretService
                        }
                        models::StorageMethod::WindowsCredentialManager => {
                            gamenode_core::StorageMethod::WindowsCredentialManager
                        }
                    }),
                    persist_failed: status.persist_failed,
                    catalog_error: status.catalog_error.map(catalog_error),
                    family_error: status.family_error.map(catalog_error),
                    can_sign_in: available && status.can_sign_in(),
                    can_focus_sign_in: available && status.can_focus_sign_in(),
                    can_cancel_sign_in: available && status.can_cancel_sign_in(),
                    can_check: available && status.can_check(),
                    can_sign_out: available && status.can_sign_out(),
                    can_remove_credential: available && status.can_remove_credential(),
                })
            })
            .collect()
    }
    pub(super) fn start_providers(&self) {
        if let Some(runtime) = self.provider_runtime() {
            for provider in runtime.accounts() {
                let engine = self.clone();
                self.shared.runtime.spawn(async move {
                    engine
                        .provider_action(provider, ProviderOperation::Check)
                        .await;
                });
            }
        }
    }
    pub async fn provider_sign_in(&self, id: &str) {
        self.provider_action(id.to_owned(), ProviderOperation::SignIn)
            .await;
    }
    pub async fn provider_focus_sign_in(&self, id: &str) {
        self.provider_action(id.to_owned(), ProviderOperation::FocusSignIn)
            .await;
    }
    pub async fn provider_cancel_sign_in(&self, id: &str) {
        self.provider_action(id.to_owned(), ProviderOperation::CancelSignIn)
            .await;
    }
    pub async fn provider_sign_out(&self, id: &str) {
        self.provider_action(id.to_owned(), ProviderOperation::SignOut)
            .await;
    }
    pub async fn provider_check(&self, id: &str) {
        if !self
            .provider_runtime()
            .is_some_and(|r| r.accounts().iter().any(|p| p == id))
        {
            return;
        }
        if self.library_is_active() {
            self.request_catalog();
        } else {
            self.provider_action(id.to_owned(), ProviderOperation::Check)
                .await;
        }
    }
    fn provider_check_gate(&self, provider: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.shared
            .provider_checks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(provider.to_owned())
            .or_default()
            .clone()
    }

    async fn provider_action(&self, provider: String, operation: ProviderOperation) {
        let Some(runtime) = self.provider_runtime() else {
            return;
        };
        if !runtime.accounts().contains(&provider) {
            return;
        }
        // A startup check can report the same identity replacement as a joined
        // catalog sync. Serialize check execution AND its commit with sync so a
        // late check cannot clear the newly imported catalog. Sign-out/sign-in
        // remain free to preempt network work through host epoch invalidation.
        let _check = if operation == ProviderOperation::Check {
            Some(self.provider_check_gate(&provider).lock_owned().await)
        } else {
            None
        };
        let Ok(completion) = runtime.execute(&provider, operation).await else {
            self.publish_current();
            return;
        };
        let engine = self.clone();
        let _ = completion
            .commit(move |result, _| {
                if result.disposition == OperationDisposition::Superseded as i32 {
                    return;
                }
                if result.signed_out || result.account_changed {
                    engine.clear_store_catalog(&provider);
                }
                if result.login_outcome == Some(AccountLoginOutcome::Connected as i32)
                    || (result.account_changed
                        && result.status.is_some_and(|s| {
                            s.state
                                == gamenode_plugin_protocol::proto::AccountConnection::Connected
                                    as i32
                        }))
                {
                    engine.request_catalog();
                }
                engine.publish_current();
            })
            .await;
    }
    pub(super) fn scan_provider(
        &self,
        runtime: Arc<dyn ProviderRuntime>,
        found: gamenode_core::provider::Provider,
        op: u64,
        provider: String,
    ) {
        let engine = self.clone();
        self.shared.runtime.spawn(async move {
            match runtime
                .execute(&provider, ProviderOperation::Discover)
                .await
            {
                Ok(completion) => {
                    let failed_engine = engine.clone();
                    let failed_provider = provider.clone();
                    let committed = completion
                        .commit(move |result, _| {
                            let result =
                                if result.disposition == OperationDisposition::Completed as i32 {
                                    result
                                        .discovery
                                        .filter(|d| d.provider_id == provider)
                                        .map(|d| {
                                            gamenode_core::provider::record::finish_records(
                                                &provider,
                                                d.records
                                                    .into_iter()
                                                    .map(|r| gamenode_core::provider::RawRecord {
                                                        id: Some(r.id),
                                                        name: Some(r.name),
                                                        install_dir: Some(r.install_dir),
                                                        fields: r.fields.into_iter().collect(),
                                                    })
                                                    .collect(),
                                                &found.manifest().exclude_ids,
                                            )
                                        })
                                        .ok_or_else(|| {
                                            "provider discovery returned no observations".into()
                                        })
                                } else {
                                    Err("provider discovery failed".into())
                                };
                            engine.library_input(LibInput::ProviderScanned {
                                op,
                                provider,
                                result,
                                now: unix_now(),
                            });
                        })
                        .await;
                    if committed.is_err() {
                        failed_engine.library_input(LibInput::ProviderScanned {
                            op,
                            provider: failed_provider,
                            result: Err("provider discovery superseded".into()),
                            now: unix_now(),
                        });
                    }
                }
                Err(_) => engine.library_input(LibInput::ProviderScanned {
                    op,
                    provider,
                    result: Err("provider discovery unavailable".into()),
                    now: unix_now(),
                }),
            }
        });
    }
    pub(super) async fn sync_provider_catalog(&self, op: u64, provider: String) {
        let _check = self.provider_check_gate(&provider).lock_owned().await;
        let Some(runtime) = self.provider_runtime() else {
            self.library_input(LibInput::CatalogSynced {
                op,
                provider,
                owned: None,
                family: None,
                now: unix_now(),
            });
            return;
        };
        if runtime.status(&provider).is_some_and(|status| {
            matches!(
                status.state,
                models::AccountState::Expired
                    | models::AccountState::Unreachable
                    | models::AccountState::CredentialError
            ) || (status.state == models::AccountState::NotConnected
                && status.last_checked_at.is_some())
        }) {
            self.library_input(LibInput::CatalogSynced {
                op,
                provider,
                owned: None,
                family: None,
                now: unix_now(),
            });
            return;
        }
        let mut completion = runtime
            .execute(&provider, ProviderOperation::SyncCatalog)
            .await;
        // A sync may join the startup session check. Once that check is done,
        // request the actual catalog once instead of accepting no observations.
        let retry = completion.as_ref().is_ok_and(|c| {
            (c.catalog.owned.is_none() && c.catalog.family.is_none()
                || c.result.disposition == OperationDisposition::Superseded as i32)
                && runtime
                    .status(&provider)
                    .is_some_and(|s| s.state == models::AccountState::Connected)
        });
        if retry {
            if let Ok(previous) = completion
                && previous.result.account_changed
            {
                let engine = self.clone();
                let provider = provider.clone();
                let _ = previous
                    .commit(move |_, _| engine.clear_store_catalog(&provider))
                    .await;
            }
            completion = runtime
                .execute(&provider, ProviderOperation::SyncCatalog)
                .await;
        }
        let engine = self.clone();
        let fallback = || {
            self.library_input(LibInput::CatalogSynced {
                op,
                provider: provider.clone(),
                owned: Some(Err(gamenode_core::library::CatalogError::Unreachable)),
                family: None,
                now: unix_now(),
            })
        };
        match completion {
            Ok(completion) => {
                let provider = provider.clone();
                let result = completion
                    .commit(move |result, catalog| {
                        if result.account_changed || result.signed_out {
                            engine.clear_store_catalog(&provider);
                            if !result.signed_out
                                && result.status.is_some_and(|s| {
                                    s.state
                                    == gamenode_plugin_protocol::proto::AccountConnection::Connected
                                        as i32
                                })
                            {
                                engine.request_catalog();
                            }
                            return;
                        }
                        let (owned, family) =
                            if result.disposition == OperationDisposition::Completed as i32 {
                                (category(catalog.owned), category(catalog.family))
                            } else {
                                (None, None)
                            };
                        if owned.is_some() || family.is_some() {
                            engine.persist_catalog(
                                provider.clone(),
                                CatalogUpdate {
                                    owned: owned.clone(),
                                    family: family.clone(),
                                    now: catalog.now,
                                },
                            );
                        }
                        engine.library_input(LibInput::CatalogSynced {
                            op,
                            provider,
                            owned,
                            family,
                            now: catalog.now,
                        });
                    })
                    .await;
                if result.is_err() {
                    fallback();
                }
            }
            Err(_) => fallback(),
        }
    }
}
