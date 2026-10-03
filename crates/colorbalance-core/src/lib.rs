//! ColorBalance color engine.
//!
//! This crate holds every color and profile concept shared by the CLI, the
//! desktop application, the browser build, and server workers. It must compile
//! for native and `wasm32` targets and must not depend on LibRaw, Tauri, or
//! any UI crate. RAW-specific decoding is injected through traits defined
//! here and implemented in `colorbalance-raw`.
//!
//! See `docs/IMPLEMENTATION_PLAN.md` for the product contract and
//! `docs/ARCHITECTURE.md` for the platform decisions.

pub mod chart;
pub mod contract;

pub use chart::{ChartModel, ChartPatch, ChartRevision};
pub use contract::{
    DecodeContract, DemosaicAlgorithm, HighlightPolicy, OrientationPolicy, OutputColor,
    OutputDepth, WhiteBalancePolicy,
};
