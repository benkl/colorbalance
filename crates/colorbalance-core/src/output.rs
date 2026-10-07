//! Deterministic RGB TIFF and JPEG output.
//!
//! Both formats embed ICC and rebuilt, reviewed metadata. TIFF retains 16-bit
//! samples; JPEG rounds output-space samples to 8-bit before YCbCr conversion.
use std::fmt;
use std::str::FromStr;

use crate::metadata::ExportMetadata;
use jpeg_encoder::{rgb_to_ycbcr, Encoder, ImageBuffer, JpegColorType, SamplingFactor};
use serde::{Deserialize, Serialize};

pub const JPEG_MAX_DIMENSION: u32 = u16::MAX as u32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum JpegSampling {
    #[default]
    #[serde(rename = "444")]
    Yuv444,
    #[serde(rename = "422")]
    Yuv422,
    #[serde(rename = "420")]
    Yuv420,
}

impl fmt::Display for JpegSampling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Yuv444 => "4:4:4",
            Self::Yuv422 => "4:2:2",
            Self::Yuv420 => "4:2:0",
        })
    }
}

impl FromStr for JpegSampling {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "444" | "4:4:4" => Ok(Self::Yuv444),
            "422" | "4:2:2" => Ok(Self::Yuv422),
            "420" | "4:2:0" => Ok(Self::Yuv420),
            _ => Err("expected 444, 422, or 420"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("output dimensions must be nonzero")]
    ZeroDimensions,
    #[error("pixel count must equal width * height * 3")]
    PixelCount,
    #[error("JPEG width and height must not exceed 65535")]
    JpegDimensions,
    #[error("JPEG quality must be between 1 and 100")]
    InvalidQuality,
    #[error("ICC profile must not be empty")]
    EmptyIcc,
    #[error("output or metadata exceeds format limits")]
    TooLarge,
    #[error("JPEG encoding failed: {0}")]
    Jpeg(String),
}

fn validate(width: u32, height: u32, pixels: &[u16], icc: &[u8]) -> Result<(), OutputError> {
    if width == 0 || height == 0 {
        return Err(OutputError::ZeroDimensions);
    }
    if icc.is_empty() {
        return Err(OutputError::EmptyIcc);
    }
    if (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(3))
        != Some(pixels.len())
    {
        return Err(OutputError::PixelCount);
    }
    Ok(())
}

/// Rebuild all directories from reviewed values; source offsets are never copied.
pub fn encode_tiff_rgb_u16_with_metadata(
    width: u32,
    height: u32,
    pixels: &[u16],
    icc_profile: &[u8],
    metadata: &ExportMetadata,
) -> Result<Vec<u8>, OutputError> {
    validate(width, height, pixels, icc_profile)?;
    crate::output_metadata::serialize(metadata, Some((width, height, pixels, icc_profile)), true)
}

struct Rgb16Image<'a> {
    pixels: &'a [u16],
    width: u16,
    height: u16,
}

impl ImageBuffer for Rgb16Image<'_> {
    fn get_jpeg_color_type(&self) -> JpegColorType {
        JpegColorType::Ycbcr
    }
    fn width(&self) -> u16 {
        self.width
    }
    fn height(&self) -> u16 {
        self.height
    }
    fn fill_buffers(&self, y: u16, buffers: &mut [Vec<u8>; 4]) {
        let row = (y as usize) * (self.width as usize) * 3;
        let (triples, _) = self.pixels[row..row + self.width as usize * 3].as_chunks::<3>();
        for rgb in triples {
            let (l, cb, cr) = rgb_to_ycbcr(
                ((rgb[0] as u32 + 128) / 257) as u8,
                ((rgb[1] as u32 + 128) / 257) as u8,
                ((rgb[2] as u32 + 128) / 257) as u8,
            );
            buffers[0].push(l);
            buffers[1].push(cb);
            buffers[2].push(cr);
        }
    }
}

/// Encode 8-bit JPEG from the output-space RGB u16 samples, with ICC and reviewed metadata.
pub fn encode_jpeg_rgb_u16(
    width: u32,
    height: u32,
    pixels: &[u16],
    icc_profile: &[u8],
    quality: u8,
    sampling: JpegSampling,
    metadata: &ExportMetadata,
) -> Result<Vec<u8>, OutputError> {
    if width > JPEG_MAX_DIMENSION || height > JPEG_MAX_DIMENSION {
        return Err(OutputError::JpegDimensions);
    }
    validate(width, height, pixels, icc_profile)?;
    if !(1..=100).contains(&quality) {
        return Err(OutputError::InvalidQuality);
    }
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, quality);
    encoder.set_sampling_factor(match sampling {
        JpegSampling::Yuv444 => SamplingFactor::R_4_4_4,
        JpegSampling::Yuv422 => SamplingFactor::R_4_2_2,
        JpegSampling::Yuv420 => SamplingFactor::R_4_2_0,
    });
    encoder
        .add_icc_profile(icc_profile)
        .map_err(|e| OutputError::Jpeg(e.to_string()))?;
    if !metadata.exif.is_empty() {
        // The source reader enforces the APP1 budget. Caller-created oversized metadata is optional.
        if let Ok(exif) = crate::output_metadata::serialize(metadata, None, false) {
            if exif.len() <= 65527 {
                encoder
                    .add_exif_metadata(&exif)
                    .map_err(|e| OutputError::Jpeg(e.to_string()))?;
            }
        }
    }
    if let Some(xmp) = &metadata.xmp {
        const PREFIX: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
        if xmp.len() <= 65533 - PREFIX.len() {
            let mut segment = Vec::with_capacity(PREFIX.len() + xmp.len());
            segment.extend_from_slice(PREFIX);
            segment.extend_from_slice(xmp);
            encoder
                .add_app_segment(1, segment)
                .map_err(|e| OutputError::Jpeg(e.to_string()))?;
        }
    }
    if let Some(iptc) = &metadata.iptc {
        // Photoshop 3.0 APP13, 8BIM resource 0x0404 (IPTC-NAA).
        const HEADER: &[u8] = b"Photoshop 3.0\0";
        if iptc.len() <= 65533 - HEADER.len() - 12 && iptc.len() <= u32::MAX as usize {
            let mut segment = Vec::with_capacity(HEADER.len() + 12 + iptc.len());
            segment.extend_from_slice(HEADER);
            segment.extend_from_slice(b"8BIM\x04\x04\0\0");
            segment.extend_from_slice(&(iptc.len() as u32).to_be_bytes());
            segment.extend_from_slice(iptc);
            if iptc.len() & 1 != 0 {
                segment.push(0);
            }
            if segment.len() <= 65533 {
                encoder
                    .add_app_segment(13, segment)
                    .map_err(|e| OutputError::Jpeg(e.to_string()))?;
            }
        }
    }
    encoder
        .encode_image(Rgb16Image {
            pixels,
            width: width as u16,
            height: height as u16,
        })
        .map_err(|e| OutputError::Jpeg(e.to_string()))?;
    Ok(bytes)
}

/// Encode TIFF without source metadata. Retained for callers using the original API.
///
/// # Panics
///
/// Panics on invalid dimensions, pixels, ICC, or classic TIFF size limits.
pub fn encode_tiff_rgb_u16(width: u32, height: u32, pixels: &[u16], icc_profile: &[u8]) -> Vec<u8> {
    assert!(!icc_profile.is_empty(), "ICC profile must not be empty");
    encode_tiff_rgb_u16_with_metadata(
        width,
        height,
        pixels,
        icc_profile,
        &ExportMetadata::default(),
    )
    .expect("TIFF dimensions, pixels, or classic TIFF file size invalid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TiffView<'a> {
        width: u32,
        height: u32,
        bits_per_sample: [u16; 3],
        strip_offset: usize,
        strip_byte_count: usize,
        icc: &'a [u8],
        bytes: &'a [u8],
    }

    fn read_u16(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    fn read_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    /// Minimal reader for the tags emitted by this module. Keeping this
    /// reader in tests means assertions inspect the encoded structure rather
    /// than relying on offsets that duplicate the writer's arithmetic.
    fn read_tiff(bytes: &[u8]) -> TiffView<'_> {
        assert_eq!(&bytes[0..2], b"II");
        assert_eq!(read_u16(bytes, 2), 42);
        let ifd = read_u32(bytes, 4) as usize;
        let count = read_u16(bytes, ifd) as usize;
        let mut width = None;
        let mut height = None;
        let mut bits = None;
        let mut strip_offset = None;
        let mut strip_byte_count = None;
        let mut icc = None;
        let mut previous_tag = 0;
        for i in 0..count {
            let at = ifd + 2 + i * 12;
            let tag = read_u16(bytes, at);
            let field_type = read_u16(bytes, at + 2);
            let item_count = read_u32(bytes, at + 4);
            let value = at + 8;
            assert!(tag > previous_tag, "IFD tags must ascend");
            previous_tag = tag;
            match tag {
                256 => {
                    assert_eq!(field_type, 4);
                    assert_eq!(item_count, 1);
                    width = Some(read_u32(bytes, value));
                }
                257 => {
                    assert_eq!(field_type, 4);
                    assert_eq!(item_count, 1);
                    height = Some(read_u32(bytes, value));
                }
                258 => {
                    assert_eq!(field_type, 3);
                    assert_eq!(item_count, 3);
                    let at = read_u32(bytes, value) as usize;
                    bits = Some([
                        read_u16(bytes, at),
                        read_u16(bytes, at + 2),
                        read_u16(bytes, at + 4),
                    ]);
                }
                259 => {
                    assert_eq!(field_type, 3);
                    assert_eq!(item_count, 1);
                    assert_eq!(read_u16(bytes, value), 1);
                }
                262 => {
                    assert_eq!(field_type, 3);
                    assert_eq!(item_count, 1);
                    assert_eq!(read_u16(bytes, value), 2);
                }
                273 => {
                    assert_eq!(field_type, 4);
                    assert_eq!(item_count, 1);
                    strip_offset = Some(read_u32(bytes, value) as usize);
                }
                277 => {
                    assert_eq!(field_type, 3);
                    assert_eq!(item_count, 1);
                    assert_eq!(read_u16(bytes, value), 3);
                }
                278 => {
                    assert_eq!(field_type, 4);
                    assert_eq!(item_count, 1);
                }
                279 => {
                    assert_eq!(field_type, 4);
                    assert_eq!(item_count, 1);
                    strip_byte_count = Some(read_u32(bytes, value) as usize);
                }
                284 => {
                    assert_eq!(field_type, 3);
                    assert_eq!(item_count, 1);
                    assert_eq!(read_u16(bytes, value), 1);
                }
                34675 => {
                    assert_eq!(field_type, 7);
                    let len = item_count as usize;
                    let at = if len <= 4 {
                        value
                    } else {
                        read_u32(bytes, value) as usize
                    };
                    icc = Some(&bytes[at..at + len]);
                }
                tag => panic!("unexpected TIFF tag {tag}"),
            }
        }
        assert_eq!(read_u32(bytes, ifd + 2 + count * 12), 0);
        let strip_offset = strip_offset.expect("StripOffsets");
        let strip_byte_count = strip_byte_count.expect("StripByteCounts");
        TiffView {
            width: width.expect("ImageWidth"),
            height: height.expect("ImageLength"),
            bits_per_sample: bits.expect("BitsPerSample"),
            strip_offset,
            strip_byte_count,
            icc: icc.expect("ICCProfile"),
            bytes,
        }
    }

    #[test]
    fn tiff_round_trips_pixel_bytes_and_tags() {
        let pixels = [0u16, 1, 65535, 32768, 1234, 54321];
        let bytes = encode_tiff_rgb_u16(2, 1, &pixels, b"profile-bytes");
        let view = read_tiff(&bytes);
        assert_eq!((view.width, view.height), (2, 1));
        assert_eq!(view.bits_per_sample, [16, 16, 16]);
        assert_eq!(view.strip_byte_count, pixels.len() * 2);
        let expected: Vec<u8> = pixels.iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(&view.bytes[view.strip_offset..], expected.as_slice());
    }

    #[test]
    fn strip_byte_count_matches_data_length() {
        let pixels: Vec<u16> = (0..27).map(|i| i * 257).collect();
        let bytes = encode_tiff_rgb_u16(3, 3, &pixels, b"icc");
        let view = read_tiff(&bytes);
        assert_eq!(view.strip_byte_count, pixels.len() * 2);
        assert_eq!(view.strip_offset + view.strip_byte_count, bytes.len());
    }

    #[test]
    fn bytes_are_deterministic() {
        let pixels = [9u16, 8, 7, 6, 5, 4];
        assert_eq!(
            encode_tiff_rgb_u16(1, 2, &pixels, b"icc"),
            encode_tiff_rgb_u16(1, 2, &pixels, b"icc")
        );
    }

    #[test]
    fn icc_profile_is_stored_verbatim_at_any_length() {
        let pixels = [1u16, 2, 3];
        for len in [1usize, 2, 3, 600, 601] {
            let icc: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let bytes = encode_tiff_rgb_u16(1, 1, &pixels, &icc);
            let view = read_tiff(&bytes);
            assert_eq!(view.icc, icc.as_slice(), "profile length {len}");
            assert_eq!(view.strip_offset % 2, 0, "strip must be word aligned");
            assert_eq!(view.strip_offset + view.strip_byte_count, bytes.len());
            let expected: Vec<u8> = pixels.iter().flat_map(|v| v.to_le_bytes()).collect();
            assert_eq!(&bytes[view.strip_offset..], expected.as_slice());
        }
    }

    #[test]
    #[should_panic(expected = "ICC profile must not be empty")]
    fn empty_icc_profile_is_rejected() {
        encode_tiff_rgb_u16(1, 1, &[0, 0, 0], &[]);
    }
}
