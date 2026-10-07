//! ColorBalance color engine.
//!
//! This crate holds every color and profile concept shared by the CLI, the
//! desktop application, the browser build, and server workers. It must compile
//! for native and `wasm32` targets and must not depend on rawler, Tauri, or
//! any UI crate. RAW-specific decoding is injected through traits defined
//! here and implemented in `colorbalance-raw`.
//!
//! See `docs/IMPLEMENTATION_PLAN.md` for the product contract,
//! `docs/ARCHITECTURE.md` for the platform decisions, and
//! `docs/CONTRACTS.md` for the binding module interfaces.

pub mod ahd;
pub mod batch;
pub mod calibration;
pub mod chart;
pub mod color;
pub mod contract;
pub mod dataset;
pub mod decode;
pub mod detection;
pub mod interchange;
pub mod metadata;
pub mod output;
mod output_metadata;
#[cfg(test)]
mod output_metadata_tests;
pub mod output_space;
pub mod par;
pub mod profile;

pub use batch::{
    collect_inputs, run_batch, unique_output_path, BatchFileResult, BatchOptions, BatchSummary,
    CancelFlag, CancelState, DEFAULT_EXTENSIONS, DEFAULT_WORKERS,
};
pub use chart::{ChartModel, ChartPatch, ChartRevision};
pub use contract::{
    DecodeContract, DemosaicAlgorithm, HighlightPolicy, OrientationPolicy, OutputColor,
    OutputDepth, WhiteBalancePolicy,
};
pub use decode::{CameraIdentity, DecodeError, DecodedImage, RawDecoder};
pub use metadata::{ExportMetadata, MetadataReport};
