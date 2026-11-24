//! Cover images a provider's plugin sets for its own library entries.
//!
//! A plugin hands over encoded bytes it got from anywhere, so nothing about them is trusted.
//! [`prepare`] is the only door: it bounds the encoded size, identifies the format from the
//! bytes themselves (the declared media type must agree), rejects animations, reads the
//! dimensions from the header and refuses oversized pixel counts *before* decoding, then decodes
//! under an allocation limit, applies the EXIF orientation, shrinks the picture to the cover size
//! and encodes it again. Metadata, trailing data and anything the decoder skipped never reach the
//! disk. The work is CPU-bound: callers run it on a blocking worker, never on the UI thread or an
//! async executor.
//!
//! A prepared cover is stored under a name that only its entry owns, derived from the entry and
//! the content, so storing the same picture twice is the same file and replacing it never touches
//! another entry's cover.

use std::fs::{self, File};
use std::io::{self, Cursor, Read};
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};

use crate::covers::write_named;

/// The largest encoded image accepted. The host's messages are capped at 8 MiB; this leaves room
/// for the request around the bytes.
pub const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
/// The most pixels a source image may have (4096 x 4096) before it is decoded.
pub const MAX_SOURCE_PIXELS: u64 = 16 * 1024 * 1024;
/// The longest side a source image may have, whatever its pixel count.
pub const MAX_SOURCE_SIDE: u32 = 8192;
/// What decoding may allocate: a 16 megapixel RGBA16 image is 128 MiB.
const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;
/// A cover is shrunk to fit in this box (2:3), keeping its proportions and never enlarged.
pub const COVER_MAX_WIDTH: u32 = 600;
pub const COVER_MAX_HEIGHT: u32 = 900;
const JPEG_QUALITY: u8 = 88;

/// Why an image was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArtworkError {
    #[error("the image is larger than {MAX_INPUT_BYTES} bytes")]
    TooLarge,
    #[error("the image is not a PNG, JPEG or WebP picture")]
    UnsupportedFormat,
    #[error("the declared media type does not match the image")]
    MediaTypeMismatch,
    #[error("animated images are not supported")]
    Animated,
    #[error("the image has more than {MAX_SOURCE_PIXELS} pixels or a side over {MAX_SOURCE_SIDE}")]
    TooManyPixels,
    #[error("the image is damaged")]
    Malformed,
}

/// A decoded, shrunk and re-encoded cover, ready to store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub bytes: Vec<u8>,
    /// `image/png` when the picture has transparency, else `image/jpeg`.
    pub media_type: &'static str,
    pub width: u32,
    pub height: u32,
}

/// The format of `bytes`, judged by their signature alone.
fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::WebP)
    } else {
        None
    }
}

fn declared(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        _ => "image/webp",
    }
}

/// Whether a PNG carries an animation control chunk (APNG) before its image data.
fn png_is_animated(bytes: &[u8]) -> bool {
    let mut at = 8;
    while let (Some(length), Some(kind)) = (
        bytes
            .get(at..at + 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok()),
        bytes.get(at + 4..at + 8),
    ) {
        match kind {
            b"acTL" => return true,
            b"IDAT" | b"IEND" => return false,
            _ => {}
        }
        // length + type + data + crc
        let Some(next) = (u32::from_be_bytes(length) as usize)
            .checked_add(12)
            .and_then(|step| at.checked_add(step))
        else {
            return false;
        };
        at = next;
    }
    false
}

/// Whether a WebP file is animated: an extended header with the animation flag, or frame data.
fn webp_is_animated(bytes: &[u8]) -> bool {
    let mut at = 12;
    while let (Some(kind), Some(length)) = (
        bytes.get(at..at + 4),
        bytes
            .get(at + 4..at + 8)
            .and_then(|b| <[u8; 4]>::try_from(b).ok()),
    ) {
        match kind {
            b"ANIM" | b"ANMF" => return true,
            b"VP8X" => {
                if bytes.get(at + 8).is_some_and(|flags| flags & 0x02 != 0) {
                    return true;
                }
            }
            b"VP8 " | b"VP8L" => return false,
            _ => {}
        }
        let padded = (u32::from_le_bytes(length) as usize).saturating_add(1) & !1;
        let Some(next) = at.checked_add(8).and_then(|head| head.checked_add(padded)) else {
            return false;
        };
        at = next;
    }
    false
}

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    limits
}

/// Validates, decodes, shrinks and re-encodes `bytes` as a cover. `media_type` is what the
/// caller declared; it must name the format the bytes really are. Blocking and CPU-heavy.
pub fn prepare(bytes: &[u8], media_type: &str) -> Result<Prepared, ArtworkError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(ArtworkError::TooLarge);
    }
    let format = sniff(bytes).ok_or(ArtworkError::UnsupportedFormat)?;
    if !media_type.trim().eq_ignore_ascii_case(declared(format)) {
        return Err(ArtworkError::MediaTypeMismatch);
    }
    let animated = match format {
        ImageFormat::Png => png_is_animated(bytes),
        ImageFormat::WebP => webp_is_animated(bytes),
        _ => false,
    };
    if animated {
        return Err(ArtworkError::Animated);
    }

    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .map_err(|_| ArtworkError::Malformed)?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || width > MAX_SOURCE_SIDE
        || height > MAX_SOURCE_SIDE
        || u64::from(width) * u64::from(height) > MAX_SOURCE_PIXELS
    {
        return Err(ArtworkError::TooManyPixels);
    }
    decoder
        .set_limits(limits())
        .map_err(|_| ArtworkError::TooManyPixels)?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = DynamicImage::from_decoder(decoder).map_err(|_| ArtworkError::Malformed)?;
    picture.apply_orientation(orientation);

    if picture.width() > COVER_MAX_WIDTH || picture.height() > COVER_MAX_HEIGHT {
        picture = picture.resize(COVER_MAX_WIDTH, COVER_MAX_HEIGHT, FilterType::Lanczos3);
    }
    let (width, height) = (picture.width(), picture.height());

    let transparent = picture.color().has_alpha()
        && picture
            .to_rgba8()
            .pixels()
            .any(|pixel| pixel.0[3] < u8::MAX);
    let mut out = Vec::new();
    let media_type = if transparent {
        picture
            .write_with_encoder(PngEncoder::new(&mut out))
            .map_err(|_| ArtworkError::Malformed)?;
        "image/png"
    } else {
        DynamicImage::ImageRgb8(picture.to_rgb8())
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY))
            .map_err(|_| ArtworkError::Malformed)?;
        "image/jpeg"
    };
    Ok(Prepared {
        bytes: out,
        media_type,
        width,
        height,
    })
}

fn hex_prefix(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().take(8).fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// The file name a prepared cover is stored under: `plugin-<entry>-<content>.<ext>`. Only this
/// entry ever uses it, and the same picture always has the same name.
pub fn file_name(entry_key: &str, prepared: &Prepared) -> String {
    let entry = hex_prefix(&Sha256::digest(entry_key.as_bytes()));
    let content = hex_prefix(&Sha256::digest(&prepared.bytes));
    let extension = if prepared.media_type == "image/png" {
        "png"
    } else {
        "jpg"
    };
    format!("plugin-{entry}-{content}.{extension}")
}

/// The media type of a stored cover, from its file name.
pub fn media_type_of(file: &str) -> &'static str {
    match file.rsplit('.').next() {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    }
}

/// Where a stored cover goes: written atomically into `dir`. Returns whether the file was
/// already there (so a refused write need not delete it).
pub fn store(dir: &Path, name: &str, prepared: &Prepared) -> io::Result<bool> {
    let existed = dir.join(name).is_file();
    write_named(dir, name, &prepared.bytes)?;
    Ok(existed)
}

/// Deletes a stored cover. Best effort: a leftover file is harmless and never read again.
pub fn remove(dir: &Path, name: &str) {
    if is_plain_name(name) {
        let _ = fs::remove_file(dir.join(name));
    }
}

/// A file name with no directory part.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// The ways reading a stored cover back can fail.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The file is missing, unreadable or not a plain name.
    Unavailable,
    /// The file is larger than the caller allows.
    TooLarge,
}

/// Reads `dir/name`, refusing more than `max_bytes`. Blocking.
pub fn read(dir: &Path, name: &str, max_bytes: u64) -> Result<Vec<u8>, ReadError> {
    if !is_plain_name(name) {
        return Err(ReadError::Unavailable);
    }
    let file = File::open(dir.join(name)).map_err(|_| ReadError::Unavailable)?;
    let metadata = file.metadata().map_err(|_| ReadError::Unavailable)?;
    if !metadata.is_file() {
        return Err(ReadError::Unavailable);
    }
    if metadata.len() > max_bytes {
        return Err(ReadError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ReadError::Unavailable)?;
    if bytes.len() as u64 > max_bytes {
        return Err(ReadError::TooLarge);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
