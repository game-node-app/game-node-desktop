//! The Epic Games Store account service: authorization-code sign-in, refresh-token lifecycle,
//! owned catalog sync and playtime totals.
//!
//! The service owns the non-secret account state (identity, last check/sync, persistence
//! failures) and drives the injected credential, store-account, Epic HTTP and login-window ports.
//! It is deliberately split into synchronous state transitions and injected ports so the
//! runtime can run every blocking or slow call off the UI thread:
//!
//! - the credential and store-account ports block; the service calls them through
//!   [`tokio::task::spawn_blocking`];
//! - Epic requests run on Tokio with bounded timeouts, and the whole catalog sync is bounded by
//!   the host's 300-second `SyncCatalog` budget;
//! - [`EpicAccount::status`] and the action helpers only lock a small in-memory state, so a
//!   command or snapshot can read them cheaply on any thread.
//!
//! Exactly one check or sync runs at a time. A request that arrives while one is in flight joins
//! the running operation and shares its result instead of starting a second request. Every
//! network result is checked against a generation counter, so a result from before a sign-in,
//! sign-out or credential removal never mutates the account.
//!
//! Only the rotating refresh token is persisted. The access token stays in this process's memory
//! for the catalog and playtime reads and is never written anywhere.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::http::{
    EpicCatalogItem, EpicClient, EpicError, EpicLibraryItem, EpicPlaytime, EpicToken,
};
use gamenode_plugin_protocol::models::{
    AccountState, CredentialError, CredentialStore, SessionCredential, StorageError,
};
use gamenode_plugin_protocol::models::{CatalogError, CatalogRecord, StoreAccount};

/// The provider id of the Epic Games Store account.
pub const PROVIDER: &str = "epic";

/// Upper bound for one token or playtime operation. The HTTP client already times out every
/// request at 20 seconds; this is the service-level guard for a stalled worker.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Revoking a session is best effort and must not delay sign-out for long.
const REVOKE_TIMEOUT: Duration = Duration::from_secs(5);
/// The host aborts a catalog sync after 300 seconds. Finish slightly earlier so the plugin can
/// report a categorized failure instead of being killed by the host.
pub const SYNC_BUDGET: Duration = Duration::from_secs(290);
/// At most this many catalog title requests run at once.
const MAX_TITLE_REQUESTS: usize = 8;

// Public account values are the same contract used by independent hosts.
pub use gamenode_plugin_protocol::models::{
    AccountState as EpicAccountState, AccountStatus as EpicAccountStatus,
    CatalogOutcome as EpicCatalogOutcome, CheckOutcome as EpicCheckOutcome,
    SignInOutcome as EpicSignInOutcome, SignInResult as EpicSignInResult,
    SignOutOutcome as EpicSignOutOutcome, StoreAccountStore,
};

/// One playtime total reported under the application name that keys the library entry. Epic's
/// ledger is keyed by artifact id, so the service maps it through the owned library before
/// reporting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpicPlaytimeTotal {
    pub app_name: String,
    pub seconds: i64,
}

/// Reads the wall clock. Injected so tests can assert `lastCheckedAt`.
pub trait Clock: Send + Sync {
    fn now(&self) -> i64;
}

/// The production clock, in unix seconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
            })
    }
}

/// The Epic HTTP operations the account service drives. [`EpicClient`] is the production
/// implementation; tests inject a deterministic fake.
#[async_trait::async_trait]
pub trait EpicApi: Send + Sync {
    async fn exchange_code(&self, code: &SessionCredential) -> Result<EpicToken, EpicError>;
    async fn refresh(&self, refresh_token: &SessionCredential) -> Result<EpicToken, EpicError>;
    async fn revoke(&self, access_token: &SessionCredential) -> Result<(), EpicError>;
    async fn library_items(
        &self,
        access_token: &SessionCredential,
    ) -> Result<Vec<EpicLibraryItem>, EpicError>;
    async fn catalog_items(
        &self,
        access_token: &SessionCredential,
        namespace: &str,
        ids: &[String],
    ) -> Result<BTreeMap<String, EpicCatalogItem>, EpicError>;
    async fn playtime(
        &self,
        access_token: &SessionCredential,
        account_id: &str,
    ) -> Result<Vec<EpicPlaytime>, EpicError>;
}

#[async_trait::async_trait]
impl EpicApi for EpicClient {
    async fn exchange_code(&self, code: &SessionCredential) -> Result<EpicToken, EpicError> {
        EpicClient::exchange_code(self, code).await
    }

    async fn refresh(&self, refresh_token: &SessionCredential) -> Result<EpicToken, EpicError> {
        EpicClient::refresh(self, refresh_token).await
    }

    async fn revoke(&self, access_token: &SessionCredential) -> Result<(), EpicError> {
        EpicClient::revoke(self, access_token).await
    }

    async fn library_items(
        &self,
        access_token: &SessionCredential,
    ) -> Result<Vec<EpicLibraryItem>, EpicError> {
        EpicClient::library_items(self, access_token).await
    }

    async fn catalog_items(
        &self,
        access_token: &SessionCredential,
        namespace: &str,
        ids: &[String],
    ) -> Result<BTreeMap<String, EpicCatalogItem>, EpicError> {
        EpicClient::catalog_items(self, access_token, namespace, ids).await
    }

    async fn playtime(
        &self,
        access_token: &SessionCredential,
        account_id: &str,
    ) -> Result<Vec<EpicPlaytime>, EpicError> {
        EpicClient::playtime(self, access_token, account_id).await
    }
}

/// How a sign-in attempt ended. The authorization code is only present when the approved page
/// probe delivered one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EpicLoginResult {
    Code(SessionCredential),
    Cancelled,
    Closed,
    TimedOut,
    Failed,
}

/// One attempt's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpicLoginOutcome {
    pub result: EpicLoginResult,
}

/// The sign-in window the account service drives.
#[async_trait::async_trait]
pub trait EpicLoginWindow: Send + Sync {
    async fn run(&self) -> EpicLoginOutcome;
    fn focus(&self) -> bool;
    fn cancel(&self) -> bool;
    /// The host owns ephemeral browser profiles and startup recovery.
    fn cleanup_profile(&self);
}

/// The adapters the account service drives.
#[derive(Clone)]
pub struct EpicAccountPorts {
    /// The provider's own secret slot. Only the refresh token is written here.
    pub credentials: Arc<dyn CredentialStore>,
    pub accounts: Arc<dyn StoreAccountStore>,
    pub api: Arc<dyn EpicApi>,
    pub login: Arc<dyn EpicLoginWindow>,
    pub clock: Arc<dyn Clock>,
    /// The whole-sync deadline. Production uses [`SYNC_BUDGET`]; tests accelerate it.
    pub catalog_budget: Duration,
}

/// Called after every status transition. It must be cheap and must not call back into the
/// service: the runtime publishes a snapshot from the operation's background task.
pub type StatusObserver = Arc<dyn Fn(&EpicAccountStatus) + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationKind {
    Check,
    Sync,
}

struct InFlight<T> {
    result: tokio::sync::watch::Sender<Option<Arc<T>>>,
}

struct OperationOutcome {
    status: EpicAccountStatus,
    superseded: bool,
    account_changed: bool,
    owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    now: i64,
}

/// A successful owned-catalog read. Besides the entries, it carries the artifact-id to
/// application-name mapping the playtime counter needs, because Epic keys its ledger by artifact id
/// while library entries, display and submissions key on the application name.
struct OwnedCatalog {
    records: Vec<CatalogRecord>,
    playtime_names: BTreeMap<String, String>,
}

struct Inner {
    loaded: bool,
    /// The persisted rotating refresh token.
    credential: Option<SessionCredential>,
    /// The in-memory access token. Never persisted.
    access: Option<SessionCredential>,
    state: AccountState,
    account_id: Option<String>,
    account_name: Option<String>,
    last_checked_at: Option<i64>,
    last_synced_at: Option<i64>,
    credential_save_failed: bool,
    account_save_failed: bool,
    catalog_error: Option<CatalogError>,
    /// Epic reports playtime by artifact id while library entries are keyed by application name.
    /// Built from the owned library on a successful sync and kept only for the current account.
    playtime_names: BTreeMap<String, String>,
    /// Bumped by every sign-in start, cancel and sign-out. Network results carry the value they
    /// started with and are ignored when it changed.
    generation: u64,
    /// The state to return to when a sign-in window is cancelled or closed.
    previous_state: Option<AccountState>,
    inflight: Option<InFlight<OperationOutcome>>,
    /// A load is in flight. Startup and a catalog request can race, and a second load would
    /// reset the status the first one or a finished check just committed.
    loading: Option<InFlight<EpicAccountStatus>>,
}

impl Inner {
    fn new() -> Self {
        Self {
            loaded: false,
            credential: None,
            access: None,
            state: AccountState::NotConnected,
            account_id: None,
            account_name: None,
            last_checked_at: None,
            last_synced_at: None,
            credential_save_failed: false,
            account_save_failed: false,
            catalog_error: None,
            playtime_names: BTreeMap::new(),
            generation: 0,
            previous_state: None,
            inflight: None,
            loading: None,
        }
    }

    fn persist_failed(&self) -> bool {
        self.credential_save_failed || self.account_save_failed
    }
}

/// The Epic Games Store account service.
pub struct EpicAccount {
    ports: EpicAccountPorts,
    observer: Mutex<StatusObserver>,
    inner: Arc<Mutex<Inner>>,
    validity: Arc<AtomicBool>,
}

impl EpicAccount {
    /// `observer` runs after every transition, outside the service lock.
    pub fn new(
        ports: EpicAccountPorts,
        observer: impl Fn(&EpicAccountStatus) + Send + Sync + 'static,
    ) -> Self {
        Self {
            ports,
            observer: Mutex::new(Arc::new(observer)),
            inner: Arc::new(Mutex::new(Inner::new())),
            validity: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Shares account state while binding every storage/login call to the immutable host context
    /// for this operation. Never retag an older task.
    pub fn for_operation(&self, ports: EpicAccountPorts, validity: Arc<AtomicBool>) -> Self {
        Self {
            ports,
            observer: Mutex::new(Arc::new(|_| {})),
            inner: self.inner.clone(),
            validity,
        }
    }

    /// Builds the service before the runtime can capture itself in an observer.
    pub fn without_observer(ports: EpicAccountPorts) -> Self {
        Self::new(ports, |_| {})
    }

    /// Replaces the callback that publishes status transitions.
    pub fn set_observer(&self, observer: impl Fn(&EpicAccountStatus) + Send + Sync + 'static) {
        *self
            .observer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(observer);
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The current card state. Cheap and non-blocking; safe for a snapshot on any thread.
    pub fn status(&self) -> EpicAccountStatus {
        let inner = self.lock();
        EpicAccountStatus {
            state: inner.state,
            account_id: inner.account_id.clone(),
            account_name: inner.account_name.clone(),
            last_checked_at: inner.last_checked_at,
            last_synced_at: inner.last_synced_at,
            storage: self.ports.credentials.method(),
            persist_failed: inner.persist_failed(),
            catalog_error: inner.catalog_error,
            // Epic declares no shared catalog access, so it never reports a shared category.
            shared_error: None,
        }
    }

    fn notify(&self) {
        let observer = {
            self.observer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        };
        let status = self.status();
        observer(&status);
    }

    fn superseded(&self, generation: u64) -> bool {
        !self.validity.load(Ordering::Acquire) || self.lock().generation != generation
    }

    /// Takes the state lock only when no sign-in, sign-out or credential removal happened since
    /// the operation started, so a late result cannot overwrite a newer state.
    fn commit_if_current(&self, generation: u64) -> Option<MutexGuard<'_, Inner>> {
        let inner = self.lock();
        (self.validity.load(Ordering::Acquire) && inner.generation == generation).then_some(inner)
    }

    /// Undoes a credential or account write that lost the race against a sign-out. A newer
    /// sign-in owns the slots, so in that case nothing is removed.
    async fn discard_saved_if_signed_out(&self) {
        if self.lock().state == AccountState::SigningIn {
            return;
        }
        let _ = self.delete_credential().await;
        if self.lock().state == AccountState::NotConnected {
            let _ = self.clear_account().await;
        }
    }

    /// Removes a profile left by a crash, then loads the credential and account row. The engine
    /// calls this once at startup from Tokio (the profile and stores are blocking, so the
    /// service calls the profile cleanup on the blocking pool).
    pub async fn startup(&self) -> EpicAccountStatus {
        if self.lock().state == AccountState::SigningIn {
            return self.status();
        }
        let login = self.ports.login.clone();
        let _ = tokio::task::spawn_blocking(move || login.cleanup_profile()).await;
        // Catalog work may have loaded and validated the account before this startup task was
        // scheduled. Do not reload a credential it just removed on expiry.
        let loaded = self.lock().loaded;
        if loaded {
            self.status()
        } else {
            self.load_stored().await
        }
    }

    /// Reloads the credential and account row from their stores. Also used by "Retry" after a
    /// credential error; unlike [`startup`](Self::startup) it does not touch the profile.
    pub async fn load_stored(&self) -> EpicAccountStatus {
        enum Entry {
            Start,
            Join(tokio::sync::watch::Receiver<Option<Arc<EpicAccountStatus>>>),
        }

        let entry = {
            let mut inner = self.lock();
            if inner.state == AccountState::SigningIn {
                return self.status();
            }
            match &inner.loading {
                Some(inflight) => Entry::Join(inflight.result.subscribe()),
                None => {
                    let (sender, _receiver) = tokio::sync::watch::channel(None);
                    inner.loading = Some(InFlight { result: sender });
                    Entry::Start
                }
            }
        };

        match entry {
            Entry::Start => {
                let status = Arc::new(self.perform_load().await);
                let sender = self.lock().loading.take().map(|inflight| inflight.result);
                if let Some(sender) = sender {
                    sender.send_replace(Some(status.clone()));
                }
                (*status).clone()
            }
            Entry::Join(mut receiver) => loop {
                if let Some(status) = receiver.borrow().clone() {
                    return (*status).clone();
                }
                if receiver.changed().await.is_err() {
                    return self.status();
                }
            },
        }
    }

    async fn perform_load(&self) -> EpicAccountStatus {
        let generation = self.lock().generation;
        let credential = self.load_credential().await;
        let account = self.load_account().await;
        if credential.is_err() || account.is_err() {
            tracing::warn!("could not read the saved Epic account");
        }
        {
            let mut inner = self.lock();
            if !self.validity.load(Ordering::Acquire) || inner.generation != generation {
                drop(inner);
                return self.status();
            }
            inner.loaded = true;
            inner.previous_state = None;
            inner.credential_save_failed = false;
            inner.account_save_failed = false;
            inner.catalog_error = None;
            inner.last_synced_at = None;
            inner.access = None;
            // A reload starts from stored state; the previous in-memory library mapping may
            // belong to another account.
            inner.playtime_names.clear();
            match credential {
                Err(_) => {
                    inner.state = AccountState::CredentialError;
                    inner.credential = None;
                    inner.account_id = account
                        .as_ref()
                        .ok()
                        .and_then(|account| account.as_ref())
                        .map(|account| account.account_id.clone());
                    inner.account_name = account
                        .as_ref()
                        .ok()
                        .and_then(|account| account.as_ref())
                        .and_then(|account| account.account_name.clone());
                    inner.last_checked_at = account
                        .as_ref()
                        .ok()
                        .and_then(|account| account.as_ref())
                        .and_then(|account| account.last_verified_at);
                }
                Ok(None) => {
                    inner.state = AccountState::NotConnected;
                    inner.credential = None;
                    inner.account_id = None;
                    inner.account_name = None;
                    inner.last_checked_at = None;
                }
                Ok(Some(credential)) => {
                    inner.state = AccountState::Checking;
                    inner.credential = Some(credential);
                    inner.account_id = account
                        .as_ref()
                        .ok()
                        .and_then(|account| account.as_ref())
                        .map(|account| account.account_id.clone());
                    inner.account_name = account
                        .as_ref()
                        .ok()
                        .and_then(|account| account.as_ref())
                        .and_then(|account| account.account_name.clone());
                    inner.last_checked_at = account
                        .as_ref()
                        .ok()
                        .and_then(|account| account.as_ref())
                        .and_then(|account| account.last_verified_at);
                }
            }
        }
        self.notify();
        let status = self.status();
        tracing::info!(
            state = ?status.state,
            storage = ?status.storage,
            "loaded the saved Epic account"
        );
        status
    }

    async fn ensure_loaded(&self) {
        let reload = {
            let inner = self.lock();
            !inner.loaded || inner.state == AccountState::CredentialError
        };
        if reload {
            self.load_stored().await;
        }
    }

    /// Validates the stored refresh token and updates the connection state. Coalesces with a
    /// running check or sync.
    pub async fn check(&self) -> EpicCheckOutcome {
        self.ensure_loaded().await;
        if self.lock().state == AccountState::CredentialError {
            return EpicCheckOutcome {
                status: self.status(),
                account_changed: false,
                superseded: false,
            };
        }
        let outcome = self.run_or_join(OperationKind::Check).await;
        EpicCheckOutcome {
            status: outcome.status.clone(),
            account_changed: outcome.account_changed,
            superseded: outcome.superseded,
        }
    }

    /// Validates the refresh token first, then fetches the owned catalog. Coalesces with a
    /// running check or sync; a check that is already running yields a result with no catalog.
    pub async fn sync(&self) -> EpicCatalogOutcome {
        self.ensure_loaded().await;
        if self.lock().state == AccountState::CredentialError {
            return EpicCatalogOutcome {
                status: self.status(),
                owned: None,
                shared: None,
                now: self.ports.clock.now(),
                account_changed: false,
                superseded: false,
            };
        }
        let outcome = self.run_or_join(OperationKind::Sync).await;
        EpicCatalogOutcome {
            status: outcome.status.clone(),
            owned: outcome.owned.clone(),
            shared: None,
            now: outcome.now,
            account_changed: outcome.account_changed,
            superseded: outcome.superseded,
        }
    }

    /// Reads Epic's cumulative playtime ledger when the account is connected and this process
    /// holds an access token. Nets each artifact id through the owned library and reports one total
    /// per application name, because library entries, display and submissions key on `appName`.
    /// Artifact ids with no library application name are skipped. Without a connection, returns no
    /// observations instead of zeros.
    pub async fn counter(&self) -> Vec<EpicPlaytimeTotal> {
        let (access, account_id, playtime_names) = {
            let inner = self.lock();
            if inner.state != AccountState::Connected {
                return Vec::new();
            }
            (
                inner.access.clone(),
                inner.account_id.clone(),
                inner.playtime_names.clone(),
            )
        };
        let (Some(access), Some(account_id)) = (access, account_id) else {
            return Vec::new();
        };
        let api = self.ports.api.clone();
        let totals =
            match tokio::time::timeout(REQUEST_TIMEOUT, api.playtime(&access, &account_id)).await {
                Ok(Ok(totals)) => totals,
                Ok(Err(error)) => {
                    tracing::warn!(error = %error, "Epic playtime read did not succeed");
                    return Vec::new();
                }
                Err(_) => {
                    tracing::warn!("Epic playtime read timed out");
                    return Vec::new();
                }
            };
        // Several artifacts can belong to one application; one observation is reported per
        // application name, so their totals add up.
        let mut by_app: BTreeMap<String, i64> = BTreeMap::new();
        for total in totals {
            let Some(app_name) = playtime_names.get(&total.artifact_id) else {
                continue;
            };
            let seconds = by_app.entry(app_name.clone()).or_default();
            *seconds = seconds.saturating_add(total.seconds);
        }
        by_app
            .into_iter()
            .map(|(app_name, seconds)| EpicPlaytimeTotal { app_name, seconds })
            .collect()
    }

    async fn run_or_join(&self, kind: OperationKind) -> Arc<OperationOutcome> {
        enum Entry {
            Start,
            Join(tokio::sync::watch::Receiver<Option<Arc<OperationOutcome>>>),
        }

        let entry = {
            let mut inner = self.lock();
            match &inner.inflight {
                Some(inflight) => Entry::Join(inflight.result.subscribe()),
                None => {
                    let (sender, _receiver) = tokio::sync::watch::channel(None);
                    inner.inflight = Some(InFlight { result: sender });
                    Entry::Start
                }
            }
        };

        match entry {
            Entry::Start => {
                let outcome = Arc::new(match kind {
                    OperationKind::Check => self.perform_check().await,
                    OperationKind::Sync => self.perform_sync().await,
                });
                self.finish_operation(outcome.clone());
                outcome
            }
            Entry::Join(mut receiver) => loop {
                if let Some(outcome) = receiver.borrow().clone() {
                    return outcome;
                }
                if receiver.changed().await.is_err() {
                    return Arc::new(self.orphaned_outcome());
                }
            },
        }
    }

    fn finish_operation(&self, outcome: Arc<OperationOutcome>) {
        let sender = self.lock().inflight.take().map(|inflight| inflight.result);
        if let Some(sender) = sender {
            sender.send_replace(Some(outcome));
        }
    }

    fn orphaned_outcome(&self) -> OperationOutcome {
        OperationOutcome {
            status: self.status(),
            superseded: true,
            account_changed: false,
            owned: None,
            now: self.ports.clock.now(),
        }
    }

    fn outcome(
        &self,
        now: i64,
        account_changed: bool,
        owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
        superseded: bool,
    ) -> OperationOutcome {
        OperationOutcome {
            status: self.status(),
            superseded,
            account_changed,
            owned,
            now,
        }
    }

    fn superseded_outcome(&self) -> OperationOutcome {
        self.outcome(self.ports.clock.now(), false, None, true)
    }

    fn begin_operation(&self) -> Option<(u64, Option<SessionCredential>)> {
        let mut inner = self.lock();
        if inner.state == AccountState::SigningIn {
            return None;
        }
        inner.previous_state = None;
        Some((inner.generation, inner.credential.clone()))
    }

    async fn perform_check(&self) -> OperationOutcome {
        let Some((generation, credential)) = self.begin_operation() else {
            return self.superseded_outcome();
        };
        let Some(credential) = credential else {
            // A concurrent sign-in owns the state now: a finished check must not overwrite it.
            let Some(mut inner) = self.commit_if_current(generation) else {
                return self.superseded_outcome();
            };
            // Expiry removed the rejected token deliberately. A later background request must
            // retain the sign-in-again state.
            if inner.state != AccountState::Expired {
                inner.state = AccountState::NotConnected;
            }
            inner.access = None;
            inner.playtime_names.clear();
            drop(inner);
            self.notify();
            return self.outcome(self.ports.clock.now(), false, None, false);
        };
        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        self.mark_checking(generation);

        let token = match self.refresh(&credential).await {
            Ok(token) => token,
            Err(error) => {
                if self.superseded(generation) {
                    return self.superseded_outcome();
                }
                self.apply_session_failure(generation, error).await;
                if self.superseded(generation) {
                    return self.superseded_outcome();
                }
                return self.outcome(self.ports.clock.now(), false, None, false);
            }
        };

        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        let account_changed = self.account_changed(&token);
        let account_name = self.resolved_account_name(&token);
        // Epic rotates the refresh token on every exchange. Write it before publishing
        // `connected`, so a crash after the transition cannot lose the only valid token.
        let credential_saved = self.save_credential(&token.refresh_token).await;
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.superseded_outcome();
        }
        let now = self.ports.clock.now();
        let account_saved = self
            .save_session_account(&token, account_name.clone(), now)
            .await;
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.superseded_outcome();
        }

        let Some(mut inner) = self.commit_if_current(generation) else {
            self.discard_saved_if_signed_out().await;
            return self.superseded_outcome();
        };
        inner.state = AccountState::Connected;
        inner.credential = Some(token.refresh_token.clone());
        inner.access = Some(token.access_token.clone());
        inner.account_id = Some(token.account_id.clone());
        inner.account_name = account_name;
        inner.last_checked_at = Some(now);
        inner.previous_state = None;
        inner.credential_save_failed = !credential_saved;
        inner.account_save_failed = !account_saved;
        if account_changed {
            // Artifact ids belong to one Epic account's library.
            inner.playtime_names.clear();
        }
        drop(inner);
        self.notify();
        tracing::info!("Epic session check succeeded");
        self.outcome(now, account_changed, None, false)
    }

    async fn perform_sync(&self) -> OperationOutcome {
        let Some((generation, credential)) = self.begin_operation() else {
            return self.superseded_outcome();
        };
        let Some(credential) = credential else {
            let Some(mut inner) = self.commit_if_current(generation) else {
                return self.superseded_outcome();
            };
            if inner.state != AccountState::Expired {
                inner.state = AccountState::NotConnected;
            }
            inner.access = None;
            inner.playtime_names.clear();
            drop(inner);
            self.notify();
            return self.outcome(self.ports.clock.now(), false, None, false);
        };
        // The host aborts `SyncCatalog` at 300 seconds. Bound the whole credential validation
        // and catalog read so this service reports a category first.
        match tokio::time::timeout(
            self.ports.catalog_budget,
            self.perform_sync_inner(generation, credential),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => self.sync_budget_expired(generation),
        }
    }

    async fn perform_sync_inner(
        &self,
        generation: u64,
        credential: SessionCredential,
    ) -> OperationOutcome {
        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        self.mark_checking(generation);

        let token = match self.refresh(&credential).await {
            Ok(token) => token,
            Err(error) => {
                if self.superseded(generation) {
                    return self.superseded_outcome();
                }
                let category = session_catalog_error(error);
                self.apply_session_failure(generation, error).await;
                if self.superseded(generation) {
                    return self.superseded_outcome();
                }
                return self.outcome(self.ports.clock.now(), false, Some(Err(category)), false);
            }
        };

        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        let account_changed = self.account_changed(&token);
        let account_name = self.resolved_account_name(&token);
        let credential_saved = self.save_credential(&token.refresh_token).await;
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.superseded_outcome();
        }
        let now = self.ports.clock.now();
        let account_saved = self
            .save_session_account(&token, account_name.clone(), now)
            .await;
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.superseded_outcome();
        }
        {
            let Some(mut inner) = self.commit_if_current(generation) else {
                self.discard_saved_if_signed_out().await;
                return self.superseded_outcome();
            };
            inner.state = AccountState::Connected;
            inner.credential = Some(token.refresh_token.clone());
            inner.access = Some(token.access_token.clone());
            inner.account_id = Some(token.account_id.clone());
            inner.account_name = account_name;
            inner.last_checked_at = Some(now);
            inner.previous_state = None;
            inner.credential_save_failed = !credential_saved;
            inner.account_save_failed = !account_saved;
            if account_changed {
                // Artifact ids belong to one Epic account's library.
                inner.playtime_names.clear();
            }
        }
        self.notify();

        let owned = self.fetch_owned(&token.access_token).await;
        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        let now = self.ports.clock.now();
        let expired = matches!(
            &owned,
            Err(CatalogError::Expired | CatalogError::InvalidCredential)
        );
        let unreachable = matches!(
            &owned,
            Err(CatalogError::Unreachable
                | CatalogError::RateLimited
                | CatalogError::Server
                | CatalogError::Changed)
        );
        // A failed read keeps the last successful mapping: it still names the same account's
        // artifacts, and a later successful check can use it without another sync.
        let playtime_names = owned
            .as_ref()
            .ok()
            .map(|catalog| catalog.playtime_names.clone());
        let records = owned.map(|catalog| catalog.records);
        let delete_failed = if expired {
            Some(!self.delete_credential().await)
        } else {
            None
        };
        if self.superseded(generation) {
            return self.superseded_outcome();
        }

        let Some(mut inner) = self.commit_if_current(generation) else {
            return self.superseded_outcome();
        };
        inner.previous_state = None;
        inner.state = if expired {
            AccountState::Expired
        } else if unreachable {
            AccountState::Unreachable
        } else {
            AccountState::Connected
        };
        if expired {
            inner.credential = None;
            inner.access = None;
            inner.playtime_names.clear();
            if delete_failed == Some(true) {
                inner.credential_save_failed = true;
            }
        }
        if let Some(playtime_names) = playtime_names {
            inner.playtime_names = playtime_names;
        }
        inner.catalog_error = records.as_ref().err().copied();
        if records.is_ok() {
            inner.last_synced_at = Some(now);
        }
        drop(inner);
        self.notify();
        tracing::info!(
            state = ?self.status().state,
            owned = records.as_ref().ok().map(Vec::len),
            owned_error = ?records.as_ref().err(),
            "Epic catalog sync finished"
        );
        self.outcome(now, account_changed, Some(records), false)
    }

    fn mark_checking(&self, generation: u64) {
        let mut inner = self.lock();
        if self.validity.load(Ordering::Acquire)
            && inner.generation == generation
            && inner.state != AccountState::SigningIn
        {
            inner.state = AccountState::Checking;
            drop(inner);
            self.notify();
        }
    }

    fn sync_budget_expired(&self, generation: u64) -> OperationOutcome {
        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        let Some(mut inner) = self.commit_if_current(generation) else {
            return self.superseded_outcome();
        };
        // The credential is kept; only this run's catalog read is reported as unreachable.
        inner.state = AccountState::Unreachable;
        inner.catalog_error = Some(CatalogError::Unreachable);
        inner.access = None;
        drop(inner);
        self.notify();
        tracing::warn!("Epic catalog sync exceeded the provider budget");
        self.outcome(
            self.ports.clock.now(),
            false,
            Some(Err(CatalogError::Unreachable)),
            false,
        )
    }

    /// Starts the Epic sign-in window. Only one attempt runs at a time; a second call while the
    /// window is open returns the current status without opening another window.
    pub async fn sign_in(&self) -> EpicSignInOutcome {
        self.ensure_loaded().await;
        if !self.validity.load(Ordering::Acquire) {
            return self.sign_in_superseded();
        }
        let generation = {
            let mut inner = self.lock();
            if inner.state == AccountState::SigningIn {
                None
            } else {
                inner.generation = inner.generation.wrapping_add(1);
                let generation = inner.generation;
                inner.previous_state = Some(inner.state);
                inner.state = AccountState::SigningIn;
                Some(generation)
            }
        };
        let Some(generation) = generation else {
            return self.sign_in_superseded();
        };
        self.notify();
        tracing::info!("opening the Epic sign-in window");

        let outcome = self.ports.login.run().await;
        if self.superseded(generation) {
            return self.sign_in_superseded();
        }
        let outcome = match outcome.result {
            // The approved page probe resolves only after the user completes Epic's login,
            // including any two-factor step. The plugin only ever sees the one-time code.
            EpicLoginResult::Code(code) => self.complete_code_sign_in(generation, code).await,
            EpicLoginResult::Cancelled => {
                self.restore_previous_state(generation, EpicSignInResult::Cancelled)
            }
            EpicLoginResult::Closed => {
                self.restore_previous_state(generation, EpicSignInResult::Closed)
            }
            EpicLoginResult::TimedOut => {
                self.restore_previous_state(generation, EpicSignInResult::TimedOut)
            }
            EpicLoginResult::Failed => {
                self.restore_previous_state(generation, EpicSignInResult::Failed)
            }
        };
        tracing::info!(
            result = ?outcome.result,
            state = ?outcome.status.state,
            "Epic sign-in finished"
        );
        outcome
    }

    async fn complete_code_sign_in(
        &self,
        generation: u64,
        code: SessionCredential,
    ) -> EpicSignInOutcome {
        let token = match self.exchange_code(&code).await {
            Ok(token) => token,
            Err(error) => {
                // The code is dropped here and never logged. No credential is saved.
                tracing::warn!(error = %error, "Epic rejected the authorization code");
                return self.restore_previous_state(generation, EpicSignInResult::Failed);
            }
        };
        if self.superseded(generation) {
            return self.sign_in_superseded();
        }
        let account_changed = self.account_changed(&token);
        let account_name = self.resolved_account_name(&token);
        let saved = self.save_credential(&token.refresh_token).await;
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.sign_in_superseded();
        }
        let now = self.ports.clock.now();
        let account_saved = self
            .save_session_account(&token, account_name.clone(), now)
            .await;
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.sign_in_superseded();
        }
        let Some(mut inner) = self.commit_if_current(generation) else {
            self.discard_saved_if_signed_out().await;
            return self.sign_in_superseded();
        };
        inner.state = AccountState::Connected;
        inner.credential = Some(token.refresh_token.clone());
        inner.access = Some(token.access_token.clone());
        inner.account_id = Some(token.account_id.clone());
        inner.account_name = account_name;
        inner.last_checked_at = Some(now);
        inner.previous_state = None;
        inner.credential_save_failed = !saved;
        inner.account_save_failed = !account_saved;
        if account_changed {
            // Artifact ids belong to one Epic account's library.
            inner.playtime_names.clear();
        }
        drop(inner);
        self.notify();
        EpicSignInOutcome {
            status: self.status(),
            result: EpicSignInResult::Connected,
            account_changed,
        }
    }

    fn restore_previous_state(
        &self,
        generation: u64,
        result: EpicSignInResult,
    ) -> EpicSignInOutcome {
        let restored = {
            let mut inner = self.lock();
            if !self.validity.load(Ordering::Acquire) || inner.generation != generation {
                false
            } else {
                inner.state = inner
                    .previous_state
                    .take()
                    .unwrap_or(AccountState::NotConnected);
                true
            }
        };
        if restored {
            self.notify();
        }
        EpicSignInOutcome {
            status: self.status(),
            result: if restored {
                result
            } else {
                EpicSignInResult::Superseded
            },
            account_changed: false,
        }
    }

    fn sign_in_superseded(&self) -> EpicSignInOutcome {
        EpicSignInOutcome {
            status: self.status(),
            result: EpicSignInResult::Superseded,
            account_changed: false,
        }
    }

    /// Cancels an open sign-in window and returns to the state before the attempt. No credential
    /// is saved. A no-op when no window is open.
    pub fn cancel_sign_in(&self) -> EpicAccountStatus {
        if !self.validity.load(Ordering::Acquire) {
            return self.status();
        }
        let cancelled = {
            let mut inner = self.lock();
            if inner.state == AccountState::SigningIn {
                inner.generation = inner.generation.wrapping_add(1);
                inner.state = inner
                    .previous_state
                    .take()
                    .unwrap_or(AccountState::NotConnected);
                true
            } else {
                false
            }
        };
        if cancelled {
            let _ = self.ports.login.cancel();
            self.notify();
        }
        self.status()
    }

    /// Brings the open sign-in window to the front. `false` when no window is open.
    pub fn focus_sign_in(&self) -> bool {
        if !self.validity.load(Ordering::Acquire) {
            return false;
        }
        if self.lock().state != AccountState::SigningIn {
            return false;
        }
        self.ports.login.focus()
    }

    /// Deletes the refresh token and clears the account, then best-effort revokes the in-memory
    /// OAuth session. A failed deletion leaves the account connected for this run and reports
    /// `persistFailed`; the caller must not claim a restart will stay signed out.
    pub async fn sign_out(&self) -> EpicSignOutOutcome {
        if !self.validity.load(Ordering::Acquire) {
            return EpicSignOutOutcome {
                status: self.status(),
                signed_out: false,
            };
        }
        let (previous_state, generation, access) = {
            let mut inner = self.lock();
            inner.generation = inner.generation.wrapping_add(1);
            (
                inner.previous_state.take().unwrap_or(inner.state),
                inner.generation,
                inner.access.clone(),
            )
        };
        let login = self.ports.login.clone();
        let _ = tokio::task::spawn_blocking(move || login.cancel()).await;
        let deleted = self.delete_credential().await;
        if self.superseded(generation) {
            return EpicSignOutOutcome {
                status: self.status(),
                signed_out: false,
            };
        }
        if !deleted {
            let mut inner = self.lock();
            inner.credential_save_failed = true;
            if matches!(
                inner.state,
                AccountState::SigningIn | AccountState::Checking
            ) {
                inner.state = if inner.credential.is_some() {
                    AccountState::Connected
                } else {
                    previous_state
                };
            }
            drop(inner);
            self.notify();
            return EpicSignOutOutcome {
                status: self.status(),
                signed_out: false,
            };
        }

        let cleared = self.clear_account().await;
        if self.superseded(generation) {
            return EpicSignOutOutcome {
                status: self.status(),
                signed_out: false,
            };
        }
        {
            let mut inner = self.lock();
            inner.credential = None;
            inner.access = None;
            inner.state = AccountState::NotConnected;
            inner.account_id = None;
            inner.account_name = None;
            inner.last_checked_at = None;
            inner.last_synced_at = None;
            inner.catalog_error = None;
            inner.previous_state = None;
            inner.credential_save_failed = false;
            inner.account_save_failed = !cleared;
            inner.playtime_names.clear();
        }
        self.notify();
        // The local credential is already gone; a revoke failure changes nothing user-visible.
        if let Some(access) = access {
            self.revoke(&access).await;
        }
        EpicSignOutOutcome {
            status: self.status(),
            signed_out: true,
        }
    }

    /// Removes a credential that could not be read, recovering from `credentialError`.
    pub async fn remove_credential(&self) -> EpicSignOutOutcome {
        self.sign_out().await
    }

    async fn refresh(&self, credential: &SessionCredential) -> Result<EpicToken, EpicError> {
        let api = self.ports.api.clone();
        let credential = credential.clone();
        match tokio::time::timeout(REQUEST_TIMEOUT, api.refresh(&credential)).await {
            Ok(result) => result,
            Err(_) => Err(EpicError::Unreachable),
        }
    }

    async fn exchange_code(&self, code: &SessionCredential) -> Result<EpicToken, EpicError> {
        let api = self.ports.api.clone();
        let code = code.clone();
        match tokio::time::timeout(REQUEST_TIMEOUT, api.exchange_code(&code)).await {
            Ok(result) => result,
            Err(_) => Err(EpicError::Unreachable),
        }
    }

    async fn revoke(&self, access: &SessionCredential) {
        let api = self.ports.api.clone();
        let access = access.clone();
        match tokio::time::timeout(REVOKE_TIMEOUT, api.revoke(&access)).await {
            Ok(Ok(())) | Err(_) => {}
            Ok(Err(error)) => {
                tracing::info!(error = %error, "Epic session revoke did not succeed");
            }
        }
    }

    async fn fetch_owned(
        &self,
        access_token: &SessionCredential,
    ) -> Result<OwnedCatalog, CatalogError> {
        let api = self.ports.api.clone();
        let token = access_token.clone();
        let items = api.library_items(&token).await.map_err(catalog_error)?;

        // Only records with a launchable application name and a catalog namespace can become
        // entry keys and lookup uids. Engine items never enter the library.
        let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut addressable = Vec::new();
        let mut playtime_names = BTreeMap::new();
        for item in items {
            let Some(app_name) = item.app_name.filter(|name| !name.is_empty()) else {
                continue;
            };
            if item
                .namespace
                .as_deref()
                .is_some_and(|namespace| namespace.eq_ignore_ascii_case("ue"))
            {
                continue;
            }
            // Epic's ledger is keyed by artifact id. Keep the application name each artifact
            // belongs to, including records that the catalog later excludes, so their totals never
            // surface under a raw artifact id.
            if let Some(artifact_id) = item
                .artifact_id
                .filter(|artifact_id| !artifact_id.is_empty())
            {
                playtime_names
                    .entry(artifact_id)
                    .or_insert_with(|| app_name.clone());
            }
            let Some(namespace) = item.namespace.filter(|namespace| !namespace.is_empty()) else {
                continue;
            };
            let Some(catalog_item_id) = item
                .catalog_item_id
                .filter(|catalog_item_id| !catalog_item_id.is_empty())
            else {
                continue;
            };
            groups
                .entry(namespace.clone())
                .or_default()
                .insert(catalog_item_id.clone());
            addressable.push((app_name, namespace, catalog_item_id));
        }

        let mut results = tokio::task::JoinSet::new();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_TITLE_REQUESTS));
        for (namespace, ids) in groups {
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| CatalogError::Unreachable)?;
            let api = api.clone();
            let token = token.clone();
            results.spawn(async move {
                let _permit = permit;
                fetch_titles(api, token, namespace, ids.into_iter().collect()).await
            });
        }
        let mut resolved: BTreeMap<String, BTreeMap<String, EpicCatalogItem>> = BTreeMap::new();
        while let Some(joined) = results.join_next().await {
            match joined {
                Ok(Ok((namespace, items))) => {
                    resolved.insert(namespace, items);
                }
                Ok(Err(error)) => {
                    results.abort_all();
                    return Err(error);
                }
                Err(_) => {
                    results.abort_all();
                    return Err(CatalogError::Unreachable);
                }
            }
        }

        let mut records = Vec::with_capacity(addressable.len());
        for (app_name, namespace, catalog_item_id) in addressable {
            let Some(item) = resolved
                .get(&namespace)
                .and_then(|items| items.get(&catalog_item_id))
            else {
                // The catalog did not resolve this record; it cannot be titled or checked for
                // DLC status, so it does not enter the owned catalog.
                continue;
            };
            if item.has_main_game || !item.is_game {
                continue;
            }
            let title = item.title.trim();
            if title.is_empty() {
                continue;
            }
            let record = CatalogRecord::new(app_name, title.to_owned())
                .with_field("namespace", namespace)
                .and_then(|record| record.with_field("catalog_item_id", catalog_item_id))
                .map_err(|_| CatalogError::Changed)?;
            records.push(record);
        }
        Ok(OwnedCatalog {
            records,
            playtime_names,
        })
    }

    fn account_changed(&self, token: &EpicToken) -> bool {
        self.lock()
            .account_id
            .as_deref()
            .is_some_and(|account_id| account_id != token.account_id)
    }

    /// A refresh can omit the display name; keep the last known name for the same account rather
    /// than blanking the card.
    fn resolved_account_name(&self, token: &EpicToken) -> Option<String> {
        if !token.display_name.trim().is_empty() {
            return Some(token.display_name.clone());
        }
        let inner = self.lock();
        if inner.account_id.as_deref() == Some(token.account_id.as_str()) {
            inner.account_name.clone()
        } else {
            None
        }
    }

    async fn apply_session_failure(&self, generation: u64, error: EpicError) {
        if self.superseded(generation) {
            return;
        }
        tracing::warn!(error = %error, "Epic session check did not succeed");
        // Epic rejects the grant with `invalid_grant`, or the request with 400/401. Every other
        // category is transient or an unusable response, not a reason to discard the credential.
        let expired = matches!(
            error,
            EpicError::InvalidGrant | EpicError::Unauthorized | EpicError::Rejected
        );
        let delete_failed = if expired {
            Some(!self.delete_credential().await)
        } else {
            None
        };
        let Some(mut inner) = self.commit_if_current(generation) else {
            return;
        };
        inner.previous_state = None;
        inner.state = if expired {
            AccountState::Expired
        } else {
            AccountState::Unreachable
        };
        inner.access = None;
        if expired {
            inner.credential = None;
            inner.playtime_names.clear();
            if delete_failed == Some(true) {
                inner.credential_save_failed = true;
            }
        }
        drop(inner);
        self.notify();
    }

    async fn save_session_account(
        &self,
        token: &EpicToken,
        account_name: Option<String>,
        now: i64,
    ) -> bool {
        let account = StoreAccount {
            provider: PROVIDER.to_owned(),
            account_id: token.account_id.clone(),
            account_name,
            last_verified_at: Some(now),
        };
        self.save_account(account).await
    }

    async fn load_credential(&self) -> Result<Option<SessionCredential>, CredentialError> {
        let store = self.ports.credentials.clone();
        match tokio::task::spawn_blocking(move || store.load()).await {
            Ok(result) => result,
            Err(_) => Err(CredentialError::Failed),
        }
    }

    async fn save_credential(&self, credential: &SessionCredential) -> bool {
        let store = self.ports.credentials.clone();
        let credential = credential.clone();
        matches!(
            tokio::task::spawn_blocking(move || store.save(&credential)).await,
            Ok(Ok(()))
        )
    }

    async fn delete_credential(&self) -> bool {
        let store = self.ports.credentials.clone();
        matches!(
            tokio::task::spawn_blocking(move || store.delete()).await,
            Ok(Ok(()))
        )
    }

    async fn load_account(&self) -> Result<Option<StoreAccount>, StorageError> {
        let store = self.ports.accounts.clone();
        match tokio::task::spawn_blocking(move || store.load()).await {
            Ok(result) => result,
            Err(_) => Err(StorageError::Failed(
                "the store account could not be read".to_owned(),
            )),
        }
    }

    async fn save_account(&self, account: StoreAccount) -> bool {
        let store = self.ports.accounts.clone();
        matches!(
            tokio::task::spawn_blocking(move || store.save(&account)).await,
            Ok(Ok(()))
        )
    }

    async fn clear_account(&self) -> bool {
        let store = self.ports.accounts.clone();
        matches!(
            tokio::task::spawn_blocking(move || store.clear()).await,
            Ok(Ok(()))
        )
    }
}

/// One namespace's catalog titles, retried once on any failure. A second failure fails the whole
/// owned catalog.
async fn fetch_titles(
    api: Arc<dyn EpicApi>,
    token: SessionCredential,
    namespace: String,
    ids: Vec<String>,
) -> Result<(String, BTreeMap<String, EpicCatalogItem>), CatalogError> {
    match api.catalog_items(&token, &namespace, &ids).await {
        Ok(items) => Ok((namespace, items)),
        Err(_) => api
            .catalog_items(&token, &namespace, &ids)
            .await
            .map(|items| (namespace, items))
            .map_err(catalog_error),
    }
}

fn catalog_error(error: EpicError) -> CatalogError {
    match error {
        EpicError::Unreachable => CatalogError::Unreachable,
        EpicError::RateLimited => CatalogError::RateLimited,
        EpicError::Server => CatalogError::Server,
        EpicError::Unauthorized | EpicError::InvalidGrant => CatalogError::Expired,
        // A rejected or unparsable response cannot replace the previous catalog facts.
        EpicError::Rejected | EpicError::Changed | EpicError::Configuration => {
            CatalogError::Changed
        }
    }
}

/// The catalog category for a failed session validation.
fn session_catalog_error(error: EpicError) -> CatalogError {
    if matches!(
        error,
        EpicError::InvalidGrant | EpicError::Unauthorized | EpicError::Rejected
    ) {
        CatalogError::Expired
    } else {
        CatalogError::Unreachable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_epic_failure_has_a_secret_free_catalog_category() {
        for (error, expected) in [
            (EpicError::Unreachable, CatalogError::Unreachable),
            (EpicError::RateLimited, CatalogError::RateLimited),
            (EpicError::Server, CatalogError::Server),
            (EpicError::Unauthorized, CatalogError::Expired),
            (EpicError::InvalidGrant, CatalogError::Expired),
            (EpicError::Rejected, CatalogError::Changed),
            (EpicError::Changed, CatalogError::Changed),
            (EpicError::Configuration, CatalogError::Changed),
        ] {
            assert_eq!(catalog_error(error), expected, "{error}");
            assert!(!format!("{error:?}").contains("token"));
        }
        assert_eq!(
            session_catalog_error(EpicError::InvalidGrant),
            CatalogError::Expired
        );
        assert_eq!(
            session_catalog_error(EpicError::Server),
            CatalogError::Unreachable
        );
    }

    #[test]
    fn the_default_sync_budget_fits_inside_the_host_operation_timeout() {
        assert!(SYNC_BUDGET < Duration::from_secs(300));
        assert_eq!(MAX_TITLE_REQUESTS, 8);
    }
}
