use clear_contracts::PictureOptions;
use clear_core::picture;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::{fs, io::Cursor, sync::atomic::AtomicBool};
#[test]
fn pixel_tolerance_alpha_and_dimension_differences_have_explicit_semantics() {
    let left = RgbaImage::from_pixel(2, 1, Rgba([100, 100, 100, 255]));
    let mut right = left.clone();
    right.put_pixel(1, 0, Rgba([101, 100, 100, 255]));
    let result = picture::compare_images(
        "test",
        Some(&left),
        Some(&right),
        &PictureOptions::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(result.summary.changed_pixels, "1");
    let bounds = result.summary.bounds.unwrap();
    assert_eq!(
        (bounds.x, bounds.y, bounds.width, bounds.height),
        (1, 0, 1, 1)
    );
    assert_eq!(
        picture::compare_images(
            "test",
            Some(&left),
            Some(&right),
            &PictureOptions {
                tolerance: 1,
                ignore_alpha: false
            },
            &AtomicBool::new(false)
        )
        .unwrap()
        .summary
        .changed_pixels,
        "0"
    );
    right.put_pixel(1, 0, Rgba([100, 100, 100, 0]));
    assert_eq!(
        picture::compare_images(
            "test",
            Some(&left),
            Some(&right),
            &PictureOptions {
                ignore_alpha: true,
                ..Default::default()
            },
            &AtomicBool::new(false)
        )
        .unwrap()
        .summary
        .changed_pixels,
        "0"
    );
    assert_eq!(
        picture::compare_images(
            "test",
            Some(&left),
            None,
            &PictureOptions {
                tolerance: 255,
                ignore_alpha: true
            },
            &AtomicBool::new(false)
        )
        .unwrap()
        .summary
        .changed_pixels,
        "2"
    );
}
#[test]
fn compressed_preview_preserves_a_single_changed_pixel_and_rejects_large_canvas() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let left = RgbaImage::from_pixel(2048, 1, Rgba([0, 0, 0, 255]));
    let mut right = left.clone();
    right.put_pixel(7, 0, Rgba([255, 0, 0, 255]));
    let result = picture::compare_images(
        "test",
        Some(&left),
        Some(&right),
        &PictureOptions::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(result.preview_width, 1024);
    assert_eq!(result.summary.changed_pixels, "1");
    let heat = image::load_from_memory(
        &STANDARD
            .decode(
                result
                    .difference_png
                    .strip_prefix("data:image/png;base64,")
                    .unwrap(),
            )
            .unwrap(),
    )
    .unwrap()
    .to_rgba8();
    assert_eq!(*heat.get_pixel(3, 0), Rgba([213, 46, 57, 255]));
    let horizontal = RgbaImage::new(5000, 1);
    let vertical = RgbaImage::new(1, 5000);
    assert!(
        picture::compare_images(
            "test",
            Some(&horizontal),
            Some(&vertical),
            &PictureOptions::default(),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(
        picture::compare_images(
            "test",
            Some(&left),
            Some(&right),
            &PictureOptions::default(),
            &AtomicBool::new(true)
        )
        .is_err()
    );
}
#[test]
fn decoder_rejects_invalid_inputs_and_applies_jpeg_exif_orientation() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("sample.bin");
    fs::write(&path, "not a picture").unwrap();
    assert!(picture::read(&path).is_err());
    let image =
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 1, Rgba([120, 20, 30, 255]))).to_rgb8();
    let mut jpeg = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image)
        .write_to(&mut jpeg, ImageFormat::Jpeg)
        .unwrap();
    let original = jpeg.into_inner();
    let exif: [u8; 32] = [
        b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0,
        0, 6, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut oriented = original[..2].to_vec();
    oriented.extend_from_slice(&[255, 225, 0, 34]);
    oriented.extend_from_slice(&exif);
    oriented.extend_from_slice(&original[2..]);
    fs::write(&path, oriented).unwrap();
    assert_eq!(picture::read(&path).unwrap().dimensions(), (1, 2));
    let mut png = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::new(20000, 1))
        .write_to(&mut png, ImageFormat::Png)
        .unwrap();
    fs::write(&path, png.into_inner()).unwrap();
    assert!(picture::read(&path).is_err());
    fs::File::create(&path)
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    assert!(
        picture::read(&path)
            .unwrap_err()
            .to_string()
            .contains("16 MiB")
    );
}
