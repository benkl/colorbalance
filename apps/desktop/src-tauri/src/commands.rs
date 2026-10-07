use std::fs;
use std::path::Path;

use colorbalance_core::calibration::{self, ChartQuad, GateConfig};
use colorbalance_core::chart::ChartRevision;
use colorbalance_core::decode::DecodedImage;
use colorbalance_core::interchange::{profile_to_clf, profile_to_cube};
use colorbalance_core::output::encode_tiff_rgb_u16;
use colorbalance_core::profile::{
    self, apply_transform, encode_srgb_u16, Profile, ValidationSummary,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Stage reporter for long commands: `(label, step, steps)`, called as each
/// stage starts. `step` is 1-based. The IPC layer turns these into events;
/// tests and other callers pass [`no_progress`].
pub type Report<'a> = &'a dyn Fn(&str, usize, usize);

/// Batch reporter: `(completed, total, file just started)`.
pub type BatchReport = std::sync::Arc<dyn Fn(usize, usize, Option<&Path>) + Send + Sync>;

/// A reporter that discards every stage.
pub fn no_progress(_label: &str, _step: usize, _steps: usize) {}

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
    preview_data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GateFailureResponse {
    patch: Option<String>,
    reason: String,
    measured: String,
}

/// Validation metrics as the UI consumes them (camelCase, includes the median).
///
/// This is deliberately separate from the profile's on-disk `ValidationSummary`,
/// whose kebab-case format is part of the `*.cbprofile.json` schema.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResponse {
    mean_delta_e: f64,
    median_delta_e: f64,
    p95_delta_e: f64,
    max_delta_e: f64,
    neutral_max_delta_e: f64,
    skin_max_delta_e: f64,
    condition_number: f64,
    patch_count: u32,
}

/// One chart patch, source vs. corrected vs. target, for the validation table.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchResponse {
    patch: String,
    source_rgb: [f64; 3],
    corrected_rgb: [f64; 3],
    target_rgb: [f64; 3],
    delta_e: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeriveResponse {
    profile_path: String,
    report_path: Option<String>,
    digest: String,
    quality_passed: bool,
    quality_override: bool,
    gate_failures: Vec<GateFailureResponse>,
    validation: ValidationResponse,
    patches: Vec<PatchResponse>,
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
    colorbalance_raw::decode_any(path).map_err(|error| BackendError::Message(error.to_string()))
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

/// Longest side, in pixels, of the preview sent to the UI.
const PREVIEW_MAX_DIM: u32 = 1600;

/// Everything the UI needs to show a freshly chosen reference image.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedReference {
    /// Upright width in pixels; chart coordinates use this space.
    pub image_width: u32,
    pub image_height: u32,
    /// Default chart quad (TL, TR, BR, BL) in full-resolution pixels.
    pub quad: [[f64; 2]; 4],
    /// `data:image/png;base64,...` preview that keeps the image aspect ratio.
    pub preview_data_url: String,
}

fn preview_data_url(image: &DecodedImage) -> Result<String, BackendError> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    let png = colorbalance_raw::render_preview_png(image, PREVIEW_MAX_DIM)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

/// Decode a reference image and return a displayable preview with its true
/// dimensions, so the light-table can show it before any calibration step.
pub fn load_reference(path: String, report: Report) -> Result<LoadedReference, BackendError> {
    report("Decoding image", 1, 2);
    let image = decode_auto(Path::new(&path))?;
    let quad = quad_from_payload(None, image.width, image.height);
    report("Rendering preview", 2, 2);
    Ok(LoadedReference {
        image_width: image.width,
        image_height: image.height,
        quad: quad.corners,
        preview_data_url: preview_data_url(&image)?,
    })
}

pub fn inspect_reference(
    path: String,
    chart_revision: String,
    quad: Option<QuadPayload>,
    report: Report,
) -> Result<InspectResponse, BackendError> {
    report("Decoding image", 1, 4);
    let image = decode_auto(Path::new(&path))?;
    let revision = parse_revision(&chart_revision)?;
    let chart_quad = quad_from_payload(quad, image.width, image.height);
    report("Sampling chart patches", 2, 4);
    let samples = calibration::sample_patches(&image, &chart_quad)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    report("Checking quality gates", 3, 4);
    let gate_config = gate_config_for(&image);
    let failures = calibration::evaluate_quality(&samples, &gate_config)
        .err()
        .unwrap_or_default();
    report("Rendering preview", 4, 4);
    let preview = Some(preview_data_url(&image)?);
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
        preview_data_url: preview,
    })
}

/// True when the source is a rendered JPEG/PNG rather than RAW.
fn is_rendered_source(image: &DecodedImage) -> bool {
    image.camera.decoder == colorbalance_raw::JPEG_DECODER_NAME
}

/// Rendered sources get the relaxed gate set; RAW never does.
fn gate_config_for(image: &DecodedImage) -> GateConfig {
    if is_rendered_source(image) {
        GateConfig::quick_and_dirty()
    } else {
        GateConfig::default()
    }
}

/// Short, human-readable warnings for the gate failures, one line per cause.
///
/// Counts patches rather than individual checks so a chart with several failed
/// channels does not read as dozens of problems.
fn gate_warnings(failures: &[calibration::GateFailure], rendered: bool) -> Vec<String> {
    let distinct = |matches: &dyn Fn(&calibration::GateFailure) -> bool| {
        let mut seen: Vec<String> = Vec::new();
        for failure in failures.iter().filter(|f| matches(f)) {
            let label = failure
                .patch
                .map_or_else(|| "chart".to_owned(), |patch| format!("{patch:?}"));
            if !seen.contains(&label) {
                seen.push(label);
            }
        }
        seen.len()
    };
    let clipped = distinct(&|f| f.reason.starts_with("clipped"));
    let noisy = distinct(&|f| f.reason.contains("coefficient of variation"));
    let layout = failures
        .iter()
        .any(|f| f.reason.starts_with("last row is not the neutral row"));
    let other = distinct(&|f| {
        !f.reason.starts_with("clipped")
            && !f.reason.contains("coefficient of variation")
            && !f.reason.starts_with("last row is not the neutral row")
    });

    let mut warnings = Vec::new();
    if rendered {
        warnings.push("Rendered JPEG/PNG source: approximate, not RAW.".to_owned());
    }
    if clipped > 0 {
        warnings.push(format!(
            "{clipped} clipped patch(es): re-shoot at lower exposure."
        ));
    }
    if noisy > 0 {
        warnings.push(format!(
            "{noisy} noisy patch(es): glare or corners off the patches."
        ));
    }
    if layout {
        warnings.push("Chart looks rotated: check the corner order.".to_owned());
    }
    if other > 0 {
        warnings.push(format!("{other} other check(s) failed."));
    }
    warnings
}

pub fn derive_profile(
    path: String,
    chart_revision: String,
    profile_path: String,
    report_path: Option<String>,
    quad: Option<QuadPayload>,
    report: Report,
) -> Result<DeriveResponse, BackendError> {
    let steps = if report_path.is_some() { 5 } else { 4 };
    report("Decoding image", 1, steps);
    let image = decode_auto(Path::new(&path))?;
    let revision = parse_revision(&chart_revision)?;
    let chart_quad = quad_from_payload(quad, image.width, image.height);
    let dataset = colorbalance_core::dataset::load(revision)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    report("Sampling chart patches", 2, steps);
    let samples = calibration::sample_patches(&image, &chart_quad)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    let rendered = is_rendered_source(&image);
    let gate_config = gate_config_for(&image);
    let failures = calibration::evaluate_quality(&samples, &gate_config)
        .err()
        .unwrap_or_default();

    report("Fitting the 3×3 transform", 3, steps);
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
        quality: Some(profile::QualityProvenance::from_gates(
            &failures, true, rendered,
        )),
        digest: String::new(),
    };
    let mut profile_value: Profile = serde_json::from_str(&profile::to_json(&initial))
        .map_err(|error| BackendError::Message(error.to_string()))?;
    profile_value.digest = profile::digest(&profile_value);
    report("Writing profile", 4, steps);
    fs::write(&profile_path, profile::to_json(&profile_value))?;

    if let Some(report_file) = &report_path {
        report("Writing report", 5, steps);
        fs::write(
            report_file,
            build_report_html(
                &profile_value,
                &chart_quad,
                &failures,
                &validation.per_patch,
                rendered,
            ),
        )?;
    }

    let warnings = gate_warnings(&failures, rendered);
    let validation_response = ValidationResponse {
        mean_delta_e: validation.mean_delta_e,
        median_delta_e: validation.median_delta_e,
        p95_delta_e: validation.p95_delta_e,
        max_delta_e: validation.max_delta_e,
        neutral_max_delta_e: validation.neutral_max_delta_e,
        skin_max_delta_e: validation.skin_max_delta_e,
        condition_number: validation.condition_number,
        patch_count: profile_value.validation.patch_count,
    };
    let patches = validation
        .per_patch
        .iter()
        .map(|row| PatchResponse {
            patch: format!("{:?}", row.patch),
            source_rgb: row.source_rgb,
            corrected_rgb: row.corrected_rgb,
            target_rgb: row.target_rgb,
            delta_e: row.delta_e,
        })
        .collect();
    Ok(DeriveResponse {
        profile_path,
        report_path,
        digest: profile_value.digest,
        quality_passed: failures.is_empty() && !rendered,
        quality_override: !failures.is_empty(),
        gate_failures: failures
            .iter()
            .map(|failure| GateFailureResponse {
                patch: failure.patch.map(|patch| format!("{patch:?}")),
                reason: failure.reason.clone(),
                measured: failure.measured.clone(),
            })
            .collect(),
        validation: validation_response,
        patches,
        warnings,
    })
}

/// Run a batch. `on_progress(completed, total, file)` fires once at the start
/// (`completed == 0`, no file), when each file starts (`file` set), and when
/// each file finishes (`completed` counts finished files, including skipped and
/// failed ones). Workers run concurrently, so `file` is the one that just began.
pub fn apply_batch(
    profile_path: String,
    input_path: String,
    output_path: String,
    overwrite: bool,
    cancellation: colorbalance_core::CancelFlag,
    on_progress: BatchReport,
) -> Result<BatchResponse, BackendError> {
    cancellation.store(false, std::sync::atomic::Ordering::Relaxed);
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
            if name.starts_with(".tmp-") && name.ends_with(".tiff") || name.ends_with(".tiff.tmp") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let total = inputs.len();
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    on_progress(0, total, None);
    let started = {
        let on_progress = on_progress.clone();
        let finished = finished.clone();
        std::sync::Arc::new(move |input: &std::path::Path, _index: usize, total: usize| {
            on_progress(
                finished.load(std::sync::atomic::Ordering::Relaxed),
                total,
                Some(input),
            );
        })
    };
    let process = {
        let on_progress = on_progress.clone();
        let finished = finished.clone();
        move |input: &std::path::Path, output: std::path::PathBuf| {
            let result = (|| {
                if output.exists() && !overwrite {
                    return Ok(None);
                }
                let mut image = decode_auto(input).map_err(|e| e.to_string())?;
                check_camera(&profile, &image)?;
                let (pixels, _) = correct_in_place(&profile, &mut image);
                let data = encode_tiff_rgb_u16(image.width, image.height, &pixels);
                write_atomically(&output, &data).map_err(|e| e.to_string())?;
                Ok(Some(output))
            })();
            let done = finished.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            on_progress(done, total, None);
            result
        }
    };
    let summary =
        colorbalance_core::run_batch(inputs, &options, Some(started), cancellation, process)
            .map_err(BackendError::Message)?;
    Ok(BatchResponse {
        succeeded: summary.succeeded.into_iter().map(|r| r.input).collect(),
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

/// Fail closed when the image was not captured by the profile's camera.
fn check_camera(profile: &Profile, image: &DecodedImage) -> Result<(), String> {
    if image.camera.make != profile.camera.make || image.camera.model != profile.camera.model {
        return Err(format!(
            "camera mismatch (profile: {} {}, image: {} {})",
            profile.camera.make, profile.camera.model, image.camera.make, image.camera.model
        ));
    }
    Ok(())
}

/// Apply the profile to every pixel and return the sRGB-encoded 16-bit samples
/// plus the fraction of pixels the transform pushed out of gamut.
///
/// `image.rgb` is overwritten with the corrected linear sRGB values so the same
/// buffer can feed the preview renderer; no second full-size copy is made.
fn correct_in_place(profile: &Profile, image: &mut DecodedImage) -> (Vec<u16>, f64) {
    let mut pixels = Vec::with_capacity(image.rgb.len());
    let mut out_of_gamut = 0usize;
    for rgb in image.rgb.as_chunks_mut::<3>().0 {
        let (corrected, flags) = apply_transform(
            profile,
            [f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])],
        );
        if flags != 0 {
            out_of_gamut += 1;
        }
        for (slot, value) in rgb.iter_mut().zip(corrected) {
            *slot = value as f32;
        }
        let encoded = [
            colorbalance_core::color::srgb_encode(corrected[0]),
            colorbalance_core::color::srgb_encode(corrected[1]),
            colorbalance_core::color::srgb_encode(corrected[2]),
        ];
        pixels.extend_from_slice(&encode_srgb_u16(encoded));
    }
    // The buffer is corrected sRGB now; the camera neutral no longer applies.
    image.display_neutral = None;
    let total = (image.rgb.len() / 3).max(1);
    (pixels, out_of_gamut as f64 / total as f64)
}

/// Write `data` next to `output` through a unique temporary file, flush it to
/// disk, then rename it into place. The temporary file is removed on failure.
fn write_atomically(output: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let directory = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = directory.join(format!(
        ".tmp-{}-{}.tiff",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, output)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectResponse {
    /// Downscaled PNG of the image as decoded, for the "before" half.
    before_data_url: String,
    /// Downscaled PNG of the corrected image, for the "after" half.
    after_data_url: String,
    /// Where the corrected TIFF was written, when an output path was given.
    output_path: Option<String>,
    warnings: Vec<String>,
}

/// Correct one image with a saved profile.
///
/// Always returns before/after previews. When `output_path` is set the full
/// resolution 16-bit TIFF is also written atomically; an existing file is an
/// error unless `overwrite` is set. Camera mismatch fails closed.
pub fn correct_image(
    profile_path: String,
    input_path: String,
    output_path: Option<String>,
    overwrite: bool,
    report: Report,
) -> Result<CorrectResponse, BackendError> {
    let steps = if output_path.is_some() { 5 } else { 4 };
    let profile = profile::from_json(&fs::read_to_string(&profile_path)?)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    if let Some(output) = &output_path {
        if Path::new(output).exists() && !overwrite {
            return Err(BackendError::Message(format!(
                "output already exists: {output}"
            )));
        }
    }
    report("Decoding image", 1, steps);
    let mut image = decode_auto(Path::new(&input_path))?;
    check_camera(&profile, &image).map_err(BackendError::Message)?;
    report("Rendering original preview", 2, steps);
    let before_data_url = preview_data_url(&image)?;
    report("Applying the transform", 3, steps);
    let (pixels, out_of_gamut) = correct_in_place(&profile, &mut image);
    report("Rendering corrected preview", 4, steps);
    let after_data_url = preview_data_url(&image)?;
    if let Some(output) = &output_path {
        report("Writing 16-bit TIFF", 5, steps);
        let data = encode_tiff_rgb_u16(image.width, image.height, &pixels);
        write_atomically(Path::new(output), &data)?;
    }

    let mut warnings = Vec::new();
    if let Some(quality) = &profile.quality {
        if quality.quick_and_dirty {
            warnings.push("Profile came from a rendered image: approximate.".to_owned());
        }
        if !quality.failures.is_empty() {
            warnings.push("Profile was derived despite failed quality checks.".to_owned());
        }
    }
    if out_of_gamut > 0.01 {
        warnings.push(format!(
            "{:.1}% of pixels fall outside sRGB and were clipped.",
            out_of_gamut * 100.0
        ));
    }
    Ok(CorrectResponse {
        before_data_url,
        after_data_url,
        output_path,
        warnings,
    })
}

/// Write the profile as CLF or `.cube`.
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

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn build_report_html(
    profile: &Profile,
    quad: &ChartQuad,
    failures: &[colorbalance_core::calibration::GateFailure],
    per_patch: &[calibration::PatchValidation],
    rendered: bool,
) -> String {
    let status = if failures.is_empty() && !rendered {
        "QUALITY GATES PASS"
    } else if failures.is_empty() {
        "RENDERED SOURCE: approximate, sRGB inversion is not RAW"
    } else if rendered {
        "RENDERED SOURCE: approximate, with quality warnings"
    } else {
        "QUALITY WARNINGS: profile derived despite failed gates"
    };
    let mut html = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>ColorBalance Report</title><h1>{status}</h1><p>Profile digest: {}</p><p>Mean ΔE2000: {:.3} | Max: {:.3}</p><p>Quad: {:?}</p>",
        profile.digest, profile.validation.mean_delta_e, profile.validation.max_delta_e, quad.corners
    );
    if !failures.is_empty() {
        html.push_str(&format!(
            "<h2>Quality gate failures ({})</h2><table border=\"1\" cellpadding=\"4\"><tr><th>Patch</th><th>Reason</th><th>Measured</th></tr>",
            failures.len()
        ));
        for failure in failures {
            let patch = failure
                .patch
                .map_or_else(|| "chart".to_owned(), |patch| format!("{patch:?}"));
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&patch),
                escape_html(&failure.reason),
                escape_html(&failure.measured)
            ));
        }
        html.push_str("</table>");
    }
    let mut ranked: Vec<&calibration::PatchValidation> = per_patch.iter().collect();
    ranked.sort_by(|a, b| b.delta_e.total_cmp(&a.delta_e));
    html.push_str(
        "<h2>Patches by fit error</h2><table border=\"1\" cellpadding=\"4\"><tr><th>Patch</th><th>ΔE2000</th></tr>",
    );
    for row in ranked {
        html.push_str(&format!(
            "<tr><td>{:?}</td><td>{:.3}</td></tr>",
            row.patch, row.delta_e
        ));
    }
    html.push_str("</table>");
    html
}
