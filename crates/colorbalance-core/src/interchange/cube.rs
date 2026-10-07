//! Adobe `.cube` 3D LUT export and accuracy evaluation.
//!
//! `.cube` is a common format supported by many image and video tools.
//! Because `.cube` does not carry camera metadata or decode contract settings,
//! it is intended only for hosts that can provide normalized linear camera RGB
//! under the profile's decode contract.

use crate::profile::{apply_transform, Profile};

/// Export a profile to an Adobe `.cube` 3D LUT string.
///
/// Output includes standard header keywords: `TITLE`, `DOMAIN_MIN 0.0 0.0 0.0`,
/// `DOMAIN_MAX 1.0 1.0 1.0`, and `LUT_3D_SIZE <size>`.
///
/// The LUT table is written in red-fastest order (red varies fastest, green
/// middle, blue slowest), containing `size^3` RGB triples clamped to `[0, 1]`.
pub fn profile_to_cube(p: &Profile, size: usize) -> String {
    let mut out = String::with_capacity(size * size * size * 24 + 128);
    out.push_str("TITLE \"ColorBalance\"\n");
    out.push_str("DOMAIN_MIN 0.0 0.0 0.0\n");
    out.push_str("DOMAIN_MAX 1.0 1.0 1.0\n");
    out.push_str(&format!("LUT_3D_SIZE {size}\n"));

    if size == 0 {
        return out;
    }

    let denom = if size > 1 { (size - 1) as f64 } else { 1.0 };

    for b_idx in 0..size {
        let b = (b_idx as f64) / denom;
        for g_idx in 0..size {
            let g = (g_idx as f64) / denom;
            for r_idx in 0..size {
                let r = (r_idx as f64) / denom;
                let (transformed, _) = apply_transform(p, [r, g, b]);
                out.push_str(&format!(
                    "{} {} {}\n",
                    transformed[0], transformed[1], transformed[2]
                ));
            }
        }
    }

    out
}

/// Compute maximum component error between trilinear LUT interpolation and direct transform.
///
/// Probes the 3D LUT at `probe_count` deterministic points using the Knuth 64-bit
/// LCG sequence starting at seed `0x5eed`. Each probe draws three coordinates
/// `[r, g, b]` in `[0, 1]`.
///
/// Returns the maximum absolute difference across all three color channels and all probes.
pub fn cube_max_error(p: &Profile, size: usize, probe_count: usize) -> f64 {
    if size < 2 || probe_count == 0 {
        return 0.0;
    }

    let denom = (size - 1) as f64;
    let mut lut = Vec::with_capacity(size * size * size);

    for b_idx in 0..size {
        let b = (b_idx as f64) / denom;
        for g_idx in 0..size {
            let g = (g_idx as f64) / denom;
            for r_idx in 0..size {
                let r = (r_idx as f64) / denom;
                let (transformed, _) = apply_transform(p, [r, g, b]);
                lut.push(transformed);
            }
        }
    }

    let mut state: u64 = 0x5eed;
    let mut max_err: f64 = 0.0;

    for _ in 0..probe_count {
        let r = next_lcg_unit(&mut state);
        let g = next_lcg_unit(&mut state);
        let b = next_lcg_unit(&mut state);
        let probe = [r, g, b];

        let r_pos = r * denom;
        let r0 = (r_pos.floor() as usize).min(size - 2);
        let r1 = r0 + 1;
        let tr = r_pos - r0 as f64;

        let g_pos = g * denom;
        let g0 = (g_pos.floor() as usize).min(size - 2);
        let g1 = g0 + 1;
        let tg = g_pos - g0 as f64;

        let b_pos = b * denom;
        let b0 = (b_pos.floor() as usize).min(size - 2);
        let b1 = b0 + 1;
        let tb = b_pos - b0 as f64;

        let c000 = lut[(b0 * size + g0) * size + r0];
        let c100 = lut[(b0 * size + g0) * size + r1];
        let c010 = lut[(b0 * size + g1) * size + r0];
        let c110 = lut[(b0 * size + g1) * size + r1];
        let c001 = lut[(b1 * size + g0) * size + r0];
        let c101 = lut[(b1 * size + g0) * size + r1];
        let c011 = lut[(b1 * size + g1) * size + r0];
        let c111 = lut[(b1 * size + g1) * size + r1];

        let (direct, _) = apply_transform(p, probe);

        for ch in 0..3 {
            let c00 = c000[ch] * (1.0 - tr) + c100[ch] * tr;
            let c10 = c010[ch] * (1.0 - tr) + c110[ch] * tr;
            let c01 = c001[ch] * (1.0 - tr) + c101[ch] * tr;
            let c11 = c011[ch] * (1.0 - tr) + c111[ch] * tr;

            let c0 = c00 * (1.0 - tg) + c10 * tg;
            let c1 = c01 * (1.0 - tg) + c11 * tg;

            let interp = c0 * (1.0 - tb) + c1 * tb;
            let err = (interp - direct[ch]).abs();
            if err > max_err {
                max_err = err;
            }
        }
    }

    max_err
}

/// Knuth 64-bit LCG step returning float in `[0, 1]`.
#[inline]
fn next_lcg_unit(state: &mut u64) -> f64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (*state as f64) / (u64::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibration::FittedStages;
    use crate::chart::ChartRevision;
    use crate::contract::DecodeContract;
    use crate::decode::CameraIdentity;
    use crate::profile::ValidationSummary;

    fn make_profile(transform: FittedStages) -> Profile {
        let dc = DecodeContract::canonical("libraw", "0.21.1");
        let camera = CameraIdentity {
            make: "Sony".into(),
            model: "A7IV".into(),
            decoder: "libraw".into(),
            decoder_version: "0.21.1".into(),
        };
        let validation = ValidationSummary {
            mean_delta_e: 0.5,
            max_delta_e: 1.0,
            p95_delta_e: 0.9,
            neutral_max_delta_e: 0.4,
            skin_max_delta_e: 0.5,
            condition_number: 1.1,
            patch_count: 24,
        };
        let mut p = Profile {
            schema_version: "1.0".into(),
            decode_contract: dc,
            camera,
            chart_revision: ChartRevision::ClassicFromNovember2014,
            dataset_digest: "abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234"
                .into(),
            reference_digest: "fedc5678fedc5678fedc5678fedc5678fedc5678fedc5678fedc5678fedc5678"
                .into(),
            transform,
            validation,
            quality: None,
            digest: String::new(),
        };
        p.digest = crate::profile::digest(&p);
        p
    }

    #[test]
    fn test_cube_format_and_ordering() {
        let p = make_profile(FittedStages {
            exposure_scale: 1.0,
            channel_scale: [1.0, 1.0, 1.0],
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        });

        let cube = profile_to_cube(&p, 3);
        let lines: Vec<&str> = cube.lines().collect();

        assert_eq!(lines[0], "TITLE \"ColorBalance\"");
        assert_eq!(lines[1], "DOMAIN_MIN 0.0 0.0 0.0");
        assert_eq!(lines[2], "DOMAIN_MAX 1.0 1.0 1.0");
        assert_eq!(lines[3], "LUT_3D_SIZE 3");

        // 4 header lines + 3^3 = 27 data lines = 31 total lines
        assert_eq!(lines.len(), 31);

        // Red fastest ordering: r=0, r=0.5, r=1.0 while g=0, b=0
        assert_eq!(lines[4], "0 0 0");
        assert_eq!(lines[5], "0.5 0 0");
        assert_eq!(lines[6], "1 0 0");
        // Next: g=0.5, r=0, r=0.5, r=1.0 while b=0
        assert_eq!(lines[7], "0 0.5 0");
    }

    #[test]
    fn test_identity_cube_error() {
        let p = make_profile(FittedStages {
            exposure_scale: 1.0,
            channel_scale: [1.0, 1.0, 1.0],
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        });

        let err = cube_max_error(&p, 33, 1000);
        assert!(
            err < 1e-12,
            "identity profile at size 33 error must be < 1e-12, got {err:e}"
        );
    }

    #[test]
    fn test_mild_matrix_cube_error_and_monotonicity() {
        // Mild matrix transform with positive coefficients that do not heavily clip
        let p = make_profile(FittedStages {
            exposure_scale: 1.0,
            channel_scale: [1.0, 1.0, 1.0],
            matrix: [[0.90, 0.05, 0.05], [0.05, 0.90, 0.05], [0.05, 0.05, 0.90]],
        });

        let err17 = cube_max_error(&p, 17, 1000);
        let err33 = cube_max_error(&p, 33, 1000);

        assert!(
            err33 < 5e-3,
            "mild matrix at size 33 error must be < 5e-3, got {err33:e}"
        );
        assert!(
            err33 <= err17 * 1.10,
            "size 33 error ({err33:e}) must not exceed 10% above size 17 error ({err17:e})"
        );
    }
}
