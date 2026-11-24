//! Scoped adapters between the public provider API and host-owned persistence.
//! A registration captures its stores; callers cannot choose an OS credential
//! key, database path, another provider or the GameNode authentication store.

use super::{
    login::LoginBroker,
    namespace::{NamespaceOwner, SecretSlot},
    provider::{HostServices, Lease},
};
use gamenode_core::{
    SessionCredential, StorageMethod,
    library::LibraryStore,
    ports::{CredentialError, CredentialStore},
};
use gamenode_plugin_protocol::{login::LoginPolicy, models, proto::*};
use serde::Deserialize;
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
    external_credentials: Option<Arc<dyn ExternalCredentialStoreFactory>>,
    accounts: Mutex<Arc<dyn AccountPersistence>>,
    external_accounts: Option<Arc<dyn ExternalAccountPersistence>>,
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

/// Blocking account persistence for a third-party provider. The owner is derived from the
/// authenticated plugin, publisher and profile on every request; implementations must use it as
/// part of their storage key and must not route these records through Core's official accounts.
pub trait ExternalAccountPersistence: Send + Sync {
    fn load(
        &self,
        owner: &NamespaceOwner,
        provider: &str,
    ) -> Result<Option<models::StoreAccount>, Status>;
    fn save(
        &self,
        owner: &NamespaceOwner,
        account: models::StoreAccount,
        clear_ownership: bool,
    ) -> Result<(), Status>;
    fn clear(&self, owner: &NamespaceOwner, provider: &str) -> Result<(), Status>;
}

/// Host-owned adapter that opens one already-resolved third-party credential slot. The service
/// invokes it only inside the blocking storage worker; plugin requests never choose a slot.
pub trait ExternalCredentialStoreFactory: Send + Sync {
    fn open(
        &self,
        slot: gamenode_platform::credentials::CredentialSlot,
    ) -> Arc<dyn CredentialStore>;
}

struct PlatformExternalCredentialStoreFactory(gamenode_platform::paths::DataPaths);

impl ExternalCredentialStoreFactory for PlatformExternalCredentialStoreFactory {
    fn open(
        &self,
        slot: gamenode_platform::credentials::CredentialSlot,
    ) -> Arc<dyn CredentialStore> {
        gamenode_platform::credentials::open_store(&self.0, slot)
    }
}

/// Signed third-party login metadata. It deliberately names a host probe instead of carrying
/// JavaScript that would be evaluated inside the privileged login WebView.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalLoginPolicy {
    pub start_url: String,
    #[serde(default)]
    pub cookie_urls: Vec<String>,
    pub probe_origin: String,
    pub title: String,
    pub allowed_hosts: Vec<String>,
    #[serde(default)]
    pub cookie_names: Vec<String>,
    pub probe_id: ExternalPageProbe,
    pub width: u32,
    pub height: u32,
    pub timeout_seconds: u64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub enum ExternalPageProbe {
    #[serde(rename = "oauth_authorization_code")]
    OAuthAuthorizationCode,
}

impl ExternalLoginPolicy {
    fn into_host_policy(self) -> Result<LoginPolicy, Status> {
        let probe_script = match self.probe_id {
            ExternalPageProbe::OAuthAuthorizationCode => {
                "(() => { const code = new URL(window.location.href).searchParams.get('code'); return code ? { authorization_code: code } : null; })()"
            }
        };
        let policy = LoginPolicy {
            start_url: self.start_url,
            cookie_urls: self.cookie_urls,
            probe_origin: self.probe_origin,
            title: self.title,
            allowed_hosts: self.allowed_hosts,
            cookie_names: self.cookie_names,
            probe_script: probe_script.into(),
            width: self.width,
            height: self.height,
            timeout_seconds: self.timeout_seconds,
        };
        policy
            .validate()
            .map_err(|_| Status::failed_precondition("invalid external login policy"))?;
        Ok(policy)
    }
}

fn external_credential_slot(slot: &SecretSlot) -> gamenode_platform::credentials::CredentialSlot {
    use gamenode_platform::credentials::{CredentialAlias, CredentialSlot};

    let suffix = slot
        .key()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    CredentialSlot::provider(
        "third-party",
        Some(CredentialAlias {
            keyring_account: format!("third-party.{suffix}"),
            private_file: format!("third-party-{suffix}.secret"),
        }),
    )
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
            external_credentials: None,
            accounts: Mutex::new(Arc::new(LibraryAccounts(library))),
            external_accounts: None,
            identity: Arc::new(Mutex::new(None)),
            login,
            login_policy: login_policy.map(Arc::new),
            actions,
            persistence: Arc::new(Mutex::new(Persistence::default())),
            status: Mutex::new(None),
            observer: Mutex::new(None),
        })
    }

    /// Compose services for a managed third-party provider. Its account adapter is separate from
    /// Core's official account table, and each secret is opened from a host-derived namespace
    /// slot inside the blocking storage worker. The login descriptor cannot supply probe code.
    pub fn new_external(
        context: ProviderContext,
        paths: gamenode_platform::paths::DataPaths,
        library: Arc<dyn LibraryStore>,
        accounts: Arc<dyn ExternalAccountPersistence>,
        login: Option<Arc<LoginBroker>>,
        login_policy: Option<ExternalLoginPolicy>,
    ) -> Result<Self, Status> {
        Self::new_external_with_credentials(
            context,
            Arc::new(PlatformExternalCredentialStoreFactory(paths)),
            library,
            accounts,
            login,
            login_policy,
        )
    }

    /// Variant for a host-owned credential backend, also used by deterministic service fixtures.
    pub fn new_external_with_credentials(
        context: ProviderContext,
        credentials: Arc<dyn ExternalCredentialStoreFactory>,
        library: Arc<dyn LibraryStore>,
        accounts: Arc<dyn ExternalAccountPersistence>,
        login: Option<Arc<LoginBroker>>,
        login_policy: Option<ExternalLoginPolicy>,
    ) -> Result<Self, Status> {
        let login_policy = login_policy
            .map(ExternalLoginPolicy::into_host_policy)
            .transpose()?;
        let mut services = Self::new(context, None, library, login, login_policy)?;
        services.external_credentials = Some(credentials);
        services.external_accounts = Some(accounts);
        Ok(services)
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
            shared_error: None,
        })
    }

    fn scope(&self, lease: &Lease) -> Result<(), Status> {
        lease.check()?;
        if lease.context.provider_id != self.context.provider_id {
            return Err(lease.deny_scope());
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
                return Err(
                    lease.deny_scope_for(gamenode_plugin_protocol::capability::ACCOUNT_PUBLISH)
                );
            }
        }
        let external_accounts = self.external_accounts.clone();
        let accounts = external_accounts
            .is_none()
            .then(|| lock(&self.accounts).clone());
        let external_owner = if external_accounts.is_some() {
            Some(
                lease
                    .namespace()?
                    .account_owner()
                    .map_err(|_| Status::failed_precondition("account namespace unavailable"))?,
            )
        } else {
            None
        };
        let identity = self.identity.clone();
        let authority = lease.clone();
        let persistence = self.persistence.clone();
        let read = matches!(action, account_request::Action::Read(true));
        lease
            .storage(move || {
                let load = |provider: &str| match (&external_accounts, &external_owner, &accounts) {
                    (Some(store), Some(owner), _) => store.load(owner, provider),
                    (None, None, Some(store)) => store.load(provider),
                    _ => Err(Status::unavailable("account storage unavailable")),
                };
                let save = |account, clear_ownership| match (
                    &external_accounts,
                    &external_owner,
                    &accounts,
                ) {
                    (Some(store), Some(owner), _) => store.save(owner, account, clear_ownership),
                    (None, None, Some(store)) => store.save(account, clear_ownership),
                    _ => Err(Status::unavailable("account storage unavailable")),
                };
                let clear = |provider: &str| match (&external_accounts, &external_owner, &accounts)
                {
                    (Some(store), Some(owner), _) => store.clear(owner, provider),
                    (None, None, Some(store)) => store.clear(provider),
                    _ => Err(Status::unavailable("account storage unavailable")),
                };
                let mut reply = AccountReply {
                    account: None,
                    accepted: true,
                };
                let result = match action {
                    account_request::Action::Read(true) => load(&provider).map(|account| {
                        *lock(&identity) = account.as_ref().map(|a| a.account_id.clone());
                        reply.account = account.map(Into::into);
                    }),
                    account_request::Action::Save(account) => {
                        // A plugin may save before it ever reads, so an empty
                        // cache cannot be treated as "no previous account":
                        // the persisted record is then the only trustworthy
                        // source for deciding whether identity changed.
                        let known = match lock(&identity).clone() {
                            Some(id) => Ok(Some(id)),
                            None => load(&provider)
                                .map(|account| account.map(|account| account.account_id)),
                        };
                        match known {
                            Ok(known) => {
                                let changed =
                                    known.as_ref().is_some_and(|id| id != &account.account_id);
                                authority.accept_identity(changed)?;
                                *lock(&identity) = Some(account.account_id.clone());
                                save(
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
                        let result = clear(&provider);
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
        let operation = SecretOperation::try_from(request.operation)
            .map_err(|_| Status::invalid_argument("invalid secret operation"))?;
        let permission = match operation {
            SecretOperation::Read => gamenode_plugin_protocol::capability::SECRET_READ,
            SecretOperation::Write => gamenode_plugin_protocol::capability::SECRET_WRITE,
            SecretOperation::Delete => gamenode_plugin_protocol::capability::SECRET_DELETE,
            _ => return Err(Status::invalid_argument("invalid secret operation")),
        };
        lease.require(permission)?;
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
        let store = self.credentials.clone();
        let external_factory = self.external_credentials.clone();
        let external_slot = if external_factory.is_some() {
            Some(
                lease
                    .namespace()?
                    .secret_slot(&request.key)
                    .map_err(|_| Status::invalid_argument("invalid logical secret key"))?,
            )
        } else {
            if request.key != "session" {
                return Err(lease.deny_scope_for(permission));
            }
            None
        };
        if store.is_none() && external_factory.is_none() {
            return Err(Status::unavailable("secret storage unavailable"));
        }
        let persistence = self.persistence.clone();
        let authority = lease.clone();
        lease
            .storage(move || {
                let store = match (store, external_factory, external_slot) {
                    (Some(store), None, None) => store,
                    (None, Some(factory), Some(slot)) => {
                        factory.open(external_credential_slot(&slot))
                    }
                    _ => return Err(Status::unavailable("secret storage unavailable")),
                };
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
        if self.external_credentials.is_some() {
            // Do not allow a trusted descriptor service to be attached to an official or
            // unauthenticated registration by accident.
            lease.namespace()?;
        }
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
            return Err(lease.deny_scope());
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
