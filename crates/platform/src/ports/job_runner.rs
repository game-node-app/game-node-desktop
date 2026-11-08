//! The PC ports install job runner (design D5, D6, D7, D8, D11).
//!
//! One worker thread owns every install. Callers enqueue a resolved plan and never wait on disk
//! work: the runner reports phase changes and progress through a callback and returns immediately.
//! Jobs run one at a time in the order they were queued; cancellation is a shared flag checked
//! between chunks of at most [`CHUNK_BYTES`](crate::ports::supplied::CHUNK_BYTES).
//!
//! ## Transactional behaviour
//!
//! - All work for `<parent>\<folder>` happens in `<parent>\.gamenode-staging-<job id>`. The finished
//!   folder reaches its final path with one rename.
//! - Every path is journaled through [`PortStore::record_created_path`] *before* it is written.
//!   Recovery (see [`super::recovery`]) removes the journal of any job that did not commit.
//! - The DRM-free registration is the commit point: [`PortStore::commit_install`] writes it, the
//!   completed job and the provenance row in one transaction. A cancellation or failure rolls the
//!   work back by deleting the journal, so no library entry exists for a job that did not commit.
//! - A verified download is cached under `<data_dir>/ports/cache/...` and is deliberately not
//!   journaled, so a retry reuses it.
//!
//! ## Threading
//!
//! Everything here performs blocking I/O. [`JobRunner::start`] creates the one worker thread; the
//! caller only posts commands. [`JobRunner::enqueue`] writes the queued job row, which is blocking,
//! so call it from a worker thread. The event callback runs on the install worker thread and must
//! be short and must never block or call [`JobRunner::shutdown`]. Progress events are emitted per
//! chunk; the app is expected to coalesce them into its `ports:progress` event (design D5).
//!
//! ## Public API for the app wiring
//!
//! - [`JobRunner::start`] with a [`JobRunnerConfig`] once at startup, after
//!   [`super::recovery::recover_interrupted_jobs`].
//! - [`JobRunner::enqueue`] with an [`InstallJobRequest`] when the user presses Install.
//! - [`JobRunner::cancel`] from the cancel button. Safe on any thread.
//! - [`JobRunner::complete_with_executable`] when the user picks an executable for a job that
//!   finished without one; the job's files stay in place while it waits.
//! - [`JobRunner::awaiting_jobs`] once at startup (on a worker thread), so jobs that waited for a
//!   pick before the last quit can be cancelled or completed.
//! - The configured event callback receives [`JobEvent::PhaseChanged`] (publish the snapshot),
//!   [`JobEvent::Progress`] (coalesce into `ports:progress`), [`JobEvent::ExecutableRejected`]
//!   (keep the picker open) and [`JobEvent::Finished`] (toast and snapshot). A completed job
//!   carries an [`InstallCompletion`] whose [`match_input`](InstallCompletion::match_input) is fed
//!   to [`gamenode_core::ports::matching`] after the app searched GameNode.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

use gamenode_core::drm_free::Registration;
use gamenode_core::ports::job::PortJobPhase;
use gamenode_core::ports::matching::PortMatchInput;
use gamenode_core::ports::plan::{PlannedSlot, PortInstallPlan, SuppliedSource};
use gamenode_core::ports::store::{PortInstallation, PortJobResume, PortJobRow, PortStore};
use gamenode_core::ports::{
    CopyRoot, Extractor, Placement, SetupFile, StorageError, is_safe_relative_path, join_folder,
    join_relative,
};

use crate::archive::PORTS_MAX_TOTAL_EXTRACTED_BYTES;
use crate::download::{
    CachedDownload, CachedDownloadRequest, DownloadFetcher, DownloadUrlPolicy,
    ensure_cached_download, ports_asset_cache_dir,
};
use crate::ports::catalog_worker::Clock;
use crate::ports::extractor::{
    CancelFlag, ExtractorError, ExtractorFailure, ExtractorRunOutcome, ExtractorRunRequest,
    ExtractorRunner, InstalledTool, OutputFolderValidator, merge_update_tree, run_extractor,
    supplied_folder_validator, template_takes_update,
};
use crate::ports::placement::{CopyRoots, PlaceError, SlotChoice, place_slots};
use crate::ports::supplied::{
    CheckOutcome, CheckedChoice, SupplyError, WorkProgress, check_choice,
};
use crate::random::session_id;

/// The job id must be usable in the staging folder name, exactly like the core planner requires.
fn valid_job_id(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where copies to the user's roaming data, settings and local data folders go.
///
/// Production resolves them from the host; tests inject temporary folders so no real user data is
/// touched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserDataRoots {
    /// `%APPDATA%`.
    pub data: PathBuf,
    /// The settings folder (`%APPDATA%` as well).
    pub config: PathBuf,
    /// `%LOCALAPPDATA%`.
    pub local_data: PathBuf,
}

impl UserDataRoots {
    /// Resolves the folders the operating system reports.
    pub fn host() -> Result<Self, PlaceError> {
        let roaming = dirs::config_dir().ok_or(PlaceError::UnresolvedRoot { root: "data" })?;
        let local =
            dirs::data_local_dir().ok_or(PlaceError::UnresolvedRoot { root: "localData" })?;
        Ok(Self {
            data: roaming.clone(),
            config: roaming,
            local_data: local,
        })
    }

    /// The four placement roots for one job, with the staging folder as the port's own folder.
    pub fn copy_roots(&self, app: impl Into<PathBuf>) -> CopyRoots {
        CopyRoots {
            app: app.into(),
            data: self.data.clone(),
            config: self.config.clone(),
            local_data: self.local_data.clone(),
        }
    }
}

/// Reads the stable identity of an installed executable.
///
/// This must return the same value the DRM-free launch validation compares, so an installed port
/// launches after the app restarts. The app can implement it with the DRM-free installation host's
/// `inspect`; [`SystemExecutableIdentity`] is the production adapter.
pub trait ExecutableIdentity: Send + Sync {
    /// The identity of `executable`, or a user-facing reason it could not be read.
    fn identity(&self, executable: &Path) -> Result<String, String>;
}

/// The identity a Windows install records: volume serial, file index high and low.
///
/// The value matches what `gamenode_platform::installations` checks before launching, so the
/// registration written at the commit point stays launchable. Port installs are Windows-only, so
/// other platforms report that the identity is unavailable.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemExecutableIdentity;

impl ExecutableIdentity for SystemExecutableIdentity {
    fn identity(&self, executable: &Path) -> Result<String, String> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
            };

            let file = std::fs::File::open(executable).map_err(|error| error.to_string())?;
            let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
            // SAFETY: the handle is valid and owned here; the output buffer is writable.
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
                return Err(format!(
                    "could not read the identity of {}",
                    executable.display()
                ));
            }
            // SAFETY: the successful call initialized the output.
            let info = unsafe { info.assume_init() };
            Ok(format!(
                "{}:{}:{}",
                info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
            ))
        }
        #[cfg(not(windows))]
        {
            let _ = executable;
            Err("PC port installs are only supported on Windows".into())
        }
    }
}

/// Prepares and runs the pinned console-package extraction tools (design D8).
///
/// Production is [`PinnedExtractor`]; tests inject a fake so they never download or run a tool.
pub trait PackageExtractor: Send + Sync {
    /// Downloads, verifies and extracts the pinned tool.
    fn install_tool(
        &self,
        extractor: &Extractor,
        cancel: &CancelFlag,
    ) -> Result<InstalledTool, ExtractorFailure>;

    /// Runs a prepared tool once.
    fn run(
        &self,
        request: ExtractorRunRequest<'_>,
    ) -> Result<ExtractorRunOutcome, ExtractorFailure>;
}

/// The production extractor: pinned GitHub releases through the shared verified download cache.
pub struct PinnedExtractor {
    runner: ExtractorRunner,
}

impl PinnedExtractor {
    /// Prepares tools under `<data_dir>/ports/tools/<id>/<tag>/`.
    pub fn new(data_dir: impl Into<PathBuf>) -> Result<Self, ExtractorError> {
        Ok(Self {
            runner: ExtractorRunner::new(data_dir)?,
        })
    }

    /// Runner with an injected downloader, for tests and mirrors.
    pub fn with_downloader(
        data_dir: impl Into<PathBuf>,
        fetcher: Box<dyn DownloadFetcher>,
        policy: Box<dyn DownloadUrlPolicy>,
    ) -> Self {
        Self {
            runner: ExtractorRunner::with_downloader(data_dir, fetcher, policy),
        }
    }
}

impl PackageExtractor for PinnedExtractor {
    fn install_tool(
        &self,
        extractor: &Extractor,
        cancel: &CancelFlag,
    ) -> Result<InstalledTool, ExtractorFailure> {
        self.runner.install_tool(extractor, cancel)
    }

    fn run(
        &self,
        request: ExtractorRunRequest<'_>,
    ) -> Result<ExtractorRunOutcome, ExtractorFailure> {
        run_extractor(request)
    }
}

/// Everything the runner needs. Built once at startup.
pub struct JobRunnerConfig {
    /// The durable job, registration and provenance state.
    pub store: Arc<dyn PortStore>,
    /// The application data folder; downloads are cached under `ports/cache`.
    pub data_dir: PathBuf,
    /// The blocking HTTPS fetcher for release assets.
    pub fetcher: Arc<dyn DownloadFetcher>,
    /// Which URLs the fetcher may use (HTTPS in production).
    pub url_policy: Arc<dyn DownloadUrlPolicy>,
    /// Where copies to the user's own data folders go.
    pub user_roots: UserDataRoots,
    /// The identity written into the DRM-free registration.
    pub identity: Arc<dyn ExecutableIdentity>,
    /// The pinned package extractor.
    pub packages: Arc<dyn PackageExtractor>,
    /// Unix milliseconds for job timestamps.
    pub clock: Arc<dyn Clock>,
    /// Runs on the install worker thread for every event; must not block.
    pub on_event: Arc<dyn Fn(JobEvent) + Send + Sync>,
}

/// A resolved install the runner will execute.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallJobRequest {
    /// The id that names the staging folder and the stored job row.
    pub job_id: String,
    /// The plan built by [`gamenode_core::ports::plan_install`].
    pub plan: PortInstallPlan,
    /// The merged recipe's setup files, by id. The plan carries what to place, but validation
    /// (markers, `hash_file`, accepted versions) needs the recipe rules.
    pub setup_files: Vec<SetupFile>,
    /// The evidence a completed install gives the local matcher (design D11).
    pub match_input: PortMatchInput,
    /// An executable the user picked, relative to the install folder. `None` uses the recipe's
    /// preferred executable.
    pub executable: Option<String>,
    /// Unix milliseconds when the user pressed Install.
    pub created_at: i64,
}

/// Bytes done and total inside one phase.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub job_id: String,
    /// The phase the bytes belong to.
    pub phase: PortJobPhase,
    /// The supplied-file slot, when the work belongs to one.
    pub slot: Option<String>,
    pub done: u64,
    /// `0` when the total is unknown.
    pub total: u64,
}

impl JobProgress {
    /// The fraction completed, when the total is known.
    pub fn fraction(&self) -> Option<f64> {
        (self.total > 0).then(|| self.done as f64 / self.total as f64)
    }
}

/// A completed install. The app applies it to the library and feeds the match input to the local
/// matcher (design D11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCompletion {
    pub job_id: String,
    /// The registration written by the commit transaction.
    pub registration: Registration,
    /// The provenance row written by the same transaction.
    pub installation: PortInstallation,
    /// The overlay game id or upstream title for [`gamenode_core::ports::matching::decide`].
    pub match_input: PortMatchInput,
}

/// What the worker reports.
#[derive(Clone, Debug)]
pub enum JobEvent {
    /// The job entered a new phase; `job` is the stored row. Publish the snapshot.
    PhaseChanged { job: PortJobRow },
    /// Bytes progress inside the current phase. Coalesce into `ports:progress`.
    Progress(JobProgress),
    /// The user's executable pick was refused; the job is unchanged and still waits for one.
    ExecutableRejected { job_id: String, reason: String },
    /// The job is terminal. `completion` is present only for a completed install.
    Finished {
        job: PortJobRow,
        completion: Option<Box<InstallCompletion>>,
    },
}

/// Marks a terminal job's result as seen, for the badge and the toast acknowledgement.
///
/// Running and queued jobs are never counted as unseen, so they are left unchanged. Returns the
/// stored row (updated or not), or `None` when the job does not exist. Blocking: call it from a
/// worker thread.
pub fn mark_job_seen(
    store: &dyn PortStore,
    job_id: &str,
    now: i64,
) -> Result<Option<PortJobRow>, StorageError> {
    let Some(mut job) = store.job(job_id)? else {
        return Ok(None);
    };
    if job.phase.is_terminal() && !job.seen {
        job.seen = true;
        job.updated_at = now;
        store.save_job(&job)?;
    }
    Ok(Some(job))
}

/// Why a job could not be enqueued.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum JobError {
    #[error("the job id {job_id:?} cannot be used in a folder name")]
    InvalidJobId { job_id: String },
    #[error("the plan's staging folder does not belong to job {job_id:?}")]
    StagingMismatch { job_id: String },
    #[error("setup file {slot:?} is missing from the request")]
    MissingSlot { slot: String },
    #[error("the executable {path:?} is not a safe relative path")]
    InvalidExecutable { path: String },
    #[error("job {job_id:?} is already queued or running")]
    DuplicateJob { job_id: String },
    #[error("job {job_id:?} is not waiting for an executable")]
    NotAwaitingExecutable { job_id: String },
    #[error("the local database could not be used: {0}")]
    Storage(#[from] StorageError),
    #[error("the install worker has stopped")]
    Closed,
}

/// Why an install failed. The message is user-facing and stored in the job row.
#[derive(Debug, thiserror::Error)]
enum InstallError {
    #[error("the download could not be verified: {0}")]
    Download(String),
    #[error("the install files could not be extracted: {0}")]
    Extract(String),
    #[error(transparent)]
    Supply(#[from] SupplyError),
    #[error(transparent)]
    Place(#[from] PlaceError),
    #[error("the install folder {path} already contains files")]
    TargetNotEmpty { path: String },
    #[error("the staging folder {path} already exists; remove it and retry")]
    StagingExists { path: String },
    #[error("the port has no launchable executable in {folder}")]
    MissingExecutable { folder: String },
    #[error("the executable's identity could not be read: {0}")]
    Identity(String),
    #[error("the install files could not be written: {0}")]
    Io(String),
    #[error("the install was cancelled")]
    Cancelled,
    #[error("the port install job lost its place in the pipeline: {0}")]
    Internal(String),
    #[error("the local database could not be used: {0}")]
    Storage(#[from] StorageError),
}

impl InstallError {
    fn is_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Cancelled
                | Self::Supply(SupplyError::Cancelled)
                | Self::Place(PlaceError::Cancelled)
        )
    }
}

fn supply_error(error: SupplyError) -> InstallError {
    if matches!(error, SupplyError::Cancelled) {
        InstallError::Cancelled
    } else {
        InstallError::Supply(error)
    }
}

fn place_error(error: PlaceError) -> InstallError {
    if error.is_cancelled() {
        InstallError::Cancelled
    } else {
        InstallError::Place(error)
    }
}

/// One queued job and its cancellation flag.
struct QueuedJob {
    request: InstallJobRequest,
    cancel: Arc<AtomicBool>,
}

struct Shared {
    queue: Mutex<VecDeque<QueuedJob>>,
    /// Every queued or running job. Removed once the job is terminal.
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Jobs whose files are complete but which wait for the user to pick an executable. They are
    /// not on the queue, so the single worker is free for the next job.
    awaiting: Mutex<HashSet<String>>,
    active: Mutex<Option<String>>,
    quitting: AtomicBool,
    stopped: AtomicBool,
}

enum Command {
    Wake,
    /// Removes the files of a job that waits for an executable pick and ends it `Cancelled`.
    CancelAwaiting {
        job_id: String,
    },
    /// Registers a waiting job with the user's executable pick.
    CompleteAwaiting {
        job_id: String,
        executable: String,
    },
    Stop,
}

/// The install worker. Cheap to share behind an `Arc`; every method takes `&self`.
pub struct JobRunner {
    commands: Sender<Command>,
    store: Arc<dyn PortStore>,
    shared: Arc<Shared>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl JobRunner {
    /// Creates the single install worker thread. The callback runs on that thread.
    pub fn start(config: JobRunnerConfig) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            cancels: Mutex::new(HashMap::new()),
            awaiting: Mutex::new(HashSet::new()),
            active: Mutex::new(None),
            quitting: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
        });
        let (commands, receiver) = mpsc::channel();
        let store = Arc::clone(&config.store);
        let worker_shared = Arc::clone(&shared);
        let join = std::thread::Builder::new()
            .name("ports-install".into())
            .spawn(move || run(receiver, config, worker_shared))
            .expect("the PC ports install worker could not start");
        Self {
            commands,
            store,
            shared,
            join: Mutex::new(Some(join)),
        }
    }

    /// Stores the queued job and posts it to the worker. Blocking (one SQLite write), so call it
    /// from a worker thread. The job runs in queue order, one at a time.
    pub fn enqueue(&self, request: InstallJobRequest) -> Result<(), JobError> {
        validate_request(&request)?;
        if self.shared.stopped.load(Ordering::SeqCst) {
            return Err(JobError::Closed);
        }
        if lock(&self.shared.cancels).contains_key(&request.job_id)
            || lock(&self.shared.awaiting).contains(&request.job_id)
        {
            return Err(JobError::DuplicateJob {
                job_id: request.job_id.clone(),
            });
        }

        let row = queued_row(&request);
        let job_id = request.job_id.clone();
        let resume = PortJobResume {
            plan: request.plan.clone(),
            match_input: request.match_input.clone(),
        };
        self.store.save_job(&row)?;
        // The plan is stored beside the row so a job that waits for the user's executable pick can
        // be registered after a restart.
        self.store.save_job_resume(&job_id, &resume)?;

        let cancel = Arc::new(AtomicBool::new(false));
        lock(&self.shared.cancels).insert(job_id.clone(), Arc::clone(&cancel));
        lock(&self.shared.queue).push_back(QueuedJob { request, cancel });
        if self.commands.send(Command::Wake).is_err() {
            // The worker stopped between the check above and the send: leave an honest terminal
            // row instead of a job that waits forever.
            lock(&self.shared.cancels).remove(&job_id);
            lock(&self.shared.queue).retain(|queued| queued.request.job_id != job_id);
            let mut interrupted = row;
            interrupted.phase = PortJobPhase::Interrupted {
                reason: "the install worker stopped".into(),
            };
            interrupted.error = Some("the install worker stopped".into());
            interrupted.finished_at = Some(interrupted.updated_at);
            let _ = self.store.save_job(&interrupted);
            return Err(JobError::Closed);
        }
        Ok(())
    }

    /// Requests cancellation. Safe from any thread. Returns false for a job that is not queued,
    /// running or waiting for an executable pick (already terminal, or unknown).
    ///
    /// Cancelling a waiting job removes the install folder and journal on the worker thread; the
    /// job ends [`PortJobPhase::Cancelled`] like any other cancellation.
    pub fn cancel(&self, job_id: &str) -> bool {
        {
            let cancels = lock(&self.shared.cancels);
            if let Some(flag) = cancels.get(job_id) {
                flag.store(true, Ordering::SeqCst);
                let _ = self.commands.send(Command::Wake);
                return true;
            }
        }
        if lock(&self.shared.awaiting).contains(job_id) {
            return self
                .commands
                .send(Command::CancelAwaiting {
                    job_id: job_id.to_string(),
                })
                .is_ok();
        }
        false
    }

    /// Registers a job that waits for the user's executable pick.
    ///
    /// `executable` is relative to the job's install folder and must name an existing `.exe` file
    /// inside it; links are resolved and must not escape the folder. Structural problems (an
    /// absolute path, `..`, a non-`.exe` name or a job that is not waiting) are reported at once;
    /// the existence and link checks run on the install worker, which reports a refusal through
    /// [`JobEvent::ExecutableRejected`] and leaves the job waiting for another pick.
    ///
    /// Safe from any thread: the registration, the commit and the notification all happen on the
    /// worker thread, like a normal completion.
    pub fn complete_with_executable(&self, job_id: &str, executable: &str) -> Result<(), JobError> {
        if !valid_job_id(job_id) {
            return Err(JobError::InvalidJobId {
                job_id: job_id.to_string(),
            });
        }
        if !is_safe_relative_path(executable) || !has_exe_extension(executable) {
            return Err(JobError::InvalidExecutable {
                path: executable.to_string(),
            });
        }
        if !lock(&self.shared.awaiting).contains(job_id) {
            return Err(JobError::NotAwaitingExecutable {
                job_id: job_id.to_string(),
            });
        }
        self.commands
            .send(Command::CompleteAwaiting {
                job_id: job_id.to_string(),
                executable: executable.to_string(),
            })
            .map_err(|_| JobError::Closed)
    }

    /// The jobs waiting for the user's executable pick, newest first.
    ///
    /// Call it once at startup (on a worker thread): it also registers the jobs with this runner,
    /// so `cancel` and `complete_with_executable` work across a restart.
    pub fn awaiting_jobs(&self) -> Result<Vec<PortJobRow>, JobError> {
        let awaiting: Vec<PortJobRow> = self
            .store
            .jobs()?
            .into_iter()
            .filter(|job| job.phase == PortJobPhase::AwaitingExecutable)
            .collect();
        lock(&self.shared.awaiting).extend(awaiting.iter().map(|job| job.id.clone()));
        Ok(awaiting)
    }

    /// The job currently running, if any.
    pub fn active_job(&self) -> Option<String> {
        lock(&self.shared.active).clone()
    }

    /// The jobs waiting behind the running one, in queue order.
    pub fn queued_jobs(&self) -> Vec<String> {
        lock(&self.shared.queue)
            .iter()
            .map(|job| job.request.job_id.clone())
            .collect()
    }

    /// True while the job is queued, running or waiting for an executable pick. The app uses this
    /// to refuse cache clearing while an install would use the cache (task 8.1).
    pub fn is_active(&self, job_id: &str) -> bool {
        lock(&self.shared.cancels).contains_key(job_id)
            || lock(&self.shared.awaiting).contains(job_id)
    }

    /// Cancels whatever is running and waits for the worker to clean up and stop.
    ///
    /// Blocking: call it from a shutdown path, never from the UI thread or the event callback.
    /// The running job is marked `Interrupted` rather than `Cancelled`, because GameNode is quitting
    /// (design D5: Quit is recovered as interrupted).
    pub fn shutdown(&self) {
        self.shared.quitting.store(true, Ordering::SeqCst);
        self.shared.stopped.store(true, Ordering::SeqCst);
        for flag in lock(&self.shared.cancels).values() {
            flag.store(true, Ordering::SeqCst);
        }
        let _ = self.commands.send(Command::Stop);
        if let Some(join) = lock(&self.join).take() {
            let _ = join.join();
        }
    }
}

impl Drop for JobRunner {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        let _ = self.commands.send(Command::Stop);
    }
}

fn validate_request(request: &InstallJobRequest) -> Result<(), JobError> {
    if !valid_job_id(&request.job_id) {
        return Err(JobError::InvalidJobId {
            job_id: request.job_id.clone(),
        });
    }
    let expected_staging = format!(".gamenode-staging-{}", request.job_id);
    if !request
        .plan
        .staging
        .replace('/', "\\")
        .to_ascii_lowercase()
        .ends_with(&expected_staging.to_ascii_lowercase())
    {
        return Err(JobError::StagingMismatch {
            job_id: request.job_id.clone(),
        });
    }
    for slot in &request.plan.slots {
        if slot.source.is_some()
            && !request
                .setup_files
                .iter()
                .any(|setup_file| setup_file.id == slot.id)
        {
            return Err(JobError::MissingSlot {
                slot: slot.id.clone(),
            });
        }
    }
    if let Some(executable) = &request.executable
        && !is_safe_relative_path(executable)
    {
        return Err(JobError::InvalidExecutable {
            path: executable.clone(),
        });
    }
    Ok(())
}

fn queued_row(request: &InstallJobRequest) -> PortJobRow {
    let plan = &request.plan;
    PortJobRow {
        id: request.job_id.clone(),
        catalog_id: plan.catalog_id.clone(),
        slug: plan.slug.clone(),
        port_name: plan.title.clone(),
        release_version: Some(plan.release.version.clone()),
        install_folder: Some(plan.target.clone()),
        phase: PortJobPhase::Queued,
        created_paths: Vec::new(),
        registration_id: None,
        error: None,
        failure_output: None,
        seen: false,
        created_at: request.created_at,
        updated_at: request.created_at,
        finished_at: None,
    }
}

fn run(receiver: Receiver<Command>, config: JobRunnerConfig, shared: Arc<Shared>) {
    // A `Stop` or a closed channel ends the loop; `Wake` drains the queue and the awaiting
    // commands act on jobs that are not on it.
    loop {
        match receiver.recv() {
            Ok(Command::Wake) => loop {
                // The lock is released before the job runs: the queue stays readable from the app
                // while a large install is in progress.
                let next = lock(&shared.queue).pop_front();
                match next {
                    Some(job) => execute(&config, &shared, job),
                    None => break,
                }
            },
            Ok(Command::CancelAwaiting { job_id }) => cancel_awaiting(&config, &shared, &job_id),
            Ok(Command::CompleteAwaiting { job_id, executable }) => {
                complete_awaiting(&config, &shared, job_id, executable);
            }
            Ok(Command::Stop) | Err(_) => break,
        }
    }
}

fn execute(config: &JobRunnerConfig, shared: &Shared, job: QueuedJob) {
    *lock(&shared.active) = Some(job.request.job_id.clone());
    let cancel = Arc::clone(&job.cancel);
    let mut context = JobContext::new(config, shared, job);
    context.run();
    let awaiting = context.job.phase == PortJobPhase::AwaitingExecutable;
    let job_id = context.job.id.clone();
    *lock(&shared.active) = None;
    lock(&shared.cancels).remove(&job_id);
    if awaiting {
        if cancel.load(Ordering::SeqCst) {
            // A cancel that arrived as the job entered the waiting phase still removes its files.
            cancel_awaiting(config, shared, &job_id);
        } else {
            // The worker is free again; the job keeps its files and waits for the user's pick.
            lock(&shared.awaiting).insert(job_id);
        }
    }
}

/// What must be copied to a user-data root during Finalizing (design D5).
struct DeferredSlot {
    slot: PlannedSlot,
    checked: CheckedChoice,
}

struct JobContext<'a> {
    config: &'a JobRunnerConfig,
    shared: &'a Shared,
    request: InstallJobRequest,
    cancel: Arc<AtomicBool>,
    job: PortJobRow,
    /// Tools already prepared during this job, by extractor id.
    tools: HashMap<String, InstalledTool>,
}

impl<'a> JobContext<'a> {
    fn new(config: &'a JobRunnerConfig, shared: &'a Shared, job: QueuedJob) -> Self {
        let row = queued_row(&job.request);
        Self::from_row(config, shared, job.request, job.cancel, row)
    }

    /// A context for a job whose row already exists, such as one resumed after a restart.
    fn from_row(
        config: &'a JobRunnerConfig,
        shared: &'a Shared,
        request: InstallJobRequest,
        cancel: Arc<AtomicBool>,
        job: PortJobRow,
    ) -> Self {
        Self {
            config,
            shared,
            request,
            cancel,
            job,
            tools: HashMap::new(),
        }
    }

    fn now(&self) -> i64 {
        self.config.clock.now_millis()
    }

    fn emit(&self, event: JobEvent) {
        (self.config.on_event)(event);
    }

    fn emit_phase(&self) {
        self.emit(JobEvent::PhaseChanged {
            job: self.job.clone(),
        });
    }

    fn emit_progress(&self, slot: Option<&str>, work: WorkProgress) {
        self.emit(JobEvent::Progress(JobProgress {
            job_id: self.job.id.clone(),
            phase: self.job.phase.clone(),
            slot: slot.map(str::to_string),
            done: work.done,
            total: work.total,
        }));
    }

    /// Saves a new phase and reports it. Illegal transitions are prevented by the core state
    /// machine, so one is a runner bug, not a user-facing failure.
    fn set_phase(&mut self, phase: PortJobPhase) -> Result<(), InstallError> {
        self.job.phase = self
            .job
            .phase
            .transition(phase)
            .map_err(|error| InstallError::Internal(error.to_string()))?;
        self.job.updated_at = self.now();
        self.config.store.save_job(&self.job)?;
        self.emit_phase();
        Ok(())
    }

    fn journal(&self, path: &Path) -> Result<(), InstallError> {
        self.config
            .store
            .record_created_path(&self.job.id, &path.display().to_string())?;
        Ok(())
    }

    fn run(&mut self) {
        // The row was stored by `enqueue`; report the queued phase from the worker so the app's
        // callback only ever runs on one thread.
        self.emit_phase();
        if self.shared.quitting.load(Ordering::SeqCst) || self.cancel.load(Ordering::SeqCst) {
            self.finish_cancelled();
            return;
        }
        match self.install() {
            Ok(Some((registration, installation))) => {
                self.finish_completed(registration, installation)
            }
            // No preferred executable exists: keep the finalized folder and let the user pick.
            // Nothing is terminal yet, so no `Finished` event is emitted and the worker is free.
            Ok(None) => {}
            Err(error) if error.is_cancelled() => self.finish_cancelled(),
            Err(error) => self.finish_failed(error),
        }
    }

    /// Runs the pipeline. `Ok(None)` means the files are final but no preferred executable exists,
    /// so the job waits for the user's pick instead of registering.
    fn install(&mut self) -> Result<Option<(Registration, PortInstallation)>, InstallError> {
        // The plan was resolved before queueing (planning is pure and fails before a job exists);
        // the phase still moves through Resolving because the pipeline has no Queued -> Downloading
        // step, and the UI shows the plan being applied.
        self.set_phase(PortJobPhase::Resolving)?;
        let cached = self.download()?;
        self.prepare_staging(&cached)?;
        let deferred = self.place_supplied()?;
        self.finalize(deferred)?;
        if self.find_executable().is_none() {
            self.set_phase(PortJobPhase::AwaitingExecutable)?;
            return Ok(None);
        }
        self.register().map(Some)
    }

    fn download(&mut self) -> Result<CachedDownload, InstallError> {
        self.set_phase(PortJobPhase::Downloading)?;
        self.check_cancelled()?;
        let plan = &self.request.plan;
        let cache_dir = ports_asset_cache_dir(
            &self.config.data_dir,
            &plan.catalog_id,
            &plan.release.version,
        );
        let request = CachedDownloadRequest {
            url: &plan.asset.url,
            cache_dir: &cache_dir,
            file_name: &plan.asset.filename,
            expected_sha256: Some(&plan.asset.sha256),
        };
        self.emit_progress(
            None,
            WorkProgress {
                done: 0,
                total: plan.asset.size,
            },
        );
        let cached = ensure_cached_download(
            self.config.fetcher.as_ref(),
            self.config.url_policy.as_ref(),
            &request,
            None,
        )
        .map_err(|error| InstallError::Download(error.to_string()))?;

        // A cancelled job still keeps the verified download: the next attempt reuses it.
        self.check_cancelled()?;
        self.set_phase(PortJobPhase::Verifying)?;
        self.emit_progress(
            None,
            WorkProgress {
                done: cached.size_bytes,
                total: cached.size_bytes,
            },
        );
        self.check_cancelled()?;
        Ok(cached)
    }

    fn prepare_staging(&mut self, cached: &CachedDownload) -> Result<(), InstallError> {
        self.set_phase(PortJobPhase::Extracting)?;
        let plan = &self.request.plan;
        let staging = Path::new(&plan.staging);
        self.journal(staging)?;
        if staging.exists() {
            return Err(InstallError::StagingExists {
                path: plan.staging.clone(),
            });
        }
        fs::create_dir_all(staging)
            .map_err(|error| InstallError::Io(format!("{}: {error}", plan.staging)))?;

        self.emit_progress(
            None,
            WorkProgress {
                done: 0,
                total: plan.asset.size,
            },
        );
        crate::archive::extract_asset_to_dir(
            &cached.path,
            staging,
            PORTS_MAX_TOTAL_EXTRACTED_BYTES,
        )
        .map_err(|error| InstallError::Extract(error.to_string()))?;
        self.check_cancelled()?;
        self.emit_progress(
            None,
            WorkProgress {
                done: plan.asset.size,
                total: plan.asset.size,
            },
        );

        for relative in &plan.files_to_add {
            let path = join_relative(&plan.staging, relative);
            let path = PathBuf::from(path);
            if path.exists() {
                continue;
            }
            self.journal(&path)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| InstallError::Io(format!("{}: {error}", parent.display())))?;
            }
            fs::write(&path, b"")
                .map_err(|error| InstallError::Io(format!("{}: {error}", path.display())))?;
        }
        Ok(())
    }

    fn place_supplied(&mut self) -> Result<Vec<DeferredSlot>, InstallError> {
        let mut deferred = Vec::new();
        for slot in self.request.plan.slots.clone() {
            let Some(source) = slot.source.clone() else {
                continue;
            };
            self.check_cancelled()?;
            let setup_file = self
                .request
                .setup_files
                .iter()
                .find(|setup_file| setup_file.id == slot.id)
                .cloned()
                .ok_or_else(|| {
                    InstallError::Internal(format!("setup file {:?} is missing", slot.id))
                })?;

            match source {
                SuppliedSource::Path { path } => {
                    self.set_phase(PortJobPhase::SuppliedFiles {
                        slot: slot.id.clone(),
                        step: gamenode_core::ports::SuppliedStep::Hashing,
                    })?;
                    let cancel = Arc::clone(&self.cancel);
                    let outcome = check_choice(
                        Some(Path::new(&path)),
                        &setup_file,
                        &mut |work| self.emit_progress(Some(&slot.id), work),
                        cancel.as_ref(),
                    )
                    .map_err(supply_error)?;
                    let CheckOutcome::Accepted(checked) = outcome else {
                        // An optional choice that vanished is simply skipped.
                        continue;
                    };
                    self.check_cancelled()?;
                    match &slot.placement {
                        Placement::Copy {
                            root: CopyRoot::App,
                            ..
                        } => {
                            self.set_phase(PortJobPhase::SuppliedFiles {
                                slot: slot.id.clone(),
                                step: gamenode_core::ports::SuppliedStep::Copying,
                            })?;
                            self.place_one(&slot, &checked)?;
                        }
                        Placement::Copy { .. } => deferred.push(DeferredSlot {
                            slot: slot.clone(),
                            checked,
                        }),
                        // Flag, positional, env and unused slots copy nothing: the plan already
                        // contains the arguments and environment values.
                        _ => {}
                    }
                }
                SuppliedSource::Packages { base, update } => {
                    self.set_phase(PortJobPhase::SuppliedFiles {
                        slot: slot.id.clone(),
                        step: gamenode_core::ports::SuppliedStep::ExtractingPackage,
                    })?;
                    let checked =
                        self.extract_package(&slot, &setup_file, &base, update.as_deref())?;
                    self.check_cancelled()?;
                    if matches!(
                        slot.placement,
                        Placement::Copy { root, .. } if root != CopyRoot::App
                    ) {
                        deferred.push(DeferredSlot {
                            slot: slot.clone(),
                            checked,
                        });
                    }
                }
            }
        }
        Ok(deferred)
    }

    fn extract_package(
        &mut self,
        slot: &PlannedSlot,
        setup_file: &SetupFile,
        base: &str,
        update: Option<&str>,
    ) -> Result<CheckedChoice, InstallError> {
        let package = slot.from_package.as_ref().ok_or_else(|| {
            InstallError::Internal(format!("slot {:?} does not accept packages", slot.id))
        })?;
        let extractor = self
            .request
            .plan
            .extractors
            .iter()
            .find(|extractor| extractor.id == package.extractor)
            .cloned()
            .ok_or_else(|| {
                InstallError::Internal(format!("extractor {:?} is not pinned", package.extractor))
            })?;

        let tool = self.install_tool(&extractor)?;
        let output = package_output(&self.request.plan, slot)?;
        let output = PathBuf::from(output);
        self.journal(&output)?;
        if output.exists() {
            return Err(InstallError::Extract(format!(
                "{} already exists",
                output.display()
            )));
        }
        let base = Path::new(base);
        let update = update.map(Path::new);
        if update.is_some() && !template_takes_update(&extractor) {
            self.extract_and_merge(
                &extractor,
                &tool,
                base,
                update.expect("checked"),
                &output,
                &slot.id,
            )?;
        } else {
            self.emit_progress(Some(&slot.id), WorkProgress { done: 0, total: 1 });
            let validator = supplied_folder_validator(setup_file, self.cancel.as_ref());
            let ran = self.run_extractor_once(&extractor, &tool, base, update, &output, &validator);
            ran.map_err(|failure| self.package_failure(failure))?;
        }
        self.check_cancelled()?;

        // The single run validated its output; after a merge this is the only validation and it
        // runs on the merged folder. The same check builds the digests a later copy re-verifies
        // (design D7, D8), and handles a tool that produced nothing usable.
        let cancel = Arc::clone(&self.cancel);
        let checked = check_choice(
            Some(&output),
            setup_file,
            &mut |work| self.emit_progress(Some(&slot.id), work),
            cancel.as_ref(),
        )
        .map_err(supply_error)?;
        match checked {
            CheckOutcome::Accepted(checked) => Ok(checked),
            CheckOutcome::Skipped => Err(InstallError::Extract(
                "the extraction tool produced no folder to check".into(),
            )),
        }
    }

    /// Runs the extractor twice and merges the update tree over the base (design D8).
    ///
    /// A tool that does not take `{update}` extracts one package per run. The base goes straight
    /// into the slot's output folder and the update into a temporary folder beside it, inside the
    /// staging folder and on the same volume. Merging then renames each update file into place, so
    /// a multi-gigabyte base is never copied. The temporary folder is removed on success and on
    /// failure; a failure never leaves a half-merged folder the job treats as complete.
    fn extract_and_merge(
        &mut self,
        extractor: &Extractor,
        tool: &InstalledTool,
        base: &Path,
        update: &Path,
        output: &Path,
        slot_id: &str,
    ) -> Result<(), InstallError> {
        self.emit_progress(Some(slot_id), WorkProgress { done: 0, total: 2 });
        let update_folder = self.update_staging_folder(output, slot_id)?;
        self.journal(&update_folder)?;
        // The intermediate trees are not validated: only the merged folder carries the slot's
        // markers. `check_choice` runs on the merged result after this returns.
        let accept = |_: &Path| Ok::<(), String>(());
        let result = (|| {
            self.run_extractor_once(extractor, tool, base, None, output, &accept)
                .map_err(|failure| self.package_failure(failure))?;
            self.emit_progress(Some(slot_id), WorkProgress { done: 1, total: 2 });
            self.run_extractor_once(extractor, tool, update, None, &update_folder, &accept)
                .map_err(|failure| self.package_failure(failure))?;
            self.emit_progress(Some(slot_id), WorkProgress { done: 2, total: 2 });
            let cancel = CancelFlag::from_shared(Arc::clone(&self.cancel));
            merge_update_tree(&update_folder, output, &cancel).map_err(|error| {
                self.package_failure(ExtractorFailure {
                    error,
                    output: String::new(),
                    created_paths: Vec::new(),
                })
            })
        })();
        let removed = remove_path(&update_folder);
        match result {
            Ok(()) => removed.map_err(InstallError::Io),
            Err(error) => {
                if let Err(remove_error) = removed {
                    tracing::warn!(
                        %remove_error,
                        path = %update_folder.display(),
                        "could not remove the update extraction folder"
                    );
                }
                Err(error)
            }
        }
    }

    /// Runs a prepared tool once. The caller decides whether the output is validated.
    fn run_extractor_once(
        &self,
        extractor: &Extractor,
        tool: &InstalledTool,
        package: &Path,
        update: Option<&Path>,
        output: &Path,
        validator: &dyn OutputFolderValidator,
    ) -> Result<ExtractorRunOutcome, ExtractorFailure> {
        let cancel = CancelFlag::from_shared(Arc::clone(&self.cancel));
        let request = ExtractorRunRequest {
            extractor,
            tool,
            package,
            update,
            output,
            working_directory: Path::new(&self.request.plan.staging),
            cancel: &cancel,
            validate_output: validator,
        };
        self.config.packages.run(request)
    }

    /// A temporary folder for the update tree, inside the staging folder and outside `output`.
    fn update_staging_folder(&self, output: &Path, slot_id: &str) -> Result<PathBuf, InstallError> {
        let staging = Path::new(&self.request.plan.staging);
        let safe_id: String = slot_id
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        let safe_id = if safe_id.is_empty() {
            "slot".to_string()
        } else {
            safe_id
        };
        for attempt in 0..100 {
            let name = if attempt == 0 {
                format!(".gamenode-update-{safe_id}")
            } else {
                format!(".gamenode-update-{safe_id}-{attempt}")
            };
            let candidate = staging.join(name);
            // Keep the two trees non-nested, and never reuse a leftover from an earlier attempt.
            if candidate.starts_with(output) || output.starts_with(&candidate) || candidate.exists()
            {
                continue;
            }
            return Ok(candidate);
        }
        Err(InstallError::Internal(format!(
            "could not name a temporary update folder for slot {slot_id:?}"
        )))
    }

    fn install_tool(&mut self, extractor: &Extractor) -> Result<InstalledTool, InstallError> {
        if let Some(tool) = self.tools.get(&extractor.id) {
            return Ok(tool.clone());
        }
        self.check_cancelled()?;
        let cancel_flag = CancelFlag::from_shared(Arc::clone(&self.cancel));
        let tool = self
            .config
            .packages
            .install_tool(extractor, &cancel_flag)
            .map_err(|failure| self.package_failure(failure))?;
        self.tools.insert(extractor.id.clone(), tool.clone());
        Ok(tool)
    }

    fn package_failure(&mut self, failure: ExtractorFailure) -> InstallError {
        if !failure.output.is_empty() {
            self.job.failure_output = Some(failure.output.clone());
        }
        if failure.error.is_cancelled() {
            InstallError::Cancelled
        } else {
            InstallError::Extract(failure.reason())
        }
    }

    fn place_one(
        &mut self,
        slot: &PlannedSlot,
        checked: &CheckedChoice,
    ) -> Result<(), InstallError> {
        let roots = self
            .config
            .user_roots
            .copy_roots(PathBuf::from(&self.request.plan.staging));
        let store = Arc::clone(&self.config.store);
        let job_id = self.job.id.clone();
        let mut journal = move |path: &Path| {
            store
                .record_created_path(&job_id, &path.display().to_string())
                .map_err(|error| PlaceError::Verify(error.to_string()))
        };
        let cancel = Arc::clone(&self.cancel);
        let mut progress = |work| self.emit_progress(Some(&slot.id), work);
        let outcome = place_slots(
            &[SlotChoice { slot, checked }],
            &roots,
            &mut journal,
            cancel.as_ref(),
            &mut progress,
        )
        .map_err(place_error)?;
        // The plan already carries this slot's arguments and environment values; the placement
        // outcome only repeats them.
        let _ = outcome;
        Ok(())
    }

    fn finalize(&mut self, deferred: Vec<DeferredSlot>) -> Result<(), InstallError> {
        self.set_phase(PortJobPhase::Finalizing)?;

        // External-root copies are written during Finalizing, through temporary names (design D5).
        for deferred in deferred {
            self.check_cancelled()?;
            self.place_one(&deferred.slot, &deferred.checked)?;
        }

        let plan = &self.request.plan;
        let staging = PathBuf::from(&plan.staging);
        let target = PathBuf::from(&plan.target);
        self.journal(&target)?;
        if target.exists() {
            let empty = fs::read_dir(&target)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if !empty {
                return Err(InstallError::TargetNotEmpty {
                    path: plan.target.clone(),
                });
            }
            fs::remove_dir(&target)
                .map_err(|error| InstallError::Io(format!("{}: {error}", target.display())))?;
        }
        fs::rename(&staging, &target).map_err(|error| {
            InstallError::Io(format!(
                "{} -> {}: {error}",
                staging.display(),
                target.display()
            ))
        })?;
        self.emit_progress(None, WorkProgress { done: 1, total: 1 });
        Ok(())
    }

    fn register(&mut self) -> Result<(Registration, PortInstallation), InstallError> {
        self.set_phase(PortJobPhase::Registering)?;
        self.check_cancelled()?;
        let (_, absolute) = self.resolve_executable()?;
        self.check_cancelled()?;
        let built = self.registration_for(&absolute)?;
        // The identity read may block; a cancel requested while it ran still wins.
        self.check_cancelled()?;
        Ok(built)
    }

    /// Registers a job that waited for the user's pick. The executable is already validated and
    /// absolute. A failure before the commit leaves the job `AwaitingExecutable` and its files in
    /// place, so the user can pick again.
    fn resume_with_executable(&mut self, absolute: &str) {
        if self.shared.quitting.load(Ordering::SeqCst) {
            return;
        }
        if self.cancel.load(Ordering::SeqCst) {
            self.finish_cancelled();
            return;
        }
        let (registration, installation) = match self.registration_for(absolute) {
            Ok(built) => built,
            Err(error) => {
                self.reject_executable(error.to_string());
                return;
            }
        };
        if self.cancel.load(Ordering::SeqCst) {
            self.finish_cancelled();
            return;
        }
        if let Err(error) = self.set_phase(PortJobPhase::Registering) {
            self.reject_executable(error.to_string());
            return;
        }
        if let Err(error) = self.check_cancelled() {
            if error.is_cancelled() {
                self.finish_cancelled();
            } else {
                self.reject_executable(error.to_string());
            }
            return;
        }
        self.finish_completed(registration, installation);
    }

    /// Builds the registration and the provenance row for an executable that exists.
    fn registration_for(
        &self,
        absolute: &str,
    ) -> Result<(Registration, PortInstallation), InstallError> {
        let file_identity = self
            .config
            .identity
            .identity(Path::new(absolute))
            .map_err(InstallError::Identity)?;
        let plan = &self.request.plan;
        let registration = plan.registration.to_registration(
            session_id(),
            absolute.to_string(),
            file_identity,
            true,
        );
        let installation = PortInstallation {
            registration_id: registration.id.clone(),
            catalog_id: plan.catalog_id.clone(),
            slug: plan.slug.clone(),
            port_name: plan.title.clone(),
            installed_version: plan.release.version.clone(),
            install_folder: plan.target.clone(),
            environment: plan.environment.clone(),
            job_id: self.job.id.clone(),
        };
        Ok((registration, installation))
    }

    /// Reports a refused pick. The job row keeps the awaiting phase and its files.
    fn reject_executable(&mut self, reason: String) {
        self.job.updated_at = self.now();
        self.emit(JobEvent::ExecutableRejected {
            job_id: self.job.id.clone(),
            reason,
        });
    }

    fn resolve_executable(&self) -> Result<(String, String), InstallError> {
        self.find_executable()
            .ok_or_else(|| InstallError::MissingExecutable {
                folder: self.request.plan.target.clone(),
            })
    }

    /// The executable the registration launches: the user's pick, else the recipe's launcher, else
    /// any preferred executable the extraction produced. All are relative to the install folder.
    fn find_executable(&self) -> Option<(String, String)> {
        let plan = &self.request.plan;
        if let Some(relative) = &self.request.executable {
            if !is_safe_relative_path(relative) {
                return None;
            }
            let absolute = join_relative(&plan.target, relative);
            if Path::new(&absolute).is_file() {
                return Some((relative.clone(), absolute));
            }
            return None;
        }

        let mut candidates = Vec::new();
        candidates.extend(plan.registration.executable.iter().cloned());
        candidates.extend(plan.registration.launchers.iter().cloned());
        for relative in candidates {
            if !is_safe_relative_path(&relative) {
                continue;
            }
            let absolute = join_relative(&plan.target, &relative);
            if Path::new(&absolute).is_file() {
                return Some((relative, absolute));
            }
        }
        None
    }

    fn check_cancelled(&self) -> Result<(), InstallError> {
        if self.cancel.load(Ordering::SeqCst) {
            Err(InstallError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn finish_completed(&mut self, registration: Registration, installation: PortInstallation) {
        let now = self.now();
        let mut completed = self.job.clone();
        let phase = match completed.phase.transition(PortJobPhase::Completed) {
            Ok(phase) => phase,
            Err(error) => {
                self.finish_failed(InstallError::Internal(error.to_string()));
                return;
            }
        };
        completed.phase = phase;
        completed.registration_id = Some(registration.id.clone());
        completed.updated_at = now;
        completed.finished_at = Some(now);
        completed.error = None;
        if let Err(error) =
            self.config
                .store
                .commit_install(&completed, &registration, &installation)
        {
            self.finish_failed(InstallError::Storage(error));
            return;
        }
        self.job = completed.clone();
        self.emit(JobEvent::Finished {
            job: completed,
            completion: Some(Box::new(InstallCompletion {
                job_id: self.job.id.clone(),
                registration,
                installation,
                match_input: self.request.match_input.clone(),
            })),
        });
    }

    fn finish_failed(&mut self, error: InstallError) {
        let now = self.now();
        let reason = error.to_string();
        let failure_output = self.job.failure_output.clone();
        let cleanup = self.cleanup();
        let mut job = self.job.clone();
        job.phase = match job.phase.transition(PortJobPhase::Failed {
            reason: reason.clone(),
        }) {
            Ok(phase) => phase,
            Err(transition) => PortJobPhase::Failed {
                reason: transition.to_string(),
            },
        };
        job.error = Some(match cleanup {
            Some(warning) => format!("{reason} ({warning})"),
            None => reason,
        });
        job.failure_output = failure_output;
        job.updated_at = now;
        job.finished_at = Some(now);
        if let Err(error) = self.config.store.save_job(&job) {
            tracing::warn!(%error, job_id = %job.id, "could not save a failed port install job");
        }
        self.job = job.clone();
        self.emit(JobEvent::Finished {
            job,
            completion: None,
        });
    }

    fn finish_cancelled(&mut self) {
        let now = self.now();
        let quitting = self.shared.quitting.load(Ordering::SeqCst);

        if !quitting && let Ok(cancelling) = self.job.phase.transition(PortJobPhase::Cancelling) {
            self.job.phase = cancelling;
            self.job.updated_at = now;
            if let Err(error) = self.config.store.save_job(&self.job) {
                tracing::warn!(%error, job_id = %self.job.id, "could not save a cancelling port install job");
            }
            self.emit_phase();
        }

        let cleanup = self.cleanup();
        let mut job = self.job.clone();
        job.phase = if quitting {
            PortJobPhase::Interrupted {
                reason: "GameNode quit before the install finished".into(),
            }
        } else {
            PortJobPhase::Cancelled
        };
        job.error = match &job.phase {
            // Quitting keeps a reason so the entry shows as retryable after the restart.
            PortJobPhase::Interrupted { reason } => Some(match cleanup {
                Some(warning) => format!("{reason} ({warning})"),
                None => reason.clone(),
            }),
            _ => cleanup,
        };
        job.finished_at = Some(now);
        job.updated_at = now;
        if let Err(error) = self.config.store.save_job(&job) {
            tracing::warn!(%error, job_id = %job.id, "could not save a cancelled port install job");
        }
        self.job = job.clone();
        self.emit(JobEvent::Finished {
            job,
            completion: None,
        });
    }

    /// Removes every path the job journaled, newest first, plus its staging folder. Verified cached
    /// downloads are never journaled and therefore survive (design D5, D6).
    fn cleanup(&self) -> Option<String> {
        cleanup_job(self.config.store.as_ref(), &self.job)
    }
}

/// Removes every path the job journaled, newest first, plus its staging folder. Verified cached
/// downloads are never journaled and therefore survive (design D5, D6).
fn cleanup_job(store: &dyn PortStore, job: &PortJobRow) -> Option<String> {
    let mut warnings = Vec::new();
    let paths = match store.created_paths(&job.id) {
        Ok(paths) => paths,
        Err(error) => {
            warnings.push(format!("the job journal could not be read: {error}"));
            Vec::new()
        }
    };
    for path in paths.iter().rev() {
        match remove_path(Path::new(path)) {
            Ok(()) => {}
            Err(error) => warnings.push(error),
        }
    }

    // The staging folder is always journaled before it is created; removing it again is a safety
    // net for a journal write that was committed but lost its final flush.
    if let Some(folder) = &job.install_folder
        && let Some(staging) = staging_folder(folder, &job.id)
        && let Err(error) = remove_path(Path::new(&staging))
    {
        warnings.push(error);
    }

    (!warnings.is_empty()).then(|| warnings.join("; "))
}

/// Ends a job that waits for an executable pick: removes its files and marks it `Cancelled`.
fn cancel_awaiting(config: &JobRunnerConfig, shared: &Shared, job_id: &str) {
    let job = match config.store.job(job_id) {
        Ok(Some(job)) => job,
        Ok(None) => {
            lock(&shared.awaiting).remove(job_id);
            return;
        }
        Err(error) => {
            tracing::warn!(%error, job_id, "could not read a waiting port install job");
            lock(&shared.awaiting).remove(job_id);
            return;
        }
    };
    if job.phase != PortJobPhase::AwaitingExecutable {
        lock(&shared.awaiting).remove(job_id);
        return;
    }
    lock(&shared.awaiting).remove(job_id);

    let now = config.clock.now_millis();
    let mut job = job;
    if let Ok(cancelling) = job.phase.transition(PortJobPhase::Cancelling) {
        job.phase = cancelling;
        job.updated_at = now;
        if let Err(error) = config.store.save_job(&job) {
            tracing::warn!(%error, job_id, "could not save a cancelling port install job");
        }
        (config.on_event)(JobEvent::PhaseChanged { job: job.clone() });
    }

    let cleanup = cleanup_job(config.store.as_ref(), &job);
    job.phase = PortJobPhase::Cancelled;
    job.error = cleanup;
    job.updated_at = now;
    job.finished_at = Some(now);
    if let Err(error) = config.store.save_job(&job) {
        tracing::warn!(%error, job_id, "could not save a cancelled port install job");
    }
    (config.on_event)(JobEvent::Finished {
        job,
        completion: None,
    });
}

/// Registers a waiting job with the user's executable pick. Every check and the commit run here,
/// on the worker thread, so the caller never touches the disk.
fn complete_awaiting(
    config: &JobRunnerConfig,
    shared: &Shared,
    job_id: String,
    executable: String,
) {
    let job = match config.store.job(&job_id) {
        Ok(Some(job)) if job.phase == PortJobPhase::AwaitingExecutable => job,
        Ok(_) => {
            lock(&shared.awaiting).remove(&job_id);
            return;
        }
        Err(error) => {
            (config.on_event)(JobEvent::ExecutableRejected {
                job_id,
                reason: format!("the install job could not be read: {error}"),
            });
            return;
        }
    };
    let Some(folder) = job.install_folder.clone() else {
        (config.on_event)(JobEvent::ExecutableRejected {
            job_id,
            reason: "the job has no install folder".into(),
        });
        return;
    };
    let absolute = match validate_picked_executable(&folder, &executable) {
        Ok(absolute) => absolute,
        Err(reason) => {
            (config.on_event)(JobEvent::ExecutableRejected { job_id, reason });
            return;
        }
    };
    let resume = match config.store.job_resume(&job_id) {
        Ok(Some(resume)) => resume,
        Ok(None) => {
            (config.on_event)(JobEvent::ExecutableRejected {
                job_id,
                reason: "the install plan is no longer stored; install the port again".into(),
            });
            return;
        }
        Err(error) => {
            (config.on_event)(JobEvent::ExecutableRejected {
                job_id,
                reason: format!("the install plan could not be read: {error}"),
            });
            return;
        }
    };

    // The job leaves the waiting set while it registers, so a cancel during the commit reaches the
    // same flag path as a running install.
    lock(&shared.awaiting).remove(&job_id);
    let request = InstallJobRequest {
        job_id: job_id.clone(),
        plan: resume.plan,
        setup_files: Vec::new(),
        match_input: resume.match_input,
        executable: Some(executable),
        created_at: job.created_at,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    lock(&shared.cancels).insert(job_id.clone(), Arc::clone(&cancel));
    *lock(&shared.active) = Some(job_id.clone());
    let mut context = JobContext::from_row(config, shared, request, cancel, job);
    context.resume_with_executable(&absolute);
    let awaiting = context.job.phase == PortJobPhase::AwaitingExecutable;
    *lock(&shared.active) = None;
    lock(&shared.cancels).remove(&job_id);
    if awaiting {
        lock(&shared.awaiting).insert(job_id);
    }
}

/// Validates a user-picked executable inside a finished install folder.
///
/// The pick must be a relative `.exe` path with no drive prefix or `..`, must exist, and its links
/// must resolve inside the install folder. It reuses the archive entry validator, so an install
/// pick cannot reach anywhere an archive entry could not. Returns the absolute path to register.
fn validate_picked_executable(install_folder: &str, relative: &str) -> Result<String, String> {
    if !is_safe_relative_path(relative) || !has_exe_extension(relative) {
        return Err(format!(
            "{relative:?} is not a Windows executable inside the install folder"
        ));
    }
    let entry = crate::archive::validate_archive_entry_path(relative)
        .map_err(|error| format!("{relative:?} is not a safe path: {error}"))?;
    if entry.as_os_str().is_empty() {
        return Err("choose an executable file".into());
    }

    let folder = Path::new(install_folder);
    let canonical_folder = folder
        .canonicalize()
        .map_err(|error| format!("the install folder could not be read: {error}"))?;
    let canonical_candidate = canonical_folder
        .join(&entry)
        .canonicalize()
        .map_err(|error| format!("{relative:?} could not be opened: {error}"))?;
    if !canonical_candidate.starts_with(&canonical_folder) {
        return Err(format!("{relative:?} points outside the install folder"));
    }
    if !canonical_candidate.is_file() {
        return Err(format!("{relative:?} is not a file"));
    }

    // Register the normal install-folder spelling; only the checks used the canonical path, which
    // starts with `\\?\` on Windows.
    Ok(folder.join(&entry).display().to_string())
}

/// True when the last component of a safe relative path names a `.exe` file.
fn has_exe_extension(path: &str) -> bool {
    let Some(name) = path.rsplit(['/', '\\']).next() else {
        return false;
    };
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty() && extension.eq_ignore_ascii_case("exe")
}

/// The absolute path of a package extraction output while the job runs.
///
/// A copy into the port's own folder is extracted straight to its destination inside the staging
/// folder, so no multi-gigabyte copy is made. Every other placement extracts to a slot folder that
/// Finalizing moves into place with the rest of the staging folder.
fn package_output(plan: &PortInstallPlan, slot: &PlannedSlot) -> Result<String, InstallError> {
    let target = match &slot.placement {
        Placement::Copy {
            root: CopyRoot::App,
            ..
        } => slot.target.clone().ok_or_else(|| {
            InstallError::Internal(format!("slot {:?} has no destination", slot.id))
        })?,
        _ => {
            return Ok(join_folder(
                &plan.staging,
                &format!(".gamenode-slot-{}", slot.id),
            ));
        }
    };
    let relative = relative_to(&target, &plan.target).ok_or_else(|| {
        InstallError::Internal(format!("slot {:?} is outside the install folder", slot.id))
    })?;
    Ok(join_relative(&plan.staging, relative))
}

/// The part of `path` below `base`, comparing the way Windows does.
fn relative_to<'a>(path: &'a str, base: &str) -> Option<&'a str> {
    let base = base
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    let candidate = path.replace('/', "\\");
    let prefix = candidate.as_bytes().get(..base.len())?;
    if !prefix.eq_ignore_ascii_case(base.as_bytes()) {
        return None;
    }
    if !matches!(candidate.as_bytes().get(base.len()), Some(b'\\' | b'/')) {
        return None;
    }
    path.get(base.len() + 1..)
        .map(|relative| relative.trim_start_matches(['/', '\\']))
}

/// The staging folder name the planner gives a job (design D5).
fn staging_folder(install_folder: &str, job_id: &str) -> Option<String> {
    let trimmed = install_folder.trim_end_matches(['/', '\\']);
    let index = trimmed.rfind(['/', '\\'])?;
    let parent = &trimmed[..index];
    if parent.is_empty() {
        return Some(format!("/.gamenode-staging-{job_id}"));
    }
    let parent = if parent.len() == 2 && parent.ends_with(':') {
        // A drive root such as `D:` needs its separator back.
        let separator = if install_folder.as_bytes().get(index) == Some(&b'\\') {
            '\\'
        } else {
            '/'
        };
        return Some(format!("{parent}{separator}.gamenode-staging-{job_id}"));
    } else {
        parent
    };
    Some(join_folder(parent, &format!(".gamenode-staging-{job_id}")))
}

/// Removes one journaled path. Paths that are already gone are not an error, and a path that could
/// escape its folder is refused (the journal is ours, but a damaged row must never delete more).
fn remove_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "refused to remove the relative path {}",
            path.display()
        ));
    }
    use std::path::Component;
    let components: Vec<Component<'_>> = path.components().collect();
    if components
        .iter()
        .any(|component| matches!(component, Component::ParentDir))
        || !components
            .iter()
            .any(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "refused to remove the unsafe path {}",
            path.display()
        ));
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let result = if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests;
