//! DNG Camera Profile (`.dcp`) export.
//!
//! A DCP is a small TIFF-structured file. This writer is clean-room: it follows
//! the public DNG 1.6 specification (the `ExtraCameraProfiles` and camera
//! profile tag descriptions) and nothing else. The header is a byte-order mark
//! (`II`), the 16-bit magic `0x4352` and the offset of IFD0, which is always 8.
//!
//! The profile is matrix only: `ColorMatrix1`, `ForwardMatrix1`, a single
//! calibration illuminant and `BaselineExposureOffset`. There is no tone
//! curve, `HueSatMap` or `LookTable`, so the fitted transform is the whole
//! rendering contribution of the profile. A DCP applies to RAW files only.
//!
//! # Mapping
//!
//! The fitted model is `n = rgb / (exposure * channel)`, `out = n * M` in
//! linear Rec.709. In column form the raw-to-XYZ-D50 map is
//!
//! ```text
//! T  = Bradford(D65 -> D50) * sRGB_to_XYZ * M^T * diag(1 / (exposure * channel))
//! w  = T^-1 * D50_white            raw vector the model maps to D50 white
//! N  = w / max(w)                  white balance at which the DCP is exact
//! FM = T * diag(w)                 ForwardMatrix1, camera (1,1,1) -> D50 white
//! CM = diag(N) * FM^-1             ColorMatrix1, XYZ D50 -> camera
//! EV = -log2(max(w))               BaselineExposureOffset
//! ```
//!
//! `FM * (1,1,1) = D50 white` holds by construction, so a reader that
//! renormalizes the forward matrix leaves it unchanged. At white balance `N`
//! the DCP reproduces the fitted model exactly. At any other white balance a
//! DCP reader re-derives the camera neutral from its own white balance, which
//! the fitted model has no equivalent of; that residual is measured in
//! `research/generate_dcp_vectors.py`.

use crate::color::{bradford_d65_to_d50, inverse, linear_srgb_to_xyz_matrix, D50_XYZ};
use crate::interchange::InterchangeError;
use crate::profile::Profile;

type Mat3 = [[f64; 3]; 3];

/// Denominator for every SRATIONAL value written. `i32::MAX / 1e6` bounds the
/// magnitude a matrix element or exposure offset may have.
const DENOMINATOR: i32 = 1_000_000;

const TYPE_BYTE: u16 = 1;
const TYPE_ASCII: u16 = 2;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_SRATIONAL: u16 = 10;

const TAG_DNG_VERSION: u16 = 50706;
const TAG_DNG_BACKWARD_VERSION: u16 = 50707;
const TAG_UNIQUE_CAMERA_MODEL: u16 = 50708;
const TAG_COLOR_MATRIX_1: u16 = 50721;
const TAG_CALIBRATION_ILLUMINANT_1: u16 = 50778;
const TAG_PROFILE_NAME: u16 = 50936;
const TAG_PROFILE_EMBED_POLICY: u16 = 50941;
const TAG_FORWARD_MATRIX_1: u16 = 50964;
const TAG_BASELINE_EXPOSURE_OFFSET: u16 = 51109;

/// `CalibrationIlluminant1` code for D50 (EXIF LightSource table).
const ILLUMINANT_D50: u16 = 23;
/// `ProfileEmbedPolicy` 0: "allow copying".
const EMBED_ALLOW_COPYING: u32 = 0;

fn mul(a: Mat3, b: Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

fn transpose(m: Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = m[j][i];
        }
    }
    out
}

fn diag(d: [f64; 3]) -> Mat3 {
    [[d[0], 0.0, 0.0], [0.0, d[1], 0.0], [0.0, 0.0, d[2]]]
}

fn column_apply(m: Mat3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn finite(m: Mat3) -> bool {
    m.iter().flatten().all(|v| v.is_finite())
}

/// DCP matrices derived from the fitted stages. Public for tests and tooling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DcpMatrices {
    /// `ColorMatrix1`, XYZ D50 to camera, row-major.
    pub color_matrix: Mat3,
    /// `ForwardMatrix1`, camera (white balanced) to XYZ D50, row-major.
    pub forward_matrix: Mat3,
    /// `BaselineExposureOffset` in EV.
    pub baseline_exposure_offset: f64,
}

/// Derives the DCP matrices from a profile's fitted stages.
pub fn dcp_matrices(p: &Profile) -> Result<DcpMatrices, InterchangeError> {
    let t = &p.transform;
    let scales = [
        t.exposure_scale * t.channel_scale[0],
        t.exposure_scale * t.channel_scale[1],
        t.exposure_scale * t.channel_scale[2],
    ];
    if scales.iter().any(|s| !s.is_finite() || *s <= 0.0) {
        return Err(InterchangeError::Refused(
            "exposure and channel scales must be finite and positive".into(),
        ));
    }
    let to_d50 = mul(bradford_d65_to_d50(), linear_srgb_to_xyz_matrix());
    let model = mul(to_d50, transpose(t.matrix));
    let t_raw = mul(
        model,
        diag([1.0 / scales[0], 1.0 / scales[1], 1.0 / scales[2]]),
    );
    if !finite(t_raw) {
        return Err(InterchangeError::Refused(
            "fitted matrix is not finite".into(),
        ));
    }
    let w = column_apply(inverse(t_raw), D50_XYZ);
    let w_max = w.iter().cloned().fold(f64::MIN, f64::max);
    if w.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(InterchangeError::Refused(
            "fitted matrix is singular or maps no positive camera neutral to D50 white".into(),
        ));
    }
    let n = [w[0] / w_max, w[1] / w_max, w[2] / w_max];
    let forward_matrix = mul(t_raw, diag(w));
    let color_matrix = mul(diag(n), inverse(forward_matrix));
    if !finite(color_matrix) {
        return Err(InterchangeError::Refused(
            "forward matrix is singular".into(),
        ));
    }
    Ok(DcpMatrices {
        color_matrix,
        forward_matrix,
        baseline_exposure_offset: -w_max.log2(),
    })
}

struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    data: Vec<u8>,
}

fn byte4(tag: u16, v: [u8; 4]) -> Entry {
    Entry {
        tag,
        kind: TYPE_BYTE,
        count: 4,
        data: v.to_vec(),
    }
}

fn ascii(tag: u16, s: &str) -> Entry {
    let mut data = s.as_bytes().to_vec();
    data.push(0);
    Entry {
        tag,
        kind: TYPE_ASCII,
        count: data.len() as u32,
        data,
    }
}

fn short(tag: u16, v: u16) -> Entry {
    Entry {
        tag,
        kind: TYPE_SHORT,
        count: 1,
        data: v.to_le_bytes().to_vec(),
    }
}

fn long(tag: u16, v: u32) -> Entry {
    Entry {
        tag,
        kind: TYPE_LONG,
        count: 1,
        data: v.to_le_bytes().to_vec(),
    }
}

fn srational(value: f64) -> Result<[u8; 8], InterchangeError> {
    let scaled = (value * f64::from(DENOMINATOR)).round();
    if !scaled.is_finite() || scaled.abs() > f64::from(i32::MAX) {
        return Err(InterchangeError::Refused(format!(
            "value {value} does not fit a signed rational with denominator {DENOMINATOR}"
        )));
    }
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&(scaled as i32).to_le_bytes());
    out[4..].copy_from_slice(&DENOMINATOR.to_le_bytes());
    Ok(out)
}

fn srationals(tag: u16, values: &[f64]) -> Result<Entry, InterchangeError> {
    let mut data = Vec::with_capacity(values.len() * 8);
    for v in values {
        data.extend_from_slice(&srational(*v)?);
    }
    Ok(Entry {
        tag,
        kind: TYPE_SRATIONAL,
        count: values.len() as u32,
        data,
    })
}

fn flat(m: Mat3) -> [f64; 9] {
    [
        m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2],
    ]
}

fn validate_camera_name(name: &str) -> Result<(), InterchangeError> {
    if name.trim().is_empty() {
        return Err(InterchangeError::Refused(
            "a camera name is required; it must match the name Lightroom shows for the camera"
                .into(),
        ));
    }
    if !name.is_ascii() || name.chars().any(char::is_control) {
        return Err(InterchangeError::Refused(
            "camera name must be printable ASCII".into(),
        ));
    }
    Ok(())
}

/// Serializes `p` as a little-endian DCP for `camera_name`.
///
/// `camera_name` becomes `UniqueCameraModel`. Readers match it against the
/// camera model of the RAW file, so it has no default.
///
/// Refuses quick-and-dirty profiles (measured from a rendered JPEG or PNG; a
/// DCP is applied to RAW sensor data, so the measurement does not transfer),
/// an empty or non-ASCII camera name, and degenerate fitted matrices.
pub fn profile_to_dcp(p: &Profile, camera_name: &str) -> Result<Vec<u8>, InterchangeError> {
    if p.quality.as_ref().is_some_and(|q| q.quick_and_dirty) {
        return Err(InterchangeError::Refused(
            "quick-and-dirty profiles are measured from rendered images and cannot become a RAW camera profile"
                .into(),
        ));
    }
    validate_camera_name(camera_name)?;
    let m = dcp_matrices(p)?;
    let digest = if p.digest.is_empty() {
        crate::profile::digest(p)
    } else {
        p.digest.clone()
    };
    let profile_name = format!("ColorBalance {}", &digest[..8.min(digest.len())]);

    let mut entries = vec![
        byte4(TAG_DNG_VERSION, [1, 6, 0, 0]),
        byte4(TAG_DNG_BACKWARD_VERSION, [1, 4, 0, 0]),
        ascii(TAG_UNIQUE_CAMERA_MODEL, camera_name.trim()),
        srationals(TAG_COLOR_MATRIX_1, &flat(m.color_matrix))?,
        short(TAG_CALIBRATION_ILLUMINANT_1, ILLUMINANT_D50),
        ascii(TAG_PROFILE_NAME, &profile_name),
        long(TAG_PROFILE_EMBED_POLICY, EMBED_ALLOW_COPYING),
        srationals(TAG_FORWARD_MATRIX_1, &flat(m.forward_matrix))?,
        srationals(TAG_BASELINE_EXPOSURE_OFFSET, &[m.baseline_exposure_offset])?,
    ];
    entries.sort_by_key(|e| e.tag);

    let ifd_len = 2 + entries.len() * 12 + 4;
    let mut out = Vec::new();
    out.extend_from_slice(b"II");
    out.extend_from_slice(&0x4352u16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());

    let mut extra: Vec<u8> = Vec::new();
    let extra_base = 8 + ifd_len;
    for e in &entries {
        out.extend_from_slice(&e.tag.to_le_bytes());
        out.extend_from_slice(&e.kind.to_le_bytes());
        out.extend_from_slice(&e.count.to_le_bytes());
        if e.data.len() <= 4 {
            let mut inline = [0u8; 4];
            inline[..e.data.len()].copy_from_slice(&e.data);
            out.extend_from_slice(&inline);
        } else {
            if (extra_base + extra.len()) % 2 == 1 {
                extra.push(0);
            }
            out.extend_from_slice(&((extra_base + extra.len()) as u32).to_le_bytes());
            extra.extend_from_slice(&e.data);
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&extra);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibration::FittedStages;
    use crate::chart::ChartRevision;
    use crate::contract::DecodeContract;
    use crate::decode::CameraIdentity;
    use crate::profile::{QualityProvenance, ValidationSummary};
    use serde_json::Value;

    fn profile(transform: FittedStages) -> Profile {
        let mut p = Profile {
            schema_version: "1.0".into(),
            decode_contract: DecodeContract::canonical("rawler-ahd", "0.8.0"),
            camera: CameraIdentity {
                make: "samsung".into(),
                model: "Galaxy S25".into(),
                decoder: "rawler-ahd".into(),
                decoder_version: "0.8.0".into(),
            },
            chart_revision: ChartRevision::ClassicFromNovember2014,
            dataset_digest: "a".repeat(64),
            reference_digest: "b".repeat(64),
            transform,
            validation: ValidationSummary {
                mean_delta_e: 1.0,
                max_delta_e: 2.0,
                p95_delta_e: 1.5,
                neutral_max_delta_e: 0.5,
                skin_max_delta_e: 0.6,
                condition_number: 2.0,
                patch_count: 24,
            },
            quality: None,
            digest: String::new(),
        };
        p.digest = crate::profile::digest(&p);
        p
    }

    fn vectors() -> Value {
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/reference/dcp-vectors.json"
        ))
        .unwrap()
    }

    fn case_profile(case: &Value) -> Profile {
        let arr = |v: &Value| -> Vec<f64> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect()
        };
        let m = case["matrix"].as_array().unwrap();
        let row = |i: usize| {
            let r = arr(&m[i]);
            [r[0], r[1], r[2]]
        };
        let cs = arr(&case["channel_scale"]);
        profile(FittedStages {
            exposure_scale: case["exposure_scale"].as_f64().unwrap(),
            channel_scale: [cs[0], cs[1], cs[2]],
            matrix: [row(0), row(1), row(2)],
        })
    }

    /// Minimal independent reader: header, one IFD, entry values by tag.
    struct Parsed {
        entries: Vec<(u16, u16, u32, Vec<u8>)>,
    }

    fn parse(bytes: &[u8]) -> Parsed {
        assert_eq!(&bytes[..4], b"II\x52\x43");
        let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let n = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
        let mut entries = Vec::new();
        for i in 0..n {
            let at = ifd + 2 + i * 12;
            let tag = u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
            let kind = u16::from_le_bytes(bytes[at + 2..at + 4].try_into().unwrap());
            let count = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap());
            let size = match kind {
                1 | 2 => 1,
                3 => 2,
                4 => 4,
                10 => 8,
                other => panic!("unexpected type {other}"),
            } * count as usize;
            let data = if size <= 4 {
                bytes[at + 8..at + 8 + size].to_vec()
            } else {
                let off = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
                assert_eq!(off % 2, 0, "out-of-line data must be word aligned");
                bytes[off..off + size].to_vec()
            };
            entries.push((tag, kind, count, data));
        }
        let next = u32::from_le_bytes(
            bytes[ifd + 2 + n * 12..ifd + 6 + n * 12]
                .try_into()
                .unwrap(),
        );
        assert_eq!(next, 0);
        Parsed { entries }
    }

    impl Parsed {
        fn get(&self, tag: u16) -> &(u16, u16, u32, Vec<u8>) {
            self.entries
                .iter()
                .find(|e| e.0 == tag)
                .expect("tag present")
        }
        fn rationals(&self, tag: u16) -> Vec<f64> {
            self.get(tag)
                .3
                .chunks(8)
                .map(|c| {
                    let num = i32::from_le_bytes(c[..4].try_into().unwrap());
                    let den = i32::from_le_bytes(c[4..].try_into().unwrap());
                    f64::from(num) / f64::from(den)
                })
                .collect()
        }
        fn text(&self, tag: u16) -> String {
            let d = &self.get(tag).3;
            assert_eq!(*d.last().unwrap(), 0, "ASCII must be NUL terminated");
            String::from_utf8(d[..d.len() - 1].to_vec()).unwrap()
        }
    }

    #[test]
    fn header_tag_order_and_scalars() {
        let v = vectors();
        let p = case_profile(&v["cases"][0]);
        let bytes = profile_to_dcp(&p, "samsung Galaxy S25").unwrap();
        let parsed = parse(&bytes);
        let tags: Vec<u16> = parsed.entries.iter().map(|e| e.0).collect();
        let mut sorted = tags.clone();
        sorted.sort_unstable();
        assert_eq!(tags, sorted, "IFD entries must be in ascending tag order");
        assert_eq!(parsed.get(TAG_DNG_VERSION).3, vec![1, 6, 0, 0]);
        assert_eq!(parsed.text(TAG_UNIQUE_CAMERA_MODEL), "samsung Galaxy S25");
        assert_eq!(
            parsed.get(TAG_CALIBRATION_ILLUMINANT_1).3,
            23u16.to_le_bytes()
        );
        assert_eq!(parsed.get(TAG_PROFILE_EMBED_POLICY).3, 0u32.to_le_bytes());
        assert!(parsed.text(TAG_PROFILE_NAME).starts_with("ColorBalance "));
        // No tone curve, hue/sat or look tables: matrix-only by design.
        for absent in [50940u16, 50938, 50982] {
            assert!(parsed.entries.iter().all(|e| e.0 != absent));
        }
    }

    #[test]
    fn matrices_match_independent_vectors() {
        let v = vectors();
        for case in v["cases"].as_array().unwrap() {
            let p = case_profile(case);
            let parsed = parse(&profile_to_dcp(&p, "cam").unwrap());
            let flat_of = |key: &str| -> Vec<f64> {
                case[key]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|r| r.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()))
                    .collect()
            };
            // Reference uses colour's 4-digit sRGB matrix; core uses the same
            // rounded matrix inverse plus a rounded Bradford, so allow 5e-4.
            for (key, tag) in [
                ("forward_matrix", TAG_FORWARD_MATRIX_1),
                ("color_matrix", TAG_COLOR_MATRIX_1),
            ] {
                let got = parsed.rationals(tag);
                let want = flat_of(key);
                assert_eq!(got.len(), 9);
                for (g, w) in got.iter().zip(&want) {
                    assert!(
                        (g - w).abs() <= 5e-4 * w.abs().max(1.0),
                        "{} {key}: got {g}, want {w}",
                        case["name"]
                    );
                }
            }
            let ev = parsed.rationals(TAG_BASELINE_EXPOSURE_OFFSET)[0];
            let want = case["baseline_exposure_offset"].as_f64().unwrap();
            assert!(
                (ev - want).abs() < 2e-3,
                "{} ev {ev} vs {want}",
                case["name"]
            );
        }
    }

    #[test]
    fn forward_matrix_maps_unit_camera_to_d50_and_color_matrix_inverts_it() {
        let v = vectors();
        for case in v["cases"].as_array().unwrap() {
            let m = dcp_matrices(&case_profile(case)).unwrap();
            let white = column_apply(m.forward_matrix, [1.0, 1.0, 1.0]);
            for (a, b) in white.iter().zip(D50_XYZ) {
                assert!((a - b).abs() < 1e-12);
            }
            let back = mul(m.color_matrix, m.forward_matrix);
            // CM * FM = diag(N): off-diagonals vanish, N has max 1.
            let mut max_n = 0.0f64;
            for (i, row) in back.iter().enumerate() {
                for (j, cell) in row.iter().enumerate() {
                    if i == j {
                        max_n = max_n.max(*cell);
                    } else {
                        assert!(cell.abs() < 1e-12);
                    }
                }
            }
            assert!((max_n - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn refuses_quick_and_dirty() {
        let v = vectors();
        let mut p = case_profile(&v["cases"][0]);
        p.quality = Some(QualityProvenance {
            passed: true,
            overridden: false,
            quick_and_dirty: true,
            failures: vec![],
        });
        assert!(matches!(
            profile_to_dcp(&p, "cam"),
            Err(InterchangeError::Refused(_))
        ));
    }

    #[test]
    fn refuses_bad_camera_names() {
        let v = vectors();
        let p = case_profile(&v["cases"][0]);
        for bad in ["", "   ", "Gälaxy", "a\nb", "a\0b"] {
            assert!(
                matches!(profile_to_dcp(&p, bad), Err(InterchangeError::Refused(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn refuses_singular_fit() {
        let p = profile(FittedStages {
            exposure_scale: 1.0,
            channel_scale: [1.0, 1.0, 1.0],
            matrix: [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        });
        assert!(matches!(
            profile_to_dcp(&p, "cam"),
            Err(InterchangeError::Refused(_))
        ));
        let p = profile(FittedStages {
            exposure_scale: 0.0,
            channel_scale: [1.0, 1.0, 1.0],
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        });
        assert!(matches!(
            profile_to_dcp(&p, "cam"),
            Err(InterchangeError::Refused(_))
        ));
    }

    #[test]
    fn near_singular_fit_is_refused_not_wrapped() {
        let p = profile(FittedStages {
            exposure_scale: 1.0,
            channel_scale: [1.0, 1.0, 1.0],
            matrix: [[1.0, 0.5, 0.2], [1.0, 0.5, 0.2000001], [0.3, 0.3, 1.0]],
        });
        match profile_to_dcp(&p, "cam") {
            Err(InterchangeError::Refused(_)) => {}
            Ok(bytes) => {
                // If it serializes, every rational must have survived intact.
                let parsed = parse(&bytes);
                let m = dcp_matrices(&p).unwrap();
                for (g, w) in parsed
                    .rationals(TAG_COLOR_MATRIX_1)
                    .iter()
                    .zip(flat(m.color_matrix))
                {
                    assert!((g - w).abs() < 1e-5, "wrapped value {g} vs {w}");
                }
            }
            Err(other) => panic!("unexpected error {other:?}"),
        }
    }
}
