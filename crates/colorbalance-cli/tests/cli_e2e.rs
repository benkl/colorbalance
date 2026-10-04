use std::fs;
use std::path::PathBuf;

use colorbalance_fixtures::{render_chart_dng, ChartScene, SceneDefect};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "colorbalance-e2e-{name}-{}-{}",
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
fn end_to_end_fixture_derive_apply_and_interchange() {
    let work = temp_dir("full");
    let ref_dng = work.join("reference.dng");
    let shoot_dir = work.join("shoot");
    fs::create_dir_all(&shoot_dir).unwrap();

    let scene_clean = ChartScene::default();
    render_chart_dng(&ref_dng, &scene_clean).unwrap();

    let shot1 = shoot_dir.join("shot_01.dng");
    let shot2 = shoot_dir.join("shot_02.dng");
    render_chart_dng(&shot1, &scene_clean).unwrap();
    render_chart_dng(&shot2, &scene_clean).unwrap();

    let profile_path = work.join("studio.cbprofile.json");
    let report_path = work.join("studio-report.html");
    let out_dir = work.join("balanced");
    let clf_path = work.join("studio.clf");
    let cube_path = work.join("studio.cube");

    let bin = env!("CARGO_BIN_EXE_colorbalance");
    let quad_str = format!(
        "{},{},{},{},{},{},{},{}",
        scene_clean.quad[0][0],
        scene_clean.quad[0][1],
        scene_clean.quad[1][0],
        scene_clean.quad[1][1],
        scene_clean.quad[2][0],
        scene_clean.quad[2][1],
        scene_clean.quad[3][0],
        scene_clean.quad[3][1],
    );

    // 1. inspect
    let inspect_out = std::process::Command::new(bin)
        .arg("inspect")
        .arg(&ref_dng)
        .arg("--chart")
        .arg("classic-before-nov-2014")
        .arg("--quad")
        .arg(&quad_str)
        .output()
        .unwrap();
    assert!(
        inspect_out.status.success(),
        "inspect command must succeed: {}",
        String::from_utf8_lossy(&inspect_out.stderr)
    );
    let inspect_json: serde_json::Value = serde_json::from_slice(&inspect_out.stdout).unwrap();
    if inspect_json["quality-passed"] != true {
        let _ = fs::write(
            std::env::temp_dir().join("cb-e2e-inspect.json"),
            serde_json::to_string_pretty(&inspect_json).unwrap(),
        );
        panic!("inspect quality did not pass: {inspect_json:#}");
    }

    // 2. derive
    let derive_out = std::process::Command::new(bin)
        .arg("derive")
        .arg(&ref_dng)
        .arg("--chart")
        .arg("classic-before-nov-2014")
        .arg("--quad")
        .arg(&quad_str)
        .arg("--profile")
        .arg(&profile_path)
        .arg("--report")
        .arg(&report_path)
        .output()
        .unwrap();
    assert!(
        derive_out.status.success(),
        "derive must succeed on clean reference: {}",
        String::from_utf8_lossy(&derive_out.stderr)
    );
    assert!(profile_path.exists());
    assert!(report_path.exists());

    // 3. apply
    let summary_path = work.join("summary.json");
    let apply_out = std::process::Command::new(bin)
        .arg("apply")
        .arg(&profile_path)
        .arg(&shoot_dir)
        .arg("--output")
        .arg(&out_dir)
        .arg("--summary")
        .arg(&summary_path)
        .output()
        .unwrap();
    if !apply_out.status.success() {
        let stderr_str = String::from_utf8_lossy(&apply_out.stderr).to_string();
        let stdout_str = String::from_utf8_lossy(&apply_out.stdout).to_string();
        let _ = std::fs::copy(
            &profile_path,
            std::env::temp_dir().join("cb-failed-profile.json"),
        );
        std::fs::write(
            std::env::temp_dir().join("cb-apply-fail.txt"),
            format!("stdout:\n{stdout_str}\nstderr:\n{stderr_str}"),
        )
        .unwrap();
        panic!("apply failed: {stderr_str}");
    }
    assert!(out_dir.join("shot_02.tiff").exists());

    let sum_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_path).unwrap()).unwrap();
    assert_eq!(sum_json["total"], 2);
    assert_eq!(sum_json["succeeded"].as_array().unwrap().len(), 2);
    assert_eq!(sum_json["failed"].as_array().unwrap().len(), 0);

    // 4. export clf
    let clf_out = std::process::Command::new(bin)
        .arg("export")
        .arg(&profile_path)
        .arg("--format")
        .arg("clf")
        .arg("--output")
        .arg(&clf_path)
        .output()
        .unwrap();
    assert!(clf_out.status.success());
    assert!(clf_path.exists());
    let clf_text = fs::read_to_string(&clf_path).unwrap();
    assert!(clf_text.contains("<ProcessList"));

    // 5. export cube
    let cube_out = std::process::Command::new(bin)
        .arg("export")
        .arg(&profile_path)
        .arg("--format")
        .arg("cube")
        .arg("--size")
        .arg("17")
        .arg("--output")
        .arg(&cube_path)
        .output()
        .unwrap();
    assert!(cube_out.status.success());
    assert!(cube_path.exists());
    let cube_text = fs::read_to_string(&cube_path).unwrap();
    assert!(cube_text.contains("LUT_3D_SIZE 17"));

    let _ = fs::remove_dir_all(work);
}

#[test]
fn derive_fails_on_clipped_reference_without_force() {
    let work = temp_dir("clipped");
    let ref_dng = work.join("clipped_reference.dng");
    let scene_clipped = ChartScene {
        defect: SceneDefect::ClipWhitePatch,
        ..ChartScene::default()
    };
    render_chart_dng(&ref_dng, &scene_clipped).unwrap();

    let profile_path = work.join("should_not_exist.cbprofile.json");
    let bin = env!("CARGO_BIN_EXE_colorbalance");

    let derive_out = std::process::Command::new(bin)
        .arg("derive")
        .arg(&ref_dng)
        .arg("--profile")
        .arg(&profile_path)
        .output()
        .unwrap();
    assert!(
        !derive_out.status.success(),
        "derive must fail without --force on clipped reference"
    );
    assert!(
        !profile_path.exists(),
        "no profile should be written on unforced failure"
    );

    let _ = fs::remove_dir_all(work);
}

#[test]
fn apply_skips_existing_output_without_overwrite_flag() {
    let work = temp_dir("skip");
    let ref_dng = work.join("ref.dng");
    let scene = ChartScene::default();
    render_chart_dng(&ref_dng, &scene).unwrap();

    let profile_path = work.join("p.json");
    let bin = env!("CARGO_BIN_EXE_colorbalance");
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
    let derive_res = std::process::Command::new(bin)
        .arg("derive")
        .arg(&ref_dng)
        .arg("--chart")
        .arg("classic-before-nov-2014")
        .arg("--quad")
        .arg(&quad_str)
        .arg("-p")
        .arg(&profile_path)
        .output()
        .unwrap();
    assert!(
        derive_res.status.success(),
        "derive must succeed: {}",
        String::from_utf8_lossy(&derive_res.stderr)
    );

    let shoot = work.join("shoot");
    fs::create_dir_all(&shoot).unwrap();
    let shot = shoot.join("one.dng");
    render_chart_dng(&shot, &scene).unwrap();

    let out_dir = work.join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let pre_existing = out_dir.join("one.tiff");
    fs::write(&pre_existing, b"original-unmodified-content").unwrap();

    let summary_path = work.join("sum.json");
    let apply_out = std::process::Command::new(bin)
        .arg("apply")
        .arg(&profile_path)
        .arg(&shoot)
        .arg("-o")
        .arg(&out_dir)
        .arg("--summary")
        .arg(&summary_path)
        .output()
        .unwrap();
    assert!(apply_out.status.success());
    assert_eq!(
        fs::read(&pre_existing).unwrap(),
        b"original-unmodified-content"
    );

    let sum_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_path).unwrap()).unwrap();
    assert_eq!(sum_json["skipped"].as_array().unwrap().len(), 1);
    assert_eq!(sum_json["succeeded"].as_array().unwrap().len(), 0);

    let _ = fs::remove_dir_all(work);
}
