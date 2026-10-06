//! The React UI reads the `derive_profile` response by field name. These tests run
//! the real command and pin that JSON shape, so a Rust-side rename or a missing
//! field fails here instead of surfacing as `undefined.toFixed` in the window.

use colorbalance_desktop::commands::{derive_profile, load_reference};
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
        false,
        false,
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

    let loaded = load_reference(reference.to_string_lossy().into_owned()).expect("loads");
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
