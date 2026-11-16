//! Cover images on disk: `<covers dir>/<game_id>.jpg`.
//!
//! A cover is written to a temporary file in the same folder and renamed over the final name, so
//! a reader never sees half an image and a failed write leaves the previous cover alone. The file
//! is not flushed to the disk first: covers are cheap to download again, and an `fsync` per cover
//! stalls a burst of downloads. Covers are keyed by game id: changing an entry's match just
//! points it at another file.

use std::fs::{self, File};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};

use image::{
    ExtendedColorType, GenericImageView, ImageEncoder, ImageFormat, ImageReader,
    codecs::webp::WebPEncoder, imageops::FilterType,
};
use sha2::{Digest, Sha256};

/// The largest source image accepted for a custom cover, before it is decoded.
pub const MAX_COVER_SOURCE_BYTES: u64 = 20 * 1024 * 1024;

/// A decoded cover larger than this is refused instead of allocating an unbounded buffer.
const MAX_COVER_SOURCE_PIXELS: u64 = 40_000_000;

/// The bounding box a custom cover is re-encoded into.
pub const MAX_LOCAL_COVER_WIDTH: u32 = 600;
pub const MAX_LOCAL_COVER_HEIGHT: u32 = 800;

/// Why a custom cover could not be picked, read or re-encoded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CoverImageError {
    #[error("the image is larger than 20 MB")]
    TooLarge,
    #[error("the image format is not PNG, JPEG or WebP")]
    UnsupportedFormat,
    #[error("the image could not be read")]
    Unreadable,
    #[error("the image picker could not be opened")]
    Picker,
    #[error("the image picker is unavailable on this platform")]
    Unsupported,
    #[error("the converted cover could not be saved")]
    Save,
    #[error("invalid local game")]
    InvalidTarget,
}

impl CoverImageError {
    /// A stable code the app passes to the UI, which owns the localized message.
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooLarge => "coverTooLarge",
            Self::UnsupportedFormat | Self::Unreadable => "coverUnreadable",
            Self::Picker => "coverPickerFailed",
            Self::Unsupported => "coverUnsupported",
            Self::Save => "coverSaveFailed",
            Self::InvalidTarget => "invalidTarget",
        }
    }
}

/// Opens the native image picker (PNG/JPEG/WebP) and reads the chosen file.
///
/// The picker and the read block, so callers run this on a worker. A cancelled dialog is
/// `Ok(None)` and is not an error.
pub fn pick_cover_image() -> Result<Option<Vec<u8>>, CoverImageError> {
    let path = super::installations::pick_file_with_extensions(
        &["png".into(), "jpg".into(), "jpeg".into(), "webp".into()],
        None,
    )
    .map_err(|error| match error {
        gamenode_core::drm_free::LocalError::Unsupported => CoverImageError::Unsupported,
        _ => CoverImageError::Picker,
    })?;
    let Some(path) = path else {
        return Ok(None);
    };
    read_cover_source(Path::new(&path)).map(Some)
}

/// Reads a source image, enforcing the size limit before any decoding happens.
pub fn read_cover_source(path: &Path) -> Result<Vec<u8>, CoverImageError> {
    let metadata = fs::metadata(path).map_err(|_| CoverImageError::Unreadable)?;
    if metadata.len() > MAX_COVER_SOURCE_BYTES {
        return Err(CoverImageError::TooLarge);
    }
    let bytes = fs::read(path).map_err(|_| CoverImageError::Unreadable)?;
    if bytes.len() as u64 > MAX_COVER_SOURCE_BYTES {
        return Err(CoverImageError::TooLarge);
    }
    Ok(bytes)
}

/// Decodes a PNG/JPEG/WebP image, fits it into 600×800 and writes it as a WebP file named
/// `local-<registration-id>-<hash>.webp` in the cover cache. It returns the file name.
///
/// The format is detected from the bytes, never from a file extension. Decoding and resizing are
/// expensive, so this must run on a blocking worker.
pub fn reencode_local_cover(
    dir: &Path,
    registration_id: &str,
    source: &[u8],
) -> Result<String, CoverImageError> {
    if source.len() as u64 > MAX_COVER_SOURCE_BYTES {
        return Err(CoverImageError::TooLarge);
    }

    let reader = ImageReader::new(Cursor::new(source))
        .with_guessed_format()
        .map_err(|_| CoverImageError::Unreadable)?;
    let format = reader.format().ok_or(CoverImageError::UnsupportedFormat)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err(CoverImageError::UnsupportedFormat);
    }

    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| CoverImageError::Unreadable)?;
    if width == 0 || height == 0 {
        return Err(CoverImageError::Unreadable);
    }
    if u64::from(width) * u64::from(height) > MAX_COVER_SOURCE_PIXELS {
        return Err(CoverImageError::TooLarge);
    }

    let decoded = ImageReader::new(Cursor::new(source))
        .with_guessed_format()
        .map_err(|_| CoverImageError::Unreadable)?
        .decode()
        .map_err(|_| CoverImageError::Unreadable)?;
    let resized = fit_cover(decoded);
    let rgba = resized.to_rgba8();
    let mut encoded = Vec::new();
    WebPEncoder::new_lossless(&mut encoded)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|_| CoverImageError::Save)?;

    let mut hasher = Sha256::new();
    hasher.update(&encoded);
    let digest = hasher.finalize();
    let hash: String = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let name = format!(
        "local-{}-{hash}.webp",
        safe_registration_id(registration_id)
    );

    fs::create_dir_all(dir).map_err(|_| CoverImageError::Save)?;
    write_atomically(dir, &name, &encoded).map_err(|_| CoverImageError::Save)?;
    Ok(name)
}

/// Whether `name` is a custom-cover file name produced for `registration_id`.
pub fn is_local_cover_name(registration_id: &str, name: &str) -> bool {
    let prefix = format!("local-{}-", safe_registration_id(registration_id));
    let Some(hash) = name
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_suffix(".webp"))
    else {
        return false;
    };
    hash.len() == 16
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !name.contains(['/', '\\', ':', '\0'])
        && name.len() <= 255
}

/// Whether `name` has the shape of any custom-cover file (no registration id check).
pub fn is_local_cover_file(name: &str) -> bool {
    if name.contains(['/', '\\', ':', '\0']) || name.len() > 255 {
        return false;
    }
    let Some(rest) = name
        .strip_prefix("local-")
        .and_then(|rest| rest.strip_suffix(".webp"))
    else {
        return false;
    };
    let Some((_, hash)) = rest.rsplit_once('-') else {
        return false;
    };
    hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Fits an image inside the 600×800 box without enlarging it or changing its aspect ratio.
fn fit_cover(image: image::DynamicImage) -> image::DynamicImage {
    let (width, height) = image.dimensions();
    if width <= MAX_LOCAL_COVER_WIDTH && height <= MAX_LOCAL_COVER_HEIGHT {
        return image;
    }
    let scale = f64::min(
        f64::from(MAX_LOCAL_COVER_WIDTH) / f64::from(width),
        f64::from(MAX_LOCAL_COVER_HEIGHT) / f64::from(height),
    );
    let width = ((f64::from(width) * scale).floor() as u32).max(1);
    let height = ((f64::from(height) * scale).floor() as u32).max(1);
    image.resize_exact(width, height, FilterType::Lanczos3)
}

/// File names only keep characters that are valid on every platform and in the asset protocol.
fn safe_registration_id(id: &str) -> String {
    let mut safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(48)
        .collect();
    if safe.is_empty() {
        safe.push_str("local");
    }
    safe
}

fn write_atomically(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let temp = dir.join(format!(".{name}.{}.part", std::process::id()));
    let written = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        drop(file);
        fs::rename(&temp, dir.join(name))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// The folder under the covers directory that holds landscape artwork.
///
/// The version suffix invalidates the cache when the requested artwork size changes: version 1
/// stored 1920x1080 (`t_1080p`) images, while the details pane now asks for 889x500
/// (`t_screenshot_big`). Bump it again if the requested image changes.
pub const ARTWORK_DIR: &str = "artworks-v2";

/// The directory a game's landscape artwork is stored in under the covers directory.
pub fn artwork_dir(covers_dir: &Path) -> PathBuf {
    covers_dir.join(ARTWORK_DIR)
}

/// The file name a game's cover is stored under.
pub fn cover_file_name(game_id: u64) -> String {
    format!("{game_id}.jpg")
}

/// Writes a game's cover atomically and returns its file name.
pub fn write_cover(dir: &Path, game_id: u64, bytes: &[u8]) -> io::Result<String> {
    fs::create_dir_all(dir)?;
    let name = cover_file_name(game_id);
    let temp = dir.join(format!(".{name}.{}.part", std::process::id()));

    let written = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        drop(file);
        fs::rename(&temp, dir.join(&name))
    })();
    if written.is_err() {
        // Best effort: a leftover `.part` file is harmless but should not pile up.
        let _ = fs::remove_file(&temp);
    }
    written.map(|()| name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{
        Rgba, RgbaImage,
        codecs::{jpeg::JpegEncoder, png::PngEncoder},
    };
    use tempfile::TempDir;

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn sample(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba([40, 80, 160, 255]))
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = sample(width, height);
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let image = image::DynamicImage::ImageRgba8(sample(width, height)).to_rgb8();
        let mut bytes = Vec::new();
        JpegEncoder::new_with_quality(&mut bytes, 88)
            .write_image(image.as_raw(), width, height, ExtendedColorType::Rgb8)
            .unwrap();
        bytes
    }

    fn webp(width: u32, height: u32) -> Vec<u8> {
        let image = sample(width, height);
        let mut bytes = Vec::new();
        WebPEncoder::new_lossless(&mut bytes)
            .write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }

    #[test]
    fn a_cover_is_written_under_its_game_id_and_leaves_nothing_else_behind() {
        let dir = TempDir::new().unwrap();
        // The folder is created on first use.
        let covers = dir.path().join("covers");
        let name = write_cover(&covers, 7, b"\xFF\xD8\xFFimage").unwrap();
        assert_eq!(name, "7.jpg");
        assert_eq!(
            fs::read(covers.join("7.jpg")).unwrap(),
            b"\xFF\xD8\xFFimage"
        );
        assert_eq!(files(&covers), ["7.jpg"]);
    }

    #[test]
    fn a_new_cover_replaces_the_old_one_whole() {
        let dir = TempDir::new().unwrap();
        write_cover(dir.path(), 7, b"old cover, longer than the new one").unwrap();
        write_cover(dir.path(), 7, b"new").unwrap();
        assert_eq!(fs::read(dir.path().join("7.jpg")).unwrap(), b"new");
        assert_eq!(files(dir.path()), ["7.jpg"]);
    }

    #[test]
    fn different_games_have_different_files() {
        let dir = TempDir::new().unwrap();
        write_cover(dir.path(), 7, b"seven").unwrap();
        write_cover(dir.path(), 8, b"eight").unwrap();
        assert_eq!(files(dir.path()), ["7.jpg", "8.jpg"]);
        assert_eq!(fs::read(dir.path().join("7.jpg")).unwrap(), b"seven");
    }

    #[test]
    fn a_failed_write_reports_it_and_cleans_up() {
        let dir = TempDir::new().unwrap();
        // The final name is taken by a folder, so the rename cannot succeed.
        fs::create_dir(dir.path().join("7.jpg")).unwrap();
        assert!(write_cover(dir.path(), 7, b"image").is_err());
        assert_eq!(files(dir.path()), ["7.jpg"], "no temporary file is left");
        assert!(dir.path().join("7.jpg").is_dir());

        // A covers path that is a file cannot hold covers at all.
        let blocked = dir.path().join("not-a-folder");
        fs::write(&blocked, b"x").unwrap();
        assert!(write_cover(&blocked, 7, b"image").is_err());
    }

    #[test]
    fn png_jpeg_and_webp_sources_become_a_webp_cover_in_the_cache() {
        let dir = TempDir::new().unwrap();
        for (label, source) in [
            ("png", png(300, 400)),
            ("jpeg", jpeg(300, 400)),
            ("webp", webp(300, 400)),
        ] {
            let name = reencode_local_cover(dir.path(), "reg-1", &source)
                .unwrap_or_else(|error| panic!("{label}: {error}"));
            assert!(name.starts_with("local-reg-1-"), "{label}: {name}");
            assert!(name.ends_with(".webp"), "{label}: {name}");
            let bytes = fs::read(dir.path().join(&name)).unwrap();
            assert_eq!(&bytes[0..4], b"RIFF", "{label} is a WebP file");
            assert_eq!(&bytes[8..12], b"WEBP", "{label} is a WebP file");
            let decoded = image::load_from_memory(&bytes).unwrap();
            assert_eq!(decoded.dimensions(), (300, 400));
        }
        // PNG and WebP are lossless and reuse one file; the lossy JPEG re-encodes to another name.
        assert_eq!(files(dir.path()).len(), 2);
    }

    #[test]
    fn a_large_source_is_scaled_into_six_hundred_by_eight_hundred() {
        let dir = TempDir::new().unwrap();
        let portrait = reencode_local_cover(dir.path(), "reg-1", &png(900, 1200)).unwrap();
        let portrait = image::load_from_memory(&fs::read(dir.path().join(portrait)).unwrap())
            .unwrap()
            .dimensions();
        assert_eq!(portrait, (600, 800));

        let landscape = reencode_local_cover(dir.path(), "reg-1", &png(1200, 300)).unwrap();
        let landscape = image::load_from_memory(&fs::read(dir.path().join(landscape)).unwrap())
            .unwrap()
            .dimensions();
        assert_eq!(landscape, (600, 150));

        let tall = reencode_local_cover(dir.path(), "reg-1", &webp(1000, 2000)).unwrap();
        let tall = image::load_from_memory(&fs::read(dir.path().join(tall)).unwrap())
            .unwrap()
            .dimensions();
        assert_eq!(tall, (400, 800));
    }

    #[test]
    fn a_small_source_is_not_enlarged() {
        let dir = TempDir::new().unwrap();
        let name = reencode_local_cover(dir.path(), "reg-1", &png(120, 90)).unwrap();
        let dimensions = image::load_from_memory(&fs::read(dir.path().join(name)).unwrap())
            .unwrap()
            .dimensions();
        assert_eq!(dimensions, (120, 90));
    }

    #[test]
    fn the_same_image_for_the_same_registration_reuses_its_file_name() {
        let dir = TempDir::new().unwrap();
        let source = png(200, 300);
        let first = reencode_local_cover(dir.path(), "reg-1", &source).unwrap();
        let second = reencode_local_cover(dir.path(), "reg-1", &source).unwrap();
        assert_eq!(first, second);
        assert_eq!(files(dir.path()), [first.clone()]);
        assert!(is_local_cover_name("reg-1", &first));
        assert!(!is_local_cover_name("reg-2", &first));
        assert!(!is_local_cover_name("reg-1", "../reg-1.jpg"));
    }

    #[test]
    fn a_corrupt_source_is_rejected_without_writing_a_file() {
        let dir = TempDir::new().unwrap();
        assert_eq!(
            reencode_local_cover(dir.path(), "reg-1", b"not an image"),
            Err(CoverImageError::UnsupportedFormat)
        );
        // A file that starts like a PNG but has no readable header is unreadable, not unsupported.
        assert_eq!(
            reencode_local_cover(dir.path(), "reg-1", b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR",),
            Err(CoverImageError::Unreadable)
        );
        assert_eq!(files(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn a_source_over_twenty_megabytes_is_rejected_before_decoding() {
        let dir = TempDir::new().unwrap();
        let too_large = vec![0u8; MAX_COVER_SOURCE_BYTES as usize + 1];
        assert_eq!(
            reencode_local_cover(dir.path(), "reg-1", &too_large),
            Err(CoverImageError::TooLarge)
        );
        assert_eq!(files(dir.path()), Vec::<String>::new());
        assert_eq!(CoverImageError::TooLarge.code(), "coverTooLarge");
    }

    #[test]
    fn registration_ids_with_path_characters_cannot_escape_the_cache() {
        let dir = TempDir::new().unwrap();
        let name = reencode_local_cover(dir.path(), "../evil:reg", &png(10, 10)).unwrap();
        assert!(!name.contains(['/', '\\', ':']));
        assert!(name.starts_with("local-"));
        assert!(dir.path().join(&name).is_file());
        assert!(!dir.path().parent().unwrap().join("evil:reg-cover").exists());
    }
}
