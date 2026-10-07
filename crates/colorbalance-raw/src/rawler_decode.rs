//! RAW file decode through rawler. Normalization and clipping remain in the RAW domain.

use std::path::Path;

use colorbalance_core::ahd::demosaic_ahd;
use colorbalance_core::decode::{
    CameraIdentity, DecodeError, DecodedImage, RawDecoder, SensorLayout,
};
use rawler::decoders::{Orientation, RawDecodeParams};
use rawler::rawimage::{BlackLevel, RawImage, RawImageData, RawPhotometricInterpretation};
use rawler::rawsource::RawSource;

pub const DECODER_NAME: &str = "rawler-ahd";
pub const DECODER_VERSION: &str = "0.8.0";

pub struct RawlerDecoder;

fn unsupported(message: impl Into<String>) -> DecodeError {
    DecodeError::UnsupportedSensorLayout(message.into())
}

fn level(value: f32) -> Result<u16, DecodeError> {
    if !value.is_finite() || value < 0.0 || value > f32::from(u16::MAX) || value.fract() != 0.0 {
        return Err(unsupported("fractional or out-of-range black level"));
    }
    Ok(value as u16)
}

/// Index of the black level for `channel` of a three-component LinearRaw image.
///
/// The grid is indexed `(row * width + column) * cpp + channel`. The decode
/// contract carries one black level per channel, so every repeat cell must agree
/// for that channel; a spatially varying grid fails closed.
fn linear_black_index(black: &BlackLevel, channel: usize) -> Result<usize, DecodeError> {
    let cells = black.width.checked_mul(black.height);
    if black.width == 0
        || black.height == 0
        || !matches!(black.cpp, 1 | 3)
        || cells.and_then(|c| c.checked_mul(black.cpp)) != Some(black.levels.len())
    {
        return Err(unsupported("unsupported LinearRaw black-level grid"));
    }
    let channel = if black.cpp == 1 { 0 } else { channel };
    let first = black.levels[channel].as_f32();
    let uniform = (1..black.width * black.height)
        .all(|cell| black.levels[cell * black.cpp + channel].as_f32() == first);
    if !uniform {
        return Err(unsupported(
            "LinearRaw black level varies across the repeat grid",
        ));
    }
    Ok(channel)
}

fn normalize(raw: u16, black: u16, white: u16) -> f32 {
    ((f64::from(raw) - f64::from(black)) / f64::from(white - black)).max(0.0) as f32
}

fn orient(
    width: u32,
    height: u32,
    orientation: Orientation,
    source_rgb: Vec<f32>,
    source_clipped: Vec<u8>,
) -> Result<(u32, u32, Vec<f32>, Vec<u8>), DecodeError> {
    if orientation == Orientation::Normal {
        return Ok((width, height, source_rgb, source_clipped));
    }
    let rotated = matches!(
        orientation,
        Orientation::Transpose
            | Orientation::Rotate90
            | Orientation::Transverse
            | Orientation::Rotate270
    );
    let (out_width, out_height) = if rotated {
        (height, width)
    } else {
        (width, height)
    };
    let mut rgb = vec![0.0; source_rgb.len()];
    let mut clipped = vec![0; source_clipped.len()];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let (ox, oy) = match orientation {
                Orientation::Normal => (x, y),
                Orientation::HorizontalFlip => (width as usize - 1 - x, y),
                Orientation::Rotate180 => (width as usize - 1 - x, height as usize - 1 - y),
                Orientation::VerticalFlip => (x, height as usize - 1 - y),
                Orientation::Transpose => (y, x),
                Orientation::Rotate90 => (height as usize - 1 - y, x),
                Orientation::Transverse => (height as usize - 1 - y, width as usize - 1 - x),
                Orientation::Rotate270 => (y, width as usize - 1 - x),
                Orientation::Unknown => return Err(unsupported("unknown RAW orientation")),
            };
            let src = y * width as usize + x;
            let dst = oy * out_width as usize + ox;
            rgb[dst * 3..dst * 3 + 3].copy_from_slice(&source_rgb[src * 3..src * 3 + 3]);
            clipped[dst] = source_clipped[src];
        }
    }
    Ok((out_width, out_height, rgb, clipped))
}

fn from_raw(image: RawImage) -> Result<DecodedImage, DecodeError> {
    let width = u32::try_from(image.width).map_err(|_| unsupported("image too wide"))?;
    let height = u32::try_from(image.height).map_err(|_| unsupported("image too tall"))?;
    let pixels = image
        .width
        .checked_mul(image.height)
        .ok_or_else(|| unsupported("image size overflow"))?;
    if pixels == 0 {
        return Err(unsupported("empty RAW image"));
    }
    if image.active_area.is_some_and(|area| {
        area.p.x != 0 || area.p.y != 0 || area.d.w != image.width || area.d.h != image.height
    }) || image.crop_area.is_some_and(|area| {
        area.p.x != 0 || area.p.y != 0 || area.d.w != image.width || area.d.h != image.height
    }) {
        return Err(unsupported(format!(
            "RAW crop or active area is not the full frame (active {:?}, crop {:?}, image {}x{})",
            image.active_area, image.crop_area, image.width, image.height
        )));
    }
    let (sensor_layout, channels, cfa_pattern) = match &image.photometric {
        RawPhotometricInterpretation::Cfa(config) => {
            if image.cpp != 1 || config.cfa.width != 2 || config.cfa.height != 2 {
                return Err(unsupported("only 2x2 Bayer CFA is supported"));
            }
            let pattern = std::array::from_fn(|i| match config.cfa.color_at(i / 2, i % 2) {
                0 => b'R',
                1 => b'G',
                2 => b'B',
                _ => 0,
            });
            if pattern.iter().filter(|&&c| c == b'R').count() != 1
                || pattern.iter().filter(|&&c| c == b'G').count() != 2
                || pattern.iter().filter(|&&c| c == b'B').count() != 1
            {
                return Err(unsupported("only RGB Bayer CFA is supported"));
            }
            (SensorLayout::Cfa, 1, pattern)
        }
        RawPhotometricInterpretation::LinearRaw if image.cpp == 3 => {
            (SensorLayout::LinearRaw, 3, [0; 4])
        }
        _ => {
            return Err(unsupported(
                "only 2x2 Bayer and three-component LinearRaw are supported",
            ))
        }
    };
    let expected = pixels
        .checked_mul(channels)
        .ok_or_else(|| unsupported("RAW sample count overflow"))?;
    let RawImageData::Integer(samples) = &image.data else {
        return Err(unsupported(
            "floating-point RAW samples are not supported by the pinned 16-bit contract",
        ));
    };
    if samples.len() != expected {
        return Err(DecodeError::CorruptFile(
            "RAW sample count differs from image dimensions".into(),
        ));
    }
    let positions = if sensor_layout == SensorLayout::Cfa {
        4
    } else {
        3
    };
    let mut black_levels = [0_u16; 4];
    let mut white_levels = [0_u16; 4];
    let black = &image.blacklevel;
    for i in 0..positions {
        let index = if sensor_layout == SensorLayout::Cfa {
            if black.cpp != 1 || !matches!((black.width, black.height), (1, 1) | (2, 2)) {
                return Err(unsupported("unsupported CFA black-level grid"));
            }
            if black.width == 1 {
                0
            } else {
                i
            }
        } else {
            linear_black_index(black, i)?
        };
        let value = black
            .levels
            .get(index)
            .ok_or_else(|| unsupported("missing black level"))?;
        black_levels[i] = level(value.as_f32())?;
        let white = image
            .whitelevel
            .0
            .get(if image.whitelevel.0.len() == 1 { 0 } else { i })
            .ok_or_else(|| unsupported("missing white level"))?;
        white_levels[i] =
            u16::try_from(*white).map_err(|_| unsupported("white level exceeds 16 bits"))?;
        if white_levels[i] <= black_levels[i] {
            return Err(unsupported("white level is not greater than black level"));
        }
    }
    let (source_rgb, source_clipped) = if sensor_layout == SensorLayout::Cfa {
        let mut normalized = Vec::with_capacity(pixels);
        let mut saturated = Vec::with_capacity(pixels);
        for (i, &raw) in samples.iter().enumerate() {
            let pos = ((i / image.width) % 2) * 2 + (i % image.width) % 2;
            normalized.push(normalize(raw, black_levels[pos], white_levels[pos]));
            saturated.push(raw >= white_levels[pos]);
        }
        demosaic_ahd(
            image.width,
            image.height,
            &normalized,
            cfa_pattern,
            &saturated,
        )
    } else {
        let mut rgb = Vec::with_capacity(expected);
        let mut clipped = Vec::with_capacity(pixels);
        for raw in samples.as_chunks::<3>().0 {
            let mut mask = 0;
            for channel in 0..3 {
                rgb.push(normalize(
                    raw[channel],
                    black_levels[channel],
                    white_levels[channel],
                ));
                if raw[channel] >= white_levels[channel] {
                    mask |= 1 << channel;
                }
            }
            clipped.push(mask);
        }
        (rgb, clipped)
    };
    let (width, height, rgb, clipped) =
        orient(width, height, image.orientation, source_rgb, source_clipped)?;
    // rawler exposes camera WB coefficients rather than the DNG neutral tag.
    // Display-only WB may be absent; never infer it from the scene.
    let display_neutral = if image.wb_coeffs[..3]
        .iter()
        .all(|c| c.is_finite() && *c > 0.0)
    {
        Some([
            1.0 / image.wb_coeffs[0],
            1.0 / image.wb_coeffs[1],
            1.0 / image.wb_coeffs[2],
        ])
    } else {
        None
    };
    Ok(DecodedImage {
        sensor_layout,
        width,
        height,
        rgb,
        clipped,
        black_levels,
        white_levels,
        cfa_pattern,
        display_neutral,
        camera: CameraIdentity {
            make: image.make,
            model: image.model,
            decoder: DECODER_NAME.into(),
            decoder_version: DECODER_VERSION.into(),
        },
    })
}

pub fn decode_raw(path: &Path) -> Result<DecodedImage, DecodeError> {
    // rawler 0.8 reaches `todo!()` for photometric interpretations it does not
    // implement. Report that as an unsupported format instead of aborting.
    let raw = std::panic::catch_unwind(|| -> Result<RawImage, DecodeError> {
        let source = RawSource::new(path)?;
        let decoder = rawler::get_decoder(&source).map_err(|e| {
            DecodeError::UnsupportedFormat(format!(
                "unrecognized image data; expected a DNG, JPEG, PNG, or camera RAW file supported by rawler ({e})"
            ))
        })?;
        decoder
            .raw_image(&source, &RawDecodeParams::default(), false)
            .map_err(|e| DecodeError::CorruptFile(e.to_string()))
    })
    .map_err(|_| {
        DecodeError::UnsupportedFormat(
            "rawler cannot decode this file: unsupported PhotometricInterpretation or layout"
                .to_owned(),
        )
    })??;
    from_raw(raw)
}

impl RawDecoder for RawlerDecoder {
    fn decode_path(&self, path: &Path) -> Result<DecodedImage, DecodeError> {
        decode_raw(path)
    }
}

#[cfg(test)]
mod pin_tests {
    use super::DECODER_VERSION;

    /// `DECODER_VERSION` is written into every profile, so it must name the
    /// rawler version that is actually pinned in this crate's manifest.
    #[test]
    fn decoder_version_matches_the_pinned_dependency() {
        let manifest = include_str!("../Cargo.toml");
        let expected = format!("rawler = \"={DECODER_VERSION}\"");
        assert!(
            manifest.contains(&expected),
            "Cargo.toml must contain `{expected}`"
        );
    }
}

#[cfg(test)]
mod linear_black_tests {
    use super::linear_black_index;
    use rawler::rawimage::BlackLevel;

    #[test]
    fn uniform_repeat_grid_selects_each_channel() {
        // 2x2 repeat, three channels; per-channel values agree across cells.
        let cells: Vec<u16> = (0..4).flat_map(|_| [64, 66, 68]).collect();
        let black = BlackLevel::new(&cells, 2, 2, 3);
        for channel in 0..3 {
            assert_eq!(linear_black_index(&black, channel).unwrap(), channel);
        }
    }

    #[test]
    fn grid_that_varies_between_cells_fails_closed() {
        let mut cells: Vec<u16> = (0..4).flat_map(|_| [64, 66, 68]).collect();
        cells[3 * 3 + 1] = 70;
        let black = BlackLevel::new(&cells, 2, 2, 3);
        assert!(linear_black_index(&black, 0).is_ok());
        assert!(linear_black_index(&black, 1).is_err());
    }

    #[test]
    fn malformed_grids_are_rejected() {
        let two_channels = BlackLevel::new(&[0_u16; 8], 2, 2, 2);
        assert!(linear_black_index(&two_channels, 0).is_err());
        let mut short = BlackLevel::new(&[0_u16; 3], 1, 1, 3);
        short.width = 2;
        assert!(linear_black_index(&short, 0).is_err());
    }
}
