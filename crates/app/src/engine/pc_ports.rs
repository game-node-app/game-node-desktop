//! PC ports app wiring: the catalog worker, the install runner and their snapshot projection.
//!
//! The service is built once at startup and shared behind an `Arc`. Every blocking step (overlay
//! load, startup recovery, SQLite reads, release fetches and native pickers) runs on a worker
//! thread; the Tauri commands that call into it are `async` and await those workers, so the WebView
//! and the main thread never wait on the network, a disk or a modal dialog.
//!
//! Two channels leave the service:
//!
//! - [`PortsEvent::Changed`] carries a fully built [`PortsView`] and an optional install
//!   completion. The engine stores the view and publishes one `engine:snapshot`; a completion also
//!   refreshes the local library and runs the D11 matcher.
//! - [`PortsEvent::Progress`] carries one guided-setup or install progress tick. The engine
//!   coalesces these into `ports:progress`; they never republish a snapshot (design D5).
//!
//! Startup order is load-bearing: the overlay is read and interrupted jobs are recovered *before*
//! the install runner starts, so the runner only sees clean state (task 6.1 / design D5).

pub mod view;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use gamenode_core::drm_free::{LocalError, LocalGameStore};
use gamenode_core::ports::plan::{
    InstallPlanRequest, PlanError, SuppliedChoice, SuppliedSource, folder_name, join_folder,
};
use gamenode_core::ports::store::{
    PortJobResume, PortJobRow, PortPreferences, PortStore, PortViewMode,
};
use gamenode_core::ports::view::{
    PortDetailView, PortHelperToolView, PortProgress, PortProgressPhase, PortToolsCacheView,
    PortsView,
};
use gamenode_core::ports::{PortPlatform, StorageError};
use gamenode_platform::download::{DownloadFetcher, DownloadUrlPolicy};
use gamenode_platform::ports::InstallCompletion;
use gamenode_platform::ports::catalog_worker::{CatalogEvent, CatalogSource, CatalogWorker, Clock};
use gamenode_platform::ports::job_runner::{
    ExecutableIdentity, InstallJobRequest, JobError, JobEvent, JobRunner, JobRunnerConfig,
    PackageExtractor, UserDataRoots, mark_job_seen,
};
use gamenode_platform::ports::overlay_source::{OverlayFetcher, OverlaySource};
use gamenode_platform::ports::recovery::recover_interrupted_jobs;
use gamenode_platform::ports::supplied::{CheckOutcome, SupplyError, check_choice};

use crate::engine::Engine;
use crate::engine::pc_ports::view::EntryFacts;

/// The event the UI subscribes to for `PortProgress` ticks.
pub const PORTS_PROGRESS_EVENT: &str = "ports:progress";

/// The host coalesces progress ticks to at most four per second (design D5).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// What the service reports to the engine. The callback runs on a worker thread.
#[derive(Clone, Debug)]
pub enum PortsEvent {
    /// The projection changed (catalog, jobs or preferences). `completion` is present when an
    /// install finished; the engine then refreshes the local library and runs the D11 matcher.
    Changed {
        view: PortsView,
        completion: Option<Box<InstallCompletion>>,
    },
    /// One progress tick. Coalesced into `ports:progress`, never into a snapshot.
    Progress(PortProgress),
    /// The user's executable pick was refused by the runner; the job still waits for one.
    ExecutableRejected { job_id: String, reason: String },
}

/// The install runner operations the service uses. [`JobRunner`] is the production implementation;
/// tests inject a fake to check the planning and wiring without touching a disk or the network.
pub trait InstallRunner: Send + Sync {
    fn enqueue(&self, request: InstallJobRequest) -> Result<(), JobError>;
    fn cancel(&self, job_id: &str) -> bool;
    fn complete_with_executable(&self, job_id: &str, executable: &str) -> Result<(), JobError>;
    fn awaiting_jobs(&self) -> Result<Vec<PortJobRow>, JobError>;
    fn active_job(&self) -> Option<String>;
    fn queued_jobs(&self) -> Vec<String>;
    /// True while the job is queued, running or waiting for an executable pick.
    fn is_active(&self, job_id: &str) -> bool;
    fn shutdown(&self);
}

impl InstallRunner for JobRunner {
    fn enqueue(&self, request: InstallJobRequest) -> Result<(), JobError> {
        JobRunner::enqueue(self, request)
    }
    fn cancel(&self, job_id: &str) -> bool {
        JobRunner::cancel(self, job_id)
    }
    fn complete_with_executable(&self, job_id: &str, executable: &str) -> Result<(), JobError> {
        JobRunner::complete_with_executable(self, job_id, executable)
    }
    fn awaiting_jobs(&self) -> Result<Vec<PortJobRow>, JobError> {
        JobRunner::awaiting_jobs(self)
    }
    fn active_job(&self) -> Option<String> {
        JobRunner::active_job(self)
    }
    fn queued_jobs(&self) -> Vec<String> {
        JobRunner::queued_jobs(self)
    }
    fn is_active(&self, job_id: &str) -> bool {
        JobRunner::is_active(self, job_id)
    }
    fn shutdown(&self) {
        JobRunner::shutdown(self)
    }
}

/// The blocking install dependencies the runner needs.
pub struct InstallDeps {
    pub fetcher: Arc<dyn DownloadFetcher>,
    pub url_policy: Arc<dyn DownloadUrlPolicy>,
    pub user_roots: UserDataRoots,
    pub identity: Arc<dyn ExecutableIdentity>,
    pub packages: Arc<dyn PackageExtractor>,
}

/// Everything the service needs to start.
pub struct PortsServiceConfig {
    pub store: Arc<dyn PortStore>,
    pub local_games: Arc<dyn LocalGameStore>,
    pub catalog: Arc<dyn CatalogSource>,
    pub overlay_fetcher: Arc<dyn OverlayFetcher>,
    pub clock: Arc<dyn Clock>,
    pub data_dir: PathBuf,
    pub platform: PortPlatform,
    /// `None` when this build cannot install ports (Linux, or a test that only reads the view).
    pub install: Option<InstallDeps>,
}

/// Why an install cannot start, in the UI's own vocabulary. The UI translates each code, so a
/// plan failure never reaches the user as a raw English message.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "code"
)]
pub enum PortInstallProblem {
    /// The resolved install folder is not empty.
    TargetNotEmpty { target: String },
    /// The resolved install folder sits inside another library entry.
    TargetInsideLibrary { target: String, root: String },
    /// The target volume has less free space than the install needs.
    NotEnoughSpace { required: u64, available: u64 },
    /// A package slot needs its update package, but only a base package was chosen.
    MissingRequiredUpdate { slot: String, version: String },
    /// The selected release was withdrawn upstream.
    Withdrawn { version: String, reason: String },
    /// The overlay pins a release the catalog no longer lists.
    ReleaseUnavailable { version: String },
    /// The entry has no verified release.
    NoVerifiedRelease,
    /// The release has no Windows asset.
    NoWindowsAsset,
    /// No asset matches the recipe's asset filter.
    NoMatchingAsset,
    /// The selected asset has no file format.
    UnknownAssetFormat { filename: String },
    /// The selected asset format is not installable.
    UnsupportedFormat { format: String },
    /// The asset has no checksum to verify the download against.
    NoChecksum,
    /// A required setup slot has no choice.
    MissingSlot { label: String },
    /// Any other plan failure; the UI shows the general message.
    Other,
}

impl From<PlanError> for PortInstallProblem {
    fn from(error: PlanError) -> Self {
        match error {
            PlanError::TargetNotEmpty { target } => Self::TargetNotEmpty { target },
            PlanError::TargetInsideLibrary { target, root } => {
                Self::TargetInsideLibrary { target, root }
            }
            PlanError::NotEnoughSpace {
                required,
                available,
            } => Self::NotEnoughSpace {
                required,
                available,
            },
            PlanError::MissingRequiredUpdate { slot, version } => {
                Self::MissingRequiredUpdate { slot, version }
            }
            PlanError::Withdrawn { version, reason } => Self::Withdrawn { version, reason },
            PlanError::ReleaseUnavailable { version } => Self::ReleaseUnavailable { version },
            PlanError::NoVerifiedRelease => Self::NoVerifiedRelease,
            PlanError::NoWindowsAsset => Self::NoWindowsAsset,
            PlanError::NoMatchingAsset { .. } => Self::NoMatchingAsset,
            PlanError::UnknownAssetFormat { filename } => Self::UnknownAssetFormat { filename },
            PlanError::UnsupportedFormat { format } => Self::UnsupportedFormat { format },
            PlanError::NoChecksum => Self::NoChecksum,
            PlanError::MissingSlot { label, .. } => Self::MissingSlot { label },
            _ => Self::Other,
        }
    }
}

/// Why a PC ports request failed. Commands turn these into a typed UI error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortsError {
    /// This build cannot install ports (Linux, or a test without the service).
    Unsupported,
    /// The workers are still starting.
    NotReady,
    /// The request names something invalid or no longer valid.
    Invalid(String),
    /// The install cannot start for another reason the host explains (network, cache, picker).
    Unavailable(String),
    /// The install plan refused the request for a reason the UI words itself.
    InstallBlocked(PortInstallProblem),
    /// The native picker failed.
    Picker(String),
    /// Local port state could not be read.
    Storage(String),
    /// A queued, running or executable-awaiting install would use the cache.
    InstallInProgress,
}

impl std::fmt::Display for PortsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => formatter.write_str("PC ports are only supported on Windows"),
            Self::NotReady => formatter.write_str("PC ports is still starting"),
            Self::Invalid(reason) => write!(formatter, "the request is invalid: {reason}"),
            Self::Unavailable(reason) => {
                write!(formatter, "the install could not start: {reason}")
            }
            Self::InstallBlocked(problem) => {
                write!(formatter, "the install is blocked: {problem:?}")
            }
            Self::Picker(reason) => write!(formatter, "the file picker failed: {reason}"),
            Self::Storage(reason) => {
                write!(formatter, "local port state could not be read: {reason}")
            }
            Self::InstallInProgress => {
                formatter.write_str("a port install is running; clear the cache when it finishes")
            }
        }
    }
}

impl std::error::Error for PortsError {}

impl From<StorageError> for PortsError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error.to_string())
    }
}

impl From<LocalError> for PortsError {
    fn from(error: LocalError) -> Self {
        Self::Picker(error.to_string())
    }
}

/// The request `ports_install` accepts; the shape is `PortInstallRequest` in `api/ports.ts`.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInstallRequest {
    pub catalog_id: String,
    /// Parent folder chosen by the user; the host creates a subfolder inside it.
    pub parent_folder: String,
    #[serde(default)]
    pub supplied: Vec<PortSuppliedChoice>,
    /// Confirm an asset that only has an unverified checksum.
    #[serde(default)]
    pub accept_unverified_checksum: bool,
    /// Confirm an asset a malware scan flagged.
    #[serde(default)]
    pub accept_malware_flag: bool,
    /// Reuse the stored plan of a failed, cancelled or interrupted job.
    #[serde(default)]
    pub retry_job_id: Option<String>,
}

/// One accepted selection sent with an install request.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortSuppliedChoice {
    pub slot_id: String,
    pub path: String,
    /// True when the path is a console package for a package-capable slot.
    #[serde(default)]
    pub package: bool,
    /// True when the package is the update package.
    #[serde(default)]
    pub update: bool,
}

/// `ports_pick_supplied` request. `catalog_id` was added to the UI contract so the host can
/// validate the pick against the actual slot; older callers may omit it and only get the picker.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortPickSuppliedRequest {
    pub setup_id: String,
    #[serde(default)]
    pub catalog_id: Option<String>,
    pub slot_id: String,
    pub kind: PortSuppliedPickKind,
    #[serde(default)]
    pub update: bool,
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// The picker a guided-setup slot uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortSuppliedPickKind {
    File,
    Folder,
    Package,
}

/// `accepted` or the rejection the UI shows next to the slot.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PortSuppliedStatus {
    Accepted,
    Rejected {
        message: String,
        accepted_labels: Vec<String>,
        /// True when the picked folder holds console packages and the slot offers the package
        /// option; the UI then points at that option instead of the missing marker.
        packages_found: bool,
    },
}

/// The result of one picker: the chosen path and whether the host accepted it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortSuppliedPick {
    pub slot_id: String,
    pub path: String,
    pub status: PortSuppliedStatus,
}

/// The patch `ports_set_preferences` accepts; only present fields change.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortPreferencesPatch {
    pub view: Option<PortViewMode>,
    pub filters: Option<PortFiltersPatch>,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub last_parent_folder: Option<Option<String>>,
}

/// The filter half of a preferences patch.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortFiltersPatch {
    pub consoles: Option<Vec<String>>,
    pub guided: Option<bool>,
    pub verified: Option<bool>,
    pub installed: Option<bool>,
}

/// Reads `null` as `Some(None)` and a value as `Some(Some(value))`; a missing field stays `None`.
fn deserialize_nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(deserializer).map(Some)
}

struct Inner {
    ready: bool,
    open_requested: bool,
    refresh_requested: bool,
    catalog: Option<Arc<CatalogWorker>>,
    runner: Option<Arc<dyn InstallRunner>>,
    overlay: Option<Arc<gamenode_core::ports::overlay::Overlay>>,
    overlay_refreshing: bool,
    /// Facts learned from the last detail response, by catalog id.
    facts: HashMap<String, EntryFacts>,
}

/// The engine's event sink for the service.
type PortsObserver = Arc<dyn Fn(PortsEvent) + Send + Sync>;

/// The PC ports subsystem. Cheap to clone behind an `Arc`; every method takes `&self`.
pub struct PortsService {
    config: PortsServiceConfig,
    overlay_source: OverlaySource,
    inner: Mutex<Inner>,
    ready: Condvar,
    observer: Mutex<Option<PortsObserver>>,
    /// Serializes view builds and emissions so two worker threads cannot deliver an older
    /// projection after a newer one.
    view_emit: Mutex<()>,
    /// Serializes enqueuing an install with clearing the cache: a clear that starts first cannot
    /// race a job into using files it is deleting, and a job that starts first is visible to the
    /// clear's active check.
    cache_gate: Mutex<()>,
}

impl PortsService {
    /// Starts the service: one worker thread loads the overlay, recovers interrupted jobs and then
    /// starts the catalog worker and install runner. Returns immediately (never blocks the caller).
    pub fn start(config: PortsServiceConfig) -> Arc<Self> {
        let overlay_source = OverlaySource::new(Arc::clone(&config.store));
        let service = Arc::new(Self {
            config,
            overlay_source,
            inner: Mutex::new(Inner {
                ready: false,
                open_requested: false,
                refresh_requested: false,
                catalog: None,
                runner: None,
                overlay: None,
                overlay_refreshing: false,
                facts: HashMap::new(),
            }),
            ready: Condvar::new(),
            observer: Mutex::new(None),
            view_emit: Mutex::new(()),
            cache_gate: Mutex::new(()),
        });
        let starting = Arc::clone(&service);
        std::thread::Builder::new()
            .name("ports-startup".into())
            .spawn(move || starting.initialize())
            .expect("the PC ports startup worker could not start");
        service
    }

    /// Registers the engine's observer. A view already built by a fast startup is delivered on a
    /// worker thread, so this call never reads SQLite on the caller's thread.
    pub fn set_observer(self: &Arc<Self>, observer: PortsObserver) {
        *lock(&self.observer) = Some(observer);
        if self.is_ready() {
            let service = Arc::clone(self);
            std::thread::spawn(move || service.emit_view(None));
        }
    }

    /// The first visit: start the first fetch. Idempotent and non-blocking.
    pub fn open(self: &Arc<Self>) {
        let catalog = {
            let mut inner = lock(&self.inner);
            if !inner.ready {
                inner.open_requested = true;
                return;
            }
            inner.catalog.clone()
        };
        if let Some(catalog) = catalog {
            catalog.open();
            self.request_overlay_refresh();
        }
    }

    /// An on-demand refresh. Coalesced into a running fetch; non-blocking.
    pub fn refresh(self: &Arc<Self>) {
        let catalog = {
            let mut inner = lock(&self.inner);
            if !inner.ready {
                inner.refresh_requested = true;
                return;
            }
            inner.catalog.clone()
        };
        if let Some(catalog) = catalog {
            catalog.refresh();
            self.request_overlay_refresh();
        }
    }

    /// Requests cancellation. Safe from any thread and non-blocking.
    pub fn cancel(&self, job_id: &str) -> bool {
        lock(&self.inner)
            .runner
            .as_ref()
            .is_some_and(|runner| runner.cancel(job_id))
    }

    /// True when this build runs port installs at all.
    pub fn is_supported(&self) -> bool {
        self.config.platform.supports_installs()
    }

    /// Replaces the install runner. Test-only: production builds get the real [`JobRunner`] from
    /// [`Self::initialize`], and a test uses this to simulate a job that owns the cache.
    #[cfg(test)]
    pub(crate) fn set_runner_for_test(&self, runner: Arc<dyn InstallRunner>) {
        lock(&self.inner).runner = Some(runner);
    }

    /// Stops the install runner (cancelling its job) and then the catalog worker. Blocking: call it
    /// from the shutdown path, never from a UI callback.
    pub fn shutdown(&self) {
        let (runner, catalog) = {
            let inner = lock(&self.inner);
            (inner.runner.clone(), inner.catalog.clone())
        };
        if let Some(runner) = runner {
            runner.shutdown();
        }
        if let Some(catalog) = catalog {
            catalog.shutdown();
        }
    }

    /// One entry's lazy detail. Blocking: call it from a worker thread.
    pub fn detail(&self, catalog_id: &str) -> Result<Option<PortDetailView>, PortsError> {
        self.wait_ready();
        let (catalog, overlay) = self.catalog_and_overlay()?;
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.catalog_id == catalog_id)
            .cloned();
        let Some(entry) = entry else {
            return Ok(None);
        };
        let merged_detail = match catalog.detail(&entry.slug) {
            Ok(detail) => overlay.merge((*detail).clone()),
            Err(error) => {
                tracing::debug!(%error, catalog_id, "could not fetch the port entry detail");
                Some(entry.clone())
            }
        };
        let Some(merged_detail) = merged_detail else {
            return Ok(None);
        };
        let releases = match catalog.releases(&entry.slug) {
            Ok(releases) => (*releases).clone(),
            Err(error) => {
                tracing::debug!(%error, catalog_id, "could not fetch the port releases");
                Vec::new()
            }
        };
        let installed_version = self.installed_version(catalog_id)?;
        let (detail, facts) = view::build_detail(
            &merged_detail,
            Some(&merged_detail),
            &releases,
            installed_version.as_deref(),
        );
        {
            let mut inner = lock(&self.inner);
            inner.facts.insert(entry.catalog_id.clone(), facts);
        }
        self.emit_view(None);
        Ok(Some(detail))
    }

    /// Marks every finished result of one catalog entry seen. Blocking.
    pub fn mark_seen(&self, catalog_id: &str) -> Result<(), PortsError> {
        let jobs = self.config.store.jobs()?;
        let now = self.config.clock.now_millis();
        for job in jobs
            .iter()
            .filter(|job| job.catalog_id == catalog_id && job.phase.is_terminal() && !job.seen)
        {
            mark_job_seen(&*self.config.store, &job.id, now)?;
        }
        self.emit_view(None);
        Ok(())
    }

    /// Persists the UI's remembered view, filters and parent folder. Blocking.
    pub fn set_preferences(&self, patch: &PortPreferencesPatch) -> Result<(), PortsError> {
        let mut preferences = self.config.store.preferences()?;
        apply_preferences(&mut preferences, patch);
        self.config.store.save_preferences(&preferences)?;
        self.emit_view(None);
        Ok(())
    }

    /// The Settings Tools panel's PC ports cache block: pinned extraction tools, their cache
    /// usage and the cached release downloads. Blocking; the commands call it on a worker.
    pub fn tools_cache(&self) -> Result<PortToolsCacheView, PortsError> {
        self.wait_ready();
        let overlay = lock(&self.inner).overlay.clone().ok_or_else(|| {
            PortsError::Unavailable("the PC ports overlay is not available".into())
        })?;
        let data_dir = &self.config.data_dir;
        let downloads_bytes = gamenode_platform::ports::ports_downloads_usage(data_dir)
            .map_err(|error| PortsError::Storage(error.to_string()))?;
        let helper_tools = overlay
            .extractor
            .iter()
            .map(|extractor| {
                let cache_bytes = gamenode_platform::ports::ports_tool_usage(
                    data_dir,
                    &extractor.id,
                    &extractor.tag,
                )
                .unwrap_or_else(|error| {
                    tracing::warn!(
                        %error,
                        tool = %extractor.id,
                        "could not measure the extraction tool cache"
                    );
                    0
                });
                PortHelperToolView {
                    id: extractor.id.clone(),
                    name: extractor.name.clone(),
                    license: extractor.license.clone(),
                    source_url: extractor_source_url(&extractor.repository),
                    version: extractor.tag.clone(),
                    cache_bytes,
                }
            })
            .collect();
        Ok(PortToolsCacheView {
            downloads_bytes,
            helper_tools,
            install_active: self.install_is_active()?,
        })
    }

    /// Deletes the cached release downloads and pinned extraction tools. Blocking.
    ///
    /// Nothing an installed port owns is touched: its folder, registration and provenance row
    /// live outside these cache roots. A running, queued or executable-awaiting install may still
    /// need a cached file, so the clear is refused with [`PortsError::InstallInProgress`] while
    /// one exists. The returned view is recomputed after the deletion.
    pub fn clear_cache(&self) -> Result<PortToolsCacheView, PortsError> {
        if !self.is_supported() {
            return Err(PortsError::Unsupported);
        }
        self.wait_ready();
        {
            let _gate = lock(&self.cache_gate);
            if self.install_is_active()? {
                return Err(PortsError::InstallInProgress);
            }
            let freed = gamenode_platform::ports::clear_ports_cache(&self.config.data_dir)
                .map_err(|error| PortsError::Storage(error.to_string()))?;
            tracing::info!(bytes = freed, "cleared the PC ports download cache");
        }
        self.tools_cache()
    }

    /// True while the runner still owns a job that could read a cached file.
    fn install_is_active(&self) -> Result<bool, PortsError> {
        let runner = lock(&self.inner).runner.clone();
        let Some(runner) = runner else {
            return Ok(false);
        };
        if runner.active_job().is_some() || !runner.queued_jobs().is_empty() {
            return Ok(true);
        }
        // A job waiting for the user's executable pick has no queue entry, so ask the store for
        // the ids and let the runner say which it still owns.
        let jobs = self.config.store.jobs()?;
        Ok(jobs.iter().any(|job| runner.is_active(&job.id)))
    }

    /// Opens the native parent-folder picker and remembers a picked folder. Blocking.
    pub fn pick_parent(&self, initial: Option<&str>) -> Result<Option<String>, PortsError> {
        let picked = gamenode_platform::installations::pick_folder_starting_at(initial)?;
        if let Some(folder) = &picked {
            let mut preferences = self.config.store.preferences()?;
            preferences.last_parent = Some(folder.clone());
            self.config.store.save_preferences(&preferences)?;
            self.emit_view(None);
        }
        Ok(picked)
    }

    /// Opens the guided-setup picker and validates the choice against the slot. Blocking.
    pub fn pick_supplied(
        self: &Arc<Self>,
        request: &PortPickSuppliedRequest,
    ) -> Result<Option<PortSuppliedPick>, PortsError> {
        self.wait_ready();
        let path = match request.kind {
            PortSuppliedPickKind::File => {
                gamenode_platform::installations::pick_file_with_extensions(
                    &request.extensions,
                    None,
                )
            }
            PortSuppliedPickKind::Folder => {
                gamenode_platform::installations::pick_folder_starting_at(None)
            }
            PortSuppliedPickKind::Package => {
                gamenode_platform::installations::pick_file_with_extensions(
                    &["pkg".to_owned()],
                    None,
                )
            }
        }?;
        let Some(path) = path else {
            return Ok(None);
        };
        let status = self.check_supplied(request, &path);
        Ok(Some(PortSuppliedPick {
            slot_id: request.slot_id.clone(),
            path,
            status,
        }))
    }

    fn check_supplied(&self, request: &PortPickSuppliedRequest, path: &str) -> PortSuppliedStatus {
        let slot = request
            .catalog_id
            .as_deref()
            .and_then(|catalog_id| self.find_slot(catalog_id, &request.slot_id));
        // A package is validated by running it through the extractor during the install; a pick
        // without a catalog id (an older caller) can only be checked by the picker filters.
        let Some(slot) = slot else {
            return PortSuppliedStatus::Accepted;
        };
        if matches!(request.kind, PortSuppliedPickKind::Package) {
            let phase = PortProgressPhase::ExtractingPackage;
            self.emit_progress(PortProgress {
                job_id: request.setup_id.clone(),
                phase,
                slot_id: Some(request.slot_id.clone()),
                bytes_done: 0,
                bytes_total: 0,
            });
            return PortSuppliedStatus::Accepted;
        }
        let phase = PortProgressPhase::Hashing;
        let mut report = |progress: gamenode_platform::ports::WorkProgress| {
            self.emit_progress(PortProgress {
                job_id: request.setup_id.clone(),
                phase,
                slot_id: Some(request.slot_id.clone()),
                bytes_done: progress.done,
                bytes_total: progress.total,
            });
        };
        let cancelled = AtomicBool::new(false);
        match check_choice(Some(Path::new(path)), &slot, &mut report, &cancelled) {
            Ok(CheckOutcome::Accepted(_)) => PortSuppliedStatus::Accepted,
            Ok(CheckOutcome::Skipped) => PortSuppliedStatus::Rejected {
                message: "choose a file or folder for this slot".to_owned(),
                accepted_labels: accepted_labels(&slot),
                packages_found: false,
            },
            Err(error) => PortSuppliedStatus::Rejected {
                message: error.to_string(),
                accepted_labels: match &error {
                    SupplyError::WrongVersion { accepted, .. } => {
                        let mut labels = accepted_labels(&slot);
                        if labels.is_empty() {
                            labels.push(accepted.clone());
                        }
                        labels
                    }
                    _ => accepted_labels(&slot),
                },
                packages_found: matches!(error, SupplyError::PackagesInFolder { .. }),
            },
        }
    }

    /// Plans and enqueues an install. Blocking: release fetches and the free-space check happen
    /// here, on a worker thread. Returns the new job id.
    pub fn install(&self, request: &PortInstallRequest) -> Result<Option<String>, PortsError> {
        if !self.is_supported() {
            return Err(PortsError::Unsupported);
        }
        let runner = lock(&self.inner).runner.clone();
        let Some(runner) = runner else {
            return Err(PortsError::Unavailable(
                "the install worker is not available".into(),
            ));
        };
        self.wait_ready();
        let prepared = self.prepare_install(request)?;
        {
            // Clearing the cache and queueing an install exclude each other, so a clear cannot
            // start between this job's active check and its cache use.
            let _gate = lock(&self.cache_gate);
            runner
                .enqueue(prepared.clone())
                .map_err(|error| PortsError::Unavailable(error.to_string()))?;
        }
        // Remembering the folder is a convenience; a failed write must not report the queued
        // install as failed.
        match self.config.store.preferences() {
            Ok(mut preferences) => {
                preferences.last_parent = Some(prepared.plan.parent.clone());
                if let Err(error) = self.config.store.save_preferences(&preferences) {
                    tracing::warn!(%error, "could not remember the last install folder");
                }
            }
            Err(error) => tracing::warn!(%error, "could not read the port preferences"),
        }
        tracing::info!(
            job_id = %prepared.job_id,
            catalog_id = %prepared.plan.catalog_id,
            "queued a PC port install"
        );
        Ok(Some(prepared.job_id))
    }

    /// The executable follow-up for a job whose recipe had no launchable file. Blocking.
    pub fn pick_executable(&self, job_id: &str) -> Result<Option<String>, PortsError> {
        if !self.is_supported() {
            return Err(PortsError::Unsupported);
        }
        let runner = lock(&self.inner).runner.clone();
        let Some(runner) = runner else {
            return Err(PortsError::Unavailable(
                "the install worker is not available".into(),
            ));
        };
        let job = self.config.store.job(job_id)?.ok_or_else(|| {
            PortsError::Invalid(format!("the install job {job_id:?} does not exist"))
        })?;
        if job.phase != gamenode_core::ports::job::PortJobPhase::AwaitingExecutable {
            return Err(PortsError::Invalid(
                "the install is not waiting for an executable".into(),
            ));
        }
        let folder = job
            .install_folder
            .clone()
            .ok_or_else(|| PortsError::Invalid("the install has no folder yet".into()))?;
        let Some(picked) = gamenode_platform::installations::pick_executable_in(&folder)? else {
            return Ok(None);
        };
        let relative = relative_inside(&folder, &picked)?;
        runner
            .complete_with_executable(job_id, &relative)
            .map_err(|error| PortsError::Unavailable(error.to_string()))?;
        Ok(Some(picked))
    }

    /// Builds the request the runner executes, including the retry path that reuses a stored plan.
    pub fn prepare_install(
        &self,
        request: &PortInstallRequest,
    ) -> Result<InstallJobRequest, PortsError> {
        if !self.is_supported() {
            return Err(PortsError::Unsupported);
        }
        self.wait_ready();
        let (catalog, overlay) = self.catalog_and_overlay()?;
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.catalog_id == request.catalog_id)
            .cloned()
            .ok_or_else(|| {
                PortsError::Invalid(format!(
                    "the catalog entry {:?} does not exist",
                    request.catalog_id
                ))
            })?;

        if let Some(retry_job_id) = &request.retry_job_id {
            let previous = self.config.store.job(retry_job_id)?.ok_or_else(|| {
                PortsError::Invalid(format!("the install job {retry_job_id:?} does not exist"))
            })?;
            if !previous.phase.is_terminal() {
                return Err(PortsError::Invalid("the install is still running".into()));
            }
            let resume: PortJobResume =
                self.config.store.job_resume(retry_job_id)?.ok_or_else(|| {
                    PortsError::Invalid("the previous install has no stored plan".into())
                })?;
            if resume.plan.catalog_id != request.catalog_id {
                return Err(PortsError::Invalid(
                    "the previous install belongs to another catalog entry".into(),
                ));
            }
            let job_id = gamenode_platform::random::session_id();
            let mut plan = resume.plan;
            plan.staging = join_folder(&plan.parent, &format!(".gamenode-staging-{job_id}"));
            return Ok(InstallJobRequest {
                job_id,
                plan,
                setup_files: entry.launcher.setup_files.clone(),
                match_input: resume.match_input,
                executable: None,
                created_at: self.config.clock.now_millis(),
            });
        }

        // Installing needs the release list, because withdrawn releases are only listed there
        // and a withdrawn release must never be installed (desktop-port-install spec).
        let merged_detail = catalog
            .detail(&entry.slug)
            .map_err(|error| {
                tracing::debug!(%error, catalog_id = %entry.catalog_id, "could not fetch the port detail for install");
                PortsError::Unavailable(
                    "the release details could not be loaded; check your connection and retry"
                        .into(),
                )
            })
            .and_then(|detail| {
                overlay.merge((*detail).clone()).ok_or_else(|| {
                    PortsError::Unavailable("this port is hidden by the overlay".into())
                })
            })?;
        let releases = catalog
            .releases(&entry.slug)
            .map(|releases| (*releases).clone())
            .map_err(|error| {
                tracing::debug!(%error, catalog_id = %entry.catalog_id, "could not fetch the port releases for install");
                PortsError::Unavailable(
                    "the release information could not be loaded; check your connection and retry"
                        .into(),
                )
            })?;

        let supplied = translate_supplied(&request.supplied)?;
        let job_id = gamenode_platform::random::session_id();
        let parent = request.parent_folder.trim();
        let target = join_folder(parent, &folder_name(&merged_detail));
        let target_empty = target_is_empty(&target);
        let free_space =
            gamenode_platform::storage::free_space(Path::new(&target)).ok_or_else(|| {
                PortsError::Unavailable(
                    "the free space of the chosen folder could not be read".into(),
                )
            })?;
        let library_roots = self
            .config
            .local_games
            .registrations()
            .map_err(|error| PortsError::Storage(error.to_string()))?
            .into_iter()
            .filter(|registration| !registration.removed)
            .map(|registration| registration.root)
            .collect();
        let supplied_bytes = supplied_bytes(&supplied);
        let plan_request = InstallPlanRequest {
            entry: merged_detail.clone(),
            releases,
            withdrawn: merged_detail.withdrawn.clone(),
            parent: parent.to_owned(),
            job_id: job_id.clone(),
            target_empty,
            free_space,
            library_roots,
            supplied: supplied.clone(),
            supplied_bytes,
            confirmed_risk: false,
        };
        let plan = match gamenode_core::ports::plan::plan_install(&plan_request) {
            Ok(plan) => plan,
            Err(PlanError::ConfirmationRequired { risks }) => {
                let confirmed = risks.iter().all(|risk| match risk {
                    gamenode_core::ports::plan::InstallRisk::UnverifiedChecksum { .. } => {
                        request.accept_unverified_checksum
                    }
                    gamenode_core::ports::plan::InstallRisk::ScanFlagged { .. } => {
                        request.accept_malware_flag
                    }
                });
                if !confirmed {
                    return Err(PortsError::Unavailable(
                        "the download needs an explicit confirmation".into(),
                    ));
                }
                gamenode_core::ports::plan::plan_install(&InstallPlanRequest {
                    confirmed_risk: true,
                    ..plan_request
                })
                .map_err(|error| PortsError::InstallBlocked(error.into()))?
            }
            Err(error) => return Err(PortsError::InstallBlocked(error.into())),
        };
        Ok(InstallJobRequest {
            job_id,
            setup_files: merged_detail.launcher.setup_files.clone(),
            match_input: gamenode_core::ports::matching::PortMatchInput::from_entry(&merged_detail),
            plan,
            executable: None,
            created_at: self.config.clock.now_millis(),
        })
    }

    fn initialize(self: Arc<Self>) {
        let overlay = match self.overlay_source.current() {
            Ok(active) => Arc::new(active.overlay),
            Err(error) => {
                // The bundled overlay is a build artifact; treat its failure as fatal for the
                // feature but never deadlock the service.
                tracing::error!(%error, "could not load the PC ports overlay");
                match gamenode_core::ports::overlay::Overlay::parse(
                    gamenode_platform::ports::overlay_source::BUNDLED,
                ) {
                    Ok(overlay) => Arc::new(overlay),
                    Err(error) => {
                        tracing::error!(%error, "the bundled PC ports overlay is invalid");
                        self.finish_initialization(None, None, None);
                        return;
                    }
                }
            }
        };

        // Recovery must finish before the runner starts (design D5).
        let local_games = Arc::clone(&self.config.local_games);
        let registration_exists = {
            let local_games = Arc::clone(&local_games);
            move |id: &str| {
                local_games
                    .registrations()
                    .map(|registrations| {
                        registrations
                            .iter()
                            .any(|registration| registration.id == id && !registration.removed)
                    })
                    .map_err(|error| StorageError::Failed(error.to_string()))
            }
        };
        if let Err(error) = recover_interrupted_jobs(
            &*self.config.store,
            &registration_exists,
            self.config.clock.now_millis(),
        ) {
            tracing::warn!(%error, "PC ports startup recovery could not finish");
        }

        let weak = Arc::downgrade(&self);
        let on_catalog: Arc<dyn Fn(CatalogEvent) + Send + Sync> =
            Arc::new(move |event: CatalogEvent| {
                if let Some(service) = weak.upgrade() {
                    service.on_catalog_event(event);
                }
            });
        let catalog = Arc::new(CatalogWorker::start(
            Arc::clone(&self.config.store),
            Arc::clone(&self.config.catalog),
            Arc::clone(&overlay),
            Arc::clone(&self.config.clock),
            on_catalog,
        ));

        let runner: Option<Arc<dyn InstallRunner>> = if self.config.platform.supports_installs() {
            self.config.install.as_ref().map(|deps| {
                let weak = Arc::downgrade(&self);
                let on_event: Arc<dyn Fn(JobEvent) + Send + Sync> = Arc::new(move |event| {
                    if let Some(service) = weak.upgrade() {
                        service.on_job_event(event);
                    }
                });
                let runner = JobRunner::start(JobRunnerConfig {
                    store: Arc::clone(&self.config.store),
                    data_dir: self.config.data_dir.clone(),
                    fetcher: Arc::clone(&deps.fetcher),
                    url_policy: Arc::clone(&deps.url_policy),
                    user_roots: deps.user_roots.clone(),
                    identity: Arc::clone(&deps.identity),
                    packages: Arc::clone(&deps.packages),
                    clock: Arc::clone(&self.config.clock),
                    on_event,
                });
                // Register jobs that were waiting for an executable pick before the last quit.
                if let Err(error) = runner.awaiting_jobs() {
                    tracing::warn!(%error, "could not resume PC port jobs awaiting an executable");
                }
                Arc::new(runner) as Arc<dyn InstallRunner>
            })
        } else {
            None
        };

        self.finish_initialization(Some(catalog), runner, Some(overlay));
    }

    fn finish_initialization(
        self: &Arc<Self>,
        catalog: Option<Arc<CatalogWorker>>,
        runner: Option<Arc<dyn InstallRunner>>,
        overlay: Option<Arc<gamenode_core::ports::overlay::Overlay>>,
    ) {
        let (open, refresh) = {
            let mut inner = lock(&self.inner);
            inner.catalog = catalog.clone();
            inner.runner = runner;
            inner.overlay = overlay;
            inner.ready = true;
            let open = std::mem::take(&mut inner.open_requested);
            let refresh = std::mem::take(&mut inner.refresh_requested);
            (open, refresh)
        };
        self.ready.notify_all();
        if let Some(catalog) = &catalog {
            if open {
                catalog.open();
            }
            if refresh {
                catalog.refresh();
            }
        }
        if open || refresh {
            self.request_overlay_refresh();
        }
        self.emit_view(None);
    }

    fn on_catalog_event(&self, event: CatalogEvent) {
        if matches!(event, CatalogEvent::Loaded(_) | CatalogEvent::Updated(_)) {
            lock(&self.inner).facts.clear();
        }
        self.emit_view(None);
    }

    fn on_job_event(&self, event: JobEvent) {
        match event {
            JobEvent::PhaseChanged { .. } => self.emit_view(None),
            JobEvent::Progress(progress) => {
                if let Some(progress) = view::progress_from_job(progress) {
                    self.emit(PortsEvent::Progress(progress));
                }
            }
            JobEvent::Finished { job, completion } => {
                tracing::info!(job_id = %job.id, phase = %job.phase, "a PC port install finished");
                self.emit_view(completion);
            }
            JobEvent::ExecutableRejected { job_id, reason } => {
                self.emit(PortsEvent::ExecutableRejected { job_id, reason });
            }
        }
    }

    fn emit_progress(&self, progress: PortProgress) {
        self.emit(PortsEvent::Progress(progress));
    }

    fn emit_view(&self, completion: Option<Box<InstallCompletion>>) {
        let _ordered = lock(&self.view_emit);
        let view = match self.build_view() {
            Ok(view) => view,
            Err(error) => {
                tracing::warn!(%error, "could not build the PC ports view");
                return;
            }
        };
        self.emit(PortsEvent::Changed { view, completion });
    }

    fn emit(&self, event: PortsEvent) {
        if let Some(observer) = lock(&self.observer).clone() {
            observer(event);
        }
    }

    fn build_view(&self) -> Result<PortsView, PortsError> {
        let (catalog, facts, active) = {
            let inner = lock(&self.inner);
            (
                inner.catalog.clone(),
                inner.facts.clone(),
                active_jobs(&inner.runner),
            )
        };
        let status = catalog
            .as_ref()
            .map(|catalog| catalog.status())
            .unwrap_or_default();
        let entries = catalog
            .as_ref()
            .map(|catalog| (*catalog.entries()).clone())
            .unwrap_or_default();
        let jobs = self.config.store.jobs()?;
        let installations = self.config.store.installations()?;
        let preferences = self.config.store.preferences()?;
        let live = self
            .config
            .local_games
            .registrations()
            .map_err(|error| PortsError::Storage(error.to_string()))?
            .into_iter()
            .filter(|registration| !registration.removed)
            .map(|registration| registration.id)
            .collect::<HashSet<_>>();
        Ok(view::build(view::ViewInput {
            status,
            entries: &entries,
            jobs: &jobs,
            installations: &installations,
            live_registrations: &live,
            preferences: &preferences,
            facts: &facts,
            active_jobs: &active,
        }))
    }

    fn installed_version(&self, catalog_id: &str) -> Result<Option<String>, PortsError> {
        let live = self
            .config
            .local_games
            .registrations()
            .map_err(|error| PortsError::Storage(error.to_string()))?
            .into_iter()
            .filter(|registration| !registration.removed)
            .map(|registration| registration.id)
            .collect::<HashSet<_>>();
        let jobs = self.config.store.jobs()?;
        let installations = self.config.store.installations()?;
        for job in &jobs {
            let Some(registration_id) = job.registration_id.as_deref() else {
                continue;
            };
            if !live.contains(registration_id) {
                continue;
            }
            let Some(installation) = installations
                .iter()
                .find(|installation| installation.job_id == job.id)
            else {
                continue;
            };
            if installation.catalog_id == catalog_id {
                return Ok(Some(installation.installed_version.clone()));
            }
        }
        Ok(None)
    }

    fn find_slot(
        &self,
        catalog_id: &str,
        slot_id: &str,
    ) -> Option<gamenode_core::ports::SetupFile> {
        let catalog = lock(&self.inner).catalog.clone()?;
        catalog
            .entries()
            .iter()
            .find(|entry| entry.catalog_id == catalog_id)?
            .launcher
            .setup_files
            .iter()
            .find(|slot| slot.id == slot_id)
            .cloned()
    }

    fn catalog_and_overlay(
        &self,
    ) -> Result<
        (
            Arc<CatalogWorker>,
            Arc<gamenode_core::ports::overlay::Overlay>,
        ),
        PortsError,
    > {
        let inner = lock(&self.inner);
        let catalog = inner
            .catalog
            .clone()
            .ok_or_else(|| PortsError::Unavailable("the catalog is not available".into()))?;
        let overlay = inner
            .overlay
            .clone()
            .ok_or_else(|| PortsError::Unavailable("the overlay is not available".into()))?;
        Ok((catalog, overlay))
    }

    fn is_ready(&self) -> bool {
        lock(&self.inner).ready
    }

    fn wait_ready(&self) {
        let mut inner = lock(&self.inner);
        while !inner.ready {
            inner = self
                .ready
                .wait(inner)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// Fetches a newer overlay with its ETag at most once at a time. Never blocks the caller.
    fn request_overlay_refresh(self: &Arc<Self>) {
        {
            let mut inner = lock(&self.inner);
            if inner.overlay_refreshing || inner.catalog.is_none() {
                return;
            }
            inner.overlay_refreshing = true;
        }
        let service = Arc::clone(self);
        std::thread::spawn(move || {
            let outcome = service
                .overlay_source
                .refresh(&*service.config.overlay_fetcher);
            match outcome {
                Ok(gamenode_platform::ports::overlay_source::RefreshOutcome::Replaced {
                    overlay,
                }) => {
                    let overlay = Arc::new(overlay);
                    let catalog = lock(&service.inner).catalog.clone();
                    if let Some(catalog) = catalog {
                        catalog.set_overlay(Arc::clone(&overlay));
                    }
                    {
                        let mut inner = lock(&service.inner);
                        inner.overlay = Some(overlay);
                        inner.facts.clear();
                    }
                    service.emit_view(None);
                }
                Ok(gamenode_platform::ports::overlay_source::RefreshOutcome::NotModified) => {}
                Ok(gamenode_platform::ports::overlay_source::RefreshOutcome::KeptPrevious {
                    reason,
                }) => tracing::debug!(%reason, "kept the previous PC ports overlay"),
                Err(error) => tracing::warn!(%error, "could not store the PC ports overlay"),
            }
            lock(&service.inner).overlay_refreshing = false;
        });
    }
}

/// The ids the runner currently owns: the running job and the queue.
fn active_jobs(runner: &Option<Arc<dyn InstallRunner>>) -> HashSet<String> {
    let Some(runner) = runner else {
        return HashSet::new();
    };
    runner
        .active_job()
        .into_iter()
        .chain(runner.queued_jobs())
        .collect()
}

/// The source page for an overlay extractor's `owner/repo`, when it is a plain GitHub name.
///
/// The overlay schema stores a repository, not a URL, so Settings shows the conventional GitHub
/// address rather than trusting a URL from remote data.
fn extractor_source_url(repository: &str) -> Option<String> {
    let repository = repository.trim();
    let (owner, name) = repository.split_once('/')?;
    if owner.is_empty()
        || name.is_empty()
        || name.contains('/')
        || repository.contains([' ', '?', '#'])
    {
        return None;
    }
    Some(format!("https://github.com/{repository}"))
}

fn accepted_labels(slot: &gamenode_core::ports::SetupFile) -> Vec<String> {
    slot.accepted
        .iter()
        .map(|accepted| {
            accepted
                .label
                .clone()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or_else(|| format!("{}:{}", accepted.algo.label(), accepted.value))
        })
        .collect()
}

fn apply_preferences(preferences: &mut PortPreferences, patch: &PortPreferencesPatch) {
    if let Some(view_mode) = patch.view {
        preferences.view = view_mode;
    }
    if let Some(filters) = &patch.filters {
        if let Some(consoles) = &filters.consoles {
            preferences.consoles = consoles.clone();
        }
        if let Some(guided) = filters.guided {
            preferences.guided = guided;
        }
        if let Some(verified) = filters.verified {
            preferences.verified = verified;
        }
        if let Some(installed) = filters.installed {
            preferences.installed = installed;
        }
    }
    if let Some(last_parent) = &patch.last_parent_folder {
        preferences.last_parent = last_parent.clone();
    }
}

/// Groups the request's choices by slot and turns package pairs into one source.
fn translate_supplied(supplied: &[PortSuppliedChoice]) -> Result<Vec<SuppliedChoice>, PortsError> {
    #[derive(Default)]
    struct Group {
        path: Option<String>,
        base: Option<String>,
        update: Option<String>,
        package: bool,
        direct: bool,
    }
    let mut order: Vec<&str> = Vec::new();
    let mut groups: HashMap<&str, Group> = HashMap::new();
    for choice in supplied {
        let group = groups.entry(choice.slot_id.as_str()).or_default();
        if !order.contains(&choice.slot_id.as_str()) {
            order.push(choice.slot_id.as_str());
        }
        if choice.package {
            group.package = true;
            if choice.update {
                group.update = Some(choice.path.clone());
            } else {
                group.base = Some(choice.path.clone());
            }
        } else {
            group.direct = true;
            group.path = Some(choice.path.clone());
        }
    }
    let mut translated = Vec::new();
    for slot in order {
        let group = &groups[slot];
        if group.direct && group.package {
            return Err(PortsError::Invalid(format!(
                "the slot {slot:?} mixes a direct pick with console packages"
            )));
        }
        let source = if group.package {
            let base = group.base.clone().ok_or_else(|| {
                PortsError::Invalid(format!("the slot {slot:?} has no base package"))
            })?;
            SuppliedSource::Packages {
                base,
                update: group.update.clone(),
            }
        } else {
            let path = group
                .path
                .clone()
                .ok_or_else(|| PortsError::Invalid(format!("the slot {slot:?} has no path")))?;
            SuppliedSource::Path { path }
        };
        translated.push(SuppliedChoice {
            slot: slot.to_owned(),
            source,
        });
    }
    Ok(translated)
}

/// A console package is compressed, and its extracted tree is what lands on the target volume.
/// Counting twice the package files keeps the free-space check on the safe side, matching the
/// release asset's extraction estimate.
const PACKAGE_EXTRACTED_SIZE_ESTIMATE: u64 = 2;

pub(crate) fn supplied_bytes(supplied: &[SuppliedChoice]) -> u64 {
    supplied
        .iter()
        .map(|choice| match &choice.source {
            SuppliedSource::Path { path } => path_bytes(Path::new(path)),
            SuppliedSource::Packages { base, update } => {
                let packages = path_bytes(Path::new(base)).saturating_add(
                    update
                        .as_deref()
                        .map(|path| path_bytes(Path::new(path)))
                        .unwrap_or(0),
                );
                packages.saturating_mul(PACKAGE_EXTRACTED_SIZE_ESTIMATE)
            }
        })
        .fold(0u64, u64::saturating_add)
}

fn path_bytes(path: &Path) -> u64 {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| path_bytes(&entry.path()))
        .fold(0u64, u64::saturating_add)
}

fn target_is_empty(target: &str) -> bool {
    match std::fs::read_dir(target) {
        Ok(mut entries) => entries.next().is_none(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

/// The picked executable, relative to the install folder, in the folder's own separators.
fn relative_inside(folder: &str, picked: &str) -> Result<String, PortsError> {
    let folder_path = Path::new(folder);
    let picked_path = Path::new(picked);
    let canonical_folder = folder_path.canonicalize().map_err(|error| {
        PortsError::Invalid(format!("the install folder could not be read: {error}"))
    })?;
    let canonical_picked = picked_path.canonicalize().map_err(|error| {
        PortsError::Invalid(format!(
            "the chosen executable could not be opened: {error}"
        ))
    })?;
    let relative = canonical_picked
        .strip_prefix(&canonical_folder)
        .map_err(|_| {
            PortsError::Invalid("choose an executable inside the install folder".into())
        })?;
    let relative = relative.to_string_lossy();
    if !relative
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".exe"))
    {
        return Err(PortsError::Invalid(
            "choose a Windows executable (.exe)".into(),
        ));
    }
    Ok(relative.replace('/', "\\"))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The coalescer between install progress callbacks and the `ports:progress` event (design D5).
///
/// It keeps the newest tick per job and emits at most one batch every [`PROGRESS_INTERVAL`]. The
/// engine owns one and its emitter is a Tauri `emit`; tests record the emitted values.
pub struct PortProgressPublisher {
    shared: Arc<ProgressShared>,
    runtime: tokio::runtime::Handle,
    emit: Arc<dyn Fn(PortProgress) + Send + Sync>,
}

struct ProgressShared {
    state: Mutex<ProgressState>,
}

#[derive(Default)]
struct ProgressState {
    pending: BTreeMap<String, PortProgress>,
    scheduled: bool,
    last_sent: Option<Instant>,
}

impl PortProgressPublisher {
    pub fn new(
        runtime: tokio::runtime::Handle,
        emit: impl Fn(PortProgress) + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(ProgressShared {
                state: Mutex::new(ProgressState::default()),
            }),
            runtime,
            emit: Arc::new(emit),
        }
    }

    /// A publisher for tests and for builds without a Tauri event bridge.
    pub fn disabled(runtime: tokio::runtime::Handle) -> Self {
        Self::new(runtime, |_| {})
    }

    /// Hands a tick over without blocking. The newest value per job wins.
    pub fn publish(&self, progress: PortProgress) {
        let wait = {
            let mut state = lock(&self.shared.state);
            state.pending.insert(progress.job_id.clone(), progress);
            if state.scheduled {
                return;
            }
            state.scheduled = true;
            state.last_sent.map_or(Duration::ZERO, |at| {
                PROGRESS_INTERVAL.saturating_sub(at.elapsed())
            })
        };
        let shared = Arc::clone(&self.shared);
        let emit = Arc::clone(&self.emit);
        self.runtime.spawn(async move {
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
            loop {
                let batch = {
                    let mut state = lock(&shared.state);
                    let batch = std::mem::take(&mut state.pending);
                    state.last_sent = Some(Instant::now());
                    batch
                };
                for progress in batch.into_values() {
                    emit(progress);
                }
                {
                    let mut state = lock(&shared.state);
                    if state.pending.is_empty() {
                        state.scheduled = false;
                        break;
                    }
                }
                tokio::time::sleep(PROGRESS_INTERVAL).await;
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Engine integration
// ---------------------------------------------------------------------------------------------

impl Engine {
    fn ports_service(&self) -> Option<Arc<PortsService>> {
        self.shared.ports.pc_ports.clone()
    }

    /// True when this build has PC ports (Windows signed-in builds).
    pub fn pc_ports_supported(&self) -> bool {
        self.shared.ports.pc_ports.is_some()
    }

    /// Starts the first catalog fetch. Non-blocking: the service posts a worker command.
    pub fn ports_open(&self) {
        if let Some(service) = self.ports_service() {
            service.open();
        }
    }

    /// Requests a catalog refresh. Non-blocking.
    pub fn ports_refresh(&self) {
        if let Some(service) = self.ports_service() {
            service.refresh();
        }
    }

    /// Requests install cancellation. Non-blocking.
    pub fn ports_cancel(&self, job_id: String) {
        if let Some(service) = self.ports_service() {
            service.cancel(&job_id);
        }
    }

    /// Marks an entry's finished result seen on a worker; returns before SQLite answers.
    pub fn ports_mark_seen(&self, catalog_id: String) {
        let Some(service) = self.ports_service() else {
            return;
        };
        self.shared.runtime.spawn_blocking(move || {
            if let Err(error) = service.mark_seen(&catalog_id) {
                tracing::warn!(%error, "could not mark the port result seen");
            }
        });
    }

    /// Saves remembered view state on a worker; returns before SQLite answers.
    pub fn ports_set_preferences(&self, patch: PortPreferencesPatch) {
        let Some(service) = self.ports_service() else {
            return;
        };
        self.shared.runtime.spawn_blocking(move || {
            if let Err(error) = service.set_preferences(&patch) {
                tracing::warn!(%error, "could not save the PC ports preferences");
            }
        });
    }

    /// The Tools panel's PC ports cache block. Measuring the cache runs on a worker thread.
    pub async fn ports_tools_cache(&self) -> Result<PortToolsCacheView, PortsError> {
        let Some(service) = self.ports_service() else {
            return Err(PortsError::Unsupported);
        };
        tokio::task::spawn_blocking(move || service.tools_cache())
            .await
            .map_err(|_| PortsError::NotReady)?
    }

    /// Deletes the cached port downloads and extraction tools. Runs on a worker thread.
    pub async fn ports_clear_cache(&self) -> Result<PortToolsCacheView, PortsError> {
        let Some(service) = self.ports_service() else {
            return Err(PortsError::Unsupported);
        };
        tokio::task::spawn_blocking(move || service.clear_cache())
            .await
            .map_err(|_| PortsError::NotReady)?
    }

    /// Lazy entry detail. The blocking fetch runs on a worker thread.
    pub async fn ports_detail(
        &self,
        catalog_id: String,
    ) -> Result<Option<PortDetailView>, PortsError> {
        let Some(service) = self.ports_service() else {
            return Err(PortsError::Unsupported);
        };
        tokio::task::spawn_blocking(move || service.detail(&catalog_id))
            .await
            .map_err(|_| PortsError::NotReady)?
    }

    /// Native parent-folder picker. The modal dialog runs on a worker thread.
    pub async fn ports_pick_parent(
        &self,
        initial_folder: Option<String>,
    ) -> Result<Option<String>, PortsError> {
        let Some(service) = self.ports_service() else {
            return Err(PortsError::Unsupported);
        };
        tokio::task::spawn_blocking(move || service.pick_parent(initial_folder.as_deref()))
            .await
            .map_err(|_| PortsError::NotReady)?
    }

    /// Guided-setup picker and validation. Hashing runs on a worker thread and reports progress.
    pub async fn ports_pick_supplied(
        &self,
        request: PortPickSuppliedRequest,
    ) -> Result<Option<PortSuppliedPick>, PortsError> {
        let Some(service) = self.ports_service() else {
            return Err(PortsError::Unsupported);
        };
        tokio::task::spawn_blocking(move || service.pick_supplied(&request))
            .await
            .map_err(|_| PortsError::NotReady)?
    }

    /// Plans and queues an install. Every blocking step runs on a worker thread.
    pub async fn ports_install(
        &self,
        request: PortInstallRequest,
    ) -> Result<Option<String>, PortsError> {
        let Some(service) = self.ports_service() else {
            return Err(PortsError::Unsupported);
        };
        tokio::task::spawn_blocking(move || service.install(&request))
            .await
            .map_err(|_| PortsError::NotReady)?
    }

    /// The executable follow-up of a completed install. The native dialog runs on a worker.
    pub async fn ports_pick_executable(
        &self,
        job_id: String,
    ) -> Result<Option<String>, PortsError> {
        let Some(service) = self.ports_service() else {
            return Err(PortsError::Unsupported);
        };
        tokio::task::spawn_blocking(move || service.pick_executable(&job_id))
            .await
            .map_err(|_| PortsError::NotReady)?
    }

    /// The environment variables a port-installed registration launches with, read from its
    /// provenance (design D7). Blocking; called from the launch worker.
    pub fn port_launch_environment(
        &self,
        registration_id: &str,
    ) -> gamenode_core::drm_free::LaunchEnvironment {
        let Some(store) = &self.shared.ports.port_store else {
            return Default::default();
        };
        match store.installations() {
            Ok(installations) => installations
                .into_iter()
                .find(|installation| installation.registration_id == registration_id)
                .map(|installation| installation.environment)
                .unwrap_or_default(),
            Err(error) => {
                tracing::warn!(%error, registration_id, "could not read the port provenance");
                Default::default()
            }
        }
    }

    /// Links an existing install's provenance to a registration the user added again (design D10).
    /// Best effort: a failure is logged and never blocks adding an entry.
    pub fn relink_port_registration(&self, registration: &gamenode_core::drm_free::Registration) {
        let Some(store) = &self.shared.ports.port_store else {
            return;
        };
        let Some(local_games) = &self.shared.ports.local_games else {
            return;
        };
        let live = match local_games.registrations() {
            Ok(registrations) => registrations
                .into_iter()
                .filter(|registration| !registration.removed)
                .map(|registration| registration.id)
                .collect::<HashSet<_>>(),
            Err(error) => {
                tracing::warn!(%error, "could not read the registrations for port relinking");
                return;
            }
        };
        let installations = match store.installations() {
            Ok(installations) => installations,
            Err(error) => {
                tracing::warn!(%error, "could not read the port provenance for relinking");
                return;
            }
        };
        for installation in installations {
            if live.contains(&installation.registration_id) {
                continue;
            }
            if !installation
                .install_folder
                .eq_ignore_ascii_case(&registration.root)
            {
                continue;
            }
            if let Err(error) = store.relink_installation(&installation.job_id, &registration.id) {
                tracing::warn!(%error, job_id = %installation.job_id, "could not relink the port provenance");
            }
        }
    }

    /// Applies a completed port install to the library and runs the D11 matcher.
    pub fn port_install_completed(&self, completion: InstallCompletion) {
        // The registration is committed. Library rows come from the manual provider's scan, so
        // the entry only appears once a scan reads it; the local view is refreshed separately.
        self.rescan();
        self.refresh_local_games();
        let engine = self.clone();
        self.shared
            .runtime
            .spawn(async move { engine.match_port_install(completion).await });
    }

    async fn match_port_install(&self, completion: InstallCompletion) {
        use gamenode_core::ports::matching::{PortMatchSearch, decide, inference};

        let (account, credential) = {
            let state = self.lock();
            (
                state.library.account().map(|account| account.id.clone()),
                state.machine.verified_credential().cloned(),
            )
        };
        let (account, credential) = match (account, credential) {
            (Some(account), Some(credential)) => (account, credential),
            _ => return,
        };
        let registration = completion.registration.clone();
        let key = registration.key();
        // A choice the user made always wins: never rewrite it.
        if self.lock().library.chosen_match(&key).is_some() {
            return;
        }
        if self
            .lock()
            .local
            .registrations
            .iter()
            .any(|known| known.id == registration.id && known.removed)
        {
            return;
        }
        let search_page = match &completion.match_input {
            gamenode_core::ports::matching::PortMatchInput::Title { title } => self
                .shared
                .ports
                .games
                .search_page(&credential, title)
                .await
                .ok(),
            gamenode_core::ports::matching::PortMatchInput::GameNodeId { .. } => self
                .shared
                .ports
                .games
                .search_page(&credential, &registration.title)
                .await
                .ok(),
            gamenode_core::ports::matching::PortMatchInput::None => None,
        };
        let decision = decide(
            &completion.match_input,
            None,
            search_page.as_ref().map(|page| PortMatchSearch {
                games: &page.games,
                complete: page.complete == Some(true),
            }),
        );
        let game = search_page.as_ref().and_then(|page| {
            let game_id = decision.game_id()?;
            page.games
                .iter()
                .find(|game| game.id == game_id)
                .map(|game| gamenode_core::library::GameRecord {
                    game_id: game.id,
                    name: game.name.clone(),
                    release_year: game.release_year,
                    cover_url: game.cover_url.clone(),
                    cover_file: None,
                    cover_failed_at: None,
                })
        });
        let inference = inference(&completion.match_input, &registration, &account, &decision);
        let store = self.shared.ports.local_games.clone();
        let library = self.shared.ports.library.clone();
        let game_for_save = game.clone();
        let saved = self
            .shared
            .runtime
            .spawn_blocking(move || {
                if let Some(store) = &store {
                    store.save_inference(&inference)?;
                }
                if let Some(game) = game_for_save {
                    library.save_games(&[game])?;
                }
                Ok::<_, StorageError>(())
            })
            .await;
        if !matches!(saved, Ok(Ok(()))) {
            return;
        }
        {
            let mut state = self.lock();
            if state
                .library
                .account()
                .is_some_and(|current| current.id == account)
                && state.library.chosen_match(&key).is_none()
            {
                let effects = state
                    .library
                    .set_local_inference(key, decision.game_id(), game);
                for effect in effects {
                    self.library_effect(&mut state, effect);
                }
                self.publish(&mut state);
            }
        }
        self.refresh_local_games();
    }
}

impl Engine {
    /// Stops the PC ports workers on a dedicated thread. The runner cancels its job and the
    /// catalog worker finishes its in-flight fetch, so quitting never blocks the UI for longer
    /// than a short flush.
    pub(super) fn shutdown_ports(&self) {
        let Some(service) = self.ports_service() else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("ports-shutdown".into())
            .spawn(move || {
                service.shutdown();
                let _ = tx.send(());
            });
        if spawned.is_err() {
            tracing::warn!("could not start the PC ports shutdown worker");
            return;
        }
        if rx.recv_timeout(Duration::from_secs(2)).is_err() {
            tracing::warn!("PC ports shutdown timed out");
        }
    }
}
