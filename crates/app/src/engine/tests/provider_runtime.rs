//! Mainline Engine fixtures inject a provider port using plugin-owned code.
//! Production hosts use HostedProviders and the authenticated process boundary.
//!
//! `Runtime` is the default fixture: it answers discovery and counters from the Steam plugin's
//! own scanner over the in-memory host files, and reports no account. `Scripted` is a
//! provider-neutral fake `ProviderRuntime` that supplies account and catalog observations, so
//! these tests exercise the Engine's store-neutral orchestration without any store adapter.
use super::*;
use crate::plugin_host::{
    provider::Completion,
    runtime::{Observer, ProviderRuntime},
};
use gamenode_core::library::{
    CatalogError as CoreCatalogError, LastAccount, LibraryCache, ProviderScanState, StoredEntry,
};
use gamenode_core::snapshot::StoreAccountState;
use gamenode_plugin_protocol::{models, proto::*};
use gamenode_plugin_steam::discovery;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use tonic::Status;

static ACHIEVEMENT_ACTIVE: AtomicUsize = AtomicUsize::new(0);
pub(crate) static ACHIEVEMENT_MAX_ACTIVE: AtomicUsize = AtomicUsize::new(0);
pub(crate) static ACHIEVEMENT_FAIL_NEXT: AtomicBool = AtomicBool::new(false);

pub(crate) struct Runtime {
    files: Arc<dyn gamenode_core::ports::HostFiles>,
    roots: Arc<dyn gamenode_core::ports::RootResolver>,
    platform: Platform,
}

struct Files(Arc<dyn gamenode_core::ports::HostFiles>);
impl discovery::Files for Files {
    fn canonicalize(&self, path: &str) -> std::io::Result<String> {
        self.0.canonicalize(path)
    }
    fn read_dir(&self, path: &str) -> std::io::Result<Vec<String>> {
        self.0.read_dir(path)
    }
    fn read_to_string(&self, path: &str) -> std::io::Result<String> {
        self.0.read_to_string(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        self.0.is_dir(path)
    }
}
struct Roots(Arc<dyn gamenode_core::ports::RootResolver>);
impl discovery::RootResolver for Roots {
    fn expand_home(&self, path: &str) -> Option<String> {
        self.0.expand_home(path)
    }
    fn registry_string(&self, key: &str, value: &str) -> Option<String> {
        self.0.registry_string(key, value)
    }
}

impl Runtime {
    pub(crate) fn new(ports: &Ports) -> Self {
        Self {
            files: ports.files.clone(),
            roots: ports.roots.clone(),
            platform: ports.platform,
        }
    }
    fn manifest() -> discovery::Manifest {
        toml::from_str(include_str!("../../../../../providers/steam.toml")).unwrap()
    }
    fn platform(&self) -> discovery::Platform {
        match self.platform {
            Platform::Linux => discovery::Platform::Linux,
            Platform::Windows => discovery::Platform::Windows,
        }
    }
}

#[async_trait::async_trait]
impl ProviderRuntime for Runtime {
    fn accounts(&self) -> Vec<String> {
        vec![]
    }
    fn has_provider(&self, id: &str) -> bool {
        id == Self::manifest().id
    }
    fn status(&self, _: &str) -> Option<models::AccountStatus> {
        None
    }
    fn available(&self, _: &str) -> bool {
        true
    }
    fn set_observer(&self, _: Observer) {}
    async fn shutdown(&self) {}
    async fn execute_achievements(
        &self,
        _: &str,
        request: models::AchievementsRequest,
    ) -> Result<Completion, Status> {
        if ACHIEVEMENT_FAIL_NEXT.swap(false, AtomicOrdering::SeqCst) {
            return Err(Status::unavailable("fixture achievement failure"));
        }
        let active = ACHIEVEMENT_ACTIVE.fetch_add(1, AtomicOrdering::SeqCst) + 1;
        ACHIEVEMENT_MAX_ACTIVE.fetch_max(active, AtomicOrdering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let outcomes = request
            .game_ids
            .iter()
            .map(|game_id| models::AchievementOutcome::Ready {
                game_id: game_id.clone(),
                achievements: vec![models::Achievement {
                    id: "ACH_WIN".into(),
                    name: "Win".into(),
                    description: "Win once".into(),
                    hidden: false,
                    icon_url: "https://cdn.example/a.jpg".into(),
                    icon_gray_url: "https://cdn.example/a_gray.jpg".into(),
                    unlocked: true,
                    unlocked_at: Some(1_700_000_000),
                }],
            })
            .collect();
        ACHIEVEMENT_ACTIVE.fetch_sub(1, AtomicOrdering::SeqCst);
        Ok(Completion::fixture_achievements(outcomes))
    }
    async fn execute(&self, _: &str, operation: ProviderOperation) -> Result<Completion, Status> {
        let mut result = OperationResult {
            disposition: OperationDisposition::Completed as i32,
            ..Default::default()
        };
        let catalog = models::CatalogUpdate::default();
        if matches!(
            operation,
            ProviderOperation::Discover | ProviderOperation::ReadCounter
        ) {
            let files = Files(self.files.clone());
            let roots = Roots(self.roots.clone());
            let platform = self.platform();
            return tokio::task::spawn_blocking(move || {
                let manifest = Self::manifest();
                if operation == ProviderOperation::Discover {
                    let records = manifest
                        .scan(platform, &files, &roots)
                        .map_err(|_| Status::unavailable("fixture scan failed"))?;
                    result.discovery = Some(DiscoveryResult {
                        provider_id: manifest.id,
                        records: records
                            .into_iter()
                            .map(|r| DiscoveryRecord {
                                id: r.id,
                                name: r.name.unwrap_or_default(),
                                install_dir: r.install_dir,
                                fields: r.fields.into_iter().collect(),
                            })
                            .collect(),
                    });
                } else {
                    let roots = manifest.resolve_roots(platform, &roots);
                    result.counters = discovery::counter::read(&discovery::ScanInput {
                        files: &files,
                        roots: &roots,
                    })
                    .into_iter()
                    .map(|(id, v)| CounterObservation {
                        id,
                        seconds: v.seconds,
                        last_played: v.last_played,
                    })
                    .collect();
                }
                Ok(Completion::fixture(result, catalog))
            })
            .await
            .map_err(|_| Status::unavailable("fixture worker failed"))?;
        }
        Ok(Completion::fixture(result, catalog))
    }
}

/// One scripted operation result: the account state it reports, plus the flags that drive the
/// Engine's account-change, sign-out and login handling.
#[derive(Clone)]
struct Step {
    status: models::AccountStatus,
    account_changed: bool,
    signed_out: bool,
    login_outcome: Option<AccountLoginOutcome>,
    disposition: OperationDisposition,
}

impl Step {
    fn new(status: models::AccountStatus) -> Self {
        Self {
            status,
            account_changed: false,
            signed_out: false,
            login_outcome: None,
            disposition: OperationDisposition::Completed,
        }
    }
    fn changed(mut self) -> Self {
        self.account_changed = true;
        self
    }
    fn signed_out(mut self) -> Self {
        self.signed_out = true;
        self
    }
    fn superseded(mut self) -> Self {
        self.disposition = OperationDisposition::Superseded;
        self
    }
}

struct ScriptedState {
    status: models::AccountStatus,
    steps: HashMap<i32, VecDeque<Step>>,
    catalog: models::CatalogUpdate,
    queued_catalogs: VecDeque<(Step, models::CatalogUpdate)>,
    discovery: Vec<DiscoveryRecord>,
}

/// A fake `ProviderRuntime` for Engine orchestration tests. It never touches a store, a
/// credential or a network: tests queue the observations the Engine should receive.
pub(crate) struct Scripted {
    provider: &'static str,
    state: Mutex<ScriptedState>,
    calls: Mutex<Vec<i32>>,
    sync_started: Arc<Semaphore>,
    sync_release: Mutex<Option<Arc<Semaphore>>>,
}

impl Scripted {
    fn new(provider: &'static str, status: models::AccountStatus) -> Self {
        Self {
            provider,
            state: Mutex::new(ScriptedState {
                status,
                steps: Default::default(),
                catalog: models::CatalogUpdate::default(),
                queued_catalogs: Default::default(),
                discovery: vec![DiscoveryRecord {
                    id: "620".into(),
                    name: "Portal 2".into(),
                    install_dir: "Portal 2".into(),
                    ..Default::default()
                }],
            }),
            calls: Mutex::new(Vec::new()),
            sync_started: Arc::new(Semaphore::new(0)),
            sync_release: Mutex::new(None),
        }
    }
    fn connected(provider: &'static str) -> Self {
        Self::new(provider, connected_status("A", "cassi"))
    }
    fn queue(&self, operation: ProviderOperation, step: Step) {
        self.state
            .lock()
            .unwrap()
            .steps
            .entry(operation as i32)
            .or_default()
            .push_back(step);
    }
    fn queue_check(&self, step: Step) {
        self.queue(ProviderOperation::Check, step);
    }
    fn queue_sign_out(&self, step: Step) {
        self.queue(ProviderOperation::SignOut, step);
    }
    /// The catalog every sync reports until it is replaced.
    fn set_catalog(
        &self,
        owned: Option<Result<Vec<models::CatalogRecord>, models::CatalogError>>,
        shared: Option<Result<Vec<models::CatalogRecord>, models::CatalogError>>,
        now: i64,
    ) {
        self.state.lock().unwrap().catalog = models::CatalogUpdate { owned, shared, now };
    }
    /// A one-shot sync answer, with its own disposition, before the default catalog applies.
    fn queue_sync(&self, step: Step, catalog: models::CatalogUpdate) {
        self.state
            .lock()
            .unwrap()
            .queued_catalogs
            .push_back((step, catalog));
    }
    /// Holds the next sync after it starts, so a test can make more requests meanwhile.
    fn hold_next_sync(&self, gate: Arc<Semaphore>) {
        *self.sync_release.lock().unwrap() = Some(gate);
    }
    fn calls(&self, operation: ProviderOperation) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| **call == operation as i32)
            .count()
    }
    async fn sync_started(&self) {
        self.sync_started.acquire().await.unwrap().forget();
    }
}

#[async_trait::async_trait]
impl ProviderRuntime for Scripted {
    fn accounts(&self) -> Vec<String> {
        vec![self.provider.to_owned()]
    }
    fn has_provider(&self, id: &str) -> bool {
        id == self.provider
    }
    fn status(&self, _: &str) -> Option<models::AccountStatus> {
        Some(self.state.lock().unwrap().status.clone())
    }
    fn available(&self, _: &str) -> bool {
        true
    }
    fn connected(&self, _: &str) -> bool {
        self.status(self.provider)
            .is_some_and(|status| status.state == models::AccountState::Connected)
    }
    fn set_observer(&self, _: Observer) {}
    async fn shutdown(&self) {}
    async fn execute_achievements(
        &self,
        _: &str,
        _: models::AchievementsRequest,
    ) -> Result<Completion, Status> {
        Ok(Completion::fixture_achievements(vec![]))
    }
    async fn execute(
        &self,
        provider: &str,
        operation: ProviderOperation,
    ) -> Result<Completion, Status> {
        if provider != self.provider {
            return Err(Status::not_found("unknown fixture provider"));
        }
        self.calls.lock().unwrap().push(operation as i32);
        if operation == ProviderOperation::Discover {
            let state = self.state.lock().unwrap();
            let records = state.discovery.clone();
            drop(state);
            return Ok(Completion::fixture(
                OperationResult {
                    disposition: OperationDisposition::Completed as i32,
                    discovery: Some(DiscoveryResult {
                        provider_id: self.provider.into(),
                        records,
                    }),
                    ..Default::default()
                },
                models::CatalogUpdate::default(),
            ));
        }
        if operation == ProviderOperation::SyncCatalog {
            self.sync_started.add_permits(1);
            let gate = self.sync_release.lock().unwrap().take();
            if let Some(gate) = gate {
                let _ = gate.acquire().await;
            }
        }
        let mut state = self.state.lock().unwrap();
        let (step, catalog) = if operation == ProviderOperation::SyncCatalog {
            match state.queued_catalogs.pop_front() {
                Some((step, catalog)) => (step, catalog),
                None => (Step::new(state.status.clone()), state.catalog.clone()),
            }
        } else {
            (
                state
                    .steps
                    .get_mut(&(operation as i32))
                    .and_then(VecDeque::pop_front)
                    .unwrap_or_else(|| Step::new(state.status.clone())),
                models::CatalogUpdate::default(),
            )
        };
        state.status = step.status.clone();
        if operation == ProviderOperation::SyncCatalog
            && step.disposition == OperationDisposition::Completed
        {
            if let Some(owned) = &catalog.owned {
                state.status.catalog_error = owned.as_ref().err().copied();
            }
            if let Some(shared) = &catalog.shared {
                state.status.shared_error = shared.as_ref().err().copied();
            }
        }
        let status = state.status.clone();
        drop(state);
        let observed_at = if operation == ProviderOperation::SyncCatalog {
            catalog.now
        } else {
            0
        };
        let result = OperationResult {
            disposition: step.disposition as i32,
            account_changed: step.account_changed,
            signed_out: step.signed_out,
            status: Some(status.into()),
            login_outcome: step.login_outcome.map(|outcome| outcome as i32),
            observed_at,
            ..Default::default()
        };
        Ok(Completion::fixture(result, catalog))
    }
}

fn account_status(state: models::AccountState) -> models::AccountStatus {
    models::AccountStatus {
        state,
        account_id: None,
        account_name: None,
        last_checked_at: None,
        last_synced_at: None,
        storage: models::StorageMethod::SecretService,
        persist_failed: false,
        catalog_error: None,
        shared_error: None,
    }
}

fn connected_status(account_id: &str, account_name: &str) -> models::AccountStatus {
    models::AccountStatus {
        account_id: Some(account_id.to_owned()),
        account_name: Some(account_name.to_owned()),
        last_checked_at: Some(1_000),
        ..account_status(models::AccountState::Connected)
    }
}

fn record(id: &str, name: &str) -> models::CatalogRecord {
    models::CatalogRecord::new(id, name)
}

fn owned(
    records: Vec<models::CatalogRecord>,
) -> Option<Result<Vec<models::CatalogRecord>, models::CatalogError>> {
    Some(Ok(records))
}

fn account(id: &str, name: &str) -> LastAccount {
    LastAccount {
        id: id.to_owned(),
        name: name.to_owned(),
    }
}

fn entry(key: &str, name: &str, installed: bool, owned: bool, shared: bool) -> StoredEntry {
    StoredEntry {
        key: key.to_owned(),
        provider: "steam".to_owned(),
        store_id: key.rsplit(':').next().expect("a key").to_owned(),
        name: Some(name.to_owned()),
        install_dir: if installed {
            name.to_owned()
        } else {
            String::new()
        },
        fields: Default::default(),
        installed,
        owned,
        shared,
        first_seen: 1,
        last_seen: 1,
    }
}

fn seed(h: &Harness, entries: Vec<StoredEntry>, last: Option<LastAccount>) {
    *h.store.account.lock().unwrap() = last;
    *h.store.cache.lock().unwrap() = LibraryCache {
        entries,
        scans: vec![ProviderScanState {
            provider: "steam".to_owned(),
            last_success_at: Some(1),
            ..ProviderScanState::default()
        }],
        ..LibraryCache::default()
    };
}

fn scripted_harness(scripted: Arc<Scripted>) -> Harness {
    let h = harness(FakeApi::new(), FakeCreds::with(Some("stored-session")));
    h.engine.set_provider_runtime(scripted);
    h
}

fn signed_out_harness(scripted: Arc<Scripted>) -> Harness {
    let h = harness(FakeApi::new(), FakeCreds::with(None));
    h.engine.set_provider_runtime(scripted);
    h
}

/// The bundled Steam manifest with its shared declaration removed, so the host loads a catalog
/// provider that may not report shared access.
fn steam_without_shared_declaration() -> Vec<gamenode_core::provider::Provider> {
    let manifest = include_str!("../../../../../providers/steam.toml")
        .replace("shared = { label = \"Steam Family\" }\n", "");
    assert!(
        !manifest.contains("shared ="),
        "the test manifest no longer declares shared access"
    );
    let backend = gamenode_core::provider::PluginBackend {
        plugin_id: "org.gamenode.plugin.steam-discovery".into(),
        provider_id: "steam".into(),
        discovery: true,
        catalog: true,
        counter: true,
        actions: true,
        achievements: true,
    };
    let loaded = gamenode_core::provider::load_with_plugins(
        &[("steam.toml", manifest.as_str())],
        Platform::Linux,
        &[backend],
    );
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    loaded.providers
}

fn scripted_harness_with_providers(
    scripted: Arc<Scripted>,
    providers: Vec<gamenode_core::provider::Provider>,
) -> Harness {
    let h = harness_with_providers(
        FakeApi::new(),
        FakeCreds::with(Some("stored-session")),
        None,
        providers,
    );
    h.engine.set_provider_runtime(scripted);
    h
}

async fn wait_for_catalog_sync(scripted: &Scripted) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while scripted.calls(ProviderOperation::SyncCatalog) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the catalog sync ran");
}

async fn settled(h: &mut Harness) {
    loop {
        h.snapshots.borrow_and_update();
        tokio::time::sleep(Duration::from_millis(50)).await;
        if !h.snapshots.has_changed().unwrap() {
            return;
        }
    }
}

fn has_row(snapshot: &Snapshot, key: &str) -> bool {
    snapshot
        .library
        .as_ref()
        .is_some_and(|view| view.rows.iter().any(|row| row.key == key))
}

fn card(snapshot: &Snapshot) -> &gamenode_core::snapshot::StoreAccountView {
    snapshot
        .store_accounts
        .first()
        .expect("the provider card is in the snapshot")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connected_provider_applies_its_owned_shared_and_installed_catalog() {
    let scripted = Arc::new(Scripted::connected("steam"));
    scripted.set_catalog(
        owned(vec![record("620", "Portal 2"), record("400", "Portals")]),
        owned(vec![record("730", "Counter-Strike 2")]),
        1_700_000_000,
    );
    let mut h = scripted_harness(scripted.clone());
    seed(&h, vec![], Some(account("u1", "Ana")));

    h.engine.start();
    let snapshot = h
        .until("connected with the catalog", |s| {
            s.store_accounts.first().is_some_and(|card| {
                card.state == StoreAccountState::Connected
                    && card.owned_count == 2
                    && card.shared_count == 1
                    && card.installed_count == 1
            })
        })
        .await;

    assert!(has_row(&snapshot, "steam:400"));
    assert!(has_row(&snapshot, "steam:730"));
    assert!(has_row(&snapshot, "steam:620"));
    let card = card(&snapshot);
    assert_eq!(card.account_id.as_deref(), Some("A"));
    assert_eq!(card.account_name.as_deref(), Some("cassi"));
    assert_eq!(card.installed_count, 1);
    assert_eq!(card.shared_label.as_deref(), Some("Steam Family"));
    assert_eq!(card.last_checked_at, Some(1_000));
    assert!(card.can_check && card.can_sign_out);
    assert!(!card.can_sign_in);
    let store = h.store.clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        while store.catalogs.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the catalog is persisted");
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_catalog_data_from_an_undeclared_provider_fails_the_sync() {
    let scripted = Arc::new(Scripted::connected("steam"));
    scripted.set_catalog(
        owned(vec![record("570", "Dota 2")]),
        owned(vec![record("999", "Never accepted")]),
        1_700_000_000,
    );
    let mut h =
        scripted_harness_with_providers(scripted.clone(), steam_without_shared_declaration());
    seed(
        &h,
        vec![
            entry("steam:400", "Portals", false, true, false),
            entry("steam:730", "Shared", false, false, true),
        ],
        Some(account("u1", "Ana")),
    );

    h.engine.start();
    wait_for_catalog_sync(&scripted).await;
    settled(&mut h).await;

    let snapshot = h.engine.snapshot();
    assert!(
        has_row(&snapshot, "steam:400"),
        "the previous owned fact is kept"
    );
    assert!(
        has_row(&snapshot, "steam:730"),
        "the previous shared fact is kept"
    );
    assert!(
        !has_row(&snapshot, "steam:570"),
        "the failed sync does not import its owned records"
    );
    assert!(
        !has_row(&snapshot, "steam:999"),
        "the failed sync does not import its shared records"
    );
    assert!(
        h.store.catalogs.lock().unwrap().is_empty(),
        "nothing from the failed sync is persisted"
    );
    let catalog_error = h
        .engine
        .lock()
        .library
        .scan_state("steam")
        .and_then(|state| state.catalog_error);
    assert_eq!(
        catalog_error,
        Some(CoreCatalogError::Unreachable),
        "the failed sync is recorded instead of completing without an error"
    );
    assert_eq!(
        card(&snapshot).shared_label,
        None,
        "a provider without the declaration has no label"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_declared_provider_may_finish_with_an_empty_shared_catalog() {
    let scripted = Arc::new(Scripted::connected("steam"));
    scripted.set_catalog(owned(vec![record("620", "Portal 2")]), owned(vec![]), 2);
    let mut h = scripted_harness(scripted.clone());
    seed(&h, vec![], Some(account("u1", "Ana")));

    h.engine.start();
    let snapshot = h
        .until("an accepted empty shared catalog", |s| {
            s.store_accounts.first().is_some_and(|card| {
                card.state == StoreAccountState::Connected
                    && card.owned_count == 1
                    && card.shared_count == 0
                    && card.shared_error.is_none()
            })
        })
        .await;
    assert!(has_row(&snapshot, "steam:620"));
    assert_eq!(
        card(&snapshot).shared_label.as_deref(),
        Some("Steam Family")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_account_change_at_startup_clears_the_cached_owned_catalog() {
    let scripted = Arc::new(Scripted::connected("steam"));
    scripted.queue_check(Step::new(connected_status("B", "new")).changed());
    scripted.set_catalog(owned(vec![record("730", "Counter-Strike 2")]), None, 2);
    let mut h = scripted_harness(scripted.clone());
    seed(
        &h,
        vec![
            entry("steam:400", "Portals", false, true, false),
            entry("steam:620", "Portal 2", true, false, false),
        ],
        Some(account("u1", "Ana")),
    );

    h.engine.start();
    let snapshot = h
        .until("the new account's catalog", |s| {
            has_row(s, "steam:730") && !has_row(s, "steam:400")
        })
        .await;
    assert!(has_row(&snapshot, "steam:620"), "installed games are kept");
    let store = h.store.clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !store.cleared.lock().unwrap().iter().any(|p| p == "steam") {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the clear is persisted");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_identity_change_before_library_activation_survives_the_old_cache_load() {
    let scripted = Arc::new(Scripted::connected("steam"));
    scripted.queue_check(Step::new(connected_status("B", "new")).changed());
    scripted.set_catalog(owned(vec![record("730", "Counter-Strike 2")]), None, 2);
    let mut h = scripted_harness(scripted);
    seed(
        &h,
        vec![entry("steam:400", "Portals", false, true, false)],
        Some(account("u1", "Ana")),
    );
    // The check commits before the library starts loading its cache.
    h.engine.provider_check("steam").await;
    h.engine.start();

    h.until(
        "new ownership after a pre-activation identity change",
        |s| has_row(s, "steam:730") && !has_row(s, "steam:400"),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_account_whose_catalog_fails_does_not_keep_the_old_owned_games() {
    let scripted = Arc::new(Scripted::connected("steam"));
    scripted.queue_check(Step::new(connected_status("B", "new")).changed());
    scripted.set_catalog(Some(Err(models::CatalogError::Privacy)), None, 2);
    let mut h = scripted_harness(scripted);
    seed(
        &h,
        vec![
            entry("steam:400", "Portals", false, true, false),
            entry("steam:620", "Portal 2", true, false, false),
        ],
        Some(account("u1", "Ana")),
    );

    h.engine.start();
    let snapshot = h
        .until("the failed new catalog", |s| {
            s.store_accounts.first().is_some_and(|card| {
                card.state == StoreAccountState::Connected
                    && card.catalog_error == Some(CoreCatalogError::Privacy)
            }) && !has_row(s, "steam:400")
        })
        .await;
    assert!(has_row(&snapshot, "steam:620"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shared_failure_keeps_the_owned_catalog_and_is_reported_separately() {
    let scripted = Arc::new(Scripted::connected("steam"));
    scripted.set_catalog(
        owned(vec![record("400", "Portals")]),
        Some(Err(models::CatalogError::SharedUnavailable)),
        2,
    );
    let mut h = scripted_harness(scripted);
    seed(&h, vec![], Some(account("u1", "Ana")));

    h.engine.start();
    let snapshot = h
        .until(
            "the owned catalog imports despite the shared failure",
            |s| {
                s.store_accounts.first().is_some_and(|card| {
                    card.state == StoreAccountState::Connected
                        && card.owned_count == 1
                        && card.shared_error == Some(CoreCatalogError::SharedUnavailable)
                })
            },
        )
        .await;
    assert!(has_row(&snapshot, "steam:400"));
    assert_eq!(card(&snapshot).catalog_error, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_out_clears_the_catalog_and_keeps_installed_games() {
    let scripted = Arc::new(Scripted::connected("steam"));
    scripted.set_catalog(
        owned(vec![record("400", "Portals"), record("620", "Portal 2")]),
        None,
        2,
    );
    let mut h = scripted_harness(scripted.clone());
    seed(
        &h,
        vec![entry("steam:620", "Portal 2", true, false, false)],
        Some(account("u1", "Ana")),
    );
    h.engine.start();
    h.until("connected with the catalog", |s| {
        s.store_accounts
            .first()
            .is_some_and(|card| card.state == StoreAccountState::Connected && card.owned_count == 2)
    })
    .await;

    scripted
        .queue_sign_out(Step::new(account_status(models::AccountState::NotConnected)).signed_out());
    h.engine.provider_sign_out("steam").await;
    let snapshot = h
        .until("signed out", |s| {
            s.store_accounts
                .first()
                .is_some_and(|card| card.state == StoreAccountState::NotConnected)
        })
        .await;
    assert!(!has_row(&snapshot, "steam:400"));
    assert!(has_row(&snapshot, "steam:620"));
    let store = h.store.clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !store.cleared.lock().unwrap().iter().any(|p| p == "steam") {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the clear is persisted");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_superseded_catalog_result_after_sign_out_is_ignored() {
    let scripted = Arc::new(Scripted::connected("steam"));
    scripted.set_catalog(owned(vec![record("400", "Portals")]), None, 2);
    let mut h = scripted_harness(scripted.clone());
    seed(&h, vec![], Some(account("u1", "Ana")));
    h.engine.start();
    h.until("connected with the catalog", |s| {
        s.store_accounts
            .first()
            .is_some_and(|card| card.state == StoreAccountState::Connected && card.owned_count == 1)
    })
    .await;

    scripted
        .queue_sign_out(Step::new(account_status(models::AccountState::NotConnected)).signed_out());
    h.engine.provider_sign_out("steam").await;
    h.until("signed out", |s| {
        s.store_accounts
            .first()
            .is_some_and(|card| card.state == StoreAccountState::NotConnected)
    })
    .await;

    scripted.set_catalog(None, None, 0);
    scripted.queue_sync(
        Step::new(account_status(models::AccountState::NotConnected)).superseded(),
        models::CatalogUpdate {
            owned: owned(vec![record("400", "Portals")]),
            shared: None,
            now: 3,
        },
    );
    let before = scripted.calls(ProviderOperation::SyncCatalog);
    h.engine
        .library_input(gamenode_core::library::Input::RequestCatalog);
    tokio::time::timeout(Duration::from_secs(5), async {
        while scripted.calls(ProviderOperation::SyncCatalog) <= before {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the requested sync ran");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !h.engine
            .snapshot()
            .library
            .as_ref()
            .is_some_and(|view| view.rows.iter().any(|row| row.key == "steam:400")),
        "a late result from the superseded attempt must not restore the catalog"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_catalog_requests_coalesce_into_one_more_sync() {
    let scripted = Arc::new(Scripted::connected("steam"));
    scripted.set_catalog(owned(vec![record("400", "Portals")]), None, 2);
    let mut h = scripted_harness(scripted.clone());
    seed(&h, vec![], Some(account("u1", "Ana")));
    h.engine.start();
    h.until("connected with the catalog", |s| {
        s.store_accounts
            .first()
            .is_some_and(|card| card.state == StoreAccountState::Connected && card.owned_count == 1)
    })
    .await;
    settled(&mut h).await;
    let baseline = scripted.calls(ProviderOperation::SyncCatalog);

    let gate = Arc::new(Semaphore::new(0));
    scripted.hold_next_sync(gate.clone());
    h.engine
        .library_input(gamenode_core::library::Input::RequestCatalog);
    scripted.sync_started().await;
    for _ in 0..5 {
        h.engine
            .library_input(gamenode_core::library::Input::RequestCatalog);
    }
    gate.add_permits(1);

    tokio::time::timeout(Duration::from_secs(5), async {
        while scripted.calls(ProviderOperation::SyncCatalog) <= baseline + 1 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the coalesced sync finishes");
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        scripted.calls(ProviderOperation::SyncCatalog),
        baseline + 2,
        "the running sync plus exactly one more for the five requests made meanwhile"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_account_card_works_without_a_gamenode_session_and_never_syncs_a_catalog() {
    let scripted = Arc::new(Scripted::connected("steam"));
    let mut h = signed_out_harness(scripted.clone());
    h.engine.start();
    let snapshot = h
        .until("connected without a library", |s| {
            s.store_accounts
                .first()
                .is_some_and(|card| card.state == StoreAccountState::Connected)
                && s.library.is_none()
        })
        .await;
    assert_eq!(card(&snapshot).account_name.as_deref(), Some("cassi"));
    assert_eq!(scripted.calls(ProviderOperation::SyncCatalog), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn every_account_state_reaches_the_snapshot() {
    for state in [
        models::AccountState::NotConnected,
        models::AccountState::Checking,
        models::AccountState::SigningIn,
        models::AccountState::Connected,
        models::AccountState::Expired,
        models::AccountState::Unreachable,
        models::AccountState::CredentialError,
    ] {
        let scripted = Arc::new(Scripted::new("steam", account_status(state)));
        let mut h = signed_out_harness(scripted);
        h.engine.start();
        let expected = match state {
            models::AccountState::NotConnected => StoreAccountState::NotConnected,
            models::AccountState::Checking => StoreAccountState::Checking,
            models::AccountState::SigningIn => StoreAccountState::SigningIn,
            models::AccountState::Connected => StoreAccountState::Connected,
            models::AccountState::Expired => StoreAccountState::Expired,
            models::AccountState::Unreachable => StoreAccountState::Unreachable,
            models::AccountState::CredentialError => StoreAccountState::CredentialError,
        };
        let snapshot = h
            .until("the scripted account state", |s| {
                s.store_accounts
                    .first()
                    .is_some_and(|card| card.state == expected)
            })
            .await;
        assert_eq!(card(&snapshot).state, expected);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_or_unreachable_account_keeps_the_cached_library_visible() {
    for state in [
        models::AccountState::Expired,
        models::AccountState::Unreachable,
    ] {
        let scripted = Arc::new(Scripted::new("steam", account_status(state)));
        let mut h = scripted_harness(scripted);
        seed(
            &h,
            vec![
                entry("steam:400", "Portals", false, true, false),
                entry("steam:620", "Portal 2", true, false, false),
            ],
            Some(account("u1", "Ana")),
        );
        h.engine.start();
        let snapshot = h
            .until("the account state with the cached library", |s| {
                s.store_accounts.first().is_some_and(|card| {
                    card.state
                        == match state {
                            models::AccountState::Expired => StoreAccountState::Expired,
                            _ => StoreAccountState::Unreachable,
                        }
                        && has_row(s, "steam:400")
                        && has_row(s, "steam:620")
                })
            })
            .await;
        assert!(has_row(&snapshot, "steam:400"));
        assert!(has_row(&snapshot, "steam:620"));
    }
}
