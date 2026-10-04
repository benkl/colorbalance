//! Deterministic synthetic ColorChecker DNG fixtures.

use std::io;
use std::path::Path;
use std::sync::LazyLock;

use colorbalance_core::color::{
    bradford_d50_to_d65, linear_srgb_to_xyz, mat_vec, xyz_to_linear_srgb,
};
use colorbalance_core::dataset;
use colorbalance_core::ChartRevision;
use colorbalance_raw::{write_dng, DngWriteSpec};

/// Definition of one rendered chart scene.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartScene {
    pub width: u32,
    pub height: u32,
    /// Chart corners in top-left, top-right, bottom-right, bottom-left order.
    pub quad: [[f64; 2]; 4],
    pub revision: ChartRevision,
    /// Multiplier applied to camera RGB before quantization.
    pub exposure: f64,
    pub defect: SceneDefect,
}

impl Default for ChartScene {
    fn default() -> Self {
        Self {
            width: 480,
            height: 320,
            quad: [[40.0, 40.0], [440.0, 34.0], [440.0, 280.0], [40.0, 280.0]],
            revision: ChartRevision::ClassicBeforeNovember2014,
            exposure: 1.0,
            defect: SceneDefect::Clean,
        }
    }
}

/// A controlled defect applied to a rendered chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneDefect {
    Clean,
    ClipWhitePatch,
    ClipSingleChannel { patch: usize },
    Glare { patch: usize },
}

const CROSS_TERMS: [[f64; 3]; 3] = [[1.02, 0.03, 0.00], [0.02, 0.99, 0.02], [0.00, 0.04, 1.04]];
const GAINS: [f64; 3] = [1.05, 0.98, 1.06];

fn transform_basis() -> [[f64; 3]; 3] {
    let bradford = bradford_d50_to_d65();
    let mut matrix = [[0.0; 3]; 3];
    for input in 0..3 {
        let mut basis = [0.0; 3];
        basis[input] = 1.0;
        let adapted = [
            bradford[0][0] * basis[0] + bradford[0][1] * basis[1] + bradford[0][2] * basis[2],
            bradford[1][0] * basis[0] + bradford[1][1] * basis[1] + bradford[1][2] * basis[2],
            bradford[2][0] * basis[0] + bradford[2][1] * basis[1] + bradford[2][2] * basis[2],
        ];
        let srgb = xyz_to_linear_srgb(adapted);
        let mixed = [
            CROSS_TERMS[0][0] * srgb[0] + CROSS_TERMS[0][1] * srgb[1] + CROSS_TERMS[0][2] * srgb[2],
            CROSS_TERMS[1][0] * srgb[0] + CROSS_TERMS[1][1] * srgb[1] + CROSS_TERMS[1][2] * srgb[2],
            CROSS_TERMS[2][0] * srgb[0] + CROSS_TERMS[2][1] * srgb[1] + CROSS_TERMS[2][2] * srgb[2],
        ];
        for output in 0..3 {
            // Fixed sensor sensitivity resolves the specified exposure=1 clean
            // scene otherwise clipping the white patch in R and B.
            matrix[input][output] = mixed[output] * GAINS[output] * 0.89;
        }
    }
    matrix
}

/// Fixed synthetic camera response matrix.
///
/// Runtime composition is required by the contract, so this is a lazily
/// initialized static rather than the impossible runtime-valued `const` form.
pub static CAMERA_MATRIX: LazyLock<[[f64; 3]; 3]> = LazyLock::new(transform_basis);

/// Return the runtime-composed synthetic camera response matrix.
pub fn camera_matrix() -> [[f64; 3]; 3] {
    *CAMERA_MATRIX
}

fn io_other(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

fn camera_rgb(xyz_d50: [f64; 3]) -> [f64; 3] {
    mat_vec(camera_matrix(), xyz_d50)
}

fn inverse_bilinear(point: [f64; 2], quad: [[f64; 2]; 4]) -> Option<[f64; 2]> {
    let mut u = (point[0] - quad[0][0]) / (quad[1][0] - quad[0][0]);
    let mut v = (point[1] - quad[0][1]) / (quad[3][1] - quad[0][1]);
    for _ in 0..8 {
        let one_u = 1.0 - u;
        let one_v = 1.0 - v;
        let mapped = [
            one_u * one_v * quad[0][0]
                + u * one_v * quad[1][0]
                + u * v * quad[2][0]
                + one_u * v * quad[3][0],
            one_u * one_v * quad[0][1]
                + u * one_v * quad[1][1]
                + u * v * quad[2][1]
                + one_u * v * quad[3][1],
        ];
        let error = [mapped[0] - point[0], mapped[1] - point[1]];
        let du = [
            one_v * (quad[1][0] - quad[0][0]) + v * (quad[2][0] - quad[3][0]),
            one_v * (quad[1][1] - quad[0][1]) + v * (quad[2][1] - quad[3][1]),
        ];
        let dv = [
            one_u * (quad[3][0] - quad[0][0]) + u * (quad[2][0] - quad[1][0]),
            one_u * (quad[3][1] - quad[0][1]) + u * (quad[2][1] - quad[1][1]),
        ];
        let determinant = du[0] * dv[1] - du[1] * dv[0];
        if determinant.abs() < 1e-12 {
            return None;
        }
        u -= (error[0] * dv[1] - error[1] * dv[0]) / determinant;
        v -= (du[0] * error[1] - du[1] * error[0]) / determinant;
    }
    ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some([u, v])
}

fn chart_sample(
    point: [f64; 2],
    scene: &ChartScene,
    colors: &[[f64; 3]],
    surround: [f64; 3],
) -> ([f64; 3], Option<usize>) {
    let Some([u, v]) = inverse_bilinear(point, scene.quad) else {
        return (surround, None);
    };
    let chart_x = (u * 6.0).min(6.0 - f64::EPSILON);
    let chart_y = (v * 4.0).min(4.0 - f64::EPSILON);
    let column = chart_x.floor() as usize;
    let row = chart_y.floor() as usize;
    let local_x = chart_x.fract();
    let local_y = chart_y.fract();
    if !(0.01..=0.99).contains(&local_x) || !(0.01..=0.99).contains(&local_y) {
        return ([0.01; 3], None);
    }
    let patch = row * 6 + column;
    let mut color = colors[patch];
    if let SceneDefect::Glare { patch: affected } = scene.defect {
        if affected == patch {
            let dx = (local_x - 0.5) * 2.0;
            let dy = (local_y - 0.5) * 2.0;
            let radial = (1.0 - (dx * dx + dy * dy).sqrt()).max(0.0);
            for channel in &mut color {
                *channel += (0.88 - *channel) * 0.9 * radial;
            }
        }
    }
    (color, Some(patch))
}

/// Render a synthetic chart as a minimal uncompressed DNG.
pub fn render_chart_dng(path: &Path, scene: &ChartScene) -> io::Result<()> {
    if scene.width == 0 || scene.height == 0 || !scene.exposure.is_finite() || scene.exposure < 0.0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid chart scene",
        ));
    }
    let data = dataset::load(scene.revision).map_err(io_other)?;
    let mut colors: Vec<[f64; 3]> = data
        .patches
        .iter()
        .map(|patch| camera_rgb(patch.xyz_d50).map(|v| v * scene.exposure))
        .collect();
    if let SceneDefect::ClipWhitePatch = scene.defect {
        colors[18] = [1.01; 3];
    }
    let surround = camera_rgb(linear_srgb_to_xyz([0.18; 3])).map(|v| v * scene.exposure);
    let cfa_pattern = *b"RGGB";
    let black_levels = [512, 520, 516, 524];
    let white_levels = [15_000; 4];
    let pixel_count = (scene.width as usize)
        .checked_mul(scene.height as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "scene dimensions overflow"))?;
    let mut photosites = Vec::with_capacity(pixel_count);
    for y in 0..scene.height {
        for x in 0..scene.width {
            let (color, patch) = chart_sample(
                [f64::from(x) + 0.5, f64::from(y) + 0.5],
                scene,
                &colors,
                surround,
            );
            let cfa = ((y % 2) * 2 + x % 2) as usize;
            let channel = match cfa_pattern[cfa] {
                b'R' => 0,
                b'G' => 1,
                b'B' => 2,
                _ => unreachable!(),
            };
            let mut value = color[channel];
            if let SceneDefect::ClipSingleChannel { patch: affected } = scene.defect {
                if patch == Some(affected) && channel == 0 {
                    value = (value * 1.5).max(1.0);
                }
            }
            let black = black_levels[cfa];
            let white = white_levels[cfa];
            let raw = (value * f64::from(white) + f64::from(black))
                .round()
                .clamp(f64::from(black), f64::from(white)) as u16;
            photosites.push(raw);
        }
    }
    write_dng(
        path,
        &DngWriteSpec {
            width: scene.width,
            height: scene.height,
            cfa_pattern,
            black_levels,
            white_levels,
            make: "ColorBalance".to_owned(),
            model: "Synthetic ColorChecker".to_owned(),
            orientation: 1,
            photosites,
        },
    )
}
