//! Conservative, thumbnail-only ColorChecker Classic locator.
//!
//! Patches are found as separate bright squares at several luminance levels,
//! grouped into a 6-by-4 lattice, and then checked against the whole grid:
//! dark gaps, descending neutral row, and colored patches. Dark backing that
//! touches other dark scene content does not matter. A grid of bright squares
//! alone is not a chart.

use crate::calibration::ChartQuad;
use crate::decode::DecodedImage;

const MAX_SIDE: u32 = 512;
const MIN_SPAN: f64 = 48.0;
const LEVELS: [f32; 7] = [0.045, 0.075, 0.11, 0.16, 0.23, 0.32, 0.44];

/// A location is returned only when one chart passes the grid checks.
#[derive(Debug, Clone, PartialEq)]
pub enum Detection {
    Found(ChartQuad),
    Missing,
    Ambiguous,
}

struct Thumbnail {
    width: usize,
    height: usize,
    rgb: Vec<[f32; 3]>,
    light: Vec<f32>,
}

impl Thumbnail {
    fn new(image: &DecodedImage) -> Option<Self> {
        let w = image.width as usize;
        let h = image.height as usize;
        let pixels = w.checked_mul(h)?;
        if w == 0 || h == 0 || pixels.checked_mul(3)? != image.rgb.len() {
            return None;
        }
        let scale = (image.width.max(image.height) as f64 / f64::from(MAX_SIDE)).max(1.0);
        let width = (w as f64 / scale).round().max(1.0) as usize;
        let height = (h as f64 / scale).round().max(1.0) as usize;
        let mut rgb = Vec::with_capacity(width * height);
        let mut light = Vec::with_capacity(width * height);
        for y in 0..height {
            let sy = ((y as f64 + 0.5) * h as f64 / height as f64) as usize;
            for x in 0..width {
                let sx = ((x as f64 + 0.5) * w as f64 / width as f64) as usize;
                let i = (sy.min(h - 1) * w + sx.min(w - 1)) * 3;
                let color = [image.rgb[i], image.rgb[i + 1], image.rgb[i + 2]];
                rgb.push(color);
                light.push((color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722).max(0.0));
            }
        }
        Some(Self {
            width,
            height,
            rgb,
            light,
        })
    }

    fn sample(&self, p: [f64; 2]) -> Option<([f64; 3], f64)> {
        let x = p[0].floor() as isize;
        let y = p[1].floor() as isize;
        if x < 0 || y < 0 || x >= self.width as isize || y >= self.height as isize {
            return None;
        }
        let i = y as usize * self.width + x as usize;
        let c = self.rgb[i];
        Some((
            [f64::from(c[0]), f64::from(c[1]), f64::from(c[2])],
            f64::from(self.light[i]),
        ))
    }
}

fn point(quad: &[[f64; 2]; 4], u: f64, v: f64) -> [f64; 2] {
    let a = (1.0 - u) * (1.0 - v);
    let b = u * (1.0 - v);
    let c = u * v;
    let d = (1.0 - u) * v;
    [
        a * quad[0][0] + b * quad[1][0] + c * quad[2][0] + d * quad[3][0],
        a * quad[0][1] + b * quad[1][1] + c * quad[2][1] + d * quad[3][1],
    ]
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

// Sample an interior 3-by-3 footprint, rather than trusting a single pixel.
fn patch(t: &Thumbnail, quad: &[[f64; 2]; 4], col: usize, row: usize) -> Option<([f64; 3], f64)> {
    let mut color = [0.0; 3];
    let mut luminance = 0.0;
    for dy in [-0.12, 0.0, 0.12] {
        for dx in [-0.12, 0.0, 0.12] {
            let (sample, l) = t.sample(point(
                quad,
                (col as f64 + 0.5 + dx) / 6.0,
                (row as f64 + 0.5 + dy) / 4.0,
            ))?;
            for k in 0..3 {
                color[k] += sample[k] / 9.0;
            }
            luminance += l / 9.0;
        }
    }
    Some((color, luminance))
}

fn valid_grid(t: &Thumbnail, quad: &[[f64; 2]; 4]) -> bool {
    let mut colors = [[[0.0; 3]; 6]; 4];
    let mut lights = [[0.0; 6]; 4];
    for row in 0..4 {
        for col in 0..6 {
            let Some((color, light)) = patch(t, quad, col, row) else {
                return false;
            };
            if !light.is_finite() || color.iter().any(|c| !c.is_finite()) {
                return false;
            }
            colors[row][col] = color;
            lights[row][col] = light;
        }
    }
    // Gap samples must be darker than their adjoining patch interiors. The
    // requirement is deliberately tolerant of dark skin, blue and black cells.
    let mut edges = 0;
    let mut dark_gaps = 0;
    for (row, line) in lights.iter().enumerate() {
        for col in 0..5 {
            let p = point(quad, (col + 1) as f64 / 6.0, (row as f64 + 0.5) / 4.0);
            let Some((_, gap)) = t.sample(p) else {
                return false;
            };
            edges += 1;
            if gap + 0.018 < line[col].min(line[col + 1]) {
                dark_gaps += 1;
            }
        }
    }
    for (row, pair) in lights.windows(2).enumerate() {
        for (col, &light) in pair[0].iter().enumerate() {
            let p = point(quad, (col as f64 + 0.5) / 6.0, (row + 1) as f64 / 4.0);
            let Some((_, gap)) = t.sample(p) else {
                return false;
            };
            edges += 1;
            if gap + 0.018 < light.min(pair[1][col]) {
                dark_gaps += 1;
            }
        }
    }
    if dark_gaps * 10 < edges * 5 {
        return false;
    }

    // The last row is six descending neutrals. Checking relative chromaticity
    // against its first patch permits ordinary camera white-balance casts.
    let neutral = &lights[3];
    if neutral[0] < 0.20 || neutral[0] - neutral[5] < 0.18 {
        return false;
    }
    if (0..5).any(|i| neutral[i] < neutral[i + 1] + 0.012) {
        return false;
    }
    let white = colors[3][0];
    let mut chromatic = 0;
    for col in 1..5 {
        for k in 0..3 {
            if white[k] > 0.08 && colors[3][col][k] > 0.04 {
                let relative = colors[3][col][k] / white[k];
                let relative_l = neutral[col] / neutral[0];
                if (relative - relative_l).abs() > 0.16 {
                    chromatic += 1;
                }
            }
        }
    }
    if chromatic > 3 {
        return false;
    }
    // A grayscale step wedge on a dark rectangle is not a ColorChecker.
    let colorful = colors[..3]
        .iter()
        .flatten()
        .filter(|c| {
            let hi = c[0].max(c[1]).max(c[2]);
            let lo = c[0].min(c[1]).min(c[2]);
            hi - lo > 0.09 && hi > lo * 1.18
        })
        .count();
    colorful >= 6
}

/// A bright, roughly square region that could be one chart patch.
struct Blob {
    center: [f64; 2],
    size: f64,
}

const MAX_BLOBS: usize = 1500;

// Patches are found as separate bright squares above `threshold`. This works
// when the chart backing touches other dark scene content, because only the
// gaps between patches need to separate them.
fn blobs(t: &Thumbnail, threshold: f32, seen: &mut [bool], queue: &mut Vec<usize>) -> Vec<Blob> {
    seen.fill(false);
    let mut out = Vec::new();
    let max_dim = t.width.max(t.height) as f64;
    for start in 0..t.light.len() {
        if seen[start] || t.light[start] <= threshold {
            continue;
        }
        seen[start] = true;
        queue.clear();
        queue.push(start);
        let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0, usize::MAX, 0);
        let mut pos = 0;
        while pos < queue.len() {
            let index = queue[pos];
            pos += 1;
            let x = index % t.width;
            let y = index / t.width;
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            let neighbours = [
                (x > 0).then(|| index - 1),
                (x + 1 < t.width).then(|| index + 1),
                (y > 0).then(|| index - t.width),
                (y + 1 < t.height).then(|| index + t.width),
            ];
            for next in neighbours.into_iter().flatten() {
                if !seen[next] && t.light[next] > threshold {
                    seen[next] = true;
                    queue.push(next);
                }
            }
        }
        if x0 == 0 || y0 == 0 || x1 + 1 == t.width || y1 + 1 == t.height {
            continue;
        }
        let w = (x1 - x0 + 1) as f64;
        let h = (y1 - y0 + 1) as f64;
        let area = queue.len() as f64;
        if w.min(h) < 5.0 || w.max(h) > 0.2 * max_dim || !(0.6..=1.6).contains(&(w / h)) {
            continue;
        }
        if area < 0.4 * w * h {
            continue;
        }
        out.push(Blob {
            center: [(x0 + x1 + 1) as f64 * 0.5, (y0 + y1 + 1) as f64 * 0.5],
            size: area.sqrt(),
        });
        if out.len() > MAX_BLOBS {
            // Texture, not a chart: fail closed instead of searching it.
            return Vec::new();
        }
    }
    out
}

// Groups similarly sized blobs that touch as lattice neighbours.
fn clusters(blobs: &[Blob]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..blobs.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..blobs.len() {
        for j in i + 1..blobs.len() {
            let ratio = blobs[i].size / blobs[j].size;
            let reach = 1.6 * blobs[i].size.max(blobs[j].size);
            if (0.7..=1.43).contains(&ratio) && distance(blobs[i].center, blobs[j].center) <= reach
            {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    for i in 0..blobs.len() {
        let root = find(&mut parent, i);
        match roots.iter().position(|r| *r == root) {
            Some(g) => groups[g].push(i),
            None => {
                roots.push(root);
                groups.push(vec![i]);
            }
        }
    }
    groups.retain(|g| (14..=24).contains(&g.len()));
    groups
}

fn solve4(mut m: [[f64; 5]; 4]) -> Option<[f64; 4]> {
    for col in 0..4 {
        let pivot = (col..4).max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs()))?;
        if m[pivot][col].abs() < 1e-9 {
            return None;
        }
        m.swap(col, pivot);
        let pivot_row = m[col];
        for (row, line) in m.iter_mut().enumerate() {
            if row != col {
                let f = line[col] / pivot_row[col];
                for (value, base) in line.iter_mut().zip(pivot_row).skip(col) {
                    *value -= f * base;
                }
            }
        }
    }
    Some([
        m[0][4] / m[0][0],
        m[1][4] / m[1][1],
        m[2][4] / m[2][2],
        m[3][4] / m[3][3],
    ])
}

// Assigns each patch a (column, row) cell from its position along the lattice
// axes, then fits a bilinear chart map so missing patches do not matter.
fn lattice(blobs: &[Blob], members: &[usize]) -> Option<[[f64; 2]; 4]> {
    let n = members.len();
    let (mut s, mut c) = (0.0, 0.0);
    let mut nearest = Vec::with_capacity(n);
    for &a in members {
        let mut best = (f64::INFINITY, [0.0; 2]);
        for &b in members {
            if a != b {
                let d = distance(blobs[a].center, blobs[b].center);
                if d < best.0 {
                    best = (
                        d,
                        [
                            blobs[b].center[0] - blobs[a].center[0],
                            blobs[b].center[1] - blobs[a].center[1],
                        ],
                    );
                }
            }
        }
        let angle = best.1[1].atan2(best.1[0]);
        s += (4.0 * angle).sin();
        c += (4.0 * angle).cos();
        nearest.push(best.0);
    }
    if s.hypot(c) / (n as f64) < 0.75 {
        return None;
    }
    nearest.sort_by(f64::total_cmp);
    let pitch = nearest[n / 2];
    let theta = s.atan2(c) / 4.0;
    let mut u = [theta.cos(), theta.sin()];
    let mut v = [-u[1], u[0]];
    let project =
        |axis: [f64; 2], b: usize| blobs[b].center[0] * axis[0] + blobs[b].center[1] * axis[1];
    let extent = |axis: [f64; 2]| {
        let (lo, hi) = members
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &b| {
                (lo.min(project(axis, b)), hi.max(project(axis, b)))
            });
        hi - lo
    };
    if extent(u) < extent(v) {
        (u, v) = (v, [-u[0], -u[1]]);
    }
    let min_u = members
        .iter()
        .map(|&b| project(u, b))
        .fold(f64::INFINITY, f64::min);
    let min_v = members
        .iter()
        .map(|&b| project(v, b))
        .fold(f64::INFINITY, f64::min);
    let mut occupied = [[false; 6]; 4];
    let mut normal = [[0.0; 5]; 4];
    let mut rhs_y = [0.0; 4];
    let mut cells = Vec::with_capacity(n);
    for &b in members {
        let col = ((project(u, b) - min_u) / pitch).round();
        let row = ((project(v, b) - min_v) / pitch).round();
        if !(0.0..=5.0).contains(&col) || !(0.0..=3.0).contains(&row) {
            return None;
        }
        let (col, row) = (col as usize, row as usize);
        if occupied[row][col] {
            return None;
        }
        occupied[row][col] = true;
        cells.push((b, (col as f64 + 0.5) / 6.0, (row as f64 + 0.5) / 4.0));
    }
    if !occupied.iter().any(|r| r[5]) || !occupied[3].iter().any(|c| *c) {
        return None;
    }
    for &(b, cu, cv) in &cells {
        let basis = [1.0, cu, cv, cu * cv];
        let p = blobs[b].center;
        for i in 0..4 {
            for j in 0..4 {
                normal[i][j] += basis[i] * basis[j];
            }
            normal[i][4] += basis[i] * p[0];
            rhs_y[i] += basis[i] * p[1];
        }
    }
    let fx = solve4(normal)?;
    for i in 0..4 {
        normal[i][4] = rhs_y[i];
    }
    let fy = solve4(normal)?;
    let map = |cu: f64, cv: f64| {
        let basis = [1.0, cu, cv, cu * cv];
        [
            (0..4).map(|i| fx[i] * basis[i]).sum::<f64>(),
            (0..4).map(|i| fy[i] * basis[i]).sum::<f64>(),
        ]
    };
    let result = [map(0.0, 0.0), map(1.0, 0.0), map(1.0, 1.0), map(0.0, 1.0)];
    let width = (distance(result[0], result[1]) + distance(result[2], result[3])) * 0.5;
    let height = (distance(result[0], result[3]) + distance(result[1], result[2])) * 0.5;
    if width < MIN_SPAN || !(1.15..=2.20).contains(&(width / height)) {
        return None;
    }
    Some(result)
}

fn same_region(a: &[[f64; 2]; 4], b: &[[f64; 2]; 4]) -> bool {
    let ca = point(a, 0.5, 0.5);
    let cb = point(b, 0.5, 0.5);
    let span = distance(a[0], a[2]).min(distance(b[0], b[2]));
    distance(ca, cb) < span * 0.25
}

/// Locate a Classic chart without choosing its physical reference revision.
/// Only up to 512 pixels per image side are read, irrespective of RAW size.
/// A too-small chart, an occluded grid, or one without separable dark gaps
/// returns `Missing`; more than one plausible region returns `Ambiguous`.
pub fn detect_chart(image: &DecodedImage) -> Detection {
    let Some(t) = Thumbnail::new(image) else {
        return Detection::Missing;
    };
    let mut candidates: Vec<[[f64; 2]; 4]> = Vec::new();
    let mut seen = vec![false; t.width * t.height];
    let mut queue = Vec::<usize>::new();
    for threshold in LEVELS {
        let found = blobs(&t, threshold, &mut seen, &mut queue);
        for members in clusters(&found) {
            let Some(quad) = lattice(&found, &members) else {
                continue;
            };
            // The lattice orientation is only known up to a half turn; the
            // neutral row disambiguates the chart's upright orientation.
            let reversed = [quad[2], quad[3], quad[0], quad[1]];
            let oriented = if valid_grid(&t, &quad) {
                Some(quad)
            } else if valid_grid(&t, &reversed) {
                Some(reversed)
            } else {
                None
            };
            if let Some(oriented) = oriented {
                if !candidates.iter().any(|old| same_region(old, &oriented)) {
                    candidates.push(oriented);
                    if candidates.len() > 1 {
                        return Detection::Ambiguous;
                    }
                }
            }
        }
    }
    let Some(quad) = candidates.pop() else {
        return Detection::Missing;
    };
    let sx = image.width as f64 / t.width as f64;
    let sy = image.height as f64 / t.height as f64;
    Detection::Found(ChartQuad {
        corners: quad.map(|p| [p[0] * sx, p[1] * sy]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{CameraIdentity, SensorLayout};

    const COLORS: [[f32; 3]; 24] = [
        [0.42, 0.25, 0.19],
        [0.74, 0.52, 0.42],
        [0.26, 0.37, 0.59],
        [0.27, 0.36, 0.20],
        [0.43, 0.40, 0.65],
        [0.27, 0.64, 0.60],
        [0.82, 0.39, 0.13],
        [0.21, 0.26, 0.54],
        [0.73, 0.24, 0.26],
        [0.36, 0.19, 0.42],
        [0.55, 0.67, 0.22],
        [0.92, 0.60, 0.15],
        [0.14, 0.20, 0.48],
        [0.19, 0.46, 0.23],
        [0.68, 0.15, 0.15],
        [0.94, 0.78, 0.17],
        [0.69, 0.22, 0.51],
        [0.16, 0.54, 0.64],
        [0.90, 0.90, 0.90],
        [0.66, 0.66, 0.66],
        [0.48, 0.48, 0.48],
        [0.32, 0.32, 0.32],
        [0.19, 0.19, 0.19],
        [0.07, 0.07, 0.07],
    ];

    fn image(width: u32, height: u32) -> DecodedImage {
        DecodedImage {
            sensor_layout: SensorLayout::Rendered,
            width,
            height,
            rgb: vec![0.72; width as usize * height as usize * 3],
            clipped: vec![0; width as usize * height as usize],
            black_levels: [0; 4],
            white_levels: [65535; 4],
            cfa_pattern: [0; 4],
            display_neutral: None,
            camera: CameraIdentity {
                make: String::new(),
                model: String::new(),
                decoder: String::new(),
                decoder_version: String::new(),
            },
        }
    }

    fn paint(image: &mut DecodedImage, quad: [[f64; 2]; 4]) {
        let left = quad
            .iter()
            .map(|p| p[0])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as u32;
        let right = quad
            .iter()
            .map(|p| p[0])
            .fold(0.0_f64, f64::max)
            .ceil()
            .min(image.width as f64) as u32;
        let top = quad
            .iter()
            .map(|p| p[1])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as u32;
        let bottom = quad
            .iter()
            .map(|p| p[1])
            .fold(0.0_f64, f64::max)
            .ceil()
            .min(image.height as f64) as u32;
        for y in top..bottom {
            for x in left..right {
                let target = [x as f64 + 0.5, y as f64 + 0.5];
                let mut u = 0.5;
                let mut v = 0.5;
                // Invert the bilinear chart map with a few Newton steps.
                for _ in 0..9 {
                    let p = point(&quad, u, v);
                    let du = [
                        quad[1][0] - quad[0][0]
                            + v * (quad[2][0] + quad[0][0] - quad[1][0] - quad[3][0]),
                        quad[1][1] - quad[0][1]
                            + v * (quad[2][1] + quad[0][1] - quad[1][1] - quad[3][1]),
                    ];
                    let dv = [
                        quad[3][0] - quad[0][0]
                            + u * (quad[2][0] + quad[0][0] - quad[1][0] - quad[3][0]),
                        quad[3][1] - quad[0][1]
                            + u * (quad[2][1] + quad[0][1] - quad[1][1] - quad[3][1]),
                    ];
                    let det = du[0] * dv[1] - du[1] * dv[0];
                    if det.abs() < 1e-6 {
                        break;
                    }
                    let dx = target[0] - p[0];
                    let dy = target[1] - p[1];
                    u += (dx * dv[1] - dy * dv[0]) / det;
                    v += (dy * du[0] - dx * du[1]) / det;
                }
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let col = (u * 6.0) as usize;
                let row = (v * 4.0) as usize;
                let frac_u = u * 6.0 - col as f64;
                let frac_v = v * 4.0 - row as f64;
                let color = if (0.08..0.92).contains(&frac_u) && (0.10..0.90).contains(&frac_v) {
                    COLORS[row * 6 + col]
                } else {
                    [0.02; 3]
                };
                let i = (y as usize * image.width as usize + x as usize) * 3;
                image.rgb[i..i + 3].copy_from_slice(&color);
            }
        }
    }

    fn assert_near(image: &DecodedImage, expected: [[f64; 2]; 4]) {
        let Detection::Found(found) = detect_chart(image) else {
            panic!("chart not found");
        };
        for (actual, wanted) in found.corners.iter().zip(expected) {
            assert!(distance(*actual, wanted) < 16.0, "{actual:?} != {wanted:?}");
        }
    }

    #[test]
    fn locates_offset_chart_and_upright_orientation() {
        let mut frame = image(720, 520);
        let quad = [[99.0, 115.0], [400.0, 115.0], [400.0, 318.0], [99.0, 318.0]];
        paint(&mut frame, quad);
        assert_near(&frame, quad);
    }

    #[test]
    fn locates_rotated_and_perspective_charts() {
        let mut rotated = image(720, 520);
        let quad = [
            [320.0, 68.0],
            [526.0, 233.0],
            [403.0, 387.0],
            [197.0, 222.0],
        ];
        paint(&mut rotated, quad);
        assert_near(&rotated, quad);

        let mut perspective = image(720, 520);
        let quad = [
            [135.0, 101.0],
            [457.0, 128.0],
            [421.0, 329.0],
            [157.0, 309.0],
        ];
        paint(&mut perspective, quad);
        assert_near(&perspective, quad);
    }

    #[test]
    fn locates_chart_whose_backing_merges_with_dark_scene() {
        let mut frame = image(720, 520);
        for value in frame.rgb.iter_mut() {
            *value = 0.02;
        }
        let quad = [[99.0, 115.0], [400.0, 115.0], [400.0, 318.0], [99.0, 318.0]];
        paint(&mut frame, quad);
        assert_near(&frame, quad);
    }

    #[test]
    fn misses_blank_and_non_chart_rectangles() {
        let mut frame = image(640, 440);
        assert_eq!(detect_chart(&frame), Detection::Missing);
        for y in 80..320 {
            for x in 100..460 {
                let i = (y * 640 + x) * 3;
                frame.rgb[i..i + 3].copy_from_slice(&[0.025; 3]);
            }
        }
        assert_eq!(detect_chart(&frame), Detection::Missing);
    }

    #[test]
    fn refuses_to_choose_between_two_charts() {
        let mut frame = image(900, 520);
        paint(
            &mut frame,
            [[55.0, 85.0], [325.0, 85.0], [325.0, 268.0], [55.0, 268.0]],
        );
        paint(
            &mut frame,
            [
                [518.0, 183.0],
                [791.0, 183.0],
                [791.0, 367.0],
                [518.0, 367.0],
            ],
        );
        assert_eq!(detect_chart(&frame), Detection::Ambiguous);
    }
}
