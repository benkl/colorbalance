//! Stored-profile chart checks and batch preflight, driven through the same
//! entry points the IPC commands call.

use std::path::PathBuf;

use colorbalance_desktop::chart_check::check_chart;
use colorbalance_desktop::commands::{derive_profile, no_progress};
use colorbalance_desktop::preflight::scan_batch;
use colorbalance_desktop::reference_cache::ReferenceCache;
use colorbalance_fixtures::{render_chart_dng, ChartScene};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cb-check-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scene(exposure: f64) -> ChartScene {
    ChartScene {
        width: 480,
        height: 320,
        quad: [[40.0, 40.0], [440.0, 34.0], [440.0, 280.0], [40.0, 280.0]],
        exposure,
        ..ChartScene::default()
    }
}

fn derive(work: &std::path::Path) -> (String, String, ChartScene) {
    let scene = scene(1.0);
    let reference = work.join("ref.dng");
    render_chart_dng(&reference, &scene).unwrap();
    let profile = work.join("p.cbprofile.json");
    derive_profile(
        reference.to_string_lossy().into_owned(),
        "classic-before-nov-2014".to_owned(),
        profile.to_string_lossy().into_owned(),
        None,
        Some(serde_json::from_value(serde_json::json!({ "corners": scene.quad })).unwrap()),
        &no_progress,
    )
    .unwrap();
    (
        profile.to_string_lossy().into_owned(),
        reference.to_string_lossy().into_owned(),
        scene,
    )
}

#[test]
fn chart_check_measures_a_new_frame_through_the_stored_stages_without_refitting() {
    let work = temp_dir("chart");
    let (profile, reference, scene) = derive(&work);
    let before = std::fs::read(&profile).unwrap();
    let cache = ReferenceCache::disabled();
    let quad =
        || Some(serde_json::from_value(serde_json::json!({ "corners": scene.quad })).unwrap());

    let same = check_chart(&cache, profile.clone(), reference, quad()).unwrap();
    let same = serde_json::to_value(&same).unwrap();

    let dark = work.join("dark.dng");
    render_chart_dng(
        &dark,
        &ChartScene {
            exposure: 0.5,
            ..scene.clone()
        },
    )
    .unwrap();
    let dark = check_chart(
        &cache,
        profile.clone(),
        dark.to_string_lossy().into_owned(),
        quad(),
    )
    .unwrap();
    let dark = serde_json::to_value(&dark).unwrap();

    let same_mean = same["validation"]["meanDeltaE"].as_f64().unwrap();
    let dark_mean = dark["validation"]["meanDeltaE"].as_f64().unwrap();
    assert!(dark_mean > same_mean + 1.0, "{same_mean} vs {dark_mean}");
    assert_eq!(
        dark["profileFitBaseline"]["meanDeltaE"], same["profileFitBaseline"]["meanDeltaE"],
        "the stored fit is reported unchanged"
    );
    assert_eq!(
        std::fs::read(&profile).unwrap(),
        before,
        "profile untouched"
    );
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn chart_check_without_corners_refuses_a_frame_where_no_chart_is_found() {
    let work = temp_dir("detect");
    let (profile, _, _) = derive(&work);
    let blank = work.join("blank.dng");
    render_chart_dng(
        &blank,
        &ChartScene {
            exposure: 0.0,
            ..scene(1.0)
        },
    )
    .unwrap();
    let error = check_chart(
        &ReferenceCache::disabled(),
        profile,
        blank.to_string_lossy().into_owned(),
        None,
    )
    .unwrap_err();
    assert!(
        format!("{error:?}").contains("select its four corners"),
        "{error:?}"
    );
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn preflight_marks_files_without_exif_as_unknown_and_never_matched() {
    let work = temp_dir("preflight");
    let (profile, _, scene) = derive(&work);
    let batch = work.join("batch");
    std::fs::create_dir_all(&batch).unwrap();
    render_chart_dng(&batch.join("a.dng"), &scene).unwrap();
    render_chart_dng(&batch.join("b.dng"), &scene).unwrap();

    let result = scan_batch(batch.to_string_lossy().into_owned(), None, profile).unwrap();
    let result = serde_json::to_value(&result).unwrap();
    assert_eq!(result["totalFiles"], 2);
    for file in result["files"].as_array().unwrap() {
        assert!(file["lensMismatch"].is_null(), "{file}");
        assert!(file["isoMismatch"].is_null(), "{file}");
        assert!(file["captureTimeDifferent"].is_null(), "{file}");
    }
    assert!(result["activeLibraryEntry"].is_null());
    let _ = std::fs::remove_dir_all(work);
}
