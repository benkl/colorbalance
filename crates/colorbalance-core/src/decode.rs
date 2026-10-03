//! Decoder-agnostic RAW decode types.
//!
//! Every decoder produces a [`DecodedImage`] under the normalization rules
//! below, so the calibration engine never knows which decoder produced the
//! data. Invariant 1 in `AGENTS.md`: all images in a profile's scope are
//! decoded with the same pinned settings.

use std::path::Path;

/// Camera and decoder identity recorded in every profile.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CameraIdentity {
    pub make: String,
    pub model: String,
    pub decoder: String,
    pub decoder_version: String,
}

/// A decoded, normalized, upright camera-RGB image.
///
/// Normalization: `v = (raw - black_c) / (white_c - black_c)` per CFA
/// position `c`, computed in f64 from the 16-bit photosite and stored as
/// f32. Values below black clamp to 0. Values above white are not clamped;
/// their photosite is flagged in [`DecodedImage::clipped`] instead.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Interleaved RGB, length `width * height * 3`, row-major, upright.
    pub rgb: Vec<f32>,
    /// Per-pixel clip bitmask: bit 0 = R, bit 1 = G, bit 2 = B. A demosaiced
    /// channel is flagged when any contributing photosite of that channel
    /// in its support window was at or above its white level.
    pub clipped: Vec<u8>,
    /// Black level per CFA position `[0]=row0col0, [1]=row0col1,
    /// `[2]=row1col0, `[3]=row1col1`.
    pub black_levels: [u16; 4],
    /// White (saturation) level per CFA position.
    pub white_levels: [u16; 4],
    /// CFA color per position, `b'R' | b'G' | b'B'`.
    pub cfa_pattern: [u8; 4],
    pub camera: CameraIdentity,
}

impl DecodedImage {
    /// Clip flags of a pixel.
    pub fn clipped_at(&self, x: u32, y: u32) -> u8 {
        self.clipped[(y as usize) * self.width as usize + x as usize]
    }

    /// Normalized RGB of a pixel.
    pub fn rgb_at(&self, x: u32, y: u32) -> [f32; 3] {
        let i = ((y as usize) * self.width as usize + x as usize) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }
}

/// A RAW decoder. Implementations must be deterministic for a given file
/// and decoder version.
pub trait RawDecoder {
    fn decode_path(&self, path: &Path) -> Result<DecodedImage, DecodeError>;
}

/// Errors raised while decoding a RAW file.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("unsupported sensor layout: {0}")]
    UnsupportedSensorLayout(String),
    #[error("corrupt file: {0}")]
    CorruptFile(String),
}
