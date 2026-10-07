//! Behavior of the one-entry decoded reference cache: repeat calls skip the
//! decode, any change to the file (or a different file) reloads, and nothing
//! stale or failed is ever served.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use colorbalance_desktop::commands::{
    detect_chart_cached, inspect_reference_cached, load_reference, load_reference_cached,
    no_progress, BackendError,
};
use colorbalance_desktop::commands::{ExportOptions, ImageExport, MismatchPolicy};
use colorbalance_desktop::preview_files::PreviewFiles;
use colorbalance_desktop::reference_cache::ReferenceCache;
use colorbalance_fixtures::{render_chart_dng, ChartScene};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cb-refcache-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scene(width: u32, height: u32) -> ChartScene {
    ChartScene {
        width,
        height,
        quad: [
            [40.0, 40.0],
            [f64::from(width) - 40.0, 34.0],
            [f64::from(width) - 40.0, f64::from(height) - 40.0],
            [40.0, f64::from(height) - 40.0],
        ],
        ..ChartScene::default()
    }
}

fn decode(path: &Path) -> Result<colorbalance_core::decode::DecodedImage, BackendError> {
    colorbalance_raw::decode_any(path).map_err(|error| BackendError::Message(error.to_string()))
}

/// Run `f` with a recording reporter and return the stage labels it saw.
fn stages<T>(f: impl FnOnce(&dyn Fn(&str, usize, usize)) -> T) -> (T, Vec<String>) {
    let seen = RefCell::new(Vec::new());
    let value = f(&|label, _, _| seen.borrow_mut().push(label.to_owned()));
    (value, seen.into_inner())
}

#[test]
fn repeated_reference_workflow_decodes_once() {
    let work = temp_dir("repeat");
    let path = work.join("ref.dng");
    render_chart_dng(&path, &scene(480, 320)).unwrap();
    let path_str = path.to_string_lossy().into_owned();
    let cache = ReferenceCache::default();
    let previews = PreviewFiles::default();

    let (loaded, load_stages) =
        stages(|r| load_reference_cached(&cache, &previews, path_str.clone(), r).unwrap());
    let (loaded_again, repeated_stages) =
        stages(|r| load_reference_cached(&cache, &previews, path_str.clone(), r).unwrap());
    assert_eq!(
        repeated_stages,
        ["Using cached image", "Using cached preview"]
    );
    assert_eq!(loaded.preview_path, loaded_again.preview_path);
    assert_eq!(
        std::fs::read_dir(previews.directory().unwrap())
            .unwrap()
            .count(),
        1
    );
    assert_eq!(load_stages, ["Decoding image", "Rendering preview"]);
    let detected =
        serde_json::to_value(detect_chart_cached(&cache, path_str.clone()).unwrap()).unwrap();
    assert!(matches!(
        detected["status"].as_str(),
        Some("found" | "missing" | "ambiguous")
    ));
    assert!(detected.get("previewPath").is_none());
    assert!(detected.get("chartRevision").is_none());

    let (inspected, inspect_stages) = stages(|r| {
        inspect_reference_cached(
            &cache,
            &previews,
            path_str.clone(),
            "classic-before-nov-2014".to_owned(),
            None,
            r,
        )
        .unwrap()
    });
    assert!(
        !inspect_stages.iter().any(|s| s == "Decoding image"),
        "inspect after load must not decode again: {inspect_stages:?}"
    );
    assert_eq!(inspect_stages[0], "Using cached image");
    assert!(inspect_stages.iter().any(|s| s == "Using cached preview"));
    assert!(!inspect_stages.iter().any(|s| s == "Rendering preview"));

    let loaded = serde_json::to_value(&loaded).unwrap();
    let inspected = serde_json::to_value(&inspected).unwrap();
    assert_eq!(loaded["previewPath"], inspected["previewPath"]);
    assert_eq!(loaded["imageWidth"], inspected["imageWidth"]);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn released_preview_is_rendered_again_without_decoding() {
    let work = temp_dir("released-preview");
    let path = work.join("ref.dng");
    render_chart_dng(&path, &scene(480, 320)).unwrap();
    let path_str = path.to_string_lossy().into_owned();
    let cache = ReferenceCache::default();
    let previews = PreviewFiles::default();

    let first = load_reference_cached(&cache, &previews, path_str.clone(), &no_progress).unwrap();
    assert!(Path::new(&first.preview_path).is_file());

    previews
        .release(std::slice::from_ref(&first.preview_path))
        .unwrap();
    assert!(!Path::new(&first.preview_path).exists());

    let (second, seen) = stages(|report| {
        load_reference_cached(&cache, &previews, path_str.clone(), report).unwrap()
    });
    assert_eq!(seen, ["Using cached image", "Rendering preview"]);
    assert_ne!(second.preview_path, first.preview_path);
    assert!(Path::new(&second.preview_path).is_file());

    let (third, seen) = stages(|report| {
        load_reference_cached(&cache, &previews, path_str.clone(), report).unwrap()
    });
    assert_eq!(seen, ["Using cached image", "Using cached preview"]);
    assert_eq!(third.preview_path, second.preview_path);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn plain_commands_never_use_a_cache() {
    let work = temp_dir("plain");
    let path = work.join("ref.dng");
    render_chart_dng(&path, &scene(480, 320)).unwrap();
    let path_str = path.to_string_lossy().into_owned();
    let previews = PreviewFiles::default();

    for _ in 0..2 {
        let (_, seen) = stages(|r| load_reference(&previews, path_str.clone(), r).unwrap());
        assert_eq!(seen, ["Decoding image", "Rendering preview"]);
    }
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn replaced_file_reloads_with_new_pixels_and_preview() {
    let work = temp_dir("replace");
    let path = work.join("ref.dng");
    let path_str = path.to_string_lossy().into_owned();
    let cache = ReferenceCache::default();
    let previews = PreviewFiles::default();

    render_chart_dng(&path, &scene(480, 320)).unwrap();
    let first = load_reference_cached(&cache, &previews, path_str.clone(), &no_progress).unwrap();
    let first = serde_json::to_value(&first).unwrap();

    render_chart_dng(&path, &scene(640, 400)).unwrap();
    let (second, seen) =
        stages(|r| load_reference_cached(&cache, &previews, path_str.clone(), r).unwrap());
    let second = serde_json::to_value(&second).unwrap();

    assert_eq!(seen, ["Decoding image", "Rendering preview"]);
    assert_eq!(first["imageWidth"], 480);
    assert_eq!(second["imageWidth"], 640);
    assert_eq!(second["imageHeight"], 400);
    assert_ne!(first["previewPath"], second["previewPath"]);
    assert!(Path::new(first["previewPath"].as_str().unwrap()).exists());
    previews
        .release(&[first["previewPath"].as_str().unwrap().to_owned()])
        .unwrap();
    assert!(!Path::new(first["previewPath"].as_str().unwrap()).exists());
    assert!(Path::new(second["previewPath"].as_str().unwrap()).exists());
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn modified_time_change_alone_invalidates() {
    let work = temp_dir("mtime");
    let path = work.join("ref.dng");
    render_chart_dng(&path, &scene(480, 320)).unwrap();
    let cache = ReferenceCache::default();
    let decodes = Cell::new(0);
    let get = || {
        cache
            .get_or_decode(
                &path,
                || {},
                |p| {
                    decodes.set(decodes.get() + 1);
                    decode(p)
                },
            )
            .unwrap()
    };

    assert!(!get().hit);
    assert!(get().hit);
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(3600))
        .unwrap();
    drop(file);
    assert!(!get().hit, "same bytes, new mtime must reload");
    assert_eq!(decodes.get(), 2);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn alternating_files_are_never_confused() {
    let work = temp_dir("two");
    let a = work.join("a.dng");
    let b = work.join("b.dng");
    render_chart_dng(&a, &scene(480, 320)).unwrap();
    render_chart_dng(&b, &scene(640, 400)).unwrap();
    let cache = ReferenceCache::default();

    for (path, width) in [(&a, 480), (&b, 640), (&a, 480), (&a, 480), (&b, 640)] {
        let got = cache.get_or_decode(path, || {}, decode).unwrap();
        assert_eq!(got.image.width, width, "{}", path.display());
    }
    // One entry only: after b, a is a miss again.
    assert!(!cache.get_or_decode(&a, || {}, decode).unwrap().hit);
    assert!(cache.get_or_decode(&a, || {}, decode).unwrap().hit);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn hit_shares_the_image_without_copying() {
    let work = temp_dir("share");
    let path = work.join("ref.dng");
    render_chart_dng(&path, &scene(480, 320)).unwrap();
    let cache = ReferenceCache::default();

    let first = cache.get_or_decode(&path, || {}, decode).unwrap();
    let second = cache.get_or_decode(&path, || {}, decode).unwrap();
    assert!(second.hit);
    assert!(Arc::ptr_eq(&first.image, &second.image));
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn file_changed_during_decode_is_not_cached() {
    let work = temp_dir("race");
    let path = work.join("ref.dng");
    render_chart_dng(&path, &scene(480, 320)).unwrap();
    let cache = ReferenceCache::default();

    let racing = cache
        .get_or_decode(
            &path,
            || {},
            |p| {
                let image = decode(p);
                render_chart_dng(p, &scene(640, 400)).unwrap();
                image
            },
        )
        .unwrap();
    assert_eq!(racing.image.width, 480, "caller still gets what it decoded");

    let after = cache.get_or_decode(&path, || {}, decode).unwrap();
    assert!(!after.hit, "the stale decode must not have been stored");
    assert_eq!(after.image.width, 640);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn failures_are_not_cached_and_do_not_evict() {
    let work = temp_dir("fail");
    let good = work.join("good.dng");
    let bad = work.join("bad.dng");
    render_chart_dng(&good, &scene(480, 320)).unwrap();
    std::fs::write(&bad, b"not an image").unwrap();
    let cache = ReferenceCache::default();

    assert!(!cache.get_or_decode(&good, || {}, decode).unwrap().hit);
    assert!(cache.get_or_decode(&bad, || {}, decode).is_err());
    assert!(cache.get_or_decode(&bad, || {}, decode).is_err());
    assert!(cache
        .get_or_decode(&work.join("missing.dng"), || {}, decode)
        .is_err());
    // A failed lookup of another file drops the old entry; the good file
    // simply decodes again.
    assert_eq!(
        cache
            .get_or_decode(&good, || {}, decode)
            .unwrap()
            .image
            .width,
        480
    );
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn oversized_images_and_disabled_caches_never_store() {
    let work = temp_dir("limit");
    let path = work.join("ref.dng");
    render_chart_dng(&path, &scene(480, 320)).unwrap();

    for cache in [ReferenceCache::with_limit(1024), ReferenceCache::disabled()] {
        assert!(!cache.get_or_decode(&path, || {}, decode).unwrap().hit);
        assert!(!cache.get_or_decode(&path, || {}, decode).unwrap().hit);
    }
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn decode_hook_runs_only_on_a_miss() {
    let work = temp_dir("hook");
    let path = work.join("ref.dng");
    render_chart_dng(&path, &scene(480, 320)).unwrap();
    let cache = ReferenceCache::default();
    let started = Cell::new(0);

    for _ in 0..3 {
        cache
            .get_or_decode(&path, || started.set(started.get() + 1), decode)
            .unwrap();
    }
    assert_eq!(started.get(), 1);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn correct_after_load_reuses_the_decode_and_leaves_the_cached_image_untouched() {
    use colorbalance_desktop::commands::{correct_image_cached, derive_profile};

    let work = temp_dir("correct");
    let scene = scene(480, 320);
    let path = work.join("ref.dng");
    render_chart_dng(&path, &scene).unwrap();
    let path_str = path.to_string_lossy().into_owned();
    let profile = work.join("p.cbprofile.json");
    derive_profile(
        path_str.clone(),
        "classic-before-nov-2014".to_owned(),
        profile.to_string_lossy().into_owned(),
        None,
        Some(serde_json::from_value(serde_json::json!({ "corners": scene.quad })).unwrap()),
        &no_progress,
    )
    .unwrap();

    let cache = ReferenceCache::default();
    let previews = PreviewFiles::default();
    load_reference_cached(&cache, &previews, path_str.clone(), &no_progress).unwrap();
    let before = cache.get_or_decode(&path, || {}, decode).unwrap().image;
    let pixels_before = before.rgb.clone();

    let run = |output: Option<String>| {
        stages(|r| {
            correct_image_cached(
                &cache,
                &previews,
                profile.to_string_lossy().into_owned(),
                path_str.clone(),
                output.map(|path| ImageExport {
                    path,
                    options: ExportOptions::default(),
                }),
                MismatchPolicy::Block,
                r,
            )
            .unwrap()
        })
    };
    let (first, first_stages) = run(None);
    let first_after = std::fs::read(
        serde_json::to_value(&first).unwrap()["afterPath"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first_stages[0], "Using cached image");
    assert_eq!(first_stages[1], "Using cached preview");
    assert!(!first_stages.iter().any(|s| s == "Decoding image"));

    let output = work.join("out.tiff");
    let (saved, saved_stages) = run(Some(output.to_string_lossy().into_owned()));
    assert_eq!(saved_stages[0], "Using cached image");
    assert!(output.exists(), "save writes the TIFF");

    // The same file gives the same result with or without the cache.
    let (uncached, _) = stages(|r| {
        correct_image_cached(
            &ReferenceCache::disabled(),
            &previews,
            profile.to_string_lossy().into_owned(),
            path_str.clone(),
            None,
            MismatchPolicy::Block,
            r,
        )
        .unwrap()
    });
    let json = |v: &_| serde_json::to_value(v).unwrap();
    assert_eq!(
        first_after,
        std::fs::read(json(&uncached)["afterPath"].as_str().unwrap()).unwrap()
    );
    assert_eq!(json(&first)["beforePath"], json(&saved)["beforePath"]);
    assert!(Path::new(json(&first)["beforePath"].as_str().unwrap()).exists());

    let after = cache.get_or_decode(&path, || {}, decode).unwrap();
    assert!(after.hit && Arc::ptr_eq(&after.image, &before));
    assert_eq!(
        after.image.rgb, pixels_before,
        "correction must not mutate the cache"
    );
    let _ = std::fs::remove_dir_all(work);
}
