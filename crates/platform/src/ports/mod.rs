//! PC ports host adapters: supplied-file validation and placement (design D5, D7).
//!
//! The catalog client, durable store, install job runner and extractor runner live in their own
//! submodules. Every function here performs blocking I/O or hashing and belongs on a worker
//! thread, never on the UI or main thread.

pub mod cache;
pub mod catalog_worker;
pub mod extractor;
pub mod job_runner;
pub mod overlay_source;
pub mod placement;
pub mod quiver;
pub mod recovery;
pub mod supplied;

pub mod art;

pub use cache::{
    clear_ports_cache, directory_usage, ports_downloads_dir, ports_downloads_usage,
    ports_tool_usage, ports_tools_dir,
};
pub use catalog_worker::{
    CatalogError, CatalogEvent, CatalogSource, CatalogState, CatalogStatus, CatalogWorker, Clock,
    DAILY_REFRESH_MS, LAZY_CACHE_MS, SystemClock,
};
pub use quiver::{
    HttpResponse, QuiverClient, QuiverError, QuiverHttp, QuiverTransportError, ReqwestQuiverHttp,
    Sleeper, ThreadSleeper, user_agent,
};

pub use extractor::{
    CancelFlag, ExtractorError, ExtractorFailure, ExtractorRunOutcome, ExtractorRunRequest,
    ExtractorRunner, InstalledTool, MAX_CAPTURED_OUTPUT_BYTES, OutputFolderValidator,
    SuppliedFolderValidator, run_extractor, supplied_folder_validator,
};
pub use job_runner::{
    ExecutableIdentity, InstallCompletion, InstallJobRequest, JobError, JobEvent, JobProgress,
    JobRunner, JobRunnerConfig, PackageExtractor, PinnedExtractor, SystemExecutableIdentity,
    UserDataRoots, mark_job_seen,
};
pub use placement::{
    CopyRoots, PlaceError, PlacedSlot, PlacementOutcome, SlotChoice, launch_arguments, place_slots,
};
pub use recovery::{RecoveryReport, recover_interrupted_jobs};
pub use supplied::{
    CheckOutcome, CheckedChoice, FileDigests, SupplyError, WorkProgress, accepted_versions,
    check_choice, hash_file,
};

pub use art::{
    HttpPortArtFetcher, MAX_PORT_ART_BYTES, PortArtCache, PortArtError, PortArtFetcher,
    PortArtImage, PortArtKind, PortArtLoader, PortArtResolver, PortArtSource, PortArtUrlPolicy,
    SnapshotPortArtResolver, image_mime, ports_art_cache_dir, resolve_in_snapshot,
};
