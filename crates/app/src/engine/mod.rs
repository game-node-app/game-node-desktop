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
use gamenode_core::snapshot::StoreAccountView;
use gamenode_core::{
    AuthMachine, Effect, Input, SessionCredential, Settings, SignInFailure, Snapshot,
};
use gamenode_platform::covers::{artwork_dir, write_cover};
use tokio::runtime::Handle;

mod achievements;
mod artwork;
mod detected_target;
mod drm_free;
pub mod pc_ports;
mod playtime;
mod plugin_ports;
mod providers;
mod tools;

pub use pc_ports::{PortProgressPublisher, PortsService};
pub use providers::{ExtensionReadinessChoice, ExtensionReadinessSelection};

/// Longest the app waits for the server to confirm a sign-out.
const REMOTE_SIGN_OUT_TIMEOUT: Duration = Duration::from_secs(5);

/// A page on the GameNode website the app can open in the default browser (D14).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum GameNodePage {
    /// `https://gamenode.app/library/<signed-in user id>`.
    Library,
    /// `https://gamenode.app/game/<id>`.
    Game { id: u64 },
}

/// Why a GameNode page could not be opened. The code is stable; the UI words the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "code")]
pub enum GameNodePageError {
    /// The library page needs a verified session, and there is none.
    NoSession,
    /// The website URL, the blocking worker or the system browser failed.
    Browser,
}

/// A recipe the authenticated plugin profile currently exposes for a matched library game.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRecipeOption {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub recipe_id: String,
    pub recipe_revision: u64,
}

/// Selection state shown beside one library entry's detected launch target.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRecipeChoices {
    pub target_revision: u64,
    pub detected_default_available: bool,
    pub selected: Option<gamenode_core::library::LaunchRecipeSelection>,
    pub recipes: Vec<LaunchRecipeOption>,
}

/// The shortest time between snapshots published for cover completions.
const COVER_FLUSH_INTERVAL: Duration = Duration::from_millis(300);

/// The adapters the engine drives.
#[derive(Clone)]
pub struct Ports {
    pub discovery: Option<Arc<dyn gamenode_core::drm_free::DiscoveryHost>>,
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
    /// The PC ports service; `None` on builds without port installs and in tests that do not
    /// exercise PC ports.
    pub pc_ports: Option<Arc<PortsService>>,
    /// Durable port provenance. Read on the launch worker so a port's environment is applied.
    pub port_store: Option<Arc<dyn gamenode_core::ports::PortStore>>,
    /// Coalesces install progress into the `ports:progress` event.
    pub port_progress: Arc<PortProgressPublisher>,
    /// Where downloaded covers are stored.
    pub covers_dir: PathBuf,
    pub platform: Platform,
    /// The local achievement cache, keyed by provider and store account.
    pub achievements: Arc<dyn gamenode_core::achievements::AchievementStore>,
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
    plugin_tool_request: Option<tools::PluginToolRequest>,
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
    catalog_clear_pending: std::collections::BTreeSet<String>,
    /// Whether the library is installing its cache after an account change.
    library_loading: bool,
    /// The selection the last snapshot showed, to notice when it changes.
    published_selection: Option<Vec<String>>,
    /// The PC ports projection built by the service on a worker thread.
    ports_view: gamenode_core::ports::PortsView,
    plugin_port_request: Option<plugin_ports::PendingPort>,
    /// Cached achievement summaries for the connected store accounts.
    achievement_summaries: std::collections::BTreeMap<
        (String, String),
        gamenode_core::achievements::StoredAchievementSummary,
    >,
    /// The UI locale achievement text is requested in.
    achievement_language: String,
}

struct Shared {
    local_catalog: Arc<tokio::sync::Semaphore>,
    local_cancel: std::sync::atomic::AtomicBool,
    local_discovering: std::sync::atomic::AtomicBool,
    local_discovery_again: std::sync::atomic::AtomicBool,
    state: Mutex<State>,
    /// Serializes each "read the local-game store, then install that view" section.
    /// Without it, a refresh that read the store before a write can apply after a newer
    /// view and temporarily hide registrations or inferences from the snapshot.
    local_views: Mutex<()>,
    plugin_scan_roots:
        Mutex<std::collections::HashMap<String, Vec<crate::plugin_host::library::PluginScanRoot>>>,
    ports: Ports,
    runtime: Handle,
    storage: StorageWorker,
    providers: Mutex<Option<Arc<dyn crate::plugin_host::runtime::ProviderRuntime>>>,
    /// Third-party plugin profiles follow the signed-in GameNode account.
    plugin_profiles: Mutex<Option<Arc<crate::plugin_host::profile::PluginProfiles>>>,
    /// Shared host resource adapter used for user-initiated custom Play selections.
    selected_launch_api: Mutex<Option<Arc<crate::plugin_host::resources::ResourceApi>>>,
    /// The library projection plugins read through `ResourceHost`.
    library_feed: Mutex<Option<Arc<crate::plugin_host::library::LibraryFeed>>>,
    /// Serialize checks and catalog commits without delaying account cancellation.
    provider_checks: Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// One catalog sync per provider, coalescing requests made while one runs.
    catalog_syncs: Mutex<std::collections::HashMap<String, CatalogSync>>,
    /// One serialized achievement worker and its priority queue per provider.
    achievement_queues: Mutex<std::collections::HashMap<String, achievements::Queue>>,
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
/// Why [`Engine::open_entry_then`] did not hand the entry's link to the operating system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OpenError {
    /// The same opening is under way, or the game is already running.
    Busy,
    /// The entry or its provider is not in the library any more.
    NotFound,
    /// The provider's action could not run.
    Launch(gamenode_core::library::LaunchError),
}

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
        #[cfg(test)]
        let test_runtime = Arc::new(tests::provider_runtime::Runtime::new(&ports));
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
                plugin_tool_request: None,
                playtime: playtime::Playtime::new(ports.platform),
                machine: AuthMachine::new(nonce),
                settings,
                library,
                last_account: None,
                launch: None,
                launch_seq: 0,
                catalog_request_pending: false,
                catalog_clear_pending: Default::default(),
                library_loading: false,
                published_selection: None,
                ports_view: gamenode_core::ports::PortsView::default(),
                plugin_port_request: None,
                achievement_summaries: Default::default(),
                achievement_language: crate::commands::normalize_locale(
                    &crate::commands::system_locale(),
                )
                .to_owned(),
            }),
            local_views: Mutex::new(()),
            plugin_scan_roots: Mutex::default(),
            ports,
            runtime,
            storage: StorageWorker::new(),
            providers: Mutex::new(None),
            plugin_profiles: Mutex::new(None),
            selected_launch_api: Mutex::new(None),
            library_feed: Mutex::new(None),
            provider_checks: Mutex::new(std::collections::HashMap::new()),
            catalog_syncs: Mutex::new(std::collections::HashMap::new()),
            achievement_queues: Mutex::new(std::collections::HashMap::new()),
            cover_flush_pending: AtomicBool::new(false),
            cover_flush_scheduled: AtomicBool::new(false),
            publish: Box::new(publish),
        });
        if let Some(service) = &shared.ports.pc_ports {
            let weak = Arc::downgrade(&shared);
            service.set_observer(Arc::new(move |event| {
                if let Some(shared) = weak.upgrade() {
                    Engine { shared }.on_ports_event(event);
                }
            }));
        }
        let engine = Self { shared };
        #[cfg(test)]
        engine.set_provider_runtime(test_runtime);
        engine
    }

    /// Handles one PC ports worker event. A changed projection is stored and published once;
    /// progress ticks go to the coalescing publisher and never touch the snapshot.
    fn on_ports_event(&self, event: pc_ports::PortsEvent) {
        match event {
            pc_ports::PortsEvent::Changed { view, completion } => {
                {
                    let mut state = self.lock();
                    let review = state.ports_view.plugin_request.clone();
                    state.ports_view = view;
                    state.ports_view.plugin_request = review;
                    self.publish(&mut state);
                }
                if let Some(completion) = completion {
                    self.port_install_completed(*completion);
                }
            }
            pc_ports::PortsEvent::Progress(progress) => {
                self.shared.ports.port_progress.publish(progress);
            }
            pc_ports::PortsEvent::ExecutableRejected { job_id, reason } => {
                tracing::warn!(job_id, %reason, "the chosen PC port executable was refused");
            }
        }
    }

    /// Publishes the current state without holding any adapter lock. Used by the provider status
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

    /// Held across a local-game store read and the snapshot view it installs, so two
    /// concurrent readers apply in the order they read. Callers may take the state lock
    /// while holding this, never the other way around.
    fn lock_local_views(&self) -> MutexGuard<'_, ()> {
        self.shared
            .local_views
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

    /// Opens one GameNode website page in the default browser, off the UI thread. The library page
    /// needs the verified session's user id; a game page does not.
    pub async fn open_gamenode_page(&self, page: GameNodePage) -> Result<(), GameNodePageError> {
        let (browser, user_id) = {
            let state = self.lock();
            let user_id = match &page {
                GameNodePage::Game { .. } => None,
                GameNodePage::Library => {
                    state
                        .machine
                        .verified_credential()
                        .ok_or(GameNodePageError::NoSession)?;
                    match state.machine.state() {
                        AuthState::SignedIn(user) => Some(user.id.clone()),
                        _ => return Err(GameNodePageError::NoSession),
                    }
                }
            };
            (self.shared.ports.browser.clone(), user_id)
        };
        let web = gamenode_api::web_url_from_env().map_err(|_| GameNodePageError::Browser)?;
        let url = match (page, user_id) {
            (GameNodePage::Game { id }, _) => gamenode_api::game_page_url(&web, id),
            (GameNodePage::Library, Some(user_id)) => {
                gamenode_api::library_page_url(&web, &user_id)
            }
            (GameNodePage::Library, None) => return Err(GameNodePageError::NoSession),
        };
        self.shared
            .runtime
            .spawn_blocking(move || browser.open(&url))
            .await
            .map_err(|_| GameNodePageError::Browser)?
            .map_err(|_| GameNodePageError::Browser)
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
        // The library does not own the local-games view; bridge the DRM-free discovery flag so
        // the scanning panel's pseudo row follows it even when no library input arrives.
        state
            .library
            .set_discovery_scanning(state.local.discovery_scanning);
        let library = state.library.view();
        // Achievements live in the engine cache, not in the library machine. The detail snapshot
        // carries only the compact summary; the full list comes from the async command.
        let library = library.map(|mut view| {
            if let Some(detail) = &mut view.detail {
                let provider = detail.row.provider.clone();
                detail.achievement_backend = state
                    .library
                    .provider(&provider)
                    .is_some_and(|provider| provider.achievement_plugin().is_some());
                if let Some((_, entry_id)) = detail.row.key.split_once(':') {
                    detail.achievements = state
                        .achievement_summaries
                        .get(&(provider, entry_id.to_owned()))
                        .map(gamenode_core::achievements::AchievementSummaryView::from);
                }
            }
            view
        });
        let store_accounts = self.store_accounts(library.as_ref(), state.library.providers());
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
        let running_sessions = if state.playtime.available {
            state.playtime.tracker.running_started_at()
        } else {
            Default::default()
        };
        snapshot.tracking = gamenode_core::snapshot::TrackingView {
            title: if running_sessions.is_empty() {
                String::new()
            } else {
                state.library.running_title().unwrap_or_default()
            },
            available: state.playtime.available,
            running: running_sessions.keys().cloned().collect(),
            session_started_at: running_sessions,
        };
        snapshot.tools = state.tools.clone();
        snapshot.store_accounts = store_accounts;
        snapshot.ports = state.ports_view.clone();
        snapshot
    }

    /// Provider account states, with the library's live counts and the providers' declared
    /// shared labels. Counts are zero while no GameNode account's library is open; the card
    /// itself still renders.
    fn store_accounts(
        &self,
        library: Option<&gamenode_core::library::LibraryView>,
        providers: &[gamenode_core::provider::Provider],
    ) -> Vec<StoreAccountView> {
        self.provider_account_views(library, providers)
    }

    fn publish(&self, state: &mut State) {
        self.sync_library_feed(state);
        (self.shared.publish)(self.snapshot_of(state));
    }

    /// Feeds the plugin library projection. Plugins see the entries the library shows while an
    /// account's library is open, each with its current GameNode match; never a store id, path
    /// or scan field. An entry stays hidden until the storage worker has given it a durable
    /// installation id, so an id a plugin saw never changes.
    fn sync_library_feed(&self, state: &State) {
        use crate::plugin_host::library::{InstallState, LibrarySource};
        let Some(feed) = self
            .shared
            .library_feed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        else {
            return;
        };
        let library = &state.library;
        let sources = if library.is_active() {
            library
                .visible_entries()
                .map(|entry| {
                    let game_id = library.match_status(entry).game_id();
                    LibrarySource {
                        entry_key: entry.key.clone(),
                        provider_id: entry.provider.clone(),
                        game_id,
                        name: entry
                            .name
                            .clone()
                            .filter(|name| !name.trim().is_empty())
                            .or_else(|| {
                                game_id
                                    .and_then(|id| library.game(id))
                                    .map(|g| g.name.clone())
                            })
                            .unwrap_or_default(),
                        display_name: library
                            .display_override(&entry.key)
                            .and_then(|over| over.display_name.clone()),
                        metadata_revision: library
                            .display_override(&entry.key)
                            .map_or(0, |over| over.revision),
                        artwork_revision: library
                            .artwork_override(&entry.key)
                            .map_or(0, |over| over.revision),
                        install_dir: entry.install_dir.clone(),
                        detected_target: entry
                            .target
                            .as_ref()
                            .map(|target| (target.revision, target.approved, target.available)),
                        install_state: if entry.installed {
                            InstallState::Installed
                        } else if entry.owned {
                            InstallState::Owned
                        } else {
                            InstallState::Shared
                        },
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let missing = feed.sync(sources);
        if missing.is_empty() {
            return;
        }
        let store = self.shared.ports.library.clone();
        self.shared
            .storage
            .submit(move || match store.installation_ids(&missing) {
                Ok(ids) => feed.assign_ids(&missing, ids),
                Err(error) => {
                    tracing::warn!(%error, "could not assign library installation ids");
                    feed.ids_failed(&missing);
                }
            });
    }

    /// Connects the plugin library projection and its write side, and fills the projection from
    /// the current library.
    pub fn set_library_feed(&self, feed: Arc<crate::plugin_host::library::LibraryFeed>) {
        feed.set_host(Arc::new(PluginLibraryHost {
            shared: Arc::downgrade(&self.shared),
        }));
        *self
            .shared
            .library_feed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(feed);
        let state = self.lock();
        self.sync_library_feed(&state);
    }

    pub fn set_selected_launch_api(&self, api: Arc<crate::plugin_host::resources::ResourceApi>) {
        *self
            .shared
            .selected_launch_api
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(api);
    }

    /// Reads the last signed-in account, then the stored session and, if there is one,
    /// validates it. The account is read first so the cached library can open while the session
    /// is still being verified.
    pub fn start(&self) {
        self.start_providers();
        self.recover_local_jobs();
        self.refresh_local_games();
        self.start_local_discovery();
        self.start_local_manifest_updates();
        self.load_achievement_cache();
        self.start_achievement_reconciliation();
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

    /// Drops one provider's owned and shared facts and persists the clear. Entries, matches and
    /// covers are kept. While the library is loading its cache the in-memory clear is deferred
    /// until the cache is installed, so the old account's cached facts cannot come back.
    fn clear_store_catalog(&self, provider: &str) {
        let stored = provider.to_owned();
        self.persist(move |store| store.clear_catalog(&stored));
        self.clear_store_catalog_memory(provider);
    }

    /// Applies a catalog clear already persisted by a provider-owned account operation.
    fn clear_store_catalog_memory(&self, provider: &str) {
        let mut state = self.lock();
        state.catalog_request_pending = false;
        if state.library_loading || !state.library.is_active() {
            state.catalog_clear_pending.insert(provider.to_owned());
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
        if !self
            .provider_runtime()
            .is_some_and(|runtime| runtime.has_provider(&provider))
        {
            // Complete the core op even without a registered runtime: nothing was attempted.
            self.later(LibInput::CatalogSynced {
                op,
                provider,
                owned: None,
                shared: None,
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
        self.sync_provider_catalog(op, provider.to_owned()).await;
    }

    /// Feeds an input to the state machine and launches its effects on a worker. An account
    /// transition can wait for an in-flight shared-data policy commit.
    pub async fn dispatch_on_worker(&self, input: Input) {
        let engine = self.clone();
        if let Err(error) = tokio::task::spawn_blocking(move || engine.dispatch(input)).await {
            tracing::error!(%error, "auth dispatch worker failed");
        }
    }

    /// Called by blocking workers and tests. Take the profile transition lock before the engine
    /// lock so unrelated UI work can read the engine state while a policy commit is syncing.
    pub fn dispatch(&self, input: Input) {
        let profiles = self
            .shared
            .plugin_profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let transition = profiles.as_ref().map(|profiles| profiles.lock_transition());
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
        self.sync_library(&mut state, transition);
        self.sync_tracker(&mut state);
        if submit_trigger {
            self.submit_totals(&mut state);
        }
        self.publish(&mut state);
    }

    /// Feeds an input to the library and launches its effects. Never blocks on I/O.
    pub fn library_input(&self, input: LibInput) {
        // PC ports speaks `drmfree:<id>` (the UI contract) while the DRM-free provider's own
        // entry keys are `manual:<id>`. Translate a selection coming from the catalog so "Open in
        // library" finds the entry.
        let input = match input {
            LibInput::Select { key, mode } if key.starts_with("drmfree:") => LibInput::Select {
                key: format!("manual:{}", key.trim_start_matches("drmfree:")),
                mode,
            },
            other => other,
        };
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
                    | LibInput::MarkEntryNoGame { .. }
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
            if !state.catalog_clear_pending.is_empty() {
                let providers = std::mem::take(&mut state.catalog_clear_pending);
                for provider in providers {
                    self.run_library(&mut state, LibInput::CatalogCleared { provider });
                }
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
        let selected_achievement_key =
            state
                .library
                .view()
                .and_then(|view| match view.selected.as_slice() {
                    [key] => Some(key.clone()),
                    _ => None,
                });
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
        drop(state);
        if finished_loading {
            self.load_achievement_cache();
            self.request_import_achievements();
        }
        if let Some(key) = selected_achievement_key {
            self.refresh_selected_achievements(key);
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
        state
            .library
            .set_discovery_scanning(state.local.discovery_scanning);
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
        self.open_entry_then(entry_key, kind, |_| {});
    }

    /// Loads active recipe choices for a matched game without blocking the UI thread.
    pub async fn launch_recipe_choices(
        &self,
        entry_key: String,
    ) -> Result<LaunchRecipeChoices, String> {
        let (game_id, target_revision, detected_default_available) = {
            let state = self.lock();
            let entry = state
                .library
                .entry(&entry_key)
                .ok_or_else(|| "library entry is unavailable".to_owned())?;
            let game_id = state.library.match_status(entry).game_id().ok_or_else(|| {
                "choose a GameNode game before selecting a launch recipe".to_owned()
            })?;
            let target = entry.target.as_ref();
            (
                game_id,
                target.map_or(0, |target| target.revision),
                target.is_some_and(|target| target.available),
            )
        };
        let profiles = self
            .shared
            .plugin_profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "plugin profiles are unavailable".to_owned())?;
        let store = self.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || {
            let selection = store
                .launch_recipe_selection(&entry_key)
                .map_err(|error| error.to_string())?;
            let identities = profiles.identities().map_err(|error| error.to_string())?;
            let mut recipes = Vec::new();
            for identity in identities {
                if !profiles
                    .is_enabled(&identity.plugin_id)
                    .map_err(|error| error.to_string())?
                {
                    continue;
                }
                let resolver = profiles
                    .resolver(&identity.plugin_id, &identity.publisher_fingerprint, None)
                    .map_err(|error| error.to_string())?;
                let owner_key = resolver.library_recipe_owner_key();
                for recipe in store
                    .custom_launch_recipes(&owner_key, game_id)
                    .map_err(|error| error.to_string())?
                {
                    if recipe.base_target_revision == target_revision && recipe.validate() {
                        recipes.push(LaunchRecipeOption {
                            plugin_id: identity.plugin_id.clone(),
                            publisher_fingerprint: identity.publisher_fingerprint.clone(),
                            recipe_id: recipe.recipe_id,
                            recipe_revision: recipe.revision,
                        });
                    }
                }
            }
            recipes.sort_by(|left, right| {
                (&left.plugin_id, &left.recipe_id).cmp(&(&right.plugin_id, &right.recipe_id))
            });
            Ok(LaunchRecipeChoices {
                target_revision,
                detected_default_available,
                selected: selection,
                recipes,
            })
        })
        .await
        .map_err(|_| "launch recipe worker stopped".to_owned())?
    }

    /// Selects an authenticated plugin recipe only while its game and detected target revisions
    /// still match the current library entry.
    pub async fn select_launch_recipe(
        &self,
        entry_key: String,
        plugin_id: String,
        publisher_fingerprint: String,
        recipe_id: String,
        recipe_revision: u64,
        expected_target_revision: u64,
    ) -> Result<(), String> {
        let (game_id, target_revision) = {
            let state = self.lock();
            let entry = state
                .library
                .entry(&entry_key)
                .ok_or_else(|| "library entry is unavailable".to_owned())?;
            let game_id = state.library.match_status(entry).game_id().ok_or_else(|| {
                "choose a GameNode game before selecting a launch recipe".to_owned()
            })?;
            let target_revision = entry.target.as_ref().map_or(0, |target| target.revision);
            (game_id, target_revision)
        };
        if target_revision != expected_target_revision || target_revision == 0 {
            return Err("the detected target changed; refresh launch choices".into());
        }
        let profiles = self
            .shared
            .plugin_profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "plugin profiles are unavailable".to_owned())?;
        let store = self.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || {
            use gamenode_core::library::{LaunchRecipeSelection, LaunchRecipeSelectionWrite};

            let identity = profiles
                .identities()
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|identity| {
                    identity.plugin_id == plugin_id
                        && identity.publisher_fingerprint == publisher_fingerprint
                })
                .ok_or_else(|| {
                    "plugin identity is not authenticated for this profile".to_owned()
                })?;
            if !profiles
                .is_enabled(&identity.plugin_id)
                .map_err(|error| error.to_string())?
            {
                return Err("the recipe's plugin is disabled".into());
            }
            let resolver = profiles
                .resolver(&identity.plugin_id, &identity.publisher_fingerprint, None)
                .map_err(|error| error.to_string())?;
            let owner_key = resolver.library_recipe_owner_key();
            let recipe = store
                .custom_launch_recipes(&owner_key, game_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|recipe| {
                    recipe.recipe_id == recipe_id
                        && recipe.revision == recipe_revision
                        && recipe.base_target_revision == expected_target_revision
                        && recipe.validate()
                })
                .ok_or_else(|| {
                    "the selected recipe changed or is no longer available".to_owned()
                })?;
            let selection = LaunchRecipeSelection {
                entry_key: entry_key.clone(),
                owner_key,
                plugin_id: identity.plugin_id,
                publisher_fingerprint: identity.publisher_fingerprint,
                recipe_id: recipe.recipe_id,
                recipe_revision: recipe.revision,
                base_target_revision: expected_target_revision,
                approved_plan_fingerprint: String::new(),
            };
            match store
                .save_launch_recipe_selection(&selection, expected_target_revision)
                .map_err(|error| error.to_string())?
            {
                LaunchRecipeSelectionWrite::Applied => Ok(()),
                LaunchRecipeSelectionWrite::Conflict => {
                    Err("the detected target or recipe changed; refresh launch choices".into())
                }
                LaunchRecipeSelectionWrite::RecipeMissing => {
                    Err("the selected recipe is no longer available".into())
                }
            }
        })
        .await
        .map_err(|_| "launch recipe worker stopped".to_owned())?
    }

    /// Clears a custom recipe so the detected default is selected explicitly.
    pub async fn use_detected_default(
        &self,
        entry_key: String,
        expected_target_revision: u64,
    ) -> Result<(), String> {
        let current_revision = {
            let state = self.lock();
            let entry = state
                .library
                .entry(&entry_key)
                .ok_or_else(|| "library entry is unavailable".to_owned())?;
            entry.target.as_ref().map_or(0, |target| target.revision)
        };
        if current_revision != expected_target_revision {
            return Err("the detected target changed; refresh launch choices".into());
        }
        let store = self.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || {
            match store
                .clear_launch_recipe_selection(&entry_key, expected_target_revision)
                .map_err(|error| error.to_string())?
            {
                gamenode_core::library::LaunchRecipeSelectionWrite::Applied => Ok(()),
                gamenode_core::library::LaunchRecipeSelectionWrite::Conflict => {
                    Err("the detected target changed; refresh launch choices".into())
                }
                gamenode_core::library::LaunchRecipeSelectionWrite::RecipeMissing => {
                    Err("library entry is unavailable".into())
                }
            }
        })
        .await
        .map_err(|_| "launch recipe worker stopped".to_owned())?
    }

    /// [`open_entry`](Self::open_entry) that also reports how it ended to `done`, which runs
    /// exactly once: on the worker when the link was handled, or at once when nothing was
    /// started (the same opening is already under way, the game is already running, or the
    /// entry is gone).
    pub(crate) fn open_entry_then(
        &self,
        entry_key: String,
        kind: LaunchKind,
        done: impl FnOnce(Result<(), OpenError>) + Send + 'static,
    ) {
        let mut state = self.lock();
        if state.launch.as_ref().is_some_and(|launch| {
            launch.entry_key == entry_key
                && launch.kind == kind
                && matches!(launch.state, LaunchState::Opening)
        }) {
            drop(state);
            return done(Err(OpenError::Busy));
        }
        if kind == LaunchKind::Play && state.playtime.tracker.running().contains(&entry_key) {
            drop(state);
            return done(Err(OpenError::Busy));
        }
        let Some(entry) = state.library.entry(&entry_key).cloned() else {
            drop(state);
            return done(Err(OpenError::NotFound));
        };
        let Some(provider) = state.library.provider(&entry.provider).cloned() else {
            drop(state);
            return done(Err(OpenError::NotFound));
        };
        state.launch_seq += 1;
        let seq = state.launch_seq;
        state.launch = Some(LaunchStatus {
            entry_key: entry_key.clone(),
            kind,
            state: LaunchState::Opening,
        });
        self.publish(&mut state);
        let (engine, opener) = (self.clone(), self.shared.ports.uri.clone());
        self.shared.runtime.spawn(async move {
            let selection_store = engine.shared.ports.library.clone();
            let selection_key = entry_key.clone();
            let selection = if kind == LaunchKind::Play {
                tokio::task::spawn_blocking(move || {
                    selection_store.launch_recipe_selection(&selection_key)
                })
                .await
                .map_err(|_| gamenode_core::library::LaunchError::Failed)
                .and_then(|result| result.map_err(|_| gamenode_core::library::LaunchError::Failed))
            } else {
                Ok(None)
            };
            let run_detected = {
                let engine = engine.clone();
                let opener = opener.clone();
                let entry = entry.clone();
                let provider = provider.clone();
                move || {
                    if kind == LaunchKind::Play
                        && let Some(target) = entry.target.as_ref()
                    {
                        detected_target::launch(&engine, &entry, target)
                    } else if provider.is_manual_executable() {
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
                                    let environment =
                                        engine.port_launch_environment(&registration.id);
                                    gamenode_core::library::launch::launch_manual_with_environment(
                                        &**host,
                                        &provider,
                                        &entry.record(),
                                        registration,
                                        &environment,
                                        kind,
                                    )
                                })
                        }
                    } else {
                        open_action(&*opener, &provider, &entry.record(), kind)
                    }
                }
            };
            let result = match selection {
                Err(error) => Err(error),
                Ok(Some(_)) => {
                    let api = engine
                        .shared
                        .selected_launch_api
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    match api {
                        Some(api) => match api.launch_selected_recipe(entry_key).await {
                            Ok(true) => Ok(()),
                            Ok(false) => Ok(()),
                            Err(status) => Err(match status.code() {
                                tonic::Code::Unimplemented => {
                                    gamenode_core::library::LaunchError::Unsupported
                                }
                                tonic::Code::FailedPrecondition
                                | tonic::Code::PermissionDenied
                                | tonic::Code::NotFound => {
                                    gamenode_core::library::LaunchError::RecipeNeedsAttention
                                }
                                _ => gamenode_core::library::LaunchError::Failed,
                            }),
                        },
                        None => Err(gamenode_core::library::LaunchError::RecipeNeedsAttention),
                    }
                }
                Ok(None) => tokio::task::spawn_blocking(run_detected)
                    .await
                    .unwrap_or(Err(gamenode_core::library::LaunchError::Failed)),
            };
            let mut state = engine.lock();
            let outcome = result.clone().map_err(OpenError::Launch);
            if state.launch_seq == seq {
                state.launch = result.err().map(|error| LaunchStatus {
                    state: LaunchState::Failed(error),
                    ..state.launch.clone().expect("launch is set while opening")
                });
                engine.publish(&mut state);
            }
            drop(state);
            done(outcome);
        });
    }

    /// Hides a launch error.
    pub fn dismiss_launch(&self) {
        let mut state = self.lock();
        state.launch_seq += 1;
        state.launch = None;
        self.publish(&mut state);
    }

    /// Binds third-party plugin profiles to who is signed in. The engine applies the same account
    /// it opens the library for, so plugins see the cached last account while a restored session
    /// is unverified, and no profile at all after sign-out or a rejected session.
    pub fn set_plugin_profiles(&self, profiles: Arc<crate::plugin_host::profile::PluginProfiles>) {
        let transition = profiles.lock_transition();
        *self
            .shared
            .plugin_profiles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(profiles.clone());
        let mut state = self.lock();
        self.sync_library(&mut state, Some(transition));
    }

    /// Brings the library in line with who is signed in: opens the cached library of the last
    /// account while the session is being verified, enables GameNode features once it is, and
    /// closes the library on sign-out or a rejected session.
    fn sync_library(
        &self,
        state: &mut State,
        transition: Option<crate::plugin_host::profile::LockedProfileTransition<'_>>,
    ) {
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
        if let Some(transition) = transition {
            transition.update(
                account.as_ref().map(|account| account.id.as_str()),
                access == Access::Verified,
            );
        }
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
    async fn check_rejected(&self, rejected: bool) {
        if rejected {
            self.dispatch_on_worker(Input::SessionRejected).await;
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
                // The progress row moves from waiting to scanning before the work starts. The
                // input is queued, so a very fast answer may arrive first; it then stays done.
                self.later(LibInput::ProviderScanStarted {
                    op,
                    provider: provider.clone(),
                });
                let Some(found) = state.library.provider(&provider).cloned() else {
                    self.later(LibInput::ProviderScanned {
                        targets: Default::default(),
                        op,
                        provider,
                        result: Err("unknown provider".to_owned()),
                        now: unix_now(),
                    });
                    return;
                };
                if found.discovery_plugin().is_some() {
                    if let Some(runtime) = self
                        .provider_runtime()
                        .filter(|runtime| runtime.has_provider(&provider))
                    {
                        self.scan_provider(runtime, found, op, provider);
                    } else {
                        self.later(LibInput::ProviderScanned {
                            targets: Default::default(),
                            op,
                            provider,
                            result: Err("provider plugin is unavailable".to_owned()),
                            now: unix_now(),
                        });
                    }
                    return;
                }
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
                        targets: Default::default(),
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
                ScanOutcome::Scanned { records, targets } => {
                    store.save_scan(&provider, &records, &targets, now)
                }
                ScanOutcome::Failed(error) => store.save_scan_failure(&provider, &error),
            }),
            // The account service validates the session (persisting a rotated cookie) and fetches
            // the owned and shared catalogs; the engine applies the result here.
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
            LibEffect::PersistView {
                op,
                account_id,
                view,
            } => {
                // The insert's AUTOINCREMENT id is the only id the machine accepts for a new
                // view, so the answer goes back as an input instead of being guessed here.
                let (engine, store) = (self.clone(), ports.library.clone());
                self.shared.storage.submit(move || {
                    let result = store.save_view(&account_id, &view).map_err(|error| {
                        tracing::warn!(%error, "could not save the library view");
                    });
                    engine.library_input(LibInput::ViewPersisted {
                        op,
                        account_id,
                        result,
                    });
                });
            }
            LibEffect::DeleteViewEffect { account_id, id } => {
                self.persist(move |store| store.delete_view(&account_id, id))
            }
            LibEffect::PersistHidden {
                account_id,
                keys,
                hidden,
                now,
            } => self.persist(move |store| store.set_hidden(&account_id, &keys, hidden, now)),
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
                    engine.check_rejected(rejected).await;
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
                    engine.check_rejected(rejected).await;
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
                    engine.check_rejected(rejected).await;
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
                    engine.check_rejected(rejected).await;
                });
            }
            LibEffect::SuggestMatches { op, key, query } => {
                let Some(credential) = credential else {
                    return self.later(LibInput::SuggestionsFound {
                        op,
                        key,
                        result: Err(ApiError::Failed),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.search_page(&credential, &query).await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::SuggestionsFound { op, key, result });
                    engine.check_rejected(rejected).await;
                });
            }
            LibEffect::CatalogSearch { op, query } => {
                let Some(credential) = credential else {
                    return self.later(LibInput::CatalogSearchDone {
                        op,
                        query,
                        result: Err(ApiError::Failed),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.search_page(&credential, &query).await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::CatalogSearchDone { op, query, result });
                    engine.check_rejected(rejected).await;
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
                    engine.check_rejected(rejected).await;
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
                    engine.check_rejected(rejected).await;
                });
            }
            LibEffect::SyncAccountLibrary { op, user_id } => {
                let Some(credential) = credential else {
                    return self.later(LibInput::AccountLibrarySynced {
                        op,
                        result: Err(ApiError::Failed),
                        now: unix_now(),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    // The complete entry list and the collections are independent requests.
                    let (entries, collections) = tokio::join!(
                        api.list_library_entries(&credential, &user_id),
                        api.list_collections(&credential, &user_id)
                    );
                    let result = match (entries, collections) {
                        (Ok(entries), Ok(collections)) => Ok((entries, collections)),
                        (Err(error), _) | (_, Err(error)) => Err(error),
                    };
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::AccountLibrarySynced {
                        op,
                        result,
                        now: unix_now(),
                    });
                    engine.check_rejected(rejected).await;
                });
            }
            LibEffect::PersistAccountLibrary {
                account_id,
                entries,
                collections,
                read_at,
            } => self.persist(move |store| {
                store.replace_account_library(&account_id, &entries, &collections, read_at)
            }),
            LibEffect::PersistAccountEntry {
                account_id,
                game_id,
                entry,
            } => self.persist(move |store| {
                store.save_account_entry(&account_id, game_id, entry.as_ref())
            }),
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
                    engine.check_rejected(rejected).await;
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
                    engine.check_rejected(rejected).await;
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
                    return self.later(LibInput::EntryUpdated {
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
                    engine.library_input(LibInput::EntryUpdated {
                        op,
                        game_id,
                        result,
                    });
                    engine.check_rejected(rejected).await;
                });
            }
            LibEffect::DeleteEntry {
                op,
                game_id,
                entry_id,
            } => {
                let Some(credential) = credential else {
                    return self.later(LibInput::EntryDeleted {
                        op,
                        game_id,
                        result: Err(ApiError::Failed),
                    });
                };
                let (engine, api) = (self.clone(), ports.games.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.delete_entry(&credential, &entry_id).await;
                    let rejected = matches!(result, Err(ApiError::Rejected));
                    engine.library_input(LibInput::EntryDeleted {
                        op,
                        game_id,
                        result,
                    });
                    engine.check_rejected(rejected).await;
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
                            engine
                                .dispatch_on_worker(Input::SignInFailed { op, reason })
                                .await;
                            return;
                        }
                    };
                    // Mark the browser as opened first so a prompt callback is never rejected
                    // for arriving "too early"; a launch failure then cancels the attempt.
                    engine.dispatch_on_worker(Input::BrowserOpened { op }).await;
                    let opened = tokio::task::spawn_blocking(move || browser.open(&url)).await;
                    if !matches!(opened, Ok(Ok(()))) {
                        engine
                            .dispatch_on_worker(Input::SignInFailed {
                                op,
                                reason: SignInFailure::BrowserLaunch,
                            })
                            .await;
                    }
                });
            }
            Effect::VerifySession { op, credential } => {
                let (engine, api) = (self.clone(), ports.api.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.get_session(&credential).await;
                    engine
                        .dispatch_on_worker(Input::SessionChecked { op, result })
                        .await;
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
            engine
                .dispatch_on_worker(Input::SignOutFinished {
                    op,
                    remote,
                    local_deleted,
                })
                .await;
        });
    }
}

#[cfg(test)]
mod tests;

/// The engine's side of plugin library services. It holds the engine weakly, so the projection
/// the engine feeds does not keep the engine alive.
struct PluginLibraryHost {
    shared: std::sync::Weak<Shared>,
}

impl PluginLibraryHost {
    fn engine(&self) -> Result<Engine, tonic::Status> {
        self.shared
            .upgrade()
            .map(|shared| Engine { shared })
            .ok_or_else(|| tonic::Status::unavailable("the library is not available"))
    }
}

#[async_trait::async_trait]
impl crate::plugin_host::library::LibraryHost for PluginLibraryHost {
    async fn register_runtime_owner(
        &self,
        owner: gamenode_core::library::RuntimeOwnerIdentity,
    ) -> Result<(), tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || store.register_runtime_owner(&owner))
            .await
            .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
            .map_err(|_| tonic::Status::internal("could not record runtime owner identity"))
    }

    async fn runtime_catalog_records(
        &self,
        profile_id: String,
    ) -> Result<Vec<gamenode_core::library::RuntimeCatalogRecord>, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || store.runtime_catalog_records(&profile_id))
            .await
            .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
            .map_err(|_| tonic::Status::internal("could not read common runtime catalog"))
    }

    async fn runtime_common_revision(&self) -> Result<u64, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || store.runtime_common_revision())
            .await
            .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
            .map_err(|_| tonic::Status::internal("could not read common runtime revision"))
    }

    async fn runtime_resources(
        &self,
        owner_key: String,
    ) -> Result<Vec<gamenode_core::library::RuntimeResource>, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || store.runtime_resources(&owner_key))
            .await
            .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
            .map_err(|_| tonic::Status::internal("could not read runtime resources"))
    }

    async fn runtime_resource(
        &self,
        owner_key: String,
        runtime_id: String,
    ) -> Result<Option<gamenode_core::library::RuntimeResource>, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || store.runtime_resource(&owner_key, &runtime_id))
            .await
            .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
            .map_err(|_| tonic::Status::internal("could not read runtime resource"))
    }

    async fn save_runtime_resource(
        &self,
        owner_key: String,
        runtime: gamenode_core::library::RuntimeResource,
    ) -> Result<gamenode_core::library::RuntimeMutation, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || store.save_runtime_resource(&owner_key, &runtime))
            .await
            .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
            .map_err(|_| tonic::Status::internal("could not save runtime resource"))
    }

    async fn remove_runtime_resource(
        &self,
        owner_key: String,
        runtime_id: String,
        expected_revision: u64,
    ) -> Result<gamenode_core::library::RuntimeMutation, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || {
            store.remove_runtime_resource(&owner_key, &runtime_id, expected_revision)
        })
        .await
        .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
        .map_err(|_| tonic::Status::internal("could not remove runtime resource"))
    }

    async fn runtime_content_bindings(
        &self,
        owner_key: String,
        game_id: u64,
        installation_id: String,
    ) -> Result<Vec<gamenode_core::library::RuntimeContentBinding>, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || {
            store.runtime_content_bindings(&owner_key, game_id, &installation_id)
        })
        .await
        .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
        .map_err(|_| tonic::Status::internal("could not read runtime content bindings"))
    }

    async fn save_runtime_content_binding(
        &self,
        owner_key: String,
        binding: gamenode_core::library::RuntimeContentBinding,
    ) -> Result<gamenode_core::library::RuntimeMutation, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || {
            store.save_runtime_content_binding(&owner_key, &binding)
        })
        .await
        .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
        .map_err(|_| tonic::Status::internal("could not save runtime content binding"))
    }

    async fn remove_runtime_content_binding(
        &self,
        owner_key: String,
        binding: gamenode_core::library::RuntimeContentBinding,
    ) -> Result<gamenode_core::library::RuntimeMutation, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || {
            store.remove_runtime_content_binding(&owner_key, &binding)
        })
        .await
        .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
        .map_err(|_| tonic::Status::internal("could not remove runtime content binding"))
    }

    async fn runtime_catalog_revision(&self, owner_key: String) -> Result<u64, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || store.runtime_catalog_revision(&owner_key))
            .await
            .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
            .map_err(|_| tonic::Status::internal("could not read runtime catalog revision"))
    }

    async fn custom_launch_recipe(
        &self,
        owner_key: String,
        recipe_id: String,
    ) -> Result<Option<gamenode_core::library::CustomLaunchRecipe>, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || store.custom_launch_recipe(&owner_key, &recipe_id))
            .await
            .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
            .map_err(|_| tonic::Status::internal("could not read custom launch recipe"))
    }

    async fn save_custom_launch_recipe(
        &self,
        owner_key: String,
        recipe: gamenode_core::library::CustomLaunchRecipe,
    ) -> Result<gamenode_core::library::CustomLaunchMutation, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || store.save_custom_launch_recipe(&owner_key, &recipe))
            .await
            .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
            .map_err(|_| tonic::Status::internal("could not save custom launch recipe"))
    }

    async fn remove_custom_launch_recipe(
        &self,
        owner_key: String,
        recipe_id: String,
        expected_revision: u64,
    ) -> Result<gamenode_core::library::CustomLaunchMutation, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || {
            store.remove_custom_launch_recipe(&owner_key, &recipe_id, expected_revision)
        })
        .await
        .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
        .map_err(|_| tonic::Status::internal("could not remove custom launch recipe"))
    }

    async fn launch_recipe_selection(
        &self,
        entry_key: String,
    ) -> Result<Option<gamenode_core::library::LaunchRecipeSelection>, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || store.launch_recipe_selection(&entry_key))
            .await
            .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
            .map_err(|_| tonic::Status::internal("could not read launch recipe selection"))
    }

    async fn approve_launch_recipe_plan(
        &self,
        entry_key: String,
        recipe_id: String,
        recipe_revision: u64,
        target_revision: u64,
        fingerprint: String,
    ) -> Result<gamenode_core::library::LaunchRecipeSelectionWrite, tonic::Status> {
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        tokio::task::spawn_blocking(move || {
            store.approve_launch_recipe_plan(
                &entry_key,
                &recipe_id,
                recipe_revision,
                target_revision,
                &fingerprint,
            )
        })
        .await
        .map_err(|_| tonic::Status::unavailable("library worker stopped"))?
        .map_err(|_| tonic::Status::internal("could not remember launch plan"))
    }

    async fn prepare_custom_launch(
        &self,
        recipe: gamenode_core::library::CustomLaunchRecipe,
        executable_path: Option<String>,
        working_directory_path: Option<String>,
        runtime: Option<crate::plugin_host::library::ResolvedRuntimeLaunch>,
    ) -> Result<crate::plugin_host::library::CustomLaunchDispatch, tonic::Status> {
        let engine = self.engine()?;
        let ports = engine.shared.ports.clone();
        tokio::task::spawn_blocking(move || {
            use crate::plugin_host::library::CustomLaunchDispatch;
            use gamenode_core::drm_free::CustomLaunchCommand;
            if !recipe.validate() {
                return Err(tonic::Status::invalid_argument(
                    "invalid custom launch recipe",
                ));
            }
            if !recipe.uri.is_empty() {
                let uri = url::Url::parse(&recipe.uri)
                    .map_err(|_| tonic::Status::invalid_argument("invalid launch URI"))?;
                let environment = recipe.environment();
                return Ok(CustomLaunchDispatch::Uri {
                    uri,
                    arguments: recipe.arguments,
                    working_directory: working_directory_path.unwrap_or_default(),
                    environment,
                });
            }
            if let Some(runtime) = runtime {
                if !recipe.executable_handle.is_empty() || recipe.executable.is_some() {
                    return Err(tonic::Status::invalid_argument(
                        "a runtime recipe cannot also select a direct executable",
                    ));
                }
                if recipe.runtime_handle != runtime.runtime_id {
                    return Err(tonic::Status::failed_precondition(
                        "the selected runtime changed during launch preparation",
                    ));
                }
                let installations = ports.installations.ok_or_else(|| {
                    tonic::Status::unimplemented("runtime launch is unavailable on this platform")
                })?;
                let registration = installations
                    .inspect(&runtime.executable)
                    .map_err(|error| match error {
                        gamenode_core::drm_free::LocalError::Unsupported => {
                            tonic::Status::unimplemented(
                                "runtime launch is unavailable on this platform",
                            )
                        }
                        gamenode_core::drm_free::LocalError::InvalidTarget => {
                            tonic::Status::failed_precondition(
                                "the approved runtime executable changed or is invalid",
                            )
                        }
                        _ => tonic::Status::unavailable("could not inspect approved runtime"),
                    })?;
                let mut arguments =
                    Vec::with_capacity(runtime.arguments.len() + recipe.arguments.len());
                for argument in runtime.arguments {
                    match argument {
                        gamenode_core::library::RuntimeArgumentTemplate::Literal(value) => {
                            arguments.push(value)
                        }
                        gamenode_core::library::RuntimeArgumentTemplate::ContentPath => {
                            let path = runtime.content_path.as_ref().ok_or_else(|| {
                                tonic::Status::failed_precondition(
                                    "the selected runtime requires a content binding",
                                )
                            })?;
                            arguments.push(path.clone());
                        }
                    }
                }
                arguments.extend(recipe.arguments.iter().cloned());
                let working_directory = working_directory_path
                    .unwrap_or_else(|| registration.working_directory.clone());
                let command = CustomLaunchCommand {
                    executable: registration.executable,
                    file_identity: registration.file_identity,
                    arguments,
                    working_directory,
                    environment: recipe.environment(),
                };
                return Ok(CustomLaunchDispatch::Runtime {
                    command,
                    runtime_id: runtime.runtime_id,
                    runtime_version: runtime.version,
                    runtime_revision: runtime.runtime_revision,
                    content_id: runtime.content_id,
                    binding_revision: runtime.binding_revision,
                });
            }
            if !recipe.runtime_handle.is_empty() {
                return Err(tonic::Status::failed_precondition(
                    "the selected runtime could not be resolved",
                ));
            }
            let executable = executable_path.ok_or_else(|| {
                tonic::Status::permission_denied("executable handle is unavailable")
            })?;
            let installations = ports.installations.ok_or_else(|| {
                tonic::Status::unimplemented("custom executable launch is unavailable")
            })?;
            let registration = installations
                .inspect(&executable)
                .map_err(|error| match error {
                    gamenode_core::drm_free::LocalError::Unsupported => {
                        tonic::Status::unimplemented("custom executable launch is unavailable")
                    }
                    gamenode_core::drm_free::LocalError::InvalidTarget => {
                        tonic::Status::failed_precondition(
                            "approved executable changed or is invalid",
                        )
                    }
                    _ => tonic::Status::unavailable("could not inspect approved executable"),
                })?;
            let working_directory =
                working_directory_path.unwrap_or_else(|| registration.working_directory.clone());
            let environment = recipe.environment();
            let command = CustomLaunchCommand {
                executable: registration.executable,
                file_identity: registration.file_identity,
                arguments: recipe.arguments,
                working_directory,
                environment,
            };
            Ok(CustomLaunchDispatch::Executable(command))
        })
        .await
        .map_err(|_| tonic::Status::unavailable("custom launch preparation worker stopped"))?
    }

    async fn launch_custom(
        &self,
        dispatch: crate::plugin_host::library::CustomLaunchDispatch,
    ) -> Result<(), tonic::Status> {
        let engine = self.engine()?;
        let ports = engine.shared.ports.clone();
        tokio::task::spawn_blocking(move || {
            use crate::plugin_host::library::CustomLaunchDispatch;
            use gamenode_core::ports::UriError;
            match dispatch {
                CustomLaunchDispatch::Uri {
                    uri,
                    arguments,
                    working_directory,
                    environment,
                } => ports
                    .uri
                    .open_custom(&uri, &arguments, &working_directory, &environment)
                    .map_err(|error| match error {
                        UriError::NoHandler => tonic::Status::failed_precondition(
                            "no application handles this launch URI",
                        ),
                        UriError::Unsupported => tonic::Status::unimplemented(
                            "the URI handler cannot apply custom launch fields",
                        ),
                        UriError::Failed => tonic::Status::unavailable("the launch URI failed"),
                    }),
                CustomLaunchDispatch::Executable(command) => {
                    let installations = ports.installations.ok_or_else(|| {
                        tonic::Status::unimplemented("custom executable launch is unavailable")
                    })?;
                    installations
                        .launch_custom(&command)
                        .map_err(|error| match error {
                            gamenode_core::drm_free::LocalError::Unsupported => {
                                tonic::Status::unimplemented(
                                    "custom executable launch is unavailable",
                                )
                            }
                            gamenode_core::drm_free::LocalError::InvalidTarget => {
                                tonic::Status::failed_precondition(
                                    "approved executable changed or is invalid",
                                )
                            }
                            _ => tonic::Status::unavailable("custom executable launch failed"),
                        })
                }
                CustomLaunchDispatch::Runtime { command, .. } => {
                    let installations = ports.installations.ok_or_else(|| {
                        tonic::Status::unimplemented(
                            "runtime launch is unavailable on this platform",
                        )
                    })?;
                    installations
                        .launch_custom(&command)
                        .map_err(|error| match error {
                            gamenode_core::drm_free::LocalError::Unsupported => {
                                tonic::Status::unimplemented(
                                    "runtime launch is unavailable on this platform",
                                )
                            }
                            gamenode_core::drm_free::LocalError::InvalidTarget => {
                                tonic::Status::failed_precondition(
                                    "approved runtime executable changed or is invalid",
                                )
                            }
                            _ => tonic::Status::unavailable("runtime launch failed"),
                        })
                }
            }
        })
        .await
        .map_err(|_| tonic::Status::unavailable("custom launch worker stopped"))?
    }

    fn ports_supported(&self) -> bool {
        self.engine()
            .ok()
            .and_then(|e| e.ports_service())
            .is_some_and(|s| s.is_supported())
    }
    async fn ports_catalog(
        &self,
        detail: Option<String>,
    ) -> Result<Vec<gamenode_plugin_protocol::proto::PortRecord>, tonic::Status> {
        self.engine()?.plugin_ports_catalog(detail).await
    }
    async fn request_port(
        &self,
        port_id: String,
        review: gamenode_core::ports::view::PortPluginRequest,
        check: crate::plugin_host::jobs::Fence,
    ) -> Result<(), tonic::Status> {
        self.engine()?
            .review_plugin_port(port_id, review, check)
            .await
    }

    fn tools_supported(&self) -> bool {
        self.engine().is_ok_and(|e| {
            cfg!(windows)
                && e.shared.ports.tool_installer.is_some()
                && e.shared.ports.tools.is_some()
                && e.shared.ports.tool_catalog.is_some()
        })
    }
    async fn tools_list(
        &self,
        entry_key: String,
    ) -> Result<Vec<gamenode_plugin_protocol::proto::ToolRecord>, tonic::Status> {
        let engine = self.engine()?;
        tokio::task::spawn_blocking(move || engine.plugin_tools_list(&entry_key))
            .await
            .map_err(|_| tonic::Status::unavailable("tools worker unavailable"))?
    }
    async fn request_tool(
        &self,
        entry_key: String,
        tool_id: String,
        review: gamenode_core::tools::ToolPluginRequest,
        check: crate::plugin_host::jobs::Fence,
    ) -> Result<(), tonic::Status> {
        self.engine()?
            .request_plugin_tool(entry_key, tool_id, review, check)
            .await
    }

    async fn backup_context(
        &self,
        entry_key: String,
    ) -> Result<crate::plugin_host::library::BackupHostContext, tonic::Status> {
        let engine = self.engine()?;
        tokio::task::spawn_blocking(move || {
            use gamenode_core::drm_free::BackupTarget;
            let adapter = engine
                .shared
                .ports
                .backups
                .clone()
                .filter(|adapter| adapter.scoped_available())
                .ok_or_else(|| {
                    tonic::Status::unavailable("backups are unavailable on this platform")
                })?;
            let (root, manual_id) = {
                let state = engine.lock();
                let entry = state
                    .library
                    .entry(&entry_key)
                    .filter(|entry| entry.installed)
                    .ok_or_else(|| tonic::Status::not_found("installed game not found"))?;
                (
                    entry.install_dir.clone(),
                    entry_key.strip_prefix("manual:").map(str::to_owned),
                )
            };
            let mut target = BackupTarget {
                installation_id: String::new(),
                root,
                gameplay: vec![],
                track_folder: manual_id.is_none(),
            };
            let local = engine.load_local_view();
            if let Some(id) = manual_id {
                let registration = local
                    .registrations
                    .iter()
                    .find(|r| r.id == id && !r.removed)
                    .ok_or_else(|| tonic::Status::not_found("installed game not found"))?;
                target.gameplay = registration.gameplay.clone();
            }
            if target.root.is_empty() {
                return Err(tonic::Status::failed_precondition(
                    "installation folder unavailable",
                ));
            }
            Ok(crate::plugin_host::library::BackupHostContext {
                engine: adapter,
                target,
                destination: local.settings.destination,
            })
        })
        .await
        .map_err(|_| tonic::Status::unavailable("backup context unavailable"))?
    }

    fn plugin_roots_supported(&self) -> bool {
        true
    }
    fn set_plugin_roots(
        &self,
        owner: String,
        roots: Vec<crate::plugin_host::library::PluginScanRoot>,
    ) -> Result<(), tonic::Status> {
        let engine = self.engine()?;
        let mut active = engine
            .shared
            .plugin_scan_roots
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let previous = active
            .get(&owner)
            .map(|roots| roots.iter().map(|r| r.root.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        let changed = previous != roots.iter().map(|r| r.root.clone()).collect::<Vec<_>>();
        if roots.is_empty() {
            active.remove(&owner);
        } else {
            active.insert(owner, roots);
        }
        drop(active);
        if changed || !previous.is_empty() {
            engine.rescan_local_folders();
        }
        Ok(())
    }

    fn favorites(&self, provider_id: &str) -> Vec<(u64, bool)> {
        let Ok(engine) = self.engine() else {
            return Vec::new();
        };
        let state = engine.lock();
        let library = &state.library;
        if !library.is_active() {
            return Vec::new();
        }
        let mut games = std::collections::BTreeMap::new();
        for entry in library
            .visible_entries()
            .filter(|entry| entry.provider == provider_id)
        {
            if let Some(game) = library.match_status(entry).game_id() {
                games.insert(
                    game,
                    library
                        .favorite_state(game)
                        .is_some_and(|favorite| favorite.is_favorite),
                );
            }
        }
        games.into_iter().collect()
    }

    fn set_favorite(
        &self,
        provider_id: &str,
        game_id: u64,
        favorite: bool,
        guard: &dyn Fn() -> Result<(), tonic::Status>,
    ) -> Result<crate::plugin_host::library::FavoriteWrite, tonic::Status> {
        use crate::plugin_host::library::FavoriteWrite;
        let engine = self.engine()?;
        let mut state = engine.lock();
        // Checked under the engine lock, in the same section that commits.
        guard()?;
        let library = &state.library;
        let key = library
            .is_active()
            .then(|| {
                library
                    .visible_entries()
                    .filter(|entry| entry.provider == provider_id)
                    .find(|entry| library.match_status(entry).game_id() == Some(game_id))
                    .map(|entry| entry.key.clone())
            })
            .flatten();
        let Some(key) = key else {
            return Ok(FavoriteWrite::NoMatchedEntry);
        };
        let is_favorite = |state: &State| {
            state
                .library
                .favorite_state(game_id)
                .is_some_and(|favorite| favorite.is_favorite)
        };
        if is_favorite(&state) == favorite {
            return Ok(FavoriteWrite::AlreadyApplied);
        }
        engine.run_library(
            &mut state,
            LibInput::SetFavorite {
                key,
                favorite,
                now: unix_now(),
            },
        );
        engine.publish(&mut state);
        if is_favorite(&state) != favorite {
            // The library had no account to keep the favorite for.
            return Err(tonic::Status::unavailable("the library is not available"));
        }
        Ok(FavoriteWrite::Applied)
    }

    async fn search_catalog(
        &self,
        query: String,
        limit: usize,
    ) -> Result<Vec<crate::plugin_host::library::CatalogGame>, tonic::Status> {
        use crate::plugin_host::library::CatalogGame;
        let engine = self.engine()?;
        let (credential, api) = {
            let state = engine.lock();
            // The catalog is the user's GameNode account: never searched without a verified,
            // online session.
            let credential = state
                .library
                .is_active()
                .then(|| state.library.network_block().is_none())
                .filter(|online| *online)
                .and_then(|_| state.machine.verified_credential().cloned());
            let Some(credential) = credential else {
                return Err(tonic::Status::unavailable(
                    "the GameNode catalog needs a verified, online session",
                ));
            };
            (credential, engine.shared.ports.games.clone())
        };
        match api.search_games(&credential, &query).await {
            Ok(mut games) => {
                games.truncate(limit);
                engine.library_input(LibInput::CatalogSearched {
                    games: games.clone(),
                });
                Ok(games
                    .into_iter()
                    .map(|game| CatalogGame {
                        id: game.id,
                        name: game.name,
                        release_year: game.release_year,
                    })
                    .collect())
            }
            Err(error) => {
                engine
                    .check_rejected(matches!(error, ApiError::Rejected))
                    .await;
                Err(tonic::Status::unavailable(
                    "the GameNode catalog could not answer",
                ))
            }
        }
    }

    fn assign_match(
        &self,
        entry_key: &str,
        game_id: u64,
        expected_revision: u64,
        guard: &dyn Fn() -> Result<(), tonic::Status>,
    ) -> Result<crate::plugin_host::library::MatchWrite, tonic::Status> {
        use crate::plugin_host::library::MatchWrite;
        use gamenode_core::library::PluginMatch;
        let engine = self.engine()?;
        let feed = engine
            .shared
            .library_feed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| tonic::Status::unavailable("the library is not available"))?;
        let mut state = engine.lock();
        // Checked under the engine lock, in the same section that commits.
        guard()?;
        // Bring the projection up to date under the same lock, so the revision compared is the
        // one the entry has now.
        engine.sync_library_feed(&state);
        let not_found = || tonic::Status::not_found("library entry not found");
        let current = feed.revision_of(entry_key).ok_or_else(not_found)?;
        let plan = state.library.plan_plugin_match(entry_key, game_id);
        match plan {
            PluginMatch::NoAccount => {
                return Err(tonic::Status::unavailable("the library is not available"));
            }
            PluginMatch::UnknownEntry => return Err(not_found()),
            PluginMatch::Already => {
                return Ok(MatchWrite::AlreadyApplied { revision: current });
            }
            _ => {}
        }
        if expected_revision != current {
            return Ok(MatchWrite::Conflict { current });
        }
        match plan {
            PluginMatch::UnknownGame => return Ok(MatchWrite::UnknownGame),
            PluginMatch::UserChoice => return Ok(MatchWrite::UserChoice),
            _ => {}
        }
        engine.run_library(
            &mut state,
            LibInput::AssignPluginMatch {
                key: entry_key.to_owned(),
                game_id,
                now: unix_now(),
            },
        );
        // Playtime is submitted per game, so a new match is a moment to submit, as for the
        // user's own pick.
        engine.submit_totals(&mut state);
        engine.publish(&mut state);
        if state.library.plan_plugin_match(entry_key, game_id) != PluginMatch::Already {
            return Err(tonic::Status::unavailable("the library is not available"));
        }
        let revision = feed.revision_of(entry_key).unwrap_or(current);
        Ok(MatchWrite::Applied { revision })
    }

    async fn read_artwork(
        &self,
        entry_key: String,
        max_bytes: u64,
    ) -> Result<crate::plugin_host::library::StoredArtwork, tonic::Status> {
        use gamenode_platform::plugin_artwork::{self, ReadError};
        let engine = self.engine()?;
        let (file, revision, dir) = {
            let state = engine.lock();
            let library = &state.library;
            (
                library
                    .is_active()
                    .then(|| library.entry_cover_file(&entry_key))
                    .flatten(),
                library
                    .artwork_override(&entry_key)
                    .map_or(0, |over| over.revision),
                engine.shared.ports.covers_dir.clone(),
            )
        };
        let file = file.ok_or_else(|| tonic::Status::not_found("the entry has no cover"))?;
        let name = file.clone();
        let read =
            tokio::task::spawn_blocking(move || plugin_artwork::read(&dir, &name, max_bytes))
                .await
                .map_err(|_| tonic::Status::unavailable("the library is not available"))?;
        match read {
            Ok(bytes) => Ok(crate::plugin_host::library::StoredArtwork {
                bytes,
                media_type: plugin_artwork::media_type_of(&file),
                revision,
            }),
            Err(ReadError::TooLarge) => Err(tonic::Status::resource_exhausted(
                "the cover is larger than max_bytes; ask for more",
            )),
            Err(ReadError::Unavailable) => Err(tonic::Status::not_found(
                "the cover is not on this device yet",
            )),
        }
    }

    async fn set_artwork(
        &self,
        entry_key: String,
        expected_revision: u64,
        image: Option<gamenode_platform::plugin_artwork::Prepared>,
        guard: crate::plugin_host::library::CommitGuard,
    ) -> Result<gamenode_core::library::ArtworkWrite, tonic::Status> {
        use gamenode_core::library::ArtworkWrite;
        use gamenode_platform::plugin_artwork;
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        let dir = engine.shared.ports.covers_dir.clone();
        let name = image
            .as_ref()
            .map(|image| plugin_artwork::file_name(&entry_key, image));
        let (done, written) = tokio::sync::oneshot::channel();
        {
            let (key, name) = (entry_key.clone(), name.clone());
            // The storage worker runs jobs in order, so this write cannot overtake or be
            // overtaken by other library writes; authorization is rechecked where it commits.
            engine.shared.storage.submit(move || {
                let result = guard().and_then(|()| {
                    // The file goes first and the row second: a crash in between leaves a file
                    // nothing refers to, never a row that points at nothing.
                    let existed = match (&image, &name) {
                        (Some(image), Some(name)) => {
                            Some(plugin_artwork::store(&dir, name, image).map_err(|error| {
                                tracing::warn!(%error, "could not store a plugin cover");
                                tonic::Status::unavailable("could not store the cover")
                            })?)
                        }
                        _ => None,
                    };
                    let outcome = store
                        .save_artwork(&key, expected_revision, name.as_deref())
                        .map_err(|error| {
                            tracing::warn!(%error, "could not record a plugin cover");
                            tonic::Status::unavailable("could not store the cover")
                        });
                    match &outcome {
                        Ok(ArtworkWrite::Applied { replaced, .. }) => {
                            if let Some(old) = replaced
                                && Some(old) != name.as_ref()
                            {
                                plugin_artwork::remove(&dir, old);
                            }
                        }
                        // The same file is the live one.
                        Ok(ArtworkWrite::Unchanged { .. }) => {}
                        // Nothing refers to the file just written, unless it was already there.
                        Ok(ArtworkWrite::Conflict { .. }) | Err(_) => {
                            if let (Some(false), Some(name)) = (existed, &name) {
                                plugin_artwork::remove(&dir, name);
                            }
                        }
                    }
                    outcome
                });
                let _ = done.send(result);
            });
        }
        let written = written
            .await
            .map_err(|_| tonic::Status::unavailable("the library is not available"))??;
        if let ArtworkWrite::Applied { revision, .. } = &written {
            engine.library_input(LibInput::ArtworkStored {
                key: entry_key,
                file: name,
                revision: *revision,
            });
        }
        Ok(written)
    }

    async fn launch_fingerprint(&self, entry_key: String) -> Result<u64, tonic::Status> {
        use sha2::{Digest, Sha256};
        let engine = self.engine()?;
        let (entry, provider) = {
            let state = engine.lock();
            let entry = state
                .library
                .entry(&entry_key)
                .cloned()
                .ok_or_else(|| tonic::Status::not_found("library entry not found"))?;
            let provider = state
                .library
                .provider(&entry.provider)
                .cloned()
                .ok_or_else(|| tonic::Status::not_found("library entry not found"))?;
            (entry, provider)
        };
        let worker = engine.clone();
        tokio::task::spawn_blocking(move || {
            let mut hash = Sha256::new();
            let mut part = |bytes: &[u8]| {
                hash.update((bytes.len() as u64).to_be_bytes());
                hash.update(bytes);
            };
            if provider.is_manual_executable() {
                let store = worker.shared.ports.local_games.as_ref().ok_or_else(|| {
                    tonic::Status::unimplemented("starting games is not available")
                })?;
                let registrations = store
                    .registrations()
                    .map_err(|_| tonic::Status::unavailable("the library is not available"))?;
                let registration = registrations
                    .iter()
                    .find(|r| r.key() == entry.key && !r.removed)
                    .ok_or_else(|| {
                        tonic::Status::failed_precondition("the game has no approved executable")
                    })?;
                part(b"manual");
                part(registration.id.as_bytes());
                part(registration.executable.as_bytes());
                part(registration.working_directory.as_bytes());
                for argument in &registration.arguments {
                    part(argument.as_bytes());
                }
                part(&registration.revision.to_be_bytes());
            } else {
                match provider.launch_uri(&entry.record()) {
                    Ok(uri) => {
                        part(b"uri");
                        part(uri.as_str().as_bytes());
                    }
                    Err(_) => part(b"none"),
                }
            }
            let digest = hash.finalize();
            Ok(u64::from_be_bytes(
                digest[..8].try_into().expect("a digest has eight bytes"),
            ))
        })
        .await
        .map_err(|_| tonic::Status::unavailable("the library is not available"))?
    }

    async fn launch_target_revision(&self, entry_key: String) -> Result<u64, tonic::Status> {
        let engine = self.engine()?;
        let state = engine.lock();
        state
            .library
            .entry(&entry_key)
            .and_then(|entry| entry.target.as_ref())
            .map(|target| target.revision)
            .ok_or_else(|| tonic::Status::failed_precondition("detected launch target changed"))
    }

    async fn open_entry(&self, entry_key: String, install: bool) -> Result<(), tonic::Status> {
        use gamenode_core::library::LaunchError;
        let engine = self.engine()?;
        let (done, opened) = tokio::sync::oneshot::channel();
        let kind = if install {
            LaunchKind::Install
        } else {
            LaunchKind::Play
        };
        engine.open_entry_then(entry_key, kind, move |result| {
            let _ = done.send(result);
        });
        match opened
            .await
            .map_err(|_| tonic::Status::unavailable("the library is not available"))?
        {
            Ok(()) => Ok(()),
            Err(OpenError::Busy) => Err(tonic::Status::failed_precondition(
                "the game is already running or being opened",
            )),
            Err(OpenError::NotFound) => Err(tonic::Status::not_found("library entry not found")),
            Err(OpenError::Launch(LaunchError::Unsupported)) => Err(tonic::Status::unimplemented(
                "this provider does not support that action",
            )),
            Err(OpenError::Launch(LaunchError::Incomplete)) => Err(
                tonic::Status::failed_precondition("the game is missing data the provider needs"),
            ),
            Err(OpenError::Launch(LaunchError::RecipeNeedsAttention)) => {
                Err(tonic::Status::failed_precondition(
                    "the selected launch recipe needs attention; review it or use the detected default",
                ))
            }
            Err(OpenError::Launch(LaunchError::NoHandler)) => Err(
                tonic::Status::failed_precondition("the store application is not available"),
            ),
            Err(OpenError::Launch(LaunchError::Failed)) => {
                Err(tonic::Status::unavailable("the link could not be opened"))
            }
        }
    }

    async fn set_display_name(
        &self,
        entry_key: String,
        expected_revision: u64,
        display_name: Option<String>,
        guard: crate::plugin_host::library::CommitGuard,
    ) -> Result<gamenode_core::library::DisplayWrite, tonic::Status> {
        use gamenode_core::library::DisplayWrite;
        let engine = self.engine()?;
        let store = engine.shared.ports.library.clone();
        let (done, written) = tokio::sync::oneshot::channel();
        {
            let (key, name) = (entry_key.clone(), display_name.clone());
            // The storage worker runs jobs in order, so this write cannot overtake or be
            // overtaken by other library writes; authorization is rechecked where it commits.
            engine.shared.storage.submit(move || {
                let result = guard().and_then(|()| {
                    store
                        .save_display_name(&key, expected_revision, name.as_deref())
                        .map_err(|error| {
                            tracing::warn!(%error, "could not store a display name");
                            tonic::Status::unavailable("could not store the display name")
                        })
                });
                let _ = done.send(result);
            });
        }
        let written = written
            .await
            .map_err(|_| tonic::Status::unavailable("the library is not available"))??;
        if let DisplayWrite::Applied { revision } = written {
            engine.library_input(LibInput::DisplayNameStored {
                key: entry_key,
                display_name,
                revision,
            });
        }
        Ok(written)
    }
}
