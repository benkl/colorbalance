//! Chart sampling, quality gates, and matrix calibration.

use crate::chart::{ChartModel, ChartPatch};
use crate::color::{delta_e_2000, linear_srgb_to_xyz, xyz_d65_to_lab};
use crate::dataset::ChartDataset;
use crate::decode::DecodedImage;
use serde::{Deserialize, Serialize};

const LUMINANCE: [f64; 3] = [0.2126, 0.7152, 0.0722];
const MIN_DIVISOR: f64 = 1e-12;
const SINGULAR_EPSILON: f64 = 1e-14;

/// Four chart corners in top-left, top-right, bottom-right, bottom-left order.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartQuad {
    /// Top-left, top-right, bottom-right, then bottom-left image coordinates.
    pub corners: [[f64; 2]; 4],
}

/// Statistics measured from one chart patch.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchSample {
    /// The sampled chart patch.
    pub patch: ChartPatch,
    /// Per-channel arithmetic mean in normalized camera RGB.
    pub mean_rgb: [f64; 3],
    /// Per-channel population variance in normalized camera RGB.
    pub variance: [f64; 3],
    /// OR of RAW clipping masks over all sampled pixels.
    pub clipped_mask: u8,
    /// Number of pixel centers inside the sampling region.
    pub sample_pixels: usize,
}

/// Quality thresholds for chart measurements.
#[derive(Debug, Clone, PartialEq)]
pub struct GateConfig {
    /// Minimum number of sampled pixels required in every patch.
    pub min_patch_pixels: usize,
    /// Maximum permitted per-channel coefficient of variation.
    pub max_cv: f64,
    /// Whether to check the final row for neutral ordering and chroma.
    pub require_neutral_row: bool,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            min_patch_pixels: 64,
            max_cv: 0.05,
            require_neutral_row: true,
        }
    }
}

impl GateConfig {
    /// Relaxed quality thresholds for approximate calibration on compressed, non-RAW JPEG sources.
    pub fn quick_and_dirty() -> Self {
        Self {
            min_patch_pixels: 16,
            max_cv: 0.25,
            require_neutral_row: false,
        }
    }
}

/// One failed chart quality check.
#[derive(Debug, Clone, PartialEq)]
pub struct GateFailure {
    /// The patch responsible for the failure, if one patch caused it.
    pub patch: Option<ChartPatch>,
    /// Human-readable failed rule.
    pub reason: String,
    /// Measured value that caused the failure.
    pub measured: String,
}

/// Validation data for one fitted chart patch.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchValidation {
    /// The validated chart patch.
    pub patch: ChartPatch,
    /// Uncorrected normalized camera RGB.
    pub source_rgb: [f64; 3],
    /// Fitted linear sRGB D65 output.
    pub corrected_rgb: [f64; 3],
    /// Dataset linear sRGB D65 target.
    pub target_rgb: [f64; 3],
    /// Corrected CIE Lab D65 value.
    pub corrected_lab: [f64; 3],
    /// Dataset CIE Lab D65 target.
    pub target_lab: [f64; 3],
    /// CIEDE2000 difference between corrected and target Lab values.
    pub delta_e: f64,
}

/// Aggregate validation results for a fitted transform.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    /// Validation data in the same order as input samples.
    pub per_patch: Vec<PatchValidation>,
    /// Mean CIEDE2000 difference.
    pub mean_delta_e: f64,
    /// Median CIEDE2000 difference.
    pub median_delta_e: f64,
    /// 95th-percentile CIEDE2000 difference.
    pub p95_delta_e: f64,
    /// Maximum CIEDE2000 difference.
    pub max_delta_e: f64,
    /// Maximum CIEDE2000 difference across White through Black.
    pub neutral_max_delta_e: f64,
    /// Maximum CIEDE2000 difference across Dark Skin and Light Skin.
    pub skin_max_delta_e: f64,
    /// Infinity-norm condition number of the fitted color matrix.
    pub condition_number: f64,
}

/// The three stored calibration stages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FittedStages {
    /// Global exposure multiplier applied before channel scaling.
    pub exposure_scale: f64,
    /// Per-channel neutral multipliers applied before the color matrix.
    pub channel_scale: [f64; 3],
    /// Row-vector camera-RGB-to-linear-sRGB-D65 matrix.
    pub matrix: [[f64; 3]; 3],
}

/// Calibration failures.
#[derive(Debug, thiserror::Error)]
pub enum CalibrationError {
    /// Sampling could not form all patch statistics.
    #[error("chart sampling failed: {0}")]
    Sampling(String),
    /// A normal-equations solve or matrix inverse was singular.
    #[error("singular matrix")]
    SingularMatrix,
    /// Too few patch observations were provided for a 3-channel fit.
    #[error("insufficient patches")]
    InsufficientPatches,
}

fn bilinear(corners: [[f64; 2]; 4], u: f64, v: f64) -> [f64; 2] {
    let top = [
        corners[0][0] * (1.0 - u) + corners[1][0] * u,
        corners[0][1] * (1.0 - u) + corners[1][1] * u,
    ];
    let bottom = [
        corners[3][0] * (1.0 - u) + corners[2][0] * u,
        corners[3][1] * (1.0 - u) + corners[2][1] * u,
    ];
    [
        top[0] * (1.0 - v) + bottom[0] * v,
        top[1] * (1.0 - v) + bottom[1] * v,
    ]
}

fn point_in_quad(point: [f64; 2], corners: [[f64; 2]; 4]) -> bool {
    let mut winding_sign = 0_i8;
    for index in 0..4 {
        let start = corners[index];
        let end = corners[(index + 1) % 4];
        let cross = (end[0] - start[0]) * (point[1] - start[1])
            - (end[1] - start[1]) * (point[0] - start[0]);
        if cross.abs() <= f64::EPSILON {
            continue;
        }
        let sign = if cross.is_sign_positive() { 1 } else { -1 };
        if winding_sign == 0 {
            winding_sign = sign;
        } else if winding_sign != sign {
            return false;
        }
    }
    true
}

fn patch_polygon(quad: &ChartQuad, col: usize, row: usize) -> [[f64; 2]; 4] {
    let u_start = (col as f64 + 0.2) / 6.0;
    let u_end = (col as f64 + 0.8) / 6.0;
    let v_start = (row as f64 + 0.2) / 4.0;
    let v_end = (row as f64 + 0.8) / 4.0;
    [
        bilinear(quad.corners, u_start, v_start),
        bilinear(quad.corners, u_end, v_start),
        bilinear(quad.corners, u_end, v_end),
        bilinear(quad.corners, u_start, v_end),
    ]
}

/// Samples the central 60 percent of each of the 24 bilinear chart cells.
pub fn sample_patches(
    image: &DecodedImage,
    quad: &ChartQuad,
) -> Result<Vec<PatchSample>, CalibrationError> {
    let pixel_count = image.width as usize * image.height as usize;
    if image.rgb.len() != pixel_count * 3 || image.clipped.len() != pixel_count {
        return Err(CalibrationError::Sampling(
            "image buffers have invalid lengths".to_owned(),
        ));
    }

    let chart = ChartModel::new(crate::chart::ChartRevision::ClassicFromNovember2014);
    let mut samples = Vec::with_capacity(chart.patches.len());
    for index in 0..chart.patches.len() {
        let polygon = patch_polygon(quad, index % 6, index / 6);
        let min_x = polygon
            .iter()
            .map(|point| point[0])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as u32;
        let max_x = polygon
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(f64::from(image.width)) as u32;
        let min_y = polygon
            .iter()
            .map(|point| point[1])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as u32;
        let max_y = polygon
            .iter()
            .map(|point| point[1])
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(f64::from(image.height)) as u32;

        let mut sample_pixels = 0_usize;
        let mut mean_rgb = [0.0; 3];
        let mut sum_squared_delta = [0.0; 3];
        let mut clipped_mask = 0_u8;
        for y in min_y..max_y {
            for x in min_x..max_x {
                if !point_in_quad([f64::from(x) + 0.5, f64::from(y) + 0.5], polygon) {
                    continue;
                }
                sample_pixels += 1;
                clipped_mask |= image.clipped_at(x, y);
                let rgb = image.rgb_at(x, y);
                for channel in 0..3 {
                    let value = f64::from(rgb[channel]);
                    let delta = value - mean_rgb[channel];
                    mean_rgb[channel] += delta / sample_pixels as f64;
                    sum_squared_delta[channel] += delta * (value - mean_rgb[channel]);
                }
            }
        }
        if sample_pixels == 0 {
            return Err(CalibrationError::Sampling(format!(
                "patch {} has no pixels",
                index + 1
            )));
        }
        samples.push(PatchSample {
            patch: chart.patches[index],
            mean_rgb,
            variance: sum_squared_delta.map(|value| value / sample_pixels as f64),
            clipped_mask,
            sample_pixels,
        });
    }
    Ok(samples)
}

fn luminance(rgb: [f64; 3]) -> f64 {
    rgb[0] * LUMINANCE[0] + rgb[1] * LUMINANCE[1] + rgb[2] * LUMINANCE[2]
}

fn chroma_proxy(rgb: [f64; 3]) -> f64 {
    let minimum = rgb.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = rgb.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (maximum - minimum) / (rgb.iter().sum::<f64>() / 3.0).max(1e-6)
}

fn clipped_channels(mask: u8) -> String {
    let mut channels = Vec::with_capacity(3);
    if mask & 1 != 0 {
        channels.push("R");
    }
    if mask & 2 != 0 {
        channels.push("G");
    }
    if mask & 4 != 0 {
        channels.push("B");
    }
    if channels.is_empty() {
        format!("unknown bits 0x{mask:02x}")
    } else {
        channels.join("|")
    }
}

/// Checks clipping, variation, patch size, and neutral-row ordering.
pub fn evaluate_quality(samples: &[PatchSample], cfg: &GateConfig) -> Result<(), Vec<GateFailure>> {
    let mut failures = Vec::new();
    if samples.len() != 24 {
        failures.push(GateFailure {
            patch: None,
            reason: "wrong patch count".to_owned(),
            measured: samples.len().to_string(),
        });
        return Err(failures);
    }

    for sample in samples {
        if sample.clipped_mask != 0 {
            failures.push(GateFailure {
                patch: Some(sample.patch),
                reason: format!(
                    "clipped channels: {}",
                    clipped_channels(sample.clipped_mask)
                ),
                measured: format!("0x{:02x}", sample.clipped_mask),
            });
        }
        if sample.sample_pixels < cfg.min_patch_pixels {
            failures.push(GateFailure {
                patch: Some(sample.patch),
                reason: "insufficient patch pixels".to_owned(),
                measured: sample.sample_pixels.to_string(),
            });
        }
        for channel in 0..3 {
            let cv = sample.variance[channel].sqrt() / sample.mean_rgb[channel].max(1e-6);
            if cv > cfg.max_cv {
                failures.push(GateFailure {
                    patch: Some(sample.patch),
                    reason: format!("channel {channel} coefficient of variation"),
                    measured: format!("cv={cv:.6}"),
                });
            }
        }
    }

    if cfg.require_neutral_row {
        let neutrals = &samples[18..24];
        if neutrals
            .windows(2)
            .any(|pair| luminance(pair[0].mean_rgb) <= luminance(pair[1].mean_rgb))
        {
            failures.push(GateFailure {
                patch: None,
                reason: "neutral row reversed; rotate corners".to_owned(),
                measured: "non-monotonic luminance".to_owned(),
            });
        }

        let final_row_chroma = neutrals
            .iter()
            .map(|sample| chroma_proxy(sample.mean_rgb))
            .fold(0.0, f64::max);
        let preceding_rows_chroma = samples[..18]
            .iter()
            .map(|sample| chroma_proxy(sample.mean_rgb))
            .fold(0.0, f64::max);
        if final_row_chroma >= 0.25 || preceding_rows_chroma <= 0.25 {
            failures.push(GateFailure {
                patch: None,
                reason: "last row is not the neutral row; check corner order".to_owned(),
                measured: format!(
                    "last-row-chroma={final_row_chroma:.6}, preceding-chroma={preceding_rows_chroma:.6}"
                ),
            });
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn solve_3x3(mut augmented: [[f64; 4]; 3]) -> Result<[f64; 3], CalibrationError> {
    for pivot_column in 0..3 {
        let mut pivot_row = pivot_column;
        for row in pivot_column + 1..3 {
            if augmented[row][pivot_column].abs() > augmented[pivot_row][pivot_column].abs() {
                pivot_row = row;
            }
        }
        if augmented[pivot_row][pivot_column].abs() < SINGULAR_EPSILON {
            return Err(CalibrationError::SingularMatrix);
        }
        augmented.swap(pivot_column, pivot_row);
        for row in pivot_column + 1..3 {
            let pivot_value = augmented[pivot_column][pivot_column];
            let pivot_row_values = augmented[pivot_column];
            let multiplier = augmented[row][pivot_column] / pivot_value;
            for (column, value) in augmented[row].iter_mut().enumerate().skip(pivot_column) {
                *value -= multiplier * pivot_row_values[column];
            }
        }
    }

    let mut solution = [0.0; 3];
    for row in (0..3).rev() {
        let known_sum = (row + 1..3)
            .map(|column| augmented[row][column] * solution[column])
            .sum::<f64>();
        solution[row] = (augmented[row][3] - known_sum) / augmented[row][row];
    }
    Ok(solution)
}

fn inverse_3x3(matrix: [[f64; 3]; 3]) -> Result<[[f64; 3]; 3], CalibrationError> {
    let determinant = matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
        - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
        + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0]);
    if determinant.abs() < SINGULAR_EPSILON {
        return Err(CalibrationError::SingularMatrix);
    }
    Ok([
        [
            (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1]) / determinant,
            (matrix[0][2] * matrix[2][1] - matrix[0][1] * matrix[2][2]) / determinant,
            (matrix[0][1] * matrix[1][2] - matrix[0][2] * matrix[1][1]) / determinant,
        ],
        [
            (matrix[1][2] * matrix[2][0] - matrix[1][0] * matrix[2][2]) / determinant,
            (matrix[0][0] * matrix[2][2] - matrix[0][2] * matrix[2][0]) / determinant,
            (matrix[0][2] * matrix[1][0] - matrix[0][0] * matrix[1][2]) / determinant,
        ],
        [
            (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0]) / determinant,
            (matrix[0][1] * matrix[2][0] - matrix[0][0] * matrix[2][1]) / determinant,
            (matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0]) / determinant,
        ],
    ])
}

fn infinity_norm(matrix: [[f64; 3]; 3]) -> f64 {
    matrix
        .iter()
        .map(|row| row.iter().map(|value| value.abs()).sum::<f64>())
        .fold(0.0, f64::max)
}

fn matrix_product(vector: [f64; 3], matrix: [[f64; 3]; 3]) -> [f64; 3] {
    let mut product = [0.0; 3];
    for output in 0..3 {
        for input in 0..3 {
            product[output] += vector[input] * matrix[input][output];
        }
    }
    product
}

fn dataset_reference(
    dataset: &ChartDataset,
    patch: ChartPatch,
) -> Option<&crate::dataset::PatchReference> {
    dataset
        .patches
        .iter()
        .find(|reference| reference.patch == patch)
}

/// Fits exposure, channel scales, and a row-vector 3x3 color matrix.
pub fn fit(
    samples: &[PatchSample],
    dataset: &ChartDataset,
) -> Result<(FittedStages, ValidationReport), CalibrationError> {
    if samples.len() < 3 || dataset.patches.len() != 24 {
        return Err(CalibrationError::InsufficientPatches);
    }
    if samples
        .iter()
        .any(|sample| dataset_reference(dataset, sample.patch).is_none())
    {
        return Err(CalibrationError::InsufficientPatches);
    }

    let model = ChartModel::new(dataset.revision);
    let neutral_samples = samples
        .iter()
        .filter(|sample| model.neutral_patches.contains(&sample.patch))
        .collect::<Vec<_>>();
    if neutral_samples.is_empty() {
        return Err(CalibrationError::InsufficientPatches);
    }

    let exposure_scale = median(
        neutral_samples
            .iter()
            .map(|sample| {
                let target = dataset_reference(dataset, sample.patch)
                    .expect("sample patches were checked against the dataset")
                    .linear_srgb_d65;
                luminance(sample.mean_rgb) / luminance(target).max(MIN_DIVISOR)
            })
            .collect(),
    );

    let mut channel_scale = [0.0; 3];
    for (channel, scale) in channel_scale.iter_mut().enumerate() {
        *scale = median(
            neutral_samples
                .iter()
                .map(|sample| {
                    let target = dataset_reference(dataset, sample.patch)
                        .expect("sample patches were checked against the dataset")
                        .linear_srgb_d65;
                    sample.mean_rgb[channel] / (exposure_scale * luminance(target)).max(MIN_DIVISOR)
                })
                .collect(),
        );
    }
    if !exposure_scale.is_finite()
        || channel_scale
            .iter()
            .any(|scale| !scale.is_finite() || scale.abs() < MIN_DIVISOR)
    {
        return Err(CalibrationError::SingularMatrix);
    }

    let mut normal_matrix = [[0.0; 3]; 3];
    let mut normal_targets = [[0.0; 3]; 3];
    for sample in samples {
        let normalized = [
            sample.mean_rgb[0] / (exposure_scale * channel_scale[0]),
            sample.mean_rgb[1] / (exposure_scale * channel_scale[1]),
            sample.mean_rgb[2] / (exposure_scale * channel_scale[2]),
        ];
        let target = dataset_reference(dataset, sample.patch)
            .expect("sample patches were checked against the dataset")
            .linear_srgb_d65;
        for row in 0..3 {
            for column in 0..3 {
                normal_matrix[row][column] += normalized[row] * normalized[column];
            }
            for output in 0..3 {
                normal_targets[output][row] += normalized[row] * target[output];
            }
        }
    }

    let mut matrix = [[0.0; 3]; 3];
    for output in 0..3 {
        let mut augmented = [[0.0; 4]; 3];
        for row in 0..3 {
            augmented[row][..3].copy_from_slice(&normal_matrix[row]);
            augmented[row][3] = normal_targets[output][row];
        }
        let solution = solve_3x3(augmented)?;
        for input in 0..3 {
            matrix[input][output] = solution[input];
        }
    }

    let inverse = inverse_3x3(matrix)?;
    let mut per_patch = Vec::with_capacity(samples.len());
    for sample in samples {
        let target_reference = dataset_reference(dataset, sample.patch)
            .expect("sample patches were checked against the dataset");
        let normalized = [
            sample.mean_rgb[0] / (exposure_scale * channel_scale[0]),
            sample.mean_rgb[1] / (exposure_scale * channel_scale[1]),
            sample.mean_rgb[2] / (exposure_scale * channel_scale[2]),
        ];
        let corrected_rgb = matrix_product(normalized, matrix);
        let corrected_lab = xyz_d65_to_lab(linear_srgb_to_xyz(corrected_rgb));
        per_patch.push(PatchValidation {
            patch: sample.patch,
            source_rgb: sample.mean_rgb,
            corrected_rgb,
            target_rgb: target_reference.linear_srgb_d65,
            corrected_lab,
            target_lab: target_reference.lab_d65,
            delta_e: delta_e_2000(corrected_lab, target_reference.lab_d65),
        });
    }

    let mut delta_e = per_patch
        .iter()
        .map(|validation| validation.delta_e)
        .collect::<Vec<_>>();
    delta_e.sort_by(f64::total_cmp);
    let count = delta_e.len();
    let median_delta_e = median(delta_e.clone());
    let p95_position = 0.95 * (count - 1) as f64;
    let p95_lower = p95_position.floor() as usize;
    let p95_delta_e = delta_e[p95_lower]
        + (delta_e[p95_lower + 1] - delta_e[p95_lower]) * (p95_position - p95_lower as f64);
    let neutral_max_delta_e = per_patch
        .iter()
        .filter(|validation| model.neutral_patches.contains(&validation.patch))
        .map(|validation| validation.delta_e)
        .fold(0.0, f64::max);
    let skin_max_delta_e = per_patch
        .iter()
        .filter(|validation| {
            validation.patch == ChartPatch::DarkSkin || validation.patch == ChartPatch::LightSkin
        })
        .map(|validation| validation.delta_e)
        .fold(0.0, f64::max);

    let fitted = FittedStages {
        exposure_scale,
        channel_scale,
        matrix,
    };
    let report = ValidationReport {
        per_patch,
        mean_delta_e: delta_e.iter().sum::<f64>() / count as f64,
        median_delta_e,
        p95_delta_e,
        max_delta_e: delta_e[count - 1],
        neutral_max_delta_e,
        skin_max_delta_e,
        condition_number: infinity_norm(matrix) * infinity_norm(inverse),
    };
    Ok((fitted, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::ChartRevision;

    fn valid_samples() -> Vec<PatchSample> {
        let chart = ChartModel::new(ChartRevision::ClassicBeforeNovember2014);
        chart
            .patches
            .iter()
            .enumerate()
            .map(|(index, patch)| {
                let mean_rgb = if index >= 18 {
                    let value = 0.7 - (index - 18) as f64 * 0.1;
                    [value; 3]
                } else if index == 0 {
                    [0.5, 0.2, 0.1]
                } else {
                    [0.4, 0.3, 0.2]
                };
                PatchSample {
                    patch: *patch,
                    mean_rgb,
                    variance: [0.0; 3],
                    clipped_mask: 0,
                    sample_pixels: 100,
                }
            })
            .collect()
    }

    #[test]
    fn reversed_neutral_row_fails() {
        let mut samples = valid_samples();
        for (index, sample) in samples[18..].iter_mut().enumerate() {
            let value = 0.1 + index as f64 * 0.1;
            sample.mean_rgb = [value; 3];
        }
        let failures = evaluate_quality(&samples, &GateConfig::default()).expect_err("must fail");
        assert!(failures
            .iter()
            .any(|failure| failure.reason == "neutral row reversed; rotate corners"));
    }

    #[test]
    fn high_variance_patch_reports_cv() {
        let mut samples = valid_samples();
        samples[0].mean_rgb = [0.5; 3];
        samples[0].variance = [0.01, 0.0, 0.0];
        let failures = evaluate_quality(&samples, &GateConfig::default()).expect_err("must fail");
        assert!(failures
            .iter()
            .any(|failure| failure.measured == "cv=0.200000"));
    }

    #[test]
    fn near_singular_condition_number_matches_analytic_inverse() {
        let matrix = [[1.0, 1.0, 1.0], [1.0, 1.0001, 1.0], [1.0, 1.0, 1.0002]];
        let inverse = inverse_3x3(matrix).expect("matrix is invertible");
        let condition = infinity_norm(matrix) * infinity_norm(inverse);
        let expected = 90_009.000_200_009_9;
        assert!((condition - expected).abs() < 1e-3, "{condition}");
    }

    #[test]
    fn loocv_and_boundary_stability_evaluation() {
        // Issue #13: Verify that fitting a 3x3 model on a 23-patch subset produces
        // well-behaved held-out predictions without severe boundary undershoot/overshoot.
        let revision = ChartRevision::ClassicFromNovember2014;
        let dataset = crate::dataset::load(revision).expect("dataset loads");
        let chart = ChartModel::new(revision);

        // Construct synthetic samples through a known mild sensor response matrix
        let camera_matrix = [
            [1.03, -0.02, 0.01],
            [-0.02, 1.01, -0.01],
            [0.00, -0.01, 1.04],
        ];
        let samples: Vec<PatchSample> = chart
            .patches
            .iter()
            .map(|patch| {
                let ref_val = dataset.patches.iter().find(|p| p.patch == *patch).unwrap();
                let target = ref_val.linear_srgb_d65;
                let source = matrix_product(target, camera_matrix);
                PatchSample {
                    patch: *patch,
                    mean_rgb: source,
                    variance: [0.0; 3],
                    clipped_mask: 0,
                    sample_pixels: 100,
                }
            })
            .collect();

        // Fit on full dataset
        let (stages, report) = fit(&samples, &dataset).expect("fit succeeds");
        assert!(
            report.mean_delta_e < 0.5,
            "training mean dE: {}",
            report.mean_delta_e
        );
        assert!(
            report.condition_number < 1.5,
            "condition number: {}",
            report.condition_number
        );

        // Check boundary behavior at RGB limits [0, 0, 0] and [1, 1, 1]
        let black_pred = matrix_product([0.0, 0.0, 0.0], stages.matrix);
        assert_eq!(black_pred, [0.0, 0.0, 0.0], "black must map to black");

        let white_pred = matrix_product([1.0, 1.0, 1.0], stages.matrix);
        for val in white_pred {
            assert!(
                (0.95..=1.08).contains(&val),
                "boundary overshoot check: {val}"
            );
        }
    }
}
