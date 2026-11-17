use std::collections::BTreeMap;
use std::fs::File;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use async_trait::async_trait;
use gamenode_core::callback::Provider;
use gamenode_core::credential::StorageMethod;
use gamenode_core::game_api::{
    AccountEntry, ApiError, Collection, CoverError, CreateCollectionError, ExternalMatch,
    FavoriteError, GameApi, GameSummary, LibraryEntry,
};
use gamenode_core::library::cover::CoverUrl;
use gamenode_core::library::{EntryStatus, LibraryMachine};
use gamenode_core::ports::{
    AuthApi, BrowserError, BrowserOpener, CredentialError, CredentialStore, SettingsStore,
    StorageError,
};
use gamenode_core::provider::{Platform, memory::FixedRoots};
use gamenode_core::{
    AppPreferenceError, AppPreferencePatch, AppPreferences, AuthView, Input, Locale, Notice,
    SessionCredential, SessionError, Settings, SignInFailure, Snapshot, User, WindowGeometry,
};
use tokio::sync::watch;
use url::Url;

use super::{AutostartHook, Engine, PortProgressPublisher, Ports};

mod achievements;
mod catalog;
mod drm_free;
mod favorites;
mod library;
mod pc_ports;
mod playtime;
pub(super) mod provider_runtime;
mod tools;
mod web_links;
use library::{FakeLibrary, FakeUri, SlowFiles};

/// A real in-memory achievement cache for engine fixtures.
pub(crate) fn test_achievement_store() -> Arc<dyn gamenode_core::achievements::AchievementStore> {
    Arc::new(
        gamenode_platform::achievement_db::SqliteAchievementStore::new(Arc::new(Mutex::new(
            gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
        ))),
    )
}

fn ana() -> User {
    User {
        id: "u1".into(),
        name: "Ana".into(),
        email: Some("ana@example.com".into()),
        image: None,
    }
}

fn cred(v: &str) -> SessionCredential {
    SessionCredential::new(v).unwrap()
}

#[derive(Default)]
struct FakeApi {
    playtimes: Mutex<Vec<gamenode_core::playtime::Submission>>,
    playtime_delay: Mutex<Duration>,
    /// When set, a submission with this total is rejected instead of accepted.
    playtime_reject_seconds: Mutex<Option<i64>>,
    delay: Mutex<Duration>,
    begin: Mutex<Option<SignInFailure>>,
    session: Mutex<Option<Result<User, SessionError>>>,
    sign_out_ok: Mutex<bool>,
    started: Mutex<Vec<(Provider, String, String)>>,
    verified: Mutex<Vec<String>>,
    signed_out: Mutex<Vec<String>>,
    /// What GameNode answers to a lookup (`None`: it knows no game).
    lookup_result: Mutex<Option<Result<Vec<ExternalMatch>, ApiError>>>,
    lookups: Mutex<Vec<(u32, Vec<String>)>>,
    covers_fetched: Mutex<Vec<String>>,
    artworks_fetched: Mutex<Vec<u64>>,
    artwork_result: Mutex<Option<Result<Option<Vec<u8>>, ApiError>>>,
    search_result: Mutex<Option<Result<gamenode_core::game_api::SearchPage, ApiError>>>,
    /// Every title that reached `search_page`, in order.
    searches: Mutex<Vec<String>>,
    entry: Mutex<Option<Result<Option<LibraryEntry>, ApiError>>>,
    /// What GameNode answers to the complete account library read and its collections.
    account_entries: Mutex<Option<Result<Vec<AccountEntry>, ApiError>>>,
    collections_result: Mutex<Option<Result<Vec<Collection>, ApiError>>>,
    /// Collection names that reached the create endpoint.
    collections_created: Mutex<Vec<String>>,
    /// What the create endpoint answers, when it must not succeed.
    create_collection_result: Mutex<Option<Result<(), CreateCollectionError>>>,
    account_library_reads: Mutex<usize>,
    collection_reads: Mutex<usize>,
    created: Mutex<Vec<u64>>,
    /// The entries a create made, so the add run's read-back returns an id.
    created_entries: Mutex<BTreeMap<u64, LibraryEntry>>,
    updated: Mutex<Vec<u64>>,
    /// When set, every update answers with this instead of success.
    update_result: Mutex<Option<Result<(), ApiError>>>,
    /// Every entry id that reached the delete endpoint.
    deleted: Mutex<Vec<String>>,
    /// When set, every delete answers with this instead of success.
    delete_result: Mutex<Option<Result<(), ApiError>>>,
    /// The server's favorite games, returned by every read.
    preferred: Mutex<Vec<u64>>,
    /// When set, the next favorite read fails instead of returning `preferred`.
    favorite_read: Mutex<Option<Result<Vec<u64>, ApiError>>>,
    /// When set, every favorite change answers with this instead of success.
    favorite_push: Mutex<Option<Result<(), FavoriteError>>>,
    /// Every favorite change that reached the API, in order.
    favorite_pushes: Mutex<Vec<(u64, bool)>>,
    favorite_reads: Mutex<usize>,
}

impl FakeApi {
    fn new() -> Arc<Self> {
        let api = Arc::new(Self::default());
        *api.session.lock().unwrap() = Some(Ok(ana()));
        *api.sign_out_ok.lock().unwrap() = true;
        api
    }
    async fn pause(&self) {
        let delay = *self.delay.lock().unwrap();
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
    fn success_url(&self) -> String {
        self.started
            .lock()
            .unwrap()
            .last()
            .expect("a sign-in was started")
            .1
            .clone()
    }
}

#[async_trait]
impl AuthApi for FakeApi {
    async fn begin_sign_in(
        &self,
        provider: Provider,
        success: &str,
        failure: &str,
    ) -> Result<Url, SignInFailure> {
        self.pause().await;
        self.started
            .lock()
            .unwrap()
            .push((provider, success.into(), failure.into()));
        match *self.begin.lock().unwrap() {
            Some(reason) => Err(reason),
            None => Ok(Url::parse(
                "https://server.test/v1/auth/capacitor-authorization-proxy?authorizationURL=x",
            )
            .unwrap()),
        }
    }
    async fn get_session(&self, credential: &SessionCredential) -> Result<User, SessionError> {
        self.pause().await;
        self.verified
            .lock()
            .unwrap()
            .push(credential.expose().into());
        self.session.lock().unwrap().clone().unwrap()
    }
    async fn sign_out(&self, credential: &SessionCredential) -> Result<(), ()> {
        self.pause().await;
        self.signed_out
            .lock()
            .unwrap()
            .push(credential.expose().into());
        if *self.sign_out_ok.lock().unwrap() {
            Ok(())
        } else {
            Err(())
        }
    }
}

#[async_trait]
impl GameApi for FakeApi {
    async fn lookup_external(
        &self,
        _credential: &SessionCredential,
        category: u32,
        uids: &[String],
    ) -> Result<Vec<ExternalMatch>, ApiError> {
        self.pause().await;
        self.lookups.lock().unwrap().push((category, uids.to_vec()));
        self.lookup_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(vec![]))
    }
    async fn search_games(
        &self,
        _credential: &SessionCredential,
        _query: &str,
    ) -> Result<Vec<GameSummary>, ApiError> {
        self.pause().await;
        if let Some(result) = self.search_result.lock().unwrap().clone() {
            return result.map(|p| p.games);
        }
        Ok(vec![])
    }
    async fn search_page(
        &self,
        credential: &SessionCredential,
        query: &str,
    ) -> Result<gamenode_core::game_api::SearchPage, ApiError> {
        self.pause().await;
        self.searches.lock().unwrap().push(query.to_owned());
        if let Some(result) = self.search_result.lock().unwrap().clone() {
            return result;
        }
        Ok(gamenode_core::game_api::SearchPage {
            games: self.search_games(credential, query).await?,
            complete: None,
        })
    }
    async fn fetch_cover(&self, cover: &CoverUrl) -> Result<Vec<u8>, CoverError> {
        self.pause().await;
        self.covers_fetched
            .lock()
            .unwrap()
            .push(cover.as_url().to_string());
        Ok(b"jpeg".to_vec())
    }
    async fn fetch_artwork(
        &self,
        _credential: &SessionCredential,
        game_id: u64,
    ) -> Result<Option<Vec<u8>>, ApiError> {
        self.artworks_fetched.lock().unwrap().push(game_id);
        self.artwork_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(None))
    }
    async fn list_collections(
        &self,
        _credential: &SessionCredential,
        _user_id: &str,
    ) -> Result<Vec<Collection>, ApiError> {
        self.pause().await;
        *self.collection_reads.lock().unwrap() += 1;
        self.collections_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(vec![]))
    }
    async fn create_collection(
        &self,
        _credential: &SessionCredential,
        name: &str,
    ) -> Result<(), CreateCollectionError> {
        self.pause().await;
        let mut created = self.collections_created.lock().unwrap();
        created.push(name.to_owned());
        let run = created.len();
        if let Some(result) = *self.create_collection_result.lock().unwrap() {
            return result;
        }
        // The server returns no body; a later list shows the new collection, as the real one does.
        if let Some(Ok(collections)) = self.collections_result.lock().unwrap().as_mut() {
            collections.push(Collection {
                id: format!("created-{run}"),
                name: name.to_owned(),
                default_status: None,
            });
        }
        Ok(())
    }
    async fn list_library_entries(
        &self,
        _credential: &SessionCredential,
        _user_id: &str,
    ) -> Result<Vec<AccountEntry>, ApiError> {
        self.pause().await;
        *self.account_library_reads.lock().unwrap() += 1;
        self.account_entries
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(vec![]))
    }
    async fn library_entry(
        &self,
        _credential: &SessionCredential,
        game_id: u64,
    ) -> Result<Option<LibraryEntry>, ApiError> {
        self.pause().await;
        if let Some(result) = self.entry.lock().unwrap().clone() {
            return result;
        }
        if let Some(entry) = self.created_entries.lock().unwrap().get(&game_id).cloned() {
            return Ok(Some(entry));
        }
        let account = self.account_entries.lock().unwrap().clone();
        if let Some(Ok(entries)) = account
            && let Some(entry) = entries.into_iter().find(|entry| entry.game_id == game_id)
        {
            return Ok(Some(LibraryEntry {
                id: entry.id,
                status: entry.status,
                collection_ids: entry.collection_ids,
                platform_ids: vec![6],
                finished_at: Some("2026-01-01T00:00:00Z".to_owned()),
            }));
        }
        Ok(None)
    }
    async fn update_entry(
        &self,
        _credential: &SessionCredential,
        game_id: u64,
        status: EntryStatus,
        collection_ids: &[String],
        _existing: &LibraryEntry,
    ) -> Result<(), ApiError> {
        self.pause().await;
        if let Some(result) = *self.update_result.lock().unwrap() {
            return result;
        }
        self.updated.lock().unwrap().push(game_id);
        if let Some(entry) = self.created_entries.lock().unwrap().get_mut(&game_id) {
            entry.status = Some(status);
            entry.collection_ids = collection_ids.to_vec();
        }
        let mut account = self.account_entries.lock().unwrap();
        if let Some(Ok(entries)) = account.as_mut()
            && let Some(entry) = entries.iter_mut().find(|entry| entry.game_id == game_id)
        {
            entry.status = Some(status);
            entry.collection_ids = collection_ids.to_vec();
        }
        Ok(())
    }
    async fn create_entry(
        &self,
        _credential: &SessionCredential,
        game_id: u64,
        status: EntryStatus,
        collection_ids: &[String],
    ) -> Result<(), ApiError> {
        self.pause().await;
        self.created.lock().unwrap().push(game_id);
        self.created_entries.lock().unwrap().insert(
            game_id,
            LibraryEntry {
                id: format!("entry-{game_id}"),
                status: Some(status),
                collection_ids: collection_ids.to_vec(),
                platform_ids: vec![6],
                finished_at: None,
            },
        );
        Ok(())
    }
    async fn delete_entry(
        &self,
        _credential: &SessionCredential,
        entry_id: &str,
    ) -> Result<(), ApiError> {
        self.pause().await;
        if let Some(result) = *self.delete_result.lock().unwrap() {
            return result;
        }
        self.deleted.lock().unwrap().push(entry_id.to_owned());
        let mut entries = self.created_entries.lock().unwrap();
        if let Some(game_id) = entries
            .iter()
            .find(|(_, entry)| entry.id == entry_id)
            .map(|(game_id, _)| *game_id)
        {
            entries.remove(&game_id);
        }
        Ok(())
    }
    async fn list_favorites(
        &self,
        _credential: &SessionCredential,
        _user_id: &str,
    ) -> Result<Vec<u64>, ApiError> {
        self.pause().await;
        *self.favorite_reads.lock().unwrap() += 1;
        if let Some(result) = self.favorite_read.lock().unwrap().clone() {
            return result;
        }
        Ok(self.preferred.lock().unwrap().clone())
    }
    async fn set_favorite(
        &self,
        _credential: &SessionCredential,
        game_id: u64,
        favorite: bool,
    ) -> Result<(), FavoriteError> {
        self.pause().await;
        self.favorite_pushes
            .lock()
            .unwrap()
            .push((game_id, favorite));
        if let Some(result) = *self.favorite_push.lock().unwrap() {
            return result;
        }
        Ok(())
    }
}

#[derive(Default)]
struct FakeCreds {
    value: Mutex<Option<SessionCredential>>,
    fail_load: Mutex<bool>,
    fail_save: Mutex<bool>,
    fail_delete: Mutex<bool>,
    delay: Mutex<Duration>,
    log: Mutex<Vec<&'static str>>,
}

impl FakeCreds {
    fn with(value: Option<&str>) -> Arc<Self> {
        let c = Arc::new(Self::default());
        *c.value.lock().unwrap() = value.map(cred);
        c
    }
    fn pause(&self) {
        let delay = *self.delay.lock().unwrap();
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
    }
    fn stored(&self) -> Option<String> {
        self.value
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| c.expose().to_owned())
    }
}

impl CredentialStore for FakeCreds {
    fn method(&self) -> StorageMethod {
        StorageMethod::SecretService
    }
    fn load(&self) -> Result<Option<SessionCredential>, CredentialError> {
        self.pause();
        self.log.lock().unwrap().push("load");
        if *self.fail_load.lock().unwrap() {
            return Err(CredentialError::Locked);
        }
        Ok(self.value.lock().unwrap().clone())
    }
    fn save(&self, credential: &SessionCredential) -> Result<(), CredentialError> {
        self.pause();
        self.log.lock().unwrap().push("save");
        if *self.fail_save.lock().unwrap() {
            return Err(CredentialError::Failed);
        }
        *self.value.lock().unwrap() = Some(credential.clone());
        Ok(())
    }
    fn delete(&self) -> Result<(), CredentialError> {
        self.pause();
        self.log.lock().unwrap().push("delete");
        if *self.fail_delete.lock().unwrap() {
            return Err(CredentialError::Failed);
        }
        self.value.lock().unwrap().take();
        Ok(())
    }
}

#[derive(Default)]
struct FakeBrowser {
    opened: Mutex<Vec<Url>>,
    fail: Mutex<bool>,
}

impl BrowserOpener for FakeBrowser {
    fn open(&self, url: &Url) -> Result<(), BrowserError> {
        if *self.fail.lock().unwrap() {
            return Err(BrowserError);
        }
        self.opened.lock().unwrap().push(url.clone());
        Ok(())
    }
}

#[derive(Default)]
struct FakeSettings {
    saved: Mutex<Vec<Settings>>,
    windows: Mutex<Vec<Option<WindowGeometry>>>,
    preferences: Mutex<Vec<gamenode_core::AppPreferences>>,
    /// When set, `save_preferences` fails instead of writing.
    fail_preferences: Mutex<bool>,
    discovery_last_checked_at: Mutex<Option<i64>>,
    discovery_writes: Mutex<Vec<i64>>,
    /// A real settings store for tests that must reload from disk instead of trusting the fake.
    backing: Mutex<Option<Arc<dyn SettingsStore>>>,
    /// When set, the next full-settings or geometry write waits for the test to release it.
    write_gate: Mutex<Option<Arc<SettingsWriteGate>>>,
}

impl FakeSettings {
    fn use_backing(&self, store: Arc<dyn SettingsStore>) {
        *self.backing.lock().unwrap() = Some(store);
    }

    fn defer_next_write(&self, gate: Arc<SettingsWriteGate>) {
        *self.write_gate.lock().unwrap() = Some(gate);
    }

    /// Runs a storage write through the one-shot gate, then its real backing store when set.
    /// The gate is taken before blocking, so a deferred geometry write cannot hold the database
    /// connection while a preference commit runs.
    fn through_write_gate<T>(&self, write: impl FnOnce() -> T) -> T {
        let gate = self.write_gate.lock().unwrap().take();
        if let Some(gate) = &gate {
            gate.block_until_released();
        }
        let result = write();
        if let Some(gate) = gate {
            gate.mark_finished();
        }
        result
    }
}

/// Lets a test force the order between a window-geometry write and a preference commit.
struct SettingsWriteGate {
    state: Mutex<SettingsWriteState>,
    released: Condvar,
}

#[derive(Default)]
struct SettingsWriteState {
    entered: bool,
    released: bool,
    finished: bool,
}

/// Upper bound so a broken code path fails the test instead of hanging the suite.
const WRITE_GATE_TIMEOUT: Duration = Duration::from_secs(10);

impl SettingsWriteGate {
    fn new() -> Self {
        Self {
            state: Mutex::new(SettingsWriteState::default()),
            released: Condvar::new(),
        }
    }

    /// Blocks until the storage thread reaches the gated write.
    fn wait_entered(&self) {
        let mut state = self.state.lock().unwrap();
        while !state.entered {
            let (next, wait) = self
                .released
                .wait_timeout(state, WRITE_GATE_TIMEOUT)
                .unwrap();
            assert!(
                !wait.timed_out(),
                "the window save never reached the storage write"
            );
            state = next;
        }
    }

    /// Runs on the storage thread: announces the write and holds it until released.
    fn block_until_released(&self) {
        let mut state = self.state.lock().unwrap();
        state.entered = true;
        self.released.notify_all();
        while !state.released {
            let (next, wait) = self
                .released
                .wait_timeout(state, WRITE_GATE_TIMEOUT)
                .unwrap();
            if wait.timed_out() {
                break;
            }
            state = next;
        }
    }

    fn release(&self) {
        self.state.lock().unwrap().released = true;
        self.released.notify_all();
    }

    fn mark_finished(&self) {
        self.state.lock().unwrap().finished = true;
        self.released.notify_all();
    }

    /// Blocks until the released write has reached the backing store.
    fn wait_finished(&self) {
        let mut state = self.state.lock().unwrap();
        while !state.finished {
            let (next, wait) = self
                .released
                .wait_timeout(state, WRITE_GATE_TIMEOUT)
                .unwrap();
            assert!(!wait.timed_out(), "the window save did not finish");
            state = next;
        }
    }
}

impl SettingsStore for FakeSettings {
    fn load(&self) -> Result<Settings, StorageError> {
        if let Some(backing) = self.backing.lock().unwrap().clone() {
            return backing.load();
        }
        Ok(Settings::default())
    }
    fn save(&self, settings: &Settings) -> Result<(), StorageError> {
        self.saved.lock().unwrap().push(settings.clone());
        let backing = self.backing.lock().unwrap().clone();
        self.through_write_gate(move || match backing {
            Some(store) => store.save(settings),
            None => Ok(()),
        })
    }
    fn save_window(&self, window: Option<WindowGeometry>) -> Result<(), StorageError> {
        self.windows.lock().unwrap().push(window);
        let backing = self.backing.lock().unwrap().clone();
        self.through_write_gate(move || match backing {
            Some(store) => store.save_window(window),
            None => Ok(()),
        })
    }
    fn save_preferences(
        &self,
        preferences: &gamenode_core::AppPreferences,
    ) -> Result<(), StorageError> {
        if *self.fail_preferences.lock().unwrap() {
            return Err(StorageError::Failed("injected preference failure".into()));
        }
        self.preferences.lock().unwrap().push(preferences.clone());
        let backing = self.backing.lock().unwrap().clone();
        match backing {
            Some(store) => store.save_preferences(preferences),
            None => Ok(()),
        }
    }
    fn load_discovery_last_checked_at(&self) -> Result<Option<i64>, StorageError> {
        Ok(*self.discovery_last_checked_at.lock().unwrap())
    }
    fn save_discovery_last_checked_at(&self, at: i64) -> Result<(), StorageError> {
        *self.discovery_last_checked_at.lock().unwrap() = Some(at);
        self.discovery_writes.lock().unwrap().push(at);
        Ok(())
    }
}

#[cfg(test)]
impl Engine {
    /// Synchronous optimistic helper for the many engine fixtures that only need the in-memory
    /// value and its side effects. Production uses [`Engine::commit_app_preference`], which waits
    /// for persistence and rolls back on failure; the preference command tests use that path.
    fn set_app_preference(&self, patch: AppPreferencePatch) -> Result<(), AppPreferenceError> {
        patch.validate()?;
        {
            let mut state = self.lock();
            patch.apply(&mut state.settings.preferences);
            let shared_excluded = state.settings.preferences.shared_excluded.clone();
            state.library.set_shared_excluded(shared_excluded);
            self.publish(&mut state);
        }
        self.apply_preference_side_effects(&patch);
        Ok(())
    }

    fn set_autostart_hook(&self, hook: AutostartHook) {
        *self.shared.autostart_hook.lock().unwrap() = Some(hook);
    }
}

struct Harness {
    processes: Arc<gamenode_core::tracker::scripted::ScriptedProcesses>,
    engine: Engine,
    snapshots: watch::Receiver<Snapshot>,
    /// How many snapshots the engine published, including ones equal to the previous value.
    publishes: Arc<AtomicUsize>,
    api: Arc<FakeApi>,
    creds: Arc<FakeCreds>,
    browser: Arc<FakeBrowser>,
    settings: Arc<FakeSettings>,
    store: Arc<FakeLibrary>,
    files: Arc<SlowFiles>,
    uri: Arc<FakeUri>,
    covers: tempfile::TempDir,
    achievements: Arc<dyn gamenode_core::achievements::AchievementStore>,
}

fn harness(api: Arc<FakeApi>, creds: Arc<FakeCreds>) -> Harness {
    harness_over(api, creds, None)
}

#[tokio::test(flavor = "multi_thread")]
async fn idle_auth_ticks_do_not_publish_rebuilt_snapshots() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let _ = h.snapshots.borrow_and_update().clone();

    h.engine.dispatch(Input::Tick);

    assert!(!h.snapshots.has_changed().unwrap());
}

/// Like [`harness`], but the GameNode auth and game requests go through `remote` when given.
fn harness_over(
    api: Arc<FakeApi>,
    creds: Arc<FakeCreds>,
    remote: Option<Arc<gamenode_api::GameNodeClient>>,
) -> Harness {
    let providers = crate::providers::load_bundled_for_tests(Platform::Linux).providers;
    harness_with_providers(api, creds, remote, providers)
}

/// Like [`harness_over`], with the provider manifests a test needs.
fn harness_with_providers(
    api: Arc<FakeApi>,
    creds: Arc<FakeCreds>,
    remote: Option<Arc<gamenode_api::GameNodeClient>>,
    providers: Vec<gamenode_core::provider::Provider>,
) -> Harness {
    let browser = Arc::new(FakeBrowser::default());
    let settings = Arc::new(FakeSettings::default());
    let store = Arc::new(FakeLibrary::default());
    let files = Arc::new(SlowFiles::steam_with_portal_2());
    let uri = Arc::new(FakeUri::default());
    let covers = tempfile::tempdir().unwrap();
    let playtime_db = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let processes = Arc::new(gamenode_core::tracker::scripted::ScriptedProcesses::default());
    let achievements = test_achievement_store();
    let ports = Ports {
        achievements: achievements.clone(),
        backups: None,
        local_games: None,
        installations: None,
        tools: None,
        tool_catalog: None,
        tool_installer: None,
        tool_picker: None,
        processes: processes.clone(),
        playtime: Arc::new(playtime_db.playtime_store()),
        playtime_api: api.clone(),
        settings: settings.clone(),
        credentials: creds.clone(),
        api: match &remote {
            Some(remote) => remote.clone(),
            None => api.clone(),
        },
        browser: browser.clone(),
        library: store.clone(),
        games: match &remote {
            Some(remote) => remote.clone(),
            None => api.clone(),
        },
        files: files.clone(),
        roots: Arc::new(FixedRoots::with_home("/home/me")),
        uri: uri.clone(),
        pc_ports: None,
        port_store: None,
        port_progress: Arc::new(PortProgressPublisher::disabled(
            tokio::runtime::Handle::current(),
        )),
        covers_dir: covers.path().to_owned(),
        platform: Platform::Linux,
    };
    let mut n = 0;
    let nonce: gamenode_core::auth::NonceSource = Box::new(move || {
        n += 1;
        format!("nonce{n}")
    });
    let (tx, rx) = watch::channel(Snapshot {
        local_games: Default::default(),
        playtime_error: false,
        tracking: Default::default(),
        sync: Default::default(),
        playtime_source: Default::default(),
        auth: AuthView::Starting,
        notice: None,
        storage: StorageMethod::SecretService,
        settings: Settings::default(),
        preferences: Default::default(),
        library: None,
        launch: None,
        add_notice: None,
        tools: Default::default(),
        store_accounts: Vec::new(),
        ports: Default::default(),
    });
    let publishes = Arc::new(AtomicUsize::new(0));
    let publish_count = publishes.clone();
    let engine = Engine::new(
        ports,
        tokio::runtime::Handle::current(),
        Settings::default(),
        LibraryMachine::new(providers, Default::default()),
        nonce,
        move |s| {
            publish_count.fetch_add(1, Ordering::SeqCst);
            let _ = tx.send(s);
        },
    );
    Harness {
        processes,
        engine,
        snapshots: rx,
        publishes,
        api,
        creds,
        browser,
        settings,
        store,
        files,
        uri,
        covers,
        achievements,
    }
}

impl Harness {
    async fn until(
        &mut self,
        what: &str,
        mut predicate: impl FnMut(&Snapshot) -> bool,
    ) -> Snapshot {
        let result = tokio::time::timeout(Duration::from_secs(45), async {
            loop {
                {
                    let snapshot = self.snapshots.borrow_and_update().clone();
                    if predicate(&snapshot) {
                        return snapshot;
                    }
                }
                self.snapshots.changed().await.unwrap();
            }
        })
        .await;
        result.unwrap_or_else(|_| {
            panic!(
                "timed out waiting for: {what}; last = {:?}",
                self.snapshots.borrow()
            )
        })
    }

    async fn signed_out(&mut self) {
        self.engine.start();
        self.until("signed out", |s| s.auth == AuthView::SignedOut)
            .await;
    }

    /// The server's redirect: the app's success URL plus the `cookie` parameter.
    fn callback(&self, token: &str) -> String {
        let cookie = format!("better-auth.session_token={token}; Max-Age=604800; Path=/; HttpOnly");
        let encoded: String = url::form_urlencoded::byte_serialize(cookie.as_bytes()).collect();
        format!("{}&cookie={encoded}", self.api.success_url())
    }

    async fn sign_in(&mut self, token: &str) {
        self.engine.dispatch(Input::StartSignIn(Provider::Google));
        self.until("browser opened", |s| {
            matches!(
                s.auth,
                AuthView::Pending {
                    awaiting_browser: true,
                    ..
                }
            )
        })
        .await;
        let callback = self.callback(token);
        self.engine.dispatch(Input::CallbackReceived(callback));
        self.until("signed in", |s| matches!(s.auth, AuthView::SignedIn { .. }))
            .await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fixture_browser_flow_signs_in_stores_and_shows_the_user() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;

    h.engine.dispatch(Input::StartSignIn(Provider::Discord));
    h.until("awaiting browser", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                provider: Provider::Discord,
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;
    // The API got the exact callbacks and the browser got the proxy URL.
    let started = h.api.started.lock().unwrap().clone();
    assert_eq!(started[0].0, Provider::Discord);
    assert_eq!(started[0].1, "app.gamenode://home?desktop_attempt=nonce1");
    assert_eq!(
        started[0].2,
        "app.gamenode://home/sign-in-failed?desktop_attempt=nonce1"
    );
    for _ in 0..100 {
        if !h.browser.opened.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(h.browser.opened.lock().unwrap().len(), 1);
    assert_eq!(
        h.browser.opened.lock().unwrap()[0].path(),
        "/v1/auth/capacitor-authorization-proxy"
    );

    let callback = h.callback("signed.token+/==");
    h.engine.dispatch(Input::CallbackReceived(callback));
    let snapshot = h
        .until("signed in", |s| matches!(s.auth, AuthView::SignedIn { .. }))
        .await;
    let AuthView::SignedIn { user } = snapshot.auth else {
        unreachable!()
    };
    assert_eq!(user.display_name(), "Ana");
    // The credential was verified with GameNode, then stored, exactly as received.
    assert_eq!(*h.api.verified.lock().unwrap(), ["signed.token+/=="]);
    for _ in 0..50 {
        if h.creds.stored().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.creds.stored().as_deref(), Some("signed.token+/=="));
    assert_eq!(snapshot.notice, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn reopen_failure_keeps_the_pending_attempt_and_reports_it() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("awaiting", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                can_reopen: true,
                ..
            }
        )
    })
    .await;
    // Let the initial launch finish before failing only the reopen.
    for _ in 0..100 {
        if !h.browser.opened.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    *h.browser.fail.lock().unwrap() = true;
    h.engine.dispatch(Input::ReopenBrowser);

    let snapshot = h
        .until("reopen failed", |s| {
            matches!(
                s.auth,
                AuthView::Pending {
                    awaiting_browser: true,
                    can_reopen: true,
                    reopen_failed: true,
                    ..
                }
            )
        })
        .await;
    assert!(matches!(
        snapshot.auth,
        AuthView::Pending {
            reopen_failed: true,
            ..
        }
    ));
    let callback = h.callback("tok");
    h.engine.dispatch(Input::CallbackReceived(callback));
    let snapshot = h
        .until("signed in", |s| matches!(s.auth, AuthView::SignedIn { .. }))
        .await;
    assert!(matches!(snapshot.auth, AuthView::SignedIn { .. }));
    assert_eq!(h.creds.stored().as_deref(), Some("tok"));
}

#[tokio::test(flavor = "multi_thread")]
async fn cover_wall_returns_recent_valid_covers_up_to_twenty_five() {
    let h = harness(FakeApi::new(), FakeCreds::with(None));
    for index in 0..30_u64 {
        let file = File::create(h.covers.path().join(format!("{index}.jpg"))).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(index))
            .unwrap();
    }
    let hidden = File::create(h.covers.path().join(".hidden.jpg")).unwrap();
    hidden
        .set_modified(UNIX_EPOCH + Duration::from_secs(1000))
        .unwrap();
    std::fs::create_dir(h.covers.path().join("artworks-v2")).unwrap();

    let wall = h.engine.sign_in_cover_wall().await;
    assert_eq!(wall.len(), 25);
    assert_eq!(wall.first().map(String::as_str), Some("29.jpg"));
    assert_eq!(wall.last().map(String::as_str), Some("5.jpg"));
    assert!(!wall.iter().any(|name| name == ".hidden.jpg"));
}

#[tokio::test(flavor = "multi_thread")]
async fn cover_wall_is_empty_when_the_cache_is_missing() {
    let h = harness(FakeApi::new(), FakeCreds::with(None));
    std::fs::remove_dir_all(h.covers.path()).unwrap();
    assert!(h.engine.sign_in_cover_wall().await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_replayed_and_late_callbacks_cannot_replace_the_session() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("good").await;

    // Replay of the consumed callback, an intruder's attempt id, and garbage.
    let replay = h.callback("evil");
    for raw in [
        replay,
        "app.gamenode://home?desktop_attempt=other&cookie=better-auth.session_token%3Devil"
            .to_string(),
        "x".to_string(),
    ] {
        h.engine.dispatch(Input::CallbackReceived(raw));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let snapshot = h.engine.snapshot();
    assert!(matches!(snapshot.auth, AuthView::SignedIn { .. }));
    assert_eq!(h.creds.stored().as_deref(), Some("good"));
    assert_eq!(
        *h.api.verified.lock().unwrap(),
        ["good"],
        "no callback reached verification"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_session_from_a_callback_does_not_sign_in_or_store() {
    let api = FakeApi::new();
    *api.session.lock().unwrap() = Some(Err(SessionError::Rejected));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("awaiting", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;
    let callback = h.callback("forged");
    h.engine.dispatch(Input::CallbackReceived(callback));
    let s = h
        .until("rejected", |s| s.notice == Some(Notice::SignInRejected))
        .await;
    assert_eq!(s.auth, AuthView::SignedOut);
    assert_eq!(h.creds.stored(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn browser_launch_and_server_failures_leave_signed_out_and_allow_retry() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;

    *h.browser.fail.lock().unwrap() = true;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("launch failed", |s| {
        s.notice == Some(Notice::BrowserLaunchFailed)
    })
    .await;
    assert_eq!(h.engine.snapshot().auth, AuthView::SignedOut);

    *h.browser.fail.lock().unwrap() = false;
    *h.api.begin.lock().unwrap() = Some(SignInFailure::ServerUnavailable);
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("server failed", |s| {
        s.notice == Some(Notice::SignInUnavailable)
    })
    .await;

    *h.api.begin.lock().unwrap() = None;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("retry works", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_with_a_valid_stored_session_signs_in_without_the_browser() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    h.engine.start();
    let s = h
        .until("restored", |s| matches!(s.auth, AuthView::SignedIn { .. }))
        .await;
    assert_eq!(s.notice, None);
    assert!(h.browser.opened.lock().unwrap().is_empty());
    assert!(h.api.started.lock().unwrap().is_empty());
    assert_eq!(*h.api.verified.lock().unwrap(), ["stored"]);
    assert_eq!(
        h.creds.stored().as_deref(),
        Some("stored"),
        "restoration does not rewrite the credential"
    );
    assert!(!h.creds.log.lock().unwrap().contains(&"save"));
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_with_a_rejected_session_removes_the_credential() {
    let api = FakeApi::new();
    *api.session.lock().unwrap() = Some(Err(SessionError::Rejected));
    let mut h = harness(api, FakeCreds::with(Some("revoked")));
    h.engine.start();
    let s = h
        .until("rejected", |s| s.notice == Some(Notice::SessionRejected))
        .await;
    assert_eq!(s.auth, AuthView::SignedOut);
    for _ in 0..50 {
        if h.creds.stored().is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.creds.stored(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_when_offline_keeps_the_credential_and_retry_recovers() {
    let api = FakeApi::new();
    *api.session.lock().unwrap() = Some(Err(SessionError::Unreachable));
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    h.engine.start();
    let s = h
        .until("offline", |s| s.auth == AuthView::RestoreFailed)
        .await;
    assert_eq!(s.notice, Some(Notice::ConnectionProblem));
    assert_eq!(h.creds.stored().as_deref(), Some("stored"));
    assert!(
        !h.creds.log.lock().unwrap().contains(&"delete"),
        "a timeout never deletes the credential"
    );

    *h.api.session.lock().unwrap() = Some(Ok(ana()));
    h.engine.dispatch(Input::RetryRestore);
    h.until("recovered", |s| matches!(s.auth, AuthView::SignedIn { .. }))
        .await;
    assert_eq!(s_notice(&h), None);
}

fn s_notice(h: &Harness) -> Option<Notice> {
    h.engine.snapshot().notice
}

#[tokio::test(flavor = "multi_thread")]
async fn unreadable_credential_store_at_startup_is_reported() {
    let creds = FakeCreds::with(Some("stored"));
    *creds.fail_load.lock().unwrap() = true;
    let mut h = harness(FakeApi::new(), creds);
    h.engine.start();
    let s = h
        .until("unreadable", |s| {
            s.notice == Some(Notice::CredentialUnreadable)
        })
        .await;
    assert_eq!(s.auth, AuthView::SignedOut);
}

#[tokio::test(flavor = "multi_thread")]
async fn persist_failure_is_reported_while_the_session_stays_usable() {
    let creds = FakeCreds::with(None);
    *creds.fail_save.lock().unwrap() = true;
    let mut h = harness(FakeApi::new(), creds);
    h.signed_out().await;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("awaiting", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;
    let callback = h.callback("tok");
    h.engine.dispatch(Input::CallbackReceived(callback));
    let s = h
        .until("persist failure", |s| {
            s.notice == Some(Notice::PersistFailed)
        })
        .await;
    assert!(matches!(s.auth, AuthView::SignedIn { .. }));
    assert_eq!(h.creds.stored(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_out_online_invalidates_the_server_session_and_is_not_restored() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("tok").await;

    h.engine.dispatch(Input::SignOut);
    let s = h
        .until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    assert_eq!(s.notice, None);
    assert_eq!(*h.api.signed_out.lock().unwrap(), ["tok"]);
    assert_eq!(h.creds.stored(), None);

    // "Restart": a new engine over the same storage stays signed out.
    let mut again = harness(FakeApi::new(), h.creds.clone());
    again.engine.start();
    again
        .until("still signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    assert!(again.api.verified.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_out_offline_removes_local_authentication_and_says_so() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("tok").await;
    *h.api.sign_out_ok.lock().unwrap() = false;

    h.engine.dispatch(Input::SignOut);
    let s = h
        .until("not confirmed", |s| {
            s.notice == Some(Notice::SignOutNotConfirmed)
        })
        .await;
    assert_eq!(s.auth, AuthView::SignedOut);
    assert_eq!(h.creds.stored(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_credential_deletion_on_sign_out_is_reported() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("tok").await;
    *h.creds.fail_delete.lock().unwrap() = true;

    h.engine.dispatch(Input::SignOut);
    let s = h
        .until("delete failed", |s| {
            s.notice == Some(Notice::CredentialDeleteFailed)
        })
        .await;
    assert_eq!(s.auth, AuthView::SignedOut);
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_out_cancels_a_pending_attempt_so_a_late_callback_does_nothing() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("awaiting", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;
    let late = h.callback("late");

    h.engine.dispatch(Input::SignOut);
    h.until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    h.engine.dispatch(Input::CallbackReceived(late));
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(h.engine.snapshot().auth, AuthView::SignedOut);
    assert!(h.api.verified.lock().unwrap().is_empty());
    assert_eq!(h.creds.stored(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_out_during_verification_prevents_a_late_sign_in() {
    let api = FakeApi::new();
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("awaiting", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;

    *h.api.delay.lock().unwrap() = Duration::from_millis(300);
    let callback = h.callback("tok");
    h.engine.dispatch(Input::CallbackReceived(callback));
    h.until("verifying", |s| s.auth == AuthView::Verifying)
        .await;
    h.engine.dispatch(Input::SignOut);

    h.until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    tokio::time::sleep(Duration::from_millis(600)).await; // the slow verification completes meanwhile
    assert_eq!(h.engine.snapshot().auth, AuthView::SignedOut);
    assert_eq!(
        h.creds.stored(),
        None,
        "nothing was stored for the abandoned sign-in"
    );
    assert!(!h.creds.log.lock().unwrap().contains(&"save"));
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_network_and_keyring_never_block_the_caller() {
    let api = FakeApi::new();
    *api.delay.lock().unwrap() = Duration::from_millis(500);
    let creds = FakeCreds::with(Some("stored"));
    *creds.delay.lock().unwrap() = Duration::from_millis(500);
    let mut h = harness(api, creds);

    let started = Instant::now();
    h.engine.start();
    h.engine.update_window_geometry(WindowGeometry {
        x: Some(10),
        y: Some(20),
        width: 800,
        height: 600,
        maximized: false,
    });
    let _ = h.engine.snapshot();
    h.engine.dispatch(Input::CancelSignIn);
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "dispatch must not wait for I/O: {:?}",
        started.elapsed()
    );

    // The settings change is visible immediately even though restoration is still running.
    assert_eq!(
        h.engine.snapshot().settings.window,
        Some(WindowGeometry {
            x: Some(10),
            y: Some(20),
            width: 800,
            height: 600,
            maximized: false,
        })
    );
    assert_eq!(h.engine.snapshot().auth, AuthView::Starting);
    h.until("restored eventually", |s| {
        matches!(s.auth, AuthView::SignedIn { .. })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_changes_are_persisted_in_order() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    let geom1 = WindowGeometry {
        x: Some(1),
        y: Some(1),
        width: 800,
        height: 600,
        maximized: false,
    };
    let geom2 = WindowGeometry {
        x: Some(2),
        y: Some(2),
        width: 1024,
        height: 768,
        maximized: true,
    };
    h.engine.update_window_geometry(geom1);
    h.engine.update_window_geometry(geom2);
    for _ in 0..50 {
        if h.settings.windows.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let saved = h.settings.windows.lock().unwrap().clone();
    assert_eq!(saved, [Some(geom1), Some(geom2)]);
    assert!(
        h.settings.saved.lock().unwrap().is_empty(),
        "a window change must not rewrite the full settings"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deferred_window_save_cannot_overwrite_a_committed_preference() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.sqlite3");
    let real = Arc::new(gamenode_platform::db::SqliteSettingsStore::open(&path).unwrap());
    let initial = WindowGeometry {
        x: Some(5),
        y: Some(6),
        width: 900,
        height: 640,
        maximized: false,
    };
    real.save(&Settings {
        window: Some(initial),
        ..Settings::default()
    })
    .unwrap();

    let h = harness(FakeApi::new(), FakeCreds::with(None));
    h.settings.use_backing(real);

    // Hold the geometry write before it reaches SQLite, as a slow storage thread would.
    let gate = Arc::new(SettingsWriteGate::new());
    h.settings.defer_next_write(gate.clone());
    let moved = WindowGeometry {
        x: Some(11),
        y: Some(12),
        width: 1280,
        height: 800,
        maximized: true,
    };
    h.engine.update_window_geometry(moved);
    gate.wait_entered();

    // The preference commit runs while the geometry write is still pending. Both writes must
    // survive, whichever reaches the database last.
    h.engine
        .commit_app_preference(AppPreferencePatch::Locale(Locale::PtBr))
        .await
        .unwrap();
    gate.release();
    gate.wait_finished();

    assert_eq!(
        h.settings.windows.lock().unwrap().as_slice(),
        [Some(moved)],
        "the geometry change went through the geometry-only path"
    );
    assert!(
        h.settings.saved.lock().unwrap().is_empty(),
        "a geometry change must never rewrite the whole settings snapshot"
    );
    assert_eq!(h.engine.preferences().locale, Locale::PtBr);

    drop(h);
    let reopened = gamenode_platform::db::SqliteSettingsStore::open(&path).unwrap();
    let loaded = reopened.load().unwrap();
    assert_eq!(loaded.window, Some(moved), "the latest geometry survives");
    assert_eq!(
        loaded.preferences.locale,
        Locale::PtBr,
        "the committed preference survives the delayed geometry write"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deferred_window_save_survives_a_preference_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.sqlite3");
    let real = Arc::new(gamenode_platform::db::SqliteSettingsStore::open(&path).unwrap());
    let initial = WindowGeometry {
        x: Some(1),
        y: Some(2),
        width: 800,
        height: 600,
        maximized: false,
    };
    real.save(&Settings {
        window: Some(initial),
        ..Settings::default()
    })
    .unwrap();

    let h = harness(FakeApi::new(), FakeCreds::with(None));
    h.settings.use_backing(real);
    h.engine.set_autostart_hook(Arc::new(|_| {
        Err("the login item could not be written".to_owned())
    }));

    let gate = Arc::new(SettingsWriteGate::new());
    h.settings.defer_next_write(gate.clone());
    let moved = WindowGeometry {
        x: Some(21),
        y: Some(22),
        width: 1024,
        height: 768,
        maximized: false,
    };
    h.engine.update_window_geometry(moved);
    gate.wait_entered();

    // A successful preference change lands while the geometry write is still pending, then a
    // lifecycle preference is stored and rolled back on the same path.
    h.engine
        .commit_app_preference(AppPreferencePatch::Locale(Locale::PtBr))
        .await
        .unwrap();
    let result = h
        .engine
        .commit_app_preference(AppPreferencePatch::StartWithWindows(false))
        .await;
    assert!(
        matches!(&result, Err(AppPreferenceError::SideEffect(message)) if message.contains("login item")),
        "the side effect fails after the value was stored: {result:?}"
    );
    gate.release();
    gate.wait_finished();

    let expected = AppPreferences {
        locale: Locale::PtBr,
        ..AppPreferences::default()
    };
    assert_eq!(
        h.engine.preferences(),
        expected,
        "the successful change stays and the rollback restored the lifecycle value"
    );
    assert!(
        h.settings.saved.lock().unwrap().is_empty(),
        "neither the geometry nor the rollbacks rewrote the full settings"
    );

    drop(h);
    let reopened = gamenode_platform::db::SqliteSettingsStore::open(&path).unwrap();
    let loaded = reopened.load().unwrap();
    assert_eq!(
        loaded.window,
        Some(moved),
        "the pending geometry write survives the preference commits and rollback"
    );
    assert_eq!(
        loaded.preferences, expected,
        "relaunching sees the committed preference, not the older snapshot"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_preference_patch_updates_the_snapshot_and_persists() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;

    h.engine
        .commit_app_preference(AppPreferencePatch::Locale(Locale::PtBr))
        .await
        .unwrap();
    h.engine
        .commit_app_preference(AppPreferencePatch::ShowStatusBar(false))
        .await
        .unwrap();

    let snapshot = h.engine.snapshot();
    assert_eq!(snapshot.preferences.locale, Locale::PtBr);
    assert!(!snapshot.preferences.show_status_bar);
    assert!(snapshot.preferences.start_with_windows);
    assert_eq!(
        h.engine.lock().achievement_language,
        "pt-BR",
        "the language choice also drives provider achievement text"
    );
    assert_eq!(
        h.engine.preferences().locale,
        Locale::PtBr,
        "the cheap reader sees the same value"
    );

    let saved = h.settings.preferences.lock().unwrap().clone();
    assert_eq!(saved.len(), 2, "each patch is persisted once: {saved:?}");
    assert_eq!(saved.last().unwrap().locale, Locale::PtBr);
    assert!(!saved.last().unwrap().show_status_bar);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_app_preference_patch_changes_nothing() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;

    let patch = AppPreferencePatch::SharedExcluded(
        ["steam", "not a store"]
            .map(str::to_owned)
            .into_iter()
            .collect(),
    );
    assert_eq!(
        h.engine.commit_app_preference(patch).await,
        Err(gamenode_core::AppPreferenceError::InvalidProviderId)
    );
    assert!(h.engine.preferences().shared_excluded.is_empty());
    assert_eq!(h.engine.snapshot().preferences, Default::default());
    assert!(
        h.settings.preferences.lock().unwrap().is_empty(),
        "a refused value is never written"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_preference_save_leaves_the_snapshot_and_reports_the_error() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    let before = h.engine.snapshot().preferences;
    *h.settings.fail_preferences.lock().unwrap() = true;

    let result = h
        .engine
        .commit_app_preference(AppPreferencePatch::ShowStatusBar(false))
        .await;
    assert!(
        matches!(&result, Err(gamenode_core::AppPreferenceError::Storage(_))),
        "the command caller receives the storage failure: {result:?}"
    );
    assert_eq!(
        h.engine.preferences(),
        before,
        "the optimistic value is not committed"
    );
    assert_eq!(
        h.engine.snapshot().preferences,
        before,
        "the UI is never told an unsaved value is in effect"
    );
    assert!(
        h.settings.preferences.lock().unwrap().is_empty(),
        "nothing was persisted"
    );

    *h.settings.fail_preferences.lock().unwrap() = false;
    h.engine
        .commit_app_preference(AppPreferencePatch::ShowStatusBar(false))
        .await
        .unwrap();
    assert!(!h.engine.snapshot().preferences.show_status_bar);
    assert!(
        !h.settings.preferences.lock().unwrap().is_empty(),
        "a later successful save is persisted"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_preference_patches_keep_each_other() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;

    let status = h
        .engine
        .commit_app_preference(AppPreferencePatch::ShowStatusBar(false));
    let locale = h
        .engine
        .commit_app_preference(AppPreferencePatch::Locale(Locale::PtBr));
    let (status, locale) = tokio::join!(status, locale);
    status.unwrap();
    locale.unwrap();

    let snapshot = h.engine.snapshot().preferences;
    assert!(!snapshot.show_status_bar, "the first field is retained");
    assert_eq!(
        snapshot.locale,
        Locale::PtBr,
        "the second field is retained"
    );
    let saved = h.settings.preferences.lock().unwrap().clone();
    assert_eq!(saved.len(), 2, "both patches were persisted once");
    assert!(!saved.last().unwrap().show_status_bar);
    assert_eq!(saved.last().unwrap().locale, Locale::PtBr);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_lifecycle_side_effect_rolls_the_saved_preference_back() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    let before = h.engine.snapshot().preferences;
    h.engine.set_autostart_hook(Arc::new(|_| {
        Err("the login item could not be written".to_owned())
    }));

    let result = h
        .engine
        .commit_app_preference(AppPreferencePatch::StartWithWindows(false))
        .await;
    assert!(
        matches!(&result, Err(gamenode_core::AppPreferenceError::SideEffect(message)) if message.contains("login item")),
        "the command caller receives the side-effect failure: {result:?}"
    );
    assert_eq!(
        h.engine.snapshot().preferences,
        before,
        "the control returns to the effective value"
    );
    let saved = h.settings.preferences.lock().unwrap().clone();
    assert_eq!(
        saved.len(),
        2,
        "the new value was written, then rolled back"
    );
    assert!(
        saved.last().unwrap().start_with_windows,
        "the stored value agrees with the snapshot"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cold_start_callback_with_no_pending_attempt_is_rejected() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.engine.dispatch(Input::CallbackReceived(
        "app.gamenode://home?desktop_attempt=nonce1&cookie=better-auth.session_token%3Dtok".into(),
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.engine.snapshot().auth, AuthView::SignedOut);
    assert!(h.api.verified.lock().unwrap().is_empty());
    assert_eq!(h.creds.stored(), None);
}

#[async_trait]
impl gamenode_core::ports::PlaytimeApi for FakeApi {
    async fn submit(
        &self,
        _credential: &SessionCredential,
        total: &gamenode_core::playtime::Submission,
    ) -> Result<(), gamenode_core::playtime::SubmitError> {
        self.playtimes.lock().unwrap().push(total.clone());
        let delay = *self.playtime_delay.lock().unwrap();
        tokio::time::sleep(delay).await;
        if *self.playtime_reject_seconds.lock().unwrap() == Some(total.total_seconds) {
            return Err(gamenode_core::playtime::SubmitError::Rejected);
        }
        Ok(())
    }
}

#[tokio::test]
async fn artwork_survives_reload_and_is_separate_from_covers() {
    let api = FakeApi::new();
    let dir = tempfile::tempdir().unwrap();
    let artwork_dir = gamenode_platform::covers::artwork_dir(dir.path());
    let bytes = b"\xff\xd8\xfflandscape".to_vec();
    *api.artwork_result.lock().unwrap() = Some(Ok(Some(bytes.clone())));
    std::fs::write(dir.path().join("7.jpg"), b"portrait").unwrap();
    assert_eq!(
        super::artwork::load(api.as_ref(), Some(&cred("test")), &artwork_dir, 7).await,
        Ok(Some(bytes.clone()))
    );
    // A fresh API adapter and no session: only the persisted file can provide this image.
    let fresh_api = FakeApi::new();
    assert_eq!(
        super::artwork::load(fresh_api.as_ref(), None, &artwork_dir, 7).await,
        Ok(Some(bytes))
    );
    assert!(fresh_api.artworks_fetched.lock().unwrap().is_empty());
    assert_eq!(
        std::fs::read(dir.path().join("7.jpg")).unwrap(),
        b"portrait"
    );
    assert_eq!(*api.artworks_fetched.lock().unwrap(), [7]);
}

#[tokio::test]
async fn corrupt_artwork_is_replaced_and_failures_can_be_retried() {
    let api = FakeApi::new();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("7.jpg"), b"broken").unwrap();
    *api.artwork_result.lock().unwrap() = Some(Err(ApiError::Failed));
    assert_eq!(
        super::artwork::load(api.as_ref(), Some(&cred("test")), dir.path(), 7).await,
        Err(ApiError::Failed)
    );
    let bytes = b"\xff\xd8\xfflandscape".to_vec();
    *api.artwork_result.lock().unwrap() = Some(Ok(Some(bytes.clone())));
    assert_eq!(
        super::artwork::load(api.as_ref(), Some(&cred("test")), dir.path(), 7).await,
        Ok(Some(bytes.clone()))
    );
    assert_eq!(std::fs::read(dir.path().join("7.jpg")).unwrap(), bytes);
}
