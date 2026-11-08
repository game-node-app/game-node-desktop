//! Port artwork: snapshot-only URL resolution, a disk cache and an HTTPS fetcher (design D9).
//!
//! The WebView loads port art only through `gamenode-asset://localhost/port-art/<catalogId>/<kind>`.
//! The protocol handler asks a [`PortArtResolver`] for the URL recorded for that catalog id in the
//! stored catalog snapshot; the request never supplies a URL. On a cache miss the image is fetched
//! with [`PortArtFetcher`] and written to `<data_dir>/ports/art/`.
//!
//! The cache is keyed by a hash of the catalog id and the snapshot URL, so a changed upstream URL
//! naturally resolves to a new file and a removed entry cannot read another entry's image.
//!
//! Everything here performs disk or network I/O and is blocking: call it on a worker thread, never
//! on the UI or main thread. The protocol handler in `gamenode-app` calls [`PortArtLoader::load`]
//! from `tokio::task::spawn_blocking`.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gamenode_core::ports::{CatalogSnapshot, PortStore};

use crate::download::{DownloadUrlPolicy, compute_bytes_sha256};

/// Largest image accepted from the catalog CDN. Anything larger is refused before it reaches the
/// WebView or the disk cache.
pub const MAX_PORT_ART_BYTES: u64 = 8 * 1024 * 1024;

static PART_COUNTER: AtomicU64 = AtomicU64::new(1);

/// The folder under `data_dir` that holds cached port artwork.
pub fn ports_art_cache_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("ports").join("art")
}

/// The two artwork sizes the catalog route and the UI contract use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortArtKind {
    /// The portrait cover shown by tiles.
    Capsule,
    /// The wide image shown by the detail pane.
    Hero,
}

impl PortArtKind {
    /// Parses the route segment. Unknown kinds are refused by the caller.
    pub fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("capsule") {
            Some(Self::Capsule)
        } else if value.eq_ignore_ascii_case("hero") {
            Some(Self::Hero)
        } else {
            None
        }
    }

    /// The route segment and diagnostic name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capsule => "capsule",
            Self::Hero => "hero",
        }
    }
}

/// The outcome of looking a catalog id up in the stored snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortArtSource {
    /// No entry with this catalog id is stored. Served as 404.
    UnknownEntry,
    /// The entry exists but the snapshot records no URL for this kind. Served as 404.
    NoArt,
    /// The URL to fetch. It always comes from the snapshot, never from the request.
    Url(String),
}

impl PortArtSource {
    /// The URL, when the snapshot has one.
    pub fn url(&self) -> Option<&str> {
        match self {
            Self::Url(url) => Some(url),
            Self::UnknownEntry | Self::NoArt => None,
        }
    }
}

/// Resolves the artwork URL recorded for a catalog id. Implementations may block on the store and
/// are therefore only called from a worker thread.
///
/// A closure `Fn(&str, PortArtKind) -> PortArtSource` implements this trait, so the app can resolve
/// from a `CatalogWorker`'s in-memory entries instead of adding a struct:
///
/// ```ignore
/// let resolver: Arc<dyn PortArtResolver> = Arc::new(move |catalog_id, kind| {
///     // Look the entry up in a snapshot-derived list and return PortArtSource::Url(..).
/// });
/// ```
pub trait PortArtResolver: Send + Sync {
    fn resolve(&self, catalog_id: &str, kind: PortArtKind) -> PortArtSource;
}

impl<F> PortArtResolver for F
where
    F: Fn(&str, PortArtKind) -> PortArtSource + Send + Sync,
{
    fn resolve(&self, catalog_id: &str, kind: PortArtKind) -> PortArtSource {
        self(catalog_id, kind)
    }
}

/// Resolves artwork URLs from [`PortStore::catalog_snapshot`], the only source of truth design D9
/// allows.
///
/// Each call reads and parses the stored snapshot, which is exact but not free. The app may prefer
/// the catalog worker's already-parsed entries through the closure form of [`PortArtResolver`].
pub struct SnapshotPortArtResolver {
    store: Arc<dyn PortStore>,
}

impl SnapshotPortArtResolver {
    pub fn new(store: Arc<dyn PortStore>) -> Self {
        Self { store }
    }
}

impl PortArtResolver for SnapshotPortArtResolver {
    fn resolve(&self, catalog_id: &str, kind: PortArtKind) -> PortArtSource {
        match self.store.catalog_snapshot() {
            Ok(Some(snapshot)) => resolve_in_snapshot(&snapshot, catalog_id, kind),
            Ok(None) => PortArtSource::UnknownEntry,
            Err(error) => {
                tracing::debug!(%error, catalog_id, "could not read the port catalog for artwork");
                PortArtSource::UnknownEntry
            }
        }
    }
}

/// Looks one catalog id up in an already-read snapshot. Shared by the store-backed resolver and
/// callers that hold the snapshot or the catalog worker's entries.
pub fn resolve_in_snapshot(
    snapshot: &CatalogSnapshot,
    catalog_id: &str,
    kind: PortArtKind,
) -> PortArtSource {
    let Some(entry) = snapshot
        .entries
        .iter()
        .find(|entry| entry.catalog_id == catalog_id)
    else {
        return PortArtSource::UnknownEntry;
    };
    let url = match kind {
        PortArtKind::Capsule => entry.art.capsule.as_deref(),
        PortArtKind::Hero => entry.art.hero.as_deref(),
    };
    match url.map(str::trim).filter(|url| !url.is_empty()) {
        Some(url) => PortArtSource::Url(url.to_owned()),
        None => PortArtSource::NoArt,
    }
}

/// Why port art could not be served.
#[derive(Debug, thiserror::Error)]
pub enum PortArtError {
    #[error("no port catalog entry matches {catalog_id:?}")]
    UnknownEntry { catalog_id: String },
    #[error("port catalog entry {catalog_id:?} has no {kind} art")]
    NoArt {
        catalog_id: String,
        kind: &'static str,
    },
    #[error("port art URL is not allowed: {url}")]
    RejectedUrl { url: String },
    #[error("port art download failed: {0}")]
    Fetch(String),
    #[error("port art exceeds the {limit}-byte limit")]
    TooLarge { limit: u64 },
    #[error("port art is not a supported image")]
    NotImage,
    #[error("port art cache I/O failed: {0}")]
    Io(String),
}

/// A validated image ready for the asset protocol.
#[derive(Debug, PartialEq, Eq)]
pub struct PortArtImage {
    pub bytes: Vec<u8>,
    /// One of `image/jpeg`, `image/png`, `image/webp` or `image/gif`, detected from magic bytes.
    pub content_type: &'static str,
    /// True when the bytes came from the disk cache, so no network request was made.
    pub from_cache: bool,
}

/// Detects an image type from magic bytes. SVG and other documents are deliberately refused: the
/// asset protocol serves them to the WebView as images, and a document could carry script.
pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("image/png");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    None
}

/// Allows only HTTPS URLs with a host. Port art comes from a public CDN recorded in the catalog;
/// no loopback or `file://` exception is granted, unlike the game-tools download policy.
#[derive(Clone, Copy, Debug, Default)]
pub struct PortArtUrlPolicy;

impl DownloadUrlPolicy for PortArtUrlPolicy {
    fn is_allowed(&self, raw_url: &str) -> bool {
        url::Url::parse(raw_url)
            .is_ok_and(|parsed| parsed.scheme() == "https" && parsed.host_str().is_some())
    }
}

/// Fetches an artwork URL. Implementations are blocking and must enforce `max_bytes` while reading
/// and refuse responses that are not images.
pub trait PortArtFetcher: Send + Sync {
    fn fetch(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, PortArtError>;
}

/// Blocking HTTPS fetcher for catalog artwork.
pub struct HttpPortArtFetcher {
    client: reqwest::blocking::Client,
}

impl HttpPortArtFetcher {
    pub fn new() -> Result<Self, PortArtError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent(super::quiver::user_agent())
            .build()
            .map_err(|error| PortArtError::Fetch(error.to_string()))?;
        Ok(Self { client })
    }
}

impl PortArtFetcher for HttpPortArtFetcher {
    fn fetch(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, PortArtError> {
        if !PortArtUrlPolicy.is_allowed(url) {
            return Err(PortArtError::RejectedUrl {
                url: url.to_owned(),
            });
        }
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|error| PortArtError::Fetch(error.to_string()))?;

        // A redirect chain must not walk away from HTTPS either.
        if response.url().scheme() != "https" {
            return Err(PortArtError::RejectedUrl {
                url: response.url().to_string(),
            });
        }
        if !response.status().is_success() {
            return Err(PortArtError::Fetch(format!(
                "HTTP {} fetching port art",
                response.status()
            )));
        }

        let media_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .unwrap_or("")
            .to_ascii_lowercase();
        if !media_type.starts_with("image/") || media_type == "image/svg+xml" {
            return Err(PortArtError::NotImage);
        }

        let mut bytes = Vec::new();
        response
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| PortArtError::Fetch(error.to_string()))?;
        if bytes.len() as u64 > max_bytes {
            return Err(PortArtError::TooLarge { limit: max_bytes });
        }
        Ok(bytes)
    }
}

/// The on-disk artwork cache: `<data_dir>/ports/art/<hash>.img`.
#[derive(Clone, Debug)]
pub struct PortArtCache {
    dir: PathBuf,
}

impl PortArtCache {
    /// A cache under `<data_dir>/ports/art/`.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: ports_art_cache_dir(data_dir),
        }
    }

    /// A cache rooted at an explicit folder, for tests and hosts with a custom layout.
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The cached image for this snapshot URL, or `None` when the cache is missing or unreadable.
    pub fn read(&self, catalog_id: &str, url: &str) -> Option<Vec<u8>> {
        fs::read(self.path_for(catalog_id, url)).ok()
    }

    /// Writes the image atomically: a temporary file in the cache folder is renamed over the final
    /// name, so a reader never sees half an image.
    pub fn write(&self, catalog_id: &str, url: &str, bytes: &[u8]) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.path_for(catalog_id, url);
        let temp = self.dir.join(format!(
            ".{}.{}.{}.part",
            cache_key(catalog_id, url),
            std::process::id(),
            PART_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let written = (|| {
            let mut file = File::create(&temp)?;
            file.write_all(bytes)?;
            file.flush()?;
            drop(file);
            fs::rename(&temp, &path)
        })();
        if written.is_err() {
            // Best effort: a leftover part file is harmless but should not pile up.
            let _ = fs::remove_file(&temp);
        }
        written
    }

    /// Removes a cached image that failed validation, so the next call refetches it.
    pub fn remove(&self, catalog_id: &str, url: &str) {
        let _ = fs::remove_file(self.path_for(catalog_id, url));
    }

    fn path_for(&self, catalog_id: &str, url: &str) -> PathBuf {
        self.dir.join(format!("{}.img", cache_key(catalog_id, url)))
    }
}

/// The cache key: a hash of the catalog id and the snapshot URL. Hashing keeps remote-controlled
/// text out of file names and lets two entries that share an image share one cached file.
fn cache_key(catalog_id: &str, url: &str) -> String {
    compute_bytes_sha256(format!("{catalog_id}\u{0}{url}").as_bytes())
}

/// Resolves, caches and fetches port artwork. Blocking; call from a worker thread.
#[derive(Clone)]
pub struct PortArtLoader {
    resolver: Arc<dyn PortArtResolver>,
    fetcher: Arc<dyn PortArtFetcher>,
    policy: Arc<dyn DownloadUrlPolicy>,
    cache: PortArtCache,
}

impl PortArtLoader {
    /// Uses the strict [`PortArtUrlPolicy`] (HTTPS only).
    pub fn new(
        resolver: Arc<dyn PortArtResolver>,
        fetcher: Arc<dyn PortArtFetcher>,
        cache: PortArtCache,
    ) -> Self {
        Self::with_policy(resolver, fetcher, Arc::new(PortArtUrlPolicy), cache)
    }

    /// Uses a custom URL policy, for tests that need to observe the refusal path.
    pub fn with_policy(
        resolver: Arc<dyn PortArtResolver>,
        fetcher: Arc<dyn PortArtFetcher>,
        policy: Arc<dyn DownloadUrlPolicy>,
        cache: PortArtCache,
    ) -> Self {
        Self {
            resolver,
            fetcher,
            policy,
            cache,
        }
    }

    /// The production loader: [`HttpPortArtFetcher`] and `<data_dir>/ports/art/`.
    pub fn production(
        resolver: Arc<dyn PortArtResolver>,
        data_dir: &Path,
    ) -> Result<Self, PortArtError> {
        Ok(Self::new(
            resolver,
            Arc::new(HttpPortArtFetcher::new()?),
            PortArtCache::new(data_dir),
        ))
    }

    /// Loads one image, from the disk cache when possible.
    ///
    /// * an unknown catalog id or an entry without that art is an error (the protocol answers 404);
    /// * a snapshot URL that is not HTTPS is never fetched;
    /// * fetched bytes must be a supported image and no larger than [`MAX_PORT_ART_BYTES`];
    /// * a cache write failure does not fail the request, because the image was already fetched.
    pub fn load(&self, catalog_id: &str, kind: PortArtKind) -> Result<PortArtImage, PortArtError> {
        let url = match self.resolver.resolve(catalog_id, kind) {
            PortArtSource::Url(url) => url,
            PortArtSource::UnknownEntry => {
                return Err(PortArtError::UnknownEntry {
                    catalog_id: catalog_id.to_owned(),
                });
            }
            PortArtSource::NoArt => {
                return Err(PortArtError::NoArt {
                    catalog_id: catalog_id.to_owned(),
                    kind: kind.as_str(),
                });
            }
        };

        if !self.policy.is_allowed(&url) {
            return Err(PortArtError::RejectedUrl { url });
        }

        if let Some(bytes) = self.cache.read(catalog_id, &url) {
            match validate_image(&bytes) {
                Ok(content_type) => {
                    return Ok(PortArtImage {
                        bytes,
                        content_type,
                        from_cache: true,
                    });
                }
                Err(_) => {
                    // A truncated or tampered cache entry is not served; fetch it again.
                    self.cache.remove(catalog_id, &url);
                }
            }
        }

        let bytes = self.fetcher.fetch(&url, MAX_PORT_ART_BYTES)?;
        let content_type = validate_image(&bytes)?;
        if let Err(error) = self.cache.write(catalog_id, &url, &bytes) {
            tracing::debug!(%error, catalog_id, "could not cache port art");
        }
        Ok(PortArtImage {
            bytes,
            content_type,
            from_cache: false,
        })
    }

    /// The cache the loader writes to.
    pub fn cache(&self) -> &PortArtCache {
        &self.cache
    }
}

fn validate_image(bytes: &[u8]) -> Result<&'static str, PortArtError> {
    if bytes.len() as u64 > MAX_PORT_ART_BYTES {
        return Err(PortArtError::TooLarge {
            limit: MAX_PORT_ART_BYTES,
        });
    }
    image_mime(bytes).ok_or(PortArtError::NotImage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::ports::{CatalogArt, CatalogEntry};
    use std::sync::atomic::AtomicUsize;

    const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0x01, 0x02, 0x03];

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

    impl PortArtFetcher for FakeFetcher {
        fn fetch(&self, _url: &str, _max_bytes: u64) -> Result<Vec<u8>, PortArtError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.bytes.clone())
        }
    }

    fn snapshot_with_art() -> CatalogSnapshot {
        CatalogSnapshot::new(
            1,
            vec![CatalogEntry {
                catalog_id: "qcat_1".into(),
                slug: "port".into(),
                name: "Port".into(),
                art: CatalogArt {
                    capsule: Some("https://cdn.example/capsule.png".into()),
                    hero: None,
                    ..CatalogArt::default()
                },
                ..CatalogEntry::default()
            }],
        )
    }

    #[test]
    fn snapshot_lookup_distinguishes_unknown_entries_from_missing_art() {
        let snapshot = snapshot_with_art();
        assert_eq!(
            resolve_in_snapshot(&snapshot, "qcat_1", PortArtKind::Capsule),
            PortArtSource::Url("https://cdn.example/capsule.png".into())
        );
        assert_eq!(
            resolve_in_snapshot(&snapshot, "qcat_1", PortArtKind::Hero),
            PortArtSource::NoArt
        );
        assert_eq!(
            resolve_in_snapshot(&snapshot, "qcat_missing", PortArtKind::Capsule),
            PortArtSource::UnknownEntry
        );
        assert_eq!(
            PortArtSource::Url("https://cdn.example/capsule.png".into()).url(),
            Some("https://cdn.example/capsule.png")
        );
    }

    #[test]
    fn the_cache_lives_under_ports_art_and_is_keyed_by_id_and_url() {
        let temp = tempfile::tempdir().unwrap();
        let cache = PortArtCache::new(temp.path());
        assert_eq!(
            cache.dir(),
            temp.path().join("ports").join("art"),
            "the cache root follows design D9"
        );

        let url = "https://cdn.example/capsule.png";
        cache.write("qcat_1", url, JPEG).unwrap();
        assert_eq!(cache.read("qcat_1", url), Some(JPEG.to_vec()));

        // Another catalog id, or another URL for the same id, is another file.
        assert_eq!(cache.read("qcat_2", url), None);
        assert_eq!(cache.read("qcat_1", "https://cdn.example/other.png"), None);

        // Rewriting replaces the image whole and leaves no part file behind.
        cache.write("qcat_1", url, b"replacement").unwrap();
        assert_eq!(cache.read("qcat_1", url), Some(b"replacement".to_vec()));
        let files: Vec<_> = fs::read_dir(cache.dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(files.len(), 1, "one cached image, no temporary file");
    }

    #[test]
    fn the_loader_reuses_a_cached_image_and_never_refetches_it() {
        let temp = tempfile::tempdir().unwrap();
        let fetcher = Arc::new(FakeFetcher::new(JPEG));
        let resolver =
            |_: &str, _: PortArtKind| PortArtSource::Url("https://cdn.example/capsule.png".into());
        let loader = PortArtLoader::new(
            Arc::new(resolver),
            fetcher.clone(),
            PortArtCache::new(temp.path()),
        );

        let first = loader.load("qcat_1", PortArtKind::Capsule).unwrap();
        assert_eq!(first.bytes, JPEG);
        assert_eq!(first.content_type, "image/jpeg");
        assert!(!first.from_cache);

        let second = loader.load("qcat_1", PortArtKind::Capsule).unwrap();
        assert!(second.from_cache);
        assert_eq!(second.bytes, JPEG);
        assert_eq!(fetcher.calls(), 1);
    }

    #[test]
    fn the_loader_refuses_non_https_urls_before_fetching() {
        let temp = tempfile::tempdir().unwrap();
        let fetcher = Arc::new(FakeFetcher::new(JPEG));
        let resolver =
            |_: &str, _: PortArtKind| PortArtSource::Url("http://cdn.example/capsule.png".into());
        let loader = PortArtLoader::new(
            Arc::new(resolver),
            fetcher.clone(),
            PortArtCache::new(temp.path()),
        );

        assert!(matches!(
            loader.load("qcat_1", PortArtKind::Capsule),
            Err(PortArtError::RejectedUrl { .. })
        ));
        assert_eq!(fetcher.calls(), 0);
        assert!(!loader.cache().dir().exists(), "nothing was written");
    }

    #[test]
    fn the_loader_refuses_oversize_and_non_image_bodies() {
        let temp = tempfile::tempdir().unwrap();
        let cache = PortArtCache::new(temp.path());
        let resolver =
            |_: &str, _: PortArtKind| PortArtSource::Url("https://cdn.example/capsule.png".into());

        let oversize = Arc::new(FakeFetcher::new(&vec![
            0u8;
            MAX_PORT_ART_BYTES as usize + 1
        ]));
        let loader = PortArtLoader::new(Arc::new(resolver), oversize, cache.clone());
        assert!(matches!(
            loader.load("qcat_1", PortArtKind::Capsule),
            Err(PortArtError::TooLarge { .. })
        ));

        let text = Arc::new(FakeFetcher::new(b"<html>not an image</html>"));
        let loader = PortArtLoader::new(Arc::new(resolver), text, cache);
        assert!(matches!(
            loader.load("qcat_1", PortArtKind::Capsule),
            Err(PortArtError::NotImage)
        ));
    }

    #[test]
    fn a_truncated_cache_entry_is_not_served() {
        let temp = tempfile::tempdir().unwrap();
        let cache = PortArtCache::new(temp.path());
        let url = "https://cdn.example/capsule.png";
        cache.write("qcat_1", url, b"not an image").unwrap();

        let fetcher = Arc::new(FakeFetcher::new(JPEG));
        let resolver = |_: &str, _: PortArtKind| PortArtSource::Url(url.into());
        let loader = PortArtLoader::new(Arc::new(resolver), fetcher.clone(), cache);

        let image = loader.load("qcat_1", PortArtKind::Capsule).unwrap();
        assert_eq!(image.bytes, JPEG);
        assert!(!image.from_cache);
        assert_eq!(fetcher.calls(), 1);
    }

    #[test]
    fn image_types_are_detected_from_magic_bytes_only() {
        let png = [0x89, b'P', b'N', b'G', 0x0d];
        let mut webp = vec![0u8; 16];
        webp[0..4].copy_from_slice(b"RIFF");
        webp[8..12].copy_from_slice(b"WEBP");

        assert_eq!(image_mime(JPEG), Some("image/jpeg"));
        assert_eq!(image_mime(&png), Some("image/png"));
        assert_eq!(image_mime(&webp), Some("image/webp"));
        assert_eq!(image_mime(b"GIF89a..."), Some("image/gif"));
        assert_eq!(image_mime(b"<svg xmlns=\"...\">"), None);
        assert_eq!(image_mime(b""), None);
    }

    #[test]
    fn only_https_urls_are_allowed() {
        assert!(PortArtUrlPolicy.is_allowed("https://cdn.example/art.png"));
        assert!(!PortArtUrlPolicy.is_allowed("http://cdn.example/art.png"));
        assert!(!PortArtUrlPolicy.is_allowed("http://127.0.0.1/art.png"));
        assert!(!PortArtUrlPolicy.is_allowed("file:///tmp/art.png"));
        assert!(!PortArtUrlPolicy.is_allowed("not a url"));
    }
}
