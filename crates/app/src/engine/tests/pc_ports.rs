//! App wiring tests: blocking work runs on workers, the snapshot carries `PortsView`, progress
//! ticks never republish it, and a removed port keeps its files and provenance.

use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::plugin_host::{
    provider::Completion,
    runtime::{Observer, ProviderRuntime},
};
use gamenode_core::drm_free::{LocalGameStore, Registration};
use gamenode_core::ports::StorageError;
use gamenode_core::ports::catalog::{
    CatalogArt, CatalogEntry, CatalogProject, CatalogSnapshot, LauncherRecipe, PortAsset,
    PortRelease, PreferredExecutables, VerifiedRelease,
};
use gamenode_core::ports::job::PortJobPhase;
use gamenode_core::ports::matching::PortMatchInput;
use gamenode_core::ports::plan::PortInstallPlan;
use gamenode_core::ports::store::{
    PortInstallation, PortJobResume, PortJobRow, PortOverlayRow, PortPreferences, PortStore,
    PortViewMode,
};
use gamenode_core::ports::view::{PortCatalogState, PortProgress, PortProgressPhase, PortsView};
use gamenode_platform::db::SqliteSettingsStore;
use gamenode_platform::ports::catalog_worker::{CatalogSource, Clock};
use gamenode_platform::ports::job_runner::{InstallJobRequest, JobError};
use gamenode_platform::ports::overlay_source::{FetchedOverlay, OverlayFetcher};
use gamenode_platform::ports::quiver::QuiverError;
use gamenode_plugin_protocol::{models, proto::ProviderOperation};
use tonic::Status;

use crate::engine::pc_ports::{
    InstallRunner, PortFiltersPatch, PortInstallRequest, PortPreferencesPatch, PortsError,
    PortsEvent, PortsService, PortsServiceConfig,
};

use super::{FakeApi, FakeCreds, harness};

struct FakeCatalog {
    entries: Vec<CatalogEntry>,
    detail: Option<CatalogEntry>,
    releases: Vec<PortRelease>,
    gate: Arc<Gate>,
}

impl FakeCatalog {
    fn new(
        entries: Vec<CatalogEntry>,
        detail: Option<CatalogEntry>,
        releases: Vec<PortRelease>,
    ) -> Self {
        Self {
            entries,
            detail,
            releases,
            gate: Arc::new(Gate::open()),
        }
    }
}

impl CatalogSource for FakeCatalog {
    fn listing(&self) -> Result<Vec<CatalogEntry>, QuiverError> {
        self.gate.wait();
        Ok(self.entries.clone())
    }
    fn detail(&self, _slug: &str) -> Result<CatalogEntry, QuiverError> {
        self.detail.clone().ok_or(QuiverError::InvalidResponse)
    }
    fn releases(&self, _slug: &str) -> Result<Vec<PortRelease>, QuiverError> {
        Ok(self.releases.clone())
    }
}

/// A one-shot gate: `close` blocks the next waiter, `open` releases it.
struct Gate {
    closed: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn open() -> Self {
        Self {
            closed: Mutex::new(false),
            changed: Condvar::new(),
        }
    }
    fn close(&self) {
        *self.closed.lock().unwrap() = true;
    }
    fn release(&self) {
        *self.closed.lock().unwrap() = false;
        self.changed.notify_all();
    }
    fn wait(&self) {
        let mut closed = self.closed.lock().unwrap();
        while *closed {
            closed = self.changed.wait(closed).unwrap();
        }
    }
}

struct FakeOverlay;

impl OverlayFetcher for FakeOverlay {
    fn fetch(&self, _etag: Option<&str>) -> FetchedOverlay {
        FetchedOverlay::Failed {
            reason: "offline in tests".into(),
        }
    }
}

struct FakeClock(AtomicI64);

impl Clock for FakeClock {
    fn now_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A store that can hold one operation until the test releases it.
struct GateStore {
    inner: Arc<dyn PortStore>,
    gate: Arc<Gate>,
}

impl GateStore {
    fn new(inner: Arc<dyn PortStore>, open: bool) -> Self {
        let gate = Arc::new(Gate::open());
        if !open {
            gate.close();
        }
        Self { inner, gate }
    }
    fn release(&self) {
        self.gate.release();
    }
    fn close(&self) {
        self.gate.close();
    }
}

impl PortStore for GateStore {
    fn replace_catalog_snapshot(&self, snapshot: &CatalogSnapshot) -> Result<(), StorageError> {
        self.inner.replace_catalog_snapshot(snapshot)
    }
    fn catalog_snapshot(&self) -> Result<Option<CatalogSnapshot>, StorageError> {
        self.inner.catalog_snapshot()
    }
    fn overlay(&self) -> Result<Option<PortOverlayRow>, StorageError> {
        self.inner.overlay()
    }
    fn save_overlay(&self, overlay: &PortOverlayRow) -> Result<(), StorageError> {
        self.inner.save_overlay(overlay)
    }
    fn jobs(&self) -> Result<Vec<PortJobRow>, StorageError> {
        self.gate.wait();
        self.inner.jobs()
    }
    fn job(&self, id: &str) -> Result<Option<PortJobRow>, StorageError> {
        self.inner.job(id)
    }
    fn save_job(&self, job: &PortJobRow) -> Result<(), StorageError> {
        self.inner.save_job(job)
    }
    fn save_job_resume(&self, job_id: &str, resume: &PortJobResume) -> Result<(), StorageError> {
        self.inner.save_job_resume(job_id, resume)
    }
    fn job_resume(&self, job_id: &str) -> Result<Option<PortJobResume>, StorageError> {
        self.inner.job_resume(job_id)
    }
    fn record_created_path(&self, job_id: &str, path: &str) -> Result<(), StorageError> {
        self.inner.record_created_path(job_id, path)
    }
    fn created_paths(&self, job_id: &str) -> Result<Vec<String>, StorageError> {
        self.inner.created_paths(job_id)
    }
    fn commit_install(
        &self,
        job: &PortJobRow,
        registration: &Registration,
        installation: &PortInstallation,
    ) -> Result<(), StorageError> {
        self.inner.commit_install(job, registration, installation)
    }
    fn installations(&self) -> Result<Vec<PortInstallation>, StorageError> {
        self.inner.installations()
    }
    fn save_installation(&self, installation: &PortInstallation) -> Result<(), StorageError> {
        self.inner.save_installation(installation)
    }
    fn relink_installation(&self, job_id: &str, registration_id: &str) -> Result<(), StorageError> {
        self.inner.relink_installation(job_id, registration_id)
    }
    fn preferences(&self) -> Result<PortPreferences, StorageError> {
        self.inner.preferences()
    }
    fn save_preferences(&self, preferences: &PortPreferences) -> Result<(), StorageError> {
        self.gate.wait();
        self.inner.save_preferences(preferences)
    }
}

/// The PC ports tests start real worker threads and SQLite handles. Running them one at a time
/// keeps the other engine tests' timing-sensitive waits from starving.
fn serial() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn database() -> SqliteSettingsStore {
    SqliteSettingsStore::new(gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap())
}

fn entry() -> CatalogEntry {
    CatalogEntry {
        catalog_id: "qcat_lantern".into(),
        slug: "bloodborne-pc".into(),
        name: "Bloodborne".into(),
        supported_os: vec!["windows".into()],
        consoles: vec!["ps4".into()],
        games: vec![gamenode_core::ports::catalog::CatalogGame {
            title: "Bloodborne".into(),
            ..Default::default()
        }],
        project: Some(CatalogProject {
            name: "Lantern Engine".into(),
            repository: Some("owner/lantern".into()),
            ..Default::default()
        }),
        art: CatalogArt::default(),
        verified: Some(VerifiedRelease {
            version: "v0.9.38".into(),
            released_at: 1_793_220_348_000,
            ..Default::default()
        }),
        launcher: LauncherRecipe {
            folder_name: Some("Bloodborne".into()),
            preferred_executables: PreferredExecutables {
                windows: vec!["Bloodborne.exe".into()],
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

fn release(checksum: Option<String>, unverified: Option<String>) -> PortRelease {
    PortRelease {
        release_id: "rel".into(),
        version: "v0.9.38".into(),
        released_at: 1_793_220_348_000,
        assets: vec![PortAsset {
            id: "win".into(),
            url: "https://example.com/port.zip".into(),
            filename: "bloodborne-windows.zip".into(),
            os: Some("windows".into()),
            architecture: Some("x86_64".into()),
            format: Some("zip".into()),
            size: 1_000,
            checksum,
            unverified_checksum: unverified,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn start(config: PortsServiceConfig) -> (Arc<PortsService>, std::sync::mpsc::Receiver<PortsEvent>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let service = PortsService::start(config);
    service.set_observer(Arc::new(move |event| {
        let _ = tx.send(event);
    }));
    (service, rx)
}

struct FailingSteamRuntime;

#[async_trait::async_trait]
impl ProviderRuntime for FailingSteamRuntime {
    fn accounts(&self) -> Vec<String> {
        vec!["steam".into()]
    }
    fn has_provider(&self, provider: &str) -> bool {
        provider == "steam"
    }
    fn status(&self, _: &str) -> Option<models::AccountStatus> {
        None
    }
    fn available(&self, _: &str) -> bool {
        false
    }
    fn availability(&self, _: &str) -> &'static str {
        "failed"
    }
    fn set_observer(&self, _: Observer) {}
    async fn execute(&self, _: &str, _: ProviderOperation) -> Result<Completion, Status> {
        Err(Status::unavailable("fixture Steam process failure"))
    }
    async fn shutdown(&self) {}
}

fn wait_for_view(
    rx: &std::sync::mpsc::Receiver<PortsEvent>,
    mut predicate: impl FnMut(&PortsView) -> bool,
) -> PortsView {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(left).expect("a ports view");
        if let PortsEvent::Changed { view, .. } = event
            && predicate(&view)
        {
            return view;
        }
    }
}

fn service_config(
    store: Arc<dyn PortStore>,
    local_games: Arc<dyn LocalGameStore>,
    catalog: Arc<dyn CatalogSource>,
) -> PortsServiceConfig {
    PortsServiceConfig {
        store,
        local_games,
        catalog,
        overlay_fetcher: Arc::new(FakeOverlay),
        clock: Arc::new(FakeClock(AtomicI64::new(1_793_220_348_000))),
        data_dir: std::env::temp_dir(),
        platform: gamenode_core::ports::PortPlatform::Windows,
        install: None,
    }
}

/// A runner that reports jobs without touching a disk, for the cache-ownership cases.
struct FakeRunner {
    active: Option<String>,
    queued: Vec<String>,
    owning: Vec<String>,
}

impl FakeRunner {
    fn idle() -> Self {
        Self {
            active: None,
            queued: Vec::new(),
            owning: Vec::new(),
        }
    }

    fn active(job_id: &str) -> Self {
        Self {
            active: Some(job_id.to_owned()),
            queued: Vec::new(),
            owning: vec![job_id.to_owned()],
        }
    }

    fn awaiting(job_id: &str) -> Self {
        Self {
            active: None,
            queued: Vec::new(),
            owning: vec![job_id.to_owned()],
        }
    }
}

impl InstallRunner for FakeRunner {
    fn enqueue(&self, _request: InstallJobRequest) -> Result<(), JobError> {
        Ok(())
    }
    fn cancel(&self, _job_id: &str) -> bool {
        false
    }
    fn complete_with_executable(&self, _job_id: &str, _executable: &str) -> Result<(), JobError> {
        Ok(())
    }
    fn awaiting_jobs(&self) -> Result<Vec<PortJobRow>, JobError> {
        Ok(Vec::new())
    }
    fn active_job(&self) -> Option<String> {
        self.active.clone()
    }
    fn queued_jobs(&self) -> Vec<String> {
        self.queued.clone()
    }
    fn is_active(&self, job_id: &str) -> bool {
        self.owning.iter().any(|owned| owned == job_id)
    }
    fn shutdown(&self) {}
}

fn write_bytes(path: &std::path::Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

/// A stored overlay with one pinned extractor, for the helper-tools group.
fn overlay_with_extractor() -> PortOverlayRow {
    let sha = "a".repeat(64);
    PortOverlayRow {
        version: 1,
        etag: None,
        payload: format!(
            r#"schema_version = 1

[[extractor]]
id = "ps4-pkg"
name = "Fixture PKG extractor"
repository = "example/fixture-pkg"
tag = "v2.4.1"
asset = "fixture-pkg-win64.zip"
sha256 = "{sha}"
license = "GPL-3.0"
executable = "fixture-pkg.exe"
args = ["extract", "{{package}}", "--out", "{{out}}"]
"#
        ),
    }
}

#[tokio::test]
async fn starting_and_opening_pc_ports_never_waits_for_the_network() {
    let _serial = serial().lock().await;
    let db = database();
    let store: Arc<dyn PortStore> = Arc::new(db.ports_store());
    let local: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    let catalog = Arc::new(FakeCatalog::new(
        vec![entry()],
        Some(entry()),
        vec![release(Some(format!("sha256:{}", "a".repeat(64))), None)],
    ));
    catalog.gate.close();

    let started = Instant::now();
    let (service, rx) = start(service_config(store, local, catalog.clone()));
    service.open();
    service.refresh();
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "starting and opening took {:?}",
        started.elapsed()
    );

    catalog.gate.release();
    let view = wait_for_view(&rx, |view| view.catalog.state == PortCatalogState::Ready);
    assert_eq!(view.catalog.entry_count, 1);
    assert_eq!(view.entries[0].catalog_id, "qcat_lantern");
    service.shutdown();
}

#[tokio::test]
async fn failed_steam_runtime_leaves_the_pc_ports_service_and_saved_job_untouched() {
    let _serial = serial().lock().await;
    let db = database();
    let ports_store: Arc<dyn PortStore> = Arc::new(db.ports_store());
    let local: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    let job = PortJobRow {
        id: "ports-job-preserved".into(),
        catalog_id: "qcat_lantern".into(),
        slug: "bloodborne-pc".into(),
        port_name: "Bloodborne".into(),
        release_version: Some("v0.9.38".into()),
        install_folder: Some("D:/Ports/Bloodborne".into()),
        phase: PortJobPhase::Completed,
        created_paths: Vec::new(),
        registration_id: None,
        error: None,
        failure_output: None,
        seen: false,
        created_at: 1,
        updated_at: 1,
        finished_at: Some(1),
    };
    ports_store.save_job(&job).unwrap();
    let (service, events) = start(service_config(
        ports_store.clone(),
        local,
        Arc::new(FakeCatalog::new(vec![entry()], None, Vec::new())),
    ));
    let initial_view = wait_for_view(&events, |view| view.jobs.len() == 1);
    assert_eq!(initial_view.jobs[0].job_id, job.id);

    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.pc_ports = Some(service.clone());
        shared.ports.port_store = Some(ports_store.clone());
    }
    h.engine.set_provider_runtime(Arc::new(FailingSteamRuntime));
    assert_eq!(h.engine.provider_availability("steam"), "failed");
    h.engine.provider_check("steam").await;

    assert!(
        h.engine.pc_ports_supported(),
        "PC ports remains available after Steam failure"
    );
    assert_eq!(ports_store.job(&job.id).unwrap(), Some(job));
    service.shutdown();
}

#[tokio::test]
async fn the_catalog_status_carries_seconds_and_the_entries_are_merged() {
    let _serial = serial().lock().await;
    let db = database();
    let store: Arc<dyn PortStore> = Arc::new(db.ports_store());
    let local: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    let catalog = Arc::new(FakeCatalog::new(
        vec![entry()],
        Some(entry()),
        vec![release(Some(format!("sha256:{}", "a".repeat(64))), None)],
    ));
    let (service, rx) = start(service_config(store, local, catalog));
    service.open();
    let view = wait_for_view(&rx, |view| view.catalog.state == PortCatalogState::Ready);
    assert_eq!(view.catalog.fetched_at, Some(1_793_220_348));
    assert_eq!(view.catalog.entry_count, 1);
    assert_eq!(view.entries[0].consoles, ["PlayStation 4"]);
    service.shutdown();
}

#[tokio::test]
async fn preferences_round_trip_and_reach_the_view() {
    let _serial = serial().lock().await;
    let db = database();
    let store: Arc<dyn PortStore> = Arc::new(db.ports_store());
    let local: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    let catalog = Arc::new(FakeCatalog::new(vec![entry()], None, Vec::new()));
    let (service, rx) = start(service_config(store.clone(), local, catalog));
    service
        .set_preferences(&PortPreferencesPatch {
            view: Some(PortViewMode::List),
            filters: Some(PortFiltersPatch {
                consoles: Some(vec!["PlayStation 4".into()]),
                guided: Some(true),
                verified: Some(false),
                installed: Some(false),
            }),
            last_parent_folder: Some(Some(r"D:\Ports".into())),
        })
        .expect("saved");
    let stored = store.preferences().unwrap();
    assert_eq!(stored.view, PortViewMode::List);
    assert_eq!(stored.consoles, ["PlayStation 4"]);
    assert!(stored.guided);
    assert_eq!(stored.last_parent.as_deref(), Some(r"D:\Ports"));
    let view = wait_for_view(&rx, |view| {
        view.preferences.view == PortViewMode::List && view.preferences.filters.guided
    });
    assert_eq!(
        view.preferences.filters.consoles,
        vec!["PlayStation 4".to_owned()]
    );
    assert_eq!(
        view.preferences.last_parent_folder.as_deref(),
        Some(r"D:\Ports")
    );
    service.shutdown();
}

#[tokio::test]
async fn clearing_the_port_cache_keeps_installed_ports_and_provenance() {
    let _serial = serial().lock().await;
    let db = database();
    let store: Arc<dyn PortStore> = Arc::new(db.ports_store());
    let local: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    store.save_overlay(&overlay_with_extractor()).unwrap();
    let catalog = Arc::new(FakeCatalog::new(vec![entry()], Some(entry()), Vec::new()));
    let data = tempfile::tempdir().unwrap();
    let mut config = service_config(store.clone(), local.clone(), catalog);
    config.data_dir = data.path().to_owned();
    let (service, _rx) = start(config);

    let install = tempfile::tempdir().unwrap();
    let executable = install.path().join("Bloodborne.exe");
    write_bytes(&executable, b"installed port");
    let registration = Registration {
        id: "port-1".into(),
        title: "Bloodborne".into(),
        executable: executable.to_string_lossy().into_owned(),
        file_identity: "1:2:3".into(),
        root: install.path().to_string_lossy().into_owned(),
        working_directory: install.path().to_string_lossy().into_owned(),
        arguments: Vec::new(),
        gameplay: Vec::new(),
        launchers: Vec::new(),
        revision: 1,
        removed: false,
        rediscovery_allowed: false,
        transferred_from: None,
        available: true,
    };
    let installation = PortInstallation {
        registration_id: registration.id.clone(),
        catalog_id: "qcat_lantern".into(),
        slug: "bloodborne-pc".into(),
        port_name: "Bloodborne".into(),
        installed_version: "v0.9.38".into(),
        install_folder: registration.root.clone(),
        environment: Default::default(),
        job_id: "job-1".into(),
    };
    let job = PortJobRow {
        id: "job-1".into(),
        catalog_id: "qcat_lantern".into(),
        slug: "bloodborne-pc".into(),
        port_name: "Bloodborne".into(),
        release_version: Some("v0.9.38".into()),
        install_folder: Some(registration.root.clone()),
        phase: PortJobPhase::Completed,
        created_paths: Vec::new(),
        registration_id: Some(registration.id.clone()),
        error: None,
        failure_output: None,
        seen: false,
        created_at: 1,
        updated_at: 1,
        finished_at: Some(1),
    };
    store
        .commit_install(&job, &registration, &installation)
        .unwrap();
    assert!(
        local
            .registrations()
            .unwrap()
            .iter()
            .any(|known| known.id == "port-1" && !known.removed),
        "the install is a real registration before the clear"
    );

    let asset = data
        .path()
        .join("ports/cache/qcat_lantern/v0.9.38/bloodborne-windows.zip");
    write_bytes(&asset, &[b'a'; 1000]);
    let tool_archive = data
        .path()
        .join("ports/tools/ps4-pkg/v2.4.1/download/fixture-pkg-win64.zip");
    write_bytes(&tool_archive, &[b'b'; 2500]);
    let tool_exe = data
        .path()
        .join("ports/tools/ps4-pkg/v2.4.1/tool/fixture-pkg.exe");
    write_bytes(&tool_exe, &[b'c'; 1500]);
    let art = data.path().join("ports/art/qcat_lantern/capsule.png");
    write_bytes(&art, b"artwork");

    let before = service.tools_cache().expect("cache info");
    assert_eq!(before.downloads_bytes, 1000);
    assert_eq!(before.helper_tools.len(), 1);
    assert_eq!(before.helper_tools[0].license, "GPL-3.0");
    assert_eq!(
        before.helper_tools[0].source_url.as_deref(),
        Some("https://github.com/example/fixture-pkg")
    );
    assert_eq!(before.helper_tools[0].version, "v2.4.1");
    assert_eq!(before.helper_tools[0].cache_bytes, 4000);
    assert!(!before.install_active);

    let after = service.clear_cache().expect("the cache is cleared");
    assert_eq!(after.total_bytes(), 0, "every cache entry is gone");
    assert!(!asset.exists());
    assert!(!tool_archive.exists());
    assert!(!tool_exe.exists());
    assert_eq!(std::fs::read(&art).unwrap(), b"artwork");
    assert_eq!(std::fs::read(&executable).unwrap(), b"installed port");
    assert!(
        local
            .registrations()
            .unwrap()
            .iter()
            .any(|known| known.id == "port-1" && !known.removed),
        "clearing the cache never removes a registration"
    );
    let installations = store.installations().unwrap();
    assert_eq!(installations.len(), 1);
    assert_eq!(installations[0].job_id, "job-1");
    service.shutdown();
}

#[tokio::test]
async fn clearing_the_port_cache_is_refused_while_an_install_is_active() {
    let _serial = serial().lock().await;
    let db = database();
    let store: Arc<dyn PortStore> = Arc::new(db.ports_store());
    let local: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    let catalog = Arc::new(FakeCatalog::new(vec![entry()], None, Vec::new()));
    let data = tempfile::tempdir().unwrap();
    let mut config = service_config(store.clone(), local, catalog);
    config.data_dir = data.path().to_owned();
    let (service, rx) = start(config);
    // Wait for startup to finish: it installs the real runner slot, which the fake replaces.
    wait_for_view(&rx, |_| true);

    let asset = data
        .path()
        .join("ports/cache/qcat_lantern/v0.9.38/bloodborne-windows.zip");
    write_bytes(&asset, b"cached release");

    service.set_runner_for_test(Arc::new(FakeRunner::active("job-running")));
    match service.clear_cache() {
        Err(PortsError::InstallInProgress) => {}
        other => panic!("a running install must refuse the clear, got {other:?}"),
    }
    assert!(asset.is_file(), "a refused clear leaves the cache intact");

    // A job waiting for the user's executable pick has no active or queued entry; the runner's
    // `is_active` is what refuses the clear.
    let waiting = PortJobRow {
        id: "job-waiting".into(),
        catalog_id: "qcat_lantern".into(),
        slug: "bloodborne-pc".into(),
        port_name: "Bloodborne".into(),
        release_version: None,
        install_folder: None,
        phase: PortJobPhase::AwaitingExecutable,
        created_paths: Vec::new(),
        registration_id: None,
        error: None,
        failure_output: None,
        seen: false,
        created_at: 1,
        updated_at: 1,
        finished_at: None,
    };
    store.save_job(&waiting).unwrap();
    service.set_runner_for_test(Arc::new(FakeRunner::awaiting("job-waiting")));
    match service.clear_cache() {
        Err(PortsError::InstallInProgress) => {}
        other => panic!("a waiting install must refuse the clear, got {other:?}"),
    }
    assert!(asset.is_file());

    service.set_runner_for_test(Arc::new(FakeRunner::idle()));
    let cleared = service
        .clear_cache()
        .expect("an idle runner allows the clear");
    assert_eq!(cleared.downloads_bytes, 0);
    assert!(!asset.exists());
    service.shutdown();
}

#[tokio::test]
async fn an_unverified_checksum_needs_the_request_flag() {
    let _serial = serial().lock().await;
    let db = database();
    let store: Arc<dyn PortStore> = Arc::new(db.ports_store());
    let local: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    let catalog = Arc::new(FakeCatalog::new(
        vec![entry()],
        Some(entry()),
        vec![release(None, Some(format!("sha256:{}", "b".repeat(64))))],
    ));
    let (service, rx) = start(service_config(store, local, catalog));
    service.open();
    wait_for_view(&rx, |view| view.catalog.state == PortCatalogState::Ready);
    let parent = tempfile::tempdir().unwrap();
    let mut request = PortInstallRequest {
        catalog_id: "qcat_lantern".into(),
        parent_folder: parent.path().to_string_lossy().into_owned(),
        supplied: Vec::new(),
        accept_unverified_checksum: false,
        accept_malware_flag: false,
        retry_job_id: None,
    };
    assert!(service.prepare_install(&request).is_err());
    request.accept_unverified_checksum = true;
    let prepared = service.prepare_install(&request).expect("confirmed plan");
    assert!(prepared.plan.asset.unverified);
    assert_eq!(prepared.plan.asset.sha256, "b".repeat(64));
    service.shutdown();
}

#[tokio::test]
async fn retry_reuses_the_stored_plan_and_its_supplied_sources() {
    let _serial = serial().lock().await;
    let db = database();
    let store: Arc<dyn PortStore> = Arc::new(db.ports_store());
    let local: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    let catalog = Arc::new(FakeCatalog::new(
        vec![entry()],
        Some(entry()),
        vec![release(Some(format!("sha256:{}", "a".repeat(64))), None)],
    ));
    let (service, rx) = start(service_config(store.clone(), local, catalog));
    service.open();
    wait_for_view(&rx, |view| view.catalog.state == PortCatalogState::Ready);
    let parent = tempfile::tempdir().unwrap();
    let prepared = service
        .prepare_install(&PortInstallRequest {
            catalog_id: "qcat_lantern".into(),
            parent_folder: parent.path().to_string_lossy().into_owned(),
            supplied: Vec::new(),
            accept_unverified_checksum: false,
            accept_malware_flag: false,
            retry_job_id: None,
        })
        .expect("plan");
    let plan: PortInstallPlan = prepared.plan.clone();
    let mut row = job_row(
        &plan,
        PortJobPhase::Failed {
            reason: "boom".into(),
        },
    );
    row.registration_id = None;
    row.finished_at = Some(1_793_220_400_000);
    store.save_job(&row).unwrap();
    store
        .save_job_resume(
            &row.id,
            &PortJobResume {
                plan: plan.clone(),
                match_input: PortMatchInput::Title {
                    title: "Bloodborne".into(),
                },
            },
        )
        .unwrap();
    let retried = service
        .prepare_install(&PortInstallRequest {
            catalog_id: "qcat_lantern".into(),
            parent_folder: parent.path().to_string_lossy().into_owned(),
            supplied: Vec::new(),
            accept_unverified_checksum: false,
            accept_malware_flag: false,
            retry_job_id: Some(row.id.clone()),
        })
        .expect("retry plan");
    assert_ne!(retried.job_id, row.id);
    assert_eq!(retried.plan.target, plan.target);
    assert!(
        retried
            .plan
            .staging
            .ends_with(&format!(".gamenode-staging-{}", retried.job_id))
    );
    assert_eq!(
        retried.match_input,
        PortMatchInput::Title {
            title: "Bloodborne".into()
        }
    );
    service.shutdown();
}

fn job_row(plan: &PortInstallPlan, phase: PortJobPhase) -> PortJobRow {
    PortJobRow {
        id: plan
            .staging
            .rsplit("-staging-")
            .next()
            .unwrap_or("job")
            .to_owned(),
        catalog_id: plan.catalog_id.clone(),
        slug: plan.slug.clone(),
        port_name: plan.title.clone(),
        release_version: Some(plan.release.version.clone()),
        install_folder: Some(plan.target.clone()),
        phase,
        created_paths: Vec::new(),
        registration_id: None,
        error: None,
        failure_output: None,
        seen: false,
        created_at: 1_793_220_348_000,
        updated_at: 1_793_220_348_000,
        finished_at: None,
    }
}

#[tokio::test]
async fn progress_ticks_do_not_publish_a_snapshot() {
    let _serial = serial().lock().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let seen = Arc::new(AtomicUsize::new(0));
    let progress = Arc::new(Mutex::new(Vec::<PortProgress>::new()));
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        let seen = Arc::clone(&seen);
        let progress = Arc::clone(&progress);
        shared.ports.port_progress = Arc::new(crate::engine::PortProgressPublisher::new(
            tokio::runtime::Handle::current(),
            move |tick| {
                seen.fetch_add(1, Ordering::SeqCst);
                progress.lock().unwrap().push(tick);
            },
        ));
    }
    let before = h.publishes.load(Ordering::SeqCst);
    h.engine
        .on_ports_event(crate::engine::pc_ports::PortsEvent::Progress(
            PortProgress {
                job_id: "job-1".into(),
                phase: PortProgressPhase::Downloading,
                slot_id: None,
                bytes_done: 2,
                bytes_total: 10,
            },
        ));
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        h.publishes.load(Ordering::SeqCst),
        before,
        "a progress tick must not republish the snapshot"
    );
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    assert_eq!(progress.lock().unwrap()[0].bytes_done, 2);
}

#[tokio::test]
async fn removing_a_port_registration_keeps_the_files_and_provenance() {
    let _serial = serial().lock().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let db = database();
    let ports_store: Arc<dyn PortStore> = Arc::new(db.ports_store());
    let local_store: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    let folder = tempfile::tempdir().unwrap();
    let executable = folder.path().join("Bloodborne.exe");
    std::fs::write(&executable, b"fixture").unwrap();
    let registration = Registration {
        id: "port-1".into(),
        title: "Bloodborne".into(),
        executable: executable.to_string_lossy().into_owned(),
        file_identity: "1:2:3".into(),
        root: folder.path().to_string_lossy().into_owned(),
        working_directory: folder.path().to_string_lossy().into_owned(),
        arguments: Vec::new(),
        gameplay: Vec::new(),
        launchers: Vec::new(),
        revision: 1,
        removed: false,
        rediscovery_allowed: false,
        transferred_from: None,
        available: true,
    };
    let installation = PortInstallation {
        registration_id: registration.id.clone(),
        catalog_id: "qcat_lantern".into(),
        slug: "bloodborne-pc".into(),
        port_name: "Bloodborne".into(),
        installed_version: "v0.9.38".into(),
        install_folder: registration.root.clone(),
        environment: [("BLOODBORNE_PATH".to_owned(), "D:/dump".to_owned())].into(),
        job_id: "job-1".into(),
    };
    let job = PortJobRow {
        id: "job-1".into(),
        catalog_id: "qcat_lantern".into(),
        slug: "bloodborne-pc".into(),
        port_name: "Bloodborne".into(),
        release_version: Some("v0.9.38".into()),
        install_folder: Some(registration.root.clone()),
        phase: PortJobPhase::Completed,
        created_paths: Vec::new(),
        registration_id: Some(registration.id.clone()),
        error: None,
        failure_output: None,
        seen: false,
        created_at: 1,
        updated_at: 1,
        finished_at: Some(1),
    };
    ports_store
        .commit_install(&job, &registration, &installation)
        .unwrap();
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(local_store.clone());
        shared.ports.port_store = Some(ports_store.clone());
    }
    // The launch environment comes from the provenance row.
    let environment = h.engine.port_launch_environment("port-1");
    assert_eq!(
        environment.get("BLOODBORNE_PATH").map(String::as_str),
        Some("D:/dump")
    );

    h.engine.remove_local_game("port-1".into(), false);
    h.until("port removed", |snapshot| {
        snapshot
            .local_games
            .registrations
            .iter()
            .any(|registration| registration.id == "port-1" && registration.removed)
    })
    .await;
    assert!(
        executable.is_file(),
        "removing a port never deletes its files"
    );
    assert_eq!(
        ports_store.installations().unwrap().len(),
        1,
        "the provenance row survives the registration"
    );
}

#[tokio::test]
async fn setting_preferences_returns_before_a_slow_database_answers() {
    let _serial = serial().lock().await;
    let db = database();
    let slow = Arc::new(GateStore::new(Arc::new(db.ports_store()), false));
    let store: Arc<dyn PortStore> = slow.clone();
    let local: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    let catalog = Arc::new(FakeCatalog::new(vec![entry()], None, Vec::new()));
    let (service, _rx) = start(service_config(store, local, catalog));
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.pc_ports = Some(service.clone());
        shared.ports.port_progress = Arc::new(crate::engine::PortProgressPublisher::disabled(
            tokio::runtime::Handle::current(),
        ));
    }
    let started = Instant::now();
    h.engine.ports_set_preferences(PortPreferencesPatch {
        view: Some(PortViewMode::List),
        ..Default::default()
    });
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "the command returned before SQLite answered: {:?}",
        started.elapsed()
    );
    slow.release();
    service.shutdown();
}

#[tokio::test]
async fn marking_a_result_seen_returns_before_a_slow_database_answers() {
    let _serial = serial().lock().await;
    let db = database();
    let slow = Arc::new(GateStore::new(Arc::new(db.ports_store()), true));
    let store: Arc<dyn PortStore> = slow.clone();
    let local: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    let catalog = Arc::new(FakeCatalog::new(vec![entry()], None, Vec::new()));
    let (service, rx) = start(service_config(store, local, catalog));
    wait_for_view(&rx, |_| true);
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.pc_ports = Some(service.clone());
    }
    slow.close();
    let started = Instant::now();
    h.engine.ports_mark_seen("qcat_lantern".into());
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "mark seen returned before SQLite answered: {:?}",
        started.elapsed()
    );
    slow.release();
    service.shutdown();
}

#[tokio::test]
async fn a_changed_ports_view_reaches_the_snapshot() {
    let _serial = serial().lock().await;
    let h = harness(FakeApi::new(), FakeCreds::with(None));
    let mut view = PortsView::default();
    view.catalog.state = PortCatalogState::Ready;
    view.catalog.fetched_at = Some(1_793_220_348);
    view.catalog.entry_count = 1;
    view.unseen_count = 2;
    h.engine.on_ports_event(PortsEvent::Changed {
        view: view.clone(),
        completion: None,
    });
    assert_eq!(h.engine.snapshot().ports, view);
}

#[test]
fn the_supplied_pick_payload_matches_the_ui_contract() {
    use crate::engine::pc_ports::{PortSuppliedPick, PortSuppliedStatus};
    let accepted = serde_json::to_value(PortSuppliedPick {
        slot_id: "game".into(),
        path: r"D:\Dumps\game".into(),
        status: PortSuppliedStatus::Accepted,
    })
    .unwrap();
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(accepted["slotId"], "game");

    let rejected = serde_json::to_value(PortSuppliedPick {
        slot_id: "game".into(),
        path: r"D:\Dumps\old".into(),
        status: PortSuppliedStatus::Rejected {
            message: "wrong version".into(),
            accepted_labels: vec!["CUSA03173 01.09".into()],
        },
    })
    .unwrap();
    assert_eq!(rejected["status"]["rejected"]["message"], "wrong version");
    assert_eq!(
        rejected["status"]["rejected"]["acceptedLabels"][0],
        "CUSA03173 01.09"
    );
}

#[test]
fn a_preferences_patch_distinguishes_a_missing_folder_from_null() {
    use crate::engine::pc_ports::PortPreferencesPatch;
    let missing: PortPreferencesPatch = serde_json::from_str("{}").unwrap();
    assert_eq!(missing.last_parent_folder, None);
    let cleared: PortPreferencesPatch =
        serde_json::from_str(r#"{"lastParentFolder":null}"#).unwrap();
    assert_eq!(cleared.last_parent_folder, Some(None));
    let set: PortPreferencesPatch =
        serde_json::from_str(r#"{"lastParentFolder":"D:\\Ports"}"#).unwrap();
    assert_eq!(set.last_parent_folder, Some(Some(r"D:\Ports".into())));
}

#[tokio::test]
async fn progress_is_coalesced_to_the_latest_value_per_job() {
    let _serial = serial().lock().await;
    let seen = Arc::new(Mutex::new(Vec::<PortProgress>::new()));
    let sink = Arc::clone(&seen);
    let publisher =
        crate::engine::PortProgressPublisher::new(tokio::runtime::Handle::current(), move |tick| {
            sink.lock().unwrap().push(tick)
        });
    for done in 1..=20 {
        publisher.publish(PortProgress {
            job_id: "job-1".into(),
            phase: PortProgressPhase::Downloading,
            slot_id: None,
            bytes_done: done,
            bytes_total: 100,
        });
    }
    tokio::time::sleep(Duration::from_millis(350)).await;
    let seen = seen.lock().unwrap();
    assert!(
        seen.len() <= 2,
        "twenty ticks burst into {} events",
        seen.len()
    );
    assert_eq!(seen.last().map(|tick| tick.bytes_done), Some(20));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_completed_install_appears_in_the_library_without_a_manual_rescan() {
    let _serial = serial().lock().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let db = database();
    let ports_store: Arc<dyn PortStore> = Arc::new(db.ports_store());
    let local_store: Arc<dyn LocalGameStore> = Arc::new(db.local_game_store());
    {
        let shared = Arc::get_mut(&mut h.engine.shared).unwrap();
        shared.ports.local_games = Some(local_store.clone());
        shared.ports.port_store = Some(ports_store.clone());
        shared.state.get_mut().unwrap().library = gamenode_core::library::LibraryMachine::new(
            crate::providers::load_bundled(gamenode_core::provider::Platform::Windows).providers,
            Default::default(),
        );
    }
    let folder = tempfile::tempdir().unwrap();
    let executable = folder.path().join("Lantern Engine.exe");
    std::fs::write(&executable, b"fixture").unwrap();
    let registration = Registration {
        id: "port-1".into(),
        title: "Bloodborne".into(),
        executable: executable.to_string_lossy().into_owned(),
        file_identity: "1:2:3".into(),
        root: folder.path().to_string_lossy().into_owned(),
        working_directory: folder.path().to_string_lossy().into_owned(),
        arguments: Vec::new(),
        gameplay: Vec::new(),
        launchers: Vec::new(),
        revision: 1,
        removed: false,
        rediscovery_allowed: false,
        transferred_from: None,
        available: true,
    };
    // An entry the sign-in scan must find: seeing it proves that scan already read the store, so
    // it cannot be the scan that picks up the install below.
    local_store
        .save_registration(&Registration {
            id: "earlier".into(),
            title: "Earlier".into(),
            file_identity: "4:5:6".into(),
            ..registration.clone()
        })
        .unwrap();
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("sign-in scan finished", |snapshot| {
        snapshot.library.as_ref().is_some_and(|library| {
            !library.scanning && library.rows.iter().any(|row| row.key == "manual:earlier")
        })
    })
    .await;

    let installation = PortInstallation {
        registration_id: registration.id.clone(),
        catalog_id: "qcat_lantern".into(),
        slug: "bloodborne-pc".into(),
        port_name: "Lantern Engine".into(),
        installed_version: "v0.9.38".into(),
        install_folder: registration.root.clone(),
        environment: Default::default(),
        job_id: "job-1".into(),
    };
    let job = PortJobRow {
        id: "job-1".into(),
        catalog_id: "qcat_lantern".into(),
        slug: "bloodborne-pc".into(),
        port_name: "Lantern Engine".into(),
        release_version: Some("v0.9.38".into()),
        install_folder: Some(registration.root.clone()),
        phase: PortJobPhase::Completed,
        created_paths: Vec::new(),
        registration_id: Some(registration.id.clone()),
        error: None,
        failure_output: None,
        seen: false,
        created_at: 1,
        updated_at: 1,
        finished_at: Some(1),
    };
    ports_store
        .commit_install(&job, &registration, &installation)
        .unwrap();

    h.engine
        .port_install_completed(gamenode_platform::ports::InstallCompletion {
            job_id: job.id.clone(),
            registration,
            installation,
            match_input: PortMatchInput::None,
        });

    h.until("the installed port is a library row", |snapshot| {
        snapshot.library.as_ref().is_some_and(|library| {
            library
                .rows
                .iter()
                .any(|row| row.key == "manual:port-1" && row.title == "Bloodborne")
        })
    })
    .await;
}
