use super::*;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage, Rgba, RgbaImage};
use tempfile::TempDir;

fn png_of(image: &DynamicImage) -> Vec<u8> {
    let mut out = Vec::new();
    image.write_with_encoder(PngEncoder::new(&mut out)).unwrap();
    out
}

fn jpeg_of(image: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    DynamicImage::ImageRgb8(image.clone())
        .write_with_encoder(JpegEncoder::new(&mut out))
        .unwrap();
    out
}

fn webp_of(image: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    WebPEncoder::new_lossless(&mut out)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .unwrap();
    out
}

fn opaque(width: u32, height: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([200, 30, 30])))
}

fn decoded(prepared: &Prepared) -> DynamicImage {
    image::load_from_memory(&prepared.bytes).unwrap()
}

#[test]
fn a_large_picture_is_shrunk_to_fit_the_cover_box_keeping_its_proportions() {
    // 1200 x 1200 is limited by the width: 600 x 600.
    let square = prepare(&png_of(&opaque(1200, 1200)), "image/png").unwrap();
    assert_eq!((square.width, square.height), (600, 600));
    // 1800 x 3600 is limited by the height: 450 x 900.
    let tall = prepare(&png_of(&opaque(1800, 3600)), "image/png").unwrap();
    assert_eq!((tall.width, tall.height), (450, 900));
    let out = decoded(&tall);
    assert_eq!((out.width(), out.height()), (450, 900));
}

#[test]
fn a_small_picture_is_not_enlarged() {
    let small = prepare(&png_of(&opaque(120, 180)), "image/png").unwrap();
    assert_eq!((small.width, small.height), (120, 180));
}

#[test]
fn an_opaque_picture_is_stored_as_jpeg_and_a_transparent_one_as_png() {
    let flat = prepare(&png_of(&opaque(64, 64)), "image/png").unwrap();
    assert_eq!(flat.media_type, "image/jpeg");
    assert!(flat.bytes.starts_with(&[0xFF, 0xD8, 0xFF]));

    let mut glass = RgbaImage::from_pixel(64, 64, Rgba([10, 20, 30, 255]));
    glass.put_pixel(3, 3, Rgba([10, 20, 30, 0]));
    let see_through = prepare(&png_of(&DynamicImage::ImageRgba8(glass)), "image/png").unwrap();
    assert_eq!(see_through.media_type, "image/png");
    assert_eq!(decoded(&see_through).to_rgba8().get_pixel(3, 3).0[3], 0);

    // An alpha channel that is fully opaque costs nothing.
    let solid = RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255]));
    let kept = prepare(&png_of(&DynamicImage::ImageRgba8(solid)), "image/png").unwrap();
    assert_eq!(kept.media_type, "image/jpeg");
}

#[test]
fn jpeg_and_webp_pictures_are_accepted_and_the_media_type_is_case_insensitive() {
    let jpeg = prepare(&jpeg_of(&RgbImage::new(40, 60)), " IMAGE/JPEG ").unwrap();
    assert_eq!((jpeg.width, jpeg.height), (40, 60));
    let webp = prepare(
        &webp_of(&RgbaImage::from_pixel(40, 60, Rgba([9, 9, 9, 255]))),
        "image/webp",
    )
    .unwrap();
    assert_eq!((webp.width, webp.height), (40, 60));
}

#[test]
fn what_the_bytes_are_decides_not_what_the_caller_says() {
    let png = png_of(&opaque(8, 8));
    assert_eq!(
        prepare(&png, "image/jpeg"),
        Err(ArtworkError::MediaTypeMismatch)
    );
    assert_eq!(prepare(&png, ""), Err(ArtworkError::MediaTypeMismatch));
    assert_eq!(
        prepare(&png, "image/svg+xml"),
        Err(ArtworkError::MediaTypeMismatch)
    );
}

#[test]
fn things_that_are_not_png_jpeg_or_webp_are_refused() {
    for bytes in [
        b"<svg xmlns='http://www.w3.org/2000/svg'/>".as_slice(),
        b"GIF89a\x01\x00\x01\x00".as_slice(),
        b"BM\x00\x00\x00\x00".as_slice(),
        b"#!/bin/sh\necho hi\n".as_slice(),
        b"".as_slice(),
    ] {
        assert_eq!(
            prepare(bytes, "image/png"),
            Err(ArtworkError::UnsupportedFormat),
            "{bytes:?}"
        );
    }
}

#[test]
fn more_bytes_than_the_cap_are_refused_before_anything_else() {
    let mut big = b"\x89PNG\r\n\x1a\n".to_vec();
    big.resize(MAX_INPUT_BYTES + 1, 0);
    assert_eq!(prepare(&big, "image/png"), Err(ArtworkError::TooLarge));
    // Exactly the cap is not "too large" (it is merely damaged here).
    big.truncate(MAX_INPUT_BYTES);
    assert_eq!(prepare(&big, "image/png"), Err(ArtworkError::Malformed));
}

#[test]
fn a_picture_with_too_many_pixels_is_refused_without_decoding_it() {
    // 4097 x 4097 gray pixels compress to a few KiB but would need 16 MiB of pixels.
    let heavy = DynamicImage::ImageLuma8(image::GrayImage::new(4097, 4097));
    let bytes = png_of(&heavy);
    assert!(bytes.len() < MAX_INPUT_BYTES);
    assert_eq!(
        prepare(&bytes, "image/png"),
        Err(ArtworkError::TooManyPixels)
    );

    // A very long thin strip is under the pixel count but over the side limit.
    let strip = png_of(&DynamicImage::ImageLuma8(image::GrayImage::new(
        MAX_SOURCE_SIDE + 1,
        1,
    )));
    assert_eq!(
        prepare(&strip, "image/png"),
        Err(ArtworkError::TooManyPixels)
    );

    // The limit itself is fine.
    let edge = png_of(&DynamicImage::ImageLuma8(image::GrayImage::new(4096, 4096)));
    assert!(prepare(&edge, "image/png").is_ok());
}

#[test]
fn damaged_pictures_are_refused() {
    let png = png_of(&opaque(64, 64));
    assert_eq!(
        prepare(&png[..png.len() / 2], "image/png"),
        Err(ArtworkError::Malformed)
    );
    let jpeg = jpeg_of(&RgbImage::new(64, 64));
    assert_eq!(
        prepare(&jpeg[..jpeg.len() / 2], "image/jpeg"),
        Err(ArtworkError::Malformed)
    );
    assert_eq!(
        prepare(b"\xFF\xD8\xFFnot really a jpeg", "image/jpeg"),
        Err(ArtworkError::Malformed)
    );
    assert_eq!(
        prepare(b"RIFF\x04\x00\x00\x00WEBP", "image/webp"),
        Err(ArtworkError::Malformed)
    );
}

fn with_chunk_after_header(png: &[u8], kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    // signature (8) + IHDR chunk (4 + 4 + 13 + 4)
    let split = 8 + 25;
    let mut out = png[..split].to_vec();
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(kind);
    out.extend(data);
    out.extend([0, 0, 0, 0]); // the checksum is never reached: the image is refused first
    out.extend(&png[split..]);
    out
}

#[test]
fn animated_pictures_are_refused() {
    let apng = with_chunk_after_header(&png_of(&opaque(8, 8)), b"acTL", &[0, 0, 0, 2, 0, 0, 0, 0]);
    assert_eq!(prepare(&apng, "image/png"), Err(ArtworkError::Animated));

    // A still image with an unrelated ancillary chunk stays fine.
    let tagged = with_chunk_after_header(&png_of(&opaque(8, 8)), b"tEXt", b"k\0v");
    assert!(!png_is_animated(&tagged));

    let mut webp = b"RIFF\x1a\x00\x00\x00WEBPVP8X\x0a\x00\x00\x00".to_vec();
    webp.extend([0x02, 0, 0, 0, 7, 0, 0, 7, 0, 0]);
    assert_eq!(prepare(&webp, "image/webp"), Err(ArtworkError::Animated));
    let mut frames = b"RIFF\x1a\x00\x00\x00WEBPANIM\x06\x00\x00\x00".to_vec();
    frames.extend([0; 6]);
    assert_eq!(prepare(&frames, "image/webp"), Err(ArtworkError::Animated));
}

#[test]
fn hostile_chunk_lengths_do_not_loop_or_overflow() {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(u32::MAX.to_be_bytes());
    png.extend(b"zzzz");
    assert!(!png_is_animated(&png));
    let mut webp = b"RIFF\x00\x00\x00\x00WEBPzzzz".to_vec();
    webp.extend(u32::MAX.to_le_bytes());
    assert!(!webp_is_animated(&webp));
}

#[test]
fn trailing_data_and_metadata_never_survive() {
    let mut png = png_of(&opaque(32, 32));
    png.extend(b"-----SECRET TRAILER-----");
    let prepared = prepare(&png, "image/png").unwrap();
    assert!(!prepared.bytes.windows(6).any(|window| window == b"SECRET"));
}

#[test]
fn a_cover_has_a_name_only_its_entry_and_content_decide() {
    let a = prepare(&png_of(&opaque(32, 32)), "image/png").unwrap();
    let b = prepare(&png_of(&opaque(48, 32)), "image/png").unwrap();
    let name = file_name("steam:1", &a);
    assert_eq!(
        name,
        file_name("steam:1", &a),
        "the same picture, the same file"
    );
    assert_ne!(
        name,
        file_name("steam:1", &b),
        "another picture, another file"
    );
    assert_ne!(
        name,
        file_name("steam:2", &a),
        "another entry never shares a file"
    );
    assert!(
        name.starts_with("plugin-") && name.ends_with(".jpg"),
        "{name}"
    );
    assert!(is_plain_name(&name));
    assert_eq!(media_type_of(&name), "image/jpeg");
    assert_eq!(media_type_of("plugin-x-y.png"), "image/png");
}

#[test]
fn a_stored_cover_reads_back_within_the_bound() {
    let dir = TempDir::new().unwrap();
    let covers = dir.path().join("covers");
    let prepared = prepare(&png_of(&opaque(32, 32)), "image/png").unwrap();
    let name = file_name("steam:1", &prepared);

    assert!(!store(&covers, &name, &prepared).unwrap(), "first write");
    assert!(store(&covers, &name, &prepared).unwrap(), "already there");
    let bytes = read(&covers, &name, 1 << 20).unwrap();
    assert_eq!(bytes, prepared.bytes);
    assert_eq!(
        read(&covers, &name, bytes.len() as u64 - 1),
        Err(ReadError::TooLarge)
    );
    assert_eq!(read(&covers, &name, bytes.len() as u64), Ok(bytes));

    remove(&covers, &name);
    assert_eq!(read(&covers, &name, 1 << 20), Err(ReadError::Unavailable));
}

#[test]
fn only_plain_names_are_read_or_removed() {
    let dir = TempDir::new().unwrap();
    let covers = dir.path().join("covers");
    fs::create_dir_all(&covers).unwrap();
    let outside = dir.path().join("secret.txt");
    fs::write(&outside, b"secret").unwrap();
    for name in [
        "../secret.txt",
        "/etc/passwd",
        "a/b.jpg",
        ".hidden",
        "",
        "..",
        "x\\y.jpg",
    ] {
        assert_eq!(
            read(&covers, name, 1 << 20),
            Err(ReadError::Unavailable),
            "{name}"
        );
        remove(&covers, name);
    }
    assert!(outside.exists());
    // A folder is not a cover.
    fs::create_dir(covers.join("folder.jpg")).unwrap();
    assert_eq!(
        read(&covers, "folder.jpg", 1 << 20),
        Err(ReadError::Unavailable)
    );
}

/// `jpeg` with an EXIF block that says "rotate 90 degrees clockwise to display" (orientation 6).
fn rotated_jpeg(jpeg: &[u8]) -> Vec<u8> {
    let mut exif = vec![0xFF, 0xE1, 0x00, 0x22];
    exif.extend(b"Exif\0\0");
    exif.extend([b'I', b'I', 0x2A, 0, 8, 0, 0, 0]);
    exif.extend([1, 0]);
    exif.extend([0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
    exif.extend([0, 0, 0, 0]);
    let mut out = jpeg[..2].to_vec();
    out.extend(exif);
    out.extend(&jpeg[2..]);
    out
}

#[test]
fn the_orientation_a_camera_recorded_is_applied_before_the_metadata_is_dropped() {
    let wide = rotated_jpeg(&jpeg_of(&RgbImage::new(80, 40)));
    let prepared = prepare(&wide, "image/jpeg").unwrap();
    assert_eq!((prepared.width, prepared.height), (40, 80));
    assert!(
        !prepared.bytes.windows(4).any(|w| w == b"Exif"),
        "the stored cover carries no EXIF"
    );
}
