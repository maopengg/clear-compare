//! Top-left-aligned RGBA8 pixel comparison with bounded display previews.
use crate::{
    Error, Result,
    compare::{Snapshot, check_cancel, checked_path},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use clear_contracts::{Kind, PictureOptions, PicturePreview, PictureSummary, PixelBounds};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, Rgba, RgbaImage};
use std::{
    fs::{self, File},
    io::{Cursor, Read},
    path::Path,
    sync::atomic::AtomicBool,
};
const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PIXELS: u64 = 8_000_000;
const MAX_SIDE: u32 = 16384;
fn invalid(e: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("Picture decoding: {e}"))
}
pub fn read(path: &Path) -> Result<RgbaImage> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(Error::Invalid("Select a regular picture file".into()));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::Invalid(
            "Picture inputs are limited to 16 MiB".into(),
        ));
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(invalid)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS || width == 0 || height == 0 {
        return Err(Error::Invalid(
            "Pictures are limited to 8 million pixels".into(),
        ));
    }
    let orientation = decoder.orientation().map_err(invalid)?;
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(invalid)?;
    decoded.apply_orientation(orientation);
    Ok(decoded.to_rgba8())
}
fn png(image: RgbaImage) -> Result<String> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(invalid)?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(bytes.into_inner())
    ))
}
fn sample(image: Option<&RgbaImage>, x: u32, y: u32) -> Option<Rgba<u8>> {
    image
        .filter(|p| x < p.width() && y < p.height())
        .map(|p| *p.get_pixel(x, y))
}
fn side_preview(image: &RgbaImage, w: u32, h: u32, pw: u32, ph: u32) -> RgbaImage {
    RgbaImage::from_fn(pw, ph, |x, y| {
        sample(
            Some(image),
            (u64::from(x) * u64::from(w) / u64::from(pw)) as u32,
            (u64::from(y) * u64::from(h) / u64::from(ph)) as u32,
        )
        .unwrap_or(Rgba([0, 0, 0, 0]))
    })
}
pub fn compare_images(
    path: &str,
    left: Option<&RgbaImage>,
    right: Option<&RgbaImage>,
    options: &PictureOptions,
    cancel: &AtomicBool,
) -> Result<PicturePreview> {
    check_cancel(cancel)?;
    let w = left
        .map_or(0, RgbaImage::width)
        .max(right.map_or(0, RgbaImage::width));
    let h = left
        .map_or(0, RgbaImage::height)
        .max(right.map_or(0, RgbaImage::height));
    let total = u64::from(w) * u64::from(h);
    if total == 0 || total > MAX_PIXELS || w > MAX_SIDE || h > MAX_SIDE {
        return Err(Error::Invalid(
            "Aligned comparison canvas is limited to 8 million pixels and 16384 per side".into(),
        ));
    }
    let scale = 1024f64 / f64::from(w.max(h));
    let (pw, ph) = if scale < 1. {
        (
            (f64::from(w) * scale).ceil() as u32,
            (f64::from(h) * scale).ceil() as u32,
        )
    } else {
        (w, h)
    };
    let mut heat = RgbaImage::from_fn(pw, ph, |_, _| Rgba([230, 233, 237, 255]));
    let mut changed = 0u64;
    let mut max_delta = 0u8;
    let (mut minx, mut miny, mut maxx, mut maxy) = (w, h, 0, 0);
    for y in 0..h {
        check_cancel(cancel)?;
        for x in 0..w {
            let (a, b) = (sample(left, x, y), sample(right, x, y));
            let (different, delta) = match (a, b) {
                (None, None) => (false, 0),
                (Some(a), Some(b)) => {
                    let n = if options.ignore_alpha { 3 } else { 4 };
                    let delta = a.0[..n]
                        .iter()
                        .zip(&b.0[..n])
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap_or_default();
                    (delta > options.tolerance, delta)
                }
                _ => (true, 255),
            };
            max_delta = max_delta.max(delta);
            if different {
                changed += 1;
                minx = minx.min(x);
                miny = miny.min(y);
                maxx = maxx.max(x);
                maxy = maxy.max(y);
                let px = (u64::from(x) * u64::from(pw) / u64::from(w)) as u32;
                let py = (u64::from(y) * u64::from(ph) / u64::from(h)) as u32;
                heat.put_pixel(px, py, Rgba([213, 46, 57, 255]));
            }
        }
    }
    let bounds = if changed == 0 {
        None
    } else {
        Some(PixelBounds {
            x: minx,
            y: miny,
            width: maxx - minx + 1,
            height: maxy - miny + 1,
        })
    };
    let summary = PictureSummary {
        left_dimensions: left.map(RgbaImage::dimensions),
        right_dimensions: right.map(RgbaImage::dimensions),
        canvas_width: w,
        canvas_height: h,
        total_pixels: total.to_string(),
        changed_pixels: changed.to_string(),
        max_channel_delta: max_delta,
        bounds,
    };
    check_cancel(cancel)?;
    let left_png = left
        .map(|p| png(side_preview(p, w, h, pw, ph)))
        .transpose()?;
    let right_png = right
        .map(|p| png(side_preview(p, w, h, pw, ph)))
        .transpose()?;
    let difference_png = png(heat)?;
    Ok(PicturePreview {
        path: path.into(),
        summary,
        preview_width: pw,
        preview_height: ph,
        left_png,
        right_png,
        difference_png,
    })
}
pub fn compare_paths(
    left: Option<&Path>,
    right: Option<&Path>,
    options: &PictureOptions,
    cancel: &AtomicBool,
) -> Result<PicturePreview> {
    check_cancel(cancel)?;
    let left = left.map(read).transpose()?;
    check_cancel(cancel)?;
    let right = right.map(read).transpose()?;
    compare_images(
        "Picture comparison",
        left.as_ref(),
        right.as_ref(),
        options,
        cancel,
    )
}
pub fn preview(
    snapshot: &Snapshot,
    row_id: &str,
    options: &PictureOptions,
    cancel: &AtomicBool,
) -> Result<PicturePreview> {
    let relative = snapshot
        .paths
        .get(row_id)
        .ok_or_else(|| Error::Invalid("Unknown picture entry".into()))?;
    let row = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.id == row_id)
        .ok_or_else(|| Error::Invalid("Unknown picture entry".into()))?;
    let path = |root: &Path,
                entry: Option<&clear_contracts::Entry>|
     -> Result<Option<std::path::PathBuf>> {
        match entry {
            None => Ok(None),
            Some(e) if e.kind == Kind::File && e.error.is_none() => {
                Ok(Some(checked_path(root, relative, false)?))
            }
            _ => Err(Error::Invalid("Select a regular picture file".into())),
        }
    };
    let mut result = compare_paths(
        path(&snapshot.left, row.left.as_ref())?.as_deref(),
        path(&snapshot.right, row.right.as_ref())?.as_deref(),
        options,
        cancel,
    )?;
    result.path = row.path.clone();
    Ok(result)
}
