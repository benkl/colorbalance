//! Measure a new chart against a stored profile without changing its transform.

use std::fs;
use std::path::Path;

use colorbalance_core::calibration::{self, ChartQuad};
use colorbalance_core::chart::ChartRevision;
use colorbalance_core::detection::{detect_chart, Detection};
use colorbalance_core::profile::{self, ValidationSummary};
use serde::Serialize;

use crate::commands::{check_camera, gate_config_for, BackendError, MismatchPolicy, QuadPayload};
use crate::reference_cache::ReferenceCache;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartCheckResponse {
    pub profile_path: String,
    pub image_path: String,
    pub chart_revision: ChartRevision,
    pub image_width: u32,
    pub image_height: u32,
    pub quad: [[f64; 2]; 4],
    pub quality_passed: bool,
    pub gate_failures: Vec<ChartGateFailure>,
    pub validation: ChartValidation,
    pub profile_fit_baseline: ValidationSummaryResponse,
    pub patches: Vec<ChartPatchResponse>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartGateFailure {
    pub patch: Option<String>,
    pub reason: String,
    pub measured: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartValidation {
    pub mean_delta_e: f64,
    pub median_delta_e: f64,
    pub p95_delta_e: f64,
    pub max_delta_e: f64,
    pub neutral_max_delta_e: f64,
    pub skin_max_delta_e: f64,
    pub condition_number: f64,
    pub patch_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationSummaryResponse {
    pub mean_delta_e: f64,
    pub p95_delta_e: f64,
    pub max_delta_e: f64,
    pub neutral_max_delta_e: f64,
    pub skin_max_delta_e: f64,
    pub condition_number: f64,
    pub patch_count: u32,
}

impl From<&ValidationSummary> for ValidationSummaryResponse {
    fn from(value: &ValidationSummary) -> Self {
        Self {
            mean_delta_e: value.mean_delta_e,
            p95_delta_e: value.p95_delta_e,
            max_delta_e: value.max_delta_e,
            neutral_max_delta_e: value.neutral_max_delta_e,
            skin_max_delta_e: value.skin_max_delta_e,
            condition_number: value.condition_number,
            patch_count: value.patch_count,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartPatchResponse {
    pub patch: String,
    pub source_rgb: [f64; 3],
    pub corrected_rgb: [f64; 3],
    pub target_rgb: [f64; 3],
    pub corrected_srgb: [f64; 3],
    pub target_srgb: [f64; 3],
    pub delta_e: f64,
}

fn display_srgb(linear: [f64; 3]) -> [f64; 3] {
    linear.map(|value| colorbalance_core::color::srgb_encode(value.clamp(0.0, 1.0)))
}

/// Check the sampled chart against the sealed profile's fixed stages and dataset.
/// Quality failures are returned for inspection, not treated as a reason to refit.
/// Camera and decode-contract mismatches remain blocking errors.
pub fn check_chart(
    cache: &ReferenceCache,
    profile_path: String,
    image_path: String,
    quad: Option<QuadPayload>,
) -> Result<ChartCheckResponse, BackendError> {
    let stored = fs::read_to_string(&profile_path)?;
    let profile =
        profile::from_json(&stored).map_err(|error| BackendError::Message(error.to_string()))?;
    let dataset = colorbalance_core::dataset::load(profile.chart_revision)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    if profile.dataset_digest != colorbalance_core::dataset::dataset_digest() {
        return Err(BackendError::Message(
            "profile chart dataset digest does not match the pinned dataset".to_owned(),
        ));
    }

    let image = cache
        .get_or_decode(
            Path::new(&image_path),
            || {},
            |path| {
                colorbalance_raw::decode_any(path)
                    .map_err(|error| BackendError::Message(error.to_string()))
            },
        )?
        .image;
    let warnings =
        check_camera(&profile, &image, MismatchPolicy::Block).map_err(BackendError::Message)?;
    let quad = match quad {
        Some(value) => ChartQuad {
            corners: value.corners,
        },
        None => match detect_chart(&image) {
            Detection::Found(quad) => quad,
            Detection::Missing => {
                return Err(BackendError::Message(
                    "chart not detected; select its four corners manually".to_owned(),
                ));
            }
            Detection::Ambiguous => {
                return Err(BackendError::Message(
                    "chart detection is ambiguous; select its four corners manually".to_owned(),
                ));
            }
        },
    };
    let samples = calibration::sample_patches(&image, &quad)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    let failures = calibration::evaluate_quality(&samples, &gate_config_for(&image))
        .err()
        .unwrap_or_default();
    let validation = calibration::validate_stages(&samples, &dataset, &profile.transform)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    let patches = validation
        .per_patch
        .iter()
        .map(|patch| ChartPatchResponse {
            patch: format!("{:?}", patch.patch),
            source_rgb: patch.source_rgb,
            corrected_rgb: patch.corrected_rgb,
            target_rgb: patch.target_rgb,
            corrected_srgb: display_srgb(patch.corrected_rgb),
            target_srgb: display_srgb(patch.target_rgb),
            delta_e: patch.delta_e,
        })
        .collect();
    let gate_failures = failures
        .into_iter()
        .map(|failure| ChartGateFailure {
            patch: failure.patch.map(|patch| format!("{patch:?}")),
            reason: failure.reason,
            measured: failure.measured,
        })
        .collect::<Vec<_>>();
    Ok(ChartCheckResponse {
        profile_path,
        image_path,
        chart_revision: profile.chart_revision,
        image_width: image.width,
        image_height: image.height,
        quad: quad.corners,
        quality_passed: gate_failures.is_empty(),
        gate_failures,
        validation: ChartValidation {
            mean_delta_e: validation.mean_delta_e,
            median_delta_e: validation.median_delta_e,
            p95_delta_e: validation.p95_delta_e,
            max_delta_e: validation.max_delta_e,
            neutral_max_delta_e: validation.neutral_max_delta_e,
            skin_max_delta_e: validation.skin_max_delta_e,
            condition_number: validation.condition_number,
            patch_count: validation.per_patch.len() as u32,
        },
        profile_fit_baseline: ValidationSummaryResponse::from(&profile.validation),
        patches,
        warnings,
    })
}
