//! Application composition: runs `core`'s effects against the platform and API adapters.
//!
//! The UI thread only calls [`Engine::dispatch`] (cheap, never blocks on I/O) and renders the
//! [`Snapshot`]s passed to the publish callback. HTTP and browser work runs on Tokio; keyring,
//! file and database work runs on one FIFO storage thread, so a credential write can never be
//! reordered after the sign-out that follows it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gamenode_core::auth::{AuthState, NonceSource, Purpose, RemoteSignOut};
use gamenode_core::game_api::{ApiError, FavoriteError, GameApi};
use gamenode_core::library::{
    Access, CatalogUpdate, Effect as LibEffect, Input as LibInput, LastAccount, LaunchKind,
    LaunchState, LaunchStatus, LibraryMachine, LibraryStore, ScanOutcome, open_action,
};
use gamenode_core::ports::{
    AuthApi, BrowserOpener, CredentialStore, HostFiles, RootResolver, SettingsStore, StorageError,
    UriOpener,
};
use gamenode_core::provider::Platform;
use gamenode_core::snapshot::{StoreAccountState, StoreAccountView};
use gamenode_core::steam::SteamWebApi;
use gamenode_core::{
    AuthMachine, Effect, Input, SessionCredential, Settings, SignInFailure, Snapshot,
};
use gamenode_platform::covers::{artwork_dir, write_cover};
use tokio::runtime::Handle;

mod artwork;
mod drm_free;
mod playtime;
pub mod steam_account;
pub mod steam_login;
mod tools;

use steam_account::{
    Clock, SteamAccount, SteamAccountPorts, SteamAccountState, SteamSignInResult, StoreAccountStore,
};
use steam_login::SteamLoginWindow;

/// Longest the app waits for the server to confirm a sign-out.
const REMOTE_SIGN_OUT_TIMEOUT: Duration = Duration::from_secs(5);

/// The shortest time between snapshots published for cover completions.
const COVER_FLUSH_INTERVAL: Duration = Duration::from_millis(300);

/// The adapters the engine drives.
#[derive(Clone)]
pub struct Ports {
    pub backups: Option<Arc<dyn gamenode_core::drm_free::BackupEngine>>,
    pub local_games: Option<Arc<dyn gamenode_core::drm_free::LocalGameStore>>,
    pub installations: Option<Arc<dyn gamenode_core::drm_free::InstallationHost>>,
    pub tools: Option<Arc<dyn gamenode_core::tools::ToolStore>>,
    pub tool_catalog: Option<Arc<dyn gamenode_core::tools::ToolCatalogApi>>,
    pub tool_installer: Option<Arc<dyn gamenode_core::tools::ToolInstallerHost>>,
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
    /// The Steam store-account adapters; `None` in tests that do not exercise Steam.
    pub steam: Option<SteamPorts>,
    /// Where downloaded covers are stored.
    pub covers_dir: PathBuf,
    pub platform: Platform,
}

/// The Steam store-account adapters: a credential slot separate from the GameNode session, the
/// non-secret account row, the web client and the isolated sign-in window.
#[derive(Clone)]
pub struct SteamPorts {
    pub credentials: Arc<dyn CredentialStore>,
    pub accounts: Arc<dyn StoreAccountStore>,
    pub client: Arc<dyn SteamWebApi>,
    pub login: Arc<dyn SteamLoginWindow>,
    pub clock: Arc<dyn Clock>,
}

impl SteamPorts {
    fn into_account_ports(self) -> SteamAccountPorts {
        SteamAccountPorts {
            credentials: self.credentials,
            accounts: self.accounts,
            client: self.client,
            login: self.login,
            clock: self.clock,
        }
    }
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
    pending_claim: Option<gamenode_core::drm_free::PendingClaim>,
    local_selection: Option<String>,
    local_generation: u64,
    local_backup_busy: bool,
    local: gamenode_core::drm_free::LocalView,
    tools: gamenode_core::tools::ToolsView,
    tool_reset_config: bool,
    tool_last_fetch: std::collections::HashMap<
        (
            gamenode_core::tools::ToolId,
            gamenode_core::tools::ToolChannel,
        ),
        Instant,
    >,
    tool_selected_target_exe: Option<String>,
    playtime: playtime::Playtime,
    machine: AuthMachine,
    settings: Settings,
    library: LibraryMachine,
    /// The account record on disk, so the cache can open before the session is verified.
    last_account: Option<LastAccount>,
    /// The latest Play or Install; `launch_seq` tells its answer from an older one.
    launch: Option<LaunchStatus>,
    launch_seq: u64,
    /// A catalog was requested while the library could not start one yet.
    catalog_request_pending: bool,
    /// A catalog clear is waiting for a cache load to finish so it cannot be undone.
    catalog_clear_pending: bool,
    /// Whether the library is installing its cache after an account change.
    library_loading: bool,
    /// The selection the last snapshot showed, to notice when it changes.
    published_selection: Option<Vec<String>>,
}

struct Shared {
    local_catalog: Arc<tokio::sync::Semaphore>,
    local_cancel: std::sync::atomic::AtomicBool,
    local_discovering: std::sync::atomic::AtomicBool,
    local_discovery_again: std::sync::atomic::AtomicBool,
    state: Mutex<State>,
    ports: Ports,
    runtime: Handle,
    storage: StorageWorker,
    /// The Steam account service when this build has Steam adapters.
    steam: Option<Arc<SteamAccount>>,
    /// One catalog sync per provider, coalescing requests made while one runs.
    catalog_syncs: Mutex<std::collections::HashMap<String, CatalogSync>>,
    /// A cover completion is waiting for the coalescing flush task to publish it.
    cover_flush_pending: AtomicBool,
    /// A task is scheduled (or running) to publish the newest cover completions.
    cover_flush_scheduled: AtomicBool,
    publish: Box<dyn Fn(Snapshot) + Send + Sync>,
}

/// The coalescing state of one provider's catalog sync: `pending` is the newest op to run once
/// the current one finishes.
#[derive(Default)]
struct CatalogSync {
    running: bool,
    pending: Option<u64>,
}

/// Cheap to clone; all clones share one state.
#[derive(Clone)]
pub struct Engine {
    shared: Arc<Shared>,
}

impl Engine {
    /// `publish` is called with every new snapshot, from arbitrary threads, while the engine's
    /// lock is held: it must hand the snapshot off (e.g. emit a Tauri `engine:snapshot` event) and
    /// must not call back into the engine.
    pub fn new(
        ports: Ports,
        runtime: Handle,
        settings: Settings,
        library: LibraryMachine,
        nonce: NonceSource,
        publish: impl Fn(Snapshot) + Send + Sync + 'static,
    ) -> Self {
        let steam = ports
            .steam
            .clone()
            .map(|steam| Arc::new(SteamAccount::without_observer(steam.into_account_ports())));
        let shared = Arc::new(Shared {
            local_catalog: Arc::new(tokio::sync::Semaphore::new(4)),
            local_cancel: std::sync::atomic::AtomicBool::new(false),
            local_discovering: std::sync::atomic::AtomicBool::new(false),
            local_discovery_again: std::sync::atomic::AtomicBool::new(false),
            state: Mutex::new(State {
                pending_claim: None,
                local_selection: None,
                local_generation: 0,
                local_backup_busy: false,
                local: Default::default(),
                tools: gamenode_core::tools::ToolsView {
                    catalog: tools::builtin_catalog_items(),
                    ..Default::default()
                },
                tool_reset_config: false,
                tool_last_fetch: Default::default(),
                tool_selected_target_exe: None,
                playtime: playtime::Playtime::new(ports.platform),
                machine: AuthMachine::new(nonce),
                settings,
                library,
                last_account: None,
                launch: None,
                launch_seq: 0,
                catalog_request_pending: false,
                catalog_clear_pending: false,
                library_loading: false,
                published_selection: None,
            }),
            ports,
            runtime,
            storage: StorageWorker::new(),
            steam: steam.clone(),
            catalog_syncs: Mutex::new(std::collections::HashMap::new()),
            cover_flush_pending: AtomicBool::new(false),
            cover_flush_scheduled: AtomicBool::new(false),
            publish: Box::new(publish),
        });
        // The service publishes every transition; a weak reference keeps the engine droppable
        // and avoids an `Arc` cycle between the observer and `Shared`.
        if let Some(steam) = &steam {
            let weak = Arc::downgrade(&shared);
            steam.set_observer(move |_| {
                if let Some(shared) = weak.upgrade() {
                    Engine { shared }.publish_current();
                }
            });
        }
        Self { shared }
    }

    /// Publishes the current state without holding any adapter lock. Used by the Steam status
    /// observer, which runs from whichever background thread changed the account.
    fn publish_current(&self) {
        let mut state = self.lock();
        self.publish(&mut state);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn covers_dir(&self) -> &std::path::Path {
        &self.shared.ports.covers_dir
    }

    pub fn host_files(&self) -> &Arc<dyn gamenode_core::ports::HostFiles> {
        &self.shared.ports.files
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut state = self.lock();
        self.snapshot_of(&mut state)
    }

    /// The artwork the details panel shows for `game_id`, if any. Takes the engine lock only
    /// long enough to clone a handle, unlike [`Engine::snapshot`], which rebuilds the view.
    pub fn artwork(&self, game_id: u64) -> Option<Arc<[u8]>> {
        self.lock().library.artwork_for(game_id)
    }

    /// The title of the game that is running, if any. Cheap enough for the tray tooltip on every
    /// maintenance tick: it takes the lock only long enough to look at the running set, instead
    /// of building a full snapshot.
    pub fn running_title(&self) -> Option<String> {
        self.lock().library.running_title()
    }

    fn snapshot_of(&self, state: &mut State) -> Snapshot {
        self.update_playtime_view(state, unix_now());
        let library = state.library.view();
        let store_accounts = self.store_accounts(library.as_ref());
        // Details loaded for the previous selection do not belong to the new one.
        let selection = library.as_ref().map(|view| view.selected.clone());
        if selection != state.published_selection {
            state.published_selection = selection;
            state.local.versions.clear();
            state.local.restore = None;
        }
        let mut snapshot = Snapshot::new(
            &state.machine,
            self.shared.ports.credentials.method(),
            state.settings,
            library,
            state.launch.clone(),
        );
        snapshot.local_games = state.local.clone();
        snapshot.local_games.pending_claim = state.pending_claim.clone();
        snapshot.playtime_error = state.playtime.error;
        snapshot.sync = state.playtime.preferences;
        snapshot.playtime_source = state.playtime.source;
        snapshot.tracking = gamenode_core::snapshot::TrackingView {
            title: state.library.running_title().unwrap_or_default(),
            available: state.playtime.available,
            running: state.playtime.tracker.running(),
        };
        snapshot.tools = state.tools.clone();
        snapshot.store_accounts = store_accounts;
        snapshot
    }

    /// The Steam card state, with the library's live counts. Counts are zero while no GameNode
    /// account's library is open; the card itself still renders.
    fn store_accounts(
        &self,
        library: Option<&gamenode_core::library::LibraryView>,
    ) -> Vec<StoreAccountView> {
        let Some(steam) = &self.shared.steam else {
            return Vec::new();
        };
        let status = steam.status();
        let count = library.and_then(|view| {
            view.providers
                .iter()
                .find(|count| count.id == steam_account::PROVIDER)
        });
        let account_id = status.account_id.clone();
        let account_name = status.account_name.clone();
        Vec::from([StoreAccountView {
            provider: steam_account::PROVIDER.to_owned(),
            state: match status.state {
                SteamAccountState::NotConnected => StoreAccountState::NotConnected,
                SteamAccountState::Checking => StoreAccountState::Checking,
                SteamAccountState::SigningIn => StoreAccountState::SigningIn,
                SteamAccountState::Connected => StoreAccountState::Connected,
                SteamAccountState::Expired => StoreAccountState::Expired,
                SteamAccountState::Unreachable => StoreAccountState::Unreachable,
                SteamAccountState::CredentialError => StoreAccountState::CredentialError,
            },
            account_id,
            account_name,
            owned_count: count.map_or(0, |count| count.owned_count),
            family_count: count.map_or(0, |count| count.family_count),
            installed_count: count.map_or(0, |count| count.installed_count),
            last_checked_at: status.last_checked_at,
            last_synced_at: status.last_synced_at,
            storage: Some(status.storage),
            persist_failed: status.persist_failed,
            catalog_error: status.catalog_error,
            family_error: status.family_error,
            can_sign_in: status.can_sign_in(),
            can_cancel_sign_in: status.can_cancel_sign_in(),
            can_focus_sign_in: status.can_focus_sign_in(),
            can_sign_out: status.can_sign_out(),
            can_check: status.can_check(),
            can_remove_credential: status.can_remove_credential(),
        }])
    }

    fn publish(&self, state: &mut State) {
        (self.shared.publish)(self.snapshot_of(state));
    }

    /// Reads the last signed-in account, then the stored session and, if there is one,
    /// validates it. The account is read first so the cached library can open while the session
    /// is still being verified.
    pub fn start(&self) {
        self.recover_local_jobs();
        self.refresh_local_games();
        self.start_local_discovery();
        self.start_local_manifest_updates();
        self.recover_and_load_tools();
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
        // Steam starts on its own, off the UI thread: restore the credential and account row,
        // then validate the session. A catalog sync is requested by the library's scan or by the
        // account commands, so this never blocks the library.
        if let Some(steam) = self.shared.steam.clone() {
            let engine = self.clone();
            self.shared.runtime.spawn(async move {
                steam.startup().await;
                let outcome = steam.check().await;
                if outcome.superseded {
                    return;
                }
                if outcome.account_changed {
                    engine.clear_store_catalog(steam_account::PROVIDER);
                    if outcome.status.state == SteamAccountState::Connected {
                        // Queue even while the library is still loading; a request made before
                        // the cache is ready is applied once it is, instead of being lost.
                        engine.request_catalog();
                    }
                }
            });
        }
    }

    /// Starts the isolated Steam sign-in window. The command returns when the attempt ends; the
    /// window and every request run off the UI thread.
    pub async fn steam_sign_in(&self) {
        let Some(steam) = self.shared.steam.clone() else {
            return;
        };
        let outcome = steam.sign_in().await;
        // The result is authoritative: a concurrent startup check can move the live state to
        // Checking before the outcome is built, even though this attempt did connect. Using the
        // live status here would skip the first catalog import for that account.
        if outcome.result != SteamSignInResult::Connected {
            return;
        }
        if outcome.account_changed {
            self.clear_store_catalog(steam_account::PROVIDER);
        }
        // Queue the import even if the library is still opening: library_input applies the
        // pending request after the cache loads, so a fast sign-in cannot lose its first sync.
        self.request_catalog();
    }

    /// Focuses the open Steam sign-in window. No window is a no-op.
    pub async fn steam_focus_sign_in(&self) {
        if let Some(steam) = self.shared.steam.clone() {
            let _ = tokio::task::spawn_blocking(move || {
                steam.focus_sign_in();
            })
            .await;
        }
    }

    /// Cancels the open Steam sign-in window and returns to the state before the attempt.
    pub async fn steam_cancel_sign_in(&self) {
        if let Some(steam) = self.shared.steam.clone() {
            let _ = tokio::task::spawn_blocking(move || {
                steam.cancel_sign_in();
            })
            .await;
        }
    }

    /// Signs out of Steam: the credential and the account row are removed and the owned and
    /// family facts leave the library. Installed entries, matches and covers stay.
    pub async fn steam_sign_out(&self) {
        let Some(steam) = self.shared.steam.clone() else {
            return;
        };
        let outcome = steam.sign_out().await;
        if outcome.signed_out {
            self.clear_store_catalog(steam_account::PROVIDER);
        }
    }

    /// Validates the session ("Sync now", "Try again", "Retry"). With an open library this
    /// requests a catalog sync, which validates before fetching; without one it only checks the
    /// card state. Never blocks the caller on the network.
    pub async fn steam_check(&self) {
        let Some(steam) = self.shared.steam.clone() else {
            return;
        };
        if self.library_is_active() {
            self.request_catalog();
            return;
        }
        let outcome = steam.check().await;
        if outcome.superseded {
            return;
        }
        if outcome.account_changed {
            self.clear_store_catalog(steam_account::PROVIDER);
        }
    }

    fn library_is_active(&self) -> bool {
        self.lock().library.is_active()
    }

    /// Asks the library for a fresh catalog. A request made while the machine cannot start one
    /// (the cache is still loading) stays pending until a library input can honor it.
    fn request_catalog(&self) {
        let mut state = self.lock();
        state.catalog_request_pending = true;
        self.run_library(&mut state, LibInput::RequestCatalog);
        self.publish(&mut state);
    }

    /// Drops one provider's owned and family facts and persists the clear. Entries, matches and
    /// covers are kept. While the library is loading its cache the in-memory clear is deferred
    /// until the cache is installed, so the old account's cached facts cannot come back.
    fn clear_store_catalog(&self, provider: &str) {
        let stored = provider.to_owned();
        self.persist(move |store| store.clear_catalog(&stored));
        let mut state = self.lock();
        state.catalog_request_pending = false;
        if state.library_loading {
            state.catalog_clear_pending = true;
        } else {
            self.run_library(
                &mut state,
                LibInput::CatalogCleared {
                    provider: provider.to_owned(),
                },
            );
        }
        self.publish(&mut state);
    }

    fn persist_catalog(&self, provider: String, update: CatalogUpdate) {
        self.persist(move |store| store.save_catalog(&provider, &update));
    }

    /// Runs at most one catalog sync per provider. A request that arrives while one runs is
    /// remembered and starts one more afterwards, so the newest core op is the one reported.
    fn sync_catalog(&self, op: u64, provider: String) {
        if self.shared.steam.is_none() || provider != steam_account::PROVIDER {
            // Complete the core op even without a Steam adapter: nothing was attempted.
            self.later(LibInput::CatalogSynced {
                op,
                provider,
                owned: None,
                family: None,
                now: unix_now(),
            });
            return;
        }
        let start = {
            let mut syncs = self.catalog_syncs();
            let entry = syncs.entry(provider.clone()).or_default();
            if entry.running {
                entry.pending = Some(op);
                false
            } else {
                entry.running = true;
                true
            }
        };
        if start {
            self.spawn_catalog_sync(provider, op);
        }
    }

    fn catalog_syncs(&self) -> MutexGuard<'_, std::collections::HashMap<String, CatalogSync>> {
        self.shared
            .catalog_syncs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn spawn_catalog_sync(&self, provider: String, op: u64) {
        let engine = self.clone();
        self.shared.runtime.spawn(async move {
            engine.run_catalog_sync(&provider, op).await;
            let next = {
                let mut syncs = engine.catalog_syncs();
                match syncs.get_mut(&provider) {
                    Some(entry) => match entry.pending.take() {
                        Some(next) => Some(next),
                        None => {
                            entry.running = false;
                            None
                        }
                    },
                    None => None,
                }
            };
            if let Some(next) = next {
                engine.spawn_catalog_sync(provider, next);
            }
        });
    }

    async fn run_catalog_sync(&self, provider: &str, op: u64) {
        let Some(steam) = self.shared.steam.clone() else {
            return;
        };
        let mut outcome = steam.sync().await;
        if outcome.superseded {
            // The sync may have joined a check that a concurrent sign-in superseded. A sign-in
            // requests the catalog after validation, but that request can arrive while the
            // library is still loading, so retry here while the account is genuinely connected.
            // A sign-out leaves NotConnected and is never retried.
            if steam.status().state == SteamAccountState::Connected {
                outcome = steam.sync().await;
            }
            if outcome.superseded {
                self.library_input(LibInput::CatalogSynced {
                    op,
                    provider: provider.to_owned(),
                    owned: None,
                    family: None,
                    now: unix_now(),
                });
                return;
            }
        }
        if outcome.account_changed {
            // The previous account's games must never stay visible as owned. The new account's
            // catalog is requested afterwards, so a failed fetch still leaves no old facts.
            self.clear_store_catalog(provider);
            if outcome.status.state == SteamAccountState::Connected {
                self.request_catalog();
            }
            return;
        }
        // A sync that joined an in-flight check has no catalog; retry once now that the check is
        // done. A second None means another operation was running, so complete the op anyway.
        if outcome.owned.is_none()
            && outcome.family.is_none()
            && outcome.status.state == SteamAccountState::Connected
        {
            outcome = steam.sync().await;
            if outcome.superseded {
                self.library_input(LibInput::CatalogSynced {
                    op,
                    provider: provider.to_owned(),
                    owned: None,
                    family: None,
                    now: unix_now(),
                });
                return;
            }
            if outcome.account_changed {
                self.clear_store_catalog(provider);
                if outcome.status.state == SteamAccountState::Connected {
                    self.request_catalog();
                }
                return;
            }
        }
        if outcome.owned.is_some() || outcome.family.is_some() {
            self.persist_catalog(
                provider.to_owned(),
                CatalogUpdate {
                    owned: outcome.owned.clone(),
                    family: outcome.family.clone(),
                    now: outcome.now,
                },
            );
        }
        self.library_input(LibInput::CatalogSynced {
            op,
            provider: provider.to_owned(),
            owned: outcome.owned,
            family: outcome.family,
            now: outcome.now,
        });
    }

    /// Feeds an input to the state machine and launches its effects. Never blocks on I/O.
    pub fn dispatch(&self, input: Input) {
        let mut state = self.lock();
        let is_tick = matches!(&input, Input::Tick);
        let auth_before = is_tick.then(|| (state.machine.state().clone(), state.machine.notice()));
        if matches!(&input, Input::SignOut | Input::SessionRejected) {
            self.stop_tracking(&mut state);
        }
        let submit_trigger = !is_tick;
        let effects = state.machine.handle(input, Instant::now());
        if effects.is_empty()
            && let Some((previous_state, previous_notice)) = auth_before
            && previous_state == *state.machine.state()
            && previous_notice == state.machine.notice()
        {
            // Tick only expires pending authentication attempts. Avoid rebuilding the library,
            // tracker input, and full UI snapshot on every idle timer tick.
            return;
        }
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
        let cover_done = matches!(&input, LibInput::CoverDone { .. });
        let read_trigger = matches!(
            &input,
            LibInput::Loaded { .. } | LibInput::ProviderScanned { .. }
        );
        let became_loading = matches!(&input, LibInput::AccountChanged(Some(_)));
        let stopped_loading = matches!(&input, LibInput::AccountChanged(None));
        let finished_loading = matches!(&input, LibInput::Loaded { .. });
        let submit_trigger = read_trigger
            || matches!(
                &input,
                LibInput::PickMatch { .. }
                    | LibInput::MarkNoGame { .. }
                    | LibInput::ClearMatch
                    | LibInput::LookupDone { .. }
            );
        if became_loading {
            state.library_loading = true;
        }
        if stopped_loading {
            state.library_loading = false;
        }
        self.run_library(&mut state, input);
        if finished_loading {
            // Cached ownership facts were just installed. A clear requested while loading lands
            // now, and the dropped scan op is replaced by a fresh catalog request.
            state.library_loading = false;
            if state.catalog_clear_pending {
                state.catalog_clear_pending = false;
                self.run_library(
                    &mut state,
                    LibInput::CatalogCleared {
                        provider: steam_account::PROVIDER.to_owned(),
                    },
                );
                state.catalog_request_pending = true;
            }
        }
        if state.catalog_request_pending {
            self.run_library(&mut state, LibInput::RequestCatalog);
        }
        if let Some(key) = state.local_selection.clone()
            && state
                .library
                .installed_entries()
                .any(|entry| entry.key == key)
        {
            state.local_selection = None;
            self.run_library(
                &mut state,
                LibInput::Select {
                    key,
                    mode: gamenode_core::library::SelectMode::Replace,
                },
            );
        }
        self.sync_tracker(&mut state);
        if read_trigger {
            self.read_counters(&mut state);
        }
        if submit_trigger {
            self.submit_totals(&mut state);
        }
        if cover_done {
            self.publish_cover(&mut state);
        } else {
            self.publish(&mut state);
        }
    }

    /// Publishes one cover completion, coalescing the rest of a burst.
    ///
    /// The first completion publishes immediately so the UI shows a cover as soon as it is
    /// ready. Further completions only update state; a flush task publishes the newest state
    /// after [`COVER_FLUSH_INTERVAL`], and keeps doing so while covers keep arriving. This keeps
    /// a first sync from rebuilding the full library snapshot once per downloaded cover while
    /// still publishing the final state.
    fn publish_cover(&self, state: &mut State) {
        if self
            .shared
            .cover_flush_scheduled
            .swap(true, Ordering::AcqRel)
        {
            self.shared
                .cover_flush_pending
                .store(true, Ordering::Release);
            return;
        }
        // This completion starts a burst: publish its state now, so nothing is pending.
        self.shared
            .cover_flush_pending
            .store(false, Ordering::Release);
        self.publish(state);

        // A weak reference keeps the engine droppable while this task waits.
        let weak = Arc::downgrade(&self.shared);
        self.shared.runtime.spawn(async move {
            loop {
                tokio::time::sleep(COVER_FLUSH_INTERVAL).await;
                let Some(shared) = weak.upgrade() else {
                    return;
                };
                let engine = Engine { shared };
                let mut state = engine.lock();
                if !engine
                    .shared
                    .cover_flush_pending
                    .swap(false, Ordering::AcqRel)
                {
                    engine
                        .shared
                        .cover_flush_scheduled
                        .store(false, Ordering::Release);
                    return;
                }
                engine.publish(&mut state);
                // Stay scheduled: another completion may start while this one publishes.
            }
        });
    }

    /// Scans every provider again. Requests made while a scan runs coalesce into one more scan.
    pub fn rescan(&self) {
        self.library_input(LibInput::ScanRequested);
    }

    pub fn remove_entry(&self, key: String, allow_rediscovery: bool) {
        if let Some(id) = key.strip_prefix("manual:") {
            self.remove_local_game(id.to_owned(), allow_rediscovery);
        } else {
            self.library_input(LibInput::RemoveEntry {
                key,
                allow_rediscovery,
            });
        }
    }

    fn run_library(&self, state: &mut State, input: LibInput) {
        // Relative filters are measured in days; the library never reads the clock itself.
        state.library.set_now(unix_now());
        self.apply_favorites_gate(state);
        // A verified session, a completed scan and a successful Add to GameNode are the moments
        // pending favorites are worth retrying.
        let retry_favorites = matches!(
            &input,
            LibInput::SetAccess {
                access: Access::Verified,
                ..
            } | LibInput::EntryCreated { result: Ok(()), .. }
        );
        let scan_was_running = state.library.scan_running();
        let effects = state.library.handle(input);
        for effect in effects {
            self.library_effect(state, effect);
        }
        if retry_favorites || (scan_was_running && !state.library.scan_running()) {
            let effects = state.library.handle(LibInput::SyncFavoritesRequested);
            for effect in effects {
                self.library_effect(state, effect);
            }
        }
    }

    /// Keeps the library's favorites gate equal to `sync enabled && explained && verified`.
    fn apply_favorites_gate(&self, state: &mut State) {
        let active = state.playtime.preferences.enabled
            && state.playtime.preferences.explained
            && state.machine.verified_credential().is_some();
        if active == state.library.favorites_sync_active() {
            return;
        }
        let effects = state.library.handle(LibInput::SetSyncActive(active));
        for effect in effects {
            self.library_effect(state, effect);
        }
    }

    /// Opens the provider's Play or Install link for an entry. The result appears in the
    /// snapshot's `launch`: "opening" until the operating system accepted the link.
    pub fn open_entry(&self, entry_key: String, kind: LaunchKind) {
        let mut state = self.lock();
        if state.launch.as_ref().is_some_and(|launch| {
            launch.entry_key == entry_key
                && launch.kind == kind
                && matches!(launch.state, LaunchState::Opening)
        }) {
            return;
        }
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
                            gamenode_core::library::launch::launch_manual(
                                &**host,
                                &provider,
                                &entry.record(),
                                registration,
                                kind,
                            )
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
        let catalog_changed =
            state.library.account() != account.as_ref() || state.library.access() != access;
        let account_changed = state.library.account() != account.as_ref();
        if account_changed {
            // A new library installs its cache before it is ready; catalog clears are deferred.
            state.library_loading = account.is_some();
            state.local_generation += 1;
        }
        self.run_library(state, LibInput::AccountChanged(account));
        if catalog_changed {
            let engine = self.clone();
            self.shared.runtime.spawn_blocking(move || {
                engine.refresh_local_games();
                engine.enrich_local_games();
            });
        }
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

    fn library_effect(&self, state: &mut State, effect: LibEffect) {
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
            // The account service validates the session (persisting a rotated cookie) and fetches
            // the owned and family catalogs; the engine applies the result here.
            LibEffect::SyncCatalog { op, provider } => {
                // The machine accepted a pending request: it emitted the sync for it. While the
                // cache is installing, this is only the scan's own sync and the request stays
                // pending so it is retried once the library is ready.
                if !state.library_loading {
                    state.catalog_request_pending = false;
                }
                self.sync_catalog(op, provider);
            }

            LibEffect::PersistLayout(layout) => {
                self.persist(move |store| store.save_layout(&layout))
            }
            LibEffect::PersistFavorite(favorite) => {
                self.persist(move |store| store.save_favorite(&favorite));
            }
            LibEffect::SyncFavorites { op, user_id } => {
                let Some(credential) = state.machine.verified_credential().cloned() else {
                    return self.later(LibInput::FavoritesSynced {
                        op,
                        result: Err(ApiError::Failed),
                        now: unix_now(),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.list_favorites(&credential, &user_id).await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::FavoritesSynced {
                        op,
                        result,
                        now: unix_now(),
                    });
                    engine.check_rejected(rejected);
                });
            }
            LibEffect::PushFavorite {
                op,
                game_id,
                favorite,
            } => {
                let Some(credential) = state.machine.verified_credential().cloned() else {
                    return self.later(LibInput::FavoritePushed {
                        op,
                        game_id,
                        favorite,
                        result: Err(FavoriteError::Failed),
                        now: unix_now(),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.set_favorite(&credential, game_id, favorite).await;
                    let rejected = matches!(result, Err(FavoriteError::Rejected));
                    engine.library_input(LibInput::FavoritePushed {
                        op,
                        game_id,
                        favorite,
                        result,
                        now: unix_now(),
                    });
                    engine.check_rejected(rejected);
                });
            }
            LibEffect::PersistExcluded(excluded) => {
                self.persist(move |store| store.save_excluded(&excluded))
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
                    artwork_dir(&ports.covers_dir),
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
