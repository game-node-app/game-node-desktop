//! Application composition: runs `core`'s effects against the platform and API adapters.
//!
//! The UI thread only calls [`Engine::dispatch`] (cheap, never blocks on I/O) and renders the
//! [`Snapshot`]s passed to the publish callback. HTTP and browser work runs on Tokio; keyring,
//! file and database work runs on one FIFO storage thread, so a credential write can never be
//! reordered after the sign-out that follows it.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gamenode_core::auth::{AuthState, NonceSource, Purpose, RemoteSignOut};
use gamenode_core::game_api::{ApiError, GameApi};
use gamenode_core::library::{
    Access, Effect as LibEffect, Input as LibInput, LastAccount, LaunchKind, LaunchState,
    LaunchStatus, LibraryMachine, LibraryStore, ScanOutcome, open_action,
};
use gamenode_core::ports::{
    AuthApi, BrowserOpener, CredentialStore, HostFiles, RootResolver, SettingsStore, StorageError,
    UriOpener,
};
use gamenode_core::provider::Platform;
use gamenode_core::{
    AuthMachine, Effect, Input, SessionCredential, Settings, SignInFailure, Snapshot,
};
use gamenode_platform::covers::write_cover;
use tokio::runtime::Handle;

mod artwork;
mod drm_free;
mod playtime;

/// Longest the app waits for the server to confirm a sign-out.
const REMOTE_SIGN_OUT_TIMEOUT: Duration = Duration::from_secs(5);

/// The adapters the engine drives.
#[derive(Clone)]
pub struct Ports {
    pub backups: Option<Arc<dyn gamenode_core::drm_free::BackupEngine>>,
    pub local_games: Option<Arc<dyn gamenode_core::drm_free::LocalGameStore>>,
    pub installations: Option<Arc<dyn gamenode_core::drm_free::InstallationHost>>,
    pub processes: Arc<dyn gamenode_core::ports::Processes>,
    pub playtime: Arc<dyn gamenode_core::ports::PlaytimeStore>,
    pub playtime_api: Arc<dyn gamenode_core::ports::PlaytimeApi>,
    pub settings: Arc<dyn SettingsStore>,
    pub credentials: Arc<dyn CredentialStore>,
    pub api: Arc<dyn AuthApi>,
    pub browser: Arc<dyn BrowserOpener>,
    pub library: Arc<dyn LibraryStore>,
    pub games: Arc<dyn GameApi>,
    pub files: Arc<dyn HostFiles>,
    pub roots: Arc<dyn RootResolver>,
    pub uri: Arc<dyn UriOpener>,
    /// Where downloaded covers are stored.
    pub covers_dir: PathBuf,
    pub platform: Platform,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

type Job = Box<dyn FnOnce() + Send>;

/// Runs blocking storage jobs one at a time, in submission order.
struct StorageWorker {
    jobs: mpsc::Sender<Job>,
}

impl StorageWorker {
    fn new() -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("gamenode-storage".into())
            .spawn(move || {
                while let Ok(job) = queue.recv() {
                    job();
                }
            })
            .expect("storage thread");
        Self { jobs }
    }

    fn submit(&self, job: impl FnOnce() + Send + 'static) {
        // The worker only stops when the engine is dropped.
        let _ = self.jobs.send(Box::new(job));
    }
}

struct State {
    local_backup_busy: bool,
    local: gamenode_core::drm_free::LocalView,
    playtime: playtime::Playtime,
    machine: AuthMachine,
    settings: Settings,
    library: LibraryMachine,
    /// The account record on disk, so the cache can open before the session is verified.
    last_account: Option<LastAccount>,
    /// The latest Play or Install; `launch_seq` tells its answer from an older one.
    launch: Option<LaunchStatus>,
    launch_seq: u64,
}

struct Shared {
    state: Mutex<State>,
    ports: Ports,
    runtime: Handle,
    storage: StorageWorker,
    publish: Box<dyn Fn(Snapshot) + Send + Sync>,
}

/// Cheap to clone; all clones share one state.
#[derive(Clone)]
pub struct Engine {
    shared: Arc<Shared>,
}

impl Engine {
    /// `publish` is called with every new snapshot, from arbitrary threads, while the engine's
    /// lock is held: it must hand the snapshot off (e.g. `slint::invoke_from_event_loop`) and
    /// must not call back into the engine.
    pub fn new(
        ports: Ports,
        runtime: Handle,
        settings: Settings,
        library: LibraryMachine,
        nonce: NonceSource,
        publish: impl Fn(Snapshot) + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    local_backup_busy: false,
                    local: Default::default(),
                    playtime: playtime::Playtime::new(ports.platform),
                    machine: AuthMachine::new(nonce),
                    settings,
                    library,
                    last_account: None,
                    launch: None,
                    launch_seq: 0,
                }),
                ports,
                runtime,
                storage: StorageWorker::new(),
                publish: Box::new(publish),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut state = self.lock();
        self.snapshot_of(&mut state)
    }

    fn snapshot_of(&self, state: &mut State) -> Snapshot {
        self.update_playtime_view(state, unix_now());
        let library = state.library.view();
        let mut snapshot = Snapshot::new(
            &state.machine,
            self.shared.ports.credentials.method(),
            state.settings,
            library,
            state.launch.clone(),
        );
        snapshot.local_games = state.local.clone();
        snapshot.playtime_error = state.playtime.error;
        snapshot.sync = state.playtime.preferences;
        snapshot.playtime_source = state.playtime.source;
        snapshot.tracking = gamenode_core::snapshot::TrackingView {
            title: state
                .library
                .all_rows()
                .into_iter()
                .find(|r| r.running)
                .map(|r| r.title)
                .unwrap_or_default(),
            available: state.playtime.available,
            running: state.playtime.tracker.running(),
        };
        snapshot
    }

    fn publish(&self, state: &mut State) {
        (self.shared.publish)(self.snapshot_of(state));
    }

    /// Reads the last signed-in account, then the stored session and, if there is one,
    /// validates it. The account is read first so the cached library can open while the session
    /// is still being verified.
    pub fn start(&self) {
        self.refresh_local_games();
        let (engine, store) = (self.clone(), self.shared.ports.library.clone());
        self.shared.storage.submit(move || {
            if let Err(error) = engine.start_playtime() {
                tracing::warn!(%error,"playtime initialization failed");
            }
            let account = store.load_last_account().unwrap_or_else(|error| {
                tracing::warn!(%error, "could not read the last account");
                None
            });
            engine.lock().last_account = account;
            engine.dispatch(Input::Start);
        });
    }

    /// Feeds an input to the state machine and launches its effects. Never blocks on I/O.
    pub fn dispatch(&self, input: Input) {
        let mut state = self.lock();
        if matches!(&input, Input::SignOut | Input::SessionRejected) {
            self.stop_tracking(&mut state);
        }
        let submit_trigger = !matches!(&input, Input::Tick);
        let effects = state.machine.handle(input, Instant::now());
        // Effects are launched under the lock so storage jobs keep their dispatch order.
        for effect in effects {
            self.execute(effect);
        }
        self.sync_library(&mut state);
        self.sync_tracker(&mut state);
        if submit_trigger {
            self.submit_totals(&mut state);
        }
        self.publish(&mut state);
    }

    /// Feeds an input to the library and launches its effects. Never blocks on I/O.
    pub fn library_input(&self, input: LibInput) {
        let mut state = self.lock();
        let read_trigger = matches!(
            &input,
            LibInput::Loaded { .. } | LibInput::ProviderScanned { .. }
        );
        let submit_trigger = read_trigger
            || matches!(
                &input,
                LibInput::PickMatch { .. }
                    | LibInput::MarkNoGame { .. }
                    | LibInput::ClearMatch
                    | LibInput::LookupDone { .. }
            );
        self.run_library(&mut state, input);
        self.sync_tracker(&mut state);
        if read_trigger {
            self.read_counters(&mut state);
        }
        if submit_trigger {
            self.submit_totals(&mut state);
        }
        self.publish(&mut state);
    }

    /// Scans every provider again. Requests made while a scan runs coalesce into one more scan.
    pub fn rescan(&self) {
        self.library_input(LibInput::ScanRequested);
    }

    fn run_library(&self, state: &mut State, input: LibInput) {
        let effects = state.library.handle(input);
        for effect in effects {
            self.library_effect(state, effect);
        }
    }

    /// Opens the provider's Play or Install link for an entry. The result appears in the
    /// snapshot's `launch`: "opening" until the operating system accepted the link.
    pub fn open_entry(&self, entry_key: String, kind: LaunchKind) {
        let mut state = self.lock();
        if kind == LaunchKind::Play && state.playtime.tracker.running().contains(&entry_key) {
            return;
        }
        let Some(entry) = state.library.entry(&entry_key).cloned() else {
            return;
        };
        let Some(provider) = state.library.provider(&entry.provider).cloned() else {
            return;
        };
        state.launch_seq += 1;
        let seq = state.launch_seq;
        state.launch = Some(LaunchStatus {
            entry_key,
            kind,
            state: LaunchState::Opening,
        });
        self.publish(&mut state);
        let (engine, opener) = (self.clone(), self.shared.ports.uri.clone());
        self.shared.runtime.spawn_blocking(move || {
            let result = if provider.is_manual_executable() {
                if kind != LaunchKind::Play {
                    Err(gamenode_core::library::LaunchError::Unsupported)
                } else {
                    engine
                        .shared
                        .ports
                        .local_games
                        .as_ref()
                        .zip(engine.shared.ports.installations.as_ref())
                        .ok_or(gamenode_core::library::LaunchError::Unsupported)
                        .and_then(|(store, host)| {
                            let registrations = store
                                .registrations()
                                .map_err(|_| gamenode_core::library::LaunchError::Failed)?;
                            let registration = registrations
                                .iter()
                                .find(|r| r.key() == entry.key && !r.removed)
                                .ok_or(gamenode_core::library::LaunchError::Incomplete)?;
                            host.launch(registration)
                                .map_err(|_| gamenode_core::library::LaunchError::Failed)
                        })
                }
            } else {
                open_action(&*opener, &provider, &entry.record(), kind)
            };
            let mut state = engine.lock();
            if state.launch_seq == seq {
                state.launch = result.err().map(|error| LaunchStatus {
                    state: LaunchState::Failed(error),
                    ..state.launch.clone().expect("launch is set while opening")
                });
                engine.publish(&mut state);
            }
        });
    }

    /// Hides a launch error.
    pub fn dismiss_launch(&self) {
        let mut state = self.lock();
        state.launch_seq += 1;
        state.launch = None;
        self.publish(&mut state);
    }

    /// Brings the library in line with who is signed in: opens the cached library of the last
    /// account while the session is being verified, enables GameNode features once it is, and
    /// closes the library on sign-out or a rejected session.
    fn sync_library(&self, state: &mut State) {
        let (account, access) = match state.machine.state() {
            AuthState::SignedIn(user) => (
                Some(LastAccount {
                    id: user.id.clone(),
                    name: user.name.clone(),
                }),
                Access::Verified,
            ),
            AuthState::Verifying(Purpose::Restore) | AuthState::RestoreFailed => {
                (state.last_account.clone(), Access::Unverified)
            }
            AuthState::SignedOut | AuthState::SigningOut => (None, Access::Unverified),
            // Nothing is known yet, or a different session is being set up.
            AuthState::Starting | AuthState::Pending { .. } | AuthState::Verifying(_) => return,
        };
        match &account {
            Some(account) if state.last_account.as_ref() != Some(account) => {
                state.last_account = Some(account.clone());
                let account = account.clone();
                self.persist(move |store| store.save_last_account(&account));
            }
            None if state.last_account.take().is_some() => {
                self.persist(|store| store.clear_last_account());
            }
            _ => {}
        }
        self.run_library(state, LibInput::AccountChanged(account));
        if state.library.access() != access {
            let input = LibInput::SetAccess {
                access,
                now: unix_now(),
            };
            self.run_library(state, input);
        }
    }

    /// Queues a write on the storage worker. A failure is logged: the in-memory state stays
    /// right for this run and the next scan or lookup writes it again.
    fn persist(
        &self,
        job: impl FnOnce(&dyn LibraryStore) -> Result<(), StorageError> + Send + 'static,
    ) {
        let store = self.shared.ports.library.clone();
        self.shared.storage.submit(move || {
            if let Err(error) = job(&*store) {
                tracing::warn!(%error, "could not save library data");
            }
        });
    }

    /// Reports an answer from a task that cannot call back into the engine while it holds the
    /// lock (the caller does).
    fn later(&self, input: LibInput) {
        let engine = self.clone();
        self.shared.runtime.spawn(async move {
            engine.library_input(input);
        });
    }

    /// A `401` on a library request is a rejected session, like a failed session check.
    fn check_rejected(&self, rejected: bool) {
        if rejected {
            self.dispatch(Input::SessionRejected);
        }
    }

    fn library_effect(&self, state: &State, effect: LibEffect) {
        let ports = &self.shared.ports;
        // GameNode requests are only ever made with a verified session.
        let credential = state.machine.verified_credential().cloned();
        match effect {
            LibEffect::LoadCache { op, account_id } => {
                let (engine, store) = (self.clone(), ports.library.clone());
                self.shared.storage.submit(move || {
                    let result = store.load_cache(account_id.as_deref()).map_err(|error| {
                        tracing::warn!(%error, "could not read the library cache");
                    });
                    engine.library_input(LibInput::Loaded { op, result });
                });
            }
            LibEffect::RunScan { op, provider } => {
                let Some(found) = state.library.provider(&provider).cloned() else {
                    self.later(LibInput::ProviderScanned {
                        op,
                        provider,
                        result: Err("unknown provider".to_owned()),
                        now: unix_now(),
                    });
                    return;
                };
                let (engine, files, roots, platform) = (
                    self.clone(),
                    ports.files.clone(),
                    ports.roots.clone(),
                    ports.platform,
                );
                self.shared.runtime.spawn_blocking(move || {
                    let result = if found.id() == "manual" {
                        engine
                            .shared
                            .ports
                            .local_games
                            .as_ref()
                            .ok_or_else(|| "local store unavailable".to_string())
                            .and_then(|store| {
                                let mut registrations =
                                    store.registrations().map_err(|e| e.to_string())?;
                                for registration in &mut registrations {
                                    registration.available =
                                        std::path::Path::new(&registration.executable).is_file();
                                }
                                Ok(gamenode_core::drm_free::records(&registrations))
                            })
                    } else {
                        found
                            .scan(platform, &*files, &*roots)
                            .map_err(|error| error.to_string())
                    };
                    engine.library_input(LibInput::ProviderScanned {
                        op,
                        provider,
                        result,
                        now: unix_now(),
                    });
                });
            }
            LibEffect::PersistScan {
                provider,
                outcome,
                now,
            } => self.persist(move |store| match outcome {
                ScanOutcome::Scanned(records) => store.save_scan(&provider, &records, now),
                ScanOutcome::Failed(error) => store.save_scan_failure(&provider, &error),
            }),
            LibEffect::PersistLayout(layout) => {
                self.persist(move |store| store.save_layout(&layout))
            }
            LibEffect::PersistChoice { account_id, choice } => {
                self.persist(move |store| store.save_chosen_match(&account_id, &choice));
            }
            LibEffect::ForgetChoice {
                account_id,
                entry_key,
            } => self.persist(move |store| store.delete_chosen_match(&account_id, &entry_key)),
            LibEffect::PersistMatches { games, matches } => self.persist(move |store| {
                store.save_games(&games)?;
                store.save_auto_matches(&matches)
            }),
            LibEffect::PersistCoverReady { game_id, file } => {
                self.persist(move |store| store.save_cover_ready(game_id, &file));
            }
            LibEffect::PersistCoverFailed { game_id, at } => {
                self.persist(move |store| store.save_cover_failed(game_id, at));
            }
            LibEffect::Lookup { op, category, uids } => {
                let Some(credential) = credential else {
                    return self.later(LibInput::LookupDone {
                        op,
                        category,
                        result: Err(ApiError::Failed),
                        now: unix_now(),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.lookup_external(&credential, category, &uids).await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::LookupDone {
                        op,
                        category,
                        result,
                        now: unix_now(),
                    });
                    engine.check_rejected(rejected);
                });
            }
            LibEffect::SearchGames { op, query } => {
                let Some(credential) = credential else {
                    return self.later(LibInput::MatchSearched {
                        op,
                        result: Err(ApiError::Failed),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.search_games(&credential, &query).await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::MatchSearched { op, result });
                    engine.check_rejected(rejected);
                });
            }
            LibEffect::FetchCover { game_id, url } => {
                let (engine, api, dir) =
                    (self.clone(), ports.games.clone(), ports.covers_dir.clone());
                self.shared.runtime.spawn(async move {
                    let stored = match api.fetch_cover(&url).await {
                        Ok(bytes) => {
                            tokio::task::spawn_blocking(move || write_cover(&dir, game_id, &bytes))
                                .await
                                .ok()
                                .and_then(Result::ok)
                        }
                        Err(_) => None,
                    };
                    engine.library_input(LibInput::CoverDone {
                        game_id,
                        result: stored.ok_or(()),
                        now: unix_now(),
                    });
                });
            }
            LibEffect::FetchArtwork { op, game_id } => {
                let (engine, api, dir) = (
                    self.clone(),
                    ports.games.clone(),
                    ports.covers_dir.join("artworks"),
                );
                self.shared.runtime.spawn(async move {
                    let result =
                        artwork::load(api.as_ref(), credential.as_ref(), &dir, game_id).await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::ArtworkDone {
                        op,
                        game_id,
                        result,
                    });
                    engine.check_rejected(rejected);
                });
            }
            LibEffect::ReadEntry { op, game_id } => {
                let Some(credential) = credential else {
                    return self.later(LibInput::EntryRead {
                        op,
                        game_id,
                        result: Err(ApiError::Failed),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.library_entry(&credential, game_id).await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::EntryRead {
                        op,
                        game_id,
                        result,
                    });
                    engine.check_rejected(rejected);
                });
            }
            LibEffect::ListCollections { op, user_id } => {
                let Some(credential) = credential else {
                    return self.later(LibInput::CollectionsListed {
                        op,
                        result: Err(ApiError::Failed),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.list_collections(&credential, &user_id).await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::CollectionsListed { op, result });
                    engine.check_rejected(rejected);
                });
            }
            LibEffect::CreateEntry {
                op,
                game_id,
                status,
                collection_ids,
            } => {
                let Some(credential) = credential else {
                    return self.later(LibInput::EntryCreated {
                        op,
                        game_id,
                        result: Err(ApiError::Failed),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api
                        .create_entry(&credential, game_id, status, &collection_ids)
                        .await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::EntryCreated {
                        op,
                        game_id,
                        result,
                    });
                    engine.check_rejected(rejected);
                });
            }
            LibEffect::UpdateEntry {
                op,
                game_id,
                status,
                collection_ids,
                existing,
            } => {
                let Some(credential) = credential else {
                    return self.later(LibInput::EntryCreated {
                        op,
                        game_id,
                        result: Err(ApiError::Failed),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api
                        .update_entry(&credential, game_id, status, &collection_ids, &existing)
                        .await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::EntryCreated {
                        op,
                        game_id,
                        result,
                    });
                    engine.check_rejected(rejected);
                });
            }
        }
    }

    /// Applies a settings change, persists it, and publishes the result.
    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) {
        let mut state = self.lock();
        change(&mut state.settings);
        let settings = state.settings;
        let store = self.shared.ports.settings.clone();
        self.shared.storage.submit(move || {
            if let Err(error) = store.save(&settings) {
                tracing::warn!(%error, "could not save settings");
            }
        });
        self.publish(&mut state);
    }

    fn execute(&self, effect: Effect) {
        let ports = &self.shared.ports;
        match effect {
            Effect::LoadCredential { op } => {
                let (engine, store) = (self.clone(), ports.credentials.clone());
                self.shared.storage.submit(move || {
                    let result = store.load().map_err(|_| ());
                    engine.dispatch(Input::CredentialLoaded { op, result });
                });
            }
            Effect::StoreCredential { op, credential } => {
                let (engine, store) = (self.clone(), ports.credentials.clone());
                self.shared.storage.submit(move || {
                    let ok = store.save(&credential).is_ok();
                    engine.dispatch(Input::CredentialStored { op, ok });
                });
            }
            Effect::DeleteCredential => {
                let (engine, store) = (self.clone(), ports.credentials.clone());
                self.shared.storage.submit(move || {
                    let ok = store.delete().is_ok();
                    engine.dispatch(Input::CredentialDeleted { ok });
                });
            }
            Effect::BeginSignIn {
                op,
                provider,
                success_url,
                failure_url,
            } => {
                let (engine, api, browser) =
                    (self.clone(), ports.api.clone(), ports.browser.clone());
                self.shared.runtime.spawn(async move {
                    let url = match api
                        .begin_sign_in(provider, &success_url, &failure_url)
                        .await
                    {
                        Ok(url) => url,
                        Err(reason) => {
                            engine.dispatch(Input::SignInFailed { op, reason });
                            return;
                        }
                    };
                    // Mark the browser as opened first so a prompt callback is never rejected
                    // for arriving "too early"; a launch failure then cancels the attempt.
                    engine.dispatch(Input::BrowserOpened { op });
                    let opened = tokio::task::spawn_blocking(move || browser.open(&url)).await;
                    if !matches!(opened, Ok(Ok(()))) {
                        engine.dispatch(Input::SignInFailed {
                            op,
                            reason: SignInFailure::BrowserLaunch,
                        });
                    }
                });
            }
            Effect::VerifySession { op, credential } => {
                let (engine, api) = (self.clone(), ports.api.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.get_session(&credential).await;
                    engine.dispatch(Input::SessionChecked { op, result });
                });
            }
            Effect::SignOut { op, credential } => self.sign_out(op, credential),
        }
    }

    /// Local deletion is queued immediately (in order after any pending credential write); the
    /// server invalidation runs concurrently and may fail without blocking local sign-out.
    fn sign_out(&self, op: u64, credential: Option<SessionCredential>) {
        let ports = &self.shared.ports;
        let (local_tx, local_rx) = tokio::sync::oneshot::channel();
        let store = ports.credentials.clone();
        self.shared.storage.submit(move || {
            let _ = local_tx.send(store.delete().is_ok());
        });
        let (engine, api) = (self.clone(), ports.api.clone());
        self.shared.runtime.spawn(async move {
            let remote = match &credential {
                None => RemoteSignOut::Skipped,
                Some(credential) => {
                    match tokio::time::timeout(REMOTE_SIGN_OUT_TIMEOUT, api.sign_out(credential))
                        .await
                    {
                        Ok(Ok(())) => RemoteSignOut::Confirmed,
                        _ => RemoteSignOut::NotConfirmed,
                    }
                }
            };
            drop(credential); // held only long enough to attempt invalidation
            let local_deleted = local_rx.await.unwrap_or(false);
            engine.dispatch(Input::SignOutFinished {
                op,
                remote,
                local_deleted,
            });
        });
    }
}

#[cfg(test)]
mod tests;
