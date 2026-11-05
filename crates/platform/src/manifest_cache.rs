//! Fixed-source data-only manifest updates, performed on a background worker.
use gamenode_core::drm_free::{LocalError, identity::ManifestIndex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
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
pub struct ManifestCache {
    pub directory: PathBuf,
    pub baseline: PathBuf,
}
fn failed(e: impl std::fmt::Display) -> LocalError {
    LocalError::Failed(e.to_string())
}
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
        if let Ok(bytes) = read_bounded(&cached)
            && let Ok(snapshot) = Self::snapshot(&bytes)
        {
            return Ok(snapshot);
        }
        Self::snapshot(&read_bounded(&self.baseline)?)
    }
    pub fn update(&self, now: i64) -> Result<ManifestSnapshot, LocalError> {
        self.update_from_source(now, SOURCE)
    }
    /// Failed updates always leave the last usable bytes intact. Jobs retain their own Arc snapshot.
    pub fn update_from_source(&self, now: i64, source: &str) -> Result<ManifestSnapshot, LocalError> {
        let mut metadata: Metadata = std::fs::read(self.directory.join("metadata.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if now.saturating_sub(metadata.checked) < 86400 {
            return self.load();
        }
        let current = self.load()?;
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
            &self.directory.join("metadata.json"),
            &serde_json::to_vec(&metadata).map_err(failed)?,
        )?;
        let response = match request.send() {
            Ok(response) => response,
            Err(_) => return Ok(current),
        };
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(current);
        }
        if response.status() != reqwest::StatusCode::OK
            || response
                .content_length()
                .is_some_and(|n| n > 32 * 1024 * 1024)
        {
            return Ok(current);
        }
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut bytes = Vec::new();
        response
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(failed)?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Ok(current);
        }
        let snapshot = match Self::snapshot(&bytes) {
            Ok(snapshot) => snapshot,
            Err(_) => return Ok(current),
        };
        atomic_write(&self.directory.join("manifest.yaml"), &bytes)?;
        metadata.etag = etag;
        metadata.hash = snapshot.hash.clone();
        atomic_write(
            &self.directory.join("metadata.json"),
            &serde_json::to_vec(&metadata).map_err(failed)?,
        )?;
        Ok(snapshot)
    }
}
fn read_bounded(path: &Path) -> Result<Vec<u8>, LocalError> {
    let file = std::fs::File::open(path).map_err(failed)?;
    let mut bytes = Vec::new();
    file.take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(failed)?;
    if bytes.len() > 32 * 1024 * 1024 {
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

    #[test]
    fn invalid_cache_falls_back_and_snapshot_is_immutable() {
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
    }

    #[test]
    fn not_modified_304_preserves_current_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: directory.path().join("cache"),
            baseline,
        };
        // Set metadata with etag
        std::fs::create_dir_all(&cache.directory).unwrap();
        let metadata = Metadata {
            etag: Some("\"12345\"".into()),
            checked: 0,
            hash: "h".into(),
        };
        std::fs::write(
            cache.directory.join("metadata.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();

        let response = b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n".to_vec();
        let (url, handle) = mock_http_server(response);
        let snapshot = cache.update_from_source(100, &url).unwrap();
        handle.join().unwrap();
        assert!(snapshot.index.games.contains_key("Game"));
    }

    #[test]
    fn corrupt_and_oversized_download_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"Game:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: directory.path().join("cache"),
            baseline,
        };

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
        assert!(!cache.directory.join("manifest.yaml").exists());

        // 2. Oversized download by Content-Length header
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 35000000\r\n\r\n".to_vec();
        let (url, handle) = mock_http_server(response);
        let snapshot = cache.update_from_source(200_000, &url).unwrap();
        handle.join().unwrap();
        assert!(snapshot.index.games.contains_key("Game"));
        assert!(!cache.directory.join("manifest.yaml").exists());
    }

    #[test]
    fn job_retains_original_snapshot_while_update_completes() {
        let directory = tempfile::tempdir().unwrap();
        let baseline = directory.path().join("baseline.yaml");
        std::fs::write(&baseline, b"OriginalGame:\n  files: {}\n").unwrap();
        let cache = ManifestCache {
            directory: directory.path().join("cache"),
            baseline,
        };

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
    }
}
