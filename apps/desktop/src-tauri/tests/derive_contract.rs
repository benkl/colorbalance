//! The React UI reads the `derive_profile` response by field name. These tests run
//! the real command and pin that JSON shape, so a Rust-side rename or a missing
//! field fails here instead of surfacing as `undefined.toFixed` in the window.

use colorbalance_core::output_space::OutputSpace;
use colorbalance_desktop::commands::{
    correct_image, correct_image_cached, derive_profile, load_reference, load_reference_cached,
    no_progress,
};
use colorbalance_desktop::commands::{ExportOptions, ImageExport, MismatchPolicy};
use colorbalance_desktop::preview_files::PreviewFiles;
use colorbalance_desktop::reference_cache::{ReferenceCache, DEFAULT_LIMIT_BYTES};
use colorbalance_fixtures::{render_chart_dng, ChartScene};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cb-desktop-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn quad_payload(scene: &ChartScene) -> serde_json::Value {
    serde_json::json!({ "corners": scene.quad })
}

#[test]
fn derive_profile_returns_every_field_the_ui_reads() {
    let work = temp_dir("derive");
    let scene = ChartScene::default();
    let reference = work.join("reference.dng");
    render_chart_dng(&reference, &scene).unwrap();

    let response = derive_profile(
        reference.to_string_lossy().into_owned(),
        "classic-before-nov-2014".to_owned(),
        work.join("p.cbprofile.json").to_string_lossy().into_owned(),
        None,
        Some(serde_json::from_value(quad_payload(&scene)).unwrap()),
        &no_progress,
    )
    .expect("derive succeeds on a clean fixture");
    let json = serde_json::to_value(&response).unwrap();
    let _ = std::fs::remove_dir_all(work);

    // Every number the validation panel formats with `.toFixed` must be present.
    let validation = &json["validation"];
    for key in [
        "meanDeltaE",
        "medianDeltaE",
        "p95DeltaE",
        "maxDeltaE",
        "neutralMaxDeltaE",
        "skinMaxDeltaE",
        "conditionNumber",
        "patchCount",
    ] {
        assert!(
            validation[key].is_number(),
            "validation.{key} must be a number, got {validation}"
        );
    }

    // The patch table reads these per row.
    let patches = json["patches"].as_array().expect("patches array");
    assert_eq!(patches.len(), 24);
    for row in patches {
        assert!(row["patch"].is_string());
        assert!(row["deltaE"].is_number());
        for key in [
            "sourceRgb",
            "correctedRgb",
            "targetRgb",
            "correctedSrgb",
            "targetSrgb",
        ] {
            assert_eq!(row[key].as_array().map(Vec::len), Some(3), "row.{key}");
        }
        for key in ["correctedSrgb", "targetSrgb"] {
            for channel in row[key].as_array().unwrap() {
                let v = channel.as_f64().expect("number");
                assert!(
                    (0.0..=1.0).contains(&v),
                    "row.{key} channel {v} outside 0..1"
                );
            }
        }
    }

    assert!(json["profilePath"].is_string());
    assert_eq!(json["digest"].as_str().map(str::len), Some(64));
    assert!(json["qualityPassed"].is_boolean());
    assert!(json["warnings"].is_array());
}

#[test]
fn load_reference_returns_a_png_with_the_true_size() {
    let work = temp_dir("load");
    let scene = ChartScene::default();
    let reference = work.join("reference.dng");
    render_chart_dng(&reference, &scene).unwrap();

    let previews = PreviewFiles::default();
    let loaded = load_reference(
        &previews,
        reference.to_string_lossy().into_owned(),
        &no_progress,
    )
    .expect("loads");
    let json = serde_json::to_value(&loaded).unwrap();
    let _ = std::fs::remove_dir_all(work);

    assert_eq!(json["imageWidth"], scene.width);
    assert_eq!(json["imageHeight"], scene.height);
    let path = json["previewPath"].as_str().unwrap();
    assert_eq!(&std::fs::read(path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(json["quad"].as_array().map(Vec::len), Some(4));
}

#[test]
fn clipped_reference_still_derives_and_reports_short_warnings() {
    use colorbalance_fixtures::SceneDefect;

    let work = temp_dir("clipped");
    let scene = ChartScene {
        defect: SceneDefect::ClipWhitePatch,
        ..ChartScene::default()
    };
    let reference = work.join("clipped.dng");
    render_chart_dng(&reference, &scene).unwrap();
    let profile_path = work.join("p.cbprofile.json");

    let response = derive_profile(
        reference.to_string_lossy().into_owned(),
        "classic-before-nov-2014".to_owned(),
        profile_path.to_string_lossy().into_owned(),
        None,
        Some(serde_json::from_value(quad_payload(&scene)).unwrap()),
        &no_progress,
    )
    .expect("quality failures are warnings, not errors");
    let json = serde_json::to_value(&response).unwrap();
    let stored = std::fs::read_to_string(&profile_path).unwrap();
    let _ = std::fs::remove_dir_all(work);

    // Never labelled as passed, and the failure is recorded in the profile.
    assert_eq!(json["qualityPassed"], false);
    assert_eq!(json["qualityOverride"], true);
    assert!(stored.contains("\"overridden\": true") || stored.contains("\"overridden\":true"));
    let warnings: Vec<&str> = json["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap())
        .collect();
    assert!(
        warnings.iter().any(|w| w.contains("clipped")),
        "names the cause: {warnings:?}"
    );
    assert!(warnings.len() <= 4, "stays compact: {warnings:?}");
    assert!(
        warnings.iter().all(|w| w.len() < 80),
        "one short line each: {warnings:?}"
    );
}

fn derive_clean_profile(work: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let scene = ChartScene::default();
    let reference = work.join("reference.dng");
    render_chart_dng(&reference, &scene).unwrap();
    let profile_path = work.join("p.cbprofile.json");
    derive_profile(
        reference.to_string_lossy().into_owned(),
        "classic-before-nov-2014".to_owned(),
        profile_path.to_string_lossy().into_owned(),
        None,
        Some(serde_json::from_value(quad_payload(&scene)).unwrap()),
        &no_progress,
    )
    .expect("derive succeeds on a clean fixture");
    (reference, profile_path)
}

fn leftover_temp_files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
        .count()
}

#[test]
fn correct_image_returns_previews_writes_a_tiff_and_protects_existing_output() {
    let work = temp_dir("correct");
    let (reference, profile_path) = derive_clean_profile(&work);
    let output = work.join("out.tiff");
    let previews = PreviewFiles::default();
    let call = |overwrite| {
        correct_image(
            &previews,
            profile_path.to_string_lossy().into_owned(),
            reference.to_string_lossy().into_owned(),
            Some(ImageExport {
                path: output.to_string_lossy().into_owned(),
                options: ExportOptions {
                    overwrite,
                    ..Default::default()
                },
            }),
            MismatchPolicy::Block,
            &no_progress,
        )
    };

    let response = call(false).expect("corrects the reference");
    let json = serde_json::to_value(&response).unwrap();
    for key in ["beforePath", "afterPath"] {
        assert_eq!(
            &std::fs::read(json[key].as_str().unwrap()).unwrap()[..8],
            b"\x89PNG\r\n\x1a\n"
        );
    }
    assert_ne!(
        std::fs::read(json["beforePath"].as_str().unwrap()).unwrap(),
        std::fs::read(json["afterPath"].as_str().unwrap()).unwrap(),
        "the transform changed pixels"
    );
    let first = std::fs::read(&output).unwrap();
    assert_eq!(&first[..2], b"II");

    // A second run without overwrite must fail and leave the first output intact.
    let error = call(false)
        .expect_err("existing output is refused")
        .to_string();
    assert!(error.contains("already exists"), "{error}");
    assert_eq!(std::fs::read(&output).unwrap(), first);
    assert_eq!(leftover_temp_files(&work), 0);
    for key in ["beforePath", "afterPath"] {
        assert!(std::path::Path::new(json[key].as_str().unwrap()).exists());
    }
    assert_eq!(
        std::fs::read_dir(previews.directory().unwrap())
            .unwrap()
            .count(),
        2
    );

    call(true).expect("explicit overwrite succeeds");
    assert_eq!(leftover_temp_files(&work), 0);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn correct_image_refuses_a_camera_mismatch_without_writing_anything() {
    use colorbalance_core::profile;

    let work = temp_dir("mismatch");
    let (reference, profile_path) = derive_clean_profile(&work);
    // Re-seal the profile as if it belonged to another camera.
    let mut other = profile::from_json(&std::fs::read_to_string(&profile_path).unwrap()).unwrap();
    other.camera.model = "Some Other Camera".to_owned();
    other.digest = String::new();
    other.digest = profile::digest(&other);
    std::fs::write(&profile_path, profile::to_json(&other)).unwrap();

    let output = work.join("out.tiff");
    let previews = PreviewFiles::default();
    let error = correct_image(
        &previews,
        profile_path.to_string_lossy().into_owned(),
        reference.to_string_lossy().into_owned(),
        Some(ImageExport {
            path: output.to_string_lossy().into_owned(),
            options: ExportOptions {
                overwrite: true,
                ..Default::default()
            },
        }),
        MismatchPolicy::Block,
        &no_progress,
    )
    .expect_err("mismatch fails closed")
    .to_string();
    let wrote = output.exists();
    let temps = leftover_temp_files(&work);
    let _ = std::fs::remove_dir_all(work);

    assert!(error.contains("camera mismatch"), "{error}");
    assert!(!wrote);
    assert_eq!(temps, 0);
}

fn preview_names(previews: &PreviewFiles) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(previews.directory().unwrap())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn failed_correction_removes_its_new_before_preview_and_keeps_a_prior_one() {
    let work = temp_dir("failed-correct");
    let (reference, profile_path) = derive_clean_profile(&work);
    // The parent directory is missing, so the TIFF write fails after both
    // previews were rendered.
    let bad_output = work.join("missing").join("out.tiff");
    let profile = profile_path.to_string_lossy().into_owned();
    let input = reference.to_string_lossy().into_owned();
    let fail = |cache: &ReferenceCache, previews: &PreviewFiles| {
        correct_image_cached(
            cache,
            previews,
            profile.clone(),
            input.clone(),
            Some(ImageExport {
                path: bad_output.to_string_lossy().into_owned(),
                options: ExportOptions::default(),
            }),
            MismatchPolicy::Block,
            &no_progress,
        )
        .expect_err("the unwritable output fails the correction")
    };

    // No cache, so the before preview is created by the failing call.
    let previews = PreviewFiles::default();
    fail(&ReferenceCache::disabled(), &previews);
    assert_eq!(preview_names(&previews), Vec::<String>::new());

    // A preview the UI already shows must survive a failed correction.
    let cache = ReferenceCache::with_limit(DEFAULT_LIMIT_BYTES);
    let previews = PreviewFiles::default();
    let loaded = load_reference_cached(&cache, &previews, input.clone(), &no_progress).unwrap();
    let loaded = serde_json::to_value(&loaded).unwrap();
    let visible = loaded["previewPath"].as_str().unwrap().to_owned();
    let before = std::fs::read(&visible).unwrap();
    fail(&cache, &previews);
    assert_eq!(std::fs::read(&visible).unwrap(), before);
    assert_eq!(preview_names(&previews).len(), 1);
    assert!(preview_names(&previews)
        .iter()
        .all(|name| name.ends_with(".png") && !name.contains("partial")));

    // The kept preview is still the cache's, so the next correction reuses it.
    let response = correct_image_cached(
        &cache,
        &previews,
        profile.clone(),
        input.clone(),
        None,
        MismatchPolicy::Block,
        &no_progress,
    )
    .unwrap();
    let response = serde_json::to_value(&response).unwrap();
    assert_eq!(response["beforePath"].as_str().unwrap(), visible);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn derive_and_correct_report_their_stages_in_order() {
    use std::cell::RefCell;

    let work = temp_dir("stages");
    let scene = ChartScene::default();
    let reference = work.join("reference.dng");
    render_chart_dng(&reference, &scene).unwrap();
    let profile_path = work.join("p.cbprofile.json");

    let derive_stages = RefCell::new(Vec::new());
    derive_profile(
        reference.to_string_lossy().into_owned(),
        "classic-before-nov-2014".to_owned(),
        profile_path.to_string_lossy().into_owned(),
        Some(work.join("report.html").to_string_lossy().into_owned()),
        Some(serde_json::from_value(quad_payload(&scene)).unwrap()),
        &|stage, step, steps| {
            derive_stages
                .borrow_mut()
                .push((stage.to_owned(), step, steps))
        },
    )
    .unwrap();

    let correct_stages = RefCell::new(Vec::new());
    let previews = PreviewFiles::default();
    correct_image(
        &previews,
        profile_path.to_string_lossy().into_owned(),
        reference.to_string_lossy().into_owned(),
        Some(ImageExport {
            path: work.join("out.tiff").to_string_lossy().into_owned(),
            options: ExportOptions::default(),
        }),
        MismatchPolicy::Block,
        &|stage, step, steps| {
            correct_stages
                .borrow_mut()
                .push((stage.to_owned(), step, steps))
        },
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(work);

    let derive_stages = derive_stages.into_inner();
    let correct_stages = correct_stages.into_inner();
    // Steps count up from 1 to the declared total, once each, so the UI bar never jumps back.
    for stages in [&derive_stages, &correct_stages] {
        let total = stages[0].2;
        let steps: Vec<usize> = stages.iter().map(|(_, step, _)| *step).collect();
        assert_eq!(steps, (1..=total).collect::<Vec<_>>(), "{stages:?}");
        assert!(stages.iter().all(|(_, _, t)| *t == total));
    }
    assert_eq!(derive_stages.len(), 5, "report requested adds a stage");
    assert_eq!(correct_stages.len(), 5, "saving adds a stage");
    assert_eq!(derive_stages[0].0, "Decoding image");
    assert_eq!(correct_stages[4].0, "Writing 16-bit TIFF");
}

#[test]
fn batch_progress_counts_finished_files_and_reaches_the_total() {
    use colorbalance_desktop::commands::apply_batch;
    use std::sync::{Arc, Mutex};

    let work = temp_dir("batch-progress");
    let (reference, profile_path) = derive_clean_profile(&work);
    let input = work.join("in");
    std::fs::create_dir_all(&input).unwrap();
    for name in ["a.dng", "b.dng", "c.dng"] {
        std::fs::copy(&reference, input.join(name)).unwrap();
    }
    let output = work.join("out");
    std::fs::create_dir_all(&output).unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let response = apply_batch(
        profile_path.to_string_lossy().into_owned(),
        input.to_string_lossy().into_owned(),
        output.to_string_lossy().into_owned(),
        ExportOptions::default(),
        MismatchPolicy::Block,
        Default::default(),
        Arc::new(move |completed, total, file| {
            sink.lock()
                .unwrap()
                .push((completed, total, file.is_some()));
        }),
    )
    .expect("batch runs");
    let temps = leftover_temp_files(&output);
    let _ = std::fs::remove_dir_all(work);

    let events = events.lock().unwrap().clone();
    assert_eq!(serde_json::to_value(&response).unwrap()["total"], 3);
    assert_eq!(temps, 0);
    assert_eq!(events[0], (0, 3, false), "starts at zero finished");
    assert_eq!(
        events.last().unwrap(),
        &(3, 3, false),
        "ends fully finished"
    );
    let finished: Vec<usize> = events
        .iter()
        .filter(|(_, _, started)| !started)
        .map(|(done, _, _)| *done)
        .collect();
    let mut sorted = finished.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![0, 1, 2, 3], "one finish event per file");
    assert_eq!(
        events.iter().filter(|(_, _, started)| *started).count(),
        3,
        "one start event per file"
    );
}

#[test]
fn correct_image_embeds_the_icc_profile_of_the_chosen_output_space() {
    let work = temp_dir("output-space");
    let (reference, profile_path) = derive_clean_profile(&work);
    let previews = PreviewFiles::default();
    let mut tiffs = Vec::new();
    for (space, name) in [
        (OutputSpace::Srgb, "srgb.tiff"),
        (OutputSpace::DisplayP3, "p3.tiff"),
    ] {
        let output = work.join(name);
        correct_image(
            &previews,
            profile_path.to_string_lossy().into_owned(),
            reference.to_string_lossy().into_owned(),
            Some(ImageExport {
                path: output.to_string_lossy().into_owned(),
                options: ExportOptions {
                    space,
                    ..Default::default()
                },
            }),
            MismatchPolicy::Block,
            &no_progress,
        )
        .expect("writes the TIFF");
        tiffs.push(std::fs::read(output).unwrap());
    }
    let _ = std::fs::remove_dir_all(work);

    // Tag 34675 (0x8773) must be present in each file's IFD and point at an
    // ICC profile whose `acsp` signature sits at byte 36.
    let icc_of = |tiff: &[u8]| -> Vec<u8> {
        let ifd = u32::from_le_bytes(tiff[4..8].try_into().unwrap()) as usize;
        let count = u16::from_le_bytes(tiff[ifd..ifd + 2].try_into().unwrap()) as usize;
        for i in 0..count {
            let at = ifd + 2 + i * 12;
            if u16::from_le_bytes(tiff[at..at + 2].try_into().unwrap()) == 34675 {
                let len = u32::from_le_bytes(tiff[at + 4..at + 8].try_into().unwrap()) as usize;
                let off = u32::from_le_bytes(tiff[at + 8..at + 12].try_into().unwrap()) as usize;
                return tiff[off..off + len].to_vec();
            }
        }
        panic!("no ICC profile tag");
    };
    let (srgb_icc, p3_icc) = (icc_of(&tiffs[0]), icc_of(&tiffs[1]));
    assert_eq!(&srgb_icc[36..40], b"acsp");
    assert_eq!(&p3_icc[36..40], b"acsp");
    assert_ne!(srgb_icc, p3_icc);
    assert_ne!(
        tiffs[0], tiffs[1],
        "the pixels were converted, not relabeled"
    );
}

#[test]
fn jpeg_export_writes_a_jpeg_and_reports_metadata() {
    use colorbalance_desktop::commands::ExportFormat;

    let work = temp_dir("jpeg-export");
    let (reference, profile_path) = derive_clean_profile(&work);
    let previews = PreviewFiles::default();
    let output = work.join("out.jpg");
    let stages = std::cell::RefCell::new(Vec::<String>::new());
    let response = correct_image(
        &previews,
        profile_path.to_string_lossy().into_owned(),
        reference.to_string_lossy().into_owned(),
        Some(ImageExport {
            path: output.to_string_lossy().into_owned(),
            options: ExportOptions {
                format: ExportFormat::Jpeg,
                quality: 90,
                ..Default::default()
            },
        }),
        MismatchPolicy::Block,
        &|stage, _, _| stages.borrow_mut().push(stage.to_owned()),
    )
    .expect("writes the JPEG");
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(&bytes[..2], [0xFF, 0xD8], "JPEG SOI marker");
    assert!(stages
        .borrow()
        .iter()
        .any(|stage| stage == "Writing 8-bit JPEG"));
    let json = serde_json::to_value(&response).unwrap();
    assert!(json["metadata"]["copied"].is_array(), "{json}");
    assert!(json["metadata"]["skipped"].is_array(), "{json}");
    assert_eq!(leftover_temp_files(&work), 0);

    // The input is never a valid output, even when overwriting is allowed.
    let error = correct_image(
        &previews,
        profile_path.to_string_lossy().into_owned(),
        reference.to_string_lossy().into_owned(),
        Some(ImageExport {
            path: reference.to_string_lossy().into_owned(),
            options: ExportOptions {
                overwrite: true,
                ..Default::default()
            },
        }),
        MismatchPolicy::Block,
        &no_progress,
    )
    .expect_err("refuses to overwrite its input");
    assert!(error.to_string().contains("input"), "{error}");
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn export_profile_writes_dcp_atomically_and_refuses_unsafe_requests() {
    use colorbalance_desktop::commands::export_profile;

    let work = temp_dir("dcp-export");
    let scene = ChartScene::default();
    let reference = work.join("reference.dng");
    render_chart_dng(&reference, &scene).unwrap();
    let profile_path = work.join("p.cbprofile.json");
    derive_profile(
        reference.to_string_lossy().into_owned(),
        "classic-before-nov-2014".to_owned(),
        profile_path.to_string_lossy().into_owned(),
        None,
        Some(serde_json::from_value(quad_payload(&scene)).unwrap()),
        &no_progress,
    )
    .expect("derive succeeds on a clean fixture");
    let profile = profile_path.to_string_lossy().into_owned();
    let out = work.join("camera.dcp");

    let written = export_profile(
        profile.clone(),
        "dcp".into(),
        out.to_string_lossy().into_owned(),
        None,
        Some("Test Camera".into()),
    )
    .expect("dcp export succeeds");
    assert_eq!(written, out.to_string_lossy());
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..4], b"II\x52\x43", "DCP magic");
    assert_eq!(leftover_temp_files(&work), 0);

    // A missing camera name, or a name on a non-DCP format, is refused.
    let other = work.join("never.dcp");
    let error = export_profile(
        profile.clone(),
        "dcp".into(),
        other.to_string_lossy().into_owned(),
        None,
        None,
    )
    .expect_err("camera name required");
    assert!(error.to_string().contains("camera name"), "{error}");
    let error = export_profile(
        profile.clone(),
        "clf".into(),
        work.join("never.clf").to_string_lossy().into_owned(),
        None,
        Some("x".into()),
    )
    .expect_err("camera name only for dcp");
    assert!(error.to_string().contains("camera name"), "{error}");

    // A quick-and-dirty profile cannot become a RAW camera profile.
    let mut quick =
        colorbalance_core::profile::from_json(&std::fs::read_to_string(&profile_path).unwrap())
            .unwrap();
    quick
        .quality
        .as_mut()
        .expect("derive records quality")
        .quick_and_dirty = true;
    quick.digest = colorbalance_core::profile::digest(&quick);
    let rendered = work.join("rendered.cbprofile.json");
    std::fs::write(&rendered, colorbalance_core::profile::to_json(&quick)).unwrap();
    let error = export_profile(
        rendered.to_string_lossy().into_owned(),
        "dcp".into(),
        other.to_string_lossy().into_owned(),
        None,
        Some("Test Camera".into()),
    )
    .expect_err("quick-and-dirty refused");
    assert!(error.to_string().contains("quick-and-dirty"), "{error}");
    assert!(!other.exists());
    assert_eq!(leftover_temp_files(&work), 0);
    let _ = std::fs::remove_dir_all(work);
}
