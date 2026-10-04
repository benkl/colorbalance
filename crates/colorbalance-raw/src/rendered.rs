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
pub fn decode_rendered_image(path: &Path) -> Result<DecodedImage, DecodeError> {
    let dyn_img = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()
        .map_err(|e| DecodeError::CorruptFile(format!("failed to decode rendered image: {e}")))?;

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
}
