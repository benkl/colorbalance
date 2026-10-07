//! RAW decoding through rawler and rendered JPEG/PNG decoding through image.

mod capture;
pub mod dng_writer;
mod metadata;
pub mod rawler_decode;
pub mod rendered;
mod restart_ljpeg;

pub use capture::{read_capture_info, CaptureGps, CaptureInfo};
pub use dng_writer::{write_dng, DngWriteSpec};
pub use metadata::{read_export_metadata, MetadataRead};
pub use rawler_decode::{decode_raw, RawlerDecoder, DECODER_NAME, DECODER_VERSION};
pub use rendered::{
    decode_rendered_image, decode_upright, render_preview_png, render_preview_rgb,
    RenderedImageDecoder, JPEG_DECODER_NAME, JPEG_DECODER_VERSION,
};

use std::io::Read;
use std::path::Path;

use colorbalance_core::contract::DecodeContract;
use colorbalance_core::decode::{DecodeError, DecodedImage, RawDecoder};

/// Image formats identified by file contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// Native RAW or DNG. Decoded through rawler.
    Raw,
    /// JPEG or PNG. Handled by the rendered-image decoder.
    Rendered,
}

/// Identify a rendered source by magic bytes; other sources go to rawler.
/// The extension cannot be trusted to select a decoder.
pub fn sniff_source(path: &Path) -> Result<SourceKind, DecodeError> {
    let mut head = [0u8; 8];
    let read = std::fs::File::open(path)?.read(&mut head)?;
    let head = &head[..read];
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) || head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(SourceKind::Rendered)
    } else {
        Ok(SourceKind::Raw)
    }
}

/// Decode a RAW or rendered source from its file contents.
pub fn decode_any(path: &Path) -> Result<DecodedImage, DecodeError> {
    match sniff_source(path)? {
        SourceKind::Rendered => decode_rendered_image(path),
        SourceKind::Raw => RawlerDecoder.decode_path(path),
    }
}

/// The decode contract of the currently shipped RAW decoder.
pub fn canonical_contract() -> DecodeContract {
    DecodeContract::canonical(DECODER_NAME, DECODER_VERSION)
}
