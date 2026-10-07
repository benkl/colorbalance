use std::fs;
use std::path::Path;

use colorbalance_core::calibration::{self, ChartQuad, GateConfig};
use colorbalance_core::chart::ChartRevision;
use colorbalance_core::contract::DecodeContract;
use colorbalance_core::decode::DecodedImage;
use colorbalance_core::detection::{detect_chart, Detection};
use colorbalance_core::interchange::{profile_to_clf, profile_to_cube};
use colorbalance_core::output::encode_tiff_rgb_u16;
use colorbalance_core::profile::{
    self, apply_transform, encode_srgb_u16, Profile, ValidationSummary,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::reference_cache::ReferenceCache;

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
    warnings: Vec<BatchWarning>,
    failed: Vec<BatchFailure>,
    total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchFailure {
    file: String,
    error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchWarning {
    file: String,
    warning: String,
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

/// Chart geometry only; the physical chart revision must be selected by the user.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum DetectResponse {
    Found { quad: [[f64; 2]; 4] },
    Missing,
    Ambiguous,
}

fn encode_png_data_url(png: &[u8]) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    format!("data:image/png;base64,{}", STANDARD.encode(png))
}

fn preview_data_url(image: &DecodedImage) -> Result<String, BackendError> {
    let png = colorbalance_raw::render_preview_png(image, PREVIEW_MAX_DIM)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    Ok(encode_png_data_url(&png))
}

/// Decode a reference image and return a displayable preview with its true
/// dimensions, so the light-table can show it before any calibration step.
pub fn load_reference(path: String, report: Report) -> Result<LoadedReference, BackendError> {
    load_reference_cached(&ReferenceCache::disabled(), path, report)
}

/// Decode `path` through `cache`, reporting step 1 as a decode only when one
/// actually runs.
fn decode_cached(
    cache: &ReferenceCache,
    path: &str,
    report: Report,
    steps: usize,
) -> Result<std::sync::Arc<DecodedImage>, BackendError> {
    let cached = cache.get_or_decode(
        Path::new(path),
        || report("Decoding image", 1, steps),
        decode_auto,
    )?;
    if cached.hit {
        report("Using cached image", 1, steps);
    }
    Ok(cached.image)
}

/// Render the preview for `image` through `cache`, reporting the stage as a
/// render or a cache hit.
fn preview_cached(
    cache: &ReferenceCache,
    image: &std::sync::Arc<DecodedImage>,
    report: Report,
    step: usize,
    steps: usize,
) -> Result<String, BackendError> {
    let (preview, hit) = cache.preview(image, |image| {
        report("Rendering preview", step, steps);
        preview_data_url(image)
    })?;
    if hit {
        report("Using cached preview", step, steps);
    }
    Ok(preview)
}

/// [`load_reference`] that reuses the cache's decode and preview.
pub fn load_reference_cached(
    cache: &ReferenceCache,
    path: String,
    report: Report,
) -> Result<LoadedReference, BackendError> {
    let image = decode_cached(cache, &path, report, 2)?;
    let quad = quad_from_payload(None, image.width, image.height);
    Ok(LoadedReference {
        image_width: image.width,
        image_height: image.height,
        quad: quad.corners,
        preview_data_url: preview_cached(cache, &image, report, 2, 2)?,
    })
}

/// Detect on the cached decode without rendering or returning another preview.
pub fn detect_chart_cached(
    cache: &ReferenceCache,
    path: String,
) -> Result<DetectResponse, BackendError> {
    let image = decode_cached(cache, &path, &no_progress, 1)?;
    Ok(match detect_chart(&image) {
        Detection::Found(quad) => DetectResponse::Found { quad: quad.corners },
        Detection::Missing => DetectResponse::Missing,
        Detection::Ambiguous => DetectResponse::Ambiguous,
    })
}

pub fn inspect_reference(
    path: String,
    chart_revision: String,
    quad: Option<QuadPayload>,
    report: Report,
) -> Result<InspectResponse, BackendError> {
    inspect_reference_cached(
        &ReferenceCache::disabled(),
        path,
        chart_revision,
        quad,
        report,
    )
}

/// [`inspect_reference`] that reuses the cache's decode and preview.
pub fn inspect_reference_cached(
    cache: &ReferenceCache,
    path: String,
    chart_revision: String,
    quad: Option<QuadPayload>,
    report: Report,
) -> Result<InspectResponse, BackendError> {
    let image = decode_cached(cache, &path, report, 4)?;
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
    let preview = Some(preview_cached(cache, &image, report, 4, 4)?);
    Ok(InspectResponse {
        camera: image.camera.clone(),
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
    derive_profile_cached(
        &ReferenceCache::disabled(),
        path,
        chart_revision,
        profile_path,
        report_path,
        quad,
        report,
    )
}

/// [`derive_profile`] that reuses the cache's decode of the reference.
pub fn derive_profile_cached(
    cache: &ReferenceCache,
    path: String,
    chart_revision: String,
    profile_path: String,
    report_path: Option<String>,
    quad: Option<QuadPayload>,
    report: Report,
) -> Result<DeriveResponse, BackendError> {
    let steps = if report_path.is_some() { 5 } else { 4 };
    let image = decode_cached(cache, &path, report, steps)?;
    let image = &*image;
    let revision = parse_revision(&chart_revision)?;
    let chart_quad = quad_from_payload(quad, image.width, image.height);
    let dataset = colorbalance_core::dataset::load(revision)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    report("Sampling chart patches", 2, steps);
    let samples = calibration::sample_patches(image, &chart_quad)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    let rendered = is_rendered_source(image);
    let gate_config = gate_config_for(image);
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
    let contract = if image.sensor_layout == colorbalance_core::decode::SensorLayout::Rendered {
        DecodeContract::canonical(&image.camera.decoder, &image.camera.decoder_version)
    } else {
        let contract = colorbalance_raw::canonical_contract();
        if contract.decoder != image.camera.decoder
            || contract.decoder_version != image.camera.decoder_version
        {
            return Err(BackendError::Message(
                "RAW decoder identity disagrees with its contract".to_owned(),
            ));
        }
        contract
    };
    let initial = Profile {
        schema_version: profile::SCHEMA_VERSION.to_owned(),
        decode_contract: contract,
        camera: image.camera.clone(),
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
        std::sync::Arc::new(
            move |input: &std::path::Path, _index: usize, total: usize| {
                on_progress(
                    finished.load(std::sync::atomic::Ordering::Relaxed),
                    total,
                    Some(input),
                );
            },
        )
    };
    let (warning_tx, warning_rx) = std::sync::mpsc::channel::<BatchWarning>();
    let process = {
        let on_progress = on_progress.clone();
        let finished = finished.clone();
        let warning_tx = warning_tx.clone();
        move |input: &std::path::Path, output: std::path::PathBuf| {
            let result = (|| {
                if output.exists() && !overwrite {
                    return Ok(None);
                }
                let mut image = decode_auto(input).map_err(|e| e.to_string())?;
                let warning = check_camera(&profile, &image)?;
                let (pixels, _) = correct_in_place(&profile, &mut image);
                let data = encode_tiff_rgb_u16(image.width, image.height, &pixels);
                write_atomically(&output, &data).map_err(|e| e.to_string())?;
                if let Some(warning) = warning {
                    let _ = warning_tx.send(BatchWarning {
                        file: input.display().to_string(),
                        warning,
                    });
                }
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
        warnings: {
            drop(warning_tx);
            let mut warnings: Vec<_> = warning_rx.into_iter().collect();
            warnings.sort_by(|a, b| a.file.cmp(&b.file));
            warnings
        },
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

/// Fail closed on camera, decoder or non-version decode-contract mismatch.
fn check_camera(profile: &Profile, image: &DecodedImage) -> Result<Option<String>, String> {
    if image.camera.make != profile.camera.make || image.camera.model != profile.camera.model {
        return Err(format!(
            "camera mismatch (profile: {} {}, image: {} {})",
            profile.camera.make, profile.camera.model, image.camera.make, image.camera.model
        ));
    }
    if profile.camera.decoder != profile.decode_contract.decoder
        || profile.camera.decoder_version != profile.decode_contract.decoder_version
    {
        return Err(
            "profile camera decoder identity disagrees with its decode contract".to_owned(),
        );
    }
    let actual =
        if image.sensor_layout == colorbalance_core::decode::SensorLayout::Rendered {
            DecodeContract::canonical(&image.camera.decoder, &image.camera.decoder_version)
        } else {
            let contract = colorbalance_raw::canonical_contract();
            if contract.decoder != image.camera.decoder
                || contract.decoder_version != image.camera.decoder_version
            {
                return Err(format!(
                "RAW decoder identity disagrees with its contract (image: {} {}, contract: {} {})",
                image.camera.decoder, image.camera.decoder_version,
                contract.decoder, contract.decoder_version
            ));
            }
            contract
        };
    profile
        .decode_contract
        .compare_for_apply(&actual)
        .map_err(|e| e.to_string())
}

/// Apply the profile to every pixel and return the sRGB-encoded 16-bit samples
/// plus the fraction of pixels the transform pushed out of gamut.
///
/// `image.rgb` is overwritten with the corrected linear sRGB values so the same
/// buffer can feed the preview renderer; no second full-size copy is made.
fn correct_in_place(profile: &Profile, image: &mut DecodedImage) -> (Vec<u16>, f64) {
    let result = correct_buffer(profile, &mut image.rgb);
    // The buffer is corrected sRGB now; the camera neutral no longer applies.
    image.display_neutral = None;
    result
}

/// [`correct_in_place`] on a bare linear RGB buffer.
fn correct_buffer(profile: &Profile, rgb_buffer: &mut [f32]) -> (Vec<u16>, f64) {
    let mut pixels = Vec::with_capacity(rgb_buffer.len());
    let mut out_of_gamut = 0usize;
    for rgb in rgb_buffer.as_chunks_mut::<3>().0 {
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
    let total = (rgb_buffer.len() / 3).max(1);
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
    correct_image_cached(
        &ReferenceCache::disabled(),
        profile_path,
        input_path,
        output_path,
        overwrite,
        report,
    )
}

/// [`correct_image`] that reuses the cache's decode and "before" preview when
/// `input_path` is the cached reference. The cached image is shared and
/// immutable: the transform runs on a copy of its pixels (or on the pixels
/// themselves when nothing else holds the image, as with a disabled cache).
pub fn correct_image_cached(
    cache: &ReferenceCache,
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
    let image = decode_cached(cache, &input_path, report, steps)?;
    let version_warning = check_camera(&profile, &image).map_err(BackendError::Message)?;
    let before_data_url = preview_cached(cache, &image, report, 2, steps)?;
    report("Applying the transform", 3, steps);
    let (width, height) = (image.width, image.height);
    let mut rgb = match std::sync::Arc::try_unwrap(image) {
        Ok(owned) => owned.rgb,
        Err(shared) => shared.rgb.clone(),
    };
    let (pixels, out_of_gamut) = correct_buffer(&profile, &mut rgb);
    report("Rendering corrected preview", 4, steps);
    let after_data_url =
        colorbalance_raw::render_preview_rgb(&rgb, width, height, None, PREVIEW_MAX_DIM)
            .map(|png| encode_png_data_url(&png))
            .map_err(|error| BackendError::Message(error.to_string()))?;
    drop(rgb);
    if let Some(output) = &output_path {
        report("Writing 16-bit TIFF", 5, steps);
        let data = encode_tiff_rgb_u16(width, height, &pixels);
        write_atomically(Path::new(output), &data)?;
    }

    let mut warnings = Vec::new();
    if let Some(warning) = version_warning {
        warnings.push(warning);
    }
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

#[cfg(test)]
mod apply_contract_tests {
    use super::*;

    #[test]
    fn apply_rejects_decoder_drift_and_reports_version_drift() {
        let mut profile =
            profile::from_json(include_str!("../colorbalance_profile.cbprofile.json"))
                .expect("valid fixture profile");
        let actual = colorbalance_raw::canonical_contract();
        profile.decode_contract = actual.clone();
        profile.camera.decoder = actual.decoder.clone();
        profile.camera.decoder_version = actual.decoder_version.clone();
        let mut image = DecodedImage {
            sensor_layout: colorbalance_core::decode::SensorLayout::Cfa,
            width: 1,
            height: 1,
            rgb: vec![0.0; 3],
            clipped: vec![0],
            black_levels: [0; 4],
            white_levels: [u16::MAX; 4],
            cfa_pattern: [0, 1, 1, 2],
            display_neutral: None,
            camera: profile.camera.clone(),
        };
        assert_eq!(check_camera(&profile, &image), Ok(None));
        profile.decode_contract.decoder_version = "old".to_owned();
        profile.camera.decoder_version = "old".to_owned();
        assert!(check_camera(&profile, &image)
            .unwrap()
            .unwrap()
            .contains("decoder version differs"));
        image.camera.decoder = "libraw".to_owned();
        assert!(check_camera(&profile, &image)
            .unwrap_err()
            .contains("RAW decoder identity"));
        image.camera.decoder = actual.decoder;
        profile.decode_contract.no_auto_scale = true;
        assert!(check_camera(&profile, &image)
            .unwrap_err()
            .contains("decode settings differ"));
    }
}
