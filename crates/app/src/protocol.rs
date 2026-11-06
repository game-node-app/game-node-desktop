//! Custom `gamenode-asset://` protocol implementation for Tauri v2.
//!
//! Serves cached game covers and landscape artwork directly to the WebView
//! off the main thread without decoding full uncompressed bitmaps in Rust memory.
//!
//! Supported schemes and formats:
//! - Standard: `gamenode-asset://localhost/cover/<file>` or `gamenode-asset://cover/<file>`
//! - Artwork: `gamenode-asset://localhost/artwork/<game_id>?rev=...` or `gamenode-asset://artwork/<game_id>`
//! - Windows WebView2 mapped URL: `http://gamenode-asset.localhost/cover/<file>` and
//!   `http://gamenode-asset.localhost/artwork/<game_id>`

use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    Cover { filename: String },
    Artwork { game_id: u64 },
    Invalid,
}

/// Parses the host and path components of an asset URI according to specification:
/// - Host `"cover"` or `"covers"` -> filename is `path.trim_start_matches('/')`
/// - Path starting with `"/cover/"` or `"/covers/"` -> filename is segment after prefix
/// - Host `"artwork"` or `"artworks"` -> game_id is `path.trim_start_matches('/')`
/// - Path starting with `"/artwork/"` or `"/artworks/"` -> game_id is segment after prefix
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

    if let Some(rest) = path_without_query
        .strip_prefix("/cover/")
        .or_else(|| path_without_query.strip_prefix("/covers/"))
    {
        let Some(decoded) = percent_decode(rest) else {
            return Some(ParsedAsset::Invalid);
        };
        return Some(ParsedAsset::Cover { filename: decoded });
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

/// Asynchronously handles a cover file request.
pub async fn handle_cover(filename: &str, covers_dir: &Path) -> Response<Vec<u8>> {
    if !valid_cover_file(filename) {
        return build_response(StatusCode::BAD_REQUEST, None, None, Vec::new());
    }

    let path = covers_dir.join(filename);
    let read_result = tokio::task::spawn_blocking(move || std::fs::read(path)).await;

    match read_result {
        Ok(Ok(bytes)) => {
            let mime = detect_image_mime(&bytes, filename);
            build_response(
                StatusCode::OK,
                Some(mime),
                Some("public, max-age=31536000, immutable"),
                bytes,
            )
        }
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
            build_response(
                StatusCode::OK,
                Some(mime),
                Some("public, max-age=31536000, immutable"),
                bytes,
            )
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
    let artwork_path = covers_dir.join("artworks").join(format!("{game_id}.jpg"));
    let fallback_path = covers_dir.join(format!("{game_id}.jpg"));

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
    let artwork_path = covers_dir.join("artworks").join(format!("{game_id}.jpg"));
    let fallback_path = covers_dir.join(format!("{game_id}.jpg"));

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

/// Asynchronously handles an asset request for a parsed `tauri::http::Uri`.
pub async fn handle_asset_uri<F>(
    uri: &Uri,
    covers_dir: &Path,
    artwork_lookup: &F,
) -> Response<Vec<u8>>
where
    F: Fn(u64) -> Option<Vec<u8>> + ?Sized,
{
    match parse_asset_uri(uri) {
        Some(ParsedAsset::Cover { filename }) => handle_cover(&filename, covers_dir).await,
        Some(ParsedAsset::Artwork { game_id }) => {
            handle_artwork(game_id, covers_dir, artwork_lookup).await
        }
        Some(ParsedAsset::Invalid) => {
            build_response(StatusCode::BAD_REQUEST, None, None, Vec::new())
        }
        None => build_response(StatusCode::NOT_FOUND, None, None, Vec::new()),
    }
}

/// Synchronously handles an asset request for a parsed `tauri::http::Uri`.
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
    handle_asset_uri(request.uri(), covers_dir, artwork_lookup).await
}

/// Extracts artwork bytes for `game_id` from an `Engine` snapshot when detail is loaded.
pub fn engine_artwork(engine: &crate::engine::Engine, game_id: u64) -> Option<Vec<u8>> {
    extract_artwork_from_snapshot(&engine.snapshot(), game_id)
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
        move |_ctx, request, responder| {
            let covers_dir = Arc::clone(&covers_dir);
            let artwork_lookup = Arc::clone(&artwork_lookup);
            tauri::async_runtime::spawn(async move {
                let response = handle_request(&request, &covers_dir, &*artwork_lookup).await;
                responder.respond(response);
            });
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let artwork_dir = dir.path().join("artworks");
        std::fs::create_dir_all(&artwork_dir).unwrap();

        let disk_bytes = vec![0xff, 0xd8, 0xff, 0xe0, 0x99, 0x88];
        std::fs::write(artwork_dir.join("777.jpg"), &disk_bytes).unwrap();

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
                },
                running: false,
                key: "steam:620".into(),
                title: "Portal 2".into(),
                detected_name: "Portal 2".into(),
                matched_name: None,
                provider: "steam".into(),
                provider_name: "Steam".into(),
                status: MatchStatus::Matched {
                    game_id: 620,
                    source: MatchSource::Automatic,
                },
                cover_file: None,
            }),
            install_dir: "/games/portal2".into(),
            release_year: Some(2011),
            has_artwork: true,
            artwork: Some(artwork_bytes.clone()),
            membership: Membership::NotChecked,
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
