//! The React UI reads the `derive_profile` response by field name. These tests run
//! the real command and pin that JSON shape, so a Rust-side rename or a missing
//! field fails here instead of surfacing as `undefined.toFixed` in the window.

use colorbalance_desktop::commands::{correct_image, derive_profile, load_reference, no_progress};
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
        for key in ["sourceRgb", "correctedRgb", "targetRgb"] {
            assert_eq!(row[key].as_array().map(Vec::len), Some(3), "row.{key}");
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

    let loaded =
        load_reference(reference.to_string_lossy().into_owned(), &no_progress).expect("loads");
    let json = serde_json::to_value(&loaded).unwrap();
    let _ = std::fs::remove_dir_all(work);

    assert_eq!(json["imageWidth"], scene.width);
    assert_eq!(json["imageHeight"], scene.height);
    assert!(
        json["previewDataUrl"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"),
        "the webview can only render formats like PNG, not PPM"
    );
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
    let call = |overwrite| {
        correct_image(
            profile_path.to_string_lossy().into_owned(),
            reference.to_string_lossy().into_owned(),
            Some(output.to_string_lossy().into_owned()),
            overwrite,
            &no_progress,
        )
    };

    let response = call(false).expect("corrects the reference");
    let json = serde_json::to_value(&response).unwrap();
    for key in ["beforeDataUrl", "afterDataUrl"] {
        assert!(json[key]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }
    assert_ne!(
        json["beforeDataUrl"], json["afterDataUrl"],
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
    let error = correct_image(
        profile_path.to_string_lossy().into_owned(),
        reference.to_string_lossy().into_owned(),
        Some(output.to_string_lossy().into_owned()),
        true,
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
    correct_image(
        profile_path.to_string_lossy().into_owned(),
        reference.to_string_lossy().into_owned(),
        Some(work.join("out.tiff").to_string_lossy().into_owned()),
        false,
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
        false,
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
