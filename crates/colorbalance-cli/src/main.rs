//! ColorBalance command-line interface.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use colorbalance_core::calibration::{self, ChartQuad, GateConfig, GateFailure, ValidationReport};
use colorbalance_core::chart::ChartRevision;
use colorbalance_core::contract::DecodeContract;
use colorbalance_core::dataset;
use colorbalance_core::decode::{CameraIdentity, DecodedImage};
use colorbalance_core::interchange::{profile_to_clf, profile_to_cube};
use colorbalance_core::output::encode_tiff_rgb_u16;
use colorbalance_core::output_space::{OutputConverter, OutputSpace, OCIO_CONFIG, OCIO_VERSION};
use colorbalance_core::profile::{self, Profile, ValidationSummary};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Parser)]
#[command(
    name = "colorbalance",
    version,
    about = "Derive a ColorChecker color transform and apply it to RAW and JPEG batches",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the canonical RAW decode contract as JSON
    DecodeContract,
    /// Inspect a reference file and print chart quality diagnostics as JSON
    Inspect(InspectArgs),
    /// Derive a measured color profile and HTML report from a reference
    Derive(DeriveArgs),
    /// Apply a measured profile to a directory of images and write 16-bit TIFFs
    Apply(ApplyArgs),
    /// Export a profile to Common LUT Format (.clf) or 3D LUT (.cube)
    Export(ExportArgs),
}

#[derive(Args)]
struct InspectArgs {
    /// Reference image file path (RAW DNG, or JPEG/PNG in quick-and-dirty mode)
    reference: PathBuf,
    /// Physical chart revision
    #[arg(long, value_enum, default_value_t = ChartRevisionArg::ClassicFromNovember2014)]
    chart: ChartRevisionArg,
    /// Optional manual corners: x1,y1,x2,y2,x3,y3,x4,y4 (TL, TR, BR, BL)
    #[arg(long, value_parser = parse_quad)]
    quad: Option<ChartQuad>,
    /// Quick-and-dirty mode: approximate calibration on non-RAW JPEG/PNG sources
    #[arg(long, default_value_t = false)]
    quick_and_dirty: bool,
}

#[derive(Args)]
struct DeriveArgs {
    /// Reference image file path (RAW DNG, or JPEG/PNG in quick-and-dirty mode)
    reference: PathBuf,
    /// Physical chart revision
    #[arg(long, value_enum, default_value_t = ChartRevisionArg::ClassicFromNovember2014)]
    chart: ChartRevisionArg,
    /// Output path for the generated profile JSON
    #[arg(long, short = 'p')]
    profile: PathBuf,
    /// Output path for the human-readable HTML quality report
    #[arg(long, short = 'r')]
    report: Option<PathBuf>,
    /// Output path for the SVG diagnostic overlay with quad and patch regions
    #[arg(long)]
    overlay: Option<PathBuf>,
    /// Optional manual corners: x1,y1,x2,y2,x3,y3,x4,y4 (TL, TR, BR, BL)
    #[arg(long, value_parser = parse_quad)]
    quad: Option<ChartQuad>,
    /// Allow writing a profile despite quality gate failures (recorded in report)
    #[arg(long, default_value_t = false)]
    force: bool,
    /// Quick-and-dirty mode: approximate calibration on non-RAW JPEG/PNG sources
    #[arg(long, default_value_t = false)]
    quick_and_dirty: bool,
}

#[derive(Args)]
struct ApplyArgs {
    /// Measured profile JSON file path
    profile: PathBuf,
    /// Input directory containing matching images (or a single file)
    input: PathBuf,
    /// Destination directory for balanced 16-bit TIFF outputs
    #[arg(long, short = 'o')]
    output: PathBuf,
    /// Encoded color space of the TIFF samples and its embedded ICC profile:
    /// srgb, display-p3 or adobe-rgb. Non-sRGB targets convert through OCIO
    /// before clipping.
    #[arg(long, default_value = "srgb")]
    output_space: OutputSpace,
    /// Output format (only 'tiff' in this release)
    #[arg(long, default_value = "tiff")]
    format: String,
    /// Overwrite existing output files (default: skip/fail existing)
    #[arg(long, default_value_t = false)]
    overwrite: bool,
    /// Ignore camera or decode-contract mismatch (default: fail closed)
    #[arg(long, default_value_t = false)]
    force: bool,
    /// In-flight image bound for parallel decoding (default: 2)
    #[arg(long, default_value_t = 2)]
    workers: usize,
    /// Output path for the JSON batch summary
    #[arg(long)]
    summary: Option<PathBuf>,
}

#[derive(Args)]
struct ExportArgs {
    /// Profile JSON file path
    profile: PathBuf,
    /// Export interchange format
    #[arg(long, value_enum)]
    format: ExportFormatArg,
    /// Output destination path
    #[arg(long, short = 'o')]
    output: PathBuf,
    /// 3D LUT size (only used for .cube export, default: 33)
    #[arg(long, default_value_t = 33)]
    size: usize,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ChartRevisionArg {
    #[value(name = "classic-before-nov-2014")]
    ClassicBeforeNovember2014,
    #[value(name = "classic-from-nov-2014")]
    ClassicFromNovember2014,
}

impl From<ChartRevisionArg> for ChartRevision {
    fn from(arg: ChartRevisionArg) -> Self {
        match arg {
            ChartRevisionArg::ClassicBeforeNovember2014 => ChartRevision::ClassicBeforeNovember2014,
            ChartRevisionArg::ClassicFromNovember2014 => ChartRevision::ClassicFromNovember2014,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ExportFormatArg {
    #[value(name = "clf")]
    Clf,
    #[value(name = "cube")]
    Cube,
}

fn parse_quad(s: &str) -> Result<ChartQuad, String> {
    let parts: Result<Vec<f64>, _> = s.split(',').map(|p| p.trim().parse::<f64>()).collect();
    let values = parts.map_err(|e| format!("invalid number in quad: {e}"))?;
    if values.len() != 8 {
        return Err(format!(
            "quad must have exactly 8 numbers (x1,y1,x2,y2,x3,y3,x4,y4), got {}",
            values.len()
        ));
    }
    Ok(ChartQuad {
        corners: [
            [values[0], values[1]],
            [values[2], values[3]],
            [values[4], values[5]],
            [values[6], values[7]],
        ],
    })
}

fn default_quad_for_image(width: u32, height: u32) -> ChartQuad {
    let w = f64::from(width);
    let h = f64::from(height);
    let margin_x = w * 0.08;
    let margin_y = h * 0.08;
    ChartQuad {
        corners: [
            [margin_x, margin_y],
            [w - margin_x, margin_y],
            [w - margin_x, h - margin_y],
            [margin_x, h - margin_y],
        ],
    }
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct InspectOutput {
    camera: CameraIdentity,
    image_width: u32,
    image_height: u32,
    chart_revision: ChartRevision,
    quality_passed: bool,
    gate_failures: Vec<InspectGateFailure>,
    quad: [[f64; 2]; 4],
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct InspectGateFailure {
    patch: Option<String>,
    reason: String,
    measured: String,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct BatchSummary {
    pub output_space: String,
    /// OCIO config and `ocio` crate version used for non-sRGB targets.
    pub ocio_config: Option<String>,
    pub ocio_version: Option<String>,
    pub succeeded: Vec<String>,
    pub skipped: Vec<String>,
    pub warnings: Vec<BatchWarning>,
    pub failed: Vec<BatchFileError>,
    pub total: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BatchFileError {
    pub file: String,
    pub error: String,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BatchWarning {
    pub file: String,
    pub warning: String,
}

fn decode_auto(path: &Path) -> Result<DecodedImage, String> {
    colorbalance_raw::decode_any(path).map_err(|e| format!("{e}"))
}

fn execute_inspect(args: InspectArgs) -> Result<(), String> {
    let image = decode_auto(&args.reference).map_err(|e| format!("decode failed: {e}"))?;
    let quad = args
        .quad
        .unwrap_or_else(|| default_quad_for_image(image.width, image.height));
    let samples = calibration::sample_patches(&image, &quad)
        .map_err(|e| format!("patch sampling failed: {e}"))?;
    let gate_cfg = if args.quick_and_dirty {
        GateConfig::quick_and_dirty()
    } else {
        GateConfig::default()
    };
    let gate_res = calibration::evaluate_quality(&samples, &gate_cfg);
    let (passed, failures) = match gate_res {
        Ok(()) => (true, Vec::new()),
        Err(f) => (
            false,
            f.into_iter()
                .map(|fail| InspectGateFailure {
                    patch: fail.patch.map(|p| format!("{p:?}")),
                    reason: fail.reason,
                    measured: fail.measured,
                })
                .collect(),
        ),
    };
    let output = InspectOutput {
        camera: image.camera,
        image_width: image.width,
        image_height: image.height,
        chart_revision: args.chart.into(),
        quality_passed: passed,
        gate_failures: failures,
        quad: quad.corners,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn execute_derive(args: DeriveArgs) -> Result<(), String> {
    let image = decode_auto(&args.reference).map_err(|e| format!("decode failed: {e}"))?;
    let quad = args
        .quad
        .unwrap_or_else(|| default_quad_for_image(image.width, image.height));
    let revision: ChartRevision = args.chart.into();
    let dataset = dataset::load(revision).map_err(|e| format!("dataset load failed: {e}"))?;
    let samples = calibration::sample_patches(&image, &quad)
        .map_err(|e| format!("patch sampling failed: {e}"))?;
    let gate_cfg = if args.quick_and_dirty {
        GateConfig::quick_and_dirty()
    } else {
        GateConfig::default()
    };
    let gate_failures = match calibration::evaluate_quality(&samples, &gate_cfg) {
        Ok(()) => Vec::new(),
        Err(f) => f,
    };

    if !gate_failures.is_empty() && !args.force {
        let reasons: Vec<String> = gate_failures
            .iter()
            .map(|f| format!("- {}: {}", f.reason, f.measured))
            .collect();
        return Err(format!(
            "quality gates failed (use --force to override):\n{}",
            reasons.join("\n")
        ));
    }

    let (stages, validation) =
        calibration::fit(&samples, &dataset).map_err(|e| format!("fit failed: {e}"))?;
    let ref_digest =
        sha256_file(&args.reference).map_err(|e| format!("failed to hash reference: {e}"))?;
    let contract = if image.sensor_layout == colorbalance_core::decode::SensorLayout::Rendered {
        DecodeContract::canonical(&image.camera.decoder, &image.camera.decoder_version)
    } else {
        let contract = colorbalance_raw::canonical_contract();
        if contract.decoder != image.camera.decoder
            || contract.decoder_version != image.camera.decoder_version
        {
            return Err("RAW decoder identity disagrees with its contract".to_owned());
        }
        contract
    };
    let p_initial = Profile {
        schema_version: profile::SCHEMA_VERSION.to_owned(),
        decode_contract: contract,
        camera: image.camera.clone(),
        chart_revision: revision,
        dataset_digest: dataset::dataset_digest(),
        reference_digest: ref_digest,
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
            &gate_failures,
            args.force,
            args.quick_and_dirty,
        )),
        digest: String::new(),
    };
    let initial_json = profile::to_json(&p_initial);
    let mut p: Profile = serde_json::from_str(&initial_json).map_err(|e| e.to_string())?;
    p.digest = profile::digest(&p);

    if let Some(parent) = args.profile.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let json_text = profile::to_json(&p);
    fs::write(&args.profile, json_text).map_err(|e| format!("writing profile: {e}"))?;

    if let Some(report_path) = args.report {
        let html = generate_html_report(
            &image,
            &quad,
            &validation,
            &gate_failures,
            args.force || args.quick_and_dirty,
            &p.digest,
        );
        if let Some(parent) = report_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        fs::write(&report_path, html).map_err(|e| format!("writing report: {e}"))?;
    }

    if let Some(overlay_path) = args.overlay {
        let svg = generate_svg_overlay(&image, &quad);
        if let Some(parent) = overlay_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        fs::write(&overlay_path, svg).map_err(|e| format!("writing overlay: {e}"))?;
    }

    eprintln!(
        "Derived profile: {} (mean dE: {:.3}, max dE: {:.3})",
        args.profile.display(),
        validation.mean_delta_e,
        validation.max_delta_e
    );
    Ok(())
}

fn generate_html_report(
    image: &DecodedImage,
    quad: &ChartQuad,
    validation: &ValidationReport,
    gate_failures: &[GateFailure],
    forced: bool,
    digest: &str,
) -> String {
    let mut rows = String::new();
    for pv in &validation.per_patch {
        let c_rgb = [
            (pv.corrected_rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (pv.corrected_rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (pv.corrected_rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        ];
        let t_rgb = [
            (pv.target_rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (pv.target_rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (pv.target_rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        ];
        rows.push_str(&format!(
            "<tr><td>{:?}</td>\
            <td style=\"background:rgb({},{},{})\"></td>\
            <td style=\"background:rgb({},{},{})\"></td>\
            <td>{:.3}</td></tr>\n",
            pv.patch, c_rgb[0], c_rgb[1], c_rgb[2], t_rgb[0], t_rgb[1], t_rgb[2], pv.delta_e
        ));
    }

    let warning_block = if gate_failures.is_empty() {
        "<p style=\"color:green;\"><strong>Reference quality: PASS</strong></p>".to_owned()
    } else {
        let mut list = String::from("<ul>");
        for f in gate_failures {
            list.push_str(&format!("<li>{}: {}</li>", f.reason, f.measured));
        }
        list.push_str("</ul>");
        let badge = if forced {
            "<p style=\"color:orange;\"><strong>Reference quality: OVERRIDDEN / QUICK & DIRTY</strong></p>"
        } else {
            "<p style=\"color:red;\"><strong>Reference quality: FAILED</strong></p>"
        };
        format!("{badge}{list}")
    };

    format!(
        "<!DOCTYPE html>\n<html>\n<head>\
        <meta charset=\"utf-8\">\
        <title>ColorBalance Calibration Report</title>\
        <style>\
        body {{ font-family: sans-serif; margin: 2rem; background: #fafafa; color: #222; }}\
        table {{ border-collapse: collapse; width: 100%; max-width: 600px; }}\
        th, td {{ border: 1px solid #ccc; padding: 6px 12px; text-align: left; }}\
        .color-cell {{ width: 50px; }}\
        svg {{ max-width: 480px; border: 1px solid #888; background: #eee; }}\
        </style>\
        </head>\n<body>\
        <h1>ColorBalance Calibration Report</h1>\
        <p>Profile digest: <code>{digest}</code></p>\
        <p>Camera: {} {} (decoder: {} {})</p>\
        {warning_block}\
        <h2>Metrics</h2>\
        <ul>\
        <li>Mean ΔE2000: <strong>{:.3}</strong></li>\
        <li>Median ΔE2000: <strong>{:.3}</strong></li>\
        <li>95th percentile ΔE2000: <strong>{:.3}</strong></li>\
        <li>Max ΔE2000: <strong>{:.3}</strong></li>\
        <li>Condition number: <strong>{:.2}</strong></li>\
        </ul>\
        <h2>Chart Quad Overlay</h2>\
        <svg viewBox=\"0 0 {} {}\">\
        <polygon points=\"{},{} {},{} {},{} {},{}\" fill=\"rgba(0,128,255,0.2)\" stroke=\"blue\" stroke-width=\"2\" />\
        </svg>\
        <h2>Patch Details</h2>\
        <table>\
        <tr><th>Patch</th><th>Corrected</th><th>Target</th><th>ΔE2000</th></tr>\
        {rows}\
        </table>\
        </body>\n</html>",
        image.camera.make,
        image.camera.model,
        image.camera.decoder,
        image.camera.decoder_version,
        validation.mean_delta_e,
        validation.median_delta_e,
        validation.p95_delta_e,
        validation.max_delta_e,
        validation.condition_number,
        image.width,
        image.height,
        quad.corners[0][0], quad.corners[0][1],
        quad.corners[1][0], quad.corners[1][1],
        quad.corners[2][0], quad.corners[2][1],
        quad.corners[3][0], quad.corners[3][1],
    )
}

/// Generate a standalone SVG overlay showing the detected/specified quad
/// and the 24 sampled patch regions.
fn generate_svg_overlay(image: &DecodedImage, quad: &ChartQuad) -> String {
    let mut patches_svg = String::new();
    for row in 0..4 {
        for col in 0..6 {
            let u0 = col as f64 / 6.0;
            let u1 = (col + 1) as f64 / 6.0;
            let v0 = row as f64 / 4.0;
            let v1 = (row + 1) as f64 / 4.0;
            let du = u1 - u0;
            let dv = v1 - v0;
            let su0 = u0 + 0.2 * du;
            let su1 = u1 - 0.2 * du;
            let sv0 = v0 + 0.2 * dv;
            let sv1 = v1 - 0.2 * dv;
            let bilinear = |u: f64, v: f64| -> [f64; 2] {
                let top = [
                    (1.0 - u) * quad.corners[0][0] + u * quad.corners[1][0],
                    (1.0 - u) * quad.corners[0][1] + u * quad.corners[1][1],
                ];
                let bot = [
                    (1.0 - u) * quad.corners[3][0] + u * quad.corners[2][0],
                    (1.0 - u) * quad.corners[3][1] + u * quad.corners[2][1],
                ];
                [
                    (1.0 - v) * top[0] + v * bot[0],
                    (1.0 - v) * top[1] + v * bot[1],
                ]
            };
            let p0 = bilinear(su0, sv0);
            let p1 = bilinear(su1, sv0);
            let p2 = bilinear(su1, sv1);
            let p3 = bilinear(su0, sv1);
            patches_svg.push_str(&format!(
                "<polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" \
                 fill=\"rgba(245,109,24,0.15)\" stroke=\"#f56d18\" stroke-width=\"1\" stroke-dasharray=\"2,2\" />\n",
                p0[0], p0[1], p1[0], p1[1], p2[0], p2[1], p3[0], p3[1]
            ));
        }
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\">\n\
         <polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" \
          fill=\"rgba(0,128,255,0.15)\" stroke=\"#0080ff\" stroke-width=\"2\" />\n\
         {}\
         </svg>\n",
        image.width,
        image.height,
        quad.corners[0][0],
        quad.corners[0][1],
        quad.corners[1][0],
        quad.corners[1][1],
        quad.corners[2][0],
        quad.corners[2][1],
        quad.corners[3][0],
        quad.corners[3][1],
        patches_svg
    )
}

fn execute_apply(args: ApplyArgs) -> Result<(), String> {
    let prof_text =
        fs::read_to_string(&args.profile).map_err(|e| format!("cannot read profile: {e}"))?;
    let prof =
        profile::from_json(&prof_text).map_err(|e| format!("profile validation failed: {e}"))?;

    let options = colorbalance_core::batch::BatchOptions {
        output: args.output.clone(),
        overwrite: args.overwrite,
        workers: args.workers,
        extensions: colorbalance_core::batch::DEFAULT_EXTENSIONS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    let inputs = colorbalance_core::batch::collect_inputs(&args.input, &options.extensions)?;
    if inputs.is_empty() {
        return Err(format!("no files found at {}", args.input.display()));
    }

    // Resolve the output space before touching the destination: a missing
    // OCIO space or ICC profile must stop the batch, not label pixels wrong.
    let converter = OutputConverter::new(args.output_space).map_err(|e| e.to_string())?;
    let icc_profile = converter.icc_profile().map_err(|e| e.to_string())?;

    cleanup_stale_temp_files(&args.output);

    let (warning_tx, warning_rx) = std::sync::mpsc::channel::<BatchWarning>();
    let prof = std::sync::Arc::new(prof);
    let force = args.force;
    let overwrite = args.overwrite;
    let file_warnings = warning_tx.clone();
    let process = move |input: &Path, output: PathBuf| -> Result<Option<PathBuf>, String> {
        if output.exists() && !overwrite {
            return Ok(None);
        }

        let mut decoded = decode_auto(input).map_err(|e| format!("decode failed: {e}"))?;

        if (decoded.camera.make != prof.camera.make || decoded.camera.model != prof.camera.model)
            && !force
        {
            return Err(format!(
                "camera mismatch (profile: {} {}, image: {} {})",
                prof.camera.make, prof.camera.model, decoded.camera.make, decoded.camera.model
            ));
        }

        if prof.camera.decoder != prof.decode_contract.decoder
            || prof.camera.decoder_version != prof.decode_contract.decoder_version
        {
            return Err(
                "profile camera decoder identity disagrees with its decode contract".to_owned(),
            );
        }
        let actual = if decoded.sensor_layout == colorbalance_core::decode::SensorLayout::Rendered {
            DecodeContract::canonical(&decoded.camera.decoder, &decoded.camera.decoder_version)
        } else {
            let contract = colorbalance_raw::canonical_contract();
            if contract.decoder != decoded.camera.decoder
                || contract.decoder_version != decoded.camera.decoder_version
            {
                return Err(format!(
                    "RAW decoder identity disagrees with its contract (image: {} {}, contract: {} {})",
                    decoded.camera.decoder, decoded.camera.decoder_version,
                    contract.decoder, contract.decoder_version
                ));
            }
            contract
        };
        let warning = match prof.decode_contract.compare_for_apply(&actual) {
            Ok(warning) => warning,
            Err(error) if force => Some(format!("forced decode-contract mismatch: {error}")),
            Err(error) => return Err(error.to_string()),
        };

        let (out_u16, _clipped) = converter.correct_to_u16(&prof, &mut decoded.rgb);
        let tiff_bytes = encode_tiff_rgb_u16(decoded.width, decoded.height, &out_u16, &icc_profile);
        let tmp_path = output
            .parent()
            .map(Path::new)
            .unwrap_or(Path::new("."))
            .join(format!(
                ".tmp-{}-{}.tiff",
                std::process::id(),
                fastrand_u64()
            ));
        if let Err(e) = fs::write(&tmp_path, &tiff_bytes) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("write failed: {e}"));
        }
        if let Err(e) = atomic_rename(&tmp_path, &output) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("rename failed: {e}"));
        }
        if let Some(warning) = warning {
            let _ = file_warnings.send(BatchWarning {
                file: input.display().to_string(),
                warning,
            });
        }
        Ok(Some(output))
    };

    let core_summary = colorbalance_core::batch::run_batch(
        inputs,
        &options,
        None,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        process,
    )?;

    let uses_ocio = args.output_space.ocio_destination().is_some();
    let summary = BatchSummary {
        output_space: args.output_space.to_string(),
        ocio_config: uses_ocio.then(|| OCIO_CONFIG.to_owned()),
        ocio_version: uses_ocio.then(|| OCIO_VERSION.to_owned()),
        total: core_summary.total,
        succeeded: core_summary
            .succeeded
            .into_iter()
            .map(|r| r.input)
            .collect(),
        skipped: core_summary.skipped,
        warnings: {
            drop(warning_tx);
            let mut warnings: Vec<_> = warning_rx.into_iter().collect();
            warnings.sort_by(|a, b| a.file.cmp(&b.file));
            warnings
        },
        failed: core_summary
            .failed
            .into_iter()
            .map(|r| BatchFileError {
                file: r.input,
                error: r.message.unwrap_or_else(|| "unknown error".to_string()),
            })
            .collect(),
    };

    if let Some(sum_path) = args.summary {
        let text = serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?;
        fs::write(sum_path, text).map_err(|e| e.to_string())?;
    }
    for warning in &summary.warnings {
        eprintln!("Warning ({}): {}", warning.file, warning.warning);
    }

    eprintln!(
        "Batch complete: {} succeeded, {} skipped, {} failed (total: {})",
        summary.succeeded.len(),
        summary.skipped.len(),
        summary.failed.len(),
        summary.total
    );

    if !summary.failed.is_empty() {
        return Err(format!("{} files failed to process", summary.failed.len()));
    }

    Ok(())
}

/// Remove leftover `.tmp-*.tiff` files from a previously crashed or cancelled run.
fn cleanup_stale_temp_files(output_dir: &Path) {
    if let Ok(entries) = fs::read_dir(output_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".tmp-") && name.ends_with(".tiff") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn fastrand_u64() -> u64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    nanos as u64
}

fn atomic_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if to.exists() {
            fs::remove_file(to)?;
        }
        fs::rename(from, to)
    }
    #[cfg(not(windows))]
    {
        fs::rename(from, to)
    }
}

fn execute_export(args: ExportArgs) -> Result<(), String> {
    let prof_text =
        fs::read_to_string(&args.profile).map_err(|e| format!("cannot read profile: {e}"))?;
    let prof =
        profile::from_json(&prof_text).map_err(|e| format!("profile validation failed: {e}"))?;

    let content = match args.format {
        ExportFormatArg::Clf => profile_to_clf(&prof),
        ExportFormatArg::Cube => profile_to_cube(&prof, args.size),
    };

    if let Some(parent) = args.output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

    fs::write(&args.output, content).map_err(|e| format!("cannot write export: {e}"))?;
    eprintln!("Exported {:?} to {}", args.format, args.output.display());
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::DecodeContract => {
            let contract = colorbalance_raw::canonical_contract();
            println!("{}", serde_json::to_string_pretty(&contract).unwrap());
            Ok(())
        }
        Commands::Inspect(args) => execute_inspect(args),
        Commands::Derive(args) => execute_derive(args),
        Commands::Apply(args) => execute_apply(args),
        Commands::Export(args) => execute_export(args),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("colorbalance error: {err}");
            ExitCode::FAILURE
        }
    }
}
