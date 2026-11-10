//! Custom `gamenode-asset://` protocol implementation for Tauri v2.
//!
//! Serves cached game covers, landscape artwork and PC port artwork directly to the WebView
//! off the main thread without decoding full uncompressed bitmaps in Rust memory.
//!
//! Supported schemes and formats:
//! - Standard: `gamenode-asset://localhost/cover/<file>` or `gamenode-asset://cover/<file>`
//! - Artwork: `gamenode-asset://localhost/artwork/<game_id>?rev=...` or `gamenode-asset://artwork/<game_id>`
//! - Port art: `gamenode-asset://localhost/port-art/<catalog_id>/<capsule|hero>`
//! - Windows WebView2 mapped URL: `http://gamenode-asset.localhost/cover/<file>`,
//!   `http://gamenode-asset.localhost/artwork/<game_id>` and
//!   `http://gamenode-asset.localhost/port-art/<catalog_id>/<capsule|hero>`
//!
//! Port art is served only while a [`PortArtState`] is managed by the Tauri application. The route
//! answers 404 before it is registered, and it never fetches a URL supplied by the request.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gamenode_platform::covers::{artwork_dir, cover_file_name};
use gamenode_platform::ports::art::{
    PortArtError, PortArtImage, PortArtKind, PortArtLoader, PortArtResolver,
};
use tauri::Manager;
use tauri::http::{Request, Response, StatusCode, Uri, header};

/// Scheme name used for asset requests.
pub const PROTOCOL_SCHEME: &str = "gamenode-asset";

/// Validates whether a file string is a safe single filename without path traversal or delimiters.
///
/// Rejects:
/// - Empty strings
/// - Strings starting with `.` (e.g. `.hidden`, `..`)
/// - Strings containing `/`, `\`, `:`, `\0`, or `..`
///
/// Accepts:
/// - Safe single filenames like `"620.jpg"`, `"cover_1.png"`
pub fn valid_cover_file(file: &str) -> bool {
    if file.is_empty() {
        return false;
    }
    if file.starts_with('.') {
        return false;
    }
    if file.contains('/')
        || file.contains('\\')
        || file.contains(':')
        || file.contains('\0')
        || file.contains("..")
    {
        return false;
    }
    true
}

/// Detects the MIME content type of an image buffer or falls back to the filename extension.
///
/// Magic bytes checked:
/// - JPEG: `[0xff, 0xd8, 0xff]`
/// - PNG: `[0x89, b'P', b'N', b'G']`
/// - WEBP: `RIFF....WEBP`
///
/// Fallbacks:
/// - `.png` -> `"image/png"`
/// - `.webp` -> `"image/webp"`
/// - Default -> `"image/jpeg"`
pub fn detect_image_mime(bytes: &[u8], filename: &str) -> &'static str {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return "image/jpeg";
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return "image/png";
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp";
    }

    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

/// Decodes standard `%XX` percent-encoded sequences into UTF-8 strings.
/// Returns `None` if percent-encoding is malformed or invalid UTF-8.
pub fn percent_decode(input: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(input.len());
    let mut iter = input.bytes();
    while let Some(b) = iter.next() {
        if b == b'%' {
            let h1 = iter.next()?;
            let h2 = iter.next()?;
            let hex_val = |b: u8| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                b'A'..=b'F' => Some(b - b'A' + 10),
                _ => None,
            };
            let byte = (hex_val(h1)? << 4) | hex_val(h2)?;
            bytes.push(byte);
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).ok()
}

/// Parsed asset route extracted from a URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedAsset {
    Cover {
        filename: String,
    },
    Artwork {
        game_id: u64,
    },
    PortArt {
        catalog_id: String,
        kind: PortArtKind,
    },
    AchievementIcon {
        hash: String,
    },
    Invalid,
}

/// Parses the host and path components of an asset URI according to specification:
/// - Host `"cover"` or `"covers"` -> filename is `path.trim_start_matches('/')`
/// - Path starting with `"/cover/"` or `"/covers/"` -> filename is segment after prefix
/// - Host `"artwork"` or `"artworks"` -> game_id is `path.trim_start_matches('/')`
/// - Path starting with `"/artwork/"` or `"/artworks/"` -> game_id is segment after prefix
/// - Path starting with `"/port-art/"` -> `<catalog_id>/<capsule|hero>`, decoded separately
///
/// A `port-art` path that does not have exactly two segments, or whose kind is not `capsule` or
/// `hero`, is not a route at all (`None`, served as 404). Percent-encoded separators inside a
/// catalog id are decoded and then rejected by the handler, so a request cannot smuggle a URL or a
/// path traversal through the segment.
pub fn parse_asset_uri_parts(host: Option<&str>, path: &str) -> Option<ParsedAsset> {
    let path_without_query = path
        .split('?')
        .next()
        .unwrap_or(path)
        .split('#')
        .next()
        .unwrap_or(path);

    let host_lower = host.map(|h| h.to_ascii_lowercase());
    let host_str = host_lower.as_deref().unwrap_or("");

    if host_str == "cover" || host_str == "covers" {
        let raw = path_without_query.trim_start_matches('/');
        let Some(decoded) = percent_decode(raw) else {
            return Some(ParsedAsset::Invalid);
        };
        return Some(ParsedAsset::Cover { filename: decoded });
    }

    if let Some(rest) = path_without_query.strip_prefix("/port-art/") {
        let mut segments = rest.split('/');
        let (Some(raw_catalog_id), Some(raw_kind), None) =
            (segments.next(), segments.next(), segments.next())
        else {
            return None;
        };
        if raw_catalog_id.is_empty() || raw_kind.is_empty() {
            return None;
        }
        let (Some(catalog_id), Some(kind)) =
            (percent_decode(raw_catalog_id), percent_decode(raw_kind))
        else {
            return Some(ParsedAsset::Invalid);
        };
        let kind = PortArtKind::parse(&kind)?;
        return Some(ParsedAsset::PortArt { catalog_id, kind });
    }

    if let Some(rest) = path_without_query
        .strip_prefix("/cover/")
        .or_else(|| path_without_query.strip_prefix("/covers/"))
    {
        let Some(decoded) = percent_decode(rest) else {
            return Some(ParsedAsset::Invalid);
        };
        return Some(ParsedAsset::Cover { filename: decoded });
    }

    if host_str == "achievement" {
        let raw = path_without_query.trim_start_matches('/');
        let Some(decoded) = percent_decode(raw) else {
            return Some(ParsedAsset::Invalid);
        };
        return Some(
            if gamenode_platform::achievement_icons::valid_icon_hash(&decoded) {
                ParsedAsset::AchievementIcon { hash: decoded }
            } else {
                ParsedAsset::Invalid
            },
        );
    }

    if let Some(rest) = path_without_query.strip_prefix("/achievement/") {
        let Some(decoded) = percent_decode(rest) else {
            return Some(ParsedAsset::Invalid);
        };
        return Some(
            if gamenode_platform::achievement_icons::valid_icon_hash(&decoded) {
                ParsedAsset::AchievementIcon { hash: decoded }
            } else {
                ParsedAsset::Invalid
            },
        );
    }

    if host_str == "artwork" || host_str == "artworks" {
        let raw = path_without_query.trim_start_matches('/');
        let Some(decoded) = percent_decode(raw) else {
            return Some(ParsedAsset::Invalid);
        };
        let Ok(game_id) = decoded.parse::<u64>() else {
            return Some(ParsedAsset::Invalid);
        };
        return Some(ParsedAsset::Artwork { game_id });
    }

    if let Some(rest) = path_without_query
        .strip_prefix("/artwork/")
        .or_else(|| path_without_query.strip_prefix("/artworks/"))
    {
        let Some(decoded) = percent_decode(rest) else {
            return Some(ParsedAsset::Invalid);
        };
        let Ok(game_id) = decoded.parse::<u64>() else {
            return Some(ParsedAsset::Invalid);
        };
        return Some(ParsedAsset::Artwork { game_id });
    }

    None
}

/// Parses a `tauri::http::Uri` into a `ParsedAsset`.
pub fn parse_asset_uri(uri: &Uri) -> Option<ParsedAsset> {
    parse_asset_uri_parts(uri.host(), uri.path())
}

/// Builds an HTTP response with common headers.
fn build_response(
    status: StatusCode,
    mime: Option<&str>,
    cache_control: Option<&str>,
    body: Vec<u8>,
) -> Response<Vec<u8>> {
    let mut builder = Response::builder().status(status);
    if let Some(m) = mime {
        builder = builder.header(header::CONTENT_TYPE, m);
    }
    if let Some(cc) = cache_control {
        builder = builder.header(header::CACHE_CONTROL, cc);
    }
    builder = builder.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    builder.body(body).unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Vec::new())
            .unwrap()
    })
}

/// Cache policy of covers: a cover file is named after its game and replaced whole, and the
/// `ETag` lets the WebView check a stale copy without downloading it again.
const COVER_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// A strong validator for a file: its size and modification time.
fn file_etag(metadata: &std::fs::Metadata) -> Option<String> {
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(format!(
        "\"{:x}-{:x}\"",
        metadata.len(),
        modified.as_nanos()
    ))
}

/// Whether an `If-None-Match` header value names `etag` (weak validators compare equal here
/// because a `GET` may use weak comparison).
fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    if_none_match.split(',').map(str::trim).any(|candidate| {
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
    })
}

fn with_etag(mut response: Response<Vec<u8>>, etag: Option<&str>) -> Response<Vec<u8>> {
    if let Some(value) = etag.and_then(|etag| header::HeaderValue::from_str(etag).ok()) {
        response.headers_mut().insert(header::ETAG, value);
    }
    response
}

/// Asynchronously handles a cover file request.
pub async fn handle_cover(filename: &str, covers_dir: &Path) -> Response<Vec<u8>> {
    handle_cover_conditional(filename, covers_dir, None).await
}

/// Handles a cover file request that may carry an `If-None-Match` header. A cover whose `ETag`
/// matches is answered with `304 Not Modified` and an empty body, without reading the file.
pub async fn handle_cover_conditional(
    filename: &str,
    covers_dir: &Path,
    if_none_match: Option<&str>,
) -> Response<Vec<u8>> {
    if !valid_cover_file(filename) {
        return build_response(StatusCode::BAD_REQUEST, None, None, Vec::new());
    }

    let path = covers_dir.join(filename);
    let if_none_match = if_none_match.map(str::to_owned);
    let filename = filename.to_owned();
    let read_result = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let etag = file.metadata().ok().and_then(|m| file_etag(&m));
        if let (Some(etag), Some(header)) = (&etag, &if_none_match)
            && etag_matches(header, etag)
        {
            return Ok((etag.clone().into(), None));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok::<_, std::io::Error>((etag, Some(bytes)))
    })
    .await;

    match read_result {
        Ok(Ok((etag, Some(bytes)))) => {
            let mime = detect_image_mime(&bytes, &filename);
            with_etag(
                build_response(StatusCode::OK, Some(mime), Some(COVER_CACHE_CONTROL), bytes),
                etag.as_deref(),
            )
        }
        Ok(Ok((etag, None))) => with_etag(
            build_response(
                StatusCode::NOT_MODIFIED,
                None,
                Some(COVER_CACHE_CONTROL),
                Vec::new(),
            ),
            etag.as_deref(),
        ),
        _ => build_response(StatusCode::NOT_FOUND, None, None, Vec::new()),
    }
}

/// Synchronously handles a cover file request.
pub fn handle_cover_sync(filename: &str, covers_dir: &Path) -> Response<Vec<u8>> {
    if !valid_cover_file(filename) {
        return build_response(StatusCode::BAD_REQUEST, None, None, Vec::new());
    }

    let path = covers_dir.join(filename);
    match std::fs::read(path) {
        Ok(bytes) => {
            let mime = detect_image_mime(&bytes, filename);
            build_response(StatusCode::OK, Some(mime), Some(COVER_CACHE_CONTROL), bytes)
        }
        Err(_) => build_response(StatusCode::NOT_FOUND, None, None, Vec::new()),
    }
}

/// Asynchronously handles an artwork request by checking memory first, then disk cache.
pub async fn handle_artwork<F>(
    game_id: u64,
    covers_dir: &Path,
    artwork_lookup: &F,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    // 1. In-memory artwork lookup
    if let Some(bytes) = artwork_lookup(game_id) {
        let mime = detect_image_mime(&bytes, &format!("{game_id}.jpg"));
        return build_response(
            StatusCode::OK,
            Some(mime),
            Some("public, max-age=3600"),
            bytes,
        );
    }

    // 2. Disk cache fallback
    let artwork_path = artwork_dir(covers_dir).join(cover_file_name(game_id));
    let fallback_path = covers_dir.join(cover_file_name(game_id));

    let disk_bytes = tokio::task::spawn_blocking(move || {
        if artwork_path.is_file() {
            std::fs::read(&artwork_path).ok()
        } else if fallback_path.is_file() {
            std::fs::read(&fallback_path).ok()
        } else {
            None
        }
    })
    .await
    .ok()
    .flatten();

    if let Some(bytes) = disk_bytes {
        let mime = detect_image_mime(&bytes, &format!("{game_id}.jpg"));
        build_response(
            StatusCode::OK,
            Some(mime),
            Some("public, max-age=3600"),
            bytes,
        )
    } else {
        build_response(StatusCode::NOT_FOUND, None, None, Vec::new())
    }
}

/// Synchronously handles an artwork request.
pub fn handle_artwork_sync<F>(
    game_id: u64,
    covers_dir: &Path,
    artwork_lookup: &F,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    // 1. In-memory lookup
    if let Some(bytes) = artwork_lookup(game_id) {
        let mime = detect_image_mime(&bytes, &format!("{game_id}.jpg"));
        return build_response(
            StatusCode::OK,
            Some(mime),
            Some("public, max-age=3600"),
            bytes,
        );
    }

    // 2. Disk cache fallback
    let artwork_path = artwork_dir(covers_dir).join(cover_file_name(game_id));
    let fallback_path = covers_dir.join(cover_file_name(game_id));

    let disk_bytes = if artwork_path.is_file() {
        std::fs::read(&artwork_path).ok()
    } else if fallback_path.is_file() {
        std::fs::read(&fallback_path).ok()
    } else {
        None
    };

    if let Some(bytes) = disk_bytes {
        let mime = detect_image_mime(&bytes, &format!("{game_id}.jpg"));
        build_response(
            StatusCode::OK,
            Some(mime),
            Some("public, max-age=3600"),
            bytes,
        )
    } else {
        build_response(StatusCode::NOT_FOUND, None, None, Vec::new())
    }
}

/// The `port-art` route state.
///
/// ## Wiring
///
/// The route is dormant until the app manages this value; every request is answered 404 while it
/// is absent, so older hosts and tests without port state keep working. The wiring agent constructs
/// it once, after the port store exists, and registers it on the builder:
///
/// ```ignore
/// use std::sync::Arc;
/// use gamenode_platform::ports::art::SnapshotPortArtResolver;
///
/// let resolver = Arc::new(SnapshotPortArtResolver::new(port_store.clone()));
/// builder = builder.manage(PortArtState::production(resolver, paths.data_dir())?);
/// ```
///
/// The resolver may instead be a closure reading the catalog worker's in-memory entries (they are
/// the stored snapshot with the overlay applied), which avoids a database read per tile:
///
/// ```ignore
/// let worker = Arc::clone(&catalog_worker);
/// let resolver: Arc<dyn PortArtResolver> = Arc::new(move |catalog_id: &str, kind| {
///     // Find the entry in `worker.entries()` and return `PortArtSource::Url(..)`,
///     // `PortArtSource::NoArt` or `PortArtSource::UnknownEntry`.
/// });
/// builder = builder.manage(PortArtState::production(resolver, paths.data_dir())?);
/// ```
///
/// `builder.manage` may run before or after protocol registration; state is looked up when a
/// request arrives. The fetcher and disk cache are production defaults
/// ([`PortArtLoader::production`]); tests build a state with a fake fetcher through
/// [`PortArtState::new`].
#[derive(Clone)]
pub struct PortArtState {
    loader: PortArtLoader,
}

impl PortArtState {
    /// Builds a state from an already-configured loader (used by tests and custom hosts).
    pub fn new(loader: PortArtLoader) -> Self {
        Self { loader }
    }

    /// The production state: HTTPS-only fetcher and `<data_dir>/ports/art/` cache.
    ///
    /// The resolver is the only source of a port's art URL; the request supplies just the catalog
    /// id and the size.
    pub fn production(
        resolver: Arc<dyn PortArtResolver>,
        data_dir: &Path,
    ) -> Result<Self, PortArtError> {
        Ok(Self {
            loader: PortArtLoader::production(resolver, data_dir)?,
        })
    }

    /// Loads one image. Blocking: only called from the protocol's blocking task.
    fn load(&self, catalog_id: &str, kind: PortArtKind) -> Result<PortArtImage, PortArtError> {
        self.loader.load(catalog_id, kind)
    }
}

/// Serves a cached achievement icon by its URL hash. The request never supplies a URL and a
/// missing file is a 404, so the list shows its placeholder.
async fn handle_achievement_icon(hash: &str, covers_dir: &Path) -> Response<Vec<u8>> {
    if !gamenode_platform::achievement_icons::valid_icon_hash(hash) {
        return build_response(StatusCode::BAD_REQUEST, None, None, Vec::new());
    }
    let path = gamenode_platform::achievement_icons::achievement_icon_dir(covers_dir)
        .join(format!("{hash}.img"));
    let read = tokio::task::spawn_blocking(move || std::fs::read(path)).await;
    match read {
        Ok(Ok(bytes)) => build_response(
            StatusCode::OK,
            Some(detect_image_mime(&bytes, hash)),
            Some("public, max-age=31536000, immutable"),
            bytes,
        ),
        _ => build_response(StatusCode::NOT_FOUND, None, None, Vec::new()),
    }
}

/// Cache policy of port art. The WebView URL is stable while the upstream URL recorded in the
/// snapshot can change, so the image must not be immutable.
const PORT_ART_CACHE_CONTROL: &str = "public, max-age=3600";

/// A catalog id is one opaque segment. It never becomes a file path or URL here, but refusing
/// separators and traversal shapes keeps a hostile request from looking like a route.
fn valid_port_art_catalog_id(catalog_id: &str) -> bool {
    !catalog_id.is_empty()
        && catalog_id.len() <= 200
        && !catalog_id.starts_with('.')
        && !catalog_id.contains(['/', '\\', ':', '\0'])
        && !catalog_id.contains("..")
}

/// Serves `port-art/<catalog_id>/<capsule|hero>`, resolving the upstream URL from the snapshot in
/// [`PortArtState`]. The fetch and cache write happen on a blocking worker.
async fn handle_port_art(
    catalog_id: &str,
    kind: PortArtKind,
    port_art: Option<&PortArtState>,
) -> Response<Vec<u8>> {
    if !valid_port_art_catalog_id(catalog_id) {
        return build_response(StatusCode::NOT_FOUND, None, None, Vec::new());
    }
    let Some(port_art) = port_art else {
        // No port state was registered on the app builder; the route has nothing to resolve.
        return build_response(StatusCode::NOT_FOUND, None, None, Vec::new());
    };

    let port_art = port_art.clone();
    let catalog_id = catalog_id.to_owned();
    let loaded = tokio::task::spawn_blocking(move || port_art.load(&catalog_id, kind)).await;

    match loaded {
        Ok(Ok(image)) => build_response(
            StatusCode::OK,
            Some(image.content_type),
            Some(PORT_ART_CACHE_CONTROL),
            image.bytes,
        ),
        Ok(Err(error)) => port_art_error_response(error),
        Err(error) => {
            // A panicking worker must not take down the protocol; the tile falls back to its
            // titled placeholder.
            tracing::warn!(%error, "the port art worker stopped unexpectedly");
            build_response(StatusCode::BAD_GATEWAY, None, None, Vec::new())
        }
    }
}

/// Maps a load failure to the response the WebView sees. A missing entry or missing art is 404;
/// an upstream failure (bad URL, network, oversize or non-image body) is 502.
fn port_art_error_response(error: PortArtError) -> Response<Vec<u8>> {
    let status = match error {
        PortArtError::UnknownEntry { .. } | PortArtError::NoArt { .. } => StatusCode::NOT_FOUND,
        PortArtError::RejectedUrl { .. }
        | PortArtError::Fetch(_)
        | PortArtError::TooLarge { .. }
        | PortArtError::NotImage
        | PortArtError::Io(_) => StatusCode::BAD_GATEWAY,
    };
    build_response(status, None, None, Vec::new())
}

/// Asynchronously handles an asset request for a parsed `tauri::http::Uri`.
pub async fn handle_asset_uri<F>(
    uri: &Uri,
    covers_dir: &Path,
    artwork_lookup: &F,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    handle_asset_uri_conditional(uri, covers_dir, artwork_lookup, None).await
}

/// Like [`handle_asset_uri`], with the request's `If-None-Match` header for covers.
pub async fn handle_asset_uri_conditional<F>(
    uri: &Uri,
    covers_dir: &Path,
    artwork_lookup: &F,
    if_none_match: Option<&str>,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    handle_asset_uri_conditional_with_port_art(uri, covers_dir, artwork_lookup, if_none_match, None)
        .await
}

/// Like [`handle_asset_uri_conditional`], with the app's optional [`PortArtState`]. Without state,
/// `port-art` requests are answered 404.
pub async fn handle_asset_uri_conditional_with_port_art<F>(
    uri: &Uri,
    covers_dir: &Path,
    artwork_lookup: &F,
    if_none_match: Option<&str>,
    port_art: Option<&PortArtState>,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    match parse_asset_uri(uri) {
        Some(ParsedAsset::Cover { filename }) => {
            handle_cover_conditional(&filename, covers_dir, if_none_match).await
        }
        Some(ParsedAsset::Artwork { game_id }) => {
            handle_artwork(game_id, covers_dir, artwork_lookup).await
        }
        Some(ParsedAsset::PortArt { catalog_id, kind }) => {
            handle_port_art(&catalog_id, kind, port_art).await
        }
        Some(ParsedAsset::AchievementIcon { hash }) => {
            handle_achievement_icon(&hash, covers_dir).await
        }
        Some(ParsedAsset::Invalid) => {
            build_response(StatusCode::BAD_REQUEST, None, None, Vec::new())
        }
        None => build_response(StatusCode::NOT_FOUND, None, None, Vec::new()),
    }
}

/// Synchronously handles an asset request for a parsed `tauri::http::Uri`.
///
/// The synchronous path never performs network I/O, so `port-art` is answered 404 here; the
/// asynchronous handler is the one the WebView uses.
pub fn handle_asset_uri_sync<F>(
    uri: &Uri,
    covers_dir: &Path,
    artwork_lookup: &F,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    match parse_asset_uri(uri) {
        Some(ParsedAsset::Cover { filename }) => handle_cover_sync(&filename, covers_dir),
        Some(ParsedAsset::Artwork { game_id }) => {
            handle_artwork_sync(game_id, covers_dir, artwork_lookup)
        }
        Some(ParsedAsset::PortArt { .. }) => {
            build_response(StatusCode::NOT_FOUND, None, None, Vec::new())
        }
        Some(ParsedAsset::AchievementIcon { hash }) => {
            let path = gamenode_platform::achievement_icons::achievement_icon_dir(covers_dir)
                .join(format!("{hash}.img"));
            match std::fs::read(path) {
                Ok(bytes) => build_response(
                    StatusCode::OK,
                    Some(detect_image_mime(&bytes, &hash)),
                    Some("public, max-age=31536000, immutable"),
                    bytes,
                ),
                Err(_) => build_response(StatusCode::NOT_FOUND, None, None, Vec::new()),
            }
        }
        Some(ParsedAsset::Invalid) => {
            build_response(StatusCode::BAD_REQUEST, None, None, Vec::new())
        }
        None => build_response(StatusCode::NOT_FOUND, None, None, Vec::new()),
    }
}

/// Asynchronously handles an asset request from a string URI.
pub async fn handle_asset_uri_str<F>(
    uri_str: &str,
    covers_dir: &Path,
    artwork_lookup: &F,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    let Ok(uri) = uri_str.parse::<Uri>() else {
        return build_response(StatusCode::BAD_REQUEST, None, None, Vec::new());
    };
    handle_asset_uri(&uri, covers_dir, artwork_lookup).await
}

/// Synchronously handles an asset request from a string URI.
pub fn handle_asset_uri_str_sync<F>(
    uri_str: &str,
    covers_dir: &Path,
    artwork_lookup: &F,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    let Ok(uri) = uri_str.parse::<Uri>() else {
        return build_response(StatusCode::BAD_REQUEST, None, None, Vec::new());
    };
    handle_asset_uri_sync(&uri, covers_dir, artwork_lookup)
}

/// Asynchronously handles a full Tauri `http::Request`.
pub async fn handle_request<F>(
    request: &Request<Vec<u8>>,
    covers_dir: &Path,
    artwork_lookup: &F,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    handle_request_with_port_art(request, covers_dir, artwork_lookup, None).await
}

/// Like [`handle_request`], with the app's optional [`PortArtState`].
pub async fn handle_request_with_port_art<F>(
    request: &Request<Vec<u8>>,
    covers_dir: &Path,
    artwork_lookup: &F,
    port_art: Option<&PortArtState>,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    let if_none_match = request
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok());
    handle_asset_uri_conditional_with_port_art(
        request.uri(),
        covers_dir,
        artwork_lookup,
        if_none_match,
        port_art,
    )
    .await
}

/// Reads the artwork bytes for `game_id` from the `Engine` when its detail is loaded, without
/// building a snapshot.
pub fn engine_artwork(engine: &crate::engine::Engine, game_id: u64) -> Option<Vec<u8>> {
    engine.artwork(game_id).map(|artwork| artwork.to_vec())
}

/// Extracts artwork bytes for `game_id` from a `Snapshot` when detail is loaded.
pub fn extract_artwork_from_snapshot(
    snapshot: &gamenode_core::Snapshot,
    game_id: u64,
) -> Option<Vec<u8>> {
    if let Some(library) = &snapshot.library
        && let Some(detail) = &library.detail
        && detail.row.status.game_id() == Some(game_id)
        && let Some(artwork) = &detail.artwork
    {
        return Some(artwork.to_vec());
    }
    None
}

/// Creates an artwork lookup closure backed by an `Engine` instance.
pub fn make_engine_artwork_lookup(
    engine: crate::engine::Engine,
) -> impl Fn(u64) -> Option<Vec<u8>> + Send + Sync + 'static {
    move |game_id| engine_artwork(&engine, game_id)
}

/// Registers the `gamenode-asset://` custom protocol on a Tauri application builder,
/// backed by the provided covers directory and optional `Engine`.
pub fn register_asset_protocol<R: tauri::Runtime>(
    builder: tauri::Builder<R>,
    covers_dir: PathBuf,
    engine: Option<crate::engine::Engine>,
) -> tauri::Builder<R> {
    match engine {
        Some(eng) => {
            let lookup = make_engine_artwork_lookup(eng);
            register_asset_protocol_with_provider(builder, covers_dir, lookup)
        }
        None => register_asset_protocol_with_provider(builder, covers_dir, |_| None),
    }
}

/// Registers the `gamenode-asset://` custom protocol on a Tauri builder with a custom artwork provider.
pub fn register_asset_protocol_with_provider<R: tauri::Runtime, F>(
    builder: tauri::Builder<R>,
    covers_dir: PathBuf,
    artwork_lookup: F,
) -> tauri::Builder<R>
where
    F: Fn(u64) -> Option<Vec<u8>> + Send + Sync + 'static,
{
    let covers_dir = Arc::new(covers_dir);
    let artwork_lookup = Arc::new(artwork_lookup);

    builder.register_asynchronous_uri_scheme_protocol(
        PROTOCOL_SCHEME,
        move |ctx, request, responder| {
            let covers_dir = Arc::clone(&covers_dir);
            let artwork_lookup = Arc::clone(&artwork_lookup);
            // The state is looked up per request, so `.manage(PortArtState)` may be registered
            // before or after this protocol. Without it, the route answers 404.
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                let port_art = app.try_state::<PortArtState>();
                let response = handle_request_with_port_art(
                    &request,
                    &covers_dir,
                    &*artwork_lookup,
                    port_art.as_deref(),
                )
                .await;
                responder.respond(response);
            });
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_platform::ports::art::{
        MAX_PORT_ART_BYTES, PortArtCache, PortArtFetcher, PortArtSource,
    };
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    #[test]
    fn valid_cover_file_rules() {
        // Must reject path traversal and unsafe characters
        assert!(!valid_cover_file("../secret.txt"));
        assert!(!valid_cover_file(".."));
        assert!(!valid_cover_file(".hidden"));
        assert!(!valid_cover_file("foo/bar.jpg"));
        assert!(!valid_cover_file("foo\\bar.jpg"));
        assert!(!valid_cover_file(""));
        assert!(!valid_cover_file("colon:test.jpg"));
        assert!(!valid_cover_file("null\0byte.png"));
        assert!(!valid_cover_file("nested/../path.jpg"));

        // Must accept safe single filenames
        assert!(valid_cover_file("620.jpg"));
        assert!(valid_cover_file("12345.png"));
        assert!(valid_cover_file("cover_1.png"));
        assert!(valid_cover_file("game-cover.webp"));
    }

    #[test]
    fn detect_image_mime_by_magic_bytes_and_extension() {
        let jpeg_magic = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10];
        assert_eq!(detect_image_mime(&jpeg_magic, "unknown"), "image/jpeg");
        assert_eq!(detect_image_mime(&jpeg_magic, "file.png"), "image/jpeg");

        let png_magic = [0x89, b'P', b'N', b'G', 0x0d, 0x0a];
        assert_eq!(detect_image_mime(&png_magic, "unknown"), "image/png");
        assert_eq!(detect_image_mime(&png_magic, "file.jpg"), "image/png");

        let mut webp_magic = vec![0u8; 16];
        webp_magic[0..4].copy_from_slice(b"RIFF");
        webp_magic[8..12].copy_from_slice(b"WEBP");
        assert_eq!(detect_image_mime(&webp_magic, "unknown"), "image/webp");

        // Fallback by extension when magic bytes are empty or unrecognized
        let empty: &[u8] = &[];
        assert_eq!(detect_image_mime(empty, "test.png"), "image/png");
        assert_eq!(detect_image_mime(empty, "test.webp"), "image/webp");
        assert_eq!(detect_image_mime(empty, "test.jpg"), "image/jpeg");
        assert_eq!(detect_image_mime(empty, "test.unknown"), "image/jpeg");
    }

    #[tokio::test]
    async fn serve_valid_jpeg_and_png_cover() {
        let dir = tempdir().unwrap();
        let jpeg_bytes = vec![0xff, 0xd8, 0xff, 0xe0, 0x01, 0x02, 0x03];
        let png_bytes = vec![0x89, b'P', b'N', b'G', 0x01, 0x02, 0x03];

        std::fs::write(dir.path().join("620.jpg"), &jpeg_bytes).unwrap();
        std::fs::write(dir.path().join("12345.png"), &png_bytes).unwrap();

        // 1. Standard URL format: gamenode-asset://localhost/cover/<file>
        let res = handle_asset_uri_str(
            "gamenode-asset://localhost/cover/620.jpg",
            dir.path(),
            &|_| None,
        )
        .await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            res.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
        assert_eq!(res.body(), &jpeg_bytes);

        // 2. Scheme-host format: gamenode-asset://cover/<file>
        let res =
            handle_asset_uri_str("gamenode-asset://cover/12345.png", dir.path(), &|_| None).await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png"
        );
        assert_eq!(
            res.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(res.body(), &png_bytes);

        // 3. Windows WebView2 mapped format: http://gamenode-asset.localhost/cover/<file>
        let res = handle_asset_uri_str(
            "http://gamenode-asset.localhost/cover/620.jpg",
            dir.path(),
            &|_| None,
        )
        .await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.body(), &jpeg_bytes);

        // Synchronous handler test
        let res_sync = handle_asset_uri_str_sync(
            "gamenode-asset://localhost/cover/620.jpg",
            dir.path(),
            &|_| None,
        );
        assert_eq!(res_sync.status(), StatusCode::OK);
        assert_eq!(res_sync.body(), &jpeg_bytes);
    }

    #[tokio::test]
    async fn covers_carry_an_etag_and_a_matching_one_gets_an_empty_304() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("620.jpg"), [0xff, 0xd8, 0xff, 0x01]).unwrap();
        let url = "gamenode-asset://localhost/cover/620.jpg";
        let request = |if_none_match: Option<&str>| {
            let mut builder = Request::builder().uri(url);
            if let Some(value) = if_none_match {
                builder = builder.header(header::IF_NONE_MATCH, value);
            }
            builder.body(Vec::new()).unwrap()
        };

        let first = handle_request(&request(None), dir.path(), &|_| None).await;
        assert_eq!(first.status(), StatusCode::OK);
        let etag = first.headers().get(header::ETAG).unwrap().to_str().unwrap();
        assert!(etag.starts_with('"') && etag.ends_with('"') && !etag.starts_with("W/"));

        for header_value in [
            etag.to_owned(),
            format!("\"other\", {etag}"),
            format!("W/{etag}"),
            "*".into(),
        ] {
            let res = handle_request(&request(Some(&header_value)), dir.path(), &|_| None).await;
            assert_eq!(res.status(), StatusCode::NOT_MODIFIED, "{header_value}");
            assert!(res.body().is_empty());
            assert_eq!(res.headers().get(header::ETAG).unwrap(), etag);
            assert_eq!(
                res.headers().get(header::CACHE_CONTROL).unwrap(),
                "public, max-age=31536000, immutable"
            );
        }

        // A different validator, or a changed file, gets the image.
        let res = handle_request(&request(Some("\"stale\"")), dir.path(), &|_| None).await;
        assert_eq!(res.status(), StatusCode::OK);
        std::fs::write(dir.path().join("620.jpg"), [0xff, 0xd8, 0xff, 0x01, 0x02]).unwrap();
        let res = handle_request(&request(Some(etag)), dir.path(), &|_| None).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_ne!(res.headers().get(header::ETAG).unwrap(), etag);

        // A missing cover is still a 404 whatever the validator.
        let missing = Request::builder()
            .uri("gamenode-asset://localhost/cover/none.jpg")
            .header(header::IF_NONE_MATCH, "*")
            .body(Vec::new())
            .unwrap();
        let res = handle_request(&missing, dir.path(), &|_| None).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn return_404_for_missing_cover_file() {
        let dir = tempdir().unwrap();
        let res = handle_asset_uri_str(
            "gamenode-asset://localhost/cover/missing.jpg",
            dir.path(),
            &|_| None,
        )
        .await;

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn return_400_for_path_traversal_attempts() {
        let dir = tempdir().unwrap();

        let malicious_uris = [
            "gamenode-asset://localhost/cover/../secret",
            "gamenode-asset://localhost/cover/sub/file.jpg",
            "gamenode-asset://localhost/cover/%2e%2e%2fsecret",
            "gamenode-asset://localhost/cover/%2e%2e",
            "gamenode-asset://localhost/cover/.hidden",
            "gamenode-asset://cover/..%2fsecret",
            "http://gamenode-asset.localhost/cover/../secret",
        ];

        for uri in malicious_uris {
            let res = handle_asset_uri_str(uri, dir.path(), &|_| None).await;
            assert!(
                matches!(
                    res.status(),
                    StatusCode::BAD_REQUEST | StatusCode::FORBIDDEN
                ),
                "expected 400 or 403 for URI {uri}, got {}",
                res.status()
            );
        }

        // Also test synchronous handler
        for uri in malicious_uris {
            let res = handle_asset_uri_str_sync(uri, dir.path(), &|_| None);
            assert!(
                matches!(
                    res.status(),
                    StatusCode::BAD_REQUEST | StatusCode::FORBIDDEN
                ),
                "expected 400 or 403 for sync URI {uri}, got {}",
                res.status()
            );
        }
    }

    #[tokio::test]
    async fn serve_in_memory_landscape_artwork_bytes() {
        let dir = tempdir().unwrap();
        let artwork_bytes = vec![0xff, 0xd8, 0xff, 0xe0, 0x10, 0x20, 0x30];

        let lookup = |id: u64| {
            if id == 620 {
                Some(artwork_bytes.clone())
            } else {
                None
            }
        };

        // 1. Standard format with query param: gamenode-asset://localhost/artwork/620?rev=1
        let res = handle_asset_uri_str(
            "gamenode-asset://localhost/artwork/620?rev=1",
            dir.path(),
            &lookup,
        )
        .await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            res.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=3600"
        );
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
        assert_eq!(res.body(), &artwork_bytes);

        // 2. Windows WebView2 mapped format: http://gamenode-asset.localhost/artwork/620
        let res = handle_asset_uri_str(
            "http://gamenode-asset.localhost/artwork/620",
            dir.path(),
            &lookup,
        )
        .await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.body(), &artwork_bytes);

        // 3. Scheme-host format: gamenode-asset://artwork/620
        let res = handle_asset_uri_str("gamenode-asset://artwork/620", dir.path(), &lookup).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.body(), &artwork_bytes);

        // 4. Missing artwork returns 404
        let res_missing = handle_asset_uri_str(
            "gamenode-asset://localhost/artwork/999",
            dir.path(),
            &lookup,
        )
        .await;
        assert_eq!(res_missing.status(), StatusCode::NOT_FOUND);

        // 5. Invalid game_id returns 400
        let res_invalid = handle_asset_uri_str(
            "gamenode-asset://localhost/artwork/not-a-number",
            dir.path(),
            &lookup,
        )
        .await;
        assert_eq!(res_invalid.status(), StatusCode::BAD_REQUEST);

        // Synchronous handler test
        let res_sync = handle_asset_uri_str_sync(
            "gamenode-asset://localhost/artwork/620?rev=1",
            dir.path(),
            &lookup,
        );
        assert_eq!(res_sync.status(), StatusCode::OK);
        assert_eq!(res_sync.body(), &artwork_bytes);
    }

    #[tokio::test]
    async fn serve_disk_cached_artwork_fallback() {
        let dir = tempdir().unwrap();
        let artwork_dir = gamenode_platform::covers::artwork_dir(dir.path());
        std::fs::create_dir_all(&artwork_dir).unwrap();

        let disk_bytes = vec![0xff, 0xd8, 0xff, 0xe0, 0x99, 0x88];
        std::fs::write(artwork_dir.join(cover_file_name(777)), &disk_bytes).unwrap();

        // Memory lookup returns None
        let lookup = |_| None;

        let res = handle_asset_uri_str(
            "gamenode-asset://localhost/artwork/777",
            dir.path(),
            &lookup,
        )
        .await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=3600"
        );
        assert_eq!(res.body(), &disk_bytes);
    }

    #[tokio::test]
    async fn artwork_from_the_previous_cache_version_is_not_served() {
        let dir = tempdir().unwrap();
        // Version 1 stored 1920x1080 images in `<covers>/artworks`; they must not be reused now
        // that the details pane asks for a different size.
        let legacy_dir = dir.path().join("artworks");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(
            legacy_dir.join(cover_file_name(777)),
            [0xff, 0xd8, 0xff, 0x01],
        )
        .unwrap();

        let lookup = |_| None;
        let uri = "gamenode-asset://localhost/artwork/777";
        let res = handle_asset_uri_str(uri, dir.path(), &lookup).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let res = handle_asset_uri_str_sync(uri, dir.path(), &lookup);
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn port_art_route_parses_the_catalog_id_and_kind() {
        assert_eq!(
            parse_asset_uri_parts(Some("localhost"), "/port-art/qcat_1%5F%32/capsule"),
            Some(ParsedAsset::PortArt {
                catalog_id: "qcat_1_2".into(),
                kind: PortArtKind::Capsule,
            })
        );
        assert_eq!(
            parse_asset_uri_parts(Some("localhost"), "/port-art/qcat_1/hero?rev=2"),
            Some(ParsedAsset::PortArt {
                catalog_id: "qcat_1".into(),
                kind: PortArtKind::Hero,
            })
        );
        // Exactly two segments and a known kind are required.
        assert_eq!(
            parse_asset_uri_parts(
                Some("localhost"),
                "/port-art/https%3A%2F%2Fevil.example%2Fx.png"
            ),
            None
        );
        assert_eq!(
            parse_asset_uri_parts(Some("localhost"), "/port-art/qcat_1/banner"),
            None
        );
        assert_eq!(
            parse_asset_uri_parts(Some("localhost"), "/port-art/qcat_1/hero/extra"),
            None
        );
    }

    /// A fetcher that records every call, so a test can prove no network was attempted.
    struct FakePortArtFetcher {
        bytes: Vec<u8>,
        calls: AtomicUsize,
        urls: Mutex<Vec<String>>,
    }

    impl FakePortArtFetcher {
        fn new(bytes: &[u8]) -> Arc<Self> {
            Arc::new(Self {
                bytes: bytes.to_vec(),
                calls: AtomicUsize::new(0),
                urls: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }

        fn urls(&self) -> Vec<String> {
            self.urls.lock().unwrap().clone()
        }
    }

    impl PortArtFetcher for FakePortArtFetcher {
        fn fetch(&self, url: &str, _max_bytes: u64) -> Result<Vec<u8>, PortArtError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.urls.lock().unwrap().push(url.to_owned());
            Ok(self.bytes.clone())
        }
    }

    const PORT_ART_JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0x01, 0x02, 0x03];

    fn port_art_state(
        source: PortArtSource,
        fetcher: Arc<FakePortArtFetcher>,
        data_dir: &Path,
    ) -> PortArtState {
        let resolver: Arc<dyn PortArtResolver> =
            Arc::new(move |_: &str, _: PortArtKind| source.clone());
        PortArtState::new(PortArtLoader::new(
            resolver,
            fetcher,
            PortArtCache::new(data_dir),
        ))
    }

    fn port_art_request(uri: &str) -> Request<Vec<u8>> {
        Request::builder().uri(uri).body(Vec::new()).unwrap()
    }

    #[tokio::test]
    async fn port_art_returns_404_when_the_route_is_not_configured() {
        let dir = tempdir().unwrap();
        let res = handle_request(
            &port_art_request("gamenode-asset://localhost/port-art/qcat_1/capsule"),
            dir.path(),
            &|_| None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn port_art_unknown_catalog_id_and_missing_art_are_404_without_fetching() {
        let dir = tempdir().unwrap();
        for source in [PortArtSource::UnknownEntry, PortArtSource::NoArt] {
            let fetcher = FakePortArtFetcher::new(PORT_ART_JPEG);
            let state = port_art_state(source.clone(), Arc::clone(&fetcher), dir.path());
            let res = handle_request_with_port_art(
                &port_art_request("gamenode-asset://localhost/port-art/qcat_1/capsule"),
                dir.path(),
                &|_| None,
                Some(&state),
            )
            .await;
            assert_eq!(res.status(), StatusCode::NOT_FOUND, "{source:?}");
            assert_eq!(fetcher.calls(), 0, "{source:?}");
        }
    }

    #[tokio::test]
    async fn port_art_never_fetches_a_url_supplied_by_the_request() {
        let dir = tempdir().unwrap();
        let fetcher = FakePortArtFetcher::new(PORT_ART_JPEG);
        let state = port_art_state(
            PortArtSource::Url("https://cdn.example/capsule.png".into()),
            Arc::clone(&fetcher),
            dir.path(),
        );

        for uri in [
            // A URL pasted in place of the catalog id.
            "gamenode-asset://localhost/port-art/https%3A%2F%2Fevil.example%2Fx.png",
            "gamenode-asset://localhost/port-art/https%3A%2F%2Fevil.example%2Fx.png/hero",
            // Encoded separators cannot smuggle a path either.
            "gamenode-asset://localhost/port-art/..%2F..%2Fetc/hero",
            // A valid-looking route with an unknown size.
            "gamenode-asset://localhost/port-art/qcat_1/banner",
            // Extra segments are not a route.
            "gamenode-asset://localhost/port-art/qcat_1/hero/extra",
        ] {
            let res = handle_request_with_port_art(
                &port_art_request(uri),
                dir.path(),
                &|_| None,
                Some(&state),
            )
            .await;
            assert_eq!(res.status(), StatusCode::NOT_FOUND, "{uri}");
        }
        assert_eq!(fetcher.calls(), 0, "no request-supplied URL was fetched");
    }

    #[tokio::test]
    async fn port_art_fetches_only_the_url_recorded_in_the_snapshot() {
        let dir = tempdir().unwrap();
        let fetcher = FakePortArtFetcher::new(PORT_ART_JPEG);
        let state = port_art_state(
            PortArtSource::Url("https://cdn.example/hero.png".into()),
            Arc::clone(&fetcher),
            dir.path(),
        );

        let res = handle_request_with_port_art(
            &port_art_request(
                "gamenode-asset://localhost/port-art/qcat_1/hero?url=https%3A%2F%2Fevil.example%2Fx.png",
            ),
            dir.path(),
            &|_| None,
            Some(&state),
        )
        .await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(fetcher.urls(), vec!["https://cdn.example/hero.png"]);
    }

    #[tokio::test]
    async fn port_art_refuses_a_non_https_snapshot_url() {
        let dir = tempdir().unwrap();
        let fetcher = FakePortArtFetcher::new(PORT_ART_JPEG);
        let state = port_art_state(
            PortArtSource::Url("http://cdn.example/capsule.png".into()),
            Arc::clone(&fetcher),
            dir.path(),
        );

        let res = handle_request_with_port_art(
            &port_art_request("gamenode-asset://localhost/port-art/qcat_1/capsule"),
            dir.path(),
            &|_| None,
            Some(&state),
        )
        .await;

        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(fetcher.calls(), 0);
    }

    #[tokio::test]
    async fn port_art_refuses_oversize_and_non_image_bodies() {
        let dir = tempdir().unwrap();
        let uri = "gamenode-asset://localhost/port-art/qcat_1/capsule";

        let oversize = FakePortArtFetcher::new(&vec![0u8; MAX_PORT_ART_BYTES as usize + 1]);
        let state = port_art_state(
            PortArtSource::Url("https://cdn.example/capsule.png".into()),
            Arc::clone(&oversize),
            dir.path(),
        );
        let res = handle_request_with_port_art(
            &port_art_request(uri),
            dir.path(),
            &|_| None,
            Some(&state),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(oversize.calls(), 1);

        let text = FakePortArtFetcher::new(b"<html>not an image</html>");
        let state = port_art_state(
            PortArtSource::Url("https://cdn.example/capsule.png".into()),
            Arc::clone(&text),
            dir.path(),
        );
        let res = handle_request_with_port_art(
            &port_art_request(uri),
            dir.path(),
            &|_| None,
            Some(&state),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(text.calls(), 1);
    }

    #[tokio::test]
    async fn port_art_serves_cached_bytes_with_the_right_content_type_and_never_refetches() {
        let dir = tempdir().unwrap();
        let fetcher = FakePortArtFetcher::new(PORT_ART_JPEG);
        let state = port_art_state(
            PortArtSource::Url("https://cdn.example/capsule.png".into()),
            Arc::clone(&fetcher),
            dir.path(),
        );
        let uri = "gamenode-asset://localhost/port-art/qcat_1/capsule";

        let first = handle_request_with_port_art(
            &port_art_request(uri),
            dir.path(),
            &|_| None,
            Some(&state),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(
            first.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            first.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=3600"
        );
        assert_eq!(
            first
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
        assert_eq!(first.body().as_slice(), PORT_ART_JPEG);

        let second = handle_request_with_port_art(
            &port_art_request(uri),
            dir.path(),
            &|_| None,
            Some(&state),
        )
        .await;
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(second.body().as_slice(), PORT_ART_JPEG);
        assert_eq!(
            fetcher.calls(),
            1,
            "the disk cache must answer the second request"
        );
    }

    #[test]
    fn engine_artwork_lookup_from_snapshot() {
        use gamenode_core::library::{
            DetailView, MatchSource, MatchStatus, Membership, Row,
            actions::{Actions, Availability, Reason},
        };
        use gamenode_core::playtime::PlaytimeView;

        let artwork_bytes: Arc<[u8]> = Arc::from(vec![0xff, 0xd8, 0xff, 0xe0, 1, 2, 3]);
        let mut snapshot = gamenode_core::Snapshot::new(
            &gamenode_core::auth::AuthMachine::new(Box::new(|| "nonce".into())),
            gamenode_core::StorageMethod::PrivateFile,
            Default::default(),
            None,
            None,
        );
        let detail = DetailView {
            running: false,
            row: Arc::new(Row {
                playtime: PlaytimeView {
                    seconds: None,
                    source: gamenode_core::playtime::PlaytimeSource::Internal,
                    last_played_at: None,
                },
                running: false,
                key: "steam:620".into(),
                title: "Portal 2".into(),
                detected_name: "Portal 2".into(),
                matched_name: None,
                provider: "steam".into(),
                provider_name: "Steam".into(),
                installed: true,
                owned: false,
                family_shared: false,
                status: MatchStatus::Matched {
                    game_id: 620,
                    source: MatchSource::Automatic,
                },
                cover_file: None,
                release_year: Some(2011),
                added_at: 0,
                is_favorite: false,
            }),
            install_dir: "/games/portal2".into(),
            release_year: Some(2011),
            has_artwork: true,
            artwork: Some(artwork_bytes.clone()),
            membership: Membership::NotChecked,
            achievement_backend: false,
            achievements: None,
        };

        // Snapshot without library returns None
        assert_eq!(extract_artwork_from_snapshot(&snapshot, 620), None);

        // Snapshot with detail
        snapshot.library = Some(gamenode_core::library::LibraryView {
            generation: 1,
            layout: Default::default(),
            search: String::new(),
            access: Default::default(),
            total: 1,
            installed_total: 1,
            providers: Vec::new(),
            rows: Arc::from([]),
            scanning: false,
            failed_providers: Vec::new(),
            selected: Vec::new(),
            focused: None,
            actions: Actions {
                selected: 0,
                play: Availability::Disabled(Reason::NothingSelected),
                install: Availability::Disabled(Reason::NothingSelected),
                match_game: Availability::Disabled(Reason::NothingSelected),
                add_to_gamenode: Availability::Disabled(Reason::NothingSelected),
                change_match: false,
            },
            detail: Some(detail),
            empty: None,
            match_dialog: None,
            add_dialog: None,
        });

        // Matching game_id returns bytes
        let found = extract_artwork_from_snapshot(&snapshot, 620);
        assert_eq!(found, Some(vec![0xff, 0xd8, 0xff, 0xe0, 1, 2, 3]));

        // Non-matching game_id returns None
        let missing = extract_artwork_from_snapshot(&snapshot, 999);
        assert_eq!(missing, None);
    }
}

#[cfg(test)]
mod achievement_icon_tests {
    use super::*;
    use gamenode_platform::achievement_icons::{achievement_icon_dir, achievement_icon_hash};

    const URL: &str =
        "https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps/620/a.jpg";

    #[tokio::test]
    async fn a_cached_icon_is_served_and_a_missing_one_is_404() {
        let dir = tempfile::tempdir().unwrap();
        let hash = achievement_icon_hash(URL);
        let icons = achievement_icon_dir(dir.path());
        std::fs::create_dir_all(&icons).unwrap();
        let bytes = [0xff, 0xd8, 0xff, 0xe0, 1, 2, 3];
        std::fs::write(icons.join(format!("{hash}.img")), bytes).unwrap();
        let uri = format!("gamenode-asset://localhost/achievement/{hash}");
        let res = handle_asset_uri_str(&uri, dir.path(), &|_| None).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.body(), &bytes);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
        let sync = handle_asset_uri_str_sync(&uri, dir.path(), &|_| None);
        assert_eq!(sync.status(), StatusCode::OK);

        let missing = achievement_icon_hash("https://cdn.akamai.steamstatic.com/missing.jpg");
        let res = handle_asset_uri_str(
            &format!("gamenode-asset://localhost/achievement/{missing}"),
            dir.path(),
            &|_| None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn path_traversal_and_malformed_hashes_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for uri in [
            "gamenode-asset://localhost/achievement/../secret",
            "gamenode-asset://localhost/achievement/%2e%2e%2fsecret",
            "gamenode-asset://localhost/achievement/short",
            "gamenode-asset://localhost/achievement/gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg",
        ] {
            let res = handle_asset_uri_str(uri, dir.path(), &|_| None).await;
            assert!(
                matches!(
                    res.status(),
                    StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
                ),
                "{uri}: {}",
                res.status()
            );
            let sync = handle_asset_uri_str_sync(uri, dir.path(), &|_| None);
            assert!(
                matches!(
                    sync.status(),
                    StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
                ),
                "{uri}: {}",
                sync.status()
            );
        }
    }

    #[test]
    fn the_route_parses_only_the_hash() {
        let hash = achievement_icon_hash(URL);
        assert_eq!(
            parse_asset_uri_parts(Some("localhost"), &format!("/achievement/{hash}")),
            Some(ParsedAsset::AchievementIcon { hash: hash.clone() })
        );
        assert_eq!(
            parse_asset_uri_parts(Some("achievement"), &format!("/{hash}")),
            Some(ParsedAsset::AchievementIcon { hash })
        );
    }
}
