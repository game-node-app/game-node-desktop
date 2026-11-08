//! Running the pinned extraction tool that turns console packages into a game folder (design D8).
//!
//! The tool is pinned by the overlay: a GitHub release asset with a SHA-256, an executable inside
//! the extracted folder and an argument template. Preparing it uses the same cached, verified
//! download as game tools ([`crate::download::ensure_cached_download`]): the archive is cached
//! under `<data_dir>/ports/tools/<id>/<tag>/download/`, extracted into a sibling `tool/` folder and
//! the executable is checked before anything runs. A checksum mismatch fails before a process is
//! created.
//!
//! Running never uses a shell. The argument list is the overlay template with `{package}`,
//! `{update}` and `{out}` replaced by canonicalized paths; a placeholder must be an argument of
//! its own, and an unknown or embedded placeholder rejects the template. The tool runs with its
//! working directory set to the staging folder, without a console window, and inside a Windows job
//! object (a process group on other systems) so cancelling kills its whole process tree.
//!
//! stdout and stderr are consumed together and the last [`MAX_CAPTURED_OUTPUT_BYTES`] bytes are
//! kept for the failure details. A non-zero exit, an encrypted retail package or an output folder
//! that fails validation is reported with the paths the attempt created, so the job runner can
//! delete them.
//!
//! Everything here is blocking and must run on a worker thread, never on the UI thread.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use gamenode_core::ports::Extractor;
use gamenode_core::tools::ToolError;

use crate::archive::{PORTS_MAX_TOTAL_EXTRACTED_BYTES, extract_asset_to_dir};
use crate::download::{
    CachedDownloadRequest, DownloadFetcher, DownloadUrlPolicy, GitHubDownloadPolicy,
    HttpDownloadFetcher, ensure_cached_download, ports_tool_cache_dir,
};

/// Bytes of combined stdout and stderr kept for the failure details (design D8).
pub const MAX_CAPTURED_OUTPUT_BYTES: usize = 64 * 1024;

/// How often a running tool checks for cancellation.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// The longest the runner waits for the output pipes to close after the tool exited.
const READER_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Read size used while draining the tool's pipes.
const OUTPUT_READ_CHUNK: usize = 8 * 1024;

/// Names a staging extraction folder uniquely within this process.
static EXTRACT_COUNTER: AtomicU64 = AtomicU64::new(1);

const PACKAGE_PLACEHOLDER: &str = "{package}";
const UPDATE_PLACEHOLDER: &str = "{update}";
const OUTPUT_PLACEHOLDER: &str = "{out}";

/// A cancellation flag shared by the job runner and a running tool.
///
/// The job runner sets it from the UI command; the extractor polls it while the process runs and
/// kills the tool's process tree when it is set.
#[derive(Clone, Debug, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wraps a flag the job runner already shares with the rest of the job.
    pub fn from_shared(flag: Arc<AtomicBool>) -> Self {
        Self(flag)
    }

    /// Requests cancellation. Safe to call from any thread.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// The underlying flag, for a job that keeps its own shared copy.
    pub fn shared(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.0)
    }
}

/// A pinned tool that is downloaded, verified and extracted, ready to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledTool {
    /// The extracted tool folder: `<data_dir>/ports/tools/<id>/<tag>/tool`.
    pub root: PathBuf,
    /// The executable named by the overlay, inside [`InstalledTool::root`].
    pub executable: PathBuf,
}

/// Decides whether an extraction tool's output folder is usable.
///
/// The job runner passes the same check a user-supplied folder goes through (design D7): every
/// marker in `contains` must exist and the marker hashes must match. This module only knows about
/// "the output is acceptable"; the slot recipe stays in the job runner so this file does not
/// depend on the supplied-file code.
pub trait OutputFolderValidator {
    /// Validates `folder`, an absolute canonical path. The message is user-facing.
    fn validate(&self, folder: &Path) -> Result<(), String>;
}

impl<F> OutputFolderValidator for F
where
    F: Fn(&Path) -> Result<(), String>,
{
    fn validate(&self, folder: &Path) -> Result<(), String> {
        self(folder)
    }
}

/// Validates an extraction tool's output folder against a folder setup slot.
///
/// This reuses the supplied-file check (design D7): every `contains` marker must exist and the
/// marker hashes must match one of the accepted versions. `cancelled` is polled between hashing
/// chunks.
pub struct SuppliedFolderValidator<'a> {
    slot: &'a gamenode_core::ports::SetupFile,
    cancelled: &'a AtomicBool,
}

impl OutputFolderValidator for SuppliedFolderValidator<'_> {
    fn validate(&self, folder: &Path) -> Result<(), String> {
        let outcome = crate::ports::supplied::check_choice(
            Some(folder),
            self.slot,
            &mut |_| {},
            self.cancelled,
        )
        .map_err(|error| error.to_string())?;
        match outcome {
            crate::ports::supplied::CheckOutcome::Accepted(_) => Ok(()),
            crate::ports::supplied::CheckOutcome::Skipped => {
                Err("the extraction tool produced no folder to check".into())
            }
        }
    }
}

/// The output check for a package slot: the same rules a user-supplied folder gets.
pub fn supplied_folder_validator<'a>(
    slot: &'a gamenode_core::ports::SetupFile,
    cancelled: &'a AtomicBool,
) -> SuppliedFolderValidator<'a> {
    SuppliedFolderValidator { slot, cancelled }
}

/// Everything a single extraction run needs.
pub struct ExtractorRunRequest<'a> {
    /// The overlay definition; its `args` template is the only source of arguments.
    pub extractor: &'a Extractor,
    /// The prepared tool from [`ExtractorRunner::install_tool`].
    pub tool: &'a InstalledTool,
    /// The user's base package, such as the game `.pkg`.
    pub package: &'a Path,
    /// The update package to merge, when the recipe has one.
    pub update: Option<&'a Path>,
    /// The folder the tool writes; validated afterwards like a supplied folder.
    pub output: &'a Path,
    /// The staging folder; the tool runs with this as its working directory.
    pub working_directory: &'a Path,
    /// Checked while the tool runs.
    pub cancel: &'a CancelFlag,
    /// The output check reused from the supplied-folder path.
    pub validate_output: &'a dyn OutputFolderValidator,
}

/// A successful extraction run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractorRunOutcome {
    /// The last [`MAX_CAPTURED_OUTPUT_BYTES`] bytes of stdout and stderr, lossily decoded.
    pub output: String,
    /// The tool's exit code, when the platform reported one.
    pub exit_code: Option<i32>,
    /// Paths the attempt created, so the job runner journals them and removes them on failure.
    ///
    /// This is the output folder when it did not exist yet plus the top-level entries that
    /// appeared in the working directory. The tool's writes inside an existing folder are covered
    /// by that folder being reported as a whole.
    pub created_paths: Vec<PathBuf>,
}

/// Why an extraction run failed.
#[derive(Debug, thiserror::Error)]
pub enum ExtractorError {
    #[error("the pinned extractor `{id}` is not complete or well formed")]
    InvalidExtractor { id: String },
    #[error("the extractor argument template is invalid: {reason}")]
    InvalidTemplate { reason: String },
    #[error("the extractor's {role} path is not usable: {reason}")]
    InvalidPath { role: &'static str, reason: String },
    #[error("the extractor needs an update package, but none was chosen")]
    MissingUpdate,
    #[error("the extractor executable was not found after extraction: {path}")]
    ExecutableMissing { path: String },
    #[error("the extraction tool could not be prepared: {0}")]
    Tool(#[from] ToolError),
    #[error("the extraction tool could not be started: {0}")]
    Spawn(String),
    #[error("the extraction tool exited with code {code}")]
    Exit { code: i32 },
    #[error("the extraction tool was terminated before it finished")]
    Terminated,
    #[error(
        "the package is an encrypted retail package; GameNode ships no keys and cannot extract it"
    )]
    EncryptedPackage,
    #[error("the extraction tool's output folder is not usable: {0}")]
    OutputRejected(String),
    #[error("the update package produced an unsafe path ({path}): {reason}")]
    UnsafeUpdatePath { path: String, reason: String },
    #[error("the extraction was cancelled")]
    Cancelled,
    #[error("I/O error: {0}")]
    Io(String),
}

impl ExtractorError {
    /// True when the job stopped because the user cancelled it.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// A failed extraction run, with what the job runner needs to report and clean up.
#[derive(Debug)]
pub struct ExtractorFailure {
    /// The structured reason, for the job's failure phase and details view.
    pub error: ExtractorError,
    /// The last [`MAX_CAPTURED_OUTPUT_BYTES`] bytes of stdout and stderr at the time of failure.
    pub output: String,
    /// Paths this attempt created. The job runner deletes them when the job fails.
    pub created_paths: Vec<PathBuf>,
}

impl ExtractorFailure {
    fn from_error(error: ExtractorError) -> Self {
        Self {
            error,
            output: String::new(),
            created_paths: Vec::new(),
        }
    }

    /// The user-facing reason, as stored in [`gamenode_core::ports::PortJobPhase::Failed`].
    pub fn reason(&self) -> String {
        self.error.to_string()
    }
}

impl fmt::Display for ExtractorFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for ExtractorFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Downloads, verifies and extracts the tools pinned by the overlay.
///
/// One runner is created per worker; it is not `Copy` but is cheap to share behind a reference.
pub struct ExtractorRunner {
    data_dir: PathBuf,
    fetcher: Box<dyn DownloadFetcher>,
    policy: Box<dyn DownloadUrlPolicy>,
}

impl ExtractorRunner {
    /// Production runner: HTTPS downloads from the pinned GitHub releases.
    pub fn new(data_dir: impl Into<PathBuf>) -> Result<Self, ExtractorError> {
        let fetcher = HttpDownloadFetcher::new()?;
        Ok(Self {
            data_dir: data_dir.into(),
            fetcher: Box::new(fetcher),
            policy: Box::new(GitHubDownloadPolicy::new()),
        })
    }

    /// Runner with an injected downloader, for tests and mirrors.
    pub fn with_downloader(
        data_dir: impl Into<PathBuf>,
        fetcher: Box<dyn DownloadFetcher>,
        policy: Box<dyn DownloadUrlPolicy>,
    ) -> Self {
        Self {
            data_dir: data_dir.into(),
            fetcher,
            policy,
        }
    }

    /// Ensures the pinned tool is downloaded, verified and extracted, and returns its executable.
    ///
    /// The cached archive is reused while it still matches the pinned SHA-256. A mismatch fails
    /// here and nothing runs. Extraction happens in a temporary folder that is renamed into place
    /// only after the executable is found, so a failed extraction never leaves a half tool.
    pub fn install_tool(
        &self,
        extractor: &Extractor,
        cancel: &CancelFlag,
    ) -> Result<InstalledTool, ExtractorFailure> {
        self.install_tool_inner(extractor, cancel)
            .map_err(ExtractorFailure::from_error)
    }

    fn install_tool_inner(
        &self,
        extractor: &Extractor,
        cancel: &CancelFlag,
    ) -> Result<InstalledTool, ExtractorError> {
        if !extractor.is_valid() {
            return Err(ExtractorError::InvalidExtractor {
                id: extractor.id.clone(),
            });
        }
        if cancel.is_cancelled() {
            return Err(ExtractorError::Cancelled);
        }

        let tool_root = ports_tool_cache_dir(&self.data_dir, &extractor.id, &extractor.tag);
        let download_dir = tool_root.join("download");
        let url = release_asset_url(extractor)?;
        let request = CachedDownloadRequest {
            url: &url,
            cache_dir: &download_dir,
            file_name: &extractor.asset,
            expected_sha256: Some(&extractor.sha256),
        };
        let cached =
            ensure_cached_download(self.fetcher.as_ref(), self.policy.as_ref(), &request, None)?;
        if cancel.is_cancelled() {
            return Err(ExtractorError::Cancelled);
        }

        let tool_dir = tool_root.join("tool");
        let relative = executable_relative_path(extractor)?;
        let executable = tool_dir.join(&relative);
        if executable.is_file() {
            return Ok(InstalledTool {
                root: canonical(&tool_dir, "tool")?,
                executable: canonical(&executable, "executable")?,
            });
        }

        // Leftovers from a crash during an earlier extraction are never reused.
        remove_stale_extractions(&tool_root);

        let staging = tool_root.join(format!(
            ".extract-{}-{}",
            std::process::id(),
            EXTRACT_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&staging);
        if let Err(error) =
            extract_asset_to_dir(&cached.path, &staging, PORTS_MAX_TOTAL_EXTRACTED_BYTES)
        {
            let _ = fs::remove_dir_all(&staging);
            return Err(error.into());
        }
        if !staging.join(&relative).is_file() {
            let _ = fs::remove_dir_all(&staging);
            return Err(ExtractorError::ExecutableMissing {
                path: executable.display().to_string(),
            });
        }
        // Replace a stale tool folder only once the new one is complete.
        let _ = fs::remove_dir_all(&tool_dir);
        fs::rename(&staging, &tool_dir).map_err(|e| {
            let _ = fs::remove_dir_all(&staging);
            ExtractorError::Io(e.to_string())
        })?;

        Ok(InstalledTool {
            root: canonical(&tool_dir, "tool")?,
            executable: canonical(&executable, "executable")?,
        })
    }
}

/// Runs a prepared tool once and validates its output folder.
///
/// The function blocks until the process exits, is cancelled, or fails to start. It captures at
/// most [`MAX_CAPTURED_OUTPUT_BYTES`] of combined stdout and stderr and reports the paths the
/// attempt created on failure.
pub fn run_extractor(
    request: ExtractorRunRequest<'_>,
) -> Result<ExtractorRunOutcome, ExtractorFailure> {
    run_extractor_inner(&request)
}

fn run_extractor_inner(
    request: &ExtractorRunRequest<'_>,
) -> Result<ExtractorRunOutcome, ExtractorFailure> {
    let package =
        canonical_file(request.package, "package").map_err(ExtractorFailure::from_error)?;
    let update = match request.update {
        Some(update) => {
            Some(canonical_file(update, "update").map_err(ExtractorFailure::from_error)?)
        }
        None => None,
    };
    if request.cancel.is_cancelled() {
        return Err(ExtractorFailure::from_error(ExtractorError::Cancelled));
    }

    let mut created_paths = Vec::new();
    if !request.output.is_absolute() || request.output.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(ExtractorFailure::from_error(ExtractorError::InvalidPath {
            role: "output",
            reason: format!(
                "{} is not an absolute path without NUL bytes",
                request.output.display()
            ),
        }));
    }
    if !request.output.exists() {
        fs::create_dir_all(request.output).map_err(|e| {
            ExtractorFailure::from_error(ExtractorError::Io(format!(
                "{}: {e}",
                request.output.display()
            )))
        })?;
        created_paths.push(request.output.to_path_buf());
    }
    let output = canonical_dir(request.output, "output").map_err(ExtractorFailure::from_error)?;
    let working = canonical_dir(request.working_directory, "working directory")
        .map_err(ExtractorFailure::from_error)?;

    let arguments = expand_arguments(
        &request.extractor.args,
        &PlaceholderPaths {
            package: &package,
            update: update.as_deref(),
            output: &output,
        },
    )
    .map_err(ExtractorFailure::from_error)?;

    if !request.tool.executable.is_absolute() || !request.tool.executable.is_file() {
        return Err(ExtractorFailure {
            error: ExtractorError::ExecutableMissing {
                path: request.tool.executable.display().to_string(),
            },
            output: String::new(),
            created_paths,
        });
    }
    if request.cancel.is_cancelled() {
        return Err(ExtractorFailure {
            error: ExtractorError::Cancelled,
            output: String::new(),
            created_paths,
        });
    }

    let before = top_level_entries(&working);
    let raw = match spawn_and_wait(
        &request.tool.executable,
        &arguments,
        &working,
        request.cancel,
    ) {
        Ok(raw) => raw,
        Err((error, output)) => {
            created_paths.extend(new_entries(&working, &before));
            return Err(ExtractorFailure {
                error,
                output,
                created_paths,
            });
        }
    };
    created_paths.extend(new_entries(&working, &before));
    created_paths.sort();
    created_paths.dedup();

    if raw.cancelled {
        return Err(ExtractorFailure {
            error: ExtractorError::Cancelled,
            output: raw.output,
            created_paths,
        });
    }

    if looks_like_encrypted_package(&raw.output) {
        return Err(ExtractorFailure {
            error: ExtractorError::EncryptedPackage,
            output: raw.output,
            created_paths,
        });
    }

    if !raw.status.success() {
        let error = match raw.status.code() {
            Some(code) => ExtractorError::Exit { code },
            None => ExtractorError::Terminated,
        };
        return Err(ExtractorFailure {
            error,
            output: raw.output,
            created_paths,
        });
    }

    if let Err(message) = request.validate_output.validate(&output) {
        return Err(ExtractorFailure {
            error: ExtractorError::OutputRejected(message),
            output: raw.output,
            created_paths,
        });
    }

    Ok(ExtractorRunOutcome {
        output: raw.output,
        exit_code: raw.status.code(),
        created_paths,
    })
}

/// True when the extractor's template takes the update package itself, as `{update}`.
///
/// A tool without it takes one package per run. The job runner then extracts the update into a
/// temporary folder and merges the two trees itself (design D8).
pub fn template_takes_update(extractor: &Extractor) -> bool {
    extractor
        .args
        .iter()
        .any(|argument| argument == UPDATE_PLACEHOLDER)
}

/// Merges an extracted update tree over the extracted base folder.
///
/// Every file in `update` replaces, or joins, the file at the same relative path below `base`.
/// Nothing in `base` is deleted. Each file is renamed into place when both folders share a volume,
/// so the base is never copied. Both trees come from an untrusted package: symbolic links, reparse
/// points and paths that resolve outside `base` are rejected, and cancellation is polled between
/// entries.
pub fn merge_update_tree(
    update: &Path,
    base: &Path,
    cancel: &CancelFlag,
) -> Result<(), ExtractorError> {
    let update_root = canonical_dir(update, "update folder")?;
    let base_root = canonical_dir(base, "game folder")?;
    if update_root == base_root
        || update_root.starts_with(&base_root)
        || base_root.starts_with(&update_root)
    {
        return Err(ExtractorError::InvalidPath {
            role: "update folder",
            reason: format!(
                "{} and {} must be separate, non-nested folders",
                update_root.display(),
                base_root.display()
            ),
        });
    }
    merge_directory(&update_root, &base_root, Path::new(""), cancel)
}

/// Overlays one directory of the update tree onto the same relative directory in the base.
fn merge_directory(
    update_root: &Path,
    base_root: &Path,
    relative: &Path,
    cancel: &CancelFlag,
) -> Result<(), ExtractorError> {
    let directory = if relative.as_os_str().is_empty() {
        update_root.to_path_buf()
    } else {
        update_root.join(relative)
    };
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory)
        .map_err(|error| ExtractorError::Io(format!("{}: {error}", directory.display())))?
    {
        entries
            .push(entry.map_err(|error| {
                ExtractorError::Io(format!("{}: {error}", directory.display()))
            })?);
    }
    for entry in entries {
        if cancel.is_cancelled() {
            return Err(ExtractorError::Cancelled);
        }
        let child = relative.join(entry.file_name());
        check_update_relative(&child)?;
        let source = update_root.join(&child);
        let metadata = fs::symlink_metadata(&source)
            .map_err(|error| ExtractorError::Io(format!("{}: {error}", source.display())))?;
        if is_link_or_reparse(&metadata) {
            return Err(unsafe_update_path(
                &child,
                "symbolic links and reparse points are not merged",
            ));
        }
        let destination = base_root.join(&child);
        if metadata.file_type().is_dir() {
            ensure_update_directory(base_root, &child)?;
            merge_directory(update_root, base_root, &child, cancel)?;
        } else if metadata.file_type().is_file() {
            let parent = child.parent().unwrap_or_else(|| Path::new(""));
            ensure_update_directory(base_root, parent)?;
            move_update_file(&source, &destination, &child)?;
        } else {
            return Err(unsafe_update_path(
                &child,
                "only regular files and folders are merged",
            ));
        }
    }
    Ok(())
}

/// Rejects a relative path that is empty or has a component other than a plain name.
fn check_update_relative(relative: &Path) -> Result<(), ExtractorError> {
    use std::path::Component;
    let safe = !relative.as_os_str().is_empty()
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if safe {
        Ok(())
    } else {
        Err(unsafe_update_path(
            relative,
            "the path escapes the update folder",
        ))
    }
}

/// Creates the folders of `relative` below `base`, refusing links on the way.
///
/// Each component is checked before the next one is created, so a link already present in the
/// base tree can never redirect a write outside the game folder.
fn ensure_update_directory(base_root: &Path, relative: &Path) -> Result<(), ExtractorError> {
    use std::path::Component;
    let mut current = base_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(unsafe_update_path(
                relative,
                "the path escapes the update folder",
            ));
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if is_link_or_reparse(&metadata) {
                    return Err(unsafe_update_path(
                        relative,
                        "the game folder contains a symbolic link or reparse point",
                    ));
                }
                if !metadata.file_type().is_dir() {
                    return Err(unsafe_update_path(
                        relative,
                        "the update has a folder where the game folder has a file",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| {
                    ExtractorError::Io(format!("{}: {error}", current.display()))
                })?;
            }
            Err(error) => {
                return Err(ExtractorError::Io(format!(
                    "{}: {error}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

/// Puts one update file at its place in the base, replacing an existing file.
fn move_update_file(
    source: &Path,
    destination: &Path,
    relative: &Path,
) -> Result<(), ExtractorError> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            if is_link_or_reparse(&metadata) {
                return Err(unsafe_update_path(
                    relative,
                    "the game folder contains a symbolic link or reparse point",
                ));
            }
            if metadata.file_type().is_dir() {
                return Err(unsafe_update_path(
                    relative,
                    "the update has a file where the game folder has a folder",
                ));
            }
            if !metadata.file_type().is_file() {
                return Err(unsafe_update_path(
                    relative,
                    "the game folder contains an unsupported entry",
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ExtractorError::Io(format!(
                "{}: {error}",
                destination.display()
            )));
        }
    }
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            // A rename can fail across volumes or while another process holds the file open.
            // Copying only ever touches the few files an update package contains.
            fs::copy(source, destination).map_err(|error| {
                ExtractorError::Io(format!(
                    "{} -> {}: {rename_error}; copy failed: {error}",
                    source.display(),
                    destination.display()
                ))
            })?;
            fs::remove_file(source)
                .map_err(|error| ExtractorError::Io(format!("{}: {error}", source.display())))?;
            Ok(())
        }
    }
}

fn unsafe_update_path(path: &Path, reason: &str) -> ExtractorError {
    ExtractorError::UnsafeUpdatePath {
        path: path.display().to_string(),
        reason: reason.to_string(),
    }
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// One completed tool process.
struct RawRun {
    status: ExitStatus,
    cancelled: bool,
    output: String,
}

/// Paths a placeholder can stand for.
struct PlaceholderPaths<'a> {
    package: &'a Path,
    update: Option<&'a Path>,
    output: &'a Path,
}

/// Builds the argument list from the overlay template.
///
/// Literal arguments are passed unchanged. A placeholder must be an argument of its own and is
/// replaced by its path as one separate argument; `{update}` needs an update package. Anything
/// else with braces is rejected, which is how a template cannot smuggle text into a path
/// (placeholder injection).
fn expand_arguments(
    template: &[String],
    paths: &PlaceholderPaths<'_>,
) -> Result<Vec<OsString>, ExtractorError> {
    let mut arguments = Vec::with_capacity(template.len());
    for argument in template {
        if argument.contains('\0') {
            return Err(ExtractorError::InvalidTemplate {
                reason: "an argument contains a NUL byte".into(),
            });
        }
        if !argument.contains(['{', '}']) {
            arguments.push(OsString::from(argument.as_str()));
            continue;
        }
        let path = match argument.as_str() {
            PACKAGE_PLACEHOLDER => paths.package,
            OUTPUT_PLACEHOLDER => paths.output,
            UPDATE_PLACEHOLDER => paths.update.ok_or(ExtractorError::MissingUpdate)?,
            other => {
                let embedded = [PACKAGE_PLACEHOLDER, UPDATE_PLACEHOLDER, OUTPUT_PLACEHOLDER]
                    .iter()
                    .any(|placeholder| other.contains(placeholder));
                let reason = if embedded {
                    format!(
                        "the argument `{other}` embeds a placeholder in other text; a placeholder must be an argument of its own"
                    )
                } else {
                    format!("unknown placeholder in the argument `{other}`")
                };
                return Err(ExtractorError::InvalidTemplate { reason });
            }
        };
        arguments.push(path.as_os_str().to_owned());
    }
    Ok(arguments)
}

/// Removes extraction staging folders left behind by a crashed earlier attempt.
fn remove_stale_extractions(tool_root: &Path) {
    let Ok(entries) = fs::read_dir(tool_root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(".extract-") {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Returns the executable path relative to the tool folder, rejecting an unsafe value.
fn executable_relative_path(extractor: &Extractor) -> Result<PathBuf, ExtractorError> {
    if !gamenode_core::ports::is_safe_relative_path(&extractor.executable) {
        return Err(ExtractorError::InvalidPath {
            role: "executable",
            reason: format!("`{}` is not a safe relative path", extractor.executable),
        });
    }
    Ok(PathBuf::from(extractor.executable.replace('\\', "/")))
}

/// Builds the GitHub release asset URL for the pinned tool.
fn release_asset_url(extractor: &Extractor) -> Result<String, ExtractorError> {
    let repository = extractor
        .repository
        .trim()
        .trim_end_matches('/')
        .trim_start_matches("https://github.com/")
        .trim_start_matches("http://github.com/");
    let mut parts = repository.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty()
        || name.is_empty()
        || parts.next().is_some()
        || !safe_url_segment(owner)
        || !safe_url_segment(name)
        || !safe_url_segment(&extractor.tag)
    {
        return Err(ExtractorError::InvalidExtractor {
            id: extractor.id.clone(),
        });
    }
    let mut url = url::Url::parse(&format!(
        "https://github.com/{owner}/{name}/releases/download"
    ))
    .map_err(|e| ExtractorError::InvalidExtractor {
        id: format!("{} ({e})", extractor.id),
    })?;
    url.path_segments_mut()
        .map_err(|_| ExtractorError::InvalidExtractor {
            id: extractor.id.clone(),
        })?
        .push(&extractor.tag)
        .push(extractor.asset.trim());
    Ok(url.to_string())
}

fn safe_url_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.contains(['/', '\\', '?', '#'])
        && !segment.chars().any(char::is_control)
}

/// Runs the tool to completion, capturing its output and killing its tree on cancellation.
fn spawn_and_wait(
    executable: &Path,
    arguments: &[OsString],
    working_directory: &Path,
    cancel: &CancelFlag,
) -> Result<RawRun, (ExtractorError, String)> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_command(&mut command);

    let mut child = command
        .spawn()
        .map_err(|e| (ExtractorError::Spawn(e.to_string()), String::new()))?;
    let mut tree = ChildTree::attach(&child);
    let capture = Arc::new(OutputCapture::new());
    let (readers, readers_done) = read_pipes(&mut child, &capture);

    let mut cancelled = false;
    let status = loop {
        if cancel.is_cancelled() {
            cancelled = true;
            tree.kill();
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                // Losing the child handle must not leave it running.
                tree.kill();
                let _ = child.wait();
                join_readers(readers, &readers_done);
                return Err((ExtractorError::Io(error.to_string()), capture.text()));
            }
        }
        std::thread::sleep(CANCEL_POLL_INTERVAL);
    };
    join_readers(readers, &readers_done);
    tree.finish();

    Ok(RawRun {
        status,
        cancelled,
        output: capture.text(),
    })
}

fn read_pipes(
    child: &mut Child,
    capture: &Arc<OutputCapture>,
) -> (Vec<JoinHandle<()>>, std::sync::mpsc::Receiver<()>) {
    let (done, finished) = std::sync::mpsc::channel();
    let mut readers = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let capture = Arc::clone(capture);
        let done = done.clone();
        readers.push(std::thread::spawn(move || {
            capture.drain(&mut stdout);
            let _ = done.send(());
        }));
    }
    if let Some(mut stderr) = child.stderr.take() {
        let capture = Arc::clone(capture);
        readers.push(std::thread::spawn(move || {
            capture.drain(&mut stderr);
            let _ = done.send(());
        }));
    }
    (readers, finished)
}

/// Joins the pipe readers, but never waits for more than [`READER_JOIN_TIMEOUT`] in total.
///
/// A tool that leaves a child process holding its stdout open would otherwise block the job long
/// after the tool itself exited. An abandoned reader keeps draining into its own `Arc` and is
/// detached rather than waited for.
fn join_readers(readers: Vec<JoinHandle<()>>, finished: &std::sync::mpsc::Receiver<()>) {
    let deadline = std::time::Instant::now() + READER_JOIN_TIMEOUT;
    for _ in 0..readers.len() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        if finished.recv_timeout(remaining).is_err() {
            break;
        }
    }
    if readers.iter().any(|reader| !reader.is_finished()) {
        tracing::warn!("an extraction tool left a process holding its output open");
    }
    drop(readers);
}

/// Keeps the last [`MAX_CAPTURED_OUTPUT_BYTES`] bytes written by both pipes.
struct OutputCapture {
    inner: Mutex<CaptureBuffer>,
}

struct CaptureBuffer {
    bytes: Vec<u8>,
    start: usize,
}

impl OutputCapture {
    fn new() -> Self {
        Self {
            inner: Mutex::new(CaptureBuffer {
                bytes: Vec::new(),
                start: 0,
            }),
        }
    }

    fn push(&self, chunk: &[u8]) {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        inner.bytes.extend_from_slice(chunk);
        let len = inner.bytes.len();
        if len - inner.start > MAX_CAPTURED_OUTPUT_BYTES {
            inner.start = len - MAX_CAPTURED_OUTPUT_BYTES;
        }
        // Compact once the discarded prefix is worth the move.
        if inner.start > MAX_CAPTURED_OUTPUT_BYTES {
            let start = inner.start;
            inner.bytes.drain(..start);
            inner.start = 0;
        }
    }

    fn text(&self) -> String {
        let inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        String::from_utf8_lossy(&inner.bytes[inner.start..]).into_owned()
    }

    fn drain(&self, reader: &mut impl Read) {
        let mut buffer = [0u8; OUTPUT_READ_CHUNK];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => self.push(&buffer[..n]),
                // A broken pipe means the child closed its end; there is nothing left to read.
                Err(_) => break,
            }
        }
    }
}

#[cfg(unix)]
fn configure_command(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // A new process group makes the child and all its descendants killable together.
    command.process_group(0);
}

#[cfg(windows)]
fn configure_command(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(unix)]
struct ChildTree {
    pgid: Option<rustix::process::Pid>,
    armed: bool,
}

#[cfg(unix)]
impl ChildTree {
    fn attach(child: &Child) -> Self {
        Self {
            pgid: rustix::process::Pid::from_raw(child.id() as i32),
            armed: true,
        }
    }

    fn kill(&self) {
        if let Some(pgid) = self.pgid {
            // ESRCH is expected when the group already exited.
            let _ = rustix::process::kill_process_group(pgid, rustix::process::Signal::KILL);
        }
    }

    fn finish(&mut self) {
        self.armed = false;
    }
}

#[cfg(unix)]
impl Drop for ChildTree {
    fn drop(&mut self) {
        if self.armed {
            self.kill();
        }
    }
}

#[cfg(windows)]
struct ChildTree {
    job: windows_sys::Win32::Foundation::HANDLE,
    pid: u32,
    armed: bool,
}

#[cfg(windows)]
impl ChildTree {
    fn attach(child: &Child) -> Self {
        use std::os::windows::io::AsRawHandle;
        use std::ptr;
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        let pid = child.id();
        // SAFETY: a nameless job object with default security; the returned handle is owned here.
        let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job.is_null() {
            tracing::warn!(pid, "could not create the extractor job object");
            return Self {
                job: ptr::null_mut(),
                pid,
                armed: true,
            };
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `limits` is a correctly sized, initialized structure for the requested class.
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        // SAFETY: both handles are valid: the job was just created and the child owns its handle.
        let assigned = if configured != 0 {
            unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) }
        } else {
            0
        };
        if configured == 0 || assigned == 0 {
            tracing::warn!(
                pid,
                "could not put the extractor in a job object; falling back to terminating it"
            );
            // SAFETY: the job is owned here and no longer used.
            unsafe { CloseHandle(job) };
            return Self {
                job: ptr::null_mut(),
                pid,
                armed: true,
            };
        }
        Self {
            job,
            pid,
            armed: true,
        }
    }

    fn kill(&self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_TERMINATE, TerminateProcess,
        };

        if !self.job.is_null() {
            // SAFETY: the job handle is owned by this value and valid for its lifetime.
            unsafe { TerminateJobObject(self.job, 1) };
            return;
        }
        if self.pid == 0 {
            return;
        }
        // SAFETY: the pid was reported by the child process; the handle is closed after use.
        let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, self.pid) };
        if process.is_null() {
            return;
        }
        // SAFETY: the handle was just opened with PROCESS_TERMINATE.
        unsafe { TerminateProcess(process, 1) };
        // SAFETY: the handle is owned here and no longer used.
        unsafe { CloseHandle(process) };
    }

    fn finish(&mut self) {
        self.armed = false;
    }
}

#[cfg(windows)]
impl Drop for ChildTree {
    fn drop(&mut self) {
        if self.armed {
            self.kill();
        }
        if !self.job.is_null() {
            // SAFETY: the job handle is owned here and closed exactly once.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.job) };
        }
    }
}

/// Canonicalizes `path` and requires it to be a file.
fn canonical_file(path: &Path, role: &'static str) -> Result<PathBuf, ExtractorError> {
    let canonical = canonical(path, role)?;
    if !canonical.is_file() {
        return Err(ExtractorError::InvalidPath {
            role,
            reason: format!("{} is not an existing file", canonical.display()),
        });
    }
    Ok(canonical)
}

/// Canonicalizes `path` and requires it to be a directory.
fn canonical_dir(path: &Path, role: &'static str) -> Result<PathBuf, ExtractorError> {
    let canonical = canonical(path, role)?;
    if !canonical.is_dir() {
        return Err(ExtractorError::InvalidPath {
            role,
            reason: format!("{} is not an existing folder", canonical.display()),
        });
    }
    Ok(canonical)
}

/// Canonicalizes an absolute path, rejecting NUL bytes and Windows verbatim prefixes.
fn canonical(path: &Path, role: &'static str) -> Result<PathBuf, ExtractorError> {
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(ExtractorError::InvalidPath {
            role,
            reason: "the path contains a NUL byte".into(),
        });
    }
    if !path.is_absolute() {
        return Err(ExtractorError::InvalidPath {
            role,
            reason: format!("{} is not an absolute path", path.display()),
        });
    }
    let canonical = fs::canonicalize(path).map_err(|error| ExtractorError::InvalidPath {
        role,
        reason: format!("{}: {error}", path.display()),
    })?;
    Ok(strip_verbatim_prefix(canonical))
}

#[cfg(windows)]
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    PathBuf::from(crate::host::strip_verbatim_prefix(
        path.to_string_lossy().into_owned(),
    ))
}

#[cfg(not(windows))]
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    path
}

/// The entries directly inside `folder`, used to see what a tool created.
fn top_level_entries(folder: &Path) -> BTreeSet<PathBuf> {
    fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect()
}

fn new_entries(folder: &Path, before: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    top_level_entries(folder)
        .difference(before)
        .cloned()
        .collect()
}

/// Heuristic for the message a console package extractor prints for an encrypted retail package.
///
/// GameNode ships no keys, so this is mapped to a clear error instead of a generic exit code.
fn looks_like_encrypted_package(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    const MARKERS: [&str; 10] = [
        "encrypted package",
        "encrypted pkg",
        "package is encrypted",
        "pkg is encrypted",
        "encrypted retail",
        "ekpfs",
        "decryption key",
        "passcode",
        "no key",
        "missing key",
    ];
    if MARKERS.iter().any(|marker| lower.contains(marker)) {
        return true;
    }
    (lower.contains("encrypt") || lower.contains("decrypt"))
        && (lower.contains("pkg") || lower.contains("package"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::ports::{Extractor, SetupFile, SetupKind};
    use std::io::Write;
    use std::sync::atomic::AtomicUsize;

    use crate::download::{DownloadFetcher, DownloadUrlPolicy, compute_bytes_sha256};

    struct FakeFetcher {
        bytes: Arc<Vec<u8>>,
        calls: Arc<AtomicUsize>,
    }

    impl FakeFetcher {
        fn new(bytes: Vec<u8>) -> (Self, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    bytes: Arc::new(bytes),
                    calls: Arc::clone(&calls),
                },
                calls,
            )
        }

        fn calls(calls: &Arc<AtomicUsize>) -> usize {
            calls.load(Ordering::Relaxed)
        }
    }

    impl Clone for FakeFetcher {
        fn clone(&self) -> Self {
            Self {
                bytes: Arc::clone(&self.bytes),
                calls: Arc::clone(&self.calls),
            }
        }
    }

    impl DownloadFetcher for FakeFetcher {
        fn fetch(&self, _url: &str, destination: &Path) -> Result<(), ToolError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            fs::write(destination, self.bytes.as_slice()).map_err(|e| ToolError::Io(e.to_string()))
        }
    }

    struct AllowAll;

    impl DownloadUrlPolicy for AllowAll {
        fn is_allowed(&self, _url: &str) -> bool {
            true
        }
    }

    #[cfg(unix)]
    fn accept_all() -> impl OutputFolderValidator {
        |_: &Path| Ok(())
    }

    fn extractor(args: Vec<String>) -> Extractor {
        Extractor {
            id: "ps4-pkg".into(),
            name: "PS4 package extractor".into(),
            repository: "owner/repo".into(),
            tag: "v1.0.0".into(),
            asset: "tool.zip".into(),
            sha256: "0".repeat(64),
            license: "GPL-3.0".into(),
            executable: "tool".into(),
            args,
        }
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    /// The stub "tool" on Unix is `/bin/sh`. The script travels as a template argument, so the test
    /// exercises the extractor's own argument handling and never has to write an executable file
    /// (which races with other tests forking while the file is still open).
    #[cfg(unix)]
    fn shell_tool() -> InstalledTool {
        InstalledTool {
            root: PathBuf::from("/bin"),
            executable: PathBuf::from("/bin/sh"),
        }
    }

    /// A template that runs `script` with `$1` = package and `$2` = output.
    #[cfg(unix)]
    fn shell_args(script: &str) -> Vec<String> {
        vec![
            "-c".into(),
            script.into(),
            "stub".into(),
            "{package}".into(),
            "{out}".into(),
        ]
    }

    #[test]
    fn release_asset_url_accepts_owner_repository_and_full_urls() {
        let extractor = extractor(Vec::new());
        assert_eq!(
            release_asset_url(&extractor).unwrap(),
            "https://github.com/owner/repo/releases/download/v1.0.0/tool.zip"
        );

        let mut full = extractor.clone();
        full.repository = "https://github.com/owner/repo/".into();
        assert_eq!(
            release_asset_url(&full).unwrap(),
            "https://github.com/owner/repo/releases/download/v1.0.0/tool.zip"
        );

        let mut extra = extractor.clone();
        extra.repository = "owner/repo/extra".into();
        assert!(matches!(
            release_asset_url(&extra),
            Err(ExtractorError::InvalidExtractor { .. })
        ));

        let mut escaping = extractor.clone();
        escaping.tag = "../v1".into();
        assert!(matches!(
            release_asset_url(&escaping),
            Err(ExtractorError::InvalidExtractor { .. })
        ));
    }

    #[test]
    fn embedded_and_unknown_placeholders_are_rejected() {
        let package = Path::new("/tmp/package.pkg");
        let output = Path::new("/tmp/out");
        let paths = PlaceholderPaths {
            package,
            update: None,
            output,
        };

        assert!(matches!(
            expand_arguments(&["{unknown}".into()], &paths),
            Err(ExtractorError::InvalidTemplate { .. })
        ));
        assert!(matches!(
            expand_arguments(&["--out={out}".into()], &paths),
            Err(ExtractorError::InvalidTemplate { .. })
        ));
        assert!(matches!(
            expand_arguments(&["{package}\0x".into()], &paths),
            Err(ExtractorError::InvalidTemplate { .. })
        ));
        assert!(matches!(
            expand_arguments(&["{update}".into()], &paths),
            Err(ExtractorError::MissingUpdate)
        ));

        let arguments = expand_arguments(
            &[
                "{package}".into(),
                "{out}".into(),
                "--literal".into(),
                "{package_suffix}".into(),
            ],
            &paths,
        );
        assert!(matches!(
            arguments,
            Err(ExtractorError::InvalidTemplate { .. })
        ));

        let arguments = expand_arguments(&["{package}".into(), "{out}".into()], &paths)
            .expect("known placeholders");
        assert_eq!(
            arguments,
            vec![
                OsString::from("/tmp/package.pkg"),
                OsString::from("/tmp/out")
            ]
        );
    }

    #[test]
    fn install_tool_verifies_the_checksum_extracts_and_caches() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("tool.zip");
        write_zip(
            &archive,
            &[("tool", b"#!/bin/sh\nexit 0\n"), ("readme", b"hi")],
        );
        let bytes = fs::read(&archive).unwrap();
        let (fetcher, calls) = FakeFetcher::new(bytes.clone());

        let mut definition = extractor(Vec::new());
        definition.sha256 = compute_bytes_sha256(&bytes);
        let runner = ExtractorRunner::with_downloader(
            temp.path(),
            Box::new(fetcher.clone()),
            Box::new(AllowAll),
        );
        let tool = runner
            .install_tool(&definition, &CancelFlag::new())
            .expect("tool installed");
        assert_eq!(fs::read(&tool.executable).unwrap(), b"#!/bin/sh\nexit 0\n");
        assert!(tool.root.ends_with(Path::new("tool")));
        // The verified archive is cached next to the tool.
        assert!(
            temp.path()
                .join("ports/tools/ps4-pkg/v1.0.0/download/tool.zip")
                .is_file()
        );

        runner
            .install_tool(&definition, &CancelFlag::new())
            .expect("cached tool reused");
        assert_eq!(
            FakeFetcher::calls(&calls),
            1,
            "second install used the cache"
        );
    }

    #[test]
    fn install_tool_rejects_a_checksum_mismatch_and_leaves_no_executable() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("tool.zip");
        write_zip(&archive, &[("tool", b"binary")]);
        let (fetcher, _calls) = FakeFetcher::new(fs::read(&archive).unwrap());

        let definition = extractor(Vec::new());
        let runner =
            ExtractorRunner::with_downloader(temp.path(), Box::new(fetcher), Box::new(AllowAll));
        let failure = runner
            .install_tool(&definition, &CancelFlag::new())
            .expect_err("wrong hash");
        assert!(matches!(
            failure.error,
            ExtractorError::Tool(ToolError::ChecksumMismatch { .. })
        ));
        assert!(!temp.path().join("ports/tools/ps4-pkg/v1.0.0/tool").exists());
        assert!(
            !temp
                .path()
                .join("ports/tools/ps4-pkg/v1.0.0/download/tool.zip.part")
                .exists()
        );
    }

    #[test]
    fn install_tool_reports_a_missing_executable() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("tool.zip");
        write_zip(&archive, &[("something-else", b"binary")]);
        let bytes = fs::read(&archive).unwrap();
        let (fetcher, _calls) = FakeFetcher::new(bytes.clone());

        let mut definition = extractor(Vec::new());
        definition.sha256 = compute_bytes_sha256(&bytes);
        let runner =
            ExtractorRunner::with_downloader(temp.path(), Box::new(fetcher), Box::new(AllowAll));
        let failure = runner
            .install_tool(&definition, &CancelFlag::new())
            .expect_err("missing executable");
        assert!(matches!(
            failure.error,
            ExtractorError::ExecutableMissing { .. }
        ));
        assert!(!temp.path().join("ports/tools/ps4-pkg/v1.0.0/tool").exists());
    }

    #[test]
    fn invalid_extractor_definitions_fail_before_downloading() {
        let temp = tempfile::tempdir().unwrap();
        let (fetcher, calls) = FakeFetcher::new(Vec::new());
        let runner =
            ExtractorRunner::with_downloader(temp.path(), Box::new(fetcher), Box::new(AllowAll));
        let mut definition = extractor(Vec::new());
        definition.sha256 = "abc".into();
        let failure = runner
            .install_tool(&definition, &CancelFlag::new())
            .expect_err("invalid definition");
        assert!(matches!(
            failure.error,
            ExtractorError::InvalidExtractor { .. }
        ));
        assert_eq!(FakeFetcher::calls(&calls), 0);
    }

    #[cfg(unix)]
    #[test]
    fn arguments_are_passed_without_a_shell() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("pkg $(touch pwned).pkg");
        fs::write(&package, b"pkg").unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        let output = temp.path().join("out");

        // `$1` is the package, `$2` the output and `$3` the literal template argument.
        let script = r#"printf '%s\n' "$1" "$2" "$3" > "$2/args.txt""#;
        let mut definition = extractor(shell_args(script));
        definition.args.push("$(touch pwned-2)".into());
        definition.executable = "stub".into();
        let outcome = run_extractor(ExtractorRunRequest {
            extractor: &definition,
            tool: &shell_tool(),
            package: &package,
            update: None,
            output: &output,
            working_directory: &staging,
            cancel: &CancelFlag::new(),
            validate_output: &accept_all(),
        })
        .expect("stub ran");

        let recorded = fs::read_to_string(output.join("args.txt")).unwrap();
        let lines: Vec<&str> = recorded.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(PathBuf::from(lines[0]), fs::canonicalize(&package).unwrap());
        assert_eq!(PathBuf::from(lines[1]), fs::canonicalize(&output).unwrap());
        assert_eq!(lines[2], "$(touch pwned-2)");
        assert!(!staging.join("pwned").exists());
        assert!(!staging.join("pwned-2").exists());
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(
            outcome.created_paths,
            vec![fs::canonicalize(&output).unwrap()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn update_placeholder_is_replaced_by_its_own_argument() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("base.pkg");
        let update = temp.path().join("update.pkg");
        fs::write(&package, b"base").unwrap();
        fs::write(&update, b"update").unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        let output = temp.path().join("out");

        let script = r#"printf '%s\n' "$1" "$2" "$3" > "$3/args.txt""#;
        let mut template = shell_args(script);
        template.insert(4, "{update}".into());
        let definition = extractor(template);
        run_extractor(ExtractorRunRequest {
            extractor: &definition,
            tool: &shell_tool(),
            package: &package,
            update: Some(&update),
            output: &output,
            working_directory: &staging,
            cancel: &CancelFlag::new(),
            validate_output: &accept_all(),
        })
        .expect("stub ran");

        let recorded = fs::read_to_string(output.join("args.txt")).unwrap();
        let lines: Vec<&str> = recorded.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(PathBuf::from(lines[0]), fs::canonicalize(&package).unwrap());
        assert_eq!(PathBuf::from(lines[1]), fs::canonicalize(&update).unwrap());
        assert_eq!(PathBuf::from(lines[2]), fs::canonicalize(&output).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn output_is_capped_at_64_kib_and_keeps_the_tail() {
        let temp = tempfile::tempdir().unwrap();
        let script = r#"i=0
while [ $i -lt 4000 ]; do
  echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  i=$((i+1))
done
echo TAIL-MARKER"#;
        let package = temp.path().join("base.pkg");
        fs::write(&package, b"base").unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        let output = temp.path().join("out");

        let definition = extractor(shell_args(script));
        let outcome = run_extractor(ExtractorRunRequest {
            extractor: &definition,
            tool: &shell_tool(),
            package: &package,
            update: None,
            output: &output,
            working_directory: &staging,
            cancel: &CancelFlag::new(),
            validate_output: &accept_all(),
        })
        .expect("stub ran");

        assert_eq!(outcome.output.len(), MAX_CAPTURED_OUTPUT_BYTES);
        assert!(outcome.output.ends_with("TAIL-MARKER\n"));
        assert!(outcome.output.starts_with('a'));
    }

    #[cfg(unix)]
    #[test]
    fn failure_reports_exit_code_output_and_created_paths() {
        let temp = tempfile::tempdir().unwrap();
        let script = r#"echo junk > cwd-junk.txt
mkdir -p "$2"
echo inside > "$2/inside.txt"
echo "boom: unsupported package" >&2
exit 3"#;
        let package = temp.path().join("base.pkg");
        fs::write(&package, b"base").unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        let output = temp.path().join("out");

        let definition = extractor(shell_args(script));
        let failure = run_extractor(ExtractorRunRequest {
            extractor: &definition,
            tool: &shell_tool(),
            package: &package,
            update: None,
            output: &output,
            working_directory: &staging,
            cancel: &CancelFlag::new(),
            validate_output: &accept_all(),
        })
        .expect_err("non-zero exit");

        assert!(matches!(failure.error, ExtractorError::Exit { code: 3 }));
        assert!(failure.output.contains("boom: unsupported package"));
        let canonical_output = fs::canonicalize(&output).unwrap();
        assert!(failure.created_paths.contains(&canonical_output));
        assert!(
            failure
                .created_paths
                .contains(&staging.join("cwd-junk.txt"))
        );

        // The job runner can delete exactly what was reported.
        for path in &failure.created_paths {
            if path.is_dir() {
                fs::remove_dir_all(path).unwrap();
            } else {
                fs::remove_file(path).unwrap();
            }
        }
        assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
        assert!(!output.exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_encrypted_package_gets_a_clear_error() {
        let temp = tempfile::tempdir().unwrap();
        let script = r#"echo "error: this is an encrypted retail package (EKPFS)" >&2
exit 1"#;
        let package = temp.path().join("base.pkg");
        fs::write(&package, b"base").unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        let output = temp.path().join("out");

        let definition = extractor(shell_args(script));
        let failure = run_extractor(ExtractorRunRequest {
            extractor: &definition,
            tool: &shell_tool(),
            package: &package,
            update: None,
            output: &output,
            working_directory: &staging,
            cancel: &CancelFlag::new(),
            validate_output: &accept_all(),
        })
        .expect_err("encrypted package");

        assert!(matches!(failure.error, ExtractorError::EncryptedPackage));
        assert!(failure.reason().contains("encrypted"));
        assert!(failure.reason().contains("no keys"));
    }

    #[cfg(unix)]
    #[test]
    fn an_output_folder_that_fails_validation_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("base.pkg");
        fs::write(&package, b"base").unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        let output = temp.path().join("out");

        let definition = extractor(shell_args("echo done"));
        let reject = |_: &Path| Err(String::from("missing eboot.bin"));
        let failure = run_extractor(ExtractorRunRequest {
            extractor: &definition,
            tool: &shell_tool(),
            package: &package,
            update: None,
            output: &output,
            working_directory: &staging,
            cancel: &CancelFlag::new(),
            validate_output: &reject,
        })
        .expect_err("output rejected");

        assert!(matches!(
            failure.error,
            ExtractorError::OutputRejected(ref message) if message == "missing eboot.bin"
        ));
        assert!(
            failure
                .created_paths
                .contains(&fs::canonicalize(&output).unwrap())
        );
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_kills_the_whole_process_tree() {
        let temp = tempfile::tempdir().unwrap();
        let script = r#"(
  while :; do
    echo tick >> "$2/progress.txt"
    sleep 0.05
  done
) &
while :; do sleep 0.05; done"#;
        let package = temp.path().join("base.pkg");
        fs::write(&package, b"base").unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        let output = temp.path().join("out");

        let definition = extractor(shell_args(script));
        let cancel = CancelFlag::new();
        let cancel_from_test = cancel.clone();
        let progress = output.join("progress.txt");
        let progress_in_thread = progress.clone();
        let canceller = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                if progress_in_thread.is_file() {
                    cancel_from_test.cancel();
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the stub never started");
        });

        let failure = run_extractor(ExtractorRunRequest {
            extractor: &definition,
            tool: &shell_tool(),
            package: &package,
            update: None,
            output: &output,
            working_directory: &staging,
            cancel: &cancel,
            validate_output: &accept_all(),
        })
        .expect_err("cancelled");
        canceller.join().unwrap();
        assert!(matches!(failure.error, ExtractorError::Cancelled));

        // The background child of the stub must be dead: it no longer appends progress.
        let after_cancel = fs::metadata(&progress).unwrap().len();
        std::thread::sleep(Duration::from_millis(400));
        let later = fs::metadata(&progress).unwrap().len();
        assert_eq!(later, after_cancel, "the stub's child kept running");
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_tool_executable_fails_before_spawning() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("base.pkg");
        fs::write(&package, b"base").unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        let output = temp.path().join("out");
        let definition = extractor(vec!["{package}".into(), "{out}".into()]);
        let failure = run_extractor(ExtractorRunRequest {
            extractor: &definition,
            tool: &InstalledTool {
                root: temp.path().to_path_buf(),
                executable: temp.path().join("missing-tool"),
            },
            package: &package,
            update: None,
            output: &output,
            working_directory: &staging,
            cancel: &CancelFlag::new(),
            validate_output: &accept_all(),
        })
        .expect_err("missing executable");
        assert!(matches!(
            failure.error,
            ExtractorError::ExecutableMissing { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn an_unsafe_executable_path_is_rejected() {
        let mut definition = extractor(Vec::new());
        definition.executable = "../tool".into();
        assert!(matches!(
            executable_relative_path(&definition),
            Err(ExtractorError::InvalidPath { .. })
        ));
    }

    #[test]
    fn a_relative_output_path_is_rejected_without_creating_anything() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("base.pkg");
        fs::write(&package, b"base").unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        let definition = extractor(vec!["{package}".into(), "{out}".into()]);
        let failure = run_extractor(ExtractorRunRequest {
            extractor: &definition,
            tool: &InstalledTool {
                root: temp.path().to_path_buf(),
                executable: temp.path().join("missing-tool"),
            },
            package: &package,
            update: None,
            output: Path::new("relative/out"),
            working_directory: &staging,
            cancel: &CancelFlag::new(),
            validate_output: &|_: &Path| Ok(()),
        })
        .expect_err("relative output");
        assert!(matches!(
            failure.error,
            ExtractorError::InvalidPath { role: "output", .. }
        ));
        assert!(!Path::new("relative/out").exists());
    }

    #[test]
    fn the_supplied_folder_check_is_reusable_for_extractor_output() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("out");
        fs::create_dir_all(&output).unwrap();
        let slot = SetupFile {
            id: "game".into(),
            label: "Game folder".into(),
            kind: SetupKind::Folder,
            contains: vec!["eboot.bin".into()],
            ..SetupFile::default()
        };
        let cancelled = AtomicBool::new(false);
        let validator = supplied_folder_validator(&slot, &cancelled);
        assert!(
            validator.validate(&output).is_err(),
            "a missing marker is rejected"
        );
        fs::write(output.join("eboot.bin"), b"eboot").unwrap();
        validator.validate(&output).expect("marker present");
    }

    #[test]
    fn cancellation_and_setup_slot_types_stay_compatible() {
        // The job runner keeps the slot and passes it to a validator; make sure the extractor
        // type itself carries no hidden state across threads.
        let slot = SetupFile {
            id: "game".into(),
            ..SetupFile::default()
        };
        assert_eq!(slot.id, "game");
        let cancel = CancelFlag::new();
        let shared = cancel.shared();
        assert!(!shared.load(Ordering::SeqCst));
        cancel.cancel();
        assert!(shared.load(Ordering::SeqCst));
    }

    #[test]
    fn only_an_exact_update_placeholder_means_the_tool_merges_the_update() {
        assert!(template_takes_update(&extractor(vec![
            "{package}".into(),
            "{update}".into(),
            "--out".into(),
            "{out}".into(),
        ])));
        assert!(!template_takes_update(&extractor(vec![
            "{package}".into(),
            "-o".into(),
            "{out}".into(),
            "--overwrite".into(),
        ])));
    }

    #[test]
    fn a_merge_replaces_update_files_adds_new_ones_and_keeps_base_only_files() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("base");
        let update = temp.path().join("update");
        fs::create_dir_all(base.join("data")).unwrap();
        fs::create_dir_all(update.join("data")).unwrap();
        fs::create_dir_all(update.join("sce_sys/changeinfo")).unwrap();
        fs::write(base.join("eboot.bin"), b"base").unwrap();
        fs::write(base.join("data/base-only.bin"), b"keep").unwrap();
        fs::write(base.join("data/replaced.bin"), b"old").unwrap();
        fs::write(update.join("eboot.bin"), b"updated").unwrap();
        fs::write(update.join("data/replaced.bin"), b"new").unwrap();
        fs::write(update.join("data/new.bin"), b"added").unwrap();
        fs::write(update.join("sce_sys/changeinfo/changeinfo.xml"), b"xml").unwrap();

        merge_update_tree(&update, &base, &CancelFlag::new()).expect("merge");

        assert_eq!(fs::read(base.join("eboot.bin")).unwrap(), b"updated");
        assert_eq!(fs::read(base.join("data/replaced.bin")).unwrap(), b"new");
        assert_eq!(fs::read(base.join("data/new.bin")).unwrap(), b"added");
        assert_eq!(fs::read(base.join("data/base-only.bin")).unwrap(), b"keep");
        assert_eq!(
            fs::read(base.join("sce_sys/changeinfo/changeinfo.xml")).unwrap(),
            b"xml"
        );
        assert!(
            !update.join("eboot.bin").exists(),
            "files are moved out of the update tree"
        );
    }

    #[test]
    fn a_merge_honours_cancellation_before_touching_the_base() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("base");
        let update = temp.path().join("update");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&update).unwrap();
        fs::write(update.join("eboot.bin"), b"updated").unwrap();
        let cancel = CancelFlag::new();
        cancel.cancel();

        let error = merge_update_tree(&update, &base, &cancel).expect_err("cancelled");
        assert!(matches!(error, ExtractorError::Cancelled));
        assert!(!base.join("eboot.bin").exists());
    }

    #[test]
    fn a_merge_rejects_one_tree_nested_inside_the_other() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("base");
        let update = base.join("update");
        fs::create_dir_all(&update).unwrap();

        let error = merge_update_tree(&update, &base, &CancelFlag::new()).expect_err("nested");
        assert!(matches!(
            error,
            ExtractorError::InvalidPath {
                role: "update folder",
                ..
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_merge_rejects_a_symbolic_link_in_the_update_tree() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("base");
        let update = temp.path().join("update");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&update).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, update.join("escape")).unwrap();

        let error = merge_update_tree(&update, &base, &CancelFlag::new()).expect_err("symlink");
        assert!(matches!(error, ExtractorError::UnsafeUpdatePath { .. }));
        assert!(!base.join("escape").exists());
        assert!(outside.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_merge_rejects_a_symbolic_link_in_the_base_tree() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("base");
        let update = temp.path().join("update");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(update.join("escape")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, base.join("escape")).unwrap();
        fs::write(update.join("escape/payload.bin"), b"payload").unwrap();

        let error = merge_update_tree(&update, &base, &CancelFlag::new()).expect_err("symlink");
        assert!(matches!(error, ExtractorError::UnsafeUpdatePath { .. }));
        assert!(!outside.join("payload.bin").exists());
    }
}
