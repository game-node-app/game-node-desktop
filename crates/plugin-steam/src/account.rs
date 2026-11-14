//! The Steam store account service: connection status, credential lifecycle and catalog syncs.
//!
//! The service owns the non-secret account state (identity, last check/sync, persistence
//! failures) and drives the injected credential, store-account, Steam and sign-in-window ports. It
//! is deliberately split into synchronous state transitions and injected ports so the engine can
//! run every blocking or slow call off the UI thread:
//!
//! - the credential and store-account ports block; the service calls them through
//!   [`tokio::task::spawn_blocking`]. The engine calls [`SteamAccount::startup`] from a Tokio
//!   task, never from the UI thread, and keeps its own FIFO storage worker for the library;
//! - Steam requests run on Tokio with a bounded timeout;
//! - [`SteamAccount::status`] and the action helpers only lock a small in-memory state,
//!   so a command or snapshot can read them cheaply on any thread.
//!
//! Exactly one check or sync runs at a time. A request that arrives while one is in flight joins
//! the running operation and shares its result instead of starting a second request. Every
//! network result is checked against a generation counter, so a result from before a sign-in,
//! sign-out or credential removal never mutates the account.

use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::steam::{
    SteamAchievement, SteamAchievementDefinition, SteamAchievementState, SteamError, SteamFamily,
    SteamOwnedGame, SteamSession, SteamWebApi,
};
use gamenode_plugin_protocol::models::SessionCredential;
#[cfg(test)]
use gamenode_plugin_protocol::models::StorageMethod;
use gamenode_plugin_protocol::models::{CatalogError, CatalogRecord, StoreAccount};
use gamenode_plugin_protocol::models::{CredentialError, CredentialStore, StorageError};

use crate::login::{SteamLoginResult, SteamLoginWindow};

/// The provider id of the Steam store account.
pub const PROVIDER: &str = "steam";

/// Upper bound for one Steam operation, including the client's own retries (three 20-second
/// requests plus backoff). A stalled network must not leave the account in `checking` forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

// Public account values are the same contract used by independent hosts.
pub use gamenode_plugin_protocol::models::{
    AccountState as SteamAccountState, AccountStatus as SteamAccountStatus,
    CatalogOutcome as SteamCatalogOutcome, CheckOutcome as SteamCheckOutcome,
    SignInOutcome as SteamSignInOutcome, SignInResult as SteamSignInResult,
    SignOutOutcome as SteamSignOutOutcome, StoreAccountStore,
};

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

/// The adapters the account service drives.
#[derive(Clone)]
pub struct SteamAccountPorts {
    /// The Steam credential slot, separate from the GameNode session slot.
    pub credentials: Arc<dyn CredentialStore>,
    pub accounts: Arc<dyn StoreAccountStore>,
    pub client: Arc<dyn SteamWebApi>,
    pub login: Arc<dyn SteamLoginWindow>,
    pub clock: Arc<dyn Clock>,
}

/// Called after every status transition. It must be cheap and must not call back into the service:
/// the engine uses it to publish a snapshot from the operation's background thread.
pub type StatusObserver = Arc<dyn Fn(&SteamAccountStatus) + Send + Sync>;

/// One requested game's achievement result: ready, confirmed none, or a failure category.
pub type SteamAchievementResult = Result<Option<Vec<SteamAchievement>>, CatalogError>;

/// The result of an achievement read. Each requested game has one outcome: `Ok(Some(_))` is
/// ready, `Ok(None)` is a confirmed no-achievements game, and `Err(_)` leaves cached data intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamAchievementsOutcome {
    pub status: SteamAccountStatus,
    pub outcomes: Vec<(String, SteamAchievementResult)>,
    pub superseded: bool,
}

/// Steam's language code for an application locale. Steam falls back to English text when a
/// translation is missing, so an unsupported locale reads English.
pub fn steam_language(locale: &str) -> &'static str {
    let locale = locale.trim().to_ascii_lowercase();
    if locale == "pt" || locale.starts_with("pt-") || locale.starts_with("pt_") {
        "brazilian"
    } else {
        "english"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationKind {
    Check,
    Sync,
}

struct InFlight<T> {
    result: tokio::sync::watch::Sender<Option<Arc<T>>>,
}

struct OperationOutcome {
    status: SteamAccountStatus,
    superseded: bool,
    account_changed: bool,
    owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    family: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    now: i64,
}

struct Inner {
    loaded: bool,
    credential: Option<SessionCredential>,
    state: SteamAccountState,
    account_id: Option<String>,
    account_name: Option<String>,
    last_checked_at: Option<i64>,
    last_synced_at: Option<i64>,
    credential_save_failed: bool,
    account_save_failed: bool,
    catalog_error: Option<CatalogError>,
    family_error: Option<CatalogError>,
    /// Bumped by every sign-in start, cancel and sign-out. Network results carry the value they
    /// started with and are ignored when it changed.
    generation: u64,
    /// The state to return to when a sign-in window is cancelled or closed.
    previous_state: Option<SteamAccountState>,
    inflight: Option<InFlight<OperationOutcome>>,
    /// A load is in flight. Startup and a catalog request can race, and a second load would
    /// reset the status the first one or a finished check just committed.
    loading: Option<InFlight<SteamAccountStatus>>,
}

impl Inner {
    fn new() -> Self {
        Self {
            loaded: false,
            credential: None,
            state: SteamAccountState::NotConnected,
            account_id: None,
            account_name: None,
            last_checked_at: None,
            last_synced_at: None,
            credential_save_failed: false,
            account_save_failed: false,
            catalog_error: None,
            family_error: None,
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

/// The Steam store account service.
pub struct SteamAccount {
    ports: SteamAccountPorts,
    observer: Mutex<StatusObserver>,
    inner: Arc<Mutex<Inner>>,
    validity: Arc<AtomicBool>,
    /// Serializes store validations so two operations cannot rotate the session cookie at the
    /// same time and overwrite each other's refresh value.
    session: Arc<tokio::sync::Mutex<()>>,
}

impl SteamAccount {
    /// `observer` runs after every transition, outside the service lock.
    pub fn new(
        ports: SteamAccountPorts,
        observer: impl Fn(&SteamAccountStatus) + Send + Sync + 'static,
    ) -> Self {
        Self {
            ports,
            observer: Mutex::new(Arc::new(observer)),
            inner: Arc::new(Mutex::new(Inner::new())),
            validity: Arc::new(AtomicBool::new(true)),
            session: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Share account state while binding every storage/login call to the
    /// immutable host context for this operation. Never retag an older task.
    pub fn for_operation(&self, ports: SteamAccountPorts, validity: Arc<AtomicBool>) -> Self {
        Self {
            ports,
            observer: Mutex::new(Arc::new(|_| {})),
            inner: self.inner.clone(),
            validity,
            session: self.session.clone(),
        }
    }

    /// Builds the service before the engine can be captured by an observer. The engine calls
    /// [`set_observer`](Self::set_observer) once it is shared.
    pub fn without_observer(ports: SteamAccountPorts) -> Self {
        Self::new(ports, |_| {})
    }

    /// Replaces the callback that publishes status transitions.
    pub fn set_observer(&self, observer: impl Fn(&SteamAccountStatus) + Send + Sync + 'static) {
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
    pub fn status(&self) -> SteamAccountStatus {
        let inner = self.lock();
        SteamAccountStatus {
            state: inner.state,
            account_id: inner.account_id.clone(),
            account_name: inner.account_name.clone(),
            last_checked_at: inner.last_checked_at,
            last_synced_at: inner.last_synced_at,
            storage: self.ports.credentials.method(),
            persist_failed: inner.persist_failed(),
            catalog_error: inner.catalog_error,
            shared_error: inner.family_error,
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
        if self.lock().state == SteamAccountState::SigningIn {
            return;
        }
        let _ = self.delete_credential().await;
        if self.lock().state == SteamAccountState::NotConnected {
            let _ = self.clear_account().await;
        }
    }

    /// Removes a profile left by a crash, then loads the credential and account row. The engine
    /// calls this once at startup from Tokio (the profile and stores are blocking, so they run on
    /// the blocking pool).
    pub async fn startup(&self) -> SteamAccountStatus {
        if self.lock().state == SteamAccountState::SigningIn {
            return self.status();
        }
        let login = self.ports.login.clone();
        let _ = tokio::task::spawn_blocking(move || login.cleanup_profile()).await;
        // Catalog work may have loaded and validated the account before this
        // startup task was scheduled. Do not reload a credential it just removed
        // on expiry, or erase a newer connected/expired state.
        let loaded = self.lock().loaded;
        if loaded {
            self.status()
        } else {
            self.load_stored().await
        }
    }

    /// Reloads the credential and account row from their stores. Also used by "Retry" after a
    /// credential error; unlike [`startup`](Self::startup) it does not touch the profile.
    ///
    /// Concurrent callers share one load: startup and a catalog request can both need the account
    /// at the same moment, and a second load finishing late would reset the status a check or sync
    /// had already committed.
    pub async fn load_stored(&self) -> SteamAccountStatus {
        enum Entry {
            Start,
            Join(tokio::sync::watch::Receiver<Option<Arc<SteamAccountStatus>>>),
        }

        let entry = {
            let mut inner = self.lock();
            // Loading is a startup/retry operation. It must never reset the window state or the
            // credential a sign-in window is about to produce.
            if inner.state == SteamAccountState::SigningIn {
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

    async fn perform_load(&self) -> SteamAccountStatus {
        let generation = self.lock().generation;
        let credential = self.load_credential().await;
        let account = self.load_account().await;
        if credential.is_err() || account.is_err() {
            tracing::warn!("could not read the saved Steam account");
        }
        {
            let mut inner = self.lock();
            if !self.validity.load(Ordering::Acquire) || inner.generation != generation {
                // A sign-in, sign-out or credential removal happened while the stores were being
                // read; its result owns the state and must not be replaced by the loaded one.
                drop(inner);
                return self.status();
            }
            inner.loaded = true;
            inner.previous_state = None;
            inner.credential_save_failed = false;
            inner.account_save_failed = false;
            inner.catalog_error = None;
            inner.family_error = None;
            inner.last_synced_at = None;
            match credential {
                Err(_) => {
                    inner.state = SteamAccountState::CredentialError;
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
                    inner.state = SteamAccountState::NotConnected;
                    inner.credential = None;
                    inner.account_id = None;
                    inner.account_name = None;
                    inner.last_checked_at = None;
                }
                Ok(Some(credential)) => {
                    inner.state = SteamAccountState::Checking;
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
            "loaded the saved Steam account"
        );
        status
    }

    async fn ensure_loaded(&self) {
        let reload = {
            let inner = self.lock();
            !inner.loaded || inner.state == SteamAccountState::CredentialError
        };
        if reload {
            self.load_stored().await;
        }
    }

    /// Validates the stored session and updates the connection state. Coalesces with a running
    /// check or sync.
    pub async fn check(&self) -> SteamCheckOutcome {
        self.ensure_loaded().await;
        if self.lock().state == SteamAccountState::CredentialError {
            return SteamCheckOutcome {
                status: self.status(),
                account_changed: false,
                superseded: false,
            };
        }
        let outcome = self.run_or_join(OperationKind::Check).await;
        SteamCheckOutcome {
            status: outcome.status.clone(),
            account_changed: outcome.account_changed,
            superseded: outcome.superseded,
        }
    }

    /// Validates the session first, then fetches the owned and family catalogs. Coalesces with a
    /// running check or sync; a check that is already running yields a result with no catalog.
    pub async fn sync(&self) -> SteamCatalogOutcome {
        self.ensure_loaded().await;
        if self.lock().state == SteamAccountState::CredentialError {
            return SteamCatalogOutcome {
                status: self.status(),
                owned: None,
                shared: None,
                now: self.ports.clock.now(),
                account_changed: false,
                superseded: false,
            };
        }
        let outcome = self.run_or_join(OperationKind::Sync).await;
        SteamCatalogOutcome {
            status: outcome.status.clone(),
            owned: outcome.owned.clone(),
            shared: outcome.family.clone(),
            now: outcome.now,
            account_changed: outcome.account_changed,
            superseded: outcome.superseded,
        }
    }

    /// Reads achievement definitions and unlock state for a bounded batch. No request is made
    /// without a stored credential. Every requested game gets its own outcome, so one failure
    /// never discards another game's data.
    pub async fn achievements(
        &self,
        game_ids: &[String],
        locale: &str,
    ) -> SteamAchievementsOutcome {
        self.ensure_loaded().await;
        let Some((generation, credential)) = self.begin_operation() else {
            return self
                .achievements_outcome(all_failed(game_ids, CatalogError::Unreachable), true);
        };
        let Some(credential) = credential else {
            // No connected account: make no request and leave every cached row untouched.
            return self.achievements_outcome(Vec::new(), false);
        };
        if self.superseded(generation) {
            return self.achievements_outcome(Vec::new(), true);
        }
        let session = match self.validate_session(generation, &credential).await {
            Ok(session) => session,
            Err(error) => {
                if self.superseded(generation) {
                    return self.achievements_outcome(Vec::new(), true);
                }
                self.apply_session_failure(generation, error).await;
                return self
                    .achievements_outcome(all_failed(game_ids, error.catalog_error()), false);
            }
        };
        // The check that just ran may have renewed the access cookie (and Steam may have
        // rotated the refresh cookie with it). Persist the renewed header before using the
        // short-lived token; otherwise the next operation would renew the same cookie again.
        if self.cookie_rotated(&session) {
            if self.superseded(generation) {
                return self.achievements_outcome(Vec::new(), true);
            }
            let saved = self.save_credential(&session.cookie).await;
            if self.superseded(generation) {
                if saved {
                    self.discard_saved_if_signed_out().await;
                }
                return self.achievements_outcome(Vec::new(), true);
            }
            let mut inner = self.lock();
            inner.credential = Some(session.cookie.clone());
            inner.credential_save_failed = !saved;
        }
        let language = steam_language(locale);
        let client = self.ports.client.clone();
        let mut outcomes = Vec::with_capacity(game_ids.len());
        for game_id in game_ids {
            if self.superseded(generation) {
                return self.achievements_outcome(Vec::new(), true);
            }
            let definitions = tokio::time::timeout(
                REQUEST_TIMEOUT,
                client.achievement_definitions(&session, game_id, language),
            )
            .await;
            let definitions = match definitions {
                Ok(Ok(value)) => value,
                Ok(Err(error)) => {
                    outcomes.push((game_id.clone(), Err(error.catalog_error())));
                    continue;
                }
                Err(_) => {
                    outcomes.push((game_id.clone(), Err(CatalogError::Unreachable)));
                    continue;
                }
            };
            let Some(definitions) = definitions.filter(|definitions| !definitions.is_empty())
            else {
                outcomes.push((game_id.clone(), Ok(None)));
                continue;
            };
            let states = tokio::time::timeout(
                REQUEST_TIMEOUT,
                client.achievement_states(&session, game_id, language),
            )
            .await;
            let states = match states {
                Ok(Ok(value)) => value,
                Ok(Err(error)) => {
                    // Definitions succeeded but unlock state failed: the whole game is a failure
                    // and its previous rows stay unchanged.
                    outcomes.push((game_id.clone(), Err(error.catalog_error())));
                    continue;
                }
                Err(_) => {
                    outcomes.push((game_id.clone(), Err(CatalogError::Unreachable)));
                    continue;
                }
            };
            let Some(states) = states else {
                outcomes.push((game_id.clone(), Ok(None)));
                continue;
            };
            outcomes.push((
                game_id.clone(),
                Ok(Some(join_achievements(definitions, states))),
            ));
        }
        if self.superseded(generation) {
            return self.achievements_outcome(Vec::new(), true);
        }
        self.achievements_outcome(outcomes, false)
    }

    fn achievements_outcome(
        &self,
        outcomes: Vec<(String, SteamAchievementResult)>,
        superseded: bool,
    ) -> SteamAchievementsOutcome {
        SteamAchievementsOutcome {
            status: self.status(),
            outcomes,
            superseded,
        }
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
        // Take the in-flight slot before publishing the result. Otherwise a joining caller can
        // wake between the send and the clear, join the completed operation again, and receive
        // its None catalog instead of starting a fresh sync.
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
            family: None,
            now: self.ports.clock.now(),
        }
    }

    fn outcome(
        &self,
        now: i64,
        account_changed: bool,
        owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
        family: Option<Result<Vec<CatalogRecord>, CatalogError>>,
        superseded: bool,
    ) -> OperationOutcome {
        OperationOutcome {
            status: self.status(),
            superseded,
            account_changed,
            owned,
            family,
            now,
        }
    }

    fn superseded_outcome(&self) -> OperationOutcome {
        self.outcome(self.ports.clock.now(), false, None, None, true)
    }

    fn begin_operation(&self) -> Option<(u64, Option<SessionCredential>)> {
        let mut inner = self.lock();
        if inner.state == SteamAccountState::SigningIn {
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
            // Expiry removed the rejected cookie deliberately. A later
            // background request must retain the sign-in-again state.
            if inner.state != SteamAccountState::Expired {
                inner.state = SteamAccountState::NotConnected;
            }
            drop(inner);
            self.notify();
            return self.outcome(self.ports.clock.now(), false, None, None, false);
        };
        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        {
            let mut inner = self.lock();
            if self.validity.load(Ordering::Acquire)
                && inner.generation == generation
                && inner.state != SteamAccountState::SigningIn
            {
                inner.state = SteamAccountState::Checking;
            }
        }
        self.notify();

        let session = match self.validate_session(generation, &credential).await {
            Ok(session) => session,
            Err(error) => {
                if self.superseded(generation) {
                    return self.superseded_outcome();
                }
                self.apply_session_failure(generation, error).await;
                if self.superseded(generation) {
                    return self.superseded_outcome();
                }
                return self.outcome(self.ports.clock.now(), false, None, None, false);
            }
        };

        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        let account_changed = self.account_changed(&session);
        let account_name = self.resolved_account_name(&session);
        let rotated = self.cookie_rotated(&session);
        let credential_saved = if rotated {
            self.save_credential(&session.cookie).await
        } else {
            true
        };
        if self.superseded(generation) {
            if rotated {
                self.discard_saved_if_signed_out().await;
            }
            return self.superseded_outcome();
        }
        let now = self.ports.clock.now();
        let account_saved = self
            .save_session_account(&session, account_name.clone(), now)
            .await;
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.superseded_outcome();
        }

        let Some(mut inner) = self.commit_if_current(generation) else {
            if rotated || !account_saved {
                self.discard_saved_if_signed_out().await;
            }
            return self.superseded_outcome();
        };
        inner.state = SteamAccountState::Connected;
        inner.account_id = Some(session.steam_id.clone());
        inner.account_name = account_name;
        inner.last_checked_at = Some(now);
        inner.previous_state = None;
        if rotated {
            inner.credential = Some(session.cookie.clone());
            inner.credential_save_failed = !credential_saved;
        }
        inner.account_save_failed = !account_saved;
        drop(inner);
        self.notify();
        tracing::info!("Steam session check succeeded");
        self.outcome(now, account_changed, None, None, false)
    }

    async fn perform_sync(&self) -> OperationOutcome {
        let Some((generation, credential)) = self.begin_operation() else {
            return self.superseded_outcome();
        };
        let Some(credential) = credential else {
            // A concurrent sign-in owns the state now: a finished sync must not overwrite it.
            let Some(mut inner) = self.commit_if_current(generation) else {
                return self.superseded_outcome();
            };
            // Expiry removed the rejected cookie deliberately. A later
            // background request must retain the sign-in-again state.
            if inner.state != SteamAccountState::Expired {
                inner.state = SteamAccountState::NotConnected;
            }
            drop(inner);
            self.notify();
            return self.outcome(self.ports.clock.now(), false, None, None, false);
        };
        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        {
            let mut inner = self.lock();
            if self.validity.load(Ordering::Acquire)
                && inner.generation == generation
                && inner.state != SteamAccountState::SigningIn
            {
                inner.state = SteamAccountState::Checking;
            }
        }
        self.notify();

        let session = match self.validate_session(generation, &credential).await {
            Ok(session) => session,
            Err(error) => {
                if self.superseded(generation) {
                    return self.superseded_outcome();
                }
                let category = error.catalog_error();
                self.apply_session_failure(generation, error).await;
                if self.superseded(generation) {
                    return self.superseded_outcome();
                }
                return self.outcome(
                    self.ports.clock.now(),
                    false,
                    Some(Err(category)),
                    None,
                    false,
                );
            }
        };

        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        let account_changed = self.account_changed(&session);
        let account_name = self.resolved_account_name(&session);
        let rotated = self.cookie_rotated(&session);
        let credential_saved = if rotated {
            self.save_credential(&session.cookie).await
        } else {
            true
        };
        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        let now = self.ports.clock.now();
        let account_saved = self
            .save_session_account(&session, account_name.clone(), now)
            .await;
        if self.superseded(generation) {
            return self.superseded_outcome();
        }
        {
            let Some(mut inner) = self.commit_if_current(generation) else {
                self.discard_saved_if_signed_out().await;
                return self.superseded_outcome();
            };
            inner.account_id = Some(session.steam_id.clone());
            inner.account_name = account_name;
            inner.last_checked_at = Some(now);
            if rotated {
                inner.credential = Some(session.cookie.clone());
                inner.credential_save_failed = !credential_saved;
            }
            inner.account_save_failed = !account_saved;
        }

        let owned = self.fetch_owned(&session).await;
        let family = match &owned {
            Ok(_) => Some(self.fetch_family(&session).await),
            Err(error) if family_still_worth_trying(*error) => {
                Some(self.fetch_family(&session).await)
            }
            Err(_) => None,
        };
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
            Err(CatalogError::Unreachable | CatalogError::RateLimited | CatalogError::Server)
        );
        if self.superseded(generation) {
            return self.superseded_outcome();
        }

        let Some(mut inner) = self.commit_if_current(generation) else {
            return self.superseded_outcome();
        };
        inner.previous_state = None;
        inner.state = if expired {
            SteamAccountState::Expired
        } else if unreachable {
            SteamAccountState::Unreachable
        } else {
            SteamAccountState::Connected
        };
        // The store session was just validated; a rejected catalog token is not proof that the
        // long-lived refresh cookie is dead. Keep it so the next check can renew the access token
        // instead of forcing a full sign-in.
        inner.catalog_error = match &owned {
            Ok(_) => None,
            Err(error) => Some(*error),
        };
        if let Some(family) = &family {
            inner.family_error = match family {
                Ok(_) => None,
                Err(error) => Some(*error),
            };
        }
        if owned.is_ok() {
            inner.last_synced_at = Some(now);
        }
        drop(inner);
        self.notify();
        tracing::info!(
            state = ?self.status().state,
            owned = owned.as_ref().ok().map(Vec::len),
            owned_error = ?owned.as_ref().err(),
            family = family.as_ref().and_then(|result| result.as_ref().ok()).map(Vec::len),
            family_error = ?family.as_ref().and_then(|result| result.as_ref().err()),
            "Steam catalog sync finished"
        );
        self.outcome(now, account_changed, Some(owned), family, false)
    }

    /// Starts the Steam sign-in window. Only one attempt runs at a time; a second call while the
    /// window is open returns the current status without opening another window.
    pub async fn sign_in(&self) -> SteamSignInOutcome {
        self.ensure_loaded().await;
        if !self.validity.load(Ordering::Acquire) {
            return self.sign_in_superseded();
        }
        let generation = {
            let mut inner = self.lock();
            if inner.state == SteamAccountState::SigningIn {
                None
            } else {
                inner.generation = inner.generation.wrapping_add(1);
                let generation = inner.generation;
                inner.previous_state = Some(inner.state);
                inner.state = SteamAccountState::SigningIn;
                Some(generation)
            }
        };
        let Some(generation) = generation else {
            return SteamSignInOutcome {
                status: self.status(),
                result: SteamSignInResult::Superseded,
                account_changed: false,
            };
        };
        self.notify();
        tracing::info!("opening the Steam sign-in window");

        let outcome = self.ports.login.run().await;
        if self.superseded(generation) {
            return self.sign_in_superseded();
        }
        let outcome = match outcome.result {
            SteamLoginResult::Verified { session } => {
                self.complete_verified_sign_in(generation, session).await
            }
            SteamLoginResult::Cancelled => {
                self.restore_previous_state(generation, SteamSignInResult::Cancelled)
            }
            SteamLoginResult::Closed => {
                self.restore_previous_state(generation, SteamSignInResult::Closed)
            }
            SteamLoginResult::TimedOut => {
                self.restore_previous_state(generation, SteamSignInResult::TimedOut)
            }
            SteamLoginResult::Failed(_) => {
                self.restore_previous_state(generation, SteamSignInResult::Failed)
            }
        };
        tracing::info!(
            result = ?outcome.result,
            state = ?outcome.status.state,
            "Steam sign-in finished"
        );
        outcome
    }

    async fn complete_verified_sign_in(
        &self,
        generation: u64,
        session: SteamSession,
    ) -> SteamSignInOutcome {
        tracing::info!("Steam sign-in verified the authenticated store page");
        let cookie = session.cookie.clone();
        let saved = self.save_credential(&cookie).await;
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.sign_in_superseded();
        }
        {
            let mut inner = self.lock();
            inner.credential = Some(cookie);
            inner.credential_save_failed = !saved;
        }
        self.finish_sign_in_session(generation, session).await
    }

    async fn finish_sign_in_session(
        &self,
        generation: u64,
        session: SteamSession,
    ) -> SteamSignInOutcome {
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.sign_in_superseded();
        }
        let account_changed = self.account_changed(&session);
        let account_name = self.resolved_account_name(&session);
        let rotated = self.cookie_rotated(&session);
        let rotated_saved = if rotated {
            self.save_credential(&session.cookie).await
        } else {
            true
        };
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.sign_in_superseded();
        }
        let now = self.ports.clock.now();
        let account_saved = self
            .save_session_account(&session, account_name.clone(), now)
            .await;
        if self.superseded(generation) {
            self.discard_saved_if_signed_out().await;
            return self.sign_in_superseded();
        }
        let Some(mut inner) = self.commit_if_current(generation) else {
            self.discard_saved_if_signed_out().await;
            return self.sign_in_superseded();
        };
        inner.state = SteamAccountState::Connected;
        inner.account_id = Some(session.steam_id.clone());
        inner.account_name = account_name;
        inner.last_checked_at = Some(now);
        inner.previous_state = None;
        if rotated {
            inner.credential = Some(session.cookie.clone());
            inner.credential_save_failed = !rotated_saved;
        }
        inner.account_save_failed = !account_saved;
        drop(inner);
        self.notify();
        SteamSignInOutcome {
            status: self.status(),
            result: SteamSignInResult::Connected,
            account_changed,
        }
    }

    fn restore_previous_state(
        &self,
        generation: u64,
        result: SteamSignInResult,
    ) -> SteamSignInOutcome {
        let restored = {
            let mut inner = self.lock();
            if !self.validity.load(Ordering::Acquire) || inner.generation != generation {
                false
            } else {
                inner.state = inner
                    .previous_state
                    .take()
                    .unwrap_or(SteamAccountState::NotConnected);
                true
            }
        };
        if restored {
            self.notify();
        }
        SteamSignInOutcome {
            status: self.status(),
            result: if restored {
                result
            } else {
                SteamSignInResult::Superseded
            },
            account_changed: false,
        }
    }

    fn sign_in_superseded(&self) -> SteamSignInOutcome {
        SteamSignInOutcome {
            status: self.status(),
            result: SteamSignInResult::Superseded,
            account_changed: false,
        }
    }

    /// Cancels an open sign-in window and returns to the state before the attempt. No credential
    /// is saved. A no-op when no window is open.
    ///
    /// The port call dispatches to the webview thread; call this from an async command or task,
    /// never from a UI callback.
    pub fn cancel_sign_in(&self) -> SteamAccountStatus {
        if !self.validity.load(Ordering::Acquire) {
            return self.status();
        }
        let cancelled = {
            let mut inner = self.lock();
            if inner.state == SteamAccountState::SigningIn {
                inner.generation = inner.generation.wrapping_add(1);
                inner.state = inner
                    .previous_state
                    .take()
                    .unwrap_or(SteamAccountState::NotConnected);
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
        if self.lock().state != SteamAccountState::SigningIn {
            return false;
        }
        self.ports.login.focus().is_ok()
    }

    /// Deletes the credential and clears the account. A failed deletion leaves the account
    /// connected for this run and reports `persistFailed`; the caller must not claim a restart
    /// will stay signed out.
    pub async fn sign_out(&self) -> SteamSignOutOutcome {
        if !self.validity.load(Ordering::Acquire) {
            return SteamSignOutOutcome {
                status: self.status(),
                signed_out: false,
            };
        }
        let (previous_state, generation) = {
            let mut inner = self.lock();
            inner.generation = inner.generation.wrapping_add(1);
            (
                inner.previous_state.take().unwrap_or(inner.state),
                inner.generation,
            )
        };
        let login = self.ports.login.clone();
        let _ = tokio::task::spawn_blocking(move || login.cancel()).await;
        let deleted = self.delete_credential().await;
        if self.superseded(generation) {
            return SteamSignOutOutcome {
                status: self.status(),
                signed_out: false,
            };
        }
        if !deleted {
            let mut inner = self.lock();
            inner.credential_save_failed = true;
            if matches!(
                inner.state,
                SteamAccountState::SigningIn | SteamAccountState::Checking
            ) {
                inner.state = if inner.credential.is_some() {
                    SteamAccountState::Connected
                } else {
                    previous_state
                };
            }
            drop(inner);
            self.notify();
            return SteamSignOutOutcome {
                status: self.status(),
                signed_out: false,
            };
        }

        let cleared = self.clear_account().await;
        if self.superseded(generation) {
            return SteamSignOutOutcome {
                status: self.status(),
                signed_out: false,
            };
        }
        {
            let mut inner = self.lock();
            inner.credential = None;
            inner.state = SteamAccountState::NotConnected;
            inner.account_id = None;
            inner.account_name = None;
            inner.last_checked_at = None;
            inner.last_synced_at = None;
            inner.catalog_error = None;
            inner.family_error = None;
            inner.previous_state = None;
            inner.credential_save_failed = false;
            inner.account_save_failed = !cleared;
        }
        self.notify();
        SteamSignOutOutcome {
            status: self.status(),
            signed_out: true,
        }
    }

    /// Removes a credential that could not be read, recovering from `credentialError`.
    pub async fn remove_credential(&self) -> SteamSignOutOutcome {
        self.sign_out().await
    }

    async fn validate_session(
        &self,
        generation: u64,
        credential: &SessionCredential,
    ) -> Result<SteamSession, SteamError> {
        // Store validation can rotate both the access and refresh cookies. Serialize it so a
        // concurrent achievement read cannot mint a second token that invalidates this one.
        let _session = self.session.lock().await;
        let client = self.ports.client.clone();
        let credential = credential.clone();
        match tokio::time::timeout(REQUEST_TIMEOUT, client.session(&credential)).await {
            Ok(Ok(session)) => Ok(session),
            Ok(Err(SteamError::Expired)) => {
                // Steam's access cookie lives about a day; the saved refresh cookie can mint a
                // new one. Persist the renewal before validating it because Steam may rotate the
                // refresh cookie in the same exchange.
                let renewed =
                    match tokio::time::timeout(REQUEST_TIMEOUT, client.renew_session(&credential))
                        .await
                    {
                        Ok(result) => result?,
                        Err(_) => return Err(SteamError::Unreachable),
                    };
                let Some(renewed) = renewed else {
                    return Err(SteamError::Expired);
                };
                if self.superseded(generation) {
                    return Err(SteamError::Expired);
                }
                let saved = self.save_credential(&renewed).await;
                if self.superseded(generation) {
                    if saved {
                        self.discard_saved_if_signed_out().await;
                    }
                    return Err(SteamError::Expired);
                }
                {
                    let mut inner = self.lock();
                    inner.credential = Some(renewed.clone());
                    inner.credential_save_failed = !saved;
                }
                tracing::info!("Steam renewed the expired store access cookie");
                match tokio::time::timeout(REQUEST_TIMEOUT, client.session(&renewed)).await {
                    Ok(Ok(session)) => Ok(session),
                    // The refresh token was accepted, so the saved credential is not dead. A
                    // store page that still refuses it is a temporary rejection, not a reason
                    // to discard the long-lived cookie.
                    Ok(Err(SteamError::Expired)) => Err(SteamError::Changed),
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(SteamError::Unreachable),
                }
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err(SteamError::Unreachable),
        }
    }

    async fn fetch_owned(
        &self,
        session: &SteamSession,
    ) -> Result<Vec<CatalogRecord>, CatalogError> {
        let client = self.ports.client.clone();
        let session = session.clone();
        match tokio::time::timeout(REQUEST_TIMEOUT, client.owned_games(&session)).await {
            Ok(Ok(games)) => Ok(owned_records(&games)),
            Ok(Err(error)) => Err(error.catalog_error()),
            Err(_) => Err(CatalogError::Unreachable),
        }
    }

    async fn fetch_family(
        &self,
        session: &SteamSession,
    ) -> Result<Vec<CatalogRecord>, CatalogError> {
        let client = self.ports.client.clone();
        let session = session.clone();
        match tokio::time::timeout(REQUEST_TIMEOUT, client.family_games(&session)).await {
            Ok(Ok(family)) => Ok(family_records(&family)),
            // A family failure is its own category and never fails the owned catalog.
            Ok(Err(error)) => Err(error.family_catalog_error()),
            Err(_) => Err(CatalogError::SharedUnavailable),
        }
    }

    fn account_changed(&self, session: &SteamSession) -> bool {
        self.lock()
            .account_id
            .as_deref()
            .is_some_and(|account_id| account_id != session.steam_id)
    }

    fn cookie_rotated(&self, session: &SteamSession) -> bool {
        self.lock()
            .credential
            .as_ref()
            .is_none_or(|credential| credential.expose() != session.cookie.expose())
    }

    /// The token fallback can omit the account name; keep the last known name for the same
    /// SteamID rather than blanking the card.
    fn resolved_account_name(&self, session: &SteamSession) -> Option<String> {
        if session.account_name.is_some() {
            return session.account_name.clone();
        }
        let inner = self.lock();
        if inner.account_id.as_deref() == Some(session.steam_id.as_str()) {
            inner.account_name.clone()
        } else {
            None
        }
    }

    async fn apply_session_failure(&self, generation: u64, error: SteamError) {
        // Expired and invalid cookies are credential failures. Every other category is a
        // transient or unusable Steam response, not a reason to discard the saved credential.
        if self.superseded(generation) {
            return;
        }
        tracing::warn!(error = %error, "Steam session check did not succeed");
        let expired = matches!(error, SteamError::Expired | SteamError::InvalidCookies);
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
            SteamAccountState::Expired
        } else {
            SteamAccountState::Unreachable
        };
        if expired {
            inner.credential = None;
            if delete_failed == Some(true) {
                inner.credential_save_failed = true;
            }
        }
        drop(inner);
        self.notify();
    }

    async fn save_session_account(
        &self,
        session: &SteamSession,
        account_name: Option<String>,
        now: i64,
    ) -> bool {
        let account = StoreAccount {
            provider: PROVIDER.to_owned(),
            account_id: session.steam_id.clone(),
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

fn all_failed(game_ids: &[String], error: CatalogError) -> Vec<(String, SteamAchievementResult)> {
    game_ids.iter().map(|id| (id.clone(), Err(error))).collect()
}

/// Joins schema definitions with unlock state by Steam's API name. Definitions keep Steam's
/// order; a missing state is a locked achievement, and the player payload's localized text wins
/// when it is present.
fn join_achievements(
    definitions: Vec<SteamAchievementDefinition>,
    states: Vec<SteamAchievementState>,
) -> Vec<SteamAchievement> {
    let mut states: std::collections::HashMap<_, _> = states
        .into_iter()
        .map(|state| (state.api_name.clone(), state))
        .collect();
    definitions
        .into_iter()
        .map(|definition| {
            let state = states.remove(&definition.api_name);
            let name = crate::steam::clean_achievement_text(
                state
                    .as_ref()
                    .map(|state| state.display_name.trim())
                    .filter(|name| !name.is_empty())
                    .unwrap_or(&definition.display_name),
            );
            let description = crate::steam::clean_achievement_text(
                state
                    .as_ref()
                    .map(|state| state.description.trim())
                    .filter(|description| !description.is_empty())
                    .unwrap_or(&definition.description),
            );
            SteamAchievement {
                id: definition.api_name,
                name,
                description,
                hidden: definition.hidden,
                icon_url: definition.icon,
                icon_gray_url: definition.icon_gray,
                unlocked: state.as_ref().is_some_and(|state| state.achieved),
                unlocked_at: state.and_then(|state| state.unlock_time),
            }
        })
        .collect()
}

fn owned_records(games: &[SteamOwnedGame]) -> Vec<CatalogRecord> {
    games
        .iter()
        .map(|game| CatalogRecord::new(game.app_id.clone(), game.name.clone()))
        .collect()
}

fn family_records(family: &SteamFamily) -> Vec<CatalogRecord> {
    owned_records(&family.games)
}

fn family_still_worth_trying(error: CatalogError) -> bool {
    matches!(
        error,
        CatalogError::Privacy | CatalogError::Changed | CatalogError::SharedUnavailable
    )
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

    use async_trait::async_trait;
    use tokio::sync::{Semaphore, oneshot};

    use super::*;
    use crate::login::{SteamLoginError, SteamLoginOutcome};
    use crate::steam::{SteamFamily, SteamOwnedGame, SteamWebApi};

    fn cred(value: &str) -> SessionCredential {
        SessionCredential::new(value).expect("a test credential")
    }

    fn session(id: &str, name: &str, cookie: &str) -> SteamSession {
        SteamSession {
            steam_id: id.to_owned(),
            account_name: Some(name.to_owned()),
            token: cred("webapi-token"),
            cookie: cred(cookie),
        }
    }

    fn stored(id: &str, name: &str, at: i64) -> StoreAccount {
        StoreAccount {
            provider: PROVIDER.to_owned(),
            account_id: id.to_owned(),
            account_name: Some(name.to_owned()),
            last_verified_at: Some(at),
        }
    }

    struct FakeCredentialStore {
        method: StorageMethod,
        value: Mutex<Option<SessionCredential>>,
        fail_load: AtomicBool,
        fail_save: AtomicBool,
        fail_delete: AtomicBool,
        saved: Mutex<Vec<String>>,
        deletes: AtomicUsize,
        loads: AtomicUsize,
        load_gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    }

    impl FakeCredentialStore {
        fn new(value: Option<SessionCredential>) -> Arc<Self> {
            Arc::new(Self {
                method: StorageMethod::SecretService,
                value: Mutex::new(value),
                fail_load: AtomicBool::new(false),
                fail_save: AtomicBool::new(false),
                fail_delete: AtomicBool::new(false),
                saved: Mutex::new(Vec::new()),
                deletes: AtomicUsize::new(0),
                loads: AtomicUsize::new(0),
                load_gate: Mutex::new(None),
            })
        }

        fn stored(&self) -> Option<String> {
            self.value
                .lock()
                .unwrap()
                .as_ref()
                .map(|credential| credential.expose().to_owned())
        }
    }

    impl CredentialStore for FakeCredentialStore {
        fn method(&self) -> StorageMethod {
            self.method
        }

        fn load(&self) -> Result<Option<SessionCredential>, CredentialError> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            let gate = self.load_gate.lock().unwrap().take();
            if let Some(gate) = gate {
                let _ = gate.recv();
            }
            if self.fail_load.load(Ordering::SeqCst) {
                Err(CredentialError::Locked)
            } else {
                Ok(self.value.lock().unwrap().clone())
            }
        }

        fn save(&self, credential: &SessionCredential) -> Result<(), CredentialError> {
            if self.fail_save.load(Ordering::SeqCst) {
                return Err(CredentialError::Failed);
            }
            self.saved
                .lock()
                .unwrap()
                .push(credential.expose().to_owned());
            *self.value.lock().unwrap() = Some(credential.clone());
            Ok(())
        }

        fn delete(&self) -> Result<(), CredentialError> {
            if self.fail_delete.load(Ordering::SeqCst) {
                return Err(CredentialError::Failed);
            }
            self.deletes.fetch_add(1, Ordering::SeqCst);
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }

    struct FakeAccounts {
        account: Mutex<Option<StoreAccount>>,
        fail_load: AtomicBool,
        fail_save: AtomicBool,
        fail_clear: AtomicBool,
        saves: Mutex<Vec<StoreAccount>>,
        clears: AtomicUsize,
    }

    impl FakeAccounts {
        fn new(account: Option<StoreAccount>) -> Arc<Self> {
            Arc::new(Self {
                account: Mutex::new(account),
                fail_load: AtomicBool::new(false),
                fail_save: AtomicBool::new(false),
                fail_clear: AtomicBool::new(false),
                saves: Mutex::new(Vec::new()),
                clears: AtomicUsize::new(0),
            })
        }
    }

    impl StoreAccountStore for FakeAccounts {
        fn load(&self) -> Result<Option<StoreAccount>, StorageError> {
            if self.fail_load.load(Ordering::SeqCst) {
                Err(StorageError::Failed("test load failure".to_owned()))
            } else {
                Ok(self.account.lock().unwrap().clone())
            }
        }

        fn save(&self, account: &StoreAccount) -> Result<(), StorageError> {
            if self.fail_save.load(Ordering::SeqCst) {
                return Err(StorageError::Failed("test save failure".to_owned()));
            }
            self.saves.lock().unwrap().push(account.clone());
            *self.account.lock().unwrap() = Some(account.clone());
            Ok(())
        }

        fn clear(&self) -> Result<(), StorageError> {
            if self.fail_clear.load(Ordering::SeqCst) {
                return Err(StorageError::Failed("test clear failure".to_owned()));
            }
            self.clears.fetch_add(1, Ordering::SeqCst);
            *self.account.lock().unwrap() = None;
            Ok(())
        }
    }

    struct FakeClient {
        session: Mutex<Option<Result<SteamSession, SteamError>>>,
        sessions: Mutex<std::collections::VecDeque<Result<SteamSession, SteamError>>>,
        renew: Mutex<Option<Result<Option<SessionCredential>, SteamError>>>,
        renew_calls: AtomicUsize,
        owned: Mutex<Option<Result<Vec<SteamOwnedGame>, SteamError>>>,
        family: Mutex<Option<Result<SteamFamily, SteamError>>>,
        definitions: Mutex<
            Option<Result<Option<Vec<crate::steam::SteamAchievementDefinition>>, SteamError>>,
        >,
        states: Mutex<Option<Result<Option<Vec<crate::steam::SteamAchievementState>>, SteamError>>>,
        session_calls: AtomicUsize,
        owned_calls: AtomicUsize,
        family_calls: AtomicUsize,
        definition_calls: AtomicUsize,
        state_calls: AtomicUsize,
        started: Arc<Semaphore>,
        release: Mutex<Option<Arc<Semaphore>>>,
    }

    impl FakeClient {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                session: Mutex::new(None),
                sessions: Mutex::new(std::collections::VecDeque::new()),
                renew: Mutex::new(None),
                renew_calls: AtomicUsize::new(0),
                owned: Mutex::new(None),
                family: Mutex::new(None),
                definitions: Mutex::new(None),
                states: Mutex::new(None),
                session_calls: AtomicUsize::new(0),
                owned_calls: AtomicUsize::new(0),
                family_calls: AtomicUsize::new(0),
                definition_calls: AtomicUsize::new(0),
                state_calls: AtomicUsize::new(0),
                started: Arc::new(Semaphore::new(0)),
                release: Mutex::new(None),
            })
        }

        fn set_session(&self, value: Result<SteamSession, SteamError>) {
            *self.session.lock().unwrap() = Some(value);
        }

        fn queue_session(&self, value: Result<SteamSession, SteamError>) {
            self.sessions.lock().unwrap().push_back(value);
        }

        fn set_renew(&self, value: Result<Option<SessionCredential>, SteamError>) {
            *self.renew.lock().unwrap() = Some(value);
        }

        fn set_owned(&self, value: Result<Vec<SteamOwnedGame>, SteamError>) {
            *self.owned.lock().unwrap() = Some(value);
        }

        fn set_family(&self, value: Result<SteamFamily, SteamError>) {
            *self.family.lock().unwrap() = Some(value);
        }

        fn set_definitions(
            &self,
            value: Result<Option<Vec<crate::steam::SteamAchievementDefinition>>, SteamError>,
        ) {
            *self.definitions.lock().unwrap() = Some(value);
        }

        fn set_states(
            &self,
            value: Result<Option<Vec<crate::steam::SteamAchievementState>>, SteamError>,
        ) {
            *self.states.lock().unwrap() = Some(value);
        }

        fn hold_until(&self, release: Arc<Semaphore>) {
            *self.release.lock().unwrap() = Some(release);
        }

        async fn hold(&self) {
            let release = { self.release.lock().unwrap().clone() };
            if let Some(release) = release {
                self.started.add_permits(1);
                let _ = release.acquire().await;
            }
        }
    }

    #[async_trait]
    impl SteamWebApi for FakeClient {
        async fn session(&self, _cookie: &SessionCredential) -> Result<SteamSession, SteamError> {
            self.session_calls.fetch_add(1, Ordering::SeqCst);
            self.hold().await;
            self.sessions
                .lock()
                .unwrap()
                .pop_front()
                .or_else(|| self.session.lock().unwrap().clone())
                .unwrap_or(Err(SteamError::Unreachable))
        }

        async fn renew_session(
            &self,
            _cookie: &SessionCredential,
        ) -> Result<Option<SessionCredential>, SteamError> {
            self.renew_calls.fetch_add(1, Ordering::SeqCst);
            self.renew.lock().unwrap().clone().unwrap_or(Ok(None))
        }

        async fn owned_games(
            &self,
            _session: &SteamSession,
        ) -> Result<Vec<SteamOwnedGame>, SteamError> {
            self.owned_calls.fetch_add(1, Ordering::SeqCst);
            self.hold().await;
            self.owned.lock().unwrap().clone().unwrap_or(Ok(Vec::new()))
        }

        async fn family_games(&self, _session: &SteamSession) -> Result<SteamFamily, SteamError> {
            self.family_calls.fetch_add(1, Ordering::SeqCst);
            self.hold().await;
            self.family
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(Ok(SteamFamily::default()))
        }

        async fn achievement_definitions(
            &self,
            _session: &SteamSession,
            _app_id: &str,
            _language: &str,
        ) -> Result<Option<Vec<crate::steam::SteamAchievementDefinition>>, SteamError> {
            self.definition_calls.fetch_add(1, Ordering::SeqCst);
            self.definitions.lock().unwrap().clone().unwrap_or(Ok(None))
        }

        async fn achievement_states(
            &self,
            _session: &SteamSession,
            _app_id: &str,
            _language: &str,
        ) -> Result<Option<Vec<crate::steam::SteamAchievementState>>, SteamError> {
            self.state_calls.fetch_add(1, Ordering::SeqCst);
            self.states.lock().unwrap().clone().unwrap_or(Ok(None))
        }
    }

    struct FakeLogin {
        immediate: Mutex<VecDeque<SteamLoginOutcome>>,
        pending: Mutex<Option<oneshot::Receiver<SteamLoginOutcome>>>,
        run_calls: AtomicUsize,
        focus_calls: AtomicUsize,
        cancel_calls: AtomicUsize,
        cleanup_calls: AtomicUsize,
        started: Arc<Semaphore>,
    }

    impl FakeLogin {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                immediate: Mutex::new(VecDeque::new()),
                pending: Mutex::new(None),
                run_calls: AtomicUsize::new(0),
                focus_calls: AtomicUsize::new(0),
                cancel_calls: AtomicUsize::new(0),
                cleanup_calls: AtomicUsize::new(0),
                started: Arc::new(Semaphore::new(0)),
            })
        }

        fn set_pending(&self, receiver: oneshot::Receiver<SteamLoginOutcome>) {
            *self.pending.lock().unwrap() = Some(receiver);
        }
    }

    #[async_trait]
    impl SteamLoginWindow for FakeLogin {
        async fn run(&self) -> SteamLoginOutcome {
            self.run_calls.fetch_add(1, Ordering::SeqCst);
            self.started.add_permits(1);
            let pending = { self.pending.lock().unwrap().take() };
            if let Some(receiver) = pending {
                receiver.await.unwrap_or(SteamLoginOutcome {
                    attempt: 1,
                    result: SteamLoginResult::Failed(SteamLoginError::OpenFailed),
                })
            } else {
                self.immediate
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or(SteamLoginOutcome {
                        attempt: 1,
                        result: SteamLoginResult::Cancelled,
                    })
            }
        }

        fn focus(&self) -> Result<(), SteamLoginError> {
            self.focus_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn cancel(&self) -> Result<(), SteamLoginError> {
            self.cancel_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn cleanup_profile(&self) {
            self.cleanup_calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct FakeClock(AtomicI64);

    impl FakeClock {
        fn new(now: i64) -> Arc<Self> {
            Arc::new(Self(AtomicI64::new(now)))
        }

        fn set(&self, now: i64) {
            self.0.store(now, Ordering::SeqCst);
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    struct Harness {
        account: Arc<SteamAccount>,
        credentials: Arc<FakeCredentialStore>,
        accounts: Arc<FakeAccounts>,
        client: Arc<FakeClient>,
        login: Arc<FakeLogin>,
        clock: Arc<FakeClock>,
        states: Arc<Mutex<Vec<SteamAccountState>>>,
    }

    fn harness(credential: Option<SessionCredential>, account: Option<StoreAccount>) -> Harness {
        let credentials = FakeCredentialStore::new(credential);
        let accounts = FakeAccounts::new(account);
        let client = FakeClient::new();
        let login = FakeLogin::new();
        let clock = FakeClock::new(1_000);
        let states = Arc::new(Mutex::new(Vec::new()));
        let service = Arc::new(SteamAccount::without_observer(SteamAccountPorts {
            credentials: credentials.clone(),
            accounts: accounts.clone(),
            client: client.clone(),
            login: login.clone(),
            clock: clock.clone(),
        }));
        let sink = states.clone();
        service.set_observer(move |status| sink.lock().unwrap().push(status.state));
        Harness {
            account: service,
            credentials,
            accounts,
            client,
            login,
            clock,
            states,
        }
    }

    async fn connected(harness: &Harness) {
        let status = harness.account.startup().await;
        assert_eq!(status.state, SteamAccountState::Checking);
        let outcome = harness.account.check().await;
        assert_eq!(outcome.status.state, SteamAccountState::Connected);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn concurrent_loads_share_one_store_read() {
        let harness = harness(Some(cred("stored-cookie")), None);
        harness
            .client
            .set_session(Ok(session("A", "cassi", "stored-cookie")));
        let (release, gate) = std::sync::mpsc::channel();
        *harness.credentials.load_gate.lock().unwrap() = Some(gate);

        let account = harness.account.clone();
        let startup = tokio::spawn(async move { account.startup().await });
        for _ in 0..200 {
            if harness.credentials.loads.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(harness.credentials.loads.load(Ordering::SeqCst), 1);

        // A catalog request during startup joins the load instead of reading the stores again.
        let account = harness.account.clone();
        let retry = tokio::spawn(async move { account.load_stored().await });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(harness.credentials.loads.load(Ordering::SeqCst), 1);

        release.send(()).unwrap();
        assert_eq!(startup.await.unwrap().state, SteamAccountState::Checking);
        assert_eq!(retry.await.unwrap().state, SteamAccountState::Checking);
        assert_eq!(harness.credentials.loads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_late_load_does_not_reset_a_finished_check() {
        let harness = harness(Some(cred("stored-cookie")), None);
        harness
            .client
            .set_session(Ok(session("A", "cassi", "stored-cookie")));
        let (startup, checked) = tokio::join!(harness.account.startup(), harness.account.check());
        assert_eq!(startup.state, SteamAccountState::Checking);
        assert_eq!(checked.status.state, SteamAccountState::Connected);
        let status = harness.account.status();
        assert_eq!(status.state, SteamAccountState::Connected);
        assert_eq!(status.last_checked_at, Some(1_000));
    }

    #[tokio::test]
    async fn startup_without_a_credential_is_not_connected() {
        let harness = harness(None, Some(stored("old", "Old", 10)));
        let status = harness.account.startup().await;
        assert_eq!(status.state, SteamAccountState::NotConnected);
        assert_eq!(status.account_id, None);
        assert_eq!(status.last_checked_at, None);
        assert_eq!(harness.login.cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(harness.client.session_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_stored_credential_starts_checking_with_identity_then_connects() {
        let harness = harness(
            Some(cred("steamLoginSecure=old")),
            Some(stored("76561198000000001", "Ana", 900)),
        );
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));

        let status = harness.account.startup().await;
        assert_eq!(status.state, SteamAccountState::Checking);
        assert_eq!(status.account_name.as_deref(), Some("Ana"));
        assert_eq!(status.last_checked_at, Some(900));

        let outcome = harness.account.check().await;
        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert_eq!(outcome.status.last_checked_at, Some(1_000));
        assert!(!outcome.account_changed);
        assert!(!outcome.superseded);
        let states = harness.states.lock().unwrap();
        assert!(states.contains(&SteamAccountState::Checking));
        assert_eq!(states.last(), Some(&SteamAccountState::Connected));
        let saves = harness.accounts.saves.lock().unwrap();
        assert_eq!(saves.last().unwrap().last_verified_at, Some(1_000));
    }

    #[tokio::test]
    async fn an_unreadable_credential_is_a_credential_error() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.credentials.fail_load.store(true, Ordering::SeqCst);
        let status = harness.account.startup().await;
        assert_eq!(status.state, SteamAccountState::CredentialError);
        assert!(status.can_remove_credential());
        assert!(status.can_sign_in());
        assert!(status.can_sign_out());
        assert!(status.can_check());
    }

    #[tokio::test]
    async fn an_expired_session_removes_the_credential_and_keeps_the_identity() {
        let harness = harness(
            Some(cred("steamLoginSecure=old")),
            Some(stored("76561198000000001", "Ana", 900)),
        );
        harness.client.set_session(Err(SteamError::Expired));
        harness.account.startup().await;
        let outcome = harness.account.check().await;
        assert_eq!(outcome.status.state, SteamAccountState::Expired);
        assert_eq!(
            outcome.status.account_id.as_deref(),
            Some("76561198000000001")
        );
        assert_eq!(harness.credentials.stored(), None);
        assert_eq!(harness.credentials.deletes.load(Ordering::SeqCst), 1);
        assert!(outcome.status.can_sign_in());
        assert!(outcome.status.can_sign_out());
    }

    #[tokio::test]
    async fn an_expired_access_cookie_is_renewed_and_saved_before_the_check_continues() {
        let harness = harness(
            Some(cred(
                "steamLoginSecure=old; steamRefresh_steam=refresh-token",
            )),
            Some(stored("76561198000000001", "Ana", 900)),
        );
        harness.client.queue_session(Err(SteamError::Expired));
        harness.client.queue_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=new; steamRefresh_steam=refresh-token",
        )));
        harness.client.set_renew(Ok(Some(cred(
            "steamLoginSecure=new; steamRefresh_steam=refresh-token",
        ))));

        harness.account.startup().await;
        let outcome = harness.account.check().await;

        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert_eq!(harness.client.renew_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            harness.credentials.stored().as_deref(),
            Some("steamLoginSecure=new; steamRefresh_steam=refresh-token")
        );
        assert_eq!(harness.credentials.deletes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_renewal_is_saved_even_when_the_store_page_still_fails() {
        let harness = harness(
            Some(cred(
                "steamLoginSecure=old; steamRefresh_steam=refresh-token",
            )),
            None,
        );
        harness.client.queue_session(Err(SteamError::Expired));
        harness.client.queue_session(Err(SteamError::Expired));
        harness.client.set_renew(Ok(Some(cred(
            "steamLoginSecure=new; steamRefresh_steam=rotated-refresh",
        ))));

        harness.account.startup().await;
        let outcome = harness.account.check().await;

        assert_eq!(outcome.status.state, SteamAccountState::Unreachable);
        // The refresh token was accepted, so a store page that still refuses the new access
        // cookie must not discard the long-lived credential.
        assert_eq!(
            harness.credentials.stored().as_deref(),
            Some("steamLoginSecure=new; steamRefresh_steam=rotated-refresh")
        );
        assert_eq!(harness.credentials.deletes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn an_unreachable_store_keeps_the_credential() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Err(SteamError::Unreachable));
        harness.account.startup().await;
        let outcome = harness.account.check().await;
        assert_eq!(outcome.status.state, SteamAccountState::Unreachable);
        assert_eq!(
            harness.credentials.stored().as_deref(),
            Some("steamLoginSecure=old")
        );
        assert_eq!(harness.credentials.deletes.load(Ordering::SeqCst), 0);
        assert!(outcome.status.can_check());
    }

    #[tokio::test]
    async fn a_changed_store_response_keeps_the_credential() {
        // "Steam changed its response" is not an authentication failure: the saved cookie must
        // survive so the next check can use it once Steam serves the expected page again.
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Err(SteamError::Changed));
        harness.account.startup().await;
        let outcome = harness.account.check().await;
        assert_eq!(outcome.status.state, SteamAccountState::Unreachable);
        assert_eq!(
            harness.credentials.stored().as_deref(),
            Some("steamLoginSecure=old")
        );
        assert_eq!(harness.credentials.deletes.load(Ordering::SeqCst), 0);
        assert!(outcome.status.can_check());
        assert!(outcome.status.can_sign_in());
    }

    #[tokio::test]
    async fn a_rotated_cookie_replaces_the_stored_one() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=new; sessionid=abc",
        )));
        harness.account.startup().await;
        let outcome = harness.account.check().await;
        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert_eq!(
            harness.credentials.stored().as_deref(),
            Some("steamLoginSecure=new; sessionid=abc")
        );
        assert!(
            harness
                .credentials
                .saved
                .lock()
                .unwrap()
                .contains(&"steamLoginSecure=new; sessionid=abc".to_owned())
        );
    }

    #[tokio::test]
    async fn a_failed_credential_save_keeps_the_account_connected_for_the_run() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.credentials.fail_save.store(true, Ordering::SeqCst);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=new",
        )));
        harness.account.startup().await;
        let outcome = harness.account.check().await;
        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert!(outcome.status.persist_failed);
    }

    #[tokio::test]
    async fn a_failed_account_save_reports_persistence_failure() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.accounts.fail_save.store(true, Ordering::SeqCst);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        harness.account.startup().await;
        let outcome = harness.account.check().await;
        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert!(outcome.status.persist_failed);
    }

    #[tokio::test]
    async fn a_missing_account_name_keeps_the_last_known_one() {
        let harness = harness(
            Some(cred("steamLoginSecure=old")),
            Some(stored("76561198000000001", "Ana", 900)),
        );
        harness.client.set_session(Ok(SteamSession {
            steam_id: "76561198000000001".to_owned(),
            account_name: None,
            token: cred("webapi-token"),
            cookie: cred("steamLoginSecure=old"),
        }));
        harness.account.startup().await;
        let outcome = harness.account.check().await;
        assert_eq!(outcome.status.account_name.as_deref(), Some("Ana"));
        assert_eq!(
            harness
                .accounts
                .saves
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .account_name
                .as_deref(),
            Some("Ana")
        );
    }

    #[tokio::test]
    async fn a_different_account_is_reported_as_an_account_change() {
        let harness = harness(
            Some(cred("steamLoginSecure=old")),
            Some(stored("76561198000000001", "Ana", 900)),
        );
        harness.client.set_session(Ok(session(
            "76561198000000002",
            "Bia",
            "steamLoginSecure=old",
        )));
        harness.account.startup().await;
        let outcome = harness.account.check().await;
        assert!(outcome.account_changed);
        assert_eq!(outcome.status.account_name.as_deref(), Some("Bia"));
        assert_eq!(
            harness
                .accounts
                .account
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .account_id,
            "76561198000000002"
        );
    }

    #[tokio::test]
    async fn the_first_check_is_not_an_account_change() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        harness.account.startup().await;
        let outcome = harness.account.check().await;
        assert!(!outcome.account_changed);
    }

    #[tokio::test]
    async fn checks_coalesce_into_one_request() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        harness.account.startup().await;
        let release = Arc::new(Semaphore::new(0));
        harness.client.hold_until(release.clone());

        let first = {
            let account = harness.account.clone();
            tokio::spawn(async move { account.check().await })
        };
        let _permit = harness.client.started.acquire().await.unwrap();
        let second = {
            let account = harness.account.clone();
            tokio::spawn(async move { account.check().await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        release.add_permits(1);

        let first = first.await.unwrap();
        let second = second.await.unwrap();
        assert_eq!(harness.client.session_calls.load(Ordering::SeqCst), 1);
        assert_eq!(first.status.state, SteamAccountState::Connected);
        assert_eq!(second.status.state, SteamAccountState::Connected);
        assert!(!first.superseded);
        assert!(!second.superseded);
    }

    #[tokio::test]
    async fn syncs_coalesce_into_one_request() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        harness.account.startup().await;
        let release = Arc::new(Semaphore::new(0));
        harness.client.hold_until(release.clone());

        let first = {
            let account = harness.account.clone();
            tokio::spawn(async move { account.sync().await })
        };
        let _permit = harness.client.started.acquire().await.unwrap();
        let second = {
            let account = harness.account.clone();
            tokio::spawn(async move { account.sync().await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        release.add_permits(1);

        let first = first.await.unwrap();
        let second = second.await.unwrap();
        assert_eq!(harness.client.session_calls.load(Ordering::SeqCst), 1);
        assert_eq!(harness.client.owned_calls.load(Ordering::SeqCst), 1);
        assert!(matches!(first.owned, Some(Ok(_))));
        assert!(matches!(second.owned, Some(Ok(_))));
    }

    #[tokio::test]
    async fn a_sync_without_a_credential_leaves_the_catalog_alone() {
        let harness = harness(None, None);
        harness.account.startup().await;
        let outcome = harness.account.sync().await;
        assert_eq!(outcome.status.state, SteamAccountState::NotConnected);
        assert!(outcome.owned.is_none());
        assert!(outcome.shared.is_none());
        assert_eq!(harness.client.session_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_sync_returns_owned_and_family_records_and_records_the_time() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        harness.client.set_owned(Ok(vec![SteamOwnedGame {
            app_id: "620".to_owned(),
            name: "Portal 2".to_owned(),
        }]));
        harness.client.set_family(Ok(SteamFamily {
            group_id: Some("42".to_owned()),
            games: vec![SteamOwnedGame {
                app_id: "400".to_owned(),
                name: "Portal".to_owned(),
            }],
        }));
        harness.account.startup().await;
        harness.clock.set(2_000);
        let outcome = harness.account.sync().await;

        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert_eq!(outcome.now, 2_000);
        assert_eq!(outcome.status.last_synced_at, Some(2_000));
        assert_eq!(
            outcome.owned,
            Some(Ok(vec![CatalogRecord::new("620", "Portal 2")]))
        );
        assert_eq!(
            outcome.shared,
            Some(Ok(vec![CatalogRecord::new("400", "Portal")]))
        );
        assert_eq!(outcome.status.shared_error, None);
    }

    #[tokio::test]
    async fn a_family_failure_does_not_fail_the_owned_catalog() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        harness.client.set_owned(Ok(vec![SteamOwnedGame {
            app_id: "620".to_owned(),
            name: "Portal 2".to_owned(),
        }]));
        harness.client.set_family(Err(SteamError::Server));
        harness.account.startup().await;
        let outcome = harness.account.sync().await;

        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert!(matches!(outcome.owned, Some(Ok(_))));
        assert_eq!(outcome.shared, Some(Err(CatalogError::SharedUnavailable)));
        assert_eq!(
            outcome.status.shared_error,
            Some(CatalogError::SharedUnavailable)
        );
        assert_eq!(outcome.status.catalog_error, None);
    }

    #[tokio::test]
    async fn a_private_library_stays_connected_with_a_catalog_error() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        harness.client.set_owned(Err(SteamError::Privacy));
        harness.account.startup().await;
        let outcome = harness.account.sync().await;

        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert_eq!(outcome.owned, Some(Err(CatalogError::Privacy)));
        assert_eq!(outcome.status.catalog_error, Some(CatalogError::Privacy));
        assert_eq!(harness.client.family_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_expired_sync_removes_the_credential() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Err(SteamError::Expired));
        harness.account.startup().await;
        let outcome = harness.account.sync().await;
        assert_eq!(outcome.status.state, SteamAccountState::Expired);
        assert_eq!(outcome.owned, Some(Err(CatalogError::Expired)));
        assert!(outcome.shared.is_none());
        assert_eq!(harness.credentials.stored(), None);
    }

    #[tokio::test]
    async fn a_rejected_catalog_token_keeps_the_long_lived_credential() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        harness.client.set_owned(Err(SteamError::Expired));
        harness.account.startup().await;
        let outcome = harness.account.sync().await;

        assert_eq!(outcome.status.state, SteamAccountState::Expired);
        assert_eq!(outcome.owned, Some(Err(CatalogError::Expired)));
        // The store session validated, so the token rejection must not discard the refresh
        // cookie; the next check can mint a new access token from it.
        assert_eq!(harness.credentials.deletes.load(Ordering::SeqCst), 0);
        assert_eq!(
            harness.credentials.stored().as_deref(),
            Some("steamLoginSecure=old")
        );
    }

    #[tokio::test]
    async fn sign_in_saves_the_cookie_and_connects() {
        let harness = harness(None, None);
        harness
            .login
            .immediate
            .lock()
            .unwrap()
            .push_back(SteamLoginOutcome {
                attempt: 1,
                result: SteamLoginResult::Verified {
                    session: session(
                        "76561198000000001",
                        "Ana",
                        "steamLoginSecure=abc; sessionid=def",
                    ),
                },
            });

        let outcome = harness.account.sign_in().await;
        assert_eq!(outcome.result, SteamSignInResult::Connected);
        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert_eq!(
            harness.credentials.stored().as_deref(),
            Some("steamLoginSecure=abc; sessionid=def")
        );
        assert_eq!(
            harness
                .accounts
                .account
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .account_name
                .as_deref(),
            Some("Ana")
        );
        assert!(!outcome.status.persist_failed);
    }

    #[tokio::test]
    async fn a_cancelled_sign_in_restores_the_previous_state() {
        let harness = harness(None, None);
        harness
            .login
            .immediate
            .lock()
            .unwrap()
            .push_back(SteamLoginOutcome {
                attempt: 1,
                result: SteamLoginResult::Cancelled,
            });
        let outcome = harness.account.sign_in().await;
        assert_eq!(outcome.result, SteamSignInResult::Cancelled);
        assert_eq!(outcome.status.state, SteamAccountState::NotConnected);
        assert_eq!(harness.credentials.stored(), None);
    }

    #[tokio::test]
    async fn cancelling_a_re_sign_in_returns_to_expired() {
        let harness = harness(
            Some(cred("steamLoginSecure=old")),
            Some(stored("76561198000000001", "Ana", 900)),
        );
        harness.client.set_session(Err(SteamError::Expired));
        harness.account.startup().await;
        let expired = harness.account.check().await;
        assert_eq!(expired.status.state, SteamAccountState::Expired);

        harness
            .login
            .immediate
            .lock()
            .unwrap()
            .push_back(SteamLoginOutcome {
                attempt: 1,
                result: SteamLoginResult::Cancelled,
            });
        let outcome = harness.account.sign_in().await;
        assert_eq!(outcome.result, SteamSignInResult::Cancelled);
        assert_eq!(outcome.status.state, SteamAccountState::Expired);
    }

    #[tokio::test]
    async fn a_check_retries_an_unreadable_credential() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        harness.credentials.fail_load.store(true, Ordering::SeqCst);
        let status = harness.account.startup().await;
        assert_eq!(status.state, SteamAccountState::CredentialError);

        harness.credentials.fail_load.store(false, Ordering::SeqCst);
        let outcome = harness.account.check().await;
        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert_eq!(harness.client.session_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_closed_window_restores_the_previous_state() {
        let harness = harness(None, None);
        harness
            .login
            .immediate
            .lock()
            .unwrap()
            .push_back(SteamLoginOutcome {
                attempt: 1,
                result: SteamLoginResult::Closed,
            });
        let outcome = harness.account.sign_in().await;
        assert_eq!(outcome.result, SteamSignInResult::Closed);
        assert_eq!(outcome.status.state, SteamAccountState::NotConnected);
    }

    #[tokio::test]
    async fn a_sign_in_timeout_restores_the_previous_state() {
        let harness = harness(None, None);
        harness
            .login
            .immediate
            .lock()
            .unwrap()
            .push_back(SteamLoginOutcome {
                attempt: 1,
                result: SteamLoginResult::TimedOut,
            });
        let outcome = harness.account.sign_in().await;
        assert_eq!(outcome.result, SteamSignInResult::TimedOut);
        assert_eq!(outcome.status.state, SteamAccountState::NotConnected);
    }

    #[tokio::test]
    async fn a_sign_in_save_failure_keeps_the_account_connected_for_the_run() {
        let harness = harness(None, None);
        harness.credentials.fail_save.store(true, Ordering::SeqCst);
        harness
            .login
            .immediate
            .lock()
            .unwrap()
            .push_back(SteamLoginOutcome {
                attempt: 1,
                result: SteamLoginResult::Verified {
                    session: session("76561198000000001", "Ana", "steamLoginSecure=abc"),
                },
            });
        let outcome = harness.account.sign_in().await;
        assert_eq!(outcome.result, SteamSignInResult::Connected);
        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert!(outcome.status.persist_failed);
    }

    #[tokio::test]
    async fn a_background_check_does_not_interrupt_the_sign_in_window() {
        let harness = harness(None, None);
        let (sender, receiver) = oneshot::channel();
        harness.login.set_pending(receiver);
        let task = {
            let account = harness.account.clone();
            tokio::spawn(async move { account.sign_in().await })
        };
        let _permit = harness.login.started.acquire().await.unwrap();

        let outcome = harness.account.check().await;
        assert!(outcome.superseded);
        assert_eq!(outcome.status.state, SteamAccountState::SigningIn);
        assert_eq!(harness.client.session_calls.load(Ordering::SeqCst), 0);

        let status = harness.account.cancel_sign_in();
        assert_eq!(status.state, SteamAccountState::NotConnected);
        drop(sender);
        let sign_in = task.await.unwrap();
        assert_eq!(sign_in.result, SteamSignInResult::Superseded);
        assert_eq!(sign_in.status.state, SteamAccountState::NotConnected);
    }

    #[tokio::test]
    async fn a_late_sign_in_result_after_sign_out_is_ignored() {
        let harness = harness(None, None);
        let (sender, receiver) = oneshot::channel();
        harness.login.set_pending(receiver);
        let task = {
            let account = harness.account.clone();
            tokio::spawn(async move { account.sign_in().await })
        };
        let _permit = harness.login.started.acquire().await.unwrap();

        let signed_out = harness.account.sign_out().await;
        assert!(signed_out.signed_out);
        assert_eq!(signed_out.status.state, SteamAccountState::NotConnected);

        sender
            .send(SteamLoginOutcome {
                attempt: 1,
                result: SteamLoginResult::Verified {
                    session: session("76561198000000001", "Ana", "steamLoginSecure=late"),
                },
            })
            .unwrap();
        let outcome = task.await.unwrap();
        assert_eq!(outcome.result, SteamSignInResult::Superseded);
        assert_eq!(outcome.status.state, SteamAccountState::NotConnected);
        assert_eq!(harness.credentials.stored(), None);
        assert_eq!(harness.accounts.clears.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn sign_out_deletes_the_credential_and_clears_the_row() {
        let harness = harness(
            Some(cred("steamLoginSecure=old")),
            Some(stored("76561198000000001", "Ana", 900)),
        );
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        connected(&harness).await;

        let outcome = harness.account.sign_out().await;
        assert!(outcome.signed_out);
        assert_eq!(outcome.status.state, SteamAccountState::NotConnected);
        assert_eq!(outcome.status.account_id, None);
        assert_eq!(harness.credentials.stored(), None);
        assert!(harness.accounts.account.lock().unwrap().is_none());
        assert_eq!(harness.accounts.clears.load(Ordering::SeqCst), 1);
        assert_eq!(harness.login.cancel_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_account_clear_still_signs_out_but_reports_the_failure() {
        let harness = harness(
            Some(cred("steamLoginSecure=old")),
            Some(stored("76561198000000001", "Ana", 900)),
        );
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        connected(&harness).await;
        harness.accounts.fail_clear.store(true, Ordering::SeqCst);

        let outcome = harness.account.sign_out().await;
        assert!(outcome.signed_out);
        assert_eq!(outcome.status.state, SteamAccountState::NotConnected);
        assert!(outcome.status.persist_failed);
        assert_eq!(harness.credentials.stored(), None);
    }

    #[test]
    fn the_action_matrix_follows_the_state() {
        let check = |state| SteamAccountStatus {
            state,
            account_id: None,
            account_name: None,
            last_checked_at: None,
            last_synced_at: None,
            storage: StorageMethod::SecretService,
            persist_failed: false,
            catalog_error: None,
            shared_error: None,
        };
        let connected = check(SteamAccountState::Connected);
        assert!(connected.can_check());
        assert!(connected.can_sign_out());
        assert!(!connected.can_sign_in());
        let signing_in = check(SteamAccountState::SigningIn);
        assert!(signing_in.can_focus_sign_in());
        assert!(signing_in.can_cancel_sign_in());
        assert!(!signing_in.can_sign_out());
        let expired = check(SteamAccountState::Expired);
        assert!(expired.can_sign_in());
        assert!(!expired.can_check());
        assert!(expired.can_sign_out());
        let credential_error = check(SteamAccountState::CredentialError);
        assert!(credential_error.can_remove_credential());
        assert!(credential_error.can_check());
    }

    #[tokio::test]
    async fn a_failed_sign_out_delete_keeps_the_account_connected() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        connected(&harness).await;
        harness
            .credentials
            .fail_delete
            .store(true, Ordering::SeqCst);

        let outcome = harness.account.sign_out().await;
        assert!(!outcome.signed_out);
        assert_eq!(outcome.status.state, SteamAccountState::Connected);
        assert!(outcome.status.persist_failed);
        assert!(harness.credentials.stored().is_some());
    }

    #[tokio::test]
    async fn removing_an_unreadable_credential_recovers_the_card() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.credentials.fail_load.store(true, Ordering::SeqCst);
        let status = harness.account.startup().await;
        assert_eq!(status.state, SteamAccountState::CredentialError);

        let outcome = harness.account.remove_credential().await;
        assert!(outcome.signed_out);
        assert_eq!(outcome.status.state, SteamAccountState::NotConnected);
        assert_eq!(harness.credentials.stored(), None);
    }

    #[tokio::test]
    async fn focus_and_cancel_reach_the_window_and_a_late_result_is_superseded() {
        let harness = harness(None, None);
        let (sender, receiver) = oneshot::channel();
        harness.login.set_pending(receiver);
        let task = {
            let account = harness.account.clone();
            tokio::spawn(async move { account.sign_in().await })
        };
        let _permit = harness.login.started.acquire().await.unwrap();

        assert!(harness.account.focus_sign_in());
        assert_eq!(harness.login.focus_calls.load(Ordering::SeqCst), 1);

        // A second sign-in request while the first window is open does not open a second one.
        let second = harness.account.sign_in().await;
        assert_eq!(second.result, SteamSignInResult::Superseded);
        assert_eq!(harness.login.run_calls.load(Ordering::SeqCst), 1);

        let status = harness.account.cancel_sign_in();
        assert_eq!(status.state, SteamAccountState::NotConnected);
        assert_eq!(harness.login.cancel_calls.load(Ordering::SeqCst), 1);

        drop(sender);
        let outcome = task.await.unwrap();
        assert_eq!(outcome.result, SteamSignInResult::Superseded);
    }

    #[tokio::test]
    async fn a_superseded_check_after_sign_out_is_ignored() {
        let harness = harness(Some(cred("steamLoginSecure=old")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=old",
        )));
        harness.account.startup().await;
        let release = Arc::new(Semaphore::new(0));
        harness.client.hold_until(release.clone());

        let check = {
            let account = harness.account.clone();
            tokio::spawn(async move { account.check().await })
        };
        let _permit = harness.client.started.acquire().await.unwrap();
        let signed_out = harness.account.sign_out().await;
        assert!(signed_out.signed_out);
        release.add_permits(1);

        let outcome = check.await.unwrap();
        assert!(outcome.superseded);
        assert_eq!(outcome.status.state, SteamAccountState::NotConnected);
        assert_eq!(harness.credentials.stored(), None);
    }

    #[tokio::test]
    async fn the_status_never_contains_the_cookie() {
        let harness = harness(Some(cred("steamLoginSecure=super-secret")), None);
        harness.client.set_session(Ok(session(
            "76561198000000001",
            "Ana",
            "steamLoginSecure=super-secret",
        )));
        connected(&harness).await;
        let debug = format!("{:?}", harness.account.status());
        assert!(debug.contains("Connected"));
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("webapi-token"));
    }
    #[tokio::test]
    async fn delayed_startup_does_not_reload_an_expired_session_after_its_credential_was_removed() {
        let harness = harness(Some(cred("stored-cookie")), Some(stored("A", "player", 10)));
        harness.client.set_session(Err(SteamError::Expired));
        harness.account.load_stored().await;
        assert_eq!(
            harness.account.check().await.status.state,
            SteamAccountState::Expired
        );
        let loads = harness.credentials.loads.load(Ordering::SeqCst);
        assert_eq!(
            harness.account.startup().await.state,
            SteamAccountState::Expired
        );
        assert_eq!(harness.credentials.loads.load(Ordering::SeqCst), loads);
    }
    #[tokio::test]
    async fn background_check_and_sync_keep_expiry_after_the_rejected_cookie_is_deleted() {
        let harness = harness(Some(cred("stored-cookie")), Some(stored("A", "player", 10)));
        harness.client.set_session(Err(SteamError::Expired));
        harness.account.startup().await;
        assert_eq!(
            harness.account.check().await.status.state,
            SteamAccountState::Expired
        );
        assert!(harness.credentials.stored().is_none());
        assert_eq!(
            harness.account.check().await.status.state,
            SteamAccountState::Expired
        );
        assert_eq!(
            harness.account.sync().await.status.state,
            SteamAccountState::Expired
        );
    }

    fn definitions() -> Vec<crate::steam::SteamAchievementDefinition> {
        vec![
            crate::steam::SteamAchievementDefinition {
                api_name: "ACH_WIN".into(),
                display_name: "Win".into(),
                description: "Win once".into(),
                hidden: false,
                icon: "https://cdn.example/a.jpg".into(),
                icon_gray: "https://cdn.example/a_gray.jpg".into(),
            },
            crate::steam::SteamAchievementDefinition {
                api_name: "ACH_SECRET".into(),
                display_name: "Secret".into(),
                description: "A secret".into(),
                hidden: true,
                icon: String::new(),
                icon_gray: String::new(),
            },
        ]
    }

    fn states() -> Vec<crate::steam::SteamAchievementState> {
        vec![
            crate::steam::SteamAchievementState {
                api_name: "ACH_WIN".into(),
                achieved: true,
                unlock_time: Some(900),
                display_name: "Vitória".into(),
                description: "Vença uma vez".into(),
            },
            crate::steam::SteamAchievementState {
                api_name: "ACH_SECRET".into(),
                achieved: true,
                unlock_time: None,
                display_name: String::new(),
                description: String::new(),
            },
        ]
    }

    #[tokio::test]
    async fn achievements_join_definitions_and_unlocks_by_api_name() {
        let harness = harness(Some(cred("stored-cookie")), None);
        harness
            .client
            .set_session(Ok(session("A", "cassi", "stored-cookie")));
        harness.client.set_definitions(Ok(Some(definitions())));
        harness.client.set_states(Ok(Some(states())));
        harness.account.startup().await;
        let outcome = harness
            .account
            .achievements(&["620".to_owned()], "pt-BR")
            .await;
        assert!(!outcome.superseded);
        let Ok(Some(achievements)) = &outcome.outcomes[0].1 else {
            panic!("expected ready");
        };
        assert_eq!(achievements.len(), 2);
        assert_eq!(achievements[0].name, "Vitória");
        assert!(achievements[0].unlocked);
        assert_eq!(achievements[0].unlocked_at, Some(900));
        assert!(achievements[1].hidden, "the schema hidden flag is kept");
        assert!(achievements[1].unlocked);
        assert_eq!(
            achievements[1].unlocked_at, None,
            "an unlock without a time stays unlocked with no date"
        );
        assert_eq!(harness.client.definition_calls.load(Ordering::SeqCst), 1);
        assert_eq!(harness.client.state_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_partial_achievement_failure_is_a_failed_outcome() {
        let harness = harness(Some(cred("stored-cookie")), None);
        harness
            .client
            .set_session(Ok(session("A", "cassi", "stored-cookie")));
        harness.client.set_definitions(Ok(Some(definitions())));
        harness.client.set_states(Err(SteamError::Privacy));
        harness.account.startup().await;
        let outcome = harness
            .account
            .achievements(&["620".to_owned()], "en")
            .await;
        assert_eq!(
            outcome.outcomes[0].1,
            Err(CatalogError::Privacy),
            "definitions alone must not become a ready result"
        );
    }

    #[tokio::test]
    async fn a_renewed_achievement_session_cookie_is_persisted() {
        let harness = harness(
            Some(cred("steamLoginSecure=old; steamRefresh_steam=refresh")),
            None,
        );
        harness.client.set_session(Ok(session(
            "A",
            "cassi",
            "steamLoginSecure=new; steamRefresh_steam=refresh",
        )));
        harness.client.set_definitions(Ok(None));
        harness.account.startup().await;

        let outcome = harness
            .account
            .achievements(&["620".to_owned()], "en")
            .await;

        assert_eq!(outcome.outcomes[0].1, Ok(None));
        assert_eq!(
            harness.credentials.stored().as_deref(),
            Some("steamLoginSecure=new; steamRefresh_steam=refresh"),
            "the achievement read must not discard a rotated session cookie"
        );
    }

    #[tokio::test]
    async fn no_connected_account_makes_no_achievement_request() {
        let harness = harness(None, None);
        harness.account.startup().await;
        let outcome = harness
            .account
            .achievements(&["620".to_owned()], "en")
            .await;
        assert!(outcome.outcomes.is_empty());
        assert_eq!(harness.client.session_calls.load(Ordering::SeqCst), 0);
        assert_eq!(harness.client.definition_calls.load(Ordering::SeqCst), 0);
        assert_eq!(harness.client.state_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn an_expired_session_fails_every_game_and_stops_on_expiry() {
        let harness = harness(Some(cred("stored-cookie")), None);
        harness.client.set_session(Err(SteamError::Expired));
        harness.account.startup().await;
        let outcome = harness
            .account
            .achievements(&["620".to_owned(), "70".to_owned()], "en")
            .await;
        assert_eq!(outcome.status.state, SteamAccountState::Expired);
        assert_eq!(outcome.outcomes.len(), 2);
        assert!(
            outcome
                .outcomes
                .iter()
                .all(|(_, result)| *result == Err(CatalogError::Expired))
        );
        assert_eq!(harness.credentials.stored(), None);
    }

    #[tokio::test]
    async fn achievements_do_not_depend_on_local_steam_stats_files() {
        // The achievement path has no file port and must never reach Steam's per-account
        // appcache/stats files; this source check fails if a future edit adds one.
        let source = include_str!("account.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default();
        assert!(!source.contains(concat!("app", "cache")));
        let harness = harness(Some(cred("stored-cookie")), None);
        harness
            .client
            .set_session(Ok(session("A", "cassi", "stored-cookie")));
        harness.client.set_definitions(Ok(Some(definitions())));
        harness.client.set_states(Ok(Some(states())));
        harness.account.startup().await;
        assert!(
            harness
                .account
                .achievements(&["620".to_owned()], "en")
                .await
                .outcomes[0]
                .1
                .is_ok()
        );
    }

    #[test]
    fn locales_map_to_steam_languages() {
        assert_eq!(steam_language("en"), "english");
        assert_eq!(steam_language("en-US"), "english");
        assert_eq!(steam_language("pt-BR"), "brazilian");
        assert_eq!(steam_language("pt_BR"), "brazilian");
        assert_eq!(steam_language("pt"), "brazilian");
        assert_eq!(steam_language("fr-FR"), "english");
    }

    #[tokio::test]
    async fn a_cancelled_achievement_read_returns_no_outcomes_and_makes_no_request() {
        let harness = harness(Some(cred("stored-cookie")), None);
        harness
            .client
            .set_session(Ok(session("A", "cassi", "stored-cookie")));
        harness.client.set_definitions(Ok(Some(definitions())));
        harness.client.set_states(Ok(Some(states())));
        harness.account.startup().await;
        let scoped = harness.account.for_operation(
            harness.account.ports.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        let outcome = scoped.achievements(&["620".to_owned()], "en").await;
        assert!(outcome.superseded);
        assert!(outcome.outcomes.is_empty());
        assert_eq!(harness.client.definition_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn achievement_text_with_tabs_and_c1_controls_is_sanitized() {
        let harness = harness(Some(cred("stored-cookie")), None);
        harness
            .client
            .set_session(Ok(session("A", "cassi", "stored-cookie")));
        harness.client.set_definitions(Ok(Some(vec![
            crate::steam::SteamAchievementDefinition {
                api_name: "ACH_CONTROL".into(),
                display_name: "Alpha\tBeta\u{85}Gamma".into(),
                description: "Line\tone\u{85}\u{85}two ".into(),
                hidden: false,
                icon: String::new(),
                icon_gray: String::new(),
            },
            crate::steam::SteamAchievementDefinition {
                api_name: "ACH_DEF".into(),
                display_name: "Definition\tname".into(),
                description: "Definition\u{85}description".into(),
                hidden: false,
                icon: String::new(),
                icon_gray: String::new(),
            },
        ])));
        harness
            .client
            .set_states(Ok(Some(vec![crate::steam::SteamAchievementState {
                api_name: "ACH_CONTROL".into(),
                achieved: false,
                unlock_time: None,
                display_name: "Player\tname\u{85}".into(),
                description: "Player\tdescription\u{85}".into(),
            }])));
        harness.account.startup().await;
        let outcome = harness
            .account
            .achievements(&["620".to_owned()], "en")
            .await;
        let Ok(Some(achievements)) = &outcome.outcomes[0].1 else {
            panic!("expected a ready outcome");
        };
        assert_eq!(achievements[0].name, "Player name");
        assert_eq!(achievements[0].description, "Player description");
        assert_eq!(achievements[1].name, "Definition name");
        assert_eq!(achievements[1].description, "Definition description");
    }
}
