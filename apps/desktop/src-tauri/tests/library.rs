//! Calibration library: saving, scanning and applying across cameras.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use colorbalance_core::metadata::{ExifEntry, ExifIfd, ExifValue, ExportMetadata};
use colorbalance_core::output::{encode_jpeg_rgb_u16, JpegSampling};
use colorbalance_core::output_space::{OutputConverter, OutputSpace};
use colorbalance_core::profile;
use colorbalance_desktop::commands::{
    apply_batch, correct_image, derive_profile, no_progress, ExportOptions, MismatchPolicy,
};
use colorbalance_desktop::library::{list_library, preview_grant, save_to_library};
use colorbalance_desktop::preview_files::PreviewFiles;
use colorbalance_fixtures::{render_chart_dng, ChartScene};
use sha2::{Digest, Sha256};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cb-library-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn derive_clean_profile(work: &Path) -> (PathBuf, PathBuf) {
    let scene = ChartScene::default();
    let reference = work.join("reference.dng");
    render_chart_dng(&reference, &scene).unwrap();
    let profile_path = work.join("p.cbprofile.json");
    derive_profile(
        reference.to_string_lossy().into_owned(),
        "classic-before-nov-2014".to_owned(),
        profile_path.to_string_lossy().into_owned(),
        None,
        Some(serde_json::from_value(serde_json::json!({ "corners": scene.quad })).unwrap()),
        &no_progress,
    )
    .expect("derive succeeds on a clean fixture");
    (reference, profile_path)
}

fn ascii(text: &str) -> ExifValue {
    ExifValue::Ascii(format!("{text}\0").into_bytes())
}

fn dms(degrees: u32, minutes: u32) -> ExifValue {
    ExifValue::Rational(vec![(degrees, 1), (minutes, 1), (0, 1)])
}

/// A 640x480 JPEG whose EXIF names a camera, lens, time and position.
fn jpeg_with_exif(path: &Path) {
    let exif = [
        (ExifIfd::Primary, 271, ascii("Canon")),
        (ExifIfd::Primary, 272, ascii("EOS R5")),
        (ExifIfd::Exif, 36867, ascii("2024:03:09 14:05:59")),
        (ExifIfd::Exif, 42036, ascii("RF24-70mm F2.8")),
        (ExifIfd::Gps, 1, ascii("S")),
        (ExifIfd::Gps, 2, dms(33, 30)),
        (ExifIfd::Gps, 3, ascii("E")),
        (ExifIfd::Gps, 4, dms(151, 0)),
        (ExifIfd::Gps, 6, ExifValue::Rational(vec![(25, 1)])),
    ]
    .into_iter()
    .map(|(ifd, tag, value)| ExifEntry { ifd, tag, value })
    .collect();
    let (width, height) = (640_u32, 480_u32);
    let pixels: Vec<u16> = (0..height)
        .flat_map(|y| (0..width).flat_map(move |x| [(x * 100) as u16, (y * 130) as u16, 20_000]))
        .collect();
    let icc = OutputConverter::new(OutputSpace::Srgb)
        .unwrap()
        .icc_profile()
        .unwrap();
    let bytes = encode_jpeg_rgb_u16(
        width,
        height,
        &pixels,
        &icc,
        90,
        JpegSampling::Yuv444,
        &ExportMetadata {
            exif,
            ..Default::default()
        },
    )
    .unwrap();
    std::fs::write(path, bytes).unwrap();
}

struct Fixture {
    work: PathBuf,
    library: PathBuf,
    profile: PathBuf,
    reference: PathBuf,
}

/// Re-seal `profile_path` as derived from `reference`, so its reference digest
/// is the SHA-256 of those exact bytes.
fn bind_to_reference(profile_path: &Path, reference: &Path) {
    let mut bound = profile::from_json(&std::fs::read_to_string(profile_path).unwrap()).unwrap();
    bound.reference_digest = format!("{:x}", Sha256::digest(std::fs::read(reference).unwrap()));
    bound.digest = String::new();
    bound.digest = profile::digest(&bound);
    std::fs::write(profile_path, profile::to_json(&bound)).unwrap();
}

fn fixture(name: &str) -> Fixture {
    let work = temp_dir(name);
    let (_, profile) = derive_clean_profile(&work);
    let reference = work.join("shot.jpg");
    jpeg_with_exif(&reference);
    bind_to_reference(&profile, &reference);
    Fixture {
        library: work.join("library"),
        work,
        profile,
        reference,
    }
}

fn save(f: &Fixture, label: &str, include_gps: bool) -> serde_json::Value {
    let entry = save_to_library(
        &f.library,
        &f.profile,
        &f.reference,
        label,
        "north window",
        &["studio".to_owned(), " ".to_owned(), "studio".to_owned()],
        include_gps,
    )
    .expect("saves");
    serde_json::to_value(entry).unwrap()
}

fn listing(f: &Fixture) -> serde_json::Value {
    serde_json::to_value(list_library(&f.library).unwrap()).unwrap()
}

fn entry_dir(f: &Fixture, entry: &serde_json::Value) -> PathBuf {
    f.library.join(entry["id"].as_str().unwrap())
}

fn edit_json(path: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    edit(&mut value);
    std::fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

#[test]
fn a_saved_entry_lists_with_its_calibration_and_capture_facts() {
    let f = fixture("round-trip");
    let saved = save(&f, "Studio / Canon", true);
    let listed = listing(&f);
    assert_eq!(listed["problems"].as_array().unwrap().len(), 0);
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    assert_eq!(listed["entries"][0], saved);

    let entry = &saved;
    let profile_text = std::fs::read_to_string(&f.profile).unwrap();
    let source = profile::from_json(&profile_text).unwrap();
    assert!(entry["id"].as_str().unwrap().starts_with("studio-canon-"));
    assert!(entry["id"].as_str().unwrap().ends_with(&source.digest[..8]));
    assert_eq!(entry["label"], "Studio / Canon");
    assert_eq!(entry["notes"], "north window");
    // Blank and repeated tags are dropped.
    assert_eq!(entry["tags"], serde_json::json!(["studio"]));
    assert_eq!(entry["digest"], source.digest.as_str());
    assert_eq!(entry["cameraMake"], source.camera.make.as_str());
    assert_eq!(entry["cameraModel"], source.camera.model.as_str());
    assert_eq!(entry["decoder"], source.camera.decoder.as_str());
    assert_eq!(entry["chartRevision"], "classic-before-november2014");
    assert_eq!(entry["patchCount"], source.validation.patch_count);
    assert_eq!(entry["qualityPassed"], true);
    assert_eq!(entry["quickAndDirty"], false);
    // Capture facts come from the reference's EXIF, not from the profile.
    assert_eq!(entry["lens"], "RF24-70mm F2.8");
    assert_eq!(entry["capturedAt"], "2024-03-09 14:05:59");
    assert_eq!(entry["gps"]["latitude"], -33.5);
    assert_eq!(entry["gps"]["longitude"], 151.0);
    assert_eq!(entry["gps"]["altitude"], 25.0);

    // The profile is copied untouched, so its digest still verifies.
    let dir = entry_dir(&f, entry);
    let stored = std::fs::read_to_string(dir.join("profile.cbprofile.json")).unwrap();
    assert_eq!(stored, profile_text);
    assert_eq!(
        entry["profilePath"].as_str().unwrap(),
        dir.join("profile.cbprofile.json").to_string_lossy()
    );

    // The preview is a PNG capped at 320 px on its longest side.
    let preview = std::fs::read(entry["previewPath"].as_str().unwrap()).unwrap();
    assert_eq!(&preview[..8], b"\x89PNG\r\n\x1a\n");
    let dimension = |at: usize| u32::from_be_bytes(preview[at..at + 4].try_into().unwrap());
    assert_eq!((dimension(16), dimension(20)), (320, 240));

    // The on-disk format uses kebab-case keys.
    let on_disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("entry.json")).unwrap()).unwrap();
    assert_eq!(on_disk["schema-version"], "1.0");
    assert_eq!(on_disk["profile-digest"], source.digest.as_str());
    assert_eq!(on_disk["capture"]["date-time"], "2024-03-09 14:05:59");
    assert!(on_disk["capture"]["gps"].is_object());

    let leftovers = std::fs::read_dir(&f.library)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
        .count();
    assert_eq!(leftovers, 0);
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn opting_out_of_gps_keeps_every_other_capture_fact() {
    let f = fixture("no-gps");
    let saved = save(&f, "No position", false);
    assert!(saved["gps"].is_null());
    assert_eq!(saved["lens"], "RF24-70mm F2.8");
    assert_eq!(saved["capturedAt"], "2024-03-09 14:05:59");
    // Nothing of the position reaches the disk either.
    let on_disk: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(entry_dir(&f, &saved).join("entry.json")).unwrap(),
    )
    .unwrap();
    assert!(on_disk["capture"]["gps"].is_null(), "{on_disk}");
    assert!(!on_disk.to_string().contains("latitude"), "{on_disk}");
    assert!(listing(&f)["entries"][0]["gps"].is_null());
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn saving_the_same_label_twice_keeps_both_entries_untouched() {
    let f = fixture("twice");
    let first = save(&f, "Same", true);
    let first_bytes = std::fs::read(entry_dir(&f, &first).join("entry.json")).unwrap();
    let second = save(&f, "Same", true);
    assert_ne!(first["id"], second["id"]);
    assert_eq!(
        std::fs::read(entry_dir(&f, &first).join("entry.json")).unwrap(),
        first_bytes
    );
    let listed = listing(&f);
    assert_eq!(listed["entries"].as_array().unwrap().len(), 2);
    // Equal labels sort by id.
    assert_eq!(listed["entries"][0]["id"], first["id"]);
    assert_eq!(listed["entries"][1]["id"], second["id"]);
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn entries_sort_by_label_then_id() {
    let f = fixture("sorted");
    for label in ["beta", "Alpha", "alpha"] {
        save(&f, label, true);
    }
    let labels: Vec<String> = listing(&f)["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["label"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(labels, ["Alpha", "alpha", "beta"]);
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn a_tampered_profile_is_a_problem_and_other_entries_still_list() {
    let f = fixture("tampered");
    let good = save(&f, "Good", true);
    let bad = save(&f, "Bad", true);
    edit_json(
        &entry_dir(&f, &bad).join("profile.cbprofile.json"),
        |profile| profile["camera"]["model"] = "Edited".into(),
    );
    let listed = listing(&f);
    let entries = listed["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], good["id"]);
    let problems = listed["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0]["id"], bad["id"]);
    assert!(
        problems[0]["message"]
            .as_str()
            .unwrap()
            .starts_with("invalid profile"),
        "{problems:?}"
    );
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn an_entry_that_names_another_profile_is_a_problem() {
    let f = fixture("mismatched-entry");
    let saved = save(&f, "Swapped", true);
    edit_json(&entry_dir(&f, &saved).join("entry.json"), |entry| {
        entry["profile-digest"] = "0".repeat(64).into();
    });
    let listed = listing(&f);
    assert_eq!(listed["entries"].as_array().unwrap().len(), 0);
    assert_eq!(listed["problems"][0]["id"], saved["id"]);
    assert_eq!(
        listed["problems"][0]["message"],
        "entry does not match its profile"
    );
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn a_directory_without_entry_files_is_a_problem() {
    let f = fixture("incomplete");
    save(&f, "Fine", true);
    std::fs::create_dir(f.library.join("empty-dir")).unwrap();
    let listed = listing(&f);
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    assert_eq!(listed["problems"][0]["id"], "empty-dir");
    let _ = std::fs::remove_dir_all(f.work);
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[test]
fn only_a_real_preview_inside_its_entry_folder_is_granted_to_the_webview() {
    let f = fixture("preview-grant");
    let saved = save(&f, "Granted", true);
    let preview = entry_dir(&f, &saved).join("preview.png");
    let entries = list_library(&f.library).unwrap().entries;
    let granted = preview_grant(&f.library, &entries[0]).expect("a real preview is granted");
    assert_eq!(granted, std::fs::canonicalize(&preview).unwrap());

    // A preview that links to a file outside the library is neither listed nor granted.
    let outside = f.work.join("outside.png");
    std::fs::copy(&preview, &outside).unwrap();
    std::fs::remove_file(&preview).unwrap();
    if symlink_file(&outside, &preview).is_err() {
        // Creating symlinks can need elevated rights on Windows.
        let _ = std::fs::remove_dir_all(f.work);
        return;
    }
    let listed = list_library(&f.library).unwrap();
    assert_eq!(listed.entries.len(), 1);
    assert!(listed.entries[0].preview_path.is_none());
    assert!(preview_grant(&f.library, &listed.entries[0]).is_none());
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn a_missing_preview_leaves_a_usable_entry_without_one() {
    let f = fixture("no-preview");
    let saved = save(&f, "Preview gone", true);
    std::fs::remove_file(entry_dir(&f, &saved).join("preview.png")).unwrap();
    let listed = listing(&f);
    assert_eq!(listed["problems"].as_array().unwrap().len(), 0);
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    assert!(listed["entries"][0]["previewPath"].is_null());
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn staging_directories_and_loose_files_are_not_entries() {
    let f = fixture("ignored");
    let saved = save(&f, "Real", true);
    // A save in progress, and its leftovers after a crash.
    std::fs::create_dir_all(f.library.join(".tmp-abc123")).unwrap();
    std::fs::write(f.library.join(".tmp-abc123").join("entry.json"), "{").unwrap();
    std::fs::write(f.library.join("notes.txt"), "hello").unwrap();
    let listed = listing(&f);
    assert_eq!(listed["problems"].as_array().unwrap().len(), 0);
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    assert_eq!(listed["entries"][0]["id"], saved["id"]);
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn a_library_folder_that_does_not_exist_lists_as_empty() {
    let f = fixture("absent");
    let listed = listing(&f);
    assert_eq!(listed["entries"].as_array().unwrap().len(), 0);
    assert_eq!(listed["problems"].as_array().unwrap().len(), 0);
    assert!(!f.library.exists(), "listing must not create the folder");
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn an_undecodable_reference_still_saves_without_preview_or_capture_facts() {
    let f = fixture("bad-reference");
    let reference = f.work.join("not-an-image.txt");
    std::fs::write(&reference, "plain text").unwrap();
    bind_to_reference(&f.profile, &reference);
    let entry = save_to_library(
        &f.library,
        &f.profile,
        &reference,
        "No picture",
        "",
        &[],
        true,
    )
    .expect("saves");
    let entry = serde_json::to_value(entry).unwrap();
    assert!(entry["previewPath"].is_null());
    assert!(entry["lens"].is_null());
    assert!(entry["capturedAt"].is_null());
    assert!(entry["gps"].is_null());
    assert_eq!(listing(&f)["entries"].as_array().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(f.work);
}

#[test]
fn a_reference_that_does_not_match_the_profile_digest_is_refused_untouched() {
    let a = fixture("mismatch-a");
    // B is a second capture: same pixels, one extra trailing byte, so another digest.
    let reference_b = a.work.join("shot-b.jpg");
    let mut bytes_b = std::fs::read(&a.reference).unwrap();
    bytes_b.push(0);
    std::fs::write(&reference_b, bytes_b).unwrap();
    let profile_b = a.work.join("b.cbprofile.json");
    std::fs::copy(&a.profile, &profile_b).unwrap();
    bind_to_reference(&profile_b, &reference_b);

    for (profile_path, reference) in [(&a.profile, &reference_b), (&profile_b, &a.reference)] {
        let error = save_to_library(&a.library, profile_path, reference, "Wrong", "", &[], true)
            .expect_err("a reference the profile was not derived from is refused")
            .to_string();
        assert!(error.contains("does not belong to this profile"), "{error}");
        assert!(
            !a.library.exists(),
            "the library folder must not be created"
        );
    }

    // Each profile still saves with its own reference.
    save_to_library(&a.library, &a.profile, &a.reference, "A", "", &[], true).expect("A saves");
    save_to_library(&a.library, &profile_b, &reference_b, "B", "", &[], true).expect("B saves");
    assert_eq!(listing(&a)["entries"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(a.work);
}

#[test]
fn an_empty_label_or_a_bad_profile_saves_nothing() {
    let f = fixture("rejected");
    for label in ["", "   "] {
        let error = save_to_library(&f.library, &f.profile, &f.reference, label, "", &[], true)
            .expect_err("empty label is refused")
            .to_string();
        assert!(error.contains("label"), "{error}");
    }
    edit_json(&f.profile, |profile| {
        profile["camera"]["model"] = "Edited".into();
    });
    save_to_library(&f.library, &f.profile, &f.reference, "Bad", "", &[], true)
        .expect_err("a profile that fails its digest is refused");
    let entries = std::fs::read_dir(&f.library)
        .map(|d| d.flatten().count())
        .unwrap_or(0);
    assert_eq!(entries, 0, "nothing, not even a staging directory, remains");
    let _ = std::fs::remove_dir_all(f.work);
}

/// Re-seal the derived profile as if it belonged to another camera.
fn other_camera_profile(profile_path: &Path) {
    let mut other = profile::from_json(&std::fs::read_to_string(profile_path).unwrap()).unwrap();
    other.camera.model = "Some Other Camera".to_owned();
    other.digest = String::new();
    other.digest = profile::digest(&other);
    std::fs::write(profile_path, profile::to_json(&other)).unwrap();
}

#[test]
fn correct_image_warns_for_a_library_calibration_but_blocks_by_default() {
    let work = temp_dir("warn-correct");
    let (reference, profile_path) = derive_clean_profile(&work);
    other_camera_profile(&profile_path);
    let run = |policy| {
        correct_image(
            &PreviewFiles::default(),
            profile_path.to_string_lossy().into_owned(),
            reference.to_string_lossy().into_owned(),
            None,
            policy,
            &no_progress,
        )
    };
    let error = run(MismatchPolicy::Block)
        .expect_err("mismatch fails closed")
        .to_string();
    assert!(error.contains("camera mismatch"), "{error}");

    let response = serde_json::to_value(run(MismatchPolicy::Warn).expect("warn allows")).unwrap();
    let warnings: Vec<&str> = response["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap())
        .collect();
    let mismatches: Vec<&&str> = warnings
        .iter()
        .filter(|w| w.contains("mismatch allowed"))
        .collect();
    assert_eq!(mismatches.len(), 1, "{warnings:?}");
    assert!(
        mismatches[0].starts_with("camera mismatch allowed for library calibration: profile "),
        "{warnings:?}"
    );
    assert!(mismatches[0].contains("Some Other Camera"), "{warnings:?}");
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn apply_batch_records_the_allowed_mismatch_in_its_warnings() {
    let work = temp_dir("warn-batch");
    let (reference, profile_path) = derive_clean_profile(&work);
    other_camera_profile(&profile_path);
    let input = work.join("in");
    std::fs::create_dir_all(&input).unwrap();
    std::fs::copy(&reference, input.join("a.dng")).unwrap();
    let run = |policy, output: &str| {
        let output = work.join(output);
        std::fs::create_dir_all(&output).unwrap();
        let response = apply_batch(
            profile_path.to_string_lossy().into_owned(),
            input.to_string_lossy().into_owned(),
            output.to_string_lossy().into_owned(),
            ExportOptions::default(),
            policy,
            Default::default(),
            Arc::new(|_, _, _| {}),
        )
        .unwrap();
        (serde_json::to_value(response).unwrap(), output)
    };

    let (blocked, blocked_out) = run(MismatchPolicy::Block, "blocked");
    assert_eq!(blocked["succeeded"].as_array().unwrap().len(), 0);
    assert_eq!(blocked["failed"].as_array().unwrap().len(), 1);
    assert_eq!(std::fs::read_dir(blocked_out).unwrap().count(), 0);

    let (allowed, allowed_out) = run(MismatchPolicy::Warn, "allowed");
    assert_eq!(allowed["failed"].as_array().unwrap().len(), 0);
    assert_eq!(allowed["succeeded"].as_array().unwrap().len(), 1);
    assert_eq!(std::fs::read_dir(allowed_out).unwrap().count(), 1);
    let warnings = allowed["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0]["file"].as_str().unwrap().ends_with("a.dng"));
    assert!(warnings[0]["warning"]
        .as_str()
        .unwrap()
        .starts_with("camera mismatch allowed for library calibration: "));
    let _ = std::fs::remove_dir_all(work);
}
