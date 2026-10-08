//! Interchange formats: Academy Common LUT Format (CLF), Adobe .cube 3D LUTs
//! and DNG Camera Profiles (.dcp).
//!
//! Provides conversion to and from CLF and export/validation for .cube LUTs
//! and .dcp camera profiles.

pub mod clf;
pub mod cube;
pub mod dcp;

pub use clf::{clf_to_matrices, profile_to_clf};
pub use cube::{cube_max_error, profile_to_cube};
pub use dcp::profile_to_dcp;

/// Errors encountered while converting or parsing interchange formats.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum InterchangeError {
    /// Failure parsing CLF XML or matrix content.
    #[error("CLF parse error: {0}")]
    Parse(String),
    /// An export was refused because the profile or arguments cannot produce
    /// an honest result.
    #[error("export refused: {0}")]
    Refused(String),
}
