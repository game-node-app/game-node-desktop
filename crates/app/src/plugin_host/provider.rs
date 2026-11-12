//! Full-provider sessions and operation routing. OS/Core services are injected;
//! every provider message is validated before those services can be called.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gamenode_plugin_protocol::{
    capability as cap, catalog::CatalogTransfer, models, proto::*, settings,
};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

const MAX_OPERATIONS: usize = 32;
const COMMAND_QUEUE: usize = 32;
const HISTORY: usize = 128;
const AUTHORIZATION_AUDIT_LIMIT: usize = 256;

/// Redacted host-side record of a denied authorization check. It deliberately
/// excludes session credentials, operation ids, and request payloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationAuditEvent {
    pub plugin_id: String,
    pub provider_id: String,
    pub permission_id: String,
    pub reason: DenialReason,
    pub grant_revision: u64,
}

fn denial_event(
    session: &Session,
    permission: &str,
    reason: DenialReason,
) -> AuthorizationAuditEvent {
    AuthorizationAuditEvent {
        plugin_id: session.provider.registration.plugin_id.clone(),
        provider_id: session.provider.registration.provider_id.clone(),
        permission_id: permission.to_owned(),
        reason,
        grant_revision: session.grant_revision,
    }
}

fn denial(reason: DenialReason, permission: &str, revision: u64) -> Status {
    use gamenode_plugin_protocol::prost::Message;
    let details = DenialDetail {
        reason: reason as i32,
        permission_id: permission.to_owned(),
        grant_revision: revision,
    }
    .encode_to_vec();
    Status::with_details(
        tonic::Code::PermissionDenied,
        "plugin permission denied",
        details.into(),
    )
}

fn permission_change(session: &Session, revoked: bool) -> PermissionChange {
    let revision = session.grant_revision;
    let mut grants = session
        .grants
        .iter()
        .map(|permission_id| PermissionGrant {
            permission_id: permission_id.clone(),
            state: "granted".into(),
            scope: "provider".into(),
            reason: String::new(),
            revision,
        })
        .collect::<Vec<_>>();
    for permission_id in cap::KNOWN {
        if !session.grants.contains(*permission_id) {
            grants.push(PermissionGrant {
                permission_id: (*permission_id).into(),
                state: "denied".into(),
                scope: "provider".into(),
                reason: "not_granted".into(),
                revision,
            });
        }
    }
    grants.sort_by(|a, b| a.permission_id.cmp(&b.permission_id));
    PermissionChange {
        revision,
        grants,
        revoked,
    }
}

/// Implementations provide only safe diagnostics. Blocking work belongs on
/// workers and mutations must check the supplied operation lease at commit.
#[tonic::async_trait]
pub trait HostServices: Send + Sync {
    async fn context(&self, lease: Lease) -> Result<ProviderContext, Status>;
    async fn account(
        &self,
        lease: Lease,
        action: account_request::Action,
    ) -> Result<AccountReply, Status>;
    async fn secret(&self, lease: Lease, request: SecretRequest) -> Result<SecretReply, Status>;
    async fn login(&self, lease: Lease, request: LoginRequest) -> Result<LoginReply, Status>;
    async fn actions(
        &self,
        lease: Lease,
        actions: ActionDeclaration,
    ) -> Result<ActionReply, Status>;
    fn publish_status(&self, _status: models::AccountStatus) {}
    async fn session_closed(&self, _session_id: &str) {}
}

pub struct Registration {
    pub provider_id: String,
    pub plugin_id: String,
    pub capabilities: Vec<String>,
    pub services: Arc<dyn HostServices>,
}

struct Provider {
    registration: Registration,
    account_epoch: std::sync::atomic::AtomicU64,
    profile_epoch: std::sync::atomic::AtomicU64,
    identity_revision: std::sync::atomic::AtomicU64,
    commits: Arc<tokio::sync::Mutex<()>>,
}

struct Pending {
    provider: Arc<Provider>,
    credential: [u8; 32],
    ready: oneshot::Sender<()>,
}

struct Session {
    provider: Arc<Provider>,
    credential: [u8; 32],
    grants: BTreeSet<String>,
    kind: i32,
    target_game: Option<TargetGameContext>,
    grant_revision: u64,
    permission_watchers: Vec<mpsc::Sender<Result<PermissionChange, Status>>>,
    commands: Option<mpsc::Sender<Result<ProviderCommand, Status>>>,
    ready: Option<oneshot::Sender<()>>,
    completed: VecDeque<String>,
}

struct Operation {
    context: ServiceContext,
    kind: ProviderOperation,
    permissions: BTreeSet<&'static str>,
    catalog: CatalogTransfer,
    result: oneshot::Sender<Result<Completion, Status>>,
    identity_revision: u64,
    secret_written: bool,
    secret_deleted: bool,
    identity_accepted: bool,
    settings: Option<settings::Request>,
}

#[derive(Default)]
struct State {
    providers: HashMap<String, Arc<Provider>>,
    pending: Vec<Pending>,
    sessions: HashMap<String, Session>,
    operations: HashMap<(String, String), Operation>,
    authorization_audit: VecDeque<AuthorizationAuditEvent>,
}

#[derive(Clone, Default)]
pub struct ProviderApi {
    state: Arc<Mutex<State>>,
}

pub struct Launch {
    pub credential: String,
    pub ready: oneshot::Receiver<()>,
}

#[derive(Debug)]
pub struct Completion {
    pub result: OperationResult,
    pub catalog: models::CatalogUpdate,
    guard: Option<ResultGuard>,
}

#[derive(Clone)]
struct ResultGuard {
    api: ProviderApi,
    context: ServiceContext,
    account_scoped: bool,
    identity_revision: u64,
    permissions: BTreeSet<&'static str>,
}

impl std::fmt::Debug for ResultGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ResultGuard(<scoped>)")
    }
}

impl ResultGuard {
    fn provider(&self) -> Result<Arc<Provider>, Status> {
        let state = lock(&self.api.state);
        let session = state
            .sessions
            .get(&self.context.session_id)
            .ok_or_else(|| Status::failed_precondition("provider result expired"))?;
        let current = epoch(&session.provider);
        if !super::constant_time_eq(&session.credential, &self.context.session_credential)
            || self
                .permissions
                .iter()
                .any(|permission| !session.grants.contains(*permission))
            || session.provider.registration.provider_id != self.context.provider_id
            || current.1 != self.context.profile_epoch
            || (self.account_scoped
                && (current.0 != self.context.account_epoch
                    || session
                        .provider
                        .identity_revision
                        .load(std::sync::atomic::Ordering::Acquire)
                        != self.identity_revision))
        {
            return Err(Status::failed_precondition("provider result superseded"));
        }
        Ok(session.provider.clone())
    }
}

impl Completion {
    /// Domain application is fenced even after the terminal event removed the
    /// operation. Queue Core persistence before releasing the provider guard.
    pub async fn commit<T: Send + 'static>(
        self,
        work: impl FnOnce(OperationResult, models::CatalogUpdate) -> T + Send + 'static,
    ) -> Result<T, Status> {
        let guard = if let Some(guard) = &self.guard {
            Some(guard.provider()?.commits.clone().lock_owned().await)
        } else {
            None
        };
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            if let Some(guard) = &self.guard {
                guard.provider()?;
            }
            Ok(work(self.result, self.catalog))
        })
        .await
        .map_err(|_| Status::unavailable("provider result unavailable"))?
    }

    #[cfg(test)]
    pub(crate) fn fixture(result: OperationResult, catalog: models::CatalogUpdate) -> Self {
        Self {
            result,
            catalog,
            guard: None,
        }
    }
}

/// A service request retains the immutable host-issued authorization context.
/// Checking a lease after I/O cannot authorize a different operation or epoch.
#[derive(Clone)]
pub struct Lease {
    api: ProviderApi,
    pub context: ServiceContext,
    permission: &'static str,
}

impl Lease {
    pub fn check(&self) -> Result<(), Status> {
        self.api
            .authorize(&self.context, self.permission)
            .map(|_| ())
    }

    pub fn require(&self, permission: &'static str) -> Result<(), Status> {
        self.api.authorize(&self.context, permission).map(|_| ())
    }

    /// Called by trusted service adapters inside `storage`, while the provider
    /// commit guard is held. Failed OS writes never establish a commit point.
    pub fn record_secret(&self, operation: SecretOperation, accepted: bool) -> Result<(), Status> {
        self.check()?;
        let mut state = lock(&self.api.state);
        let current = state
            .operations
            .get_mut(&(
                self.context.session_id.clone(),
                self.context.operation_id.clone(),
            ))
            .ok_or_else(|| Status::failed_precondition("stale secret operation"))?;
        if accepted {
            if operation == SecretOperation::Write {
                current.secret_written = true;
                current.secret_deleted = false;
            }
            if operation == SecretOperation::Delete {
                current.secret_deleted = true;
            }
        }
        Ok(())
    }

    pub fn accept_identity(&self, changed: bool) -> Result<(), Status> {
        let provider = self.api.authorize(&self.context, self.permission)?;
        let mut state = lock(&self.api.state);
        let revision = if changed {
            provider
                .identity_revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                + 1
        } else {
            provider
                .identity_revision
                .load(std::sync::atomic::Ordering::Acquire)
        };
        let current = state
            .operations
            .get_mut(&(
                self.context.session_id.clone(),
                self.context.operation_id.clone(),
            ))
            .ok_or_else(|| Status::failed_precondition("stale account operation"))?;
        current.identity_revision = revision;
        current.identity_accepted = true;
        if changed {
            let obsolete: Vec<_> = state
                .operations
                .iter()
                .filter(|((session, operation), entry)| {
                    entry.context.provider_id == self.context.provider_id
                        && (session != &self.context.session_id
                            || operation != &self.context.operation_id)
                        && account_scoped(entry.kind)
                })
                .map(|(key, _)| key.clone())
                .collect();
            for key in obsolete {
                if let Some(entry) = state.operations.remove(&key) {
                    let _ = entry.result.send(Err(Status::failed_precondition(
                        "provider identity changed",
                    )));
                }
            }
        }
        Ok(())
    }

    /// Serializes blocking storage with epoch invalidation. The worker owns the
    /// guard, so cancelling an RPC cannot release the queue while OS I/O is
    /// still running. Queued work is reauthorized immediately before execution.
    pub async fn storage<T: Send + 'static>(
        self,
        work: impl FnOnce() -> Result<T, Status> + Send + 'static,
    ) -> Result<T, Status> {
        let provider = self.api.authorize(&self.context, self.permission)?;
        let guard = provider.commits.clone().lock_owned().await;
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            self.check()?;
            work()
        })
        .await
        .map_err(|_| Status::unavailable("provider storage unavailable"))?
    }
}

// Steam sync performs session validation, owned games, then Family games,
// each bounded at 90 seconds. Leave bounded headroom for host storage/RPCs.
fn operation_timeout(kind: ProviderOperation) -> Duration {
    match kind {
        ProviderOperation::SignIn => Duration::from_secs(615),
        ProviderOperation::SyncCatalog => Duration::from_secs(300),
        ProviderOperation::Initialize | ProviderOperation::Check => Duration::from_secs(105),
        ProviderOperation::SettingsCancel => Duration::from_secs(3),
        _ => Duration::from_secs(90),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn epoch(provider: &Provider) -> (u64, u64) {
    use std::sync::atomic::Ordering;
    (
        provider.account_epoch.load(Ordering::Acquire),
        provider.profile_epoch.load(Ordering::Acquire),
    )
}

fn is_settings(kind: ProviderOperation) -> bool {
    matches!(
        kind,
        ProviderOperation::SettingsDescribeRead
            | ProviderOperation::SettingsSubmit
            | ProviderOperation::SettingsCancel
    )
}

fn is_configuration(kind: ProviderOperation) -> bool {
    matches!(
        kind,
        ProviderOperation::ConfigurationOpen
            | ProviderOperation::ConfigurationFocus
            | ProviderOperation::ConfigurationClose
    )
}

fn account_scoped(kind: ProviderOperation) -> bool {
    !is_settings(kind)
        && !is_configuration(kind)
        && !matches!(
            kind,
            ProviderOperation::Discover | ProviderOperation::ReadCounter
        )
}

fn permission_set(kind: ProviderOperation) -> BTreeSet<&'static str> {
    use ProviderOperation::*;
    let permissions: &[&str] = match kind {
        Initialize => &[
            cap::CONTEXT,
            cap::ACCOUNT_READ,
            cap::ACCOUNT_PUBLISH,
            cap::SECRET_READ,
            cap::ACTIONS,
        ],
        Discover => &[cap::CONTEXT, cap::DISCOVERY],
        SettingsDescribeRead | SettingsSubmit | SettingsCancel => &[cap::SETTINGS],
        ConfigurationOpen | ConfigurationFocus | ConfigurationClose => &[cap::CONFIGURE],
        SignIn => &[
            cap::CONTEXT,
            cap::ACCOUNT_READ,
            cap::ACCOUNT_PUBLISH,
            cap::SECRET_READ,
            cap::SECRET_WRITE,
            cap::SECRET_DELETE,
            cap::LOGIN,
            cap::COOKIES,
            cap::PAGE,
        ],
        Check => &[
            cap::CONTEXT,
            cap::ACCOUNT_READ,
            cap::ACCOUNT_PUBLISH,
            cap::SECRET_READ,
            cap::SECRET_WRITE,
            cap::SECRET_DELETE,
        ],
        SyncCatalog => &[
            cap::CONTEXT,
            cap::ACCOUNT_READ,
            cap::ACCOUNT_PUBLISH,
            cap::SECRET_READ,
            cap::SECRET_WRITE,
            cap::SECRET_DELETE,
            cap::CATALOG,
        ],
        SignOut => &[
            cap::CONTEXT,
            cap::ACCOUNT_READ,
            cap::ACCOUNT_PUBLISH,
            cap::SECRET_READ,
            cap::SECRET_DELETE,
            cap::LOGIN,
        ],
        FocusSignIn | CancelSignIn | Cancel => &[cap::CONTEXT, cap::LOGIN, cap::ACCOUNT_PUBLISH],
        ReadCounter => &[cap::CONTEXT, cap::COUNTER],
        _ => &[],
    };
    permissions.iter().copied().collect()
}

impl ProviderApi {
    fn record_authorization_denial(&self, event: AuthorizationAuditEvent) {
        let mut state = lock(&self.state);
        if state.authorization_audit.len() == AUTHORIZATION_AUDIT_LIMIT {
            state.authorization_audit.pop_front();
        }
        state.authorization_audit.push_back(event);
    }

    /// Returns the bounded, redacted authorization-denial history for host UI
    /// and diagnostics. This is never exposed through the plugin RPC surface.
    pub fn authorization_audit(&self) -> Vec<AuthorizationAuditEvent> {
        lock(&self.state).authorization_audit.iter().cloned().collect()
    }

    /// Authenticates the permission-control plane without requiring any data grant.
    pub fn effective_permissions(
        &self,
        session_id: &str,
        credential: &[u8],
    ) -> Result<(u64, Vec<PermissionGrant>), Status> {
        let state = lock(&self.state);
        let session = state
            .sessions
            .get(session_id)
            .ok_or_else(|| Status::unauthenticated("unknown plugin session"))?;
        if !super::constant_time_eq(&session.credential, credential) {
            return Err(Status::unauthenticated("invalid plugin session credential"));
        }
        let change = permission_change(session, false);
        Ok((change.revision, change.grants))
    }

    pub fn watch_permissions(
        &self,
        session_id: &str,
        credential: &[u8],
        after_revision: u64,
    ) -> Result<ReceiverStream<Result<PermissionChange, Status>>, Status> {
        let mut state = lock(&self.state);
        let session = state
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| Status::unauthenticated("unknown plugin session"))?;
        if !super::constant_time_eq(&session.credential, credential) {
            return Err(Status::unauthenticated("invalid plugin session credential"));
        }
        let (tx, rx) = mpsc::channel(16);
        if session.grant_revision > after_revision {
            let change = permission_change(session, false);
            let _ = tx.try_send(Ok(change));
        }
        session.permission_watchers.push(tx);
        Ok(ReceiverStream::new(rx))
    }

    /// Host-only grant update entry point. Persist the grant change before
    /// calling this method; consumers then receive the new revision.
    pub fn replace_session_grants(
        &self,
        session_id: &str,
        grants: BTreeSet<String>,
    ) -> Result<u64, Status> {
        if grants
            .iter()
            .any(|grant| !cap::KNOWN.contains(&grant.as_str()))
        {
            return Err(Status::invalid_argument("unknown plugin permission"));
        }
        let mut state = lock(&self.state);
        let session = state
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| Status::unauthenticated("unknown plugin session"))?;
        let revoked = !session.grants.is_subset(&grants);
        session.grants = grants;
        session.grant_revision = session.grant_revision.saturating_add(1);
        let change = permission_change(session, revoked);
        session
            .permission_watchers
            .retain(|watcher| watcher.try_send(Ok(change.clone())).is_ok());
        if revoked {
            // Existing operations are fenced by immediately disconnecting the
            // affected session; queued work can no longer commit.
            session.commands.take();
        }
        Ok(session.grant_revision)
    }

    pub fn registered(&self, provider_id: &str) -> bool {
        lock(&self.state).providers.contains_key(provider_id)
    }

    pub fn request_shutdown(&self, provider_id: &str) {
        let state = lock(&self.state);
        for (id, session) in &state.sessions {
            if session.provider.registration.provider_id != provider_id {
                continue;
            }
            if let Some(commands) = &session.commands {
                let (account_epoch, profile_epoch) = epoch(&session.provider);
                if let Ok(operation_id) = super::random_session_id() {
                    let _ = commands.try_send(Ok(ProviderCommand {
                        context: Some(ServiceContext {
                            session_id: id.clone(),
                            session_credential: session.credential.to_vec(),
                            provider_id: provider_id.into(),
                            operation_id,
                            account_epoch,
                            profile_epoch,
                            session_kind: session.kind,
                            target_game: session.target_game.clone(),
                        }),
                        operation: ProviderOperation::Shutdown as i32,
                        target_operation_id: String::new(),
                        settings: None,
                        runtime_session_result: None,
                    }));
                }
            }
        }
    }

    /// Registration is host-owned composition, never a plugin RPC.
    pub fn register(&self, registration: Registration) -> Result<(), Status> {
        if registration.provider_id.is_empty()
            || registration.provider_id.len() > 128
            || registration.plugin_id.is_empty()
            || registration.plugin_id.len() > 256
        {
            return Err(Status::invalid_argument("invalid provider registration"));
        }
        cap::negotiate(
            gamenode_plugin_protocol::API_MAJOR,
            gamenode_plugin_protocol::API_MINOR,
            &registration.capabilities,
            cap::KNOWN,
        )
        .map_err(|_| Status::invalid_argument("invalid provider grants"))?;
        if !registration
            .capabilities
            .iter()
            .any(|s| s == cap::CONTEXT || s == cap::SETTINGS || s == cap::CONFIGURE)
        {
            return Err(Status::invalid_argument(
                "full provider requires context access",
            ));
        }
        let mut state = lock(&self.state);
        if state.providers.contains_key(&registration.provider_id)
            || state
                .providers
                .values()
                .any(|p| p.registration.plugin_id == registration.plugin_id)
        {
            return Err(Status::already_exists("provider is already registered"));
        }
        state.providers.insert(
            registration.provider_id.clone(),
            Arc::new(Provider {
                registration,
                account_epoch: 1.into(),
                profile_epoch: 1.into(),
                identity_revision: 1.into(),
                commits: Arc::new(tokio::sync::Mutex::new(())),
            }),
        );
        Ok(())
    }

    pub fn begin_launch(&self, provider_id: &str) -> Result<Launch, Status> {
        let mut state = lock(&self.state);
        let provider = state
            .providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| Status::not_found("provider is unavailable"))?;
        if state.pending.len() + state.sessions.len() >= 16 {
            return Err(Status::resource_exhausted("provider session capacity"));
        }
        if state
            .pending
            .iter()
            .any(|p| p.provider.registration.provider_id == provider_id)
            || state
                .sessions
                .values()
                .any(|s| s.provider.registration.provider_id == provider_id)
        {
            return Err(Status::already_exists(
                "provider already has a process session",
            ));
        }
        let mut credential = [0; 32];
        getrandom::fill(&mut credential)
            .map_err(|_| Status::internal("could not create launch credential"))?;
        let (ready, receiver) = oneshot::channel();
        state.pending.push(Pending {
            provider,
            credential,
            ready,
        });
        Ok(Launch {
            credential: super::hex(&credential),
            ready: receiver,
        })
    }

    pub fn owns_launch(&self, credential: &[u8]) -> bool {
        lock(&self.state)
            .pending
            .iter()
            .any(|p| super::constant_time_eq(&p.credential, credential))
    }

    pub fn open_session(&self, hello: &OpenSessionRequest) -> Result<OpenSessionReply, Status> {
        let mut state = lock(&self.state);
        let index = state
            .pending
            .iter()
            .position(|p| super::constant_time_eq(&p.credential, &hello.session_credential))
            .ok_or_else(|| Status::unauthenticated("unknown provider launch"))?;
        let pending = &state.pending[index];
        if pending.provider.registration.plugin_id != hello.plugin_id
            || hello.plugin_version.is_empty()
            || hello.plugin_version.len() > 128
        {
            return Err(Status::permission_denied("wrong provider identity"));
        }
        let allowed: Vec<&str> = pending
            .provider
            .registration
            .capabilities
            .iter()
            .map(String::as_str)
            .collect();
        cap::validate_session_scope(
            u32::try_from(hello.session_kind).unwrap_or(u32::MAX),
            hello.target_game.is_some(),
        )
        .map_err(|_| Status::invalid_argument("invalid session scope"))?;
        if let Some(target) = hello.target_game.as_ref() {
            cap::validate_target(target)
                .map_err(|_| Status::invalid_argument("invalid target game context"))?;
        }
        let selection = cap::negotiate_range(
            hello.api_major,
            hello.api_minor,
            hello.min_api_major,
            hello.min_api_minor,
            hello.max_api_major,
            hello.max_api_minor,
            &hello
                .features
                .iter()
                .map(|f| (f.id.clone(), f.required))
                .collect::<Vec<_>>(),
        )
        .map_err(|error| match error {
            cap::RangeNegotiationError::RequiredFeature => {
                Status::failed_precondition(error.to_string())
            }
            cap::RangeNegotiationError::Session => Status::invalid_argument(error.to_string()),
            _ => Status::failed_precondition(error.to_string()),
        })?;
        let minor = cap::negotiate(
            selection.api_major,
            selection.api_minor,
            &hello.requested_capabilities,
            &allowed,
        )
        .map_err(|error| match error {
            cap::NegotiationError::Capability => Status::permission_denied(error.to_string()),
            _ => Status::failed_precondition(error.to_string()),
        })?;
        let requested: BTreeSet<String> = hello.requested_capabilities.iter().cloned().collect();
        if requested
            != pending
                .provider
                .registration
                .capabilities
                .iter()
                .cloned()
                .collect()
        {
            return Err(Status::failed_precondition(
                "required provider capabilities are missing",
            ));
        }
        let pending = state.pending.remove(index);
        let session_id = super::random_session_id()?;
        state.sessions.insert(
            session_id.clone(),
            Session {
                provider: pending.provider,
                credential: pending.credential,
                grants: requested.clone(),
                kind: hello.session_kind,
                target_game: hello.target_game.clone(),
                grant_revision: 1,
                permission_watchers: Vec::new(),
                commands: None,
                ready: Some(pending.ready),
                completed: VecDeque::new(),
            },
        );
        Ok(OpenSessionReply {
            session_id,
            granted_capabilities: requested.into_iter().collect(),
            api_major: gamenode_plugin_protocol::API_MAJOR,
            api_minor: minor,
            features: selection
                .features
                .into_iter()
                .map(|(id, supported, introduced_minor)| FeatureStatus {
                    id,
                    availability: if supported {
                        FeatureAvailability::Supported as i32
                    } else {
                        FeatureAvailability::Unsupported as i32
                    },
                    introduced_minor,
                })
                .collect(),
            support_window: Some(SupportWindow {
                minimum_api_minor: gamenode_plugin_protocol::MIN_API_MINOR,
                maximum_api_minor: gamenode_plugin_protocol::API_MINOR,
                support_ends_at: None,
                policy: "supported by this GameNode build".into(),
            }),
        })
    }

    pub fn connected(&self, provider_id: &str) -> bool {
        lock(&self.state)
            .sessions
            .values()
            .any(|s| s.provider.registration.provider_id == provider_id && s.commands.is_some())
    }

    pub async fn disconnect(&self, provider_id: &str) {
        let ids = {
            let mut state = lock(&self.state);
            state
                .pending
                .retain(|p| p.provider.registration.provider_id != provider_id);
            state
                .sessions
                .iter()
                .filter(|(_, session)| session.provider.registration.provider_id == provider_id)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        for id in ids {
            self.disconnect_session(&id).await;
        }
    }

    /// Called by the profile owner before replacing profile-backed adapters.
    /// Existing sessions cannot acquire authority over the next profile.
    pub async fn invalidate_profile(&self) {
        let providers: Vec<_> = lock(&self.state).providers.values().cloned().collect();
        for provider in providers {
            let guard = provider.commits.lock().await;
            provider
                .profile_epoch
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            drop(guard);
            self.disconnect(&provider.registration.provider_id).await;
        }
    }

    async fn disconnect_session(&self, session_id: &str) {
        let provider = lock(&self.state)
            .sessions
            .get(session_id)
            .map(|s| s.provider.clone());
        let Some(provider) = provider else {
            return;
        };
        let guard = provider.commits.lock().await;
        let services = {
            let mut state = lock(&self.state);
            let Some(session) = state.sessions.remove(session_id) else {
                return;
            };
            let keys: Vec<_> = state
                .operations
                .keys()
                .filter(|(id, _)| id == session_id)
                .cloned()
                .collect();
            for key in keys {
                if let Some(operation) = state.operations.remove(&key) {
                    let _ = operation
                        .result
                        .send(Err(Status::unavailable("provider process disconnected")));
                }
            }
            session.provider.registration.services.clone()
        };
        drop(guard);
        services.session_closed(session_id).await;
    }

    fn authorize(
        &self,
        context: &ServiceContext,
        permission: &'static str,
    ) -> Result<Arc<Provider>, Status> {
        let state = lock(&self.state);
        let Some(session) = state
            .sessions
            .get(&context.session_id)
        else {
            return Err(Status::unauthenticated("provider session expired"));
        };
        if !super::constant_time_eq(&session.credential, &context.session_credential) {
            let event = denial_event(session, permission, DenialReason::ScopeDenied);
            let revision = event.grant_revision;
            drop(state);
            self.record_authorization_denial(event);
            return Err(denial(
                DenialReason::ScopeDenied,
                permission,
                revision,
            ));
        }
        if context.session_kind != session.kind || context.target_game != session.target_game {
            let event = denial_event(session, permission, DenialReason::ScopeDenied);
            let revision = event.grant_revision;
            drop(state);
            self.record_authorization_denial(event);
            return Err(denial(
                DenialReason::ScopeDenied,
                permission,
                revision,
            ));
        }
        if context.provider_id != session.provider.registration.provider_id {
            let event = denial_event(session, permission, DenialReason::ScopeDenied);
            let revision = event.grant_revision;
            drop(state);
            self.record_authorization_denial(event);
            return Err(denial(
                DenialReason::ScopeDenied,
                permission,
                revision,
            ));
        }
        if !session.grants.contains(permission) {
            let event = denial_event(session, permission, DenialReason::NotGranted);
            let revision = event.grant_revision;
            drop(state);
            self.record_authorization_denial(event);
            return Err(denial(
                DenialReason::NotGranted,
                permission,
                revision,
            ));
        }
        let operation = state
            .operations
            .get(&(context.session_id.clone(), context.operation_id.clone()))
            .ok_or_else(|| Status::failed_precondition("provider operation is stale"))?;
        let current = epoch(&session.provider);
        let account_scoped = account_scoped(operation.kind);
        if operation.context != *context
            || current.1 != context.profile_epoch
            || (account_scoped
                && (current.0 != context.account_epoch
                    || operation.identity_revision
                        != session
                            .provider
                            .identity_revision
                            .load(std::sync::atomic::Ordering::Acquire)))
        {
            return Err(Status::failed_precondition("provider operation is stale"));
        }
        if !operation.permissions.contains(permission) {
            let event = denial_event(session, permission, DenialReason::NotGranted);
            let revision = event.grant_revision;
            drop(state);
            self.record_authorization_denial(event);
            return Err(denial(
                DenialReason::NotGranted,
                permission,
                revision,
            ));
        }
        Ok(session.provider.clone())
    }

    fn lease(
        &self,
        context: Option<ServiceContext>,
        permission: &'static str,
    ) -> Result<(Lease, Arc<dyn HostServices>), Status> {
        let context =
            context.ok_or_else(|| Status::unauthenticated("provider context is required"))?;
        let provider = self.authorize(&context, permission)?;
        Ok((
            Lease {
                api: self.clone(),
                context,
                permission,
            },
            provider.registration.services.clone(),
        ))
    }

    pub async fn execute(
        &self,
        provider_id: &str,
        kind: ProviderOperation,
    ) -> Result<Completion, Status> {
        self.execute_payload(provider_id, kind, None).await
    }

    pub async fn execute_settings(&self, request: settings::Request) -> Result<Completion, Status> {
        request
            .validate()
            .map_err(|_| Status::invalid_argument("invalid settings request"))?;
        self.execute_payload(
            &request.context.provider_id.clone(),
            request.operation(),
            Some(request),
        )
        .await
    }

    pub fn settings_scope(
        &self,
        provider: &str,
        handle: String,
    ) -> Result<settings::Context, Status> {
        let state = lock(&self.state);
        let (id, session) = state
            .sessions
            .iter()
            .find(|(_, s)| s.provider.registration.provider_id == provider && s.commands.is_some())
            .ok_or_else(|| Status::unavailable("provider process unavailable"))?;
        if !session.grants.contains(cap::SETTINGS) {
            return Err(denial(
                DenialReason::NotGranted,
                cap::SETTINGS,
                session.grant_revision,
            ));
        }
        Ok(settings::Context {
            provider_id: provider.into(),
            plugin_id: session.provider.registration.plugin_id.clone(),
            session_id: id.clone(),
            profile_epoch: epoch(&session.provider).1,
            form_handle: handle,
        })
    }

    pub fn settings_authorized(&self, provider: &str) -> bool {
        lock(&self.state).providers.get(provider).is_some_and(|p| {
            p.registration
                .capabilities
                .iter()
                .any(|c| c == cap::SETTINGS)
        })
    }

    /// Revocation affects this process session immediately. A later launch
    /// negotiates again from the trusted descriptor's approved permission set.
    pub async fn revoke_session_permission(&self, provider_id: &str, permission: &str) {
        let provider = lock(&self.state).providers.get(provider_id).cloned();
        let Some(provider) = provider else {
            return;
        };
        let _guard = provider.commits.lock().await;
        let mut state = lock(&self.state);
        for session in state
            .sessions
            .values_mut()
            .filter(|s| s.provider.registration.provider_id == provider_id)
        {
            session.grants.remove(permission);
        }
        let obsolete: Vec<_> = state
            .operations
            .iter()
            .filter(|(_, op)| {
                op.context.provider_id == provider_id && op.permissions.contains(permission)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in obsolete {
            if let Some(op) = state.operations.remove(&key) {
                let _ = op.result.send(Err(Status::permission_denied(
                    "provider permission revoked",
                )));
            }
        }
    }

    pub async fn cancel_settings(&self, context: &settings::Context) {
        let provider = lock(&self.state)
            .providers
            .get(&context.provider_id)
            .cloned();
        let Some(provider) = provider else {
            return;
        };
        let _guard = provider.commits.lock().await;
        let mut state = lock(&self.state);
        let obsolete: Vec<_> = state
            .operations
            .iter()
            .filter(|(_, op)| {
                op.settings
                    .as_ref()
                    .is_some_and(|request| request.context == *context)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in obsolete {
            if let Some(op) = state.operations.remove(&key) {
                let _ = op
                    .result
                    .send(Err(Status::cancelled("settings form closed")));
            }
        }
    }

    async fn execute_payload(
        &self,
        provider_id: &str,
        kind: ProviderOperation,
        settings: Option<settings::Request>,
    ) -> Result<Completion, Status> {
        if is_settings(kind) != settings.is_some() {
            return Err(Status::invalid_argument("missing settings scope"));
        }
        let provider = lock(&self.state)
            .providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| Status::unavailable("provider unavailable"))?;
        let guard = if matches!(
            kind,
            ProviderOperation::SignIn
                | ProviderOperation::SignOut
                | ProviderOperation::CancelSignIn
        ) {
            Some(provider.commits.lock().await)
        } else {
            None
        };
        let (context, commands, result) = {
            let mut state = lock(&self.state);
            let (session_id, session) = state
                .sessions
                .iter()
                .find(|(_, s)| {
                    s.provider.registration.provider_id == provider_id && s.commands.is_some()
                })
                .ok_or_else(|| Status::unavailable("provider process is unavailable"))?;
            let session_id = session_id.clone();
            let provider = session.provider.clone();
            let credential = session.credential;
            let grants = session.grants.clone();
            let session_kind = session.kind;
            let target_game = session.target_game.clone();
            let required = match kind {
                ProviderOperation::Discover => cap::DISCOVERY,
                ProviderOperation::SyncCatalog => cap::CATALOG,
                ProviderOperation::ReadCounter => cap::COUNTER,
                ProviderOperation::SettingsDescribeRead
                | ProviderOperation::SettingsSubmit
                | ProviderOperation::SettingsCancel => cap::SETTINGS,
                ProviderOperation::ConfigurationOpen
                | ProviderOperation::ConfigurationFocus
                | ProviderOperation::ConfigurationClose => cap::CONFIGURE,
                ProviderOperation::SignIn
                | ProviderOperation::FocusSignIn
                | ProviderOperation::CancelSignIn => cap::LOGIN,
                ProviderOperation::Check | ProviderOperation::SignOut => cap::ACCOUNT_PUBLISH,
                _ => cap::CONTEXT,
            };
            if !grants.contains(required) {
                return Err(Status::unimplemented(
                    "required provider capability is unavailable",
                ));
            }
            let login_pending = state.operations.iter().any(|((id, _), operation)| {
                id == &session_id && operation.kind == ProviderOperation::SignIn
            });
            let login_committed = state.operations.iter().any(|((id, _), operation)| {
                id == &session_id
                    && operation.kind == ProviderOperation::SignIn
                    && (operation.secret_written || operation.identity_accepted)
            });
            if (kind == ProviderOperation::SignIn && login_pending)
                || (kind == ProviderOperation::CancelSignIn && login_committed)
            {
                return Ok(Completion {
                    result: OperationResult {
                        disposition: OperationDisposition::Superseded as i32,
                        ..Default::default()
                    },
                    catalog: models::CatalogUpdate::default(),
                    guard: None,
                });
            }
            let commands = session
                .commands
                .clone()
                .ok_or_else(|| Status::unavailable("provider command channel unavailable"))?;
            if matches!(
                kind,
                ProviderOperation::Unspecified
                    | ProviderOperation::Acknowledge
                    | ProviderOperation::Shutdown
            ) {
                return Err(Status::invalid_argument("invalid operation"));
            }
            if state
                .operations
                .keys()
                .filter(|(session, _)| *session == session_id)
                .count()
                >= MAX_OPERATIONS
            {
                return Err(Status::resource_exhausted("provider operation capacity"));
            }
            if matches!(kind, ProviderOperation::SignIn | ProviderOperation::SignOut)
                || (kind == ProviderOperation::CancelSignIn && login_pending)
            {
                provider
                    .account_epoch
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                let obsolete: Vec<_> = state
                    .operations
                    .iter()
                    .filter(|((session, _), operation)| {
                        *session == session_id && account_scoped(operation.kind)
                    })
                    .map(|(key, _)| key.clone())
                    .collect();
                for key in obsolete {
                    if let Some(operation) = state.operations.remove(&key) {
                        let _ = operation.result.send(Err(Status::failed_precondition(
                            "provider operation superseded",
                        )));
                    }
                }
            }
            let (account_epoch, profile_epoch) = epoch(&provider);
            let context = ServiceContext {
                session_id,
                session_credential: credential.to_vec(),
                provider_id: provider_id.into(),
                operation_id: super::random_session_id()?,
                account_epoch,
                profile_epoch,
                session_kind,
                target_game,
            };
            if let Some(request) = &settings
                && !request.matches_service(&context, &provider.registration.plugin_id)
            {
                return Err(Status::failed_precondition("settings form expired"));
            }
            let (complete, result) = oneshot::channel();
            let permissions = permission_set(kind)
                .into_iter()
                .filter(|cap| grants.contains(*cap))
                .collect();
            state.operations.insert(
                (context.session_id.clone(), context.operation_id.clone()),
                Operation {
                    context: context.clone(),
                    kind,
                    permissions,
                    catalog: CatalogTransfer::default(),
                    result: complete,
                    identity_revision: provider
                        .identity_revision
                        .load(std::sync::atomic::Ordering::Acquire),
                    secret_written: false,
                    secret_deleted: false,
                    identity_accepted: false,
                    settings: settings.clone(),
                },
            );
            (context, commands, result)
        };
        drop(guard);
        let key = (context.session_id.clone(), context.operation_id.clone());
        if commands
            .try_send(Ok(ProviderCommand {
                context: Some(context.clone()),
                operation: kind as i32,
                target_operation_id: String::new(),
                settings: settings.map(Into::into),
                runtime_session_result: None,
            }))
            .is_err()
        {
            lock(&self.state).operations.remove(&key);
            return Err(Status::resource_exhausted("provider command channel full"));
        }
        let duration = operation_timeout(kind);
        match tokio::time::timeout(duration, result).await {
            Ok(Ok(result)) => result,
            _ => {
                let _guard = provider.commits.lock().await;
                lock(&self.state).operations.remove(&key);
                Err(Status::deadline_exceeded(
                    "provider operation did not complete",
                ))
            }
        }
    }

    async fn event(&self, event: ProviderEvent) -> Result<(), Status> {
        let context = event
            .context
            .ok_or_else(|| Status::unauthenticated("provider context is required"))?;
        let permission = match event.event.as_ref() {
            Some(provider_event::Event::AccountStatus(_)) => cap::ACCOUNT_PUBLISH,
            Some(provider_event::Event::CatalogChunk(_)) => cap::CATALOG,
            Some(provider_event::Event::Result(result)) if result.settings.is_some() => {
                cap::SETTINGS
            }
            Some(provider_event::Event::Result(result)) if result.configuration.is_some() => {
                cap::CONFIGURE
            }
            Some(provider_event::Event::Result(_)) => cap::CONTEXT,
            Some(provider_event::Event::RuntimeSession(_)) => {
                return Err(Status::unimplemented(
                    "runtime session reporting is not available in this host version",
                ));
            }
            _ => return Err(Status::invalid_argument("invalid provider event")),
        };
        let provider = self.authorize(&context, permission)?;
        let _guard = provider.commits.lock().await;
        self.authorize(&context, permission)?;
        match event
            .event
            .ok_or_else(|| Status::invalid_argument("missing provider event"))?
        {
            provider_event::Event::AccountStatus(status) => {
                if status.state == AccountConnection::NotConnected as i32 {
                    let state = lock(&self.state);
                    if state
                        .operations
                        .get(&(context.session_id.clone(), context.operation_id.clone()))
                        .is_some_and(|operation| {
                            operation.kind == ProviderOperation::SignOut
                                && !operation.secret_deleted
                        })
                    {
                        return Err(Status::permission_denied(
                            "sign-out requires successful credential deletion",
                        ));
                    }
                }
                let status = status
                    .try_into()
                    .map_err(|_| Status::invalid_argument("invalid account status"))?;
                provider.registration.services.publish_status(status);
            }
            provider_event::Event::CatalogChunk(chunk) => {
                lock(&self.state)
                    .operations
                    .get_mut(&(context.session_id, context.operation_id))
                    .ok_or_else(|| Status::failed_precondition("stale operation"))?
                    .catalog
                    .push(chunk)
                    .map_err(|_| Status::invalid_argument("invalid catalog transfer"))?;
            }
            provider_event::Event::Result(result) => {
                for (present, permission) in [
                    (result.status.is_some(), cap::ACCOUNT_PUBLISH),
                    (result.discovery.is_some(), cap::DISCOVERY),
                    (!result.counters.is_empty(), cap::COUNTER),
                    (!result.catalogs.is_empty(), cap::CATALOG),
                ] {
                    if present {
                        self.authorize(&context, permission)?;
                    }
                }
                let disposition = OperationDisposition::try_from(result.disposition)
                    .map_err(|_| Status::invalid_argument("invalid operation disposition"))?;
                if disposition == OperationDisposition::Unspecified {
                    return Err(Status::invalid_argument("missing operation disposition"));
                }
                if result.login_outcome.is_some_and(|value| {
                    models::SignInResult::try_from(
                        AccountLoginOutcome::try_from(value)
                            .unwrap_or(AccountLoginOutcome::Unspecified),
                    )
                    .is_err()
                }) {
                    return Err(Status::invalid_argument("invalid login outcome"));
                }
                let operation_kind = lock(&self.state)
                    .operations
                    .get(&(context.session_id.clone(), context.operation_id.clone()))
                    .map(|operation| operation.kind)
                    .ok_or_else(|| Status::failed_precondition("operation already completed"))?;
                if (result.login_outcome.is_some() && operation_kind != ProviderOperation::SignIn)
                    || (result.focused.is_some()
                        && operation_kind != ProviderOperation::FocusSignIn)
                {
                    return Err(Status::permission_denied("invalid account result scope"));
                }
                if result.counters.len() > 50_000
                    || result.counters.iter().any(|c| {
                        c.id.is_empty() || c.id.len() > 256 || c.seconds < 0 || c.last_played < 0
                    })
                {
                    return Err(Status::invalid_argument("invalid counter observations"));
                }
                if let Some(discovery) = &result.discovery
                    && (discovery.provider_id != context.provider_id
                        || discovery.records.len() > 50_000)
                {
                    return Err(Status::permission_denied("wrong discovery provider"));
                }
                let status: Option<models::AccountStatus> = result
                    .status
                    .clone()
                    .map(TryInto::try_into)
                    .transpose()
                    .map_err(|_| Status::invalid_argument("invalid account status"))?;
                let (operation, commands) = {
                    let mut state = lock(&self.state);
                    let key = (context.session_id.clone(), context.operation_id.clone());
                    let operation = state.operations.remove(&key).ok_or_else(|| {
                        Status::failed_precondition("operation already completed")
                    })?;
                    let session = state
                        .sessions
                        .get_mut(&context.session_id)
                        .ok_or_else(|| Status::unauthenticated("session ended"))?;
                    session.completed.push_back(context.operation_id.clone());
                    while session.completed.len() > HISTORY {
                        session.completed.pop_front();
                    }
                    (operation, session.commands.clone())
                };
                let valid_settings = match (&operation.settings, &result.settings) {
                    (Some(request), Some(reply)) => settings::Reply::try_from(reply.clone())
                        .is_ok_and(|reply| reply.validate_for(request).is_ok()),
                    (None, None) => true,
                    _ => false,
                };
                if !valid_settings {
                    let _ = operation
                        .result
                        .send(Err(Status::invalid_argument("invalid settings response")));
                    return Err(Status::invalid_argument("invalid settings response"));
                }
                if operation.kind == ProviderOperation::SignOut
                    && !operation.secret_deleted
                    && (result.signed_out
                        || result
                            .status
                            .as_ref()
                            .is_some_and(|s| s.state == AccountConnection::NotConnected as i32))
                {
                    let _ = operation.result.send(Err(Status::permission_denied(
                        "sign-out requires successful credential deletion",
                    )));
                    return Err(Status::permission_denied(
                        "sign-out requires successful credential deletion",
                    ));
                }
                if result.signed_out && operation.kind != ProviderOperation::SignOut {
                    let _ = operation
                        .result
                        .send(Err(Status::permission_denied("invalid account transition")));
                    return Err(Status::permission_denied("invalid account transition"));
                }
                let catalog = match operation
                    .catalog
                    .finish(result.catalogs.clone(), result.observed_at)
                {
                    Ok(catalog) => catalog,
                    Err(_) => {
                        let _ = operation
                            .result
                            .send(Err(Status::invalid_argument("incomplete catalog")));
                        return Err(Status::invalid_argument("incomplete catalog"));
                    }
                };
                if let Some(status) = status {
                    provider.registration.services.publish_status(status);
                }
                let guard = ResultGuard {
                    api: self.clone(),
                    context: context.clone(),
                    account_scoped: account_scoped(operation.kind),
                    identity_revision: operation.identity_revision,
                    permissions: operation.permissions.clone(),
                };
                let _ = operation.result.send(Ok(Completion {
                    result,
                    catalog,
                    guard: Some(guard),
                }));
                if let Some(commands) = commands {
                    let _ = commands.try_send(Ok(ProviderCommand {
                        context: Some(context),
                        operation: ProviderOperation::Acknowledge as i32,
                        target_operation_id: String::new(),
                        settings: None,
                        runtime_session_result: None,
                    }));
                }
            }
            _ => return Err(Status::invalid_argument("invalid provider event")),
        }
        Ok(())
    }
}

#[tonic::async_trait]
impl provider_host_server::ProviderHost for ProviderApi {
    type ExchangeStream = ReceiverStream<Result<ProviderCommand, Status>>;

    async fn exchange(
        &self,
        request: Request<tonic::Streaming<ProviderEvent>>,
    ) -> Result<Response<Self::ExchangeStream>, Status> {
        let mut input = request.into_inner();
        let event = tokio::time::timeout(Duration::from_secs(10), input.message())
            .await
            .map_err(|_| Status::deadline_exceeded("provider stream startup timed out"))??
            .ok_or_else(|| Status::unauthenticated("missing ready event"))?;
        let context = event
            .context
            .ok_or_else(|| Status::unauthenticated("missing ready context"))?;
        if !matches!(event.event, Some(provider_event::Event::Ready(true))) {
            return Err(Status::invalid_argument("expected ready event"));
        }
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
        {
            let mut state = lock(&self.state);
            let session = state
                .sessions
                .get_mut(&context.session_id)
                .ok_or_else(|| Status::unauthenticated("unknown session"))?;
            if !super::constant_time_eq(&session.credential, &context.session_credential)
                || session.provider.registration.provider_id != context.provider_id
                || session.commands.is_some()
            {
                return Err(Status::permission_denied("invalid stream session"));
            }
            session.commands = Some(tx.clone());
            if let Some(ready) = session.ready.take() {
                let _ = ready.send(());
            }
        }
        let api = self.clone();
        let session_id = context.session_id;
        tokio::spawn(async move {
            while let Ok(Some(event)) = input.message().await {
                if event
                    .context
                    .as_ref()
                    .is_none_or(|context| context.session_id != session_id)
                {
                    let _ = tx
                        .send(Err(Status::permission_denied(
                            "event belongs to another stream session",
                        )))
                        .await;
                    break;
                }
                if let Err(error) = api.event(event).await {
                    // Obsolete/duplicate results are ignored and cannot tear down
                    // newer work on this same persistent provider connection.
                    if error.code() == tonic::Code::FailedPrecondition {
                        continue;
                    }
                    let _ = tx.send(Err(error)).await;
                    break;
                }
            }
            api.disconnect_session(&session_id).await;
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn get_context(
        &self,
        request: Request<ServiceContext>,
    ) -> Result<Response<ProviderContext>, Status> {
        let (lease, services) = self.lease(Some(request.into_inner()), cap::CONTEXT)?;
        Ok(Response::new(services.context(lease).await?))
    }

    async fn account(
        &self,
        request: Request<AccountRequest>,
    ) -> Result<Response<AccountReply>, Status> {
        let request = request.into_inner();
        let action = request
            .action
            .ok_or_else(|| Status::invalid_argument("missing account action"))?;
        if matches!(
            action,
            account_request::Action::Read(false) | account_request::Action::Clear(false)
        ) {
            return Err(Status::invalid_argument("invalid account action"));
        }
        let permission = if matches!(action, account_request::Action::Read(true)) {
            cap::ACCOUNT_READ
        } else {
            cap::ACCOUNT_PUBLISH
        };
        let (lease, services) = self.lease(request.context, permission)?;
        if matches!(action, account_request::Action::Clear(true)) {
            let state = lock(&self.state);
            let operation = state
                .operations
                .get(&(
                    lease.context.session_id.clone(),
                    lease.context.operation_id.clone(),
                ))
                .ok_or_else(|| Status::failed_precondition("stale account operation"))?;
            if operation.kind != ProviderOperation::SignOut || !operation.secret_deleted {
                return Err(Status::permission_denied(
                    "account removal requires credential deletion",
                ));
            }
        }
        Ok(Response::new(services.account(lease, action).await?))
    }

    async fn secret(
        &self,
        request: Request<SecretRequest>,
    ) -> Result<Response<SecretReply>, Status> {
        let request = request.into_inner();
        if request.key != "session"
            || request
                .value
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 32 * 1024)
        {
            return Err(Status::permission_denied("secret slot is not authorized"));
        }
        let operation = SecretOperation::try_from(request.operation)
            .map_err(|_| Status::invalid_argument("invalid secret action"))?;
        if (operation == SecretOperation::Write) != request.value.is_some() {
            return Err(Status::invalid_argument("invalid secret value"));
        }
        let permission = match operation {
            SecretOperation::Read => cap::SECRET_READ,
            SecretOperation::Write => cap::SECRET_WRITE,
            SecretOperation::Delete => cap::SECRET_DELETE,
            _ => return Err(Status::invalid_argument("missing secret action")),
        };
        let (lease, services) = self.lease(request.context.clone(), permission)?;
        Ok(Response::new(services.secret(lease, request).await?))
    }

    async fn login(&self, request: Request<LoginRequest>) -> Result<Response<LoginReply>, Status> {
        let request = request.into_inner();
        let (lease, services) = self.lease(request.context.clone(), cap::LOGIN)?;
        if request.operation == LoginOperation::Read as i32 {
            self.authorize(&lease.context, cap::COOKIES)?;
            self.authorize(&lease.context, cap::PAGE)?;
        }
        Ok(Response::new(services.login(lease, request).await?))
    }

    async fn declare_actions(
        &self,
        request: Request<ActionRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        let request = request.into_inner();
        let (lease, services) = self.lease(request.context, cap::ACTIONS)?;
        Ok(Response::new(
            services
                .actions(
                    lease,
                    request
                        .actions
                        .ok_or_else(|| Status::invalid_argument("missing actions"))?,
                )
                .await?,
        ))
    }
}

#[cfg(all(test, unix))]
mod tests;
