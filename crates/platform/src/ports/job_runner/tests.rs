use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gamenode_core::drm_free::LocalGameStore;
use gamenode_core::ports::matching::PortMatchInput;
use gamenode_core::ports::plan::{SuppliedChoice, SuppliedSource};
use gamenode_core::ports::{
    CatalogEntry, CatalogGame, CopyRoot, FromPackage, HashAlgorithm, InstallPlanRequest, PortAsset,
    PortInstallPlan, PortJobPhase, PortRelease, PortStore, SetupFile, SetupKind, StorageError,
    VerifiedRelease, plan_install,
};
use gamenode_core::tools::ToolError;
use tempfile::TempDir;

use super::*;
use crate::db::SqliteSettingsStore;
use crate::download::{DownloadFetcher, DownloadUrlPolicy, compute_bytes_sha256};

const FIXED_NOW: i64 = 1_700_000_000_000;

struct FixedClock(i64);

impl Clock for FixedClock {
    fn now_millis(&self) -> i64 {
        self.0
    }
}

struct AllowAll;

impl DownloadUrlPolicy for AllowAll {
    fn is_allowed(&self, _url: &str) -> bool {
        true
    }
}

#[derive(Clone, Default)]
struct FetchStats {
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

impl FetchStats {
    fn max_active(&self) -> usize {
        self.max_active.load(Ordering::SeqCst)
    }
}

struct FakeFetcher {
    bytes: Vec<u8>,
    /// Runs before writing the bytes, so a test can observe the worker without a second thread.
    started: Mutex<Option<Sender<()>>>,
    release: Mutex<Option<Receiver<()>>>,
    stats: FetchStats,
}

impl FakeFetcher {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            started: Mutex::new(None),
            release: Mutex::new(None),
            stats: FetchStats::default(),
        }
    }

    fn gated(bytes: Vec<u8>) -> (Self, Receiver<()>, Sender<()>) {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let fetcher = Self {
            bytes,
            started: Mutex::new(Some(started_tx)),
            release: Mutex::new(Some(release_rx)),
            stats: FetchStats::default(),
        };
        (fetcher, started_rx, release_tx)
    }

    fn stats(&self) -> FetchStats {
        self.stats.clone()
    }
}

impl DownloadFetcher for FakeFetcher {
    fn fetch(&self, _url: &str, destination: &Path) -> Result<(), ToolError> {
        let active = self.stats.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.stats.max_active.fetch_max(active, Ordering::SeqCst);
        let result = {
            if let Some(started) = self.started.lock().unwrap().take() {
                let _ = started.send(());
                if let Some(release) = self.release.lock().unwrap().take() {
                    let _ = release.recv();
                }
            }
            fs::write(destination, &self.bytes).map_err(|error| ToolError::Io(error.to_string()))
        };
        self.stats.active.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

/// A deterministic identity for a test install.
struct PathIdentity;

impl ExecutableIdentity for PathIdentity {
    fn identity(&self, executable: &Path) -> Result<String, String> {
        Ok(format!("test:{}", executable.display()))
    }
}

struct FailingIdentity;

impl ExecutableIdentity for FailingIdentity {
    fn identity(&self, _executable: &Path) -> Result<String, String> {
        Err("the test identity is unavailable".into())
    }
}

/// Reaches the commit point, reports it, and waits before returning the identity. That lets a test
/// cancel a job after its files are in place, without a sleep.
struct GatedIdentity {
    reached: Mutex<Option<Sender<()>>>,
    release: Mutex<Option<Receiver<()>>>,
}

impl GatedIdentity {
    fn new() -> (Self, Receiver<()>, Sender<()>) {
        let (reached_tx, reached_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        (
            Self {
                reached: Mutex::new(Some(reached_tx)),
                release: Mutex::new(Some(release_rx)),
            },
            reached_rx,
            release_tx,
        )
    }
}

impl ExecutableIdentity for GatedIdentity {
    fn identity(&self, executable: &Path) -> Result<String, String> {
        if let Some(reached) = self.reached.lock().unwrap().take() {
            let _ = reached.send(());
            if let Some(release) = self.release.lock().unwrap().take() {
                let _ = release.recv();
            }
        }
        Ok(format!("test:{}", executable.display()))
    }
}

struct NoPackages;

impl PackageExtractor for NoPackages {
    fn install_tool(
        &self,
        extractor: &Extractor,
        _cancel: &CancelFlag,
    ) -> Result<InstalledTool, ExtractorFailure> {
        Err(ExtractorFailure {
            error: ExtractorError::InvalidExtractor {
                id: extractor.id.clone(),
            },
            output: String::new(),
            created_paths: Vec::new(),
        })
    }

    fn run(
        &self,
        _request: ExtractorRunRequest<'_>,
    ) -> Result<ExtractorRunOutcome, ExtractorFailure> {
        Err(ExtractorFailure {
            error: ExtractorError::InvalidExtractor {
                id: "unused".into(),
            },
            output: String::new(),
            created_paths: Vec::new(),
        })
    }
}

struct StubPackages {
    calls: AtomicUsize,
}

impl PackageExtractor for StubPackages {
    fn install_tool(
        &self,
        _extractor: &Extractor,
        _cancel: &CancelFlag,
    ) -> Result<InstalledTool, ExtractorFailure> {
        Ok(InstalledTool {
            root: PathBuf::from("/stub"),
            executable: PathBuf::from("/stub/tool.exe"),
        })
    }

    fn run(
        &self,
        request: ExtractorRunRequest<'_>,
    ) -> Result<ExtractorRunOutcome, ExtractorFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        fs::create_dir_all(request.output).unwrap();
        fs::write(request.output.join("eboot.bin"), b"boot").unwrap();
        Ok(ExtractorRunOutcome {
            output: "stub extractor output".into(),
            exit_code: Some(0),
            created_paths: vec![request.output.to_path_buf()],
        })
    }
}

/// One recorded call to the fake extractor.
#[derive(Clone, Debug)]
struct PackageCall {
    package: PathBuf,
    update: Option<PathBuf>,
    output: PathBuf,
    /// Whether the validator handed to this run accepted its output.
    validated: bool,
}

/// A fake that writes a distinct base and update tree, like a real one-package-per-run tool.
struct TreePackages {
    runs: Mutex<Vec<PackageCall>>,
    include_update_marker: bool,
    cancel_on_update: bool,
    fail_update: bool,
    #[cfg(unix)]
    escape_update: Option<PathBuf>,
}

impl TreePackages {
    fn new() -> Self {
        Self {
            runs: Mutex::new(Vec::new()),
            include_update_marker: true,
            cancel_on_update: false,
            fail_update: false,
            #[cfg(unix)]
            escape_update: None,
        }
    }

    fn runs(&self) -> Vec<PackageCall> {
        self.runs.lock().unwrap().clone()
    }
}

impl PackageExtractor for TreePackages {
    fn install_tool(
        &self,
        _extractor: &Extractor,
        _cancel: &CancelFlag,
    ) -> Result<InstalledTool, ExtractorFailure> {
        Ok(InstalledTool {
            root: PathBuf::from("/stub"),
            executable: PathBuf::from("/stub/tool.exe"),
        })
    }

    fn run(
        &self,
        request: ExtractorRunRequest<'_>,
    ) -> Result<ExtractorRunOutcome, ExtractorFailure> {
        let is_update = request.package.file_name() == Some(std::ffi::OsStr::new("update.pkg"));
        fs::create_dir_all(request.output).unwrap();
        if is_update {
            fs::create_dir_all(request.output.join("data")).unwrap();
            fs::write(request.output.join("eboot.bin"), b"update-boot").unwrap();
            fs::write(request.output.join("data/replaced.bin"), b"update-data").unwrap();
            fs::write(request.output.join("data/new.bin"), b"new").unwrap();
            if self.include_update_marker {
                fs::write(request.output.join("update-marker.txt"), b"marker").unwrap();
            }
            #[cfg(unix)]
            if let Some(outside) = &self.escape_update {
                std::os::unix::fs::symlink(outside, request.output.join("escape")).unwrap();
            }
        } else {
            fs::create_dir_all(request.output.join("data")).unwrap();
            fs::create_dir_all(request.output.join("sce_sys")).unwrap();
            fs::write(request.output.join("eboot.bin"), b"base-boot").unwrap();
            fs::write(request.output.join("sce_sys/param.sfo"), b"sfo").unwrap();
            fs::write(request.output.join("data/replaced.bin"), b"base-data").unwrap();
            fs::write(request.output.join("data/base-only.bin"), b"base-only").unwrap();
        }
        let validated = request.validate_output.validate(request.output).is_ok();
        self.runs.lock().unwrap().push(PackageCall {
            package: request.package.to_path_buf(),
            update: request.update.map(Path::to_path_buf),
            output: request.output.to_path_buf(),
            validated,
        });
        if is_update && self.cancel_on_update {
            request.cancel.cancel();
            return Err(ExtractorFailure {
                error: ExtractorError::Cancelled,
                output: "cancelled".into(),
                created_paths: Vec::new(),
            });
        }
        if is_update && self.fail_update {
            return Err(ExtractorFailure {
                error: ExtractorError::Exit { code: 1 },
                output: "the update package is unsupported".into(),
                created_paths: Vec::new(),
            });
        }
        Ok(ExtractorRunOutcome {
            output: "stub extractor output".into(),
            exit_code: Some(0),
            created_paths: vec![request.output.to_path_buf()],
        })
    }
}

struct Fixture {
    _root: TempDir,
    ports: Arc<dyn PortStore>,
    local: crate::local_game_db::SqliteLocalGameStore,
    data: PathBuf,
    parent: PathBuf,
    user_roots: UserDataRoots,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("Ports");
        fs::create_dir_all(&parent).unwrap();
        let settings = SqliteSettingsStore::open(&root.path().join("test.sqlite3")).unwrap();
        let ports: Arc<dyn PortStore> = Arc::new(settings.ports_store());
        let local = settings.local_game_store();
        let user_roots = UserDataRoots {
            data: root.path().join("roaming"),
            config: root.path().join("settings"),
            local_data: root.path().join("local"),
        };
        let data = root.path().join("data");
        Self {
            _root: root,
            ports,
            local,
            data,
            parent,
            user_roots,
        }
    }

    fn start(
        &self,
        fetcher: FakeFetcher,
        identity: Arc<dyn ExecutableIdentity>,
    ) -> (JobRunner, Receiver<JobEvent>) {
        self.start_with(fetcher, identity, Arc::new(NoPackages))
    }

    fn start_with(
        &self,
        fetcher: FakeFetcher,
        identity: Arc<dyn ExecutableIdentity>,
        packages: Arc<dyn PackageExtractor>,
    ) -> (JobRunner, Receiver<JobEvent>) {
        let (events, received) = mpsc::channel();
        let runner = JobRunner::start(JobRunnerConfig {
            store: Arc::clone(&self.ports),
            data_dir: self.data.clone(),
            fetcher: Arc::new(fetcher),
            url_policy: Arc::new(AllowAll),
            user_roots: self.user_roots.clone(),
            identity,
            packages,
            clock: Arc::new(FixedClock(FIXED_NOW)),
            on_event: Arc::new(move |event| {
                let _ = events.send(event);
            }),
        });
        (runner, received)
    }

    fn cache_file(&self, catalog_id: &str, version: &str, filename: &str) -> PathBuf {
        self.data
            .join("ports/cache")
            .join(catalog_id)
            .join(version)
            .join(filename)
    }
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    buffer.into_inner()
}

fn entry(catalog_id: &str, name: &str, launcher: &str) -> CatalogEntry {
    CatalogEntry {
        catalog_id: catalog_id.into(),
        slug: format!("{catalog_id}-slug"),
        name: name.into(),
        supported_os: vec!["windows".into()],
        games: vec![CatalogGame {
            title: name.into(),
            ..CatalogGame::default()
        }],
        gamenode_game_id: Some(42),
        verified: Some(VerifiedRelease {
            version: "v0.9.38".into(),
            released_at: 1,
            ..VerifiedRelease::default()
        }),
        launcher: gamenode_core::ports::LauncherRecipe {
            folder_name: Some(name.into()),
            files_to_add: vec!["portable.txt".into()],
            preferred_executables: gamenode_core::ports::PreferredExecutables {
                windows: vec![launcher.into()],
                ..Default::default()
            },
            ..Default::default()
        },
        gameplay_executables: vec![launcher.into()],
        ..CatalogEntry::default()
    }
}

fn release(bytes: &[u8], filename: &str) -> PortRelease {
    PortRelease {
        release_id: "rel-verified".into(),
        version: "v0.9.38".into(),
        released_at: 1,
        assets: vec![PortAsset {
            id: "windows".into(),
            url: format!("https://example.invalid/{filename}"),
            filename: filename.into(),
            os: Some("windows".into()),
            architecture: Some("x86_64".into()),
            format: Some("zip".into()),
            size: bytes.len() as u64,
            checksum: Some(format!("sha256:{}", compute_bytes_sha256(bytes))),
            ..PortAsset::default()
        }],
        ..PortRelease::default()
    }
}

/// A folder slot copied into the port's own folder.
fn game_slot() -> SetupFile {
    SetupFile {
        id: "game".into(),
        label: "Game folder".into(),
        kind: SetupKind::Folder,
        destination: Some("CUSA03173".into()),
        contains: vec!["eboot.bin".into()],
        required: true,
        ..SetupFile::default()
    }
}

/// An entry whose only slot can be satisfied by an installed folder or by console packages.
fn package_entry(catalog_id: &str, args: &[&str], contains: &[&str]) -> CatalogEntry {
    let mut entry = entry(catalog_id, "PackagePort", "Bloodborne.exe");
    entry.extractors = vec![Extractor {
        id: "ps4-pkg".into(),
        name: "Fake package extractor".into(),
        repository: "owner/repo".into(),
        tag: "v1.0.0".into(),
        asset: "tool.zip".into(),
        sha256: "a".repeat(64),
        license: "Unlicense".into(),
        executable: "tool.exe".into(),
        args: args
            .iter()
            .map(|argument| (*argument).to_string())
            .collect(),
    }];
    entry.launcher.setup_files = vec![SetupFile {
        id: "game".into(),
        label: "Game folder".into(),
        kind: SetupKind::Folder,
        destination: Some("CUSA03173".into()),
        contains: contains
            .iter()
            .map(|marker| (*marker).to_string())
            .collect(),
        from_package: Some(FromPackage {
            extractor: "ps4-pkg".into(),
            title_id: None,
            needs_update: Some("01.09".into()),
        }),
        ..SetupFile::default()
    }];
    entry
}

fn write_packages(root: &Path) -> (String, String) {
    fs::create_dir_all(root).unwrap();
    let base = root.join("base.pkg");
    let update = root.join("update.pkg");
    fs::write(&base, b"base package").unwrap();
    fs::write(&update, b"update package").unwrap();
    (base.display().to_string(), update.display().to_string())
}

/// A file slot copied to the user's roaming data.
fn settings_slot() -> SetupFile {
    SetupFile {
        id: "settings".into(),
        label: "Settings file".into(),
        kind: SetupKind::File,
        destination: Some("GameNode/port.ini".into()),
        root: Some(CopyRoot::Data),
        required: true,
        ..SetupFile::default()
    }
}

fn write_dumps(root: &Path) -> (String, String) {
    let game = root.join("dump/CUSA03173");
    fs::create_dir_all(&game).unwrap();
    fs::write(game.join("eboot.bin"), b"boot").unwrap();
    fs::write(game.join("data.bin"), b"data").unwrap();
    let settings = root.join("dump/settings.ini");
    fs::write(&settings, b"fullscreen=1").unwrap();
    (game.display().to_string(), settings.display().to_string())
}

#[allow(clippy::too_many_arguments)]
fn plan_for(
    entry: CatalogEntry,
    bytes: &[u8],
    filename: &str,
    parent: &Path,
    job_id: &str,
    supplied: Vec<SuppliedChoice>,
) -> (PortInstallPlan, Vec<SetupFile>, PortMatchInput) {
    let setup_files = entry.launcher.setup_files.clone();
    let match_input = PortMatchInput::from_entry(&entry);
    let request = InstallPlanRequest {
        entry,
        releases: vec![release(bytes, filename)],
        withdrawn: Vec::new(),
        parent: parent.display().to_string(),
        job_id: job_id.into(),
        target_empty: true,
        free_space: u64::MAX,
        library_roots: Vec::new(),
        supplied,
        supplied_bytes: 0,
        confirmed_risk: false,
    };
    (
        plan_install(&request).expect("test plan"),
        setup_files,
        match_input,
    )
}

fn request(
    job_id: &str,
    plan: PortInstallPlan,
    setup_files: Vec<SetupFile>,
    match_input: PortMatchInput,
) -> InstallJobRequest {
    InstallJobRequest {
        job_id: job_id.into(),
        plan,
        setup_files,
        match_input,
        executable: None,
        created_at: FIXED_NOW,
    }
}

/// Runs one package install to its terminal event and returns the plan for path assertions.
fn run_package_job(
    fixture: &Fixture,
    entry: CatalogEntry,
    job_id: &str,
    base: &str,
    update: Option<&str>,
    packages: Arc<TreePackages>,
) -> (PortJobRow, Option<Box<InstallCompletion>>, PortInstallPlan) {
    let bytes = zip_bytes(&[("Bloodborne.exe", b"launcher")]);
    let (plan, setup_files, match_input) = plan_for(
        entry,
        &bytes,
        &format!("{job_id}.zip"),
        &fixture.parent,
        job_id,
        vec![SuppliedChoice {
            slot: "game".into(),
            source: SuppliedSource::Packages {
                base: base.into(),
                update: update.map(str::to_string),
            },
        }],
    );
    let (runner, events) =
        fixture.start_with(FakeFetcher::new(bytes), Arc::new(PathIdentity), packages);
    runner
        .enqueue(request(job_id, plan.clone(), setup_files, match_input))
        .unwrap();
    let (job, completion) = wait_for_finished(&events);
    runner.shutdown();
    (job, completion, plan)
}

fn wait_for_finished(events: &Receiver<JobEvent>) -> (PortJobRow, Option<Box<InstallCompletion>>) {
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(15))
            .expect("an install event");
        if let JobEvent::Finished { job, completion } = event {
            return (job, completion);
        }
    }
}

fn wait_for_phase(events: &Receiver<JobEvent>, job_id: &str, wanted: PortJobPhase) -> PortJobRow {
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(15))
            .expect("an install event");
        if let JobEvent::PhaseChanged { job } = event
            && job.id == job_id
            && job.phase == wanted
        {
            return job;
        }
    }
}

fn wait_for_rejection(events: &Receiver<JobEvent>, job_id: &str) -> String {
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(15))
            .expect("an install event");
        if let JobEvent::ExecutableRejected {
            job_id: rejected,
            reason,
        } = event
            && rejected == job_id
        {
            return reason;
        }
    }
}

#[test]
fn a_completed_install_registers_and_keeps_every_file() {
    let fixture = Fixture::new();
    let dumps = fixture._root.path().join("dumps");
    let (game, settings) = write_dumps(&dumps);
    let bytes = zip_bytes(&[("Bloodborne.exe", b"launcher"), ("readme.txt", b"read")]);

    let mut target = entry("qcat_lantern", "Bloodborne", "Bloodborne.exe");
    target.launcher.setup_files = vec![game_slot(), settings_slot()];
    let (plan, setup_files, match_input) = plan_for(
        target,
        &bytes,
        "bloodborne.zip",
        &fixture.parent,
        "job-1",
        vec![
            SuppliedChoice {
                slot: "game".into(),
                source: SuppliedSource::Path { path: game },
            },
            SuppliedChoice {
                slot: "settings".into(),
                source: SuppliedSource::Path { path: settings },
            },
        ],
    );
    let install_folder = plan.target.clone();

    let (runner, events) = fixture.start(FakeFetcher::new(bytes.clone()), Arc::new(PathIdentity));
    runner
        .enqueue(request("job-1", plan, setup_files, match_input))
        .unwrap();
    let (job, completion) = wait_for_finished(&events);
    runner.shutdown();

    assert_eq!(job.phase, PortJobPhase::Completed);
    assert!(job.finished_at.is_some());
    let completion = completion.expect("a completed install has a completion");
    assert_eq!(completion.job_id, "job-1");
    assert_eq!(
        completion.match_input,
        PortMatchInput::GameNodeId { game_id: 42 }
    );
    assert_eq!(
        job.registration_id.as_deref(),
        Some(completion.registration.id.as_str())
    );

    // The port folder has the archive, the recipe marker and the supplied folder.
    let target = Path::new(&install_folder);
    assert_eq!(
        fs::read(target.join("Bloodborne.exe")).unwrap(),
        b"launcher"
    );
    assert_eq!(fs::read(target.join("portable.txt")).unwrap(), b"");
    assert_eq!(
        fs::read(target.join("CUSA03173/eboot.bin")).unwrap(),
        b"boot"
    );
    assert_eq!(
        fs::read(target.join("CUSA03173/data.bin")).unwrap(),
        b"data"
    );

    // The user-data copy was written during Finalizing and the source was left alone.
    assert_eq!(
        fs::read(fixture.user_roots.data.join("GameNode/port.ini")).unwrap(),
        b"fullscreen=1"
    );
    assert_eq!(
        fs::read(dumps.join("dump/settings.ini")).unwrap(),
        b"fullscreen=1"
    );

    // One transaction wrote the registration, the completed job and the provenance row.
    let registrations = fixture.local.registrations().unwrap();
    assert_eq!(registrations.len(), 1);
    let registration = &registrations[0];
    assert_eq!(registration.title, "Bloodborne");
    assert_eq!(registration.root, install_folder);
    assert_eq!(
        registration.executable,
        target.join("Bloodborne.exe").display().to_string()
    );
    assert_eq!(
        registration.gameplay,
        vec![target.join("Bloodborne.exe").display().to_string()]
    );
    assert_eq!(registration.arguments, Vec::<String>::new());
    assert!(registration.available);
    let installations = fixture.ports.installations().unwrap();
    assert_eq!(installations.len(), 1);
    assert_eq!(installations[0].registration_id, registration.id);
    assert_eq!(installations[0].installed_version, "v0.9.38");
    assert_eq!(installations[0].install_folder, install_folder);

    // The verified download is kept for a retry or a second install.
    assert!(
        fixture
            .cache_file("qcat_lantern", "v0.9.38", "bloodborne.zip")
            .is_file()
    );
}

#[test]
fn a_checksum_mismatch_fails_without_touching_the_install_folder() {
    let fixture = Fixture::new();
    let bytes = zip_bytes(&[("Bloodborne.exe", b"launcher")]);
    let mut target = entry("qcat_bad", "BadDownload", "Bloodborne.exe");
    target.verified = Some(VerifiedRelease {
        version: "v0.9.38".into(),
        ..VerifiedRelease::default()
    });
    let (plan, setup_files, match_input) = plan_for(
        target,
        &bytes,
        "bad.zip",
        &fixture.parent,
        "job-bad",
        Vec::new(),
    );
    let install_folder = PathBuf::from(&plan.target);
    let staging = PathBuf::from(&plan.staging);

    // The release declares the right hash, but the fetcher writes different bytes.
    let wrong = b"not the verified archive".to_vec();
    let (runner, events) = fixture.start(FakeFetcher::new(wrong), Arc::new(PathIdentity));
    runner
        .enqueue(request("job-bad", plan, setup_files, match_input))
        .unwrap();
    let (job, completion) = wait_for_finished(&events);
    runner.shutdown();

    assert!(matches!(job.phase, PortJobPhase::Failed { .. }));
    assert!(completion.is_none());
    assert!(
        job.error
            .as_deref()
            .is_some_and(|error| error.contains("download could not be verified")),
        "{:?}",
        job.error
    );
    assert!(!install_folder.exists());
    assert!(!staging.exists());
    assert!(fixture.local.registrations().unwrap().is_empty());
    assert!(fixture.ports.installations().unwrap().is_empty());
    let cache = fixture.data.join("ports/cache/qcat_bad/v0.9.38");
    assert!(
        !cache.exists() || fs::read_dir(&cache).unwrap().next().is_none(),
        "a mismatched download leaves no cached file"
    );
}

#[test]
fn a_failure_after_the_rename_removes_the_folder_and_external_copies() {
    let fixture = Fixture::new();
    let dumps = fixture._root.path().join("dumps");
    let (game, settings) = write_dumps(&dumps);
    let bytes = zip_bytes(&[("Bloodborne.exe", b"launcher")]);

    let mut target = entry("qcat_fail", "FailAfterCopy", "Bloodborne.exe");
    target.launcher.setup_files = vec![game_slot(), settings_slot()];
    let (plan, setup_files, match_input) = plan_for(
        target,
        &bytes,
        "fail.zip",
        &fixture.parent,
        "job-fail",
        vec![
            SuppliedChoice {
                slot: "game".into(),
                source: SuppliedSource::Path { path: game },
            },
            SuppliedChoice {
                slot: "settings".into(),
                source: SuppliedSource::Path { path: settings },
            },
        ],
    );
    let install_folder = PathBuf::from(&plan.target);
    let staging = PathBuf::from(&plan.staging);

    let (runner, events) = fixture.start(FakeFetcher::new(bytes), Arc::new(FailingIdentity));
    runner
        .enqueue(request("job-fail", plan, setup_files, match_input))
        .unwrap();
    let (job, completion) = wait_for_finished(&events);
    runner.shutdown();

    assert!(matches!(job.phase, PortJobPhase::Failed { .. }));
    assert!(completion.is_none());
    assert!(
        !install_folder.exists(),
        "the renamed folder was rolled back"
    );
    assert!(!staging.exists());
    assert!(
        !fixture.user_roots.data.join("GameNode/port.ini").exists(),
        "the external-root copy was rolled back"
    );
    assert!(fixture.local.registrations().unwrap().is_empty());
    assert!(fixture.ports.installations().unwrap().is_empty());
    // The verified download survives a later failure (spec: retry reuses it).
    assert!(
        fixture
            .cache_file("qcat_fail", "v0.9.38", "fail.zip")
            .is_file()
    );
}

#[test]
fn cancelling_after_the_rename_removes_the_installed_folder() {
    let fixture = Fixture::new();
    let dumps = fixture._root.path().join("dumps");
    let (game, settings) = write_dumps(&dumps);
    let bytes = zip_bytes(&[("Bloodborne.exe", b"launcher")]);

    let mut target = entry("qcat_cancel", "CancelAfterCopy", "Bloodborne.exe");
    target.launcher.setup_files = vec![game_slot(), settings_slot()];
    let (plan, setup_files, match_input) = plan_for(
        target,
        &bytes,
        "cancel.zip",
        &fixture.parent,
        "job-cancel",
        vec![
            SuppliedChoice {
                slot: "game".into(),
                source: SuppliedSource::Path { path: game },
            },
            SuppliedChoice {
                slot: "settings".into(),
                source: SuppliedSource::Path { path: settings },
            },
        ],
    );
    let install_folder = PathBuf::from(&plan.target);
    let staging = PathBuf::from(&plan.staging);

    let (identity, reached, release) = GatedIdentity::new();
    let (runner, events) = fixture.start(FakeFetcher::new(bytes), Arc::new(identity));
    runner
        .enqueue(request("job-cancel", plan, setup_files, match_input))
        .unwrap();

    // The identity is read during Registering, after the rename and the external copies.
    reached
        .recv_timeout(Duration::from_secs(15))
        .expect("the job reached the commit point");
    assert!(install_folder.is_dir(), "the folder is already renamed");
    assert!(runner.is_active("job-cancel"));
    assert!(runner.cancel("job-cancel"));
    let _ = release.send(());

    let (job, completion) = wait_for_finished(&events);
    runner.shutdown();

    assert_eq!(job.phase, PortJobPhase::Cancelled);
    assert!(completion.is_none());
    assert!(!install_folder.exists());
    assert!(!staging.exists());
    assert!(!fixture.user_roots.data.join("GameNode/port.ini").exists());
    assert!(fixture.local.registrations().unwrap().is_empty());
    assert!(fixture.ports.installations().unwrap().is_empty());
    // Cancelling never removes a verified cached download.
    assert!(
        fixture
            .cache_file("qcat_cancel", "v0.9.38", "cancel.zip")
            .is_file()
    );
}

#[test]
fn jobs_run_one_at_a_time_and_a_cancelled_queued_job_creates_nothing() {
    let fixture = Fixture::new();
    let first_bytes = zip_bytes(&[("Bloodborne.exe", b"launcher")]);
    let second_bytes = zip_bytes(&[("Celeste.exe", b"launcher")]);

    let first = entry("qcat_first", "FirstPort", "Bloodborne.exe");
    let (first_plan, first_setup, first_match) = plan_for(
        first,
        &first_bytes,
        "first.zip",
        &fixture.parent,
        "job-first",
        Vec::new(),
    );
    let second = entry("qcat_second", "SecondPort", "Celeste.exe");
    let (second_plan, second_setup, second_match) = plan_for(
        second,
        &second_bytes,
        "second.zip",
        &fixture.parent,
        "job-second",
        Vec::new(),
    );
    let second_folder = PathBuf::from(&second_plan.target);
    let second_staging = PathBuf::from(&second_plan.staging);

    let (fetcher, started, release) = FakeFetcher::gated(first_bytes);
    let stats = fetcher.stats();
    let (runner, events) = fixture.start(fetcher, Arc::new(PathIdentity));
    runner
        .enqueue(request("job-first", first_plan, first_setup, first_match))
        .unwrap();
    started
        .recv_timeout(Duration::from_secs(15))
        .expect("the first job is downloading");

    runner
        .enqueue(request(
            "job-second",
            second_plan,
            second_setup,
            second_match,
        ))
        .unwrap();
    assert_eq!(runner.queued_jobs(), vec!["job-second".to_string()]);
    assert!(runner.cancel("job-second"));
    let _ = release.send(());

    let mut finished = Vec::new();
    while finished.len() < 2 {
        let event = events
            .recv_timeout(Duration::from_secs(15))
            .expect("both jobs finish");
        if let JobEvent::Finished { job, .. } = event {
            finished.push(job);
        }
    }
    runner.shutdown();

    let first = finished
        .iter()
        .find(|job| job.id == "job-first")
        .expect("the first job finished");
    let second = finished
        .iter()
        .find(|job| job.id == "job-second")
        .expect("the second job finished");
    assert_eq!(first.phase, PortJobPhase::Completed);
    assert_eq!(second.phase, PortJobPhase::Cancelled);
    assert!(
        !second_folder.exists(),
        "a cancelled queued job writes nothing"
    );
    assert!(!second_staging.exists());
    assert_eq!(
        fixture.local.registrations().unwrap().len(),
        1,
        "only the completed job registered"
    );
    assert_eq!(
        stats.max_active(),
        1,
        "the single worker never downloads two assets at once"
    );
}

#[test]
fn cancelling_during_download_creates_nothing() {
    let fixture = Fixture::new();
    let bytes = zip_bytes(&[("Bloodborne.exe", b"launcher")]);
    let entry = entry("qcat_queued", "QueuedPort", "Bloodborne.exe");
    let (plan, setup_files, match_input) = plan_for(
        entry,
        &bytes,
        "queued.zip",
        &fixture.parent,
        "job-queued",
        Vec::new(),
    );
    let install_folder = PathBuf::from(&plan.target);

    // The runner's own queue is polled immediately, so pause it in the fetcher.
    let (fetcher, started, release) = FakeFetcher::gated(bytes.clone());
    let (runner, events) = fixture.start(fetcher, Arc::new(PathIdentity));
    runner
        .enqueue(request("job-queued", plan, setup_files, match_input))
        .unwrap();
    started.recv_timeout(Duration::from_secs(15)).unwrap();
    assert!(runner.cancel("job-queued"));
    let _ = release.send(());
    let (job, completion) = wait_for_finished(&events);
    runner.shutdown();

    assert_eq!(job.phase, PortJobPhase::Cancelled);
    assert!(completion.is_none());
    assert!(!install_folder.exists());
    assert!(fixture.local.registrations().unwrap().is_empty());
}

#[test]
fn invalid_requests_are_refused_before_a_job_exists() {
    let fixture = Fixture::new();
    let bytes = zip_bytes(&[("Bloodborne.exe", b"launcher")]);
    let entry = entry("qcat_invalid", "InvalidPort", "Bloodborne.exe");
    let (plan, setup_files, match_input) = plan_for(
        entry,
        &bytes,
        "invalid.zip",
        &fixture.parent,
        "job-invalid",
        Vec::new(),
    );
    let (runner, _events) = fixture.start(FakeFetcher::new(bytes), Arc::new(PathIdentity));

    let mut invalid = request(
        "job-invalid",
        plan.clone(),
        setup_files.clone(),
        match_input.clone(),
    );
    invalid.job_id = "../escape".into();
    assert!(matches!(
        runner.enqueue(invalid),
        Err(JobError::InvalidJobId { .. })
    ));

    let mut invalid = request(
        "job-invalid",
        plan.clone(),
        setup_files.clone(),
        match_input.clone(),
    );
    invalid.plan.staging = "/tmp/elsewhere".into();
    assert!(matches!(
        runner.enqueue(invalid),
        Err(JobError::StagingMismatch { .. })
    ));

    let mut invalid = request("job-invalid", plan.clone(), Vec::new(), match_input.clone());
    invalid.plan.slots = vec![gamenode_core::ports::PlannedSlot {
        id: "game".into(),
        label: "Game".into(),
        required: true,
        kind: SetupKind::Folder,
        accepted: Vec::new(),
        placement: gamenode_core::ports::Placement::Copy {
            root: CopyRoot::App,
            destination: "CUSA03173".into(),
        },
        from_package: None,
        source: Some(SuppliedSource::Path {
            path: "/tmp/game".into(),
        }),
        target: Some("/tmp/game".into()),
    }];
    assert!(matches!(
        runner.enqueue(invalid),
        Err(JobError::MissingSlot { .. })
    ));

    let mut invalid = request("job-invalid", plan, setup_files, match_input);
    invalid.executable = Some("../evil.exe".into());
    assert!(matches!(
        runner.enqueue(invalid),
        Err(JobError::InvalidExecutable { .. })
    ));

    assert!(fixture.ports.jobs().unwrap().is_empty());
    runner.shutdown();
}

#[test]
fn hashing_progress_is_reported_for_a_supplied_file() {
    let fixture = Fixture::new();
    let dumps = fixture._root.path().join("dumps");
    fs::create_dir_all(&dumps).unwrap();
    let iso = dumps.join("disc.iso");
    fs::write(&iso, vec![7u8; 64 * 1024]).unwrap();
    let bytes = zip_bytes(&[("Bloodborne.exe", b"launcher")]);

    let mut entry = entry("qcat_progress", "ProgressPort", "Bloodborne.exe");
    entry.launcher.setup_files = vec![SetupFile {
        id: "disc".into(),
        label: "Disc image".into(),
        kind: SetupKind::File,
        flag: Some("--iso".into()),
        required: true,
        accepted: vec![gamenode_core::ports::AcceptedHash {
            algo: HashAlgorithm::Sha256,
            value: compute_bytes_sha256(&[7u8; 64 * 1024]),
            label: Some("test disc".into()),
        }],
        ..SetupFile::default()
    }];
    let (plan, setup_files, match_input) = plan_for(
        entry,
        &bytes,
        "progress.zip",
        &fixture.parent,
        "job-progress",
        vec![SuppliedChoice {
            slot: "disc".into(),
            source: SuppliedSource::Path {
                path: iso.display().to_string(),
            },
        }],
    );
    assert_eq!(plan.registration.arguments[0], "--iso");
    assert_eq!(plan.registration.arguments[1], iso.display().to_string());

    let (runner, events) = fixture.start(FakeFetcher::new(bytes), Arc::new(PathIdentity));
    runner
        .enqueue(request("job-progress", plan, setup_files, match_input))
        .unwrap();
    let mut saw_hash_progress = false;
    let mut saw_supplied_phase = false;
    let (job, _) = loop {
        let event = events
            .recv_timeout(Duration::from_secs(15))
            .expect("install events");
        match event {
            JobEvent::PhaseChanged { job } => {
                if matches!(job.phase, PortJobPhase::SuppliedFiles { .. }) {
                    saw_supplied_phase = true;
                }
            }
            JobEvent::Progress(progress) => {
                if progress.slot.as_deref() == Some("disc")
                    && progress.done > 0
                    && progress.total > 0
                {
                    saw_hash_progress = true;
                }
            }
            JobEvent::ExecutableRejected { .. } => {}
            JobEvent::Finished { job, completion } => break (job, completion),
        }
    };
    runner.shutdown();
    assert_eq!(job.phase, PortJobPhase::Completed);
    assert!(saw_supplied_phase);
    assert!(saw_hash_progress, "hashing reports bytes done and total");
}

#[test]
fn a_result_is_marked_seen_without_rewriting_the_job() {
    let fixture = Fixture::new();
    let bytes = zip_bytes(&[("Bloodborne.exe", b"launcher")]);
    let entry = entry("qcat_seen", "SeenPort", "Bloodborne.exe");
    let (plan, setup_files, match_input) = plan_for(
        entry,
        &bytes,
        "seen.zip",
        &fixture.parent,
        "job-seen",
        Vec::new(),
    );
    let (runner, events) = fixture.start(FakeFetcher::new(bytes), Arc::new(PathIdentity));
    runner
        .enqueue(request("job-seen", plan, setup_files, match_input))
        .unwrap();
    let (job, _) = wait_for_finished(&events);
    runner.shutdown();
    assert!(!job.seen);

    let seen = mark_job_seen(fixture.ports.as_ref(), "job-seen", FIXED_NOW + 1)
        .unwrap()
        .expect("the job exists");
    assert!(seen.seen);
    assert_eq!(seen.phase, PortJobPhase::Completed);
    assert!(fixture.ports.job("job-seen").unwrap().unwrap().seen);
    // The journal is untouched by marking the result seen.
    assert!(!fixture.ports.created_paths("job-seen").unwrap().is_empty());
    assert!(
        mark_job_seen(fixture.ports.as_ref(), "missing", FIXED_NOW)
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_package_slot_is_extracted_straight_into_the_staging_folder() {
    let fixture = Fixture::new();
    let bytes = zip_bytes(&[("Bloodborne.exe", b"launcher")]);
    let mut entry = entry("qcat_package", "PackagePort", "Bloodborne.exe");
    entry.extractors = vec![Extractor {
        id: "ps4-pkg".into(),
        name: "Stub extractor".into(),
        repository: "owner/repo".into(),
        tag: "v1.0.0".into(),
        asset: "tool.zip".into(),
        sha256: "a".repeat(64),
        license: "GPL-3.0".into(),
        executable: "tool.exe".into(),
        args: vec!["{package}".into()],
    }];
    entry.launcher.setup_files = vec![SetupFile {
        id: "game".into(),
        label: "Game folder".into(),
        kind: SetupKind::Folder,
        destination: Some("CUSA03173".into()),
        contains: vec!["eboot.bin".into()],
        from_package: Some(gamenode_core::ports::FromPackage {
            extractor: "ps4-pkg".into(),
            title_id: Some("CUSA03173".into()),
            needs_update: None,
        }),
        ..SetupFile::default()
    }];
    let (plan, setup_files, match_input) = plan_for(
        entry,
        &bytes,
        "package.zip",
        &fixture.parent,
        "job-package",
        vec![SuppliedChoice {
            slot: "game".into(),
            source: SuppliedSource::Packages {
                base: "base.pkg".into(),
                update: None,
            },
        }],
    );
    let install_folder = PathBuf::from(&plan.target);

    let packages = Arc::new(StubPackages {
        calls: AtomicUsize::new(0),
    });
    let (runner, events) = fixture.start_with(
        FakeFetcher::new(bytes),
        Arc::new(PathIdentity),
        packages.clone(),
    );
    runner
        .enqueue(request("job-package", plan, setup_files, match_input))
        .unwrap();
    let (job, completion) = wait_for_finished(&events);
    runner.shutdown();

    assert_eq!(job.phase, PortJobPhase::Completed);
    assert!(completion.is_some());
    assert_eq!(packages.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read(install_folder.join("CUSA03173/eboot.bin")).unwrap(),
        b"boot"
    );
    assert!(
        fixture
            .local
            .registrations()
            .unwrap()
            .iter()
            .any(|registration| registration.title == "PackagePort")
    );
}

#[test]
fn a_two_run_extraction_merges_the_update_into_the_base() {
    let fixture = Fixture::new();
    let (base, update) = write_packages(&fixture._root.path().join("packages"));
    let entry = package_entry(
        "qcat_pkg_merge",
        &["{package}", "-o", "{out}", "--overwrite"],
        &["eboot.bin", "update-marker.txt"],
    );
    let packages = Arc::new(TreePackages::new());
    let (job, completion, plan) = run_package_job(
        &fixture,
        entry,
        "job-pkg-merge",
        &base,
        Some(&update),
        packages.clone(),
    );
    assert_eq!(job.phase, PortJobPhase::Completed, "{:?}", job.error);
    assert!(completion.is_some());

    let runs = packages.runs();
    assert_eq!(runs.len(), 2, "the base and the update are two runs");
    assert_eq!(runs[0].package, PathBuf::from(&base));
    assert_eq!(runs[0].update, None);
    assert_eq!(
        runs[0].output,
        Path::new(&plan.staging).join("CUSA03173"),
        "the base extracts straight into the slot's output"
    );
    assert_eq!(runs[1].package, PathBuf::from(&update));
    assert_eq!(runs[1].update, None);
    assert_ne!(runs[1].output, runs[0].output);
    assert!(
        !runs[1].output.starts_with(&runs[0].output),
        "the update tree is never extracted inside the final game folder"
    );
    assert!(runs[1].output.starts_with(Path::new(&plan.staging)));
    assert!(
        runs.iter().all(|run| run.validated),
        "the intermediate trees are not validated"
    );

    let game = PathBuf::from(&plan.target).join("CUSA03173");
    assert_eq!(fs::read(game.join("eboot.bin")).unwrap(), b"update-boot");
    assert_eq!(
        fs::read(game.join("data/replaced.bin")).unwrap(),
        b"update-data"
    );
    assert_eq!(fs::read(game.join("data/new.bin")).unwrap(), b"new");
    assert_eq!(
        fs::read(game.join("data/base-only.bin")).unwrap(),
        b"base-only"
    );
    assert!(
        game.join("update-marker.txt").is_file(),
        "a file only the update has is added"
    );
    assert!(
        !runs[1].output.exists(),
        "the temporary update folder is removed after a successful merge"
    );
    assert!(
        !fs::read_dir(&game).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".gamenode-update-")),
        "no update staging folder reaches the install folder"
    );
}

#[test]
fn only_the_merged_folder_is_validated() {
    let fixture = Fixture::new();
    let (base, update) = write_packages(&fixture._root.path().join("packages"));
    let mut tree = TreePackages::new();
    // The update tree alone cannot satisfy the slot's markers.
    tree.include_update_marker = false;
    let packages = Arc::new(tree);
    let entry = package_entry(
        "qcat_pkg_validate",
        &["{package}", "-o", "{out}", "--overwrite"],
        &["eboot.bin", "update-marker.txt"],
    );
    let (job, completion, _plan) = run_package_job(
        &fixture,
        entry,
        "job-pkg-validate",
        &base,
        Some(&update),
        packages.clone(),
    );

    assert!(matches!(job.phase, PortJobPhase::Failed { .. }));
    assert!(completion.is_none());
    assert!(
        job.error
            .as_deref()
            .unwrap_or_default()
            .contains("update-marker.txt"),
        "the merged folder is what fails validation: {:?}",
        job.error
    );
    let runs = packages.runs();
    assert_eq!(runs.len(), 2);
    assert!(
        runs.iter().all(|run| run.validated),
        "each run used the permissive intermediate check"
    );
}

#[test]
fn cancelling_the_update_run_leaves_no_successful_install() {
    let fixture = Fixture::new();
    let (base, update) = write_packages(&fixture._root.path().join("packages"));
    let mut tree = TreePackages::new();
    tree.cancel_on_update = true;
    let packages = Arc::new(tree);
    let entry = package_entry(
        "qcat_pkg_cancel",
        &["{package}", "-o", "{out}", "--overwrite"],
        &["eboot.bin", "update-marker.txt"],
    );
    let (job, completion, plan) = run_package_job(
        &fixture,
        entry,
        "job-pkg-cancel",
        &base,
        Some(&update),
        packages.clone(),
    );

    assert_eq!(job.phase, PortJobPhase::Cancelled);
    assert!(completion.is_none());
    assert!(fixture.local.registrations().unwrap().is_empty());
    assert!(!Path::new(&plan.target).exists());
    let runs = packages.runs();
    assert_eq!(runs.len(), 2);
    assert!(!runs[1].output.exists());
}

#[test]
fn a_failed_update_run_removes_its_temporary_folder() {
    let fixture = Fixture::new();
    let (base, update) = write_packages(&fixture._root.path().join("packages"));
    let mut tree = TreePackages::new();
    tree.fail_update = true;
    let packages = Arc::new(tree);
    let entry = package_entry(
        "qcat_pkg_fail",
        &["{package}", "-o", "{out}", "--overwrite"],
        &["eboot.bin", "update-marker.txt"],
    );
    let (job, completion, plan) = run_package_job(
        &fixture,
        entry,
        "job-pkg-fail",
        &base,
        Some(&update),
        packages.clone(),
    );

    assert!(matches!(job.phase, PortJobPhase::Failed { .. }));
    assert!(completion.is_none());
    assert!(
        job.error
            .as_deref()
            .unwrap_or_default()
            .contains("exited with code 1")
    );
    assert!(!Path::new(&plan.target).exists());
    let runs = packages.runs();
    assert_eq!(runs.len(), 2);
    assert!(
        !runs[1].output.exists(),
        "the temporary update folder is removed on failure"
    );
}

#[cfg(unix)]
#[test]
fn an_update_tree_that_escapes_the_game_folder_is_rejected() {
    let fixture = Fixture::new();
    let (base, update) = write_packages(&fixture._root.path().join("packages"));
    let outside = fixture._root.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let mut tree = TreePackages::new();
    tree.escape_update = Some(outside.clone());
    let packages = Arc::new(tree);
    let entry = package_entry(
        "qcat_pkg_escape",
        &["{package}", "-o", "{out}", "--overwrite"],
        &["eboot.bin", "update-marker.txt"],
    );
    let (job, completion, plan) = run_package_job(
        &fixture,
        entry,
        "job-pkg-escape",
        &base,
        Some(&update),
        packages.clone(),
    );

    assert!(matches!(job.phase, PortJobPhase::Failed { .. }));
    assert!(completion.is_none());
    assert!(
        job.error
            .as_deref()
            .unwrap_or_default()
            .contains("unsafe path"),
        "the escaping link is refused: {:?}",
        job.error
    );
    assert!(!Path::new(&plan.target).exists());
    assert!(
        !outside.read_dir().unwrap().next().is_some(),
        "nothing is written through the link"
    );
    assert!(!packages.runs()[1].output.exists());
}

#[test]
fn an_update_argument_template_still_does_a_single_run() {
    let fixture = Fixture::new();
    let (base, update) = write_packages(&fixture._root.path().join("packages"));
    let entry = package_entry(
        "qcat_pkg_single",
        &["{package}", "{update}", "-o", "{out}"],
        &["eboot.bin"],
    );
    let packages = Arc::new(TreePackages::new());
    let (job, completion, _plan) = run_package_job(
        &fixture,
        entry,
        "job-pkg-single",
        &base,
        Some(&update),
        packages.clone(),
    );

    assert_eq!(job.phase, PortJobPhase::Completed, "{:?}", job.error);
    assert!(completion.is_some());
    let runs = packages.runs();
    assert_eq!(runs.len(), 1, "the tool merged the update itself");
    assert_eq!(runs[0].package, PathBuf::from(&base));
    assert_eq!(runs[0].update, Some(PathBuf::from(&update)));
    assert!(runs[0].validated);
}

#[test]
fn the_runner_and_its_events_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<JobRunner>();
    assert_send_sync::<JobEvent>();
    assert_send_sync::<InstallJobRequest>();
    assert_send_sync::<JobRunnerConfig>();
}

#[test]
fn the_queue_state_is_readable_before_the_worker_starts() {
    // The runner is created but no job is enqueued: the app can poll cheap shared state.
    let fixture = Fixture::new();
    let (runner, _events) = fixture.start(FakeFetcher::new(Vec::new()), Arc::new(PathIdentity));
    assert!(runner.active_job().is_none());
    assert!(runner.queued_jobs().is_empty());
    assert!(!runner.is_active("anything"));
    assert!(!runner.cancel("anything"));
    runner.shutdown();
}

/// Enqueues one job whose preferred executable is absent and waits for it to finalize. The archive
/// carries `Second.exe`, which the test can pick, but the recipe prefers `Missing.exe`.
fn start_awaiting(fixture: &Fixture, job_id: &str) -> (JobRunner, Receiver<JobEvent>, PathBuf) {
    let bytes = zip_bytes(&[("Second.exe", b"second"), ("notes.txt", b"text")]);
    let target = entry("qcat_await", "AwaitPort", "Missing.exe");
    let (plan, setup_files, match_input) = plan_for(
        target,
        &bytes,
        "await.zip",
        &fixture.parent,
        job_id,
        Vec::new(),
    );
    let folder = PathBuf::from(&plan.target);
    let (runner, events) = fixture.start(FakeFetcher::new(bytes), Arc::new(PathIdentity));
    runner
        .enqueue(request(job_id, plan, setup_files, match_input))
        .unwrap();
    wait_for_phase(&events, job_id, PortJobPhase::AwaitingExecutable);
    (runner, events, folder)
}

#[test]
fn a_missing_preferred_executable_awaits_a_pick_keeps_files_and_frees_the_worker() {
    let fixture = Fixture::new();
    let (runner, events, folder) = start_awaiting(&fixture, "job-await");

    // The install folder is final and journaled, but nothing was registered.
    assert!(folder.is_dir());
    assert_eq!(fs::read(folder.join("Second.exe")).unwrap(), b"second");
    assert!(!folder.join("Missing.exe").exists());
    assert!(fixture.ports.created_paths("job-await").unwrap().len() >= 2);
    assert!(fixture.ports.installations().unwrap().is_empty());
    assert!(fixture.local.registrations().unwrap().is_empty());
    let awaiting = fixture.ports.job("job-await").unwrap().unwrap();
    assert_eq!(awaiting.phase, PortJobPhase::AwaitingExecutable);
    assert_eq!(awaiting.registration_id, None);
    assert_eq!(awaiting.finished_at, None);
    assert!(!awaiting.phase.is_terminal());
    assert!(runner.is_active("job-await"));
    assert_eq!(
        runner
            .awaiting_jobs()
            .unwrap()
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        vec!["job-await"]
    );

    // A queued job runs to completion: the waiting job holds no worker slot.
    let bytes = zip_bytes(&[("Second.exe", b"second"), ("notes.txt", b"text")]);
    let next = entry("qcat_next", "NextPort", "Second.exe");
    let (next_plan, next_setup, next_match) = plan_for(
        next,
        &bytes,
        "next.zip",
        &fixture.parent,
        "job-next",
        Vec::new(),
    );
    runner
        .enqueue(request("job-next", next_plan, next_setup, next_match))
        .unwrap();
    let (finished, completion) = wait_for_finished(&events);
    assert_eq!(finished.id, "job-next");
    assert_eq!(finished.phase, PortJobPhase::Completed);
    assert!(completion.is_some());
    assert_eq!(fixture.local.registrations().unwrap().len(), 1);

    // Cancelling the waiting job removes the finalized folder and leaves no entry behind.
    assert!(runner.cancel("job-await"));
    let cancelled = loop {
        let event = events
            .recv_timeout(Duration::from_secs(15))
            .expect("the waiting job ends");
        if let JobEvent::Finished { job, .. } = event
            && job.id == "job-await"
        {
            break job;
        }
    };
    runner.shutdown();

    assert_eq!(cancelled.phase, PortJobPhase::Cancelled);
    assert!(cancelled.phase.is_terminal());
    assert!(!folder.exists(), "the install folder was removed");
    assert!(
        !folder
            .parent()
            .expect("a parent")
            .join(".gamenode-staging-job-await")
            .exists()
    );
    assert_eq!(fixture.local.registrations().unwrap().len(), 1);
    assert_eq!(fixture.ports.installations().unwrap().len(), 1);
    assert!(runner.awaiting_jobs().unwrap().is_empty());
}

#[test]
fn picking_an_executable_registers_the_entry_with_that_choice() {
    let fixture = Fixture::new();
    let (runner, events, folder) = start_awaiting(&fixture, "job-pick");

    runner
        .complete_with_executable("job-pick", "Second.exe")
        .unwrap();
    let (job, completion) = wait_for_finished(&events);
    runner.shutdown();

    assert_eq!(job.phase, PortJobPhase::Completed);
    assert!(job.finished_at.is_some());
    let completion = completion.expect("a completed install has a completion");
    assert_eq!(completion.job_id, "job-pick");
    assert_eq!(
        completion.match_input,
        PortMatchInput::GameNodeId { game_id: 42 }
    );
    assert_eq!(
        completion.registration.executable,
        folder.join("Second.exe").display().to_string()
    );
    assert_eq!(
        job.registration_id.as_deref(),
        Some(completion.registration.id.as_str())
    );

    // The entry, the provenance row and the files exist together.
    let registrations = fixture.local.registrations().unwrap();
    assert_eq!(registrations.len(), 1);
    assert_eq!(
        registrations[0].executable,
        folder.join("Second.exe").display().to_string()
    );
    assert_eq!(registrations[0].root, folder.display().to_string());
    let installations = fixture.ports.installations().unwrap();
    assert_eq!(installations.len(), 1);
    assert_eq!(installations[0].registration_id, registrations[0].id);
    assert_eq!(installations[0].job_id, "job-pick");
    assert_eq!(
        installations[0].install_folder,
        folder.display().to_string()
    );
    assert!(folder.join("Second.exe").is_file());
    assert!(runner.awaiting_jobs().unwrap().is_empty());
}

#[test]
fn invalid_executable_picks_are_refused_and_the_job_keeps_waiting() {
    let fixture = Fixture::new();
    let (runner, events, folder) = start_awaiting(&fixture, "job-invalid-pick");

    // Traversal, absolute paths and non-executables are refused before any disk work.
    for invalid in [
        "../Second.exe",
        r"..\Second.exe",
        "/Second.exe",
        r"C:\Windows\System32\cmd.exe",
        "Second.dll",
        "again",
    ] {
        assert!(
            matches!(
                runner.complete_with_executable("job-invalid-pick", invalid),
                Err(JobError::InvalidExecutable { .. })
            ),
            "{invalid:?} must be refused"
        );
    }

    // A missing file is checked on the worker and reported without touching the job.
    runner
        .complete_with_executable("job-invalid-pick", "Nope.exe")
        .unwrap();
    let reason = wait_for_rejection(&events, "job-invalid-pick");
    assert!(reason.contains("Nope.exe"), "{reason}");
    assert!(fixture.local.registrations().unwrap().is_empty());

    // A link that escapes the install folder is refused too, even when its name ends in `.exe`.
    let outside = fixture._root.path().join("outside.exe");
    fs::write(&outside, b"evil").unwrap();
    let link = folder.join("escape.exe");
    #[cfg(unix)]
    let link_created = std::os::unix::fs::symlink(&outside, &link).is_ok();
    #[cfg(windows)]
    let link_created = std::os::windows::fs::symlink_file(&outside, &link).is_ok();
    #[cfg(not(any(unix, windows)))]
    let link_created = false;
    if link_created {
        runner
            .complete_with_executable("job-invalid-pick", "escape.exe")
            .unwrap();
        let reason = wait_for_rejection(&events, "job-invalid-pick");
        assert!(reason.contains("outside the install folder"), "{reason}");
    }

    let still_waiting = fixture.ports.job("job-invalid-pick").unwrap().unwrap();
    assert_eq!(still_waiting.phase, PortJobPhase::AwaitingExecutable);
    assert!(folder.join("Second.exe").is_file());
    assert!(fixture.local.registrations().unwrap().is_empty());
    assert!(fixture.ports.installations().unwrap().is_empty());

    // The rejected picks did not take the job off the waiting list.
    runner
        .complete_with_executable("job-invalid-pick", "Second.exe")
        .unwrap();
    let (job, completion) = wait_for_finished(&events);
    runner.shutdown();
    assert_eq!(job.phase, PortJobPhase::Completed);
    assert!(completion.is_some());
}

#[test]
fn an_awaiting_job_survives_a_restart_and_is_completed_from_the_stored_plan() {
    let fixture = Fixture::new();
    let bytes = zip_bytes(&[("Second.exe", b"second")]);
    let target = entry("qcat_restart", "RestartPort", "Missing.exe");
    let (plan, setup_files, match_input) = plan_for(
        target,
        &bytes,
        "restart.zip",
        &fixture.parent,
        "job-restart",
        Vec::new(),
    );
    let folder = PathBuf::from(&plan.target);

    let (runner, events) = fixture.start(FakeFetcher::new(bytes.clone()), Arc::new(PathIdentity));
    runner
        .enqueue(request("job-restart", plan, setup_files, match_input))
        .unwrap();
    wait_for_phase(&events, "job-restart", PortJobPhase::AwaitingExecutable);
    runner.shutdown();

    // Startup recovery keeps a complete install that only waits for the pick.
    let report = {
        let local = &fixture.local;
        let registration_exists = |id: &str| -> Result<bool, StorageError> {
            Ok(local
                .registrations()?
                .iter()
                .any(|registration| registration.id == id && !registration.removed))
        };
        crate::ports::recovery::recover_interrupted_jobs(
            fixture.ports.as_ref(),
            &registration_exists,
            FIXED_NOW + 100,
        )
        .unwrap()
    };
    assert_eq!(report.awaiting, vec!["job-restart".to_string()]);
    assert!(report.completed.is_empty());
    assert!(report.interrupted.is_empty());
    assert!(folder.join("Second.exe").is_file());
    assert_eq!(
        fixture.ports.job("job-restart").unwrap().unwrap().phase,
        PortJobPhase::AwaitingExecutable
    );

    // A new runner is the restart: it loads the waiting job and finishes it from the stored plan.
    let (runner, events) = fixture.start(FakeFetcher::new(bytes), Arc::new(PathIdentity));
    let awaiting = runner.awaiting_jobs().unwrap();
    assert_eq!(
        awaiting
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        vec!["job-restart"]
    );
    assert!(runner.is_active("job-restart"));
    runner
        .complete_with_executable("job-restart", "Second.exe")
        .unwrap();
    let (job, completion) = wait_for_finished(&events);
    runner.shutdown();

    assert_eq!(job.phase, PortJobPhase::Completed);
    let completion = completion.expect("a completed install has a completion");
    assert_eq!(
        completion.match_input,
        PortMatchInput::GameNodeId { game_id: 42 }
    );
    assert_eq!(
        completion.registration.executable,
        folder.join("Second.exe").display().to_string()
    );
    assert_eq!(
        completion.installation.install_folder,
        folder.display().to_string()
    );
    assert_eq!(fixture.local.registrations().unwrap().len(), 1);
    assert_eq!(fixture.ports.installations().unwrap().len(), 1);
}
