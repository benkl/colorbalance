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

    // A temp file owned by another run is never swept.
    fs::write(out_dir.join(".tmp-99999-1.tiff"), b"foreign").unwrap();
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
    assert_eq!(
        fs::read(out_dir.join(".tmp-99999-1.tiff")).unwrap(),
        b"foreign"
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

// Keep this reader separate from the export code: these assertions inspect the
// bytes a photo viewer receives, not the metadata selected before encoding.
fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn tiff_tag(bytes: &[u8], ifd: usize, tag: u16) -> Option<Vec<u8>> {
    assert_eq!(&bytes[..4], b"II*\0");
    let count = le16(bytes, ifd) as usize;
    for i in 0..count {
        let at = ifd + 2 + i * 12;
        if le16(bytes, at) != tag {
            continue;
        }
        let size = match le16(bytes, at + 2) {
            1 | 2 | 7 => 1,
            3 => 2,
            4 | 9 => 4,
            5 | 10 => 8,
            other => panic!("unexpected TIFF type {other} for tag {tag}"),
        };
        let len = le32(bytes, at + 4) as usize * size;
        let start = if len <= 4 {
            at + 8
        } else {
            le32(bytes, at + 8) as usize
        };
        return Some(bytes[start..start + len].to_vec());
    }
    None
}

fn tiff_nested_tag(bytes: &[u8], parent: u16, tag: u16) -> Option<Vec<u8>> {
    let root = le32(bytes, 4) as usize;
    let pointer = tiff_tag(bytes, root, parent)?;
    tiff_tag(bytes, le32(&pointer, 0) as usize, tag)
}

// A tiny, standards-shaped EXIF source. Every payload is generated here so the
// test doesn't depend on a camera file or a parser used by the implementation.
fn exif_source() -> Vec<u8> {
    fn ifd(bytes: &mut Vec<u8>, entries: &[(u16, u16, Vec<u8>)]) -> usize {
        let start = bytes.len();
        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        bytes.resize(bytes.len() + entries.len() * 12 + 4, 0);
        for (index, (tag, kind, data)) in entries.iter().enumerate() {
            let at = start + 2 + index * 12;
            bytes[at..at + 2].copy_from_slice(&tag.to_le_bytes());
            bytes[at + 2..at + 4].copy_from_slice(&kind.to_le_bytes());
            let unit = match kind {
                3 => 2,
                5 => 8,
                4 => 4,
                _ => 1,
            };
            bytes[at + 4..at + 8].copy_from_slice(&((data.len() / unit) as u32).to_le_bytes());
            if data.len() <= 4 {
                bytes[at + 8..at + 8 + data.len()].copy_from_slice(data);
            } else {
                let offset = bytes.len() as u32;
                bytes[at + 8..at + 12].copy_from_slice(&offset.to_le_bytes());
                bytes.extend_from_slice(data);
                if !bytes.len().is_multiple_of(2) {
                    bytes.push(0);
                }
            }
        }
        start
    }
    let mut exif = b"II*\0\x08\0\0\0".to_vec();
    let root = ifd(
        &mut exif,
        &[
            (0x010f, 2, b"Test Camera\0".to_vec()),
            (0x0110, 2, b"Model 42\0".to_vec()),
            (0x0112, 3, 6u16.to_le_bytes().to_vec()),
            (0x013b, 2, b"Ada Tester\0".to_vec()),
            (0x8769, 4, vec![0; 4]),
            (0x8825, 4, vec![0; 4]),
        ],
    );
    let exif_ifd = ifd(
        &mut exif,
        &[
            (0x9003, 2, b"2025:06:14 12:34:56\0".to_vec()),
            (0x927c, 7, b"private maker note".to_vec()),
            (0xa001, 3, 1u16.to_le_bytes().to_vec()),
            (0xa403, 3, 1u16.to_le_bytes().to_vec()),
        ],
    );
    let coordinate = |degrees: u32| {
        [(degrees, 1u32), (30, 1), (0, 1)]
            .into_iter()
            .flat_map(|(n, d)| [n.to_le_bytes(), d.to_le_bytes()].concat())
            .collect::<Vec<_>>()
    };
    let gps_ifd = ifd(
        &mut exif,
        &[
            (1, 2, b"N\0".to_vec()),
            (2, 5, coordinate(51)),
            (3, 2, b"W\0".to_vec()),
            (4, 5, coordinate(0)),
        ],
    );
    for (tag, offset) in [(0x8769, exif_ifd), (0x8825, gps_ifd)] {
        let count = le16(&exif, root) as usize;
        let at = (0..count)
            .map(|i| root + 2 + i * 12)
            .find(|&at| le16(&exif, at) == tag)
            .unwrap();
        exif[at + 8..at + 12].copy_from_slice(&(offset as u32).to_le_bytes());
    }
    exif
}

fn jpeg_segment(jpeg: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    jpeg.extend_from_slice(&[0xff, marker]);
    jpeg.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    jpeg.extend_from_slice(payload);
}

fn jpeg_segments(bytes: &[u8]) -> Vec<(u8, &[u8])> {
    assert_eq!(&bytes[..2], b"\xff\xd8");
    let mut at = 2;
    let mut segments = Vec::new();
    while at + 4 <= bytes.len() && bytes[at] == 0xff {
        let marker = bytes[at + 1];
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        let length = u16::from_be_bytes(bytes[at + 2..at + 4].try_into().unwrap()) as usize;
        segments.push((marker, &bytes[at + 4..at + 2 + length]));
        at += 2 + length;
    }
    segments
}

fn jpeg_exif(bytes: &[u8]) -> &[u8] {
    jpeg_segments(bytes)
        .into_iter()
        .find_map(|(marker, payload)| {
            (marker == 0xe1 && payload.starts_with(b"Exif\0\0")).then_some(&payload[6..])
        })
        .expect("JPEG EXIF APP1")
}

fn tagged_jpeg_source(path: &std::path::Path) {
    let reference = path.with_extension("dng");
    render_chart_dng(&reference, &ChartScene::default()).unwrap();
    let decoded = colorbalance_raw::decode_raw(&reference).unwrap();
    let mut image = image::RgbImage::new(decoded.width, decoded.height);
    for y in 0..decoded.height {
        for x in 0..decoded.width {
            let rgb = decoded.rgb_at(x, y);
            image.put_pixel(
                x,
                y,
                image::Rgb(rgb.map(|channel| {
                    (colorbalance_core::color::srgb_encode(f64::from(channel)) * 255.0)
                        .round()
                        .clamp(0.0, 255.0) as u8
                })),
            );
        }
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
        .encode_image(&image::DynamicImage::ImageRgb8(image))
        .unwrap();
    let mut tagged = jpeg[..2].to_vec();
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend(exif_source());
    jpeg_segment(&mut tagged, 0xe1, &exif);
    jpeg_segment(
        &mut tagged,
        0xe1,
        b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta xmlns:x='adobe:ns:meta/'>test-packet</x:xmpmeta>",
    );
    let iim = b"\x1c\x02\x05\0\x0bTestCaption";
    let mut photoshop = b"Photoshop 3.0\0\x38BIM\x04\x04\0\0".to_vec();
    photoshop.extend_from_slice(&(iim.len() as u32).to_be_bytes());
    photoshop.extend_from_slice(iim);
    if !iim.len().is_multiple_of(2) {
        photoshop.push(0);
    }
    jpeg_segment(&mut tagged, 0xed, &photoshop);
    tagged.extend_from_slice(&jpeg[2..]);
    fs::write(path, tagged).unwrap();
    fs::remove_file(reference).unwrap();
}

fn export_tiff_metadata(bytes: &[u8], gps: bool, adjuncts: bool) {
    let root = le32(bytes, 4) as usize;
    assert_eq!(tiff_tag(bytes, root, 0x010f).unwrap(), b"Test Camera\0");
    assert_eq!(tiff_tag(bytes, root, 0x0110).unwrap(), b"Model 42\0");
    assert_eq!(tiff_tag(bytes, root, 0x013b).unwrap(), b"Ada Tester\0");
    assert_eq!(
        tiff_nested_tag(bytes, 0x8769, 0x9003).unwrap(),
        b"2025:06:14 12:34:56\0"
    );
    assert!(
        tiff_tag(bytes, root, 0x0112).is_none_or(|orientation| orientation == 1u16.to_le_bytes())
    );
    for tag in [0x927c, 0xa001, 0xa403] {
        assert!(
            tiff_nested_tag(bytes, 0x8769, tag).is_none(),
            "unsafe EXIF tag {tag:#x}"
        );
    }
    let gps_pointer = tiff_tag(bytes, root, 0x8825);
    assert_eq!(gps_pointer.is_some(), gps);
    if gps {
        assert_eq!(tiff_nested_tag(bytes, 0x8825, 1).unwrap(), b"N\0");
        assert!(tiff_nested_tag(bytes, 0x8825, 2).is_some());
    }
    assert_eq!(tiff_tag(bytes, root, 700).is_some(), adjuncts);
    assert_eq!(tiff_tag(bytes, root, 33723).is_some(), adjuncts);
    if adjuncts {
        assert!(tiff_tag(bytes, root, 700)
            .unwrap()
            .windows(11)
            .any(|w| w == b"test-packet"));
        assert!(tiff_tag(bytes, root, 33723)
            .unwrap()
            .windows(11)
            .any(|w| w == b"TestCaption"));
    }
    assert!(tiff_tag(bytes, root, 34675).is_some(), "ICC missing");
    let width = le32(&tiff_tag(bytes, root, 256).unwrap(), 0) as usize;
    let height = le32(&tiff_tag(bytes, root, 257).unwrap(), 0) as usize;
    let strip_at = le32(&tiff_tag(bytes, root, 273).unwrap(), 0) as usize;
    let strip_len = le32(&tiff_tag(bytes, root, 279).unwrap(), 0) as usize;
    assert!(width > 0 && height > 0);
    assert_eq!(strip_len, width * height * 3 * 2);
    let pixels = &bytes[strip_at..strip_at + strip_len];
    assert!(pixels
        .as_chunks::<2>()
        .0
        .iter()
        .any(|sample| sample != &[0, 0]));
    assert_eq!(tiff_tag(bytes, root, 259).unwrap(), 1u16.to_le_bytes());
    assert_eq!(
        tiff_tag(bytes, root, 258).unwrap(),
        [16u16.to_le_bytes(); 3].concat()
    );
}

fn export_jpeg_metadata(bytes: &[u8], gps: bool, adjuncts: bool, sampling: &[u8; 3]) {
    let segments = jpeg_segments(bytes);
    let exif = jpeg_exif(bytes);
    let root = le32(exif, 4) as usize;
    assert_eq!(tiff_tag(exif, root, 0x010f).unwrap(), b"Test Camera\0");
    assert_eq!(tiff_tag(exif, root, 0x0110).unwrap(), b"Model 42\0");
    assert_eq!(tiff_tag(exif, root, 0x013b).unwrap(), b"Ada Tester\0");
    assert_eq!(
        tiff_nested_tag(exif, 0x8769, 0x9003).unwrap(),
        b"2025:06:14 12:34:56\0"
    );
    assert!(
        tiff_tag(exif, root, 0x0112).is_none_or(|orientation| orientation == 1u16.to_le_bytes())
    );
    for tag in [0x927c, 0xa001, 0xa403] {
        assert!(
            tiff_nested_tag(exif, 0x8769, tag).is_none(),
            "unsafe EXIF tag {tag:#x}"
        );
    }
    assert_eq!(tiff_tag(exif, root, 0x8825).is_some(), gps);
    if gps {
        assert!(tiff_nested_tag(exif, 0x8825, 2).is_some());
    }
    let xmp = segments.iter().find(|(marker, data)| {
        *marker == 0xe1 && data.starts_with(b"http://ns.adobe.com/xap/1.0/\0")
    });
    let iptc = segments
        .iter()
        .find(|(marker, data)| *marker == 0xed && data.starts_with(b"Photoshop 3.0\0"));
    assert_eq!(xmp.is_some(), adjuncts);
    assert_eq!(iptc.is_some(), adjuncts);
    if adjuncts {
        assert!(xmp.unwrap().1.windows(11).any(|w| w == b"test-packet"));
        assert!(iptc.unwrap().1.windows(11).any(|w| w == b"TestCaption"));
    }
    assert!(
        segments
            .iter()
            .any(|(marker, data)| *marker == 0xe2 && data.starts_with(b"ICC_PROFILE\0")),
        "ICC APP2 missing"
    );
    let sof = segments
        .iter()
        .find(|(marker, _)| matches!(*marker, 0xc0..=0xc2))
        .unwrap()
        .1;
    assert_eq!(sof[5], 3, "expected three JPEG components");
    assert_eq!(&[sof[7], sof[10], sof[13]], sampling);
}

#[test]
fn metadata_export_preserves_reviewed_fields_and_respects_choices() {
    let work = temp_dir("metadata-export");
    let scene = ChartScene::default();
    let reference = work.join("reference.dng");
    render_chart_dng(&reference, &scene).unwrap();
    let source = work.join("source.jpg");
    tagged_jpeg_source(&source);
    let quad = scene
        .quad
        .iter()
        .flatten()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let profile = work.join("profile.json");
    let bin = env!("CARGO_BIN_EXE_colorbalance");
    let derive = std::process::Command::new(bin)
        .arg("derive")
        .arg(&reference)
        .args(["--chart", "classic-before-nov-2014", "--quad", &quad])
        .arg("--profile")
        .arg(&profile)
        .output()
        .unwrap();
    assert!(
        derive.status.success(),
        "{}",
        String::from_utf8_lossy(&derive.stderr)
    );

    for (label, format, quality, subsampling, expected_sampling, adjuncts, strip_gps) in [
        (
            "tiff",
            "tiff",
            "95",
            "444",
            [0x11, 0x11, 0x11],
            false,
            false,
        ),
        (
            "jpeg-444",
            "jpeg",
            "73",
            "444",
            [0x11, 0x11, 0x11],
            false,
            false,
        ),
        (
            "jpeg-422",
            "jpeg",
            "82",
            "422",
            [0x21, 0x11, 0x11],
            true,
            false,
        ),
        (
            "jpeg-420",
            "jpeg",
            "100",
            "420",
            [0x22, 0x11, 0x11],
            true,
            true,
        ),
    ] {
        let out_dir = work.join(label);
        let summary = work.join(format!("{label}.json"));
        let mut apply = std::process::Command::new(bin);
        apply
            .arg("apply")
            .arg(&profile)
            .arg(&source)
            .arg("-o")
            .arg(&out_dir)
            .args([
                "--format",
                format,
                "--jpeg-quality",
                quality,
                "--jpeg-subsampling",
                subsampling,
            ])
            .arg("--force")
            .arg("--summary")
            .arg(&summary);
        if adjuncts {
            apply.arg("--copy-xmp-iptc");
        }
        if strip_gps {
            apply.arg("--strip-gps");
        }
        let result = apply.output().unwrap();
        assert!(
            result.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output = out_dir.join(if format == "tiff" {
            "source.tiff"
        } else {
            "source.jpg"
        });
        let bytes = fs::read(&output).unwrap();
        if format == "tiff" {
            export_tiff_metadata(&bytes, !strip_gps, adjuncts);
        } else {
            export_jpeg_metadata(&bytes, !strip_gps, adjuncts, &expected_sampling);
            let image = image::load_from_memory(&bytes).unwrap();
            assert!(image.width() > 0 && image.height() > 0);
        }
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(summary).unwrap()).unwrap();
        assert_eq!(report["format"], format);
        if format == "jpeg" {
            assert_eq!(report["jpeg-quality"], quality.parse::<u8>().unwrap());
            assert_eq!(report["jpeg-subsampling"], subsampling);
        }
        assert_eq!(
            report["metadata"][0]["file"],
            source.to_string_lossy().as_ref()
        );
        assert_eq!(report["metadata"].as_array().unwrap().len(), 1);
        let copied = report["metadata"][0]["copied"].as_array().unwrap();
        let skipped = report["metadata"][0]["skipped"].as_array().unwrap();
        for tag in ["Make", "Model", "Artist", "DateTimeOriginal"] {
            assert!(
                copied.iter().any(|v| v == tag),
                "{label}: {tag} absent from copied: {copied:?}"
            );
        }
        for tag in ["MakerNote", "ColorSpace", "WhiteBalance"] {
            assert!(
                skipped.iter().any(|v| v == tag),
                "{label}: {tag} absent from skipped: {skipped:?}"
            );
        }
        assert_eq!(copied.iter().any(|v| v == "XMP"), adjuncts);
        assert_eq!(copied.iter().any(|v| v == "GPSLatitude"), !strip_gps);
        if strip_gps {
            assert!(skipped
                .iter()
                .any(|v| v.as_str() == Some("GPSLatitude (GPS stripped)")));
        }
        let original_output = fs::read(&output).unwrap();
        let skip_summary = work.join(format!("{label}-skip.json"));
        let skipped_run = std::process::Command::new(bin)
            .arg("apply")
            .arg(&profile)
            .arg(&source)
            .arg("-o")
            .arg(&out_dir)
            .args(["--format", format, "--force"])
            .arg("--summary")
            .arg(&skip_summary)
            .output()
            .unwrap();
        assert!(
            skipped_run.status.success(),
            "{}",
            String::from_utf8_lossy(&skipped_run.stderr)
        );
        assert_eq!(fs::read(&output).unwrap(), original_output);
        let skipped_report: serde_json::Value =
            serde_json::from_slice(&fs::read(skip_summary).unwrap()).unwrap();
        assert_eq!(skipped_report["skipped"].as_array().unwrap().len(), 1);
        assert!(skipped_report["succeeded"].as_array().unwrap().is_empty());
    }
    let replace = work.join("jpeg-444").join("source.jpg");
    fs::write(&replace, b"stale output").unwrap();
    let replaced = std::process::Command::new(bin)
        .arg("apply")
        .arg(&profile)
        .arg(&source)
        .arg("-o")
        .arg(work.join("jpeg-444"))
        .args(["--format", "jpeg", "--force", "--overwrite"])
        .output()
        .unwrap();
    assert!(
        replaced.status.success(),
        "{}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    let replacement = fs::read(replace).unwrap();
    export_jpeg_metadata(&replacement, true, false, &[0x11, 0x11, 0x11]);
    assert!(image::load_from_memory(&replacement).is_ok());

    for (flag, value) in [
        ("--jpeg-quality", "0"),
        ("--jpeg-quality", "101"),
        ("--jpeg-subsampling", "411"),
        ("--format", "png"),
    ] {
        let out = work.join(format!("invalid-{}-{value}", &flag[2..]));
        let result = std::process::Command::new(bin)
            .arg("apply")
            .arg(&profile)
            .arg(&source)
            .arg("-o")
            .arg(&out)
            .arg(flag)
            .arg(value)
            .output()
            .unwrap();
        assert!(!result.status.success(), "{flag} {value} accepted");
    }
    let original = fs::read(&source).unwrap();
    let result = std::process::Command::new(bin)
        .arg("apply")
        .arg(&profile)
        .arg(&source)
        .arg("-o")
        .arg(&work)
        .args(["--format", "jpeg", "--force", "--overwrite"])
        .output()
        .unwrap();
    assert!(!result.status.success(), "input replaced by output");
    assert_eq!(fs::read(&source).unwrap(), original);
    fs::remove_dir_all(work).unwrap();
}
