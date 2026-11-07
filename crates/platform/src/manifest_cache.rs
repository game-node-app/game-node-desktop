//! Fixed-source data-only manifest updates, performed on a background worker.
use gamenode_core::drm_free::{LocalError, identity::ManifestIndex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    io::Read,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::SystemTime,
};
pub const SOURCE: &str =
    "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/master/data/manifest.yaml";
#[derive(Clone)]
pub struct ManifestSnapshot {
    pub hash: String,
    pub index: Arc<ManifestIndex>,
}
#[derive(Default, Serialize, Deserialize)]
struct Metadata {
    etag: Option<String>,
    checked: i64,
    hash: String,
}
/// The parsed manifest is large, so every consumer shares one parse until the file changes.
struct Loaded {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    snapshot: ManifestSnapshot,
}
static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);
/// Serializes parses so concurrent callers share one result instead of retaining two copies.
static PARSING: Mutex<()> = Mutex::new(());
thread_local! {
    /// Nested scopes on one worker thread share a parse until the outermost one ends.
    static SCOPES: Cell<usize> = const { Cell::new(0) };
}
fn stamp(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()))
}
fn lock_loaded() -> MutexGuard<'static, Option<Loaded>> {
    LOADED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
fn shared(path: &Path) -> Option<ManifestSnapshot> {
    let (len, modified) = stamp(path)?;
    let loaded = lock_loaded();
    let loaded = loaded.as_ref()?;
    (loaded.path == path && loaded.len == len && loaded.modified == modified)
        .then(|| loaded.snapshot.clone())
}
fn load_file(path: &Path) -> Result<ManifestSnapshot, LocalError> {
    if let Some(snapshot) = shared(path) {
        return Ok(snapshot);
    }
    // Re-check after acquiring the parse lock so a startup race costs one parse, not two.
    let _parsing = PARSING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(snapshot) = shared(path) {
        return Ok(snapshot);
    }
    let stamp = stamp(path);
    let snapshot = ManifestCache::snapshot(&read_bounded(path)?)?;
    #[cfg(test)]
    {
        record_parse(path);
    }
    if let Some((len, modified)) = stamp {
        *lock_loaded() = Some(Loaded {
            path: path.to_owned(),
            len,
            modified,
            snapshot: snapshot.clone(),
        });
    }
    Ok(snapshot)
}
/// Drops the shared parsed manifest so an idle app does not keep it resident.
pub fn release_loaded() {
    forget_loaded();
}
/// Releases the shared parse when dropped, including on early returns.
/// Not `Send`: the scope is meant to live and end on the worker that created it.
pub struct ReleaseOnDrop(PhantomData<*const ()>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let outermost = SCOPES.with(|scopes| {
            let remaining = scopes.get().saturating_sub(1);
            scopes.set(remaining);
            remaining == 0
        });
        if outermost {
            forget_loaded();
        }
    }
}
/// Returns a guard that drops the shared parse once every nested guard has been dropped.
pub fn release_on_drop() -> ReleaseOnDrop {
    SCOPES.with(|scopes| scopes.set(scopes.get() + 1));
    ReleaseOnDrop(PhantomData)
}
fn forget_loaded() {
    *lock_loaded() = None;
}
pub struct ManifestCache {
    pub directory: PathBuf,
    pub baseline: PathBuf,
}
fn failed(e: impl std::fmt::Display) -> LocalError {
    LocalError::Failed(e.to_string())
}
#[cfg(test)]
static PARSES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
#[cfg(test)]
fn record_parse(path: &Path) {
    if let Ok(mut parses) = PARSES.lock() {
        parses.push(path.to_owned());
    }
}
const DAY: i64 = 86_400;
const MAX_BYTES: u64 = 32 * 1024 * 1024;
impl ManifestCache {
    pub fn bundled(data: &Path) -> Result<Self, LocalError> {
        let exe = std::env::current_exe().map_err(failed)?;
        Ok(Self {
            directory: data.join("manifest"),
            baseline: exe
                .parent()
                .ok_or(LocalError::InvalidTarget)?
                .join("backup-assets/manifest.yaml"),
        })
    }
    fn snapshot(bytes: &[u8]) -> Result<ManifestSnapshot, LocalError> {
        let index = ManifestIndex::parse(bytes).map_err(failed)?;
        Ok(ManifestSnapshot {
            hash: format!("{:x}", Sha256::digest(bytes)),
            index: Arc::new(index),
        })
    }
    pub fn load(&self) -> Result<ManifestSnapshot, LocalError> {
        let cached = self.directory.join("manifest.yaml");
        if let Ok(snapshot) = load_file(&cached) {
            return Ok(snapshot);
        }
        load_file(&self.baseline)
    }
    pub fn update(&self, now: i64) -> Result<ManifestSnapshot, LocalError> {
        self.update_from_source(now, SOURCE)
    }
    /// Failed updates always leave the last usable bytes intact. Jobs retain their own Arc snapshot.
    pub fn update_from_source(
        &self,
        now: i64,
        source: &str,
    ) -> Result<ManifestSnapshot, LocalError> {
        let result = self.update_inner(now, source);
        // Nothing may keep the parsed index resident once the caller owns its snapshot.
        forget_loaded();
        result
    }
    fn update_inner(&self, now: i64, source: &str) -> Result<ManifestSnapshot, LocalError> {
        let saved_metadata = self.saved_metadata();
        if is_fresh(&saved_metadata, now) {
            return self.load();
        }
        let current = self.load()?;
        let downloaded = self.download(now, source, saved_metadata.unwrap_or_default())?;
        Ok(downloaded.unwrap_or(current))
    }
    /// Cheap daily check: only a newly downloaded manifest is parsed and written.
    ///
    /// Fresh metadata, offline starts, 304 responses, rejected downloads and parse failures
    /// all return without loading the current file, so no large index becomes resident.
    pub fn refresh(&self, now: i64) -> Result<(), LocalError> {
        self.refresh_from_source(now, SOURCE)
    }
    fn refresh_from_source(&self, now: i64, source: &str) -> Result<(), LocalError> {
        let result = self.refresh_inner(now, source);
        forget_loaded();
        result
    }
    fn refresh_inner(&self, now: i64, source: &str) -> Result<(), LocalError> {
        let saved_metadata = self.saved_metadata();
        if is_fresh(&saved_metadata, now) {
            return Ok(());
        }
        self.download(now, source, saved_metadata.unwrap_or_default())?;
        Ok(())
    }
    fn metadata_path(&self) -> PathBuf {
        self.directory.join("metadata.json")
    }
    fn saved_metadata(&self) -> Option<Metadata> {
        std::fs::read(self.metadata_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    }
    /// Downloads a replacement only when the source changed. Returns it only after it is written.
    fn download(
        &self,
        now: i64,
        source: &str,
        mut metadata: Metadata,
    ) -> Result<Option<ManifestSnapshot>, LocalError> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(failed)?;
        let mut request = client.get(source);
        if let Some(etag) = &metadata.etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        // Persist the attempt time as well, so offline startup cannot create an update storm.
        metadata.checked = now;
        std::fs::create_dir_all(&self.directory).map_err(failed)?;
        atomic_write(
            &self.metadata_path(),
            &serde_json::to_vec(&metadata).map_err(failed)?,
        )?;
        let response = match request.send() {
            Ok(response) => response,
            Err(_) => return Ok(None),
        };
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        if response.status() != reqwest::StatusCode::OK
            || response.content_length().is_some_and(|n| n > MAX_BYTES)
        {
            return Ok(None);
        }
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut bytes = Vec::new();
        response
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(failed)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Ok(None);
        }
        let cached = self.directory.join("manifest.yaml");
        let snapshot = match Self::snapshot(&bytes) {
            Ok(snapshot) => snapshot,
            Err(_) => return Ok(None),
        };
        #[cfg(test)]
        {
            record_parse(&cached);
        }
        atomic_write(&cached, &bytes)?;
        metadata.etag = etag;
        metadata.hash = snapshot.hash.clone();
        atomic_write(
            &self.metadata_path(),
            &serde_json::to_vec(&metadata).map_err(failed)?,
        )?;
        Ok(Some(snapshot))
    }
}
fn is_fresh(metadata: &Option<Metadata>, now: i64) -> bool {
    metadata
        .as_ref()
        .is_some_and(|metadata| now.saturating_sub(metadata.checked) < DAY)
}
fn read_bounded(path: &Path) -> Result<Vec<u8>, LocalError> {
    let file = std::fs::File::open(path).map_err(failed)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(failed)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(failed("manifest exceeds size limit"));
    }
    Ok(bytes)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), LocalError> {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes).map_err(failed)?;
    // std::fs::rename replaces a file atomically on supported Windows and Unix filesystems.
    std::fs::rename(temporary, path).map_err(failed)
}
#[cfg(test)]
mod tests {
    use super::*;

    /// These tests inspect global caches, so they must not observe each other.
    fn serial() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn parses(path: &Path) -> usize {
        PARSES
            .lock()
            .map(|parses| {
                parses
                    .iter()
                    .filter(|entry| entry.as_path() == path)
                    .count()
            })
            .unwrap_or(0)
    }

    fn is_loaded(path: &Path) -> bool {
        lock_loaded()
            .as_ref()
            .is_some_and(|loaded| loaded.path == path)
    }

    fn mock_http_server(response: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{Read, Write};
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(&response);
            }
        });
        (format!("http://{}", addr), handle)
    }

    /// Accepts the request and closes without a response so the client fails fast.
    fn mock_connection_close() -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::Read;
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
            }
        });
        (format!("http://{}", addr), handle)
    }

    fn write_metadata(cache: &ManifestCache, etag: Option<&str>, checked: i64) {
        std::fs::create_dir_all(&cache.directory).unwrap();
        let metadata = Metadata {
            etag: etag.map(str::to_owned),
            checked,
            hash: "h".into(),
        };
        std::fs::write(
            cache.metadata_path(),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn invalid_cache_falls_back_and_snapshot_is_immutable() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: directory.path().join("cache"),
            baseline,
        };
        std::fs::create_dir_all(&cache.directory).unwrap();
        std::fs::write(cache.directory.join("manifest.yaml"), b"invalid").unwrap();
        let old = cache.load().unwrap();
        std::fs::write(
            cache.directory.join("manifest.yaml"),
            b"Other:\n  files: {}\n",
        )
        .unwrap();
        assert!(old.index.games.contains_key("Game"));
        assert!(cache.load().unwrap().index.games.contains_key("Other"));
    }

    #[test]
    fn offline_fallback_and_failed_update_preserves_usable_snapshot() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: directory.path().join("cache"),
            baseline,
        };
        // Unreachable port triggers network error, falling back to current snapshot
        let snapshot = cache
            .update_from_source(100, "http://127.0.0.1:1/manifest.yaml")
            .unwrap();
        assert!(snapshot.index.games.contains_key("Game"));
        assert!(!is_loaded(&cache.directory.join("manifest.yaml")));
        assert!(!is_loaded(&cache.baseline));
    }

    #[test]
    fn not_modified_304_preserves_current_snapshot() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: directory.path().join("cache"),
            baseline,
        };
        // Set metadata with etag
        write_metadata(&cache, Some("\"12345\""), 0);

        let response = b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n".to_vec();
        let (url, handle) = mock_http_server(response);
        // The last check must be over a day old to exercise the HTTP 304 path.
        let snapshot = cache.update_from_source(100_000, &url).unwrap();
        handle.join().unwrap();
        assert!(snapshot.index.games.contains_key("Game"));
        assert!(!is_loaded(&cache.directory.join("manifest.yaml")));
        assert!(!is_loaded(&cache.baseline));
    }

    #[test]
    fn corrupt_and_oversized_download_rejected() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: directory.path().join("cache"),
            baseline,
        };
        let cached = cache.directory.join("manifest.yaml");

        // 1. Corrupt YAML response
        let corrupt_body = b"corrupt: [\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            corrupt_body.len(),
            std::str::from_utf8(corrupt_body).unwrap()
        )
        .into_bytes();
        let (url, handle) = mock_http_server(response);
        let snapshot = cache.update_from_source(100, &url).unwrap();
        handle.join().unwrap();
        assert!(snapshot.index.games.contains_key("Game"));
        assert!(!cached.exists());
        assert!(!is_loaded(&cached));

        // 2. Oversized download by Content-Length header
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 35000000\r\n\r\n".to_vec();
        let (url, handle) = mock_http_server(response);
        let snapshot = cache.update_from_source(200_000, &url).unwrap();
        handle.join().unwrap();
        assert!(snapshot.index.games.contains_key("Game"));
        assert!(!cached.exists());
        assert!(!is_loaded(&cached));
    }

    #[test]
    fn job_retains_original_snapshot_while_update_completes() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"OriginalGame:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: directory.path().join("cache"),
            baseline,
        };
        let cached = cache.directory.join("manifest.yaml");

        // Job loads snapshot
        let job_snapshot = cache.load().unwrap();
        assert!(job_snapshot.index.games.contains_key("OriginalGame"));

        // New update arrives with UpdatedGame
        let new_yaml = b"UpdatedGame:\n  files: {}\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nETag: \"etag-new\"\r\nContent-Length: {}\r\n\r\n{}",
            new_yaml.len(),
            std::str::from_utf8(new_yaml).unwrap()
        )
        .into_bytes();
        let (url, handle) = mock_http_server(response);
        let updated_snapshot = cache.update_from_source(100, &url).unwrap();
        handle.join().unwrap();

        // Job still has original snapshot in its Arc
        assert!(job_snapshot.index.games.contains_key("OriginalGame"));
        assert!(!job_snapshot.index.games.contains_key("UpdatedGame"));

        // Newly activated snapshot has UpdatedGame
        assert!(updated_snapshot.index.games.contains_key("UpdatedGame"));
        assert!(!updated_snapshot.index.games.contains_key("OriginalGame"));
        // The shared parse is gone; only the callers' Arcs keep the indexes alive.
        assert!(!is_loaded(&cached));
    }

    #[test]
    fn concurrent_load_parses_manifest_once() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache_directory = directory.path().join("cache");
        std::fs::create_dir_all(&cache_directory).unwrap();
        let cached = cache_directory.join("manifest.yaml");
        std::fs::write(&cached, b"Concurrent:\n  files: {}\n").unwrap();

        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let cached = cached.clone();
                let baseline = baseline.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let cache = ManifestCache {
                        directory: cached.parent().unwrap().to_owned(),
                        baseline,
                    };
                    barrier.wait();
                    cache.load()
                })
            })
            .collect();
        for handle in handles {
            let snapshot = handle.join().unwrap().unwrap();
            assert!(snapshot.index.games.contains_key("Concurrent"));
        }
        assert_eq!(parses(&cached), 1);
    }

    #[test]
    fn scoped_release_drops_parse_on_normal_and_early_exit() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache_directory = directory.path().join("cache");
        std::fs::create_dir_all(&cache_directory).unwrap();
        let cached = cache_directory.join("manifest.yaml");
        std::fs::write(&cached, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: cache_directory,
            baseline,
        };

        let normal = {
            let _release = release_on_drop();
            cache.load()
        }
        .unwrap();
        assert!(!is_loaded(&cached));
        // The caller's Arc stays usable after the shared parse is released.
        assert!(normal.index.games.contains_key("Game"));

        let early = (|| {
            let _release = release_on_drop();
            cache.load()?;
            assert!(is_loaded(&cached));
            Err::<(), _>(LocalError::Cancelled)
        })();
        assert!(early.is_err());
        assert!(!is_loaded(&cached));
    }

    #[test]
    fn nested_scopes_release_only_when_outermost_ends() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache_directory = directory.path().join("cache");
        std::fs::create_dir_all(&cache_directory).unwrap();
        let cached = cache_directory.join("manifest.yaml");
        std::fs::write(&cached, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: cache_directory,
            baseline,
        };

        let outer = release_on_drop();
        let inner = release_on_drop();
        cache.load().unwrap();
        assert!(is_loaded(&cached));
        drop(inner);
        assert!(is_loaded(&cached));
        drop(outer);
        assert!(!is_loaded(&cached));
    }

    #[test]
    fn fresh_metadata_refresh_skips_parsing() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: directory.path().join("cache"),
            baseline,
        };
        let cached = cache.directory.join("manifest.yaml");
        write_metadata(&cache, Some("\"12345\""), 1_000);

        cache
            .refresh_from_source(1_000, "http://127.0.0.1:1/manifest.yaml")
            .unwrap();
        assert_eq!(parses(&cached), 0);
        assert_eq!(parses(&cache.baseline), 0);
        assert!(!is_loaded(&cached));
    }

    #[test]
    fn not_modified_refresh_releases_shared_parse() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache_directory = directory.path().join("cache");
        std::fs::create_dir_all(&cache_directory).unwrap();
        let cached = cache_directory.join("manifest.yaml");
        std::fs::write(&cached, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: cache_directory,
            baseline,
        };
        write_metadata(&cache, Some("\"12345\""), 0);
        let snapshot = cache.load().unwrap();
        assert!(is_loaded(&cached));

        let response = b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n".to_vec();
        let (url, handle) = mock_http_server(response);
        cache.refresh_from_source(100_000, &url).unwrap();
        handle.join().unwrap();
        assert!(!is_loaded(&cached));
        assert_eq!(parses(&cached), 1);
        assert!(snapshot.index.games.contains_key("Game"));
    }

    #[test]
    fn offline_refresh_releases_shared_parse_without_parsing() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache_directory = directory.path().join("cache");
        std::fs::create_dir_all(&cache_directory).unwrap();
        let cached = cache_directory.join("manifest.yaml");
        std::fs::write(&cached, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: cache_directory,
            baseline,
        };
        write_metadata(&cache, Some("\"12345\""), 0);
        cache.load().unwrap();
        assert!(is_loaded(&cached));

        let (url, handle) = mock_connection_close();
        cache.refresh_from_source(100_000, &url).unwrap();
        handle.join().unwrap();
        assert!(!is_loaded(&cached));
        assert_eq!(parses(&cached), 1);
    }

    #[test]
    fn rejected_refresh_response_preserves_bytes_without_parsing() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache_directory = directory.path().join("cache");
        std::fs::create_dir_all(&cache_directory).unwrap();
        let cached = cache_directory.join("manifest.yaml");
        std::fs::write(&cached, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: cache_directory,
            baseline,
        };
        write_metadata(&cache, None, 0);

        // A 500 response is rejected before any parsing or writing.
        let response = b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_vec();
        let (url, handle) = mock_http_server(response);
        cache.refresh_from_source(100_000, &url).unwrap();
        handle.join().unwrap();
        assert_eq!(parses(&cached), 0);
        assert!(!is_loaded(&cached));
        assert_eq!(std::fs::read(&cached).unwrap(), b"Game:\n  files: {}\n");

        // An unparsable 200 response is rejected without replacing the stored bytes.
        write_metadata(&cache, None, 0);
        let corrupt = b"corrupt: [\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            corrupt.len(),
            std::str::from_utf8(corrupt).unwrap()
        )
        .into_bytes();
        let (url, handle) = mock_http_server(response);
        cache.refresh_from_source(200_000, &url).unwrap();
        handle.join().unwrap();
        assert_eq!(parses(&cached), 0);
        assert!(!is_loaded(&cached));
        assert_eq!(std::fs::read(&cached).unwrap(), b"Game:\n  files: {}\n");
    }

    #[test]
    fn successful_refresh_parses_once_and_releases() {
        let _serial = serial();
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache_directory = directory.path().join("cache");
        std::fs::create_dir_all(&cache_directory).unwrap();
        let cached = cache_directory.join("manifest.yaml");
        std::fs::write(&cached, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: cache_directory,
            baseline,
        };
        write_metadata(&cache, None, 0);

        let new_yaml = b"UpdatedGame:\n  files: {}\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nETag: \"etag-new\"\r\nContent-Length: {}\r\n\r\n{}",
            new_yaml.len(),
            std::str::from_utf8(new_yaml).unwrap()
        )
        .into_bytes();
        let (url, handle) = mock_http_server(response);
        cache.refresh_from_source(100_000, &url).unwrap();
        handle.join().unwrap();

        assert_eq!(parses(&cached), 1);
        assert!(!is_loaded(&cached));
        assert_eq!(std::fs::read(&cached).unwrap(), new_yaml);
        let metadata = cache.saved_metadata().unwrap();
        assert_eq!(metadata.checked, 100_000);
        assert_eq!(metadata.etag.as_deref(), Some("\"etag-new\""));
        assert!(
            cache
                .load()
                .unwrap()
                .index
                .games
                .contains_key("UpdatedGame")
        );
    }
}
