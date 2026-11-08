//! Cached, SHA-256-verified downloads shared by game tools and PC port installs.
//!
//! Downloads are written to a temporary `.part` file in the caller's cache root, verified, and
//! only then promoted to the final name. A cached file that still matches its expected checksum
//! is reused without touching the network. All functions here are blocking and must run on a
//! worker thread, never on the UI/main thread.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gamenode_core::tools::ToolError;
use sha2::{Digest, Sha256};

static PART_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Computes the lowercase hexadecimal SHA-256 digest of arbitrary bytes.
pub fn compute_bytes_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Computes the lowercase hexadecimal SHA-256 digest of a file on disk.
pub fn compute_file_sha256(path: &Path) -> Result<String, ToolError> {
    let mut file = File::open(path).map_err(|e| ToolError::Io(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| ToolError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Sanitizes a single path segment that came from remote data before it is used in a cache path.
pub fn sanitize_path_segment(segment: &str) -> String {
    segment
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect()
}

/// Cache root for a port release asset:
/// `<data_dir>/ports/cache/<catalog_id>/<version>/`.
pub fn ports_asset_cache_dir(data_dir: &Path, catalog_id: &str, version: &str) -> PathBuf {
    data_dir
        .join("ports")
        .join("cache")
        .join(sanitize_path_segment(catalog_id))
        .join(sanitize_path_segment(version))
}

/// Cache root for a pinned port extraction tool:
/// `<data_dir>/ports/tools/<tool_id>/<tag>/`.
pub fn ports_tool_cache_dir(data_dir: &Path, tool_id: &str, tag: &str) -> PathBuf {
    data_dir
        .join("ports")
        .join("tools")
        .join(sanitize_path_segment(tool_id))
        .join(sanitize_path_segment(tag))
}

/// Decides which URLs a download may use.
pub trait DownloadUrlPolicy: Send + Sync {
    fn is_allowed(&self, url: &str) -> bool;
}

/// Allows HTTPS downloads from any host, plus plain HTTP on loopback for local tests.
///
/// PC port assets come from the catalog's CDN, which is not known in advance, so the host is not
/// restricted. The scheme still has to be HTTPS in production.
#[derive(Clone, Copy, Debug, Default)]
pub struct HttpsDownloadPolicy;

impl DownloadUrlPolicy for HttpsDownloadPolicy {
    fn is_allowed(&self, url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        match parsed.scheme() {
            "https" => parsed.host_str().is_some(),
            "http" => matches!(parsed.host_str(), Some("127.0.0.1") | Some("localhost")),
            _ => false,
        }
    }
}

/// Allows the download hosts used by game-tool releases: GitHub HTTPS hosts, an optional custom
/// API host (for tests and mirrors), loopback HTTP/HTTPS and local `file://` URLs.
#[derive(Clone, Debug, Default)]
pub struct GitHubDownloadPolicy {
    api_base_url: Option<String>,
}

impl GitHubDownloadPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a policy that also trusts the host and port of a custom API base URL.
    pub fn with_api_base_url(api_base_url: impl Into<String>) -> Self {
        Self {
            api_base_url: Some(api_base_url.into()),
        }
    }
}

impl DownloadUrlPolicy for GitHubDownloadPolicy {
    fn is_allowed(&self, raw_url: &str) -> bool {
        if raw_url.starts_with("file://") {
            return true;
        }

        let Ok(parsed) = url::Url::parse(raw_url) else {
            return false;
        };

        // When a custom/mock base URL is configured, allow matching that host & port.
        if let Some(api_base_url) = self.api_base_url.as_deref()
            && api_base_url != "https://api.github.com"
            && let Ok(base_url) = url::Url::parse(api_base_url)
            && parsed.host_str() == base_url.host_str()
            && parsed.port() == base_url.port()
        {
            return true;
        }

        // Local unit tests and mocks.
        if let Some(host) = parsed.host_str()
            && (host == "127.0.0.1" || host == "localhost")
        {
            return parsed.scheme() == "http" || parsed.scheme() == "https";
        }

        // Production security rule: HTTPS from authorized GitHub hosts.
        if parsed.scheme() != "https" {
            return false;
        }

        if let Some(host) = parsed.host_str() {
            let host_lower = host.to_ascii_lowercase();
            if host_lower == "github.com"
                || host_lower == "api.github.com"
                || host_lower == "objects.githubusercontent.com"
                || host_lower == "release-assets.githubusercontent.com"
                || host_lower.ends_with(".githubusercontent.com")
            {
                return true;
            }
        }

        false
    }
}

/// Fetches a URL into a destination path. Implementations are blocking and run on workers.
pub trait DownloadFetcher: Send + Sync {
    fn fetch(&self, url: &str, destination: &Path) -> Result<(), ToolError>;
}

/// Blocking HTTP/HTTPS fetcher. Local `file://` URLs and existing absolute paths are copied so
/// tests and local mirrors do not need a server.
pub struct HttpDownloadFetcher {
    client: reqwest::blocking::Client,
}

impl HttpDownloadFetcher {
    pub fn new() -> Result<Self, ToolError> {
        Self::with_user_agent("gamenode-desktop")
    }

    pub fn with_user_agent(user_agent: &str) -> Result<Self, ToolError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent(user_agent)
            .build()
            .map_err(|e| ToolError::Failed(format!("Failed to build HTTP client: {e}")))?;
        Ok(Self { client })
    }

    /// Shared client for API requests made by the same catalog.
    pub fn client(&self) -> &reqwest::blocking::Client {
        &self.client
    }
}

impl DownloadFetcher for HttpDownloadFetcher {
    fn fetch(&self, url: &str, destination: &Path) -> Result<(), ToolError> {
        if url.starts_with("file://") {
            let file_path = if let Ok(parsed) = url::Url::parse(url) {
                parsed
                    .to_file_path()
                    .map_err(|_| ToolError::DownloadFailed(format!("Invalid file URL: {url}")))?
            } else {
                PathBuf::from(url.strip_prefix("file://").unwrap_or(url))
            };
            std::fs::copy(&file_path, destination).map_err(|e| ToolError::Io(e.to_string()))?;
            return Ok(());
        }

        if Path::new(url).is_absolute() && Path::new(url).exists() {
            std::fs::copy(url, destination).map_err(|e| ToolError::Io(e.to_string()))?;
            return Ok(());
        }

        let mut response = self
            .client
            .get(url)
            .send()
            .map_err(|e| ToolError::DownloadFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ToolError::DownloadFailed(format!(
                "HTTP {} downloading {url}",
                response.status()
            )));
        }

        let mut part_file = File::create(destination).map_err(|e| ToolError::Io(e.to_string()))?;
        std::io::copy(&mut response, &mut part_file).map_err(|e| ToolError::Io(e.to_string()))?;
        Ok(())
    }
}

/// One file to keep in a cache root, with the checksum it must match.
#[derive(Clone, Copy, Debug)]
pub struct CachedDownloadRequest<'a> {
    pub url: &'a str,
    pub cache_dir: &'a Path,
    pub file_name: &'a str,
    /// Expected lowercase or uppercase hexadecimal SHA-256, when the source publishes one.
    pub expected_sha256: Option<&'a str>,
}

/// A verified file in the cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedDownload {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
    /// True when the file was already cached and verified, so no download happened.
    pub from_cache: bool,
}

/// Optional integrity check run on a freshly downloaded file before it is promoted.
pub type DownloadValidator<'a> = dyn Fn(&Path) -> Result<(), ToolError> + 'a;

/// Ensures `request.file_name` exists in `request.cache_dir`, downloading and verifying it when
/// the cached copy is missing or does not match the expected checksum.
///
/// `validate` runs on a freshly downloaded file before it is promoted; the game-tools catalog
/// uses it to reject corrupt archives when no checksum is published. A file that fails the
/// checksum or validation is deleted, so a retry starts clean.
pub fn ensure_cached_download(
    fetcher: &dyn DownloadFetcher,
    policy: &dyn DownloadUrlPolicy,
    request: &CachedDownloadRequest<'_>,
    validate: Option<&DownloadValidator<'_>>,
) -> Result<CachedDownload, ToolError> {
    validate_file_name(request.file_name)?;
    let target_path = request.cache_dir.join(request.file_name);

    // Reuse a cached file only when it still matches what the release declares.
    if target_path.is_file() {
        if let Ok(computed) = compute_file_sha256(&target_path) {
            let valid = match request.expected_sha256 {
                Some(expected) => expected.eq_ignore_ascii_case(&computed),
                None => validate.is_none_or(|check| check(&target_path).is_ok()),
            };
            if valid {
                let size_bytes = std::fs::metadata(&target_path)
                    .map(|m| m.len())
                    .unwrap_or_default();
                return Ok(CachedDownload {
                    path: target_path,
                    size_bytes,
                    sha256: computed,
                    from_cache: true,
                });
            }
        }
        // Existing file is corrupt or stale; remove it before downloading again.
        let _ = std::fs::remove_file(&target_path);
    }

    if !policy.is_allowed(request.url) {
        return Err(ToolError::DownloadFailed(format!(
            "Untrusted or forbidden download URL: {}",
            request.url
        )));
    }

    std::fs::create_dir_all(request.cache_dir).map_err(|e| ToolError::Io(e.to_string()))?;

    let part_filename = format!(
        "{}.part.{}_{}",
        request.file_name,
        std::process::id(),
        PART_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let part_path = request.cache_dir.join(part_filename);

    if let Err(error) = fetcher.fetch(request.url, &part_path) {
        remove_part_file(&part_path);
        return Err(error);
    }

    let computed = match compute_file_sha256(&part_path) {
        Ok(hash) => hash,
        Err(error) => {
            remove_part_file(&part_path);
            return Err(error);
        }
    };

    if let Some(expected) = request.expected_sha256
        && !expected.eq_ignore_ascii_case(&computed)
    {
        remove_part_file(&part_path);
        return Err(ToolError::ChecksumMismatch {
            path: target_path.to_string_lossy().to_string(),
            expected: expected.to_string(),
            actual: computed,
        });
    }

    if let Some(check) = validate
        && let Err(error) = check(&part_path)
    {
        remove_part_file(&part_path);
        return Err(error);
    }

    std::fs::rename(&part_path, &target_path).map_err(|e| ToolError::Io(e.to_string()))?;
    let size_bytes = std::fs::metadata(&target_path)
        .map(|m| m.len())
        .unwrap_or_default();

    Ok(CachedDownload {
        path: target_path,
        size_bytes,
        sha256: computed,
        from_cache: false,
    })
}

fn validate_file_name(file_name: &str) -> Result<(), ToolError> {
    if file_name.is_empty()
        || file_name == "."
        || file_name == ".."
        || file_name.contains(['/', '\\', ':'])
    {
        return Err(ToolError::DownloadFailed(format!(
            "Invalid download file name: {file_name}"
        )));
    }
    Ok(())
}

fn remove_part_file(part_path: &Path) {
    if part_path.exists() {
        let _ = std::fs::remove_file(part_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct FakeFetcher {
        bytes: Vec<u8>,
        calls: AtomicUsize,
    }

    impl FakeFetcher {
        fn new(bytes: &[u8]) -> Self {
            Self {
                bytes: bytes.to_vec(),
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }

    impl DownloadFetcher for FakeFetcher {
        fn fetch(&self, _url: &str, destination: &Path) -> Result<(), ToolError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            std::fs::write(destination, &self.bytes).map_err(|e| ToolError::Io(e.to_string()))
        }
    }

    fn cache_entries(cache_dir: &Path) -> Vec<PathBuf> {
        if !cache_dir.exists() {
            return Vec::new();
        }
        std::fs::read_dir(cache_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }

    #[test]
    fn mismatched_download_is_deleted() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join("cache");
        let fetcher = FakeFetcher::new(b"downloaded bytes");
        let expected = "0000000000000000000000000000000000000000000000000000000000000000";

        let request = CachedDownloadRequest {
            url: "https://example.invalid/asset.zip",
            cache_dir: &cache_dir,
            file_name: "asset.zip",
            expected_sha256: Some(expected),
        };

        let error =
            ensure_cached_download(&fetcher, &HttpsDownloadPolicy, &request, None).unwrap_err();
        match error {
            ToolError::ChecksumMismatch {
                expected: reported,
                actual,
                ..
            } => {
                assert_eq!(reported, expected);
                assert_eq!(actual, compute_bytes_sha256(b"downloaded bytes"));
            }
            other => panic!("Expected ChecksumMismatch, got: {other:?}"),
        }

        assert_eq!(fetcher.calls(), 1);
        assert!(
            cache_entries(&cache_dir).is_empty(),
            "a mismatched download must leave no files behind"
        );
    }

    #[test]
    fn cached_verified_file_is_reused() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join("cache");
        let bytes = b"verified release bytes";
        let expected = compute_bytes_sha256(bytes);
        let fetcher = FakeFetcher::new(bytes);

        let request = CachedDownloadRequest {
            url: "https://example.invalid/asset.7z",
            cache_dir: &cache_dir,
            file_name: "asset.7z",
            expected_sha256: Some(&expected),
        };

        let first = ensure_cached_download(&fetcher, &HttpsDownloadPolicy, &request, None).unwrap();
        assert!(!first.from_cache);
        assert_eq!(first.sha256, expected);
        assert_eq!(fetcher.calls(), 1);
        assert_eq!(std::fs::read(&first.path).unwrap(), bytes);

        let second =
            ensure_cached_download(&fetcher, &HttpsDownloadPolicy, &request, None).unwrap();
        assert!(second.from_cache);
        assert_eq!(second.path, first.path);
        assert_eq!(second.sha256, expected);
        assert_eq!(
            fetcher.calls(),
            1,
            "the cached file must not be fetched again"
        );
    }

    #[test]
    fn corrupt_cached_file_is_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join("cache");
        let bytes = b"correct bytes";
        let expected = compute_bytes_sha256(bytes);
        let fetcher = FakeFetcher::new(bytes);

        let request = CachedDownloadRequest {
            url: "https://example.invalid/asset.zip",
            cache_dir: &cache_dir,
            file_name: "asset.zip",
            expected_sha256: Some(&expected),
        };

        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("asset.zip"), b"corrupt bytes").unwrap();

        let cached =
            ensure_cached_download(&fetcher, &HttpsDownloadPolicy, &request, None).unwrap();
        assert!(!cached.from_cache);
        assert_eq!(fetcher.calls(), 1);
        assert_eq!(std::fs::read(cache_dir.join("asset.zip")).unwrap(), bytes);
    }

    #[test]
    fn rejects_untrusted_download_urls() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join("cache");
        let fetcher = FakeFetcher::new(b"bytes");
        let request = CachedDownloadRequest {
            url: "http://example.invalid/asset.zip",
            cache_dir: &cache_dir,
            file_name: "asset.zip",
            expected_sha256: None,
        };

        let error =
            ensure_cached_download(&fetcher, &HttpsDownloadPolicy, &request, None).unwrap_err();
        assert!(matches!(error, ToolError::DownloadFailed(_)));
        assert_eq!(fetcher.calls(), 0);
    }

    #[test]
    fn ports_cache_roots_follow_the_design_layout() {
        let data_dir = Path::new("/data");
        assert_eq!(
            ports_asset_cache_dir(data_dir, "qcat_123", "v0.9.38"),
            PathBuf::from("/data/ports/cache/qcat_123/v0.9.38")
        );
        assert_eq!(
            ports_tool_cache_dir(data_dir, "ps4-pkg", "v1.2.3"),
            PathBuf::from("/data/ports/tools/ps4-pkg/v1.2.3")
        );

        // Remote segments cannot escape their cache root.
        assert_eq!(
            ports_asset_cache_dir(data_dir, "../../evil", "v1/../../x"),
            PathBuf::from("/data/ports/cache/.._.._evil/v1_.._.._x")
        );
    }

    #[test]
    fn validation_runs_before_promoting_a_download() {
        struct RejectingValidator;
        impl DownloadFetcher for RejectingValidator {
            fn fetch(&self, _url: &str, destination: &Path) -> Result<(), ToolError> {
                std::fs::write(destination, b"not an archive")
                    .map_err(|e| ToolError::Io(e.to_string()))
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join("cache");
        let request = CachedDownloadRequest {
            url: "https://example.invalid/asset.zip",
            cache_dir: &cache_dir,
            file_name: "asset.zip",
            expected_sha256: None,
        };
        let check = |_path: &Path| Err(ToolError::ExtractionFailed("bad archive".to_string()));

        let error = ensure_cached_download(
            &RejectingValidator,
            &HttpsDownloadPolicy,
            &request,
            Some(&check),
        )
        .unwrap_err();
        assert!(matches!(error, ToolError::ExtractionFailed(_)));
        assert!(cache_entries(&cache_dir).is_empty());
    }
}
