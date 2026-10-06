use std::fs;
use std::path::Path;

use colorbalance_core::calibration::{self, ChartQuad, GateConfig};
use colorbalance_core::chart::ChartRevision;
use colorbalance_core::decode::{DecodedImage, RawDecoder};
use colorbalance_core::interchange::{profile_to_clf, profile_to_cube};
use colorbalance_core::output::encode_tiff_rgb_u16;
use colorbalance_core::profile::{
    self, apply_transform, encode_srgb_u16, Profile, ValidationSummary,
};
use colorbalance_raw::dng::DngDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Emitter, State, Window};

use crate::AppState;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuadPayload {
    corners: [[f64; 2]; 4],
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectResponse {
    camera: colorbalance_core::CameraIdentity,
    image_width: u32,
    image_height: u32,
    chart_revision: ChartRevision,
    quality_passed: bool,
    gate_failures: Vec<GateFailureResponse>,
    quad: [[f64; 2]; 4],
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GateFailureResponse {
    patch: Option<String>,
    reason: String,
    measured: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeriveResponse {
    profile_path: String,
    report_path: Option<String>,
    digest: String,
    quality_passed: bool,
    validation: ValidationSummary,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResponse {
    succeeded: Vec<String>,
    skipped: Vec<String>,
    failed: Vec<BatchFailure>,
    total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchFailure {
    file: String,
    error: String,
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("{0}")]
    Message(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl serde::Serialize for BackendError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

fn parse_revision(value: &str) -> Result<ChartRevision, BackendError> {
    match value {
        "classic-before-nov-2014" => Ok(ChartRevision::ClassicBeforeNovember2014),
        "classic-from-nov-2014" => Ok(ChartRevision::ClassicFromNovember2014),
        other => Err(BackendError::Message(format!(
            "unsupported chart revision: {other}"
        ))),
    }
}

fn decode_auto(path: &Path) -> Result<DecodedImage, BackendError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "jpg" | "jpeg" | "png") {
        colorbalance_raw::decode_rendered_image(path)
            .map_err(|error| BackendError::Message(error.to_string()))
    } else {
        DngDecoder
            .decode_path(path)
            .map_err(|error| BackendError::Message(error.to_string()))
    }
}

fn quad_from_payload(payload: Option<QuadPayload>, width: u32, height: u32) -> ChartQuad {
    payload
        .map(|value| ChartQuad {
            corners: value.corners,
        })
        .unwrap_or_else(|| {
            let w = f64::from(width);
            let h = f64::from(height);
            let mx = w * 0.08;
            let my = h * 0.08;
            ChartQuad {
                corners: [[mx, my], [w - mx, my], [w - mx, h - my], [mx, h - my]],
            }
        })
}

#[tauri::command]
pub fn inspect_reference(
    path: String,
    chart_revision: String,
    quad: Option<QuadPayload>,
    quick_and_dirty: bool,
) -> Result<InspectResponse, BackendError> {
    let image = decode_auto(Path::new(&path))?;
    let revision = parse_revision(&chart_revision)?;
    let chart_quad = quad_from_payload(quad, image.width, image.height);
    let samples = calibration::sample_patches(&image, &chart_quad)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    let gate_config = if quick_and_dirty {
        GateConfig::quick_and_dirty()
    } else {
        GateConfig::default()
    };
    let failures = calibration::evaluate_quality(&samples, &gate_config)
        .err()
        .unwrap_or_default();
    Ok(InspectResponse {
        camera: image.camera,
        image_width: image.width,
        image_height: image.height,
        chart_revision: revision,
        quality_passed: failures.is_empty(),
        gate_failures: failures
            .into_iter()
            .map(|failure| GateFailureResponse {
                patch: failure.patch.map(|patch| format!("{patch:?}")),
                reason: failure.reason,
                measured: failure.measured,
            })
            .collect(),
        quad: chart_quad.corners,
    })
}

#[tauri::command]
pub fn derive_profile(
    path: String,
    chart_revision: String,
    profile_path: String,
    report_path: Option<String>,
    quad: Option<QuadPayload>,
    quick_and_dirty: bool,
    force: bool,
) -> Result<DeriveResponse, BackendError> {
    let image = decode_auto(Path::new(&path))?;
    let revision = parse_revision(&chart_revision)?;
    let chart_quad = quad_from_payload(quad, image.width, image.height);
    let dataset = colorbalance_core::dataset::load(revision)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    let samples = calibration::sample_patches(&image, &chart_quad)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    let gate_config = if quick_and_dirty {
        GateConfig::quick_and_dirty()
    } else {
        GateConfig::default()
    };
    let failures = calibration::evaluate_quality(&samples, &gate_config)
        .err()
        .unwrap_or_default();
    if !failures.is_empty() && !force && !quick_and_dirty {
        return Err(BackendError::Message(format!(
            "quality gates failed: {}",
            failures
                .iter()
                .map(|failure| failure.reason.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    let (stages, validation) = calibration::fit(&samples, &dataset)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    let reference_bytes = fs::read(&path)?;
    let mut reference_hasher = Sha256::new();
    reference_hasher.update(reference_bytes);
    let reference_digest = format!("{:x}", reference_hasher.finalize());
    let contract = colorbalance_core::contract::DecodeContract::canonical(
        &image.camera.decoder,
        &image.camera.decoder_version,
    );
    let initial = Profile {
        schema_version: profile::SCHEMA_VERSION.to_owned(),
        decode_contract: contract,
        camera: image.camera,
        chart_revision: revision,
        dataset_digest: colorbalance_core::dataset::dataset_digest(),
        reference_digest,
        transform: stages,
        validation: ValidationSummary {
            mean_delta_e: validation.mean_delta_e,
            max_delta_e: validation.max_delta_e,
            p95_delta_e: validation.p95_delta_e,
            neutral_max_delta_e: validation.neutral_max_delta_e,
            skin_max_delta_e: validation.skin_max_delta_e,
            condition_number: validation.condition_number,
            patch_count: validation.per_patch.len() as u32,
        },
        digest: String::new(),
    };
    let mut profile_value: Profile = serde_json::from_str(&profile::to_json(&initial))
        .map_err(|error| BackendError::Message(error.to_string()))?;
    profile_value.digest = profile::digest(&profile_value);
    fs::write(&profile_path, profile::to_json(&profile_value))?;

    if let Some(report) = &report_path {
        fs::write(
            report,
            build_report_html(&profile_value, &chart_quad, &failures, quick_and_dirty),
        )?;
    }

    let mut warnings = failures
        .iter()
        .map(|failure| format!("{}: {}", failure.reason, failure.measured))
        .collect::<Vec<_>>();
    if quick_and_dirty {
        warnings.insert(
            0,
            "Quick-and-dirty approximation: source was rendered JPEG/PNG, not RAW.".to_owned(),
        );
    }
    Ok(DeriveResponse {
        profile_path,
        report_path,
        digest: profile_value.digest,
        quality_passed: warnings.is_empty(),
        validation: profile_value.validation,
        warnings,
    })
}

#[tauri::command]
pub fn apply_batch(
    profile_path: String,
    input_path: String,
    output_path: String,
    overwrite: bool,
    force: bool,
    state: State<'_, AppState>,
    window: Window,
) -> Result<BatchResponse, BackendError> {
    state
        .cancellation
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let profile_text = fs::read_to_string(&profile_path)?;
    let profile = std::sync::Arc::new(
        profile::from_json(&profile_text)
            .map_err(|error| BackendError::Message(error.to_string()))?,
    );
    let input = Path::new(&input_path);
    let options = colorbalance_core::BatchOptions {
        output: Path::new(&output_path).to_path_buf(),
        overwrite,
        workers: colorbalance_core::DEFAULT_WORKERS,
        extensions: colorbalance_core::DEFAULT_EXTENSIONS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    let inputs = colorbalance_core::collect_inputs(input, &options.extensions)
        .map_err(BackendError::Message)?;
    if let Ok(entries) = fs::read_dir(&options.output) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".tmp-") && name.ends_with(".tiff")
                || name.ends_with(".tiff.tmp")
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let result_total = inputs.len();
    let window_clone = window.clone();
    let progress = std::sync::Arc::new(move |input: &std::path::Path, index: usize, total: usize| {
        let _ = window_clone.emit(
            "batch-progress",
            serde_json::json!({
                "completed": index,
                "total": total,
                "file": input.display().to_string()
            }),
        );
    });
    let process = move |input: &std::path::Path, output: std::path::PathBuf| {
        if output.exists() && !overwrite {
            return Ok(None);
        }
        let image = decode_auto(input).map_err(|e| e.to_string())?;
        if (image.camera.make != profile.camera.make || image.camera.model != profile.camera.model)
            && !force
        {
            return Err(format!(
                "camera mismatch: {} {}",
                image.camera.make, image.camera.model
            ));
        }
        let mut pixels = Vec::with_capacity(image.rgb.len());
        for rgb in image.rgb.as_chunks::<3>().0 {
            let (corrected, _) = apply_transform(
                &profile,
                [f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])],
            );
            let encoded = [
                colorbalance_core::color::srgb_encode(corrected[0]),
                colorbalance_core::color::srgb_encode(corrected[1]),
                colorbalance_core::color::srgb_encode(corrected[2]),
            ];
            pixels.extend_from_slice(&encode_srgb_u16(encoded));
        }
        let data = encode_tiff_rgb_u16(image.width, image.height, &pixels);
        let temporary = output.with_extension("tiff.tmp");
        fs::write(&temporary, data).map_err(|e| e.to_string())?;
        fs::rename(&temporary, &output).map_err(|e| e.to_string())?;
        Ok(Some(output))
    };
    let summary = colorbalance_core::run_batch(
        inputs,
        &options,
        Some(progress),
        state.cancellation.clone(),
        process,
    )
    .map_err(BackendError::Message)?;
    let _ = window.emit(
        "batch-progress",
        serde_json::json!({
            "completed": result_total,
            "total": result_total,
            "file": null
        }),
    );
    Ok(BatchResponse {
        succeeded: summary
            .succeeded
            .into_iter()
            .map(|r| r.input)
            .collect(),
        skipped: summary.skipped,
        failed: summary
            .failed
            .into_iter()
            .map(|r| BatchFailure {
                file: r.input,
                error: r.message.unwrap_or_default(),
            })
            .collect(),
        total: summary.total,
    })
}

#[tauri::command]
pub fn cancel_batch(state: State<'_, AppState>) -> Result<(), BackendError> {
    state
        .cancellation
        .store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn export_profile(
    profile_path: String,
    format: String,
    output_path: String,
    size: Option<usize>,
) -> Result<String, BackendError> {
    let text = fs::read_to_string(&profile_path)?;
    let profile =
        profile::from_json(&text).map_err(|error| BackendError::Message(error.to_string()))?;
    let content = match format.as_str() {
        "clf" => profile_to_clf(&profile),
        "cube" => profile_to_cube(&profile, size.unwrap_or(33)),
        other => {
            return Err(BackendError::Message(format!(
                "unsupported export format: {other}"
            )))
        }
    };
    fs::write(&output_path, content)?;
    Ok(output_path)
}

fn build_report_html(
    profile: &Profile,
    quad: &ChartQuad,
    failures: &[colorbalance_core::calibration::GateFailure],
    quick_and_dirty: bool,
) -> String {
    let warning = if quick_and_dirty {
        "QUICK & DIRTY APPROXIMATION: rendered source; sRGB inversion is approximate."
    } else if failures.is_empty() {
        "QUALITY GATES PASS"
    } else {
        "QUALITY GATES OVERRIDDEN"
    };
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>ColorBalance Report</title><h1>{warning}</h1><p>Profile digest: {}</p><p>Mean ΔE2000: {:.3} | Max: {:.3}</p><p>Quad: {:?}</p>",
        profile.digest, profile.validation.mean_delta_e, profile.validation.max_delta_e, quad.corners
    )
}
