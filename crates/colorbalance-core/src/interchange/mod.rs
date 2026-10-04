//! Interchange formats: Academy Common LUT Format (CLF) and Adobe .cube 3D LUTs.
//!
//! Provides conversion to and from CLF and export/validation for .cube LUTs.

pub mod clf;
pub mod cube;

pub use clf::{clf_to_matrices, profile_to_clf};
pub use cube::{cube_max_error, profile_to_cube};

/// Errors encountered while converting or parsing interchange formats.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum InterchangeError {
    /// Failure parsing CLF XML or matrix content.
    #[error("CLF parse error: {0}")]
    Parse(String),
}
