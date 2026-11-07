//! Application logging.
//!
//! The desktop app is a GUI process, so stderr is invisible on Windows. Logs are therefore also
//! written to a rotating file under the data directory:
//!
//! ```text
//! <data dir>/logs/gamenode.<UTC date>.log
//! ```
//!
//! A daily rotation keeps ten days of files. Writes happen on a dedicated worker thread, so a
//! slow disk never blocks the UI or the engine. `GAMENODE_LOG` (or the usual `RUST_LOG`) overrides
//! the default filter without a rebuild.

use std::io::Read;
use std::path::{Path, PathBuf};

use gamenode_platform::paths::{DataPaths, LOG_FILE_PREFIX, LOG_FILE_SUFFIX};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Default filter: our crates at `info`, noisy third-party crates at `warn`.
const DEFAULT_FILTER: &str =
    "warn,gamenode_app=info,gamenode_api=info,gamenode_core=info,gamenode_platform=info";

/// How many daily log files are kept. Older files are removed at startup.
const KEPT_LOG_FILES: usize = 10;

/// Where the logs are, for the Settings screen and diagnostics commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLocation {
    pub directory: PathBuf,
}

impl LogLocation {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// The newest log file, or `None` before the first line is written.
    pub fn latest_file(&self) -> Option<PathBuf> {
        latest_log_file(&self.directory)
    }
}

/// Keeps the non-blocking writer alive. Dropping it flushes pending lines.
pub struct Logging {
    location: LogLocation,
    _guard: Option<WorkerGuard>,
}

impl Logging {
    pub fn location(&self) -> &LogLocation {
        &self.location
    }
}

/// Installs the global subscriber. A failure to open the log file falls back to stderr only; the
/// application must never refuse to start because logging is unavailable.
pub fn init(paths: &DataPaths) -> Logging {
    let directory = paths.logs_dir();
    let appender = open_appender(&directory);
    let (writer, guard) = match appender {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (Some(writer), Some(guard))
        }
        Err(error) => {
            eprintln!(
                "GameNode: could not open the log file in {}: {error}",
                directory.display()
            );
            (None, None)
        }
    };

    let filter = EnvFilter::try_from_env("GAMENODE_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_writer(std::io::stderr);

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer);

    match writer {
        Some(writer) => {
            let file_layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer);
            let _ = registry.with(file_layer).try_init();
        }
        None => {
            let _ = registry.try_init();
        }
    }

    Logging {
        location: LogLocation::new(directory),
        _guard: guard,
    }
}

fn open_appender(directory: &Path) -> std::io::Result<RollingFileAppender> {
    std::fs::create_dir_all(directory)?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(KEPT_LOG_FILES)
        .build(directory)
        .map_err(std::io::Error::other)?;
    Ok(appender)
}

/// The lexicographically newest `gamenode.*.log` file. Dates in the name sort chronologically.
pub fn latest_log_file(directory: &Path) -> Option<PathBuf> {
    let prefix = format!("{LOG_FILE_PREFIX}.");
    let suffix = format!(".{LOG_FILE_SUFFIX}");
    let mut files: Vec<PathBuf> = std::fs::read_dir(directory)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(&suffix))
        })
        .collect();
    files.sort();
    files.pop()
}

/// The last `max_bytes` of the newest log, cut at a line boundary. Intended for the Settings
/// screen's "recent log" view, not for parsing.
pub fn read_recent(directory: &Path, max_bytes: usize) -> std::io::Result<String> {
    let path = latest_log_file(directory)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no log file yet"))?;
    let mut file = std::fs::File::open(&path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(max_bytes as u64);
    if start > 0 {
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(start))?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Ok(text);
    }
    // Drop the partial first line and mark that the excerpt is a tail.
    let body = text.split_once('\n').map(|(_, rest)| rest).unwrap_or(&text);
    Ok(format!("… (earlier lines omitted)\n{body}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_platform::paths::LOGS_DIR;

    #[test]
    fn the_latest_log_is_the_newest_dated_file() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("gamenode.2026-11-06.log"), "old").unwrap();
        std::fs::write(dir.path().join("gamenode.2026-11-07.log"), "new").unwrap();
        std::fs::write(dir.path().join("unrelated.txt"), "x").unwrap();
        assert_eq!(
            latest_log_file(dir.path()).unwrap(),
            dir.path().join("gamenode.2026-11-07.log")
        );
    }

    #[test]
    fn reading_recent_logs_only_returns_the_tail() {
        let dir = tempfile::TempDir::new().unwrap();
        let body = "first line\nsecond line\nthird line\n";
        std::fs::write(dir.path().join("gamenode.2026-11-07.log"), body).unwrap();
        let tail = read_recent(dir.path(), 12).unwrap();
        assert!(tail.starts_with("… (earlier lines omitted)"));
        assert!(tail.ends_with("third line\n"));
        assert!(!tail.contains("first line"));
    }

    #[test]
    fn a_missing_log_is_not_an_error_for_the_caller() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(latest_log_file(dir.path()).is_none());
        assert!(read_recent(dir.path(), 1024).is_err());
    }

    #[test]
    fn the_logs_directory_is_under_the_data_directory() {
        let paths = DataPaths::at("/data");
        assert_eq!(paths.logs_dir(), Path::new("/data").join(LOGS_DIR));
    }
}
