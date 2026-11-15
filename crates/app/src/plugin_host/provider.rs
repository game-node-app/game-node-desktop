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

use super::namespace::ExternalNamespaceResolver;
use super::profile::PluginProfiles;

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
    /// Whether this provider's trusted login policy reads cookies. The transport requires the
    /// cookie grant only when it does; a probe-only policy needs just the page grant.
    fn login_reads_cookies(&self) -> bool {
        true
    }
}

/// Who composed a registration. Official providers are bundled with GameNode; third-party
/// providers come from managed packages and are bound to the active GameNode account profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistrationOrigin {
    Official,
    ThirdParty { publisher_fingerprint: String },
}

/// A provider owns a library namespace; an extension is launched for a host-selected game and
/// must never claim the target game's provider identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationKind {
    Provider,
    Extension,
}

pub struct Registration {
    /// Provider id for provider registrations or a host-generated internal key for extensions.
    pub provider_id: String,
    pub plugin_id: String,
    pub kind: RegistrationKind,
    pub origin: RegistrationOrigin,
    pub capabilities: Vec<String>,
    /// Whether this provider's trusted manifest declares shared catalog access.
    pub shared_catalog: bool,
    pub services: Arc<dyn HostServices>,
}

/// Internal lookup key for one provider-free extension. It is not a provider id and is never
/// placed into the extension's authenticated `ServiceContext`.
pub(crate) fn extension_registration_id(plugin_id: &str) -> String {
    format!("extension:{plugin_id}")
}

fn context_provider_id(registration: &Registration) -> &str {
    match registration.kind {
        RegistrationKind::Provider => &registration.provider_id,
        RegistrationKind::Extension => "",
    }
}

struct Provider {
    registration: Registration,
    /// Set for third-party providers: their profile epoch is the account profile's epoch.
    profiles: Option<Arc<PluginProfiles>>,
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
    /// The operation id of the session's resource context (API 1.6+); `None` for sessions that
    /// negotiated an earlier minor, which cannot call resource services.
    resource_operation: Option<String>,
    resource_context: Option<ServiceContext>,
}

/// An authorized plugin-initiated resource call. It snapshots the session's scope at check time;
/// long-lived streams check again before each delivery.
#[derive(Clone, Debug)]
pub(crate) struct ResourceCaller {
    pub session_id: String,
    pub provider_id: String,
    pub grants: BTreeSet<String>,
    pub grant_revision: u64,
    pub profile_epoch: u64,
    /// Host-owned private-resource namespace of an authenticated third-party plugin in the
    /// active profile; `None` for official providers, which use trusted host adapters.
    pub namespace: Option<ExternalNamespaceResolver>,
}

impl ResourceCaller {
    pub fn has(&self, permission: &str) -> bool {
        self.grants.contains(permission)
    }
}

struct Operation {
    context: ServiceContext,
    kind: ProviderOperation,
    permissions: BTreeSet<&'static str>,
    catalog: CatalogTransfer,
    requested_achievements: BTreeSet<String>,
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
    profiles: Arc<PluginProfiles>,
}

pub struct Launch {
    pub credential: String,
    pub ready: oneshot::Receiver<()>,
}

#[derive(Debug)]
pub struct Completion {
    pub result: OperationResult,
    pub catalog: models::CatalogUpdate,
    /// Validated per-game achievement outcomes for a `ReadAchievements` operation.
    pub achievements: Vec<models::AchievementOutcome>,
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
            achievements: Vec::new(),
            guard: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture_achievements(outcomes: Vec<models::AchievementOutcome>) -> Self {
        Self {
            result: OperationResult {
                disposition: OperationDisposition::Completed as i32,
                ..Default::default()
            },
            catalog: models::CatalogUpdate::default(),
            achievements: outcomes,
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

    /// Host-owned namespace of the authenticated third-party plugin in its active account
    /// profile. Ownership comes from the registration and profile, never from the request.
    pub fn namespace(&self) -> Result<ExternalNamespaceResolver, Status> {
        let provider = self.api.authorize(&self.context, self.permission)?;
        let RegistrationOrigin::ThirdParty {
            publisher_fingerprint,
        } = &provider.registration.origin
        else {
            return Err(Status::failed_precondition(
                "official providers use trusted host adapters",
            ));
        };
        let profiles = provider
            .profiles
            .as_ref()
            .ok_or_else(super::profile::no_profile)?;
        let resolver = profiles.resolver(
            &provider.registration.plugin_id,
            publisher_fingerprint,
            Some(&provider.registration.provider_id),
        )?;
        if !resolver.is_current_profile_epoch(self.context.profile_epoch) {
            return Err(Status::failed_precondition("provider operation is stale"));
        }
        Ok(resolver)
    }

    /// Reject a host-side scope mismatch with the same machine-readable denial
    /// contract used for missing grants.
    pub fn deny_scope(&self) -> Status {
        self.deny_scope_for(self.permission)
    }

    pub fn deny_scope_for(&self, permission: &'static str) -> Status {
        self.api
            .scope_denial(&self.context, permission)
            .unwrap_or_else(|| Status::unauthenticated("provider session expired"))
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

// A catalog sync may validate the session, read owned games and read shared games,
// each bounded at 90 seconds. Leave bounded headroom for host storage/RPCs.
fn operation_timeout(kind: ProviderOperation) -> Duration {
    match kind {
        ProviderOperation::SignIn => Duration::from_secs(615),
        ProviderOperation::SyncCatalog => Duration::from_secs(300),
        ProviderOperation::ReadAchievements => Duration::from_secs(300),
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
        match &provider.profiles {
            Some(profiles) => profiles.epoch(),
            None => provider.profile_epoch.load(Ordering::Acquire),
        },
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
        ReadAchievements => &[
            cap::CONTEXT,
            cap::ACCOUNT_READ,
            cap::ACCOUNT_PUBLISH,
            cap::SECRET_READ,
            cap::SECRET_WRITE,
            cap::SECRET_DELETE,
            cap::ACHIEVEMENTS,
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
    fn scope_denial(&self, context: &ServiceContext, permission: &str) -> Option<Status> {
        let state = lock(&self.state);
        let session = state.sessions.get(&context.session_id)?;
        let event = denial_event(session, permission, DenialReason::ScopeDenied);
        let revision = event.grant_revision;
        drop(state);
        self.record_authorization_denial(event);
        Some(denial(DenialReason::ScopeDenied, permission, revision))
    }

    fn record_authorization_denial(&self, event: AuthorizationAuditEvent) {
        let mut state = lock(&self.state);
        if state.authorization_audit.len() == AUTHORIZATION_AUDIT_LIMIT {
            state.authorization_audit.pop_front();
        }
        state.authorization_audit.push_back(event);
    }

    /// Deliver a rejected stream submission to its authenticated operation
    /// waiter before the Exchange stream is closed. A forged context cannot
    /// cancel another operation: correlation requires the exact session,
    /// credential, provider, and host-issued operation context.
    fn reject_stream_operation(&self, context: &ServiceContext, error: Status) {
        let operation = {
            let mut state = lock(&self.state);
            let Some(session) = state.sessions.get(&context.session_id) else {
                return;
            };
            if !super::constant_time_eq(&session.credential, &context.session_credential)
                || session.provider.registration.provider_id != context.provider_id
            {
                return;
            }
            let key = (context.session_id.clone(), context.operation_id.clone());
            if !state
                .operations
                .get(&key)
                .is_some_and(|operation| operation.context == *context)
            {
                return;
            }
            let operation = state.operations.remove(&key);
            if let Some(session) = state.sessions.get_mut(&context.session_id) {
                session.completed.push_back(context.operation_id.clone());
                while session.completed.len() > HISTORY {
                    session.completed.pop_front();
                }
            }
            operation
        };
        if let Some(operation) = operation {
            let _ = operation.result.send(Err(error));
        }
    }

    /// Returns the bounded, redacted authorization-denial history for host UI
    /// and diagnostics. This is never exposed through the plugin RPC surface.
    pub fn authorization_audit(&self) -> Vec<AuthorizationAuditEvent> {
        lock(&self.state)
            .authorization_audit
            .iter()
            .cloned()
            .collect()
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
    pub async fn replace_session_grants(
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
        // Serialize grant changes with every host-side effect commit. A queued
        // operation which has not crossed its commit point will reauthorize
        // against the new grant set after this guard is released.
        let provider = lock(&self.state)
            .sessions
            .get(session_id)
            .map(|session| session.provider.clone())
            .ok_or_else(|| Status::unauthenticated("unknown plugin session"))?;
        let _commit = provider.commits.lock().await;
        let mut state = lock(&self.state);
        let session = state
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| Status::unauthenticated("unknown plugin session"))?;
        let removed: BTreeSet<_> = session.grants.difference(&grants).cloned().collect();
        let revoked = !removed.is_empty();
        session.grants = grants;
        session.grant_revision = session.grant_revision.saturating_add(1);
        let change = permission_change(session, revoked);
        session
            .permission_watchers
            .retain(|watcher| watcher.try_send(Ok(change.clone())).is_ok());
        let revision = session.grant_revision;
        if revoked {
            let obsolete: Vec<_> = state
                .operations
                .iter()
                .filter(|((id, _), operation)| {
                    id == session_id && operation.permissions.iter().any(|p| removed.contains(*p))
                })
                .map(|(key, operation)| {
                    let permission = operation
                        .permissions
                        .iter()
                        .find(|permission| removed.contains(**permission))
                        .copied()
                        .unwrap_or(cap::CONTEXT);
                    (key.clone(), permission)
                })
                .collect();
            for (key, permission) in obsolete {
                if let Some(operation) = state.operations.remove(&key) {
                    let _ = operation.result.send(Err(denial(
                        DenialReason::Revoked,
                        permission,
                        revision,
                    )));
                }
            }
        }
        Ok(revision)
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
                            provider_id: context_provider_id(&session.provider.registration).into(),
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
                        achievements: None,
                    }));
                }
            }
        }
    }

    /// Registration is host-owned composition, never a plugin RPC.
    pub fn register(&self, registration: Registration) -> Result<(), Status> {
        if registration.provider_id.is_empty()
            || registration.provider_id.len() > 256
            || registration.plugin_id.is_empty()
            || registration.plugin_id.len() > 256
        {
            return Err(Status::invalid_argument("invalid provider registration"));
        }
        match registration.kind {
            RegistrationKind::Provider if registration.provider_id.contains(':') => {
                return Err(Status::invalid_argument("invalid provider registration"));
            }
            RegistrationKind::Extension
                if !matches!(&registration.origin, RegistrationOrigin::ThirdParty { .. })
                    || registration.provider_id
                        != extension_registration_id(&registration.plugin_id)
                    || registration.shared_catalog =>
            {
                return Err(Status::invalid_argument(
                    "invalid provider-free extension registration",
                ));
            }
            _ => {}
        }
        cap::negotiate(
            gamenode_plugin_protocol::API_MAJOR,
            gamenode_plugin_protocol::API_MINOR,
            &registration.capabilities,
            cap::KNOWN,
        )
        .map_err(|_| Status::invalid_argument("invalid provider grants"))?;
        match registration.kind {
            RegistrationKind::Provider
                if !registration
                    .capabilities
                    .iter()
                    .any(|s| s == cap::CONTEXT || s == cap::SETTINGS || s == cap::CONFIGURE) =>
            {
                return Err(Status::invalid_argument(
                    "full provider requires context access",
                ));
            }
            RegistrationKind::Extension
                if !registration
                    .capabilities
                    .iter()
                    .any(|s| s == cap::GAME_LAUNCH || s == cap::GAME_CUSTOM_LAUNCH) =>
            {
                return Err(Status::invalid_argument(
                    "extension requires an approved game action",
                ));
            }
            _ => {}
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
                profiles: matches!(registration.origin, RegistrationOrigin::ThirdParty { .. })
                    .then(|| self.profiles.clone()),
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
        // A third-party process runs only inside an active GameNode account profile.
        if provider
            .profiles
            .as_ref()
            .is_some_and(|profiles| !profiles.may_start_third_party())
        {
            return Err(super::profile::no_profile());
        }
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
        if pending.provider.registration.plugin_id != hello.plugin_id {
            return Err(Status::unauthenticated(
                "provider identity does not match launch",
            ));
        }
        if hello.plugin_version.is_empty() || hello.plugin_version.len() > 128 {
            return Err(Status::invalid_argument("invalid plugin version"));
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
        let extension_session = hello.session_kind == SessionKind::Extension as i32;
        if extension_session != (pending.provider.registration.kind == RegistrationKind::Extension)
        {
            return Err(Status::permission_denied(
                "session kind does not match the registered plugin kind",
            ));
        }
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
        if extension_session && selection.api_minor < cap::THIRD_PARTY_API_MINOR {
            return Err(Status::failed_precondition(
                "extension sessions require API 1.6",
            ));
        }
        let mut seen_capabilities = BTreeSet::new();
        if hello.requested_capabilities.is_empty()
            || hello
                .requested_capabilities
                .iter()
                .any(|capability| !seen_capabilities.insert(capability.as_str()))
        {
            return Err(Status::invalid_argument(
                "invalid provider capability request",
            ));
        }
        let minor = cap::negotiate(
            selection.api_major,
            selection.api_minor,
            &hello.requested_capabilities,
            &allowed,
        )
        .map_err(|error| match error {
            cap::NegotiationError::Capability => {
                if let Some(permission) = hello
                    .requested_capabilities
                    .iter()
                    .find(|permission| !cap::KNOWN.contains(&permission.as_str()))
                {
                    Status::failed_precondition(format!(
                        "unsupported provider permission: {permission}"
                    ))
                } else if let Some(permission) = hello
                    .requested_capabilities
                    .iter()
                    .find(|permission| !allowed.contains(&permission.as_str()))
                {
                    denial(DenialReason::NotGranted, permission, 0)
                } else {
                    Status::invalid_argument(error.to_string())
                }
            }
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
        let resource_context = if minor >= cap::THIRD_PARTY_API_MINOR {
            let (account_epoch, profile_epoch) = epoch(&pending.provider);
            Some(ServiceContext {
                session_id: session_id.clone(),
                session_credential: pending.credential.to_vec(),
                provider_id: context_provider_id(&pending.provider.registration).into(),
                operation_id: super::random_session_id()?,
                account_epoch,
                profile_epoch,
                session_kind: hello.session_kind,
                target_game: hello.target_game.clone(),
            })
        } else {
            None
        };
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
                resource_context: resource_context.clone(),
                resource_operation: resource_context
                    .as_ref()
                    .map(|context| context.operation_id.clone()),
            },
        );
        Ok(OpenSessionReply {
            resource_context,
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

    /// Registered providers bound to an account profile. Their supervised processes must stop
    /// whenever that profile changes; official providers are deliberately excluded.
    pub(crate) fn third_party_provider_ids(&self) -> Vec<String> {
        lock(&self.state)
            .providers
            .values()
            .filter(|provider| provider.profiles.is_some())
            .map(|provider| provider.registration.provider_id.clone())
            .collect()
    }

    /// Provider process registrations owned by one installed third-party plugin.
    pub(crate) fn third_party_provider_ids_for_plugin(&self, plugin_id: &str) -> Vec<String> {
        lock(&self.state)
            .providers
            .values()
            .filter(|provider| {
                provider.profiles.is_some() && provider.registration.plugin_id == plugin_id
            })
            .map(|provider| provider.registration.provider_id.clone())
            .collect()
    }

    /// Fences sessions and unregisters a third-party plugin after its process has been stopped.
    /// Official registrations are never eligible for removal through this lifecycle path.
    pub(crate) async fn unregister_third_party_plugin(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<String>, Status> {
        let providers: Vec<_> = {
            let state = lock(&self.state);
            let matching = state
                .providers
                .values()
                .filter(|provider| provider.registration.plugin_id == plugin_id)
                .cloned()
                .collect::<Vec<_>>();
            if matching.iter().any(|provider| provider.profiles.is_none()) {
                return Err(Status::permission_denied(
                    "official plugin registrations cannot be removed here",
                ));
            }
            matching
        };
        let provider_ids = providers
            .iter()
            .map(|provider| provider.registration.provider_id.clone())
            .collect::<Vec<_>>();
        for provider in &providers {
            drop(provider.commits.lock().await);
            self.disconnect(&provider.registration.provider_id).await;
        }
        let mut state = lock(&self.state);
        for provider_id in &provider_ids {
            state.providers.remove(provider_id);
        }
        Ok(provider_ids)
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

    /// The account-bound profile source shared by every third-party registration.
    pub fn profiles(&self) -> Arc<PluginProfiles> {
        self.profiles.clone()
    }

    /// Stops every third-party session after an account profile change. The profile epoch has
    /// already advanced, so their contexts are stale; waiting for each commit guard lets a commit
    /// that passed its check finish in the previous profile before the session goes away.
    /// Official providers are not account-profile scoped and keep running.
    pub async fn fence_third_party_sessions(&self) {
        let providers: Vec<_> = lock(&self.state)
            .providers
            .values()
            .filter(|provider| provider.profiles.is_some())
            .cloned()
            .collect();
        for provider in providers {
            drop(provider.commits.lock().await);
            self.disconnect(&provider.registration.provider_id).await;
        }
    }

    /// Follows account profile changes for the lifetime of this API: each change fences and
    /// stops third-party sessions. Call once from the host runtime.
    pub fn spawn_profile_fencing(&self, runtime: &tokio::runtime::Handle) {
        let api = self.clone();
        let mut changes = self.profiles.subscribe();
        runtime.spawn(async move {
            let mut epoch = api.profiles.epoch();
            while changes.changed().await.is_ok() {
                let current = api.profiles.epoch();
                if current != epoch {
                    epoch = current;
                    api.fence_third_party_sessions().await;
                }
            }
        });
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

    /// Delivers the validation error that ended a provider event stream to the operation it
    /// belongs to, instead of reporting only that the process disconnected.
    fn authorize(
        &self,
        context: &ServiceContext,
        permission: &'static str,
    ) -> Result<Arc<Provider>, Status> {
        let state = lock(&self.state);
        let Some(session) = state.sessions.get(&context.session_id) else {
            return Err(Status::unauthenticated("provider session expired"));
        };
        if !super::constant_time_eq(&session.credential, &context.session_credential) {
            let event = denial_event(session, permission, DenialReason::ScopeDenied);
            let revision = event.grant_revision;
            drop(state);
            self.record_authorization_denial(event);
            return Err(denial(DenialReason::ScopeDenied, permission, revision));
        }
        if session.provider.registration.kind == RegistrationKind::Extension {
            return Err(Status::permission_denied(
                "provider operations are unavailable to extensions",
            ));
        }
        // Without an account profile, profile-scoped services are unavailable, not denied.
        if session
            .provider
            .profiles
            .as_ref()
            .is_some_and(|profiles| profiles.current().is_none())
        {
            return Err(super::profile::no_profile());
        }
        if context.session_kind != session.kind || context.target_game != session.target_game {
            let event = denial_event(session, permission, DenialReason::ScopeDenied);
            let revision = event.grant_revision;
            drop(state);
            self.record_authorization_denial(event);
            return Err(denial(DenialReason::ScopeDenied, permission, revision));
        }
        if context.provider_id != session.provider.registration.provider_id {
            let event = denial_event(session, permission, DenialReason::ScopeDenied);
            let revision = event.grant_revision;
            drop(state);
            self.record_authorization_denial(event);
            return Err(denial(DenialReason::ScopeDenied, permission, revision));
        }
        if !session.grants.contains(permission) {
            let event = denial_event(session, permission, DenialReason::NotGranted);
            let revision = event.grant_revision;
            drop(state);
            self.record_authorization_denial(event);
            return Err(denial(DenialReason::NotGranted, permission, revision));
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
            return Err(denial(DenialReason::NotGranted, permission, revision));
        }
        Ok(session.provider.clone())
    }

    /// Authorizes a plugin-initiated resource call. The context must be the session's resource
    /// context from `OpenSession`, for the active profile; it adds no authority, so `permission`
    /// must still be one of the session's grants. A missing account profile makes the call
    /// unavailable rather than denied.
    pub(crate) fn authorize_resource(
        &self,
        context: Option<&ServiceContext>,
        permission: &'static str,
    ) -> Result<ResourceCaller, Status> {
        self.resource_caller(context, Some(permission))
    }

    /// [`authorize_resource`](Self::authorize_resource) without naming a permission: the session,
    /// credential, profile and operation checks only. A call whose permission is only known after
    /// finding the record it touches (a job's) authorizes here, looks the record up under the
    /// caller's own identity, and then calls `authorize_resource` with the record's permission.
    pub(crate) fn authorize_resource_session(
        &self,
        context: Option<&ServiceContext>,
    ) -> Result<ResourceCaller, Status> {
        self.resource_caller(context, None)
    }

    fn resource_caller(
        &self,
        context: Option<&ServiceContext>,
        permission: Option<&'static str>,
    ) -> Result<ResourceCaller, Status> {
        // What a denial event names when no permission is being asked for.
        let label = permission.unwrap_or("resource.session");
        let context =
            context.ok_or_else(|| Status::unauthenticated("resource context required"))?;
        let state = lock(&self.state);
        let Some(session) = state.sessions.get(&context.session_id) else {
            return Err(Status::unauthenticated("plugin session expired"));
        };
        let scope_denied = |session: &Session| {
            let event = denial_event(session, label, DenialReason::ScopeDenied);
            let revision = event.grant_revision;
            (event, denial(DenialReason::ScopeDenied, label, revision))
        };
        if !super::constant_time_eq(&session.credential, &context.session_credential)
            || context.session_kind != session.kind
            || context.target_game != session.target_game
            || context.provider_id != context_provider_id(&session.provider.registration)
        {
            let (event, status) = scope_denied(session);
            drop(state);
            self.record_authorization_denial(event);
            return Err(status);
        }
        if session
            .provider
            .profiles
            .as_ref()
            .is_some_and(|profiles| profiles.current().is_none())
        {
            return Err(super::profile::no_profile());
        }
        let Some(operation) = session.resource_operation.as_deref() else {
            return Err(Status::failed_precondition(
                "resource services need API 1.6 or later",
            ));
        };
        let (_, profile_epoch) = epoch(&session.provider);
        if !super::constant_time_eq(operation.as_bytes(), context.operation_id.as_bytes())
            || context.profile_epoch != profile_epoch
        {
            return Err(Status::failed_precondition("resource context is stale"));
        }
        if let Some(permission) = permission
            && !session.grants.contains(permission)
        {
            let event = denial_event(session, permission, DenialReason::NotGranted);
            let revision = event.grant_revision;
            drop(state);
            self.record_authorization_denial(event);
            return Err(denial(DenialReason::NotGranted, permission, revision));
        }
        let namespace = match (
            &session.provider.registration.origin,
            session.provider.profiles.as_ref(),
        ) {
            (
                RegistrationOrigin::ThirdParty {
                    publisher_fingerprint,
                },
                Some(profiles),
            ) => profiles
                .resolver(
                    &session.provider.registration.plugin_id,
                    publisher_fingerprint,
                    (session.provider.registration.kind == RegistrationKind::Provider)
                        .then_some(session.provider.registration.provider_id.as_str()),
                )
                .ok(),
            _ => None,
        };
        Ok(ResourceCaller {
            session_id: context.session_id.clone(),
            provider_id: session.provider.registration.provider_id.clone(),
            grants: session.grants.clone(),
            grant_revision: session.grant_revision,
            profile_epoch,
            namespace,
        })
    }

    /// The denial for a resource call whose permission is held but whose *scope* is not: it is
    /// recorded like any authorization denial and carries the machine-readable detail.
    pub(crate) fn deny_resource_scope(
        &self,
        caller: &ResourceCaller,
        permission: &'static str,
    ) -> Status {
        let event = {
            let state = lock(&self.state);
            let Some(session) = state.sessions.get(&caller.session_id) else {
                return Status::unauthenticated("plugin session expired");
            };
            denial_event(session, permission, DenialReason::ScopeDenied)
        };
        let revision = event.grant_revision;
        self.record_authorization_denial(event);
        denial(DenialReason::ScopeDenied, permission, revision)
    }

    /// Host-only original contexts for lifecycle consumers. Never refresh an old
    /// session's profile epoch, and never return these credentials through a public RPC.
    pub(crate) fn resource_session_contexts(&self) -> Vec<ServiceContext> {
        lock(&self.state)
            .sessions
            .values()
            .filter(|session| {
                matches!(
                    session.provider.registration.origin,
                    RegistrationOrigin::ThirdParty { .. }
                )
            })
            .filter_map(|session| session.resource_context.clone())
            .collect()
    }

    /// Whether the plugin session `session_id` is still open.
    pub(crate) fn session_is_open(&self, session_id: &str) -> bool {
        lock(&self.state).sessions.contains_key(session_id)
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
        self.execute_payload(provider_id, kind, None, None).await
    }

    pub async fn execute_achievements(
        &self,
        provider_id: &str,
        request: models::AchievementsRequest,
    ) -> Result<Completion, Status> {
        gamenode_plugin_protocol::achievements::validate_request(&request)
            .map_err(|_| Status::invalid_argument("invalid achievements request"))?;
        self.execute_payload(
            provider_id,
            ProviderOperation::ReadAchievements,
            None,
            Some(request),
        )
        .await
    }

    pub async fn execute_settings(&self, request: settings::Request) -> Result<Completion, Status> {
        request
            .validate()
            .map_err(|_| Status::invalid_argument("invalid settings request"))?;
        self.execute_payload(
            &request.context.provider_id.clone(),
            request.operation(),
            Some(request),
            None,
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
        if !cap::KNOWN.contains(&permission) {
            return;
        }
        let sessions: Vec<_> = lock(&self.state)
            .sessions
            .iter()
            .filter(|(_, session)| session.provider.registration.provider_id == provider_id)
            .map(|(id, session)| (id.clone(), session.grants.clone()))
            .collect();
        for (session_id, mut grants) in sessions {
            if grants.remove(permission) {
                let _ = self.replace_session_grants(&session_id, grants).await;
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
        achievements: Option<models::AchievementsRequest>,
    ) -> Result<Completion, Status> {
        if is_settings(kind) != settings.is_some()
            || (kind == ProviderOperation::ReadAchievements) != achievements.is_some()
        {
            return Err(Status::invalid_argument("missing operation scope"));
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
                ProviderOperation::ReadAchievements => cap::ACHIEVEMENTS,
                ProviderOperation::SignIn
                | ProviderOperation::FocusSignIn
                | ProviderOperation::CancelSignIn => cap::LOGIN,
                ProviderOperation::Check | ProviderOperation::SignOut => cap::ACCOUNT_PUBLISH,
                _ => cap::CONTEXT,
            };
            if !grants.contains(required) {
                let event = denial_event(session, required, DenialReason::NotGranted);
                let revision = event.grant_revision;
                drop(state);
                self.record_authorization_denial(event);
                return Err(denial(DenialReason::NotGranted, required, revision));
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
                    achievements: Vec::new(),
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
                    requested_achievements: achievements
                        .as_ref()
                        .map(|request| request.game_ids.iter().cloned().collect())
                        .unwrap_or_default(),
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
                achievements: achievements.map(Into::into),
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
            Some(provider_event::Event::RuntimeSession(_)) => cap::GAME_SESSIONS_REPORT,
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
                    // The temporary lock ends with this statement; the denial locks state again.
                    let undeleted_sign_out = lock(&self.state)
                        .operations
                        .get(&(context.session_id.clone(), context.operation_id.clone()))
                        .is_some_and(|operation| {
                            operation.kind == ProviderOperation::SignOut
                                && !operation.secret_deleted
                        });
                    if undeleted_sign_out {
                        return Err(self.scope_denial(&context, cap::ACCOUNT_PUBLISH).unwrap());
                    }
                }
                let status = status
                    .try_into()
                    .map_err(|_| Status::invalid_argument("invalid account status"))?;
                provider.registration.services.publish_status(status);
            }
            provider_event::Event::CatalogChunk(chunk) => {
                if matches!(
                    CatalogCategory::try_from(chunk.category),
                    Ok(CatalogCategory::Shared)
                ) && !provider.registration.shared_catalog
                {
                    return Err(Status::invalid_argument(
                        "shared catalog data requires declared shared access",
                    ));
                }
                lock(&self.state)
                    .operations
                    .get_mut(&(context.session_id, context.operation_id))
                    .ok_or_else(|| Status::failed_precondition("stale operation"))?
                    .catalog
                    .push(chunk)
                    .map_err(|_| Status::invalid_argument("invalid catalog transfer"))?;
            }
            provider_event::Event::RuntimeSession(_) => {
                return Err(Status::unimplemented(
                    "runtime session reporting is not available in this host version",
                ));
            }
            provider_event::Event::Result(result) => {
                for (present, permission) in [
                    (result.status.is_some(), cap::ACCOUNT_PUBLISH),
                    (result.discovery.is_some(), cap::DISCOVERY),
                    (!result.counters.is_empty(), cap::COUNTER),
                    (!result.catalogs.is_empty(), cap::CATALOG),
                    (!result.achievements.is_empty(), cap::ACHIEVEMENTS),
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
                    return Err(self.scope_denial(&context, cap::LOGIN).unwrap());
                }
                if !result.achievements.is_empty()
                    && operation_kind != ProviderOperation::ReadAchievements
                {
                    return Err(self.scope_denial(&context, cap::ACHIEVEMENTS).unwrap());
                }
                if result.counters.len() > 50_000
                    || result.counters.iter().any(|c| {
                        c.id.is_empty()
                            || c.id.len() > 256
                            || c.seconds < 0
                            || c.last_played.is_some_and(|last_played| last_played < 0)
                    })
                {
                    return Err(Status::invalid_argument("invalid counter observations"));
                }
                if let Some(discovery) = &result.discovery
                    && (discovery.provider_id != context.provider_id
                        || discovery.records.len() > 50_000)
                {
                    return Err(self.scope_denial(&context, cap::DISCOVERY).unwrap());
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
                    let error = self
                        .scope_denial(&context, cap::SECRET_DELETE)
                        .unwrap_or_else(|| Status::unauthenticated("unknown session"));
                    let _ = operation.result.send(Err(error.clone()));
                    return Err(error);
                }
                if result.signed_out && operation.kind != ProviderOperation::SignOut {
                    let error = self
                        .scope_denial(&context, cap::ACCOUNT_PUBLISH)
                        .unwrap_or_else(|| Status::unauthenticated("unknown session"));
                    let _ = operation.result.send(Err(error.clone()));
                    return Err(error);
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
                if catalog.shared.is_some() && !provider.registration.shared_catalog {
                    let _ = operation.result.send(Err(Status::invalid_argument(
                        "shared catalog data requires declared shared access",
                    )));
                    return Err(Status::invalid_argument(
                        "shared catalog data requires declared shared access",
                    ));
                }
                let achievements = if operation.kind == ProviderOperation::ReadAchievements {
                    match gamenode_plugin_protocol::achievements::parse_outcomes(
                        &result.achievements,
                        &operation.requested_achievements,
                        disposition == OperationDisposition::Completed,
                    ) {
                        Ok(outcomes) => outcomes,
                        Err(_) => {
                            let _ = operation
                                .result
                                .send(Err(Status::invalid_argument("invalid achievements result")));
                            return Err(Status::invalid_argument("invalid achievements result"));
                        }
                    }
                } else {
                    Vec::new()
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
                    achievements,
                    guard: Some(guard),
                }));
                if let Some(commands) = commands {
                    let _ = commands.try_send(Ok(ProviderCommand {
                        context: Some(context),
                        operation: ProviderOperation::Acknowledge as i32,
                        target_operation_id: String::new(),
                        settings: None,
                        runtime_session_result: None,
                        achievements: None,
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
            if !super::constant_time_eq(&session.credential, &context.session_credential) {
                return Err(Status::unauthenticated("invalid stream credential"));
            }
            if context_provider_id(&session.provider.registration) != context.provider_id {
                let permission = cap::CONTEXT;
                let detail = denial(
                    DenialReason::ScopeDenied,
                    permission,
                    session.grant_revision,
                );
                let event = denial_event(session, permission, DenialReason::ScopeDenied);
                drop(state);
                self.record_authorization_denial(event);
                return Err(detail);
            }
            if session.commands.is_some() {
                return Err(Status::failed_precondition(
                    "provider stream already connected",
                ));
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
                        .send(Err(Status::unauthenticated(
                            "event belongs to another stream session",
                        )))
                        .await;
                    break;
                }
                let event_context = event.context.clone();
                if let Err(error) = api.event(event).await {
                    // Obsolete/duplicate results are ignored and cannot tear down
                    // newer work on this same persistent provider connection.
                    if error.code() == tonic::Code::FailedPrecondition {
                        continue;
                    }
                    if let Some(context) = event_context.as_ref() {
                        api.reject_stream_operation(context, error.clone());
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
            // Release the state lock before denying: recording the denial locks it again.
            let deleted_on_sign_out = {
                let state = lock(&self.state);
                let operation = state
                    .operations
                    .get(&(
                        lease.context.session_id.clone(),
                        lease.context.operation_id.clone(),
                    ))
                    .ok_or_else(|| Status::failed_precondition("stale account operation"))?;
                operation.kind == ProviderOperation::SignOut && operation.secret_deleted
            };
            if !deleted_on_sign_out {
                return Err(lease.deny_scope_for(cap::SECRET_DELETE));
            }
        }
        Ok(Response::new(services.account(lease, action).await?))
    }

    async fn secret(
        &self,
        request: Request<SecretRequest>,
    ) -> Result<Response<SecretReply>, Status> {
        let request = request.into_inner();
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
        if request.key != "session" {
            let namespace = lease
                .namespace()
                .map_err(|_| lease.deny_scope_for(permission))?;
            namespace
                .secret_slot(&request.key)
                .map_err(|_| Status::invalid_argument("invalid logical secret key"))?;
        }
        if request
            .value
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 32 * 1024)
        {
            return Err(Status::invalid_argument("invalid secret value"));
        }
        Ok(Response::new(services.secret(lease, request).await?))
    }

    async fn login(&self, request: Request<LoginRequest>) -> Result<Response<LoginReply>, Status> {
        let request = request.into_inner();
        let (lease, services) = self.lease(request.context.clone(), cap::LOGIN)?;
        if request.operation == LoginOperation::Read as i32 {
            self.authorize(&lease.context, cap::PAGE)?;
            if services.login_reads_cookies() {
                self.authorize(&lease.context, cap::COOKIES)?;
            }
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
