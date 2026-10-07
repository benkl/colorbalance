//! Adaptive homogeneity-directed demosaicing of a normalized Bayer mosaic.

const RED: u8 = b'R';
const GREEN: u8 = b'G';
const BLUE: u8 = b'B';
const CLIP_RADIUS: usize = 5;

fn color(cfa: [u8; 4], x: usize, y: usize) -> u8 {
    cfa[(y & 1) * 2 + (x & 1)]
}

/// Find the reflected neighbor with the requested CFA color. If the entire
/// image lacks it, fall back to the current site on a degenerate image.
fn neighbor_index(
    width: usize,
    height: usize,
    cfa: [u8; 4],
    x: usize,
    y: usize,
    dx: isize,
    dy: isize,
) -> usize {
    let target = cfa[((y & 1) ^ (dy.unsigned_abs() & 1)) * 2 + ((x & 1) ^ (dx.unsigned_abs() & 1))];
    for (ox, oy) in [(dx, dy), (-dx, -dy)] {
        if let (Some(nx), Some(ny)) = (x.checked_add_signed(ox), y.checked_add_signed(oy)) {
            if nx < width && ny < height && color(cfa, nx, ny) == target {
                return ny * width + nx;
            }
        }
    }
    for radius in 0..=2_usize {
        for ny in y.saturating_sub(radius)..=y.saturating_add(radius).min(height - 1) {
            for nx in x.saturating_sub(radius)..=x.saturating_add(radius).min(width - 1) {
                if color(cfa, nx, ny) == target {
                    return ny * width + nx;
                }
            }
        }
    }
    y * width + x
}

#[allow(clippy::too_many_arguments)]
fn neighbor(
    raw: &[f32],
    width: usize,
    height: usize,
    cfa: [u8; 4],
    x: usize,
    y: usize,
    dx: isize,
    dy: isize,
) -> f32 {
    raw[neighbor_index(width, height, cfa, x, y, dx, dy)]
}

fn clipping(width: usize, height: usize, cfa: [u8; 4], saturated: &[bool]) -> Vec<u8> {
    let mut result = vec![0; width * height];
    let mut columns = vec![[0_usize; 3]; width];
    for y in 0..height {
        if y > CLIP_RADIUS {
            let departing = y - CLIP_RADIUS - 1;
            for x in 0..width {
                if saturated[departing * width + x] {
                    let channel = match color(cfa, x, departing) {
                        RED => 0,
                        GREEN => 1,
                        BLUE => 2,
                        _ => unreachable!(),
                    };
                    columns[x][channel] -= 1;
                }
            }
        }
        let first = if y == 0 {
            0
        } else {
            y.saturating_add(CLIP_RADIUS).min(height)
        };
        let end = y.saturating_add(CLIP_RADIUS).saturating_add(1).min(height);
        for entering in first..end {
            for x in 0..width {
                if saturated[entering * width + x] {
                    let channel = match color(cfa, x, entering) {
                        RED => 0,
                        GREEN => 1,
                        BLUE => 2,
                        _ => unreachable!(),
                    };
                    columns[x][channel] += 1;
                }
            }
        }
        let mut counts = [0_usize; 3];
        for x in 0..width {
            if x > CLIP_RADIUS {
                for channel in 0..3 {
                    counts[channel] -= columns[x - CLIP_RADIUS - 1][channel];
                }
            }
            let first = if x == 0 {
                0
            } else {
                x.saturating_add(CLIP_RADIUS).min(width)
            };
            let end = x.saturating_add(CLIP_RADIUS).saturating_add(1).min(width);
            for column in &columns[first..end] {
                for channel in 0..3 {
                    counts[channel] += column[channel];
                }
            }
            result[y * width + x] = u8::from(counts[0] > 0)
                | (u8::from(counts[1] > 0) << 1)
                | (u8::from(counts[2] > 0) << 2);
        }
    }
    result
}

fn lab(rgb: &[f32; 3]) -> [f32; 3] {
    // A fixed D65 linear RGB to XYZ metric. This is used solely to compare
    // local directions; the output remains unaltered camera RGB.
    let [r, g, b] = [rgb[0], rgb[1], rgb[2]];
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = (0.0193339 * r + 0.119_192 * g + 0.9503041 * b) / 1.08883;
    fn f(t: f32) -> f32 {
        if t > 0.008856452 {
            t.cbrt()
        } else {
            7.787037 * t + 16.0 / 116.0
        }
    }
    let [fx, fy, fz] = [f(x), f(y), f(z)];
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn difference(a: &[f32; 3], b: &[f32; 3]) -> (f32, f32) {
    let dl = (a[0] - b[0]).abs();
    let da = a[1] - b[1];
    let db = a[2] - b[2];
    (dl, da * da + db * db)
}

/// Demosaic a Bayer mosaic using two green directions, interpolated color
/// differences, and a 3x3 neighborhood vote over CIELab homogeneity.
///
/// `cfa` is one row-major 2x2 tile of ASCII `R`, `G`, `B` with two greens;
/// `normalized` and `saturated` contain one entry per photosite. The caller
/// validates those lengths, dimensions, layout, and finite samples. No white
/// balance, highlight replacement, or output clamp is applied.
///
/// Clip bit 0/1/2 denotes R/G/B. Each output bit is the OR of saturated RAW
/// photosites of that color in the image-clipped 11x11 square centered on the
/// output pixel. Radius five covers both interpolation directions, the 3x3
/// homogeneity vote, and its four-neighbor comparisons. The square is a
/// conservative bound, independent of which direction wins, so a saturated
/// site cannot disappear when the homogeneity decision changes.
///
/// For a truncated border, unavailable neighbors reflect across that border;
/// if an entire CFA color is absent, its channels use the current site as a
/// defined small-image fallback.
///
/// # Panics
///
/// Panics if the caller violates the input preconditions or dimensions exceed
/// addressable memory.
#[must_use]
pub fn demosaic_ahd(
    width: usize,
    height: usize,
    normalized: &[f32],
    cfa: [u8; 4],
    saturated: &[bool],
) -> (Vec<f32>, Vec<u8>) {
    let pixels = width
        .checked_mul(height)
        .expect("image dimensions overflow");
    assert!(width > 0 && height > 0 && normalized.len() == pixels && saturated.len() == pixels);
    assert!(
        cfa.iter().filter(|&&v| v == GREEN).count() == 2
            && cfa.iter().filter(|&&v| v == RED).count() == 1
            && cfa.iter().filter(|&&v| v == BLUE).count() == 1
    );
    assert!(
        (cfa[0] == GREEN && cfa[3] == GREEN) || (cfa[1] == GREEN && cfa[2] == GREEN),
        "Bayer tile must have diagonal greens"
    );
    let len = pixels.checked_mul(3).expect("RGB buffer length overflow");
    let mut rgb = [vec![0.0_f32; len], vec![0.0_f32; len]];
    for y in 0..height {
        for x in 0..width {
            let p = (y * width + x) * 3;
            let site = color(cfa, x, y);
            for (direction, plane) in rgb.iter_mut().enumerate() {
                let green = if site == GREEN {
                    normalized[y * width + x]
                } else {
                    let (dx, dy) = if direction == 0 { (1, 0) } else { (0, 1) };
                    (neighbor(normalized, width, height, cfa, x, y, dx, dy)
                        + neighbor(normalized, width, height, cfa, x, y, -dx, -dy))
                        * 0.5
                        + (2.0 * normalized[y * width + x]
                            - neighbor(normalized, width, height, cfa, x, y, 2 * dx, 2 * dy)
                            - neighbor(normalized, width, height, cfa, x, y, -2 * dx, -2 * dy))
                            * 0.25
                };
                plane[p + 1] = green;
                if site != GREEN {
                    plane[p + if site == RED { 0 } else { 2 }] = normalized[y * width + x];
                }
            }
        }
    }
    // Interpolate (color - green), rather than interpolating color directly.
    // Green at every supporting site belongs to the same directional candidate.
    for plane in &mut rgb {
        for y in 0..height {
            for x in 0..width {
                let p = (y * width + x) * 3;
                let site = color(cfa, x, y);
                for (channel, wanted) in [(0, RED), (2, BLUE)] {
                    if site == wanted {
                        continue;
                    }
                    let offsets: &[(isize, isize)] = if site == GREEN {
                        if color(cfa, x ^ 1, y) == wanted {
                            &[(-1, 0), (1, 0)]
                        } else {
                            &[(0, -1), (0, 1)]
                        }
                    } else {
                        &[(-1, -1), (-1, 1), (1, -1), (1, 1)]
                    };
                    let mut sum = 0.0;
                    for &(dx, dy) in offsets {
                        let support = neighbor_index(width, height, cfa, x, y, dx, dy);
                        sum += normalized[support] - plane[support * 3 + 1];
                    }
                    plane[p + channel] = plane[p + 1] + sum / offsets.len() as f32;
                }
            }
        }
    }
    let labs = [
        rgb[0]
            .as_chunks::<3>()
            .0
            .iter()
            .map(lab)
            .collect::<Vec<_>>(),
        rgb[1]
            .as_chunks::<3>()
            .0
            .iter()
            .map(lab)
            .collect::<Vec<_>>(),
    ];
    let mut votes = [vec![0_u8; pixels], vec![0_u8; pixels]];
    for y in 0..height {
        for x in 0..width {
            let p = y * width + x;
            let adjacent = [
                (x.saturating_sub(1), y),
                ((x + 1).min(width - 1), y),
                (x, y.saturating_sub(1)),
                (x, (y + 1).min(height - 1)),
            ];
            let mut differences = [[(0.0, 0.0); 4]; 2];
            for d in 0..2 {
                for (n, &(nx, ny)) in adjacent.iter().enumerate() {
                    differences[d][n] = difference(&labs[d][p], &labs[d][ny * width + nx]);
                }
            }
            // Each candidate sets its tolerance from the direction in which
            // its green was interpolated (horizontal first, vertical second).
            let threshold_l = differences[0][0]
                .0
                .max(differences[0][1].0)
                .min(differences[1][2].0.max(differences[1][3].0));
            let threshold_ab = differences[0][0]
                .1
                .max(differences[0][1].1)
                .min(differences[1][2].1.max(differences[1][3].1));
            for d in 0..2 {
                votes[d][p] = differences[d]
                    .iter()
                    .filter(|&&(l, ab)| l <= threshold_l && ab <= threshold_ab)
                    .count() as u8;
            }
        }
    }
    let mut out = vec![0.0_f32; len];
    let clipped = clipping(width, height, cfa, saturated);
    for y in 0..height {
        for x in 0..width {
            let p = y * width + x;
            let mut scores = [0_u16; 2];
            for ny in y.saturating_sub(1)..=y.saturating_add(1).min(height - 1) {
                for nx in x.saturating_sub(1)..=x.saturating_add(1).min(width - 1) {
                    for d in 0..2 {
                        scores[d] += u16::from(votes[d][ny * width + nx]);
                    }
                }
            }
            for channel in 0..3 {
                let i = p * 3 + channel;
                out[i] = match scores[0].cmp(&scores[1]) {
                    std::cmp::Ordering::Greater => rgb[0][i],
                    std::cmp::Ordering::Less => rgb[1][i],
                    std::cmp::Ordering::Equal => (rgb[0][i] + rgb[1][i]) * 0.5,
                };
            }
        }
    }
    (out, clipped)
}

#[cfg(test)]
mod tests {
    use super::demosaic_ahd;

    #[test]
    fn constant_color_recovers_unbalanced_camera_channels() {
        // Analytical reference: constant color differences and zero second
        // differences make both candidates identical at every interior pixel.
        let cfa = *b"RGGB";
        let raw = (0..81)
            .map(|i| match cfa[(i / 9 % 2) * 2 + (i % 9) % 2] {
                b'R' => 0.2,
                b'G' => 0.4,
                _ => 0.8,
            })
            .collect::<Vec<_>>();
        let (rgb, clipped) = demosaic_ahd(9, 9, &raw, cfa, &[false; 81]);
        for pixel in rgb.as_chunks::<3>().0 {
            for (value, expected) in pixel.iter().zip([0.2, 0.4, 0.8]) {
                assert!((value - expected).abs() < 1e-6, "{pixel:?}");
            }
        }
        assert_eq!(clipped, vec![0; 81]);
    }

    #[test]
    fn affine_scene_matches_independent_interior_reference() {
        // Exact affine reconstruction: the second directional differences
        // vanish, and R-G and B-G are constant at every supporting site.
        let cfa = *b"GBRG";
        let mut raw = vec![0.0; 121];
        for y in 0..11 {
            for x in 0..11 {
                let green = 0.25 + x as f32 * 0.015 + y as f32 * 0.01;
                raw[y * 11 + x] = green
                    + match cfa[(y % 2) * 2 + x % 2] {
                        b'R' => 0.12,
                        b'B' => -0.08,
                        _ => 0.0,
                    };
            }
        }
        let (rgb, _) = demosaic_ahd(11, 11, &raw, cfa, &[false; 121]);
        for y in 4..=6 {
            for x in 4..=6 {
                let green = 0.25 + x as f32 * 0.015 + y as f32 * 0.01;
                for (actual, expected) in
                    rgb[(y * 11 + x) * 3..][..3]
                        .iter()
                        .zip([green + 0.12, green, green - 0.08])
                {
                    assert!(
                        (actual - expected).abs() < 1e-5,
                        "({x}, {y}): {actual} != {expected}"
                    );
                }
            }
        }
    }

    #[test]
    fn directional_green_uses_second_difference_not_bilinear() {
        // At the red center, the horizontal green candidate is
        // (0.2+0.4)/2 + (2*0.5-0.6-0.8)/4 = 0.2;
        // the vertical candidate is (0.7+0.9)/2 - 0.1 = 0.7.
        let mut raw = vec![0.5; 81];
        for (x, y, value) in [
            (3, 4, 0.2),
            (5, 4, 0.4),
            (2, 4, 0.6),
            (6, 4, 0.8),
            (4, 3, 0.7),
            (4, 5, 0.9),
            (4, 2, 0.6),
            (4, 6, 0.8),
        ] {
            raw[y * 9 + x] = value;
        }
        let (rgb, _) = demosaic_ahd(9, 9, &raw, *b"RGGB", &[false; 81]);
        assert_eq!(rgb[(4 * 9 + 4) * 3], 0.5);
        let green = rgb[(4 * 9 + 4) * 3 + 1];
        assert!(
            (green - 0.2).abs() < 1e-5 || (green - 0.7).abs() < 1e-5 || (green - 0.45).abs() < 1e-5,
            "unexpected green: {green}"
        );
    }

    #[test]
    fn raw_clips_remain_visible_at_edge_of_support() {
        let mut clipped = vec![false; 17 * 17];
        clipped[8 * 17 + 8] = true; // R photosite
        clipped[9 * 17 + 9] = true; // B photosite
        let (_, flags) = demosaic_ahd(17, 17, &vec![0.25; 289], *b"RGGB", &clipped);
        assert_eq!(flags[8 * 17 + 3], 1);
        assert_eq!(flags[8 * 17 + 14], 4);
        assert_eq!(flags[8 * 17 + 4], 5);
        assert_eq!(flags[0], 0);
    }

    #[test]
    fn edge_sizes_and_cfa_phases_are_deterministic() {
        for (width, height) in [(1, 1), (1, 2), (2, 1), (2, 2), (3, 9), (9, 3)] {
            for cfa in [*b"RGGB", *b"BGGR", *b"GRBG", *b"GBRG"] {
                let n = width * height;
                let raw = (0..n)
                    .map(|i| i as f32 / (n + 1) as f32)
                    .collect::<Vec<_>>();
                let masks = (0..n).map(|i| i % 3 == 0).collect::<Vec<_>>();
                let a = demosaic_ahd(width, height, &raw, cfa, &masks);
                let b = demosaic_ahd(width, height, &raw, cfa, &masks);
                assert_eq!(a, b);
                assert_eq!(a.0.len(), n * 3);
                assert_eq!(a.1.len(), n);
                assert!(a.0.iter().all(|v| v.is_finite()));
            }
        }
    }
}
