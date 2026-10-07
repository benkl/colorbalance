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
    let overlay_path = work.join("studio-overlay.svg");
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
        .arg("--overlay")
        .arg(&overlay_path)
        .output()
        .unwrap();
    assert!(
        derive_out.status.success(),
        "derive must succeed on clean reference: {}",
        String::from_utf8_lossy(&derive_out.stderr)
    );
    assert!(profile_path.exists());
    assert!(report_path.exists());
    assert!(overlay_path.exists());
    let overlay_svg = fs::read_to_string(&overlay_path).unwrap();
    assert!(overlay_svg.contains("<svg"));
    assert!(overlay_svg.contains("<polygon"));
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

#[test]
fn batch_handles_good_corrupt_existing_and_duplicate_names() {
    let work = temp_dir("batch-mixed");
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
        .arg("--profile")
        .arg(&profile_path)
        .output()
        .unwrap();
    assert!(
        derive_res.status.success(),
        "derive: {}",
        String::from_utf8_lossy(&derive_res.stderr)
    );

    // Input tree: two good files (duplicated names across subdirs), one corrupt
    // file, one file whose output already exists.
    let shoot = work.join("shoot");
    let sub_a = shoot.join("a");
    let sub_b = shoot.join("b");
    fs::create_dir_all(&sub_a).unwrap();
    fs::create_dir_all(&sub_b).unwrap();
    render_chart_dng(&sub_a.join("dup.dng"), &scene).unwrap();
    render_chart_dng(&sub_b.join("dup.dng"), &scene).unwrap();
    render_chart_dng(&shoot.join("good.dng"), &scene).unwrap();
    fs::write(shoot.join("corrupt.dng"), b"not a DNG file").unwrap();

    let out_dir = work.join("out");
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(out_dir.join("good.tiff"), b"original-unmodified-content").unwrap();

    let summary_path = work.join("summary.json");
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
    // One file failed, so the command exits non-zero, but the summary must
    // still be written and the completed files must remain valid.
    assert!(!apply_out.status.success());

    let sum_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_path).unwrap()).unwrap();
    assert_eq!(sum_json["total"], 4);
    assert_eq!(sum_json["succeeded"].as_array().unwrap().len(), 2);
    assert_eq!(sum_json["skipped"].as_array().unwrap().len(), 1);
    assert_eq!(sum_json["failed"].as_array().unwrap().len(), 1);

    // Collision-safe names: the two duplicate-stem inputs produce distinct outputs.
    assert!(out_dir.join("dup.tiff").exists());
    assert!(out_dir.join("dup-1.tiff").exists());
    // The skipped output is untouched.
    assert_eq!(
        fs::read(out_dir.join("good.tiff")).unwrap(),
        b"original-unmodified-content"
    );
    // No unfinished temporary files remain in the output directory.
    let temp_files: Vec<_> = fs::read_dir(&out_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".tmp-"))
        .collect();
    assert!(
        temp_files.is_empty(),
        "stale temp files remain: {temp_files:?}"
    );

    // Stale temp files from a crashed run are cleaned on the next run.
    fs::write(out_dir.join(".tmp-99999-1.tiff"), b"stale").unwrap();
    let apply_out2 = std::process::Command::new(bin)
        .arg("apply")
        .arg(&profile_path)
        .arg(&shoot)
        .arg("-o")
        .arg(&out_dir)
        .arg("--overwrite")
        .arg("--summary")
        .arg(&summary_path)
        .output()
        .unwrap();
    assert!(!apply_out2.status.success());
    let leftover: Vec<_> = fs::read_dir(&out_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".tmp-"))
        .collect();
    assert!(
        leftover.is_empty(),
        "stale temp files not cleaned: {leftover:?}"
    );

    let _ = fs::remove_dir_all(work);
}

/// The embedded ICC profile of a little-endian TIFF written by `apply`.
fn tiff_icc_profile(path: &std::path::Path) -> Vec<u8> {
    let tiff = fs::read(path).unwrap();
    let ifd = u32::from_le_bytes(tiff[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(tiff[ifd..ifd + 2].try_into().unwrap()) as usize;
    for entry in 0..count {
        let at = ifd + 2 + entry * 12;
        if u16::from_le_bytes(tiff[at..at + 2].try_into().unwrap()) == 34675 {
            let len = u32::from_le_bytes(tiff[at + 4..at + 8].try_into().unwrap()) as usize;
            let offset = u32::from_le_bytes(tiff[at + 8..at + 12].try_into().unwrap()) as usize;
            return tiff[offset..offset + len].to_vec();
        }
    }
    panic!("{} has no ICC profile tag", path.display());
}

#[test]
fn apply_output_space_embeds_matching_icc_and_unknown_space_fails_closed() {
    let work = temp_dir("output-space");
    let reference = work.join("reference.dng");
    let shoot = work.join("shoot");
    fs::create_dir_all(&shoot).unwrap();
    let scene = ChartScene::default();
    render_chart_dng(&reference, &scene).unwrap();
    render_chart_dng(&shoot.join("shot.dng"), &scene).unwrap();
    let quad = scene
        .quad
        .iter()
        .flatten()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let bin = env!("CARGO_BIN_EXE_colorbalance");
    let profile = work.join("p.cbprofile.json");
    let derive = std::process::Command::new(bin)
        .args(["derive"])
        .arg(&reference)
        .args(["--chart", "classic-before-nov-2014", "--quad", &quad])
        .arg("--profile")
        .arg(&profile)
        .arg("--report")
        .arg(work.join("r.html"))
        .output()
        .unwrap();
    assert!(
        derive.status.success(),
        "{}",
        String::from_utf8_lossy(&derive.stderr)
    );

    let apply = |space: &str, out: &std::path::Path| {
        std::process::Command::new(bin)
            .arg("apply")
            .arg(&profile)
            .arg(&shoot)
            .arg("--output")
            .arg(out)
            .arg("--summary")
            .arg(out.join("summary.json"))
            .args(["--output-space", space])
            .output()
            .unwrap()
    };

    let mut icc = Vec::new();
    for (space, config_recorded) in [("srgb", false), ("display-p3", true), ("adobe-rgb", true)] {
        let out = work.join(space);
        let run = apply(space, &out);
        assert!(
            run.status.success(),
            "{space}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        let summary: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
        assert_eq!(summary["output-space"], space);
        assert_eq!(
            summary["ocio-config"].is_string(),
            config_recorded,
            "{space}"
        );
        assert_eq!(
            summary["ocio-version"].is_string(),
            config_recorded,
            "{space}"
        );
        let profile_bytes = tiff_icc_profile(&out.join("shot.tiff"));
        assert_eq!(&profile_bytes[36..40], b"acsp", "{space}");
        icc.push(profile_bytes);
    }
    assert!(icc[0] != icc[1] && icc[1] != icc[2] && icc[0] != icc[2]);

    let bogus = work.join("bogus");
    let run = apply("rec2020", &bogus);
    assert!(!run.status.success(), "unknown space must fail");
    assert!(
        !bogus.exists(),
        "nothing is written before the space is resolved"
    );
    assert!(String::from_utf8_lossy(&run.stderr).contains("rec2020"));

    let _ = fs::remove_dir_all(work);
}
