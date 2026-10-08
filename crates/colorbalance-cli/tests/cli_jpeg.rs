use std::fs;
use std::path::PathBuf;

use colorbalance_fixtures::{render_chart_dng, ChartScene};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "colorbalance-jpeg-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn quick_and_dirty_jpeg_derive_and_apply_workflow() {
    let work = temp_dir("quick_dirty");
    let ref_dng = work.join("ref_temp.dng");
    let ref_jpeg = work.join("stinky_reference.jpg");
    let shoot_dir = work.join("shoot");
    fs::create_dir_all(&shoot_dir).unwrap();

    // 1. Render a chart DNG, decode it, and save it as a compressed JPEG to model a stinky source
    let scene = ChartScene::default();
    render_chart_dng(&ref_dng, &scene).unwrap();
    let decoded = colorbalance_raw::decode_raw(&ref_dng).unwrap();
    let mut img_buf = image::RgbImage::new(decoded.width, decoded.height);
    for y in 0..decoded.height {
        for x in 0..decoded.width {
            let rgb = decoded.rgb_at(x, y);
            // Gamma encode roughly to sRGB 8-bit
            let r = (colorbalance_core::color::srgb_encode(f64::from(rgb[0])) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            let g = (colorbalance_core::color::srgb_encode(f64::from(rgb[1])) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            let b = (colorbalance_core::color::srgb_encode(f64::from(rgb[2])) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            img_buf.put_pixel(x, y, image::Rgb([r, g, b]));
        }
    }
    img_buf.save(&ref_jpeg).unwrap();

    let shot_jpg1 = shoot_dir.join("shot_01.jpg");
    let shot_jpg2 = shoot_dir.join("shot_02.jpg");
    img_buf.save(&shot_jpg1).unwrap();
    img_buf.save(&shot_jpg2).unwrap();

    let bin = env!("CARGO_BIN_EXE_colorbalance");
    let profile_path = work.join("quick_dirty.cbprofile.json");
    let report_path = work.join("quick_dirty_report.html");
    let out_dir = work.join("balanced_out");
    let summary_path = work.join("batch_summary.json");

    let quad_str = format!(
        "{},{},{},{},{},{},{},{}",
        scene.quad[0][0],
        scene.quad[0][1],
        scene.quad[1][0],
        scene.quad[1][1],
        scene.quad[2][0],
        scene.quad[2][1],
        scene.quad[3][0],
        scene.quad[3][1],
    );

    // 2. Inspect in quick-and-dirty mode
    let inspect_out = std::process::Command::new(bin)
        .arg("inspect")
        .arg(&ref_jpeg)
        .arg("--chart")
        .arg("classic-before-nov-2014")
        .arg("--quad")
        .arg(&quad_str)
        .arg("--quick-and-dirty")
        .output()
        .unwrap();
    assert!(
        inspect_out.status.success(),
        "inspect on jpeg failed: {}",
        String::from_utf8_lossy(&inspect_out.stderr)
    );

    // 3. Derive profile in quick-and-dirty mode
    let derive_out = std::process::Command::new(bin)
        .arg("derive")
        .arg(&ref_jpeg)
        .arg("--chart")
        .arg("classic-before-nov-2014")
        .arg("--quad")
        .arg(&quad_str)
        .arg("-p")
        .arg(&profile_path)
        .arg("-r")
        .arg(&report_path)
        .arg("--quick-and-dirty")
        .output()
        .unwrap();
    assert!(
        derive_out.status.success(),
        "derive on jpeg failed: {}",
        String::from_utf8_lossy(&derive_out.stderr)
    );
    assert!(profile_path.exists());
    assert!(report_path.exists());

    // 4. Batch apply to the jpeg directory
    let apply_out = std::process::Command::new(bin)
        .arg("apply")
        .arg(&profile_path)
        .arg(&shoot_dir)
        .arg("-o")
        .arg(&out_dir)
        .arg("--summary")
        .arg(&summary_path)
        .output()
        .unwrap();
    assert!(
        apply_out.status.success(),
        "apply on jpeg batch failed: {}",
        String::from_utf8_lossy(&apply_out.stderr)
    );
    assert!(out_dir.join("shot_01.tiff").exists());
    assert!(out_dir.join("shot_02.tiff").exists());

    let sum_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_path).unwrap()).unwrap();
    assert_eq!(sum_json["total"], 2);
    assert_eq!(sum_json["succeeded"].as_array().unwrap().len(), 2);
    assert_eq!(sum_json["failed"].as_array().unwrap().len(), 0);
    assert!(sum_json["warnings"].as_array().unwrap().is_empty());

    // 5. A DCP is RAW-only: a rendered-image profile must be refused, no file written
    let dcp_path = work.join("quick_dirty.dcp");
    let dcp_out = std::process::Command::new(bin)
        .arg("export")
        .arg(&profile_path)
        .arg("--format")
        .arg("dcp")
        .arg("--camera-name")
        .arg("Any Camera")
        .arg("--output")
        .arg(&dcp_path)
        .output()
        .unwrap();
    assert!(!dcp_out.status.success());
    assert!(!dcp_path.exists());
    assert!(String::from_utf8_lossy(&dcp_out.stderr).contains("quick-and-dirty"));

    let original = fs::read_to_string(&profile_path).unwrap();
    let mut profile = colorbalance_core::profile::from_json(&original).unwrap();
    profile.decode_contract.decoder_version = "older-rendered-decoder".to_owned();
    profile.camera.decoder_version = "older-rendered-decoder".to_owned();
    profile.digest = colorbalance_core::profile::digest(&profile);
    fs::write(&profile_path, colorbalance_core::profile::to_json(&profile)).unwrap();

    let drift_summary = work.join("drift_summary.json");
    let drift_out = std::process::Command::new(bin)
        .arg("apply")
        .arg(&profile_path)
        .arg(&shot_jpg1)
        .arg("-o")
        .arg(work.join("drift_output"))
        .arg("--summary")
        .arg(&drift_summary)
        .output()
        .unwrap();
    assert!(drift_out.status.success());
    let drift: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&drift_summary).unwrap()).unwrap();
    assert_eq!(drift["warnings"].as_array().unwrap().len(), 1);
    assert!(drift["warnings"][0]["warning"]
        .as_str()
        .unwrap()
        .contains("decoder version differs"));

    profile.decode_contract.decoder = "libraw".to_owned();
    profile.camera.decoder = "libraw".to_owned();
    profile.digest = colorbalance_core::profile::digest(&profile);
    fs::write(&profile_path, colorbalance_core::profile::to_json(&profile)).unwrap();
    let mismatch_summary = work.join("mismatch_summary.json");
    let mismatch_out = std::process::Command::new(bin)
        .arg("apply")
        .arg(&profile_path)
        .arg(&shot_jpg1)
        .arg("-o")
        .arg(work.join("mismatch_output"))
        .arg("--summary")
        .arg(&mismatch_summary)
        .output()
        .unwrap();
    assert!(!mismatch_out.status.success());
    let mismatch: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&mismatch_summary).unwrap()).unwrap();
    assert!(mismatch["failed"][0]["error"]
        .as_str()
        .unwrap()
        .contains("decoder mismatch"));
    assert!(mismatch["warnings"].as_array().unwrap().is_empty());

    let forced_out = std::process::Command::new(bin)
        .arg("apply")
        .arg(&profile_path)
        .arg(&shot_jpg1)
        .arg("-o")
        .arg(work.join("forced_output"))
        .arg("--force")
        .arg("--summary")
        .arg(work.join("forced_summary.json"))
        .output()
        .unwrap();
    assert!(forced_out.status.success());
    let forced: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(work.join("forced_summary.json")).unwrap())
            .unwrap();
    assert!(forced["warnings"][0]["warning"]
        .as_str()
        .unwrap()
        .contains("forced decode-contract mismatch"));

    let _ = fs::remove_dir_all(work);
}
