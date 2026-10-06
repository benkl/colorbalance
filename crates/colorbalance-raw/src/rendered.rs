//! Rendered image decoder (JPEG, PNG) for approximation on non-RAW sources.
//!
//! When processing camera-rendered JPEG or PNG files in quick-and-dirty mode,
//! pixels are already non-linearly encoded (sRGB gamma and unknown tone curves)
//! and compressed. This decoder inverts the standard sRGB transfer function to
//! produce approximate linear sRGB camera values for rough calibration and batch
//! correction.

use std::path::Path;

use colorbalance_core::color::srgb_decode;
use colorbalance_core::decode::{CameraIdentity, DecodeError, DecodedImage, RawDecoder};
use image::{DynamicImage, ImageDecoder};

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
/// The image is sRGB-encoded and downscaled so its longest side is at most
/// `max_dim` (never upscaled). PNG is used because every webview decodes it;
/// the preview keeps the decoded image's aspect ratio and orientation, so
/// chart coordinates stay in full-resolution pixel space.
pub fn render_preview_png(image: &DecodedImage, max_dim: u32) -> Result<Vec<u8>, DecodeError> {
    let mut bytes = Vec::with_capacity(image.rgb.len());
    for &value in &image.rgb {
        let encoded = colorbalance_core::color::srgb_encode(f64::from(value).clamp(0.0, 1.0));
        bytes.push((encoded * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    let full = image::RgbImage::from_raw(image.width, image.height, bytes).ok_or_else(|| {
        DecodeError::CorruptFile("pixel buffer does not match dimensions".to_owned())
    })?;
    let longest = image.width.max(image.height);
    let preview = if longest > max_dim {
        let scale = f64::from(max_dim) / f64::from(longest);
        let width = ((f64::from(image.width) * scale).round() as u32).max(1);
        let height = ((f64::from(image.height) * scale).round() as u32).max(1);
        image::imageops::resize(&full, width, height, image::imageops::FilterType::Triangle)
    } else {
        full
    };
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
        width,
        height,
        rgb,
        clipped,
        black_levels: [0; 4],
        white_levels: [255; 4],
        cfa_pattern: *b"RGBG",
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
}
