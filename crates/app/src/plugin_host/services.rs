//! Scoped adapters between the public provider API and host-owned persistence.
//! A registration captures its stores; callers cannot choose an OS credential
//! key, database path, another provider or the GameNode authentication store.

use super::{
    login::LoginBroker,
    provider::{HostServices, Lease},
};
use gamenode_core::{
    SessionCredential, StorageMethod,
    library::LibraryStore,
    ports::{CredentialError, CredentialStore},
};
use gamenode_plugin_protocol::{login::LoginPolicy, models, proto::*};
use std::sync::{Arc, Mutex};
use tonic::Status;

#[derive(Default)]
struct Failures {
    read: bool,
    mutation: bool,
}

impl Failures {
    fn record(&mut self, read: bool, failed: bool) {
        if read {
            self.read = failed;
        } else {
            self.mutation = failed;
        }
    }
    fn any(&self) -> bool {
        self.read || self.mutation
    }
}

#[derive(Default)]
struct Persistence {
    secret: Failures,
    account: Failures,
}

pub struct ProviderServices {
    context: ProviderContext,
    credentials: Option<Arc<dyn CredentialStore>>,
    accounts: Mutex<Arc<dyn AccountPersistence>>,
    identity: Arc<Mutex<Option<String>>>,
    login: Option<Arc<LoginBroker>>,
    login_policy: Option<Arc<LoginPolicy>>,
    actions: ActionDeclaration,
    persistence: Arc<Mutex<Persistence>>,
    status: Mutex<Option<models::AccountStatus>>,
    observer: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

/// Blocking, host-owned account persistence. Production routes these calls
/// through the Engine's FIFO storage worker; the plugin never sees this port.
pub trait AccountPersistence: Send + Sync {
    fn load(&self, provider: &str) -> Result<Option<models::StoreAccount>, Status>;
    fn save(&self, account: models::StoreAccount, clear_ownership: bool) -> Result<(), Status>;
    fn clear(&self, provider: &str) -> Result<(), Status>;
}

pub struct LibraryAccounts(pub Arc<dyn LibraryStore>);
impl AccountPersistence for LibraryAccounts {
    fn load(&self, provider: &str) -> Result<Option<models::StoreAccount>, Status> {
        self.0
            .load_store_accounts()
            .map(|accounts| {
                accounts
                    .into_iter()
                    .find(|a| a.provider == provider)
                    .map(|a| models::StoreAccount {
                        provider: a.provider,
                        account_id: a.account_id,
                        account_name: a.account_name,
                        last_verified_at: a.last_verified_at,
                    })
            })
            .map_err(|_| Status::unavailable("account storage unavailable"))
    }
    fn save(&self, account: models::StoreAccount, clear_ownership: bool) -> Result<(), Status> {
        if clear_ownership {
            self.0
                .clear_catalog(&account.provider)
                .map_err(|_| Status::unavailable("account catalog storage unavailable"))?;
        }
        self.0
            .save_store_account(&gamenode_core::library::StoreAccount {
                provider: account.provider,
                account_id: account.account_id,
                account_name: account.account_name,
                last_verified_at: account.last_verified_at,
            })
            .map_err(|_| Status::unavailable("account storage unavailable"))
    }
    fn clear(&self, provider: &str) -> Result<(), Status> {
        self.0
            .clear_catalog(provider)
            .and_then(|_| self.0.clear_store_account(provider))
            .map_err(|_| Status::unavailable("account storage unavailable"))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn storage(method: StorageMethod) -> SecretStorage {
    match method {
        StorageMethod::WindowsCredentialManager => SecretStorage::WindowsCredentialManager,
        StorageMethod::SecretService => SecretStorage::SecretService,
        StorageMethod::PrivateFile => SecretStorage::PrivateFile,
    }
}

impl ProviderServices {
    /// Open only the credential slot approved by the trusted descriptor. OS credential
    /// initialization and descriptor parsing run on a worker.
    pub async fn bundled(
        provider_id: &str,
        paths: gamenode_platform::paths::DataPaths,
        library: Arc<dyn LibraryStore>,
        login: Option<Arc<LoginBroker>>,
    ) -> Result<Self, Status> {
        let descriptor = super::descriptor_for_provider(provider_id)
            .ok_or_else(|| Status::unavailable("provider is not registered"))?;
        if descriptor.provider_manifest != descriptor.provider_id {
            return Err(Status::failed_precondition(
                "provider manifest does not match the descriptor",
            ));
        }
        if gamenode_core::provider::Platform::current()
            .is_none_or(|platform| !descriptor.supports(platform))
        {
            return Err(Status::unavailable("provider platform unavailable"));
        }
        tokio::task::spawn_blocking(move || {
            let credentials = descriptor.uses_secrets().then(|| {
                gamenode_platform::credentials::open_store(&paths, descriptor.credential_slot())
            });
            Self::new(
                ProviderContext {
                    provider_id: descriptor.provider_id.clone(),
                    platform: std::env::consts::OS.into(),
                    manifest_toml: descriptor.manifest_toml.clone(),
                    granted_capabilities: descriptor.capabilities.clone(),
                    ..Default::default()
                },
                credentials,
                library,
                login,
                descriptor.approved_login()?,
            )
        })
        .await
        .map_err(|_| Status::unavailable("provider services unavailable"))?
    }

    /// `context`, stores and policy come from trusted host composition, never a
    /// runtime request. Passing the existing OS store preserves its physical
    /// credential alias without copying or migrating secret material.
    pub fn new(
        mut context: ProviderContext,
        credentials: Option<Arc<dyn CredentialStore>>,
        library: Arc<dyn LibraryStore>,
        login: Option<Arc<LoginBroker>>,
        login_policy: Option<LoginPolicy>,
    ) -> Result<Self, Status> {
        let manifest: gamenode_core::provider::Manifest = toml::from_str(&context.manifest_toml)
            .map_err(|_| Status::failed_precondition("provider configuration unavailable"))?;
        if manifest.id != context.provider_id {
            return Err(Status::failed_precondition(
                "provider configuration mismatch",
            ));
        }
        if let Some(policy) = &login_policy {
            policy
                .validate()
                .map_err(|_| Status::failed_precondition("login configuration unavailable"))?;
        }
        let uri = |action| match action {
            Some(gamenode_core::provider::manifest::UriAction::Uri { uri }) => Some(uri),
            _ => None,
        };
        let actions = ActionDeclaration {
            play_uri: uri(manifest.launch),
            install_uri: uri(manifest.install),
        };
        context.secret_storage = credentials
            .as_ref()
            .map_or(SecretStorage::Unspecified, |s| storage(s.method()))
            as i32;
        context.max_records = super::MAX_DISCOVERY_RECORDS as u32;
        context.max_message_bytes = super::MAX_MESSAGE_BYTES as u32;
        context.catalog_chunk_records = gamenode_plugin_protocol::catalog::CHUNK_RECORDS as u32;
        Ok(Self {
            context,
            credentials,
            accounts: Mutex::new(Arc::new(LibraryAccounts(library))),
            identity: Arc::new(Mutex::new(None)),
            login,
            login_policy: login_policy.map(Arc::new),
            actions,
            persistence: Arc::new(Mutex::new(Persistence::default())),
            status: Mutex::new(None),
            observer: Mutex::new(None),
        })
    }

    pub fn status(&self) -> Option<models::AccountStatus> {
        lock(&self.status).clone()
    }

    pub fn set_observer(&self, observer: Arc<dyn Fn() + Send + Sync>) {
        *lock(&self.observer) = Some(observer);
    }
    pub fn set_account_persistence(&self, accounts: Arc<dyn AccountPersistence>) {
        *lock(&self.accounts) = accounts;
    }

    pub fn initial_status(&self) -> Option<models::AccountStatus> {
        let method: models::StorageMethod = SecretStorage::try_from(self.context.secret_storage)
            .ok()?
            .try_into()
            .ok()?;
        Some(models::AccountStatus {
            state: models::AccountState::NotConnected,
            account_id: None,
            account_name: None,
            last_checked_at: None,
            last_synced_at: None,
            storage: method,
            persist_failed: false,
            catalog_error: None,
            family_error: None,
        })
    }

    fn scope(&self, lease: &Lease) -> Result<(), Status> {
        lease.check()?;
        if lease.context.provider_id != self.context.provider_id {
            return Err(Status::permission_denied("provider service scope denied"));
        }
        Ok(())
    }
}

#[tonic::async_trait]
impl HostServices for ProviderServices {
    async fn context(&self, lease: Lease) -> Result<ProviderContext, Status> {
        self.scope(&lease)?;
        lease.require(gamenode_plugin_protocol::capability::CONTEXT)?;
        Ok(self.context.clone())
    }

    async fn account(
        &self,
        lease: Lease,
        action: account_request::Action,
    ) -> Result<AccountReply, Status> {
        self.scope(&lease)?;
        lease.require(if matches!(action, account_request::Action::Read(true)) {
            gamenode_plugin_protocol::capability::ACCOUNT_READ
        } else {
            gamenode_plugin_protocol::capability::ACCOUNT_PUBLISH
        })?;
        let provider = self.context.provider_id.clone();
        if let account_request::Action::Save(account) = &action {
            let validated: models::StoreAccount = account
                .clone()
                .try_into()
                .map_err(|_| Status::invalid_argument("invalid account identity"))?;
            if validated.provider != provider {
                return Err(Status::permission_denied("account scope denied"));
            }
        }
        let accounts = lock(&self.accounts).clone();
        let identity = self.identity.clone();
        let authority = lease.clone();
        let persistence = self.persistence.clone();
        let read = matches!(action, account_request::Action::Read(true));
        lease
            .storage(move || {
                let mut reply = AccountReply {
                    account: None,
                    accepted: true,
                };
                let result = match action {
                    account_request::Action::Read(true) => {
                        accounts.load(&provider).map(|account| {
                            *lock(&identity) = account.as_ref().map(|a| a.account_id.clone());
                            reply.account = account.map(Into::into);
                        })
                    }
                    account_request::Action::Save(account) => {
                        // A plugin may save before it ever reads, so an empty
                        // cache cannot be treated as "no previous account":
                        // the persisted record is then the only trustworthy
                        // source for deciding whether identity changed.
                        let known = match lock(&identity).clone() {
                            Some(id) => Ok(Some(id)),
                            None => accounts
                                .load(&provider)
                                .map(|account| account.map(|account| account.account_id)),
                        };
                        match known {
                            Ok(known) => {
                                let changed =
                                    known.as_ref().is_some_and(|id| id != &account.account_id);
                                authority.accept_identity(changed)?;
                                *lock(&identity) = Some(account.account_id.clone());
                                accounts.save(
                                    account.try_into().map_err(|_| {
                                        Status::invalid_argument("invalid account identity")
                                    })?,
                                    changed,
                                )
                            }
                            Err(error) => Err(error),
                        }
                    }
                    account_request::Action::Clear(true) => {
                        let result = accounts.clear(&provider);
                        if result.is_ok() {
                            *lock(&identity) = None;
                        }
                        result
                    }
                    _ => return Err(Status::invalid_argument("invalid account action")),
                };
                lock(&persistence).account.record(read, result.is_err());
                result?;
                Ok(reply)
            })
            .await
    }

    async fn secret(&self, lease: Lease, request: SecretRequest) -> Result<SecretReply, Status> {
        self.scope(&lease)?;
        if request.key != "session" {
            return Err(Status::permission_denied("secret scope denied"));
        }
        let operation = SecretOperation::try_from(request.operation)
            .map_err(|_| Status::invalid_argument("invalid secret operation"))?;
        lease.require(match operation {
            SecretOperation::Read => gamenode_plugin_protocol::capability::SECRET_READ,
            SecretOperation::Write => gamenode_plugin_protocol::capability::SECRET_WRITE,
            SecretOperation::Delete => gamenode_plugin_protocol::capability::SECRET_DELETE,
            _ => return Err(Status::invalid_argument("invalid secret operation")),
        })?;
        let value = request
            .value
            .map(|value| {
                if value.is_empty() || value.len() > 32 * 1024 {
                    return Err(Status::invalid_argument("invalid secret value"));
                }
                let value = String::from_utf8(value)
                    .map_err(|_| Status::invalid_argument("invalid secret encoding"))?;
                SessionCredential::new(value)
                    .ok_or_else(|| Status::invalid_argument("invalid secret value"))
            })
            .transpose()?;
        if (operation == SecretOperation::Write) != value.is_some()
            || operation == SecretOperation::Unspecified
        {
            return Err(Status::invalid_argument("invalid secret operation"));
        }
        let store = self
            .credentials
            .clone()
            .ok_or_else(|| Status::unavailable("secret storage unavailable"))?;
        let persistence = self.persistence.clone();
        let authority = lease.clone();
        lease
            .storage(move || {
                use secret_reply::Outcome;
                let outcome = match operation {
                    SecretOperation::Read => store.load().map(|value| {
                        value.map_or(Outcome::Absent(true), |value| {
                            Outcome::Value(value.expose().as_bytes().to_vec())
                        })
                    }),
                    SecretOperation::Write => value
                        .as_ref()
                        .map_or(Err(CredentialError::Failed), |value| store.save(value))
                        .map(|()| Outcome::Accepted(true)),
                    SecretOperation::Delete => store.delete().map(|()| Outcome::Accepted(true)),
                    _ => unreachable!("validated secret operation"),
                };
                authority
                    .record_secret(operation, matches!(outcome, Ok(Outcome::Accepted(true))))?;
                lock(&persistence)
                    .secret
                    .record(operation == SecretOperation::Read, outcome.is_err());
                Ok(SecretReply {
                    storage: storage(store.method()) as i32,
                    outcome: Some(outcome.unwrap_or_else(|e| {
                        Outcome::Failed(match e {
                            CredentialError::Locked => ServiceFailure::Locked,
                            CredentialError::Failed => ServiceFailure::Failed,
                        } as i32)
                    })),
                })
            })
            .await
    }

    async fn login(&self, lease: Lease, request: LoginRequest) -> Result<LoginReply, Status> {
        self.scope(&lease)?;
        let broker = self
            .login
            .as_ref()
            .ok_or_else(|| Status::unavailable("login service unavailable"))?;
        let policy = self
            .login_policy
            .clone()
            .ok_or_else(|| Status::permission_denied("login policy not granted"))?;
        broker.request(lease, request, policy).await
    }

    async fn actions(
        &self,
        lease: Lease,
        actions: ActionDeclaration,
    ) -> Result<ActionReply, Status> {
        self.scope(&lease)?;
        lease.require(gamenode_plugin_protocol::capability::ACTIONS)?;
        if actions
            .play_uri
            .as_ref()
            .is_some_and(|uri| Some(uri) != self.actions.play_uri.as_ref())
            || actions
                .install_uri
                .as_ref()
                .is_some_and(|uri| Some(uri) != self.actions.install_uri.as_ref())
        {
            return Err(Status::permission_denied("provider action scope denied"));
        }
        Ok(ActionReply { accepted: true })
    }

    fn publish_status(&self, mut status: models::AccountStatus) {
        // Never trust a plugin's assertion that a failed OS write was durable.
        if let Some(store) = &self.credentials {
            status.storage = match store.method() {
                StorageMethod::WindowsCredentialManager => {
                    models::StorageMethod::WindowsCredentialManager
                }
                StorageMethod::SecretService => models::StorageMethod::SecretService,
                StorageMethod::PrivateFile => models::StorageMethod::PrivateFile,
            };
        }
        let persistence = lock(&self.persistence);
        status.persist_failed |= persistence.secret.any() || persistence.account.any();
        *lock(&self.status) = Some(status);
        drop(persistence);
        let observer = lock(&self.observer).clone();
        if let Some(observer) = observer {
            observer();
        }
    }

    async fn session_closed(&self, session_id: &str) {
        if let Some(broker) = &self.login {
            broker.session_closed(session_id).await;
        }
    }

    fn login_reads_cookies(&self) -> bool {
        self.login_policy
            .as_ref()
            .is_some_and(|policy| !policy.cookie_names.is_empty())
    }
}

#[cfg(test)]
mod tests;
