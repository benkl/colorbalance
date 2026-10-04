use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use colorbalance_fixtures::{render_chart_dng, ChartScene, SceneDefect};
use colorbalance_raw::decode_dng;

fn temp_path(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "colorbalance-fixture-{name}-{}-{nonce}.dng",
        std::process::id()
    ))
}

fn patch_samples(
    image: &colorbalance_core::DecodedImage,
    scene: &ChartScene,
    patch: usize,
) -> Vec<[f32; 3]> {
    let column = patch % 6;
    let row = patch / 6;
    let mut values = Vec::new();
    for gy in 0..20 {
        for gx in 0..20 {
            let u = (column as f64 + 0.2 + 0.6 * (gx as f64 + 0.5) / 20.0) / 6.0;
            let v = (row as f64 + 0.2 + 0.6 * (gy as f64 + 0.5) / 20.0) / 4.0;
            let one_u = 1.0 - u;
            let one_v = 1.0 - v;
            let x = one_u * one_v * scene.quad[0][0]
                + u * one_v * scene.quad[1][0]
                + u * v * scene.quad[2][0]
                + one_u * v * scene.quad[3][0];
            let y = one_u * one_v * scene.quad[0][1]
                + u * one_v * scene.quad[1][1]
                + u * v * scene.quad[2][1]
                + one_u * v * scene.quad[3][1];
            values.push(image.rgb_at(x.round() as u32, y.round() as u32));
        }
    }
    values
}

fn coefficient_of_variation(samples: &[[f32; 3]]) -> f64 {
    let luminance: Vec<f64> = samples
        .iter()
        .map(|rgb| (f64::from(rgb[0]) + f64::from(rgb[1]) + f64::from(rgb[2])) / 3.0)
        .collect();
    let mean = luminance.iter().sum::<f64>() / luminance.len() as f64;
    let variance = luminance
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / luminance.len() as f64;
    variance.sqrt() / mean
}

#[test]
fn clean_white_patch_is_in_band_without_clipping() {
    let path = temp_path("clean");
    let scene = ChartScene::default();
    render_chart_dng(&path, &scene).unwrap();
    let image = decode_dng(&path).unwrap();
    fs::remove_file(path).unwrap();
    let samples = patch_samples(&image, &scene, 18);
    let mut means = [0.0_f64; 3];
    for rgb in &samples {
        for channel in 0..3 {
            means[channel] += f64::from(rgb[channel]);
        }
    }
    for mean in &mut means {
        *mean /= samples.len() as f64;
    }
    println!("clean white means: {means:?}");
    assert!(
        means.iter().all(|mean| (0.5..=0.95).contains(mean)),
        "white means: {means:?}"
    );
    assert_eq!(image.clipped.iter().filter(|flags| **flags != 0).count(), 0);
}

#[test]
fn glare_patch_exceeds_cv_gate_without_clipping() {
    let path = temp_path("glare");
    let scene = ChartScene {
        defect: SceneDefect::Glare { patch: 11 },
        ..ChartScene::default()
    };
    render_chart_dng(&path, &scene).unwrap();
    let image = decode_dng(&path).unwrap();
    fs::remove_file(path).unwrap();
    let samples = patch_samples(&image, &scene, 11);
    let cv = coefficient_of_variation(&samples);
    println!("glare coefficient of variation: {cv:.9}");
    assert!(cv > 0.05, "glare CV: {cv}");
    assert_eq!(image.clipped.iter().filter(|flags| **flags != 0).count(), 0);
}
