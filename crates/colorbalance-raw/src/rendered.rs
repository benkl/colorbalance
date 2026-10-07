//! Rendered image decoder (JPEG, PNG) for approximation on non-RAW sources.
//!
//! When processing camera-rendered JPEG or PNG files in quick-and-dirty mode,
//! pixels are already non-linearly encoded (sRGB gamma and unknown tone curves)
//! and compressed. This decoder inverts the standard sRGB transfer function to
//! produce approximate linear sRGB camera values for rough calibration and batch
//! correction.

use std::path::Path;

use colorbalance_core::color::srgb_decode;
use colorbalance_core::decode::{
    CameraIdentity, DecodeError, DecodedImage, RawDecoder, SensorLayout,
};
use fast_image_resize::images::{TypedImage, TypedImageRef};
use fast_image_resize::pixels::F32x3;
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, ImageDecoder};
use rayon::prelude::*;

/// Output rows resized per parallel work item in [`render_preview_rgb`].
const BAND_OUTPUT_ROWS: usize = 16;

/// Decoder identity for rendered image sources.
pub const JPEG_DECODER_NAME: &str = "colorbalance-rendered-jpeg";
/// Version of the rendered image decoder.
pub const JPEG_DECODER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Rendered image decoder supporting JPEG and PNG files.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderedImageDecoder;

impl RawDecoder for RenderedImageDecoder {
    fn decode_path(&self, path: &Path) -> Result<DecodedImage, DecodeError> {
        decode_rendered_image(path)
    }
}

/// Decode a rendered image upright: the EXIF orientation recorded by the camera
/// is applied so pixels match what any viewer (including a browser `<img>`)
/// displays. Chart coordinates therefore refer to the visible orientation.
pub fn decode_upright(path: &Path) -> Result<DynamicImage, DecodeError> {
    let corrupt = |e: image::ImageError| {
        DecodeError::CorruptFile(format!("failed to decode rendered image: {e}"))
    };
    let mut decoder = image::ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()
        .map_err(corrupt)?;
    let orientation = decoder.orientation().map_err(corrupt)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(corrupt)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// Render a display preview of a decoded image as PNG bytes.
///
/// The image is downscaled so its longest side is at most `max_dim` (never
/// upscaled), then sRGB-encoded. PNG is used because every webview decodes it;
/// the preview keeps the decoded image's aspect ratio and orientation, so
/// chart coordinates stay in full-resolution pixel space.
///
/// Downscaling is a box average in linear light, done before the transfer
/// function: every source pixel is read once and only the small output pays
/// for the sRGB encode.
pub fn render_preview_png(image: &DecodedImage, max_dim: u32) -> Result<Vec<u8>, DecodeError> {
    render_preview_rgb(
        &image.rgb,
        image.width,
        image.height,
        image.display_neutral,
        max_dim,
    )
}

/// [`render_preview_png`] for a bare buffer of `width * height * 3` linear
/// samples, so a corrected copy can be previewed without a second
/// `DecodedImage`. `display_neutral` is the optional display-only gain.
pub fn render_preview_rgb(
    rgb: &[f32],
    image_width: u32,
    image_height: u32,
    display_neutral: Option<[f32; 3]>,
    max_dim: u32,
) -> Result<Vec<u8>, DecodeError> {
    let (width, height) = (image_width as usize, image_height as usize);
    if width == 0 || height == 0 || rgb.len() != width * height * 3 {
        return Err(DecodeError::CorruptFile(
            "pixel buffer does not match dimensions".to_owned(),
        ));
    }
    // Camera RGB has no white balance under the decode contract, so a raw
    // preview is strongly tinted. Divide by the camera's recorded neutral for
    // display only; the buffer and every measurement stay untouched.
    let gain = display_neutral
        .filter(|n| n.iter().all(|v| v.is_finite() && *v > 0.0))
        .map_or([1.0_f32; 3], |n| n.map(|v| 1.0 / v));

    let longest = image_width.max(image_height);
    let (out_w, out_h) = if longest > max_dim {
        let scale = f64::from(max_dim) / f64::from(longest);
        (
            ((f64::from(image_width) * scale).round() as usize).max(1),
            ((f64::from(image_height) * scale).round() as usize).max(1),
        )
    } else {
        (width, height)
    };

    // Area-average the linear, gain-corrected, clamped image down to the
    // output size (D15). Each band of output rows copies only the source rows
    // it covers into a small f32 buffer, so no full-frame copy is made, and
    // bands run in parallel. The resize itself is exact for the band because
    // its crop box is the band's fractional source extent.
    let mut resized = vec![F32x3::new([0.0; 3]); out_w * out_h];
    let band_rows = BAND_OUTPUT_ROWS.min(out_h);
    let failure = std::sync::Mutex::new(None::<String>);
    resized
        .par_chunks_mut(band_rows * out_w)
        .enumerate()
        .for_each(|(band, destination)| {
            let first = band * band_rows;
            let rows = destination.len() / out_w;
            let last = first + rows;
            // Source rows [start, end) cover output rows [first, last).
            let start = first * height / out_h;
            let end = (last * height).div_ceil(out_h);
            let source: Vec<F32x3> = rgb[start * width * 3..end * width * 3]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|pixel| {
                    F32x3::new([
                        (pixel[0] * gain[0]).clamp(0.0, 1.0),
                        (pixel[1] * gain[1]).clamp(0.0, 1.0),
                        (pixel[2] * gain[2]).clamp(0.0, 1.0),
                    ])
                })
                .collect();
            let result = (|| -> Result<(), String> {
                let source = TypedImageRef::new(width as u32, (end - start) as u32, &source)
                    .map_err(|e| e.to_string())?;
                let mut target =
                    TypedImage::from_pixels_slice(out_w as u32, rows as u32, destination)
                        .map_err(|e| e.to_string())?;
                let top = (first * height) as f64 / out_h as f64 - start as f64;
                let extent = (rows * height) as f64 / out_h as f64;
                let options = ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(FilterType::Box))
                    .crop(0.0, top, width as f64, extent);
                Resizer::new()
                    .resize_typed(&source, &mut target, &options)
                    .map_err(|e| e.to_string())
            })();
            if let Err(message) = result {
                *failure.lock().expect("preview failure lock") = Some(message);
            }
        });
    if let Some(message) = failure.into_inner().expect("preview failure lock") {
        return Err(DecodeError::CorruptFile(format!(
            "preview resize failed: {message}"
        )));
    }
    let mut bytes = vec![0_u8; out_w * out_h * 3];
    bytes
        .par_chunks_mut(out_w * 3)
        .zip(resized.par_chunks(out_w))
        .for_each(|(line, pixels)| {
            for (out, pixel) in line.as_chunks_mut::<3>().0.iter_mut().zip(pixels) {
                for (byte, &value) in out.iter_mut().zip(&pixel.0) {
                    let encoded = colorbalance_core::color::srgb_encode(f64::from(value));
                    *byte = (encoded * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        });
    let preview =
        image::RgbImage::from_raw(out_w as u32, out_h as u32, bytes).ok_or_else(|| {
            DecodeError::CorruptFile("pixel buffer does not match dimensions".to_owned())
        })?;
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(preview)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| DecodeError::CorruptFile(format!("failed to encode preview: {e}")))?;
    Ok(out)
}

/// Decode a JPEG or PNG file into an approximate linear `DecodedImage`.
pub fn decode_rendered_image(path: &Path) -> Result<DecodedImage, DecodeError> {
    let dyn_img = decode_upright(path)?;

    let rgb8 = dyn_img.into_rgb8();
    let width = rgb8.width();
    let height = rgb8.height();
    let pixel_count = (width as usize) * (height as usize);

    let mut rgb = Vec::with_capacity(pixel_count * 3);
    let mut clipped = vec![0u8; pixel_count];

    for (i, pixel) in rgb8.pixels().enumerate() {
        let r_u8 = pixel[0];
        let g_u8 = pixel[1];
        let b_u8 = pixel[2];

        // Flag highlights clipped at 8-bit limit (255)
        if r_u8 == 255 {
            clipped[i] |= 1;
        }
        if g_u8 == 255 {
            clipped[i] |= 2;
        }
        if b_u8 == 255 {
            clipped[i] |= 4;
        }

        // Invert sRGB non-linear curve to get approximate linear values
        let r_lin = srgb_decode(f64::from(r_u8) / 255.0) as f32;
        let g_lin = srgb_decode(f64::from(g_u8) / 255.0) as f32;
        let b_lin = srgb_decode(f64::from(b_u8) / 255.0) as f32;

        rgb.push(r_lin);
        rgb.push(g_lin);
        rgb.push(b_lin);
    }

    Ok(DecodedImage {
        sensor_layout: SensorLayout::Rendered,
        width,
        height,
        rgb,
        clipped,
        black_levels: [0; 4],
        white_levels: [255, 255, 255, 0],
        cfa_pattern: [0; 4],
        display_neutral: None,
        camera: CameraIdentity {
            make: "Rendered Image (Quick & Dirty)".to_owned(),
            model: path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("jpeg")
                .to_uppercase(),
            decoder: JPEG_DECODER_NAME.to_owned(),
            decoder_version: JPEG_DECODER_VERSION.to_owned(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_simple_synthetic_png() {
        let tmp = std::env::temp_dir().join(format!("test-img-{}.png", std::process::id()));
        let mut img = image::RgbImage::new(4, 4);
        img.put_pixel(0, 0, image::Rgb([255, 128, 0]));
        img.save(&tmp).unwrap();

        let decoded = decode_rendered_image(&tmp).unwrap();
        let _ = std::fs::remove_file(tmp);

        assert_eq!(decoded.width, 4);
        assert_eq!(decoded.height, 4);
        // First pixel R is clipped (255)
        assert_eq!(decoded.clipped[0] & 1, 1);
        // G is unclipped
        assert_eq!(decoded.clipped[0] & 2, 0);
    }

    /// Splice an EXIF APP1 segment carrying `orientation` right after the SOI marker.
    fn with_exif_orientation(jpeg: &[u8], orientation: u8) -> Vec<u8> {
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);
        payload.extend_from_slice(&[0x01, 0x00]);
        payload.extend_from_slice(&[0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00]);
        payload.extend_from_slice(&[orientation, 0x00, 0x00, 0x00]);
        payload.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        let length = (payload.len() + 2) as u16;
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&payload);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    #[test]
    fn exif_orientation_is_applied_so_decode_matches_what_viewers_show() {
        // A landscape 8x4 JPEG tagged "rotate 90 clockwise" displays as portrait 4x8.
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            8,
            4,
            image::Rgb([200, 100, 50]),
        ));
        let mut plain = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut plain),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
        let tagged = with_exif_orientation(&plain, 6);

        let dir = std::env::temp_dir();
        let plain_path = dir.join(format!("cb-orient-plain-{}.jpg", std::process::id()));
        let tagged_path = dir.join(format!("cb-orient-6-{}.jpg", std::process::id()));
        std::fs::write(&plain_path, &plain).unwrap();
        std::fs::write(&tagged_path, &tagged).unwrap();

        let upright_plain = decode_rendered_image(&plain_path).unwrap();
        let upright_tagged = decode_rendered_image(&tagged_path).unwrap();
        let _ = std::fs::remove_file(plain_path);
        let _ = std::fs::remove_file(tagged_path);

        assert_eq!((upright_plain.width, upright_plain.height), (8, 4));
        assert_eq!((upright_tagged.width, upright_tagged.height), (4, 8));
    }

    fn flat_image(rgb: [f32; 3], display_neutral: Option<[f32; 3]>) -> DecodedImage {
        DecodedImage {
            sensor_layout: colorbalance_core::decode::SensorLayout::LinearRaw,
            width: 2,
            height: 1,
            rgb: [rgb, rgb].concat(),
            clipped: vec![0; 2],
            black_levels: [0; 4],
            white_levels: [1; 4],
            cfa_pattern: [0; 4],
            display_neutral,
            camera: colorbalance_core::decode::CameraIdentity {
                make: "m".into(),
                model: "m".into(),
                decoder: "d".into(),
                decoder_version: "1".into(),
            },
        }
    }

    fn first_pixel(png: &[u8]) -> [u8; 3] {
        image::load_from_memory(png)
            .unwrap()
            .to_rgb8()
            .get_pixel(0, 0)
            .0
    }

    #[test]
    fn preview_divides_by_the_as_shot_neutral_so_a_neutral_patch_shows_gray() {
        // The neutral a camera records is exactly the raw RGB of a gray object.
        let neutral = [0.5, 1.0, 0.25];
        let image = flat_image(neutral, Some(neutral));
        let [r, g, b] = first_pixel(&render_preview_png(&image, 16).unwrap());
        assert_eq!((r, b), (g, g), "gray expected, got {r},{g},{b}");
        assert_eq!(image.rgb[0], 0.5, "decoded buffer must stay untouched");
    }

    #[test]
    fn preview_without_a_neutral_shows_the_raw_tint() {
        let [r, g, b] =
            first_pixel(&render_preview_png(&flat_image([0.5, 1.0, 0.25], None), 16).unwrap());
        assert!(
            r < g && b < r,
            "unbalanced RGB must stay tinted, got {r},{g},{b}"
        );
    }

    fn decoded_png(png: &[u8]) -> image::RgbImage {
        image::load_from_memory(png).unwrap().to_rgb8()
    }

    #[test]
    fn preview_is_downscaled_to_the_longest_side_and_never_upscaled() {
        let rgb = vec![0.5_f32; 300 * 100 * 3];
        let big = decoded_png(&render_preview_rgb(&rgb, 300, 100, None, 120).unwrap());
        assert_eq!((big.width(), big.height()), (120, 40));

        let small = decoded_png(&render_preview_rgb(&rgb, 300, 100, None, 1000).unwrap());
        assert_eq!((small.width(), small.height()), (300, 100));
    }

    #[test]
    fn preview_downscale_averages_in_linear_light() {
        // A black/white checkerboard averages to 0.5 linear, which is sRGB 188;
        // averaging the encoded values would give 128.
        let (w, h) = (8usize, 8usize);
        let mut rgb = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = if (x + y) % 2 == 0 { 1.0 } else { 0.0 };
                rgb.extend_from_slice(&[v, v, v]);
            }
        }
        let png = render_preview_rgb(&rgb, 8, 8, None, 2).unwrap();
        let out = decoded_png(&png);
        assert_eq!((out.width(), out.height()), (2, 2));
        for pixel in out.pixels() {
            assert_eq!(pixel.0, [188, 188, 188]);
        }
    }

    #[test]
    fn preview_bands_agree_with_the_closed_form_box_average() {
        // 40x4000 grey ramp down to 1x100: output row i averages source rows
        // 40i..40i+40, whose linear mean is (40i + 19.5) / 4000. 100 rows span
        // several parallel bands, so a seam error shows up as a step.
        let (w, h) = (40usize, 4000usize);
        let mut rgb = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for _ in 0..w {
                rgb.extend_from_slice(&[y as f32 / h as f32; 3]);
            }
        }
        let png = render_preview_rgb(&rgb, w as u32, h as u32, None, 100).unwrap();
        let out = decoded_png(&png);
        assert_eq!((out.width(), out.height()), (1, 100));
        for (i, pixel) in out.pixels().enumerate() {
            let linear = (40.0 * i as f64 + 19.5) / h as f64;
            let expected = (colorbalance_core::color::srgb_encode(linear) * 255.0).round() as i32;
            for channel in pixel.0 {
                assert!(
                    (i32::from(channel) - expected).abs() <= 1,
                    "row {i}: got {channel}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn preview_rejects_a_buffer_that_does_not_match_its_dimensions() {
        assert!(render_preview_rgb(&[0.0; 5], 2, 1, None, 16).is_err());
    }
}
