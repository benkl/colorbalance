//! Profile container: serialization, integrity digest, and transform apply.
//!
//! A profile is the portable unit of calibration: the fitted transform
//! stages, the decode contract and camera identity it is valid for, the
//! chart dataset it was fitted against, and the validation summary. The
//! JSON form is canonical (fixed field order, kebab-case) and sealed with
//! a sha256 digest over the canonical serialization, so tampering with
//! any recorded number is detectable on load.

use crate::calibration::FittedStages;
use crate::chart::ChartRevision;
use crate::contract::{ContractError, DecodeContract};
use crate::decode::CameraIdentity;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Schema version of the profile JSON this build reads and writes.
pub const SCHEMA_VERSION: &str = "1.0";

/// A measured color profile for one camera and decode contract.
///
/// Serialization order is the canonical field order: `digest` is written
/// last and is excluded from the digest computation itself (see
/// [`digest`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Profile {
    /// Schema version string, currently `"1.0"`.
    pub schema_version: String,
    /// Decode settings every image in this profile's scope must use.
    pub decode_contract: DecodeContract,
    /// Camera and decoder identity the profile was measured with.
    pub camera: CameraIdentity,
    /// Physical chart revision the reference values come from.
    pub chart_revision: ChartRevision,
    /// sha256 hex digest of the pinned chart dataset JSON.
    pub dataset_digest: String,
    /// sha256 hex digest of the reference image file, provided by the caller.
    pub reference_digest: String,
    /// Fitted transform stages (exposure, channel scale, color matrix).
    pub transform: FittedStages,
    /// Validation summary of the fit.
    pub validation: ValidationSummary,
    /// sha256 hex digest of the canonical JSON with this field empty.
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ValidationSummary {
    /// Mean ΔE2000 over all patches.
    pub mean_delta_e: f64,
    /// Maximum ΔE2000 over all patches.
    pub max_delta_e: f64,
    /// 95th percentile ΔE2000 (linear interpolation between order statistics).
    pub p95_delta_e: f64,
    /// Maximum ΔE2000 over the neutral patch row (White through Black).
    pub neutral_max_delta_e: f64,
    /// Maximum ΔE2000 over the two skin patches.
    pub skin_max_delta_e: f64,
    /// Condition number of the fitted matrix (infinity norm).
    pub condition_number: f64,
    /// Number of patches the summary was computed over.
    pub patch_count: u32,
}

/// Errors raised while loading a profile from JSON.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// The string is not a profile at all.
    #[error("invalid profile JSON: {0}")]
    Parse(#[from] serde_json::Error),
    /// The schema major version is not the one this build supports.
    #[error("unsupported schema version {0}, expected {1}")]
    SchemaVersion(String, String),
    /// The recomputed digest does not match the embedded one.
    #[error("profile digest mismatch")]
    DigestMismatch,
    /// The embedded decode contract is not a valid pinned contract.
    #[error("invalid decode contract: {0}")]
    Contract(#[from] ContractError),
}

/// Serialize a profile to canonical pretty JSON.
///
/// Fields are written in declaration order (kebab-case, two-space
/// indent), floats in serde's shortest round-trip form. The output is
/// deterministic for a given profile value.
pub fn to_json(p: &Profile) -> String {
    serde_json::to_string_pretty(p).expect("profile serialization cannot fail")
}

/// Compute the profile digest: lowercase sha256 hex over [`to_json`] of
/// the profile with its `digest` field set to the empty string.
pub fn digest(p: &Profile) -> String {
    let mut bare = p.clone();
    bare.digest = String::new();
    let mut hasher = Sha256::new();
    hasher.update(to_json(&bare).as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Parse and verify profile JSON.
///
/// Verification happens in this order and stops at the first failure:
/// the schema major version must match [`SCHEMA_VERSION`], the digest
/// recomputed over the loaded struct must equal the embedded `digest`,
/// and the decode contract must pass [`DecodeContract::validate`].
pub fn from_json(s: &str) -> Result<Profile, ProfileError> {
    let profile: Profile = serde_json::from_str(s)?;
    if schema_major(&profile.schema_version) != schema_major(SCHEMA_VERSION) {
        return Err(ProfileError::SchemaVersion(
            profile.schema_version,
            SCHEMA_VERSION.to_owned(),
        ));
    }
    if profile.digest != digest(&profile) {
        return Err(ProfileError::DigestMismatch);
    }
    profile.decode_contract.validate()?;
    Ok(profile)
}

/// Major version component of a `"major.minor"` version string.
fn schema_major(version: &str) -> &str {
    version.split('.').next().unwrap_or(version)
}

/// Apply the profile transform to one normalized linear camera RGB value.
///
/// Stages in order: divide by `exposure_scale`, divide channel `j` by
/// `channel_scale[j]`, then apply the color matrix in the row-vector
/// convention `out_j = Σ_r n_r · matrix[r][j]`, then clamp to `[0, 1]`.
///
/// The returned `u8` packs the clipping decisions made before clamping:
/// bits 0..2 are set when R, G, B respectively fell below 0, bits 3..5
/// when they exceeded 1.
pub fn apply_transform(p: &Profile, rgb: [f64; 3]) -> ([f64; 3], u8) {
    let n = [
        rgb[0] / (p.transform.exposure_scale * p.transform.channel_scale[0]),
        rgb[1] / (p.transform.exposure_scale * p.transform.channel_scale[1]),
        rgb[2] / (p.transform.exposure_scale * p.transform.channel_scale[2]),
    ];
    let mut out = [0.0f64; 3];
    for (j, o) in out.iter_mut().enumerate() {
        *o = n[0] * p.transform.matrix[0][j]
            + n[1] * p.transform.matrix[1][j]
            + n[2] * p.transform.matrix[2][j];
    }
    let mut flags = 0u8;
    for (i, v) in out.iter().enumerate() {
        if *v < 0.0 {
            flags |= 1 << i;
        }
        if *v > 1.0 {
            flags |= 1 << (i + 3);
        }
    }
    for v in out.iter_mut() {
        *v = v.clamp(0.0, 1.0);
    }
    (out, flags)
}

/// Quantize an already-clamped `[0, 1]` linear-RGB value to 16 bit.
///
/// The values are scaled by 65535 and rounded half away from zero; no
/// gamma curve is applied here (these are linear-light samples destined
/// for a TIFF sample buffer).
pub fn encode_srgb_u16(rgb: [f64; 3]) -> [u16; 3] {
    let mut out = [0u16; 3];
    for (i, v) in rgb.iter().enumerate() {
        let scaled = (v * 65535.0).round();
        out[i] = scaled.clamp(0.0, 65535.0) as u16;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::WhiteBalancePolicy;

    fn test_profile() -> Profile {
        Profile {
            schema_version: SCHEMA_VERSION.to_owned(),
            decode_contract: DecodeContract::canonical("libraw", "0.21.4"),
            camera: CameraIdentity {
                make: "ColorBalance Test Works".to_owned(),
                model: "CB-X1".to_owned(),
                decoder: "libraw".to_owned(),
                decoder_version: "0.21.4".to_owned(),
            },
            chart_revision: ChartRevision::ClassicFromNovember2014,
            dataset_digest: "1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f"
                .to_owned(),
            reference_digest:
                "2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e".to_owned(),
            transform: FittedStages {
                exposure_scale: 0.937,
                channel_scale: [1.031, 0.998, 1.045],
                matrix: [
                    [1.0123, -0.0045, 0.0011],
                    [-0.0021, 0.9987, 0.0009],
                    [0.0007, -0.0033, 1.0221],
                ],
            },
            validation: ValidationSummary {
                mean_delta_e: 0.4123,
                max_delta_e: 1.2345,
                p95_delta_e: 0.9876,
                neutral_max_delta_e: 0.6789,
                skin_max_delta_e: 0.8123,
                condition_number: 1.0432,
                patch_count: 24,
            },
            digest: String::new(),
        }
    }

    fn sealed_profile() -> Profile {
        let mut p = test_profile();
        p.digest = digest(&p);
        p
    }

    fn identity_profile() -> Profile {
        let mut p = test_profile();
        p.transform = FittedStages {
            exposure_scale: 1.0,
            channel_scale: [1.0, 1.0, 1.0],
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        };
        p
    }

    fn assert_f64_eq(a: f64, b: f64) {
        assert_eq!(a.to_bits(), b.to_bits(), "{a} and {b} differ");
    }

    #[test]
    fn round_trip_preserves_every_field_bit_exactly() {
        let p = sealed_profile();
        let json = to_json(&p);
        let q = from_json(&json).expect("sealed profile must load");

        assert_eq!(q.schema_version, p.schema_version);
        assert_eq!(q.decode_contract, p.decode_contract);
        assert_eq!(q.camera, p.camera);
        assert_eq!(q.chart_revision, p.chart_revision);
        assert_eq!(q.dataset_digest, p.dataset_digest);
        assert_eq!(q.reference_digest, p.reference_digest);
        assert_eq!(q.digest, p.digest);

        assert_f64_eq(q.transform.exposure_scale, p.transform.exposure_scale);
        for j in 0..3 {
            assert_f64_eq(q.transform.channel_scale[j], p.transform.channel_scale[j]);
        }
        for r in 0..3 {
            for c in 0..3 {
                assert_f64_eq(q.transform.matrix[r][c], p.transform.matrix[r][c]);
            }
        }

        assert_f64_eq(q.validation.mean_delta_e, p.validation.mean_delta_e);
        assert_f64_eq(q.validation.max_delta_e, p.validation.max_delta_e);
        assert_f64_eq(q.validation.p95_delta_e, p.validation.p95_delta_e);
        assert_f64_eq(
            q.validation.neutral_max_delta_e,
            p.validation.neutral_max_delta_e,
        );
        assert_f64_eq(q.validation.skin_max_delta_e, p.validation.skin_max_delta_e);
        assert_f64_eq(q.validation.condition_number, p.validation.condition_number);
        assert_eq!(q.validation.patch_count, p.validation.patch_count);

        // Canonical order is stable: the digest field is serialized last.
        let schema_pos = json.find("\"schema-version\"").expect("schema key");
        let transform_pos = json.find("\"transform\"").expect("transform key");
        let digest_pos = json.find("\"digest\"").expect("digest key");
        assert!(schema_pos < transform_pos);
        assert!(transform_pos < digest_pos);
        let tail = &json[digest_pos..];
        assert!(tail.ends_with('}'), "digest must be the last field");
        assert_eq!(to_json(&q), json);
    }

    #[test]
    fn tampered_float_fails_digest() {
        let p = sealed_profile();
        let base: serde_json::Value = serde_json::from_str(&to_json(&p)).unwrap();
        let tampered_fields = [
            ("transform", "exposure-scale", serde_json::json!(0.5)),
            (
                "transform",
                "channel-scale",
                serde_json::json!([1.0, 0.9, 1.0]),
            ),
            (
                "transform",
                "matrix",
                serde_json::json!([
                    [1.0123, -0.0045, 0.0011],
                    [-0.0021, 0.9987, 0.0009],
                    [0.0007, -0.0033, 1.0222]
                ]),
            ),
            ("validation", "mean-delta-e", serde_json::json!(0.4124)),
            ("validation", "condition-number", serde_json::json!(2.0)),
        ];
        for (section, field, value) in tampered_fields {
            let mut v = base.clone();
            v[section][field] = value;
            let s = serde_json::to_string(&v).unwrap();
            match from_json(&s) {
                Err(ProfileError::DigestMismatch) => {}
                other => panic!("tampering {section}.{field} gave {other:?}"),
            }
        }
    }

    #[test]
    fn wrong_major_version_rejected() {
        let p = sealed_profile();
        let mut v: serde_json::Value = serde_json::from_str(&to_json(&p)).unwrap();
        v["schema-version"] = serde_json::json!("2.0");
        let s = serde_json::to_string(&v).unwrap();
        match from_json(&s) {
            Err(ProfileError::SchemaVersion(version, expected)) => {
                assert_eq!(version, "2.0");
                assert_eq!(expected, SCHEMA_VERSION);
            }
            other => panic!("version tamper gave {other:?}"),
        }
        // Minor-version drift within the same major is not a rejection.
        v["schema-version"] = serde_json::json!("1.1");
        let s = serde_json::to_string(&v).unwrap();
        // The digest still guards content; here it also mismatches, but the
        // check that matters is that no SchemaVersion error is produced.
        assert!(!matches!(
            from_json(&s),
            Err(ProfileError::SchemaVersion(_, _))
        ));
    }

    #[test]
    fn invalid_decode_contract_rejected_after_digest() {
        let mut p = sealed_profile();
        p.decode_contract = DecodeContract {
            white_balance: WhiteBalancePolicy::CameraMetadata,
            ..DecodeContract::canonical("libraw", "0.21.4")
        };
        p.digest = digest(&p);
        match from_json(&to_json(&p)) {
            Err(ProfileError::Contract(_)) => {}
            other => panic!("invalid contract gave {other:?}"),
        }
    }

    #[test]
    fn unknown_field_rejected() {
        let p = sealed_profile();
        let mut v: serde_json::Value = serde_json::from_str(&to_json(&p)).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("bogus".to_owned(), serde_json::json!(1));
        let s = serde_json::to_string(&v).unwrap();
        assert!(matches!(from_json(&s), Err(ProfileError::Parse(_))));
        assert!(matches!(from_json("{\"oops"), Err(ProfileError::Parse(_))));
    }

    #[test]
    fn identity_profile_apply_is_identity() {
        let p = identity_profile();
        for rgb in [
            [0.0, 0.5, 1.0],
            [0.123456789, 0.987654321, 0.333333333],
            [0.0, 0.0, 0.0],
        ] {
            let (out, flags) = apply_transform(&p, rgb);
            for j in 0..3 {
                assert!(
                    (out[j] - rgb[j]).abs() < 1e-12,
                    "identity changed channel {j}: {rgb:?} -> {out:?}"
                );
            }
            assert_eq!(flags, 0, "identity must not clip");
        }
    }

    #[test]
    fn clip_bits_pack_low_r_and_high_g_exactly() {
        let mut p = identity_profile();
        p.transform.matrix = [[-0.1, 1.4, 0.0], [0.0, 0.0, 0.5], [0.0, 0.0, 0.5]];
        let (out, flags) = apply_transform(&p, [1.0, 1.0, 1.0]);
        // Pre-clamp values are R = -0.1, G = 1.4, B = 1.0.
        assert_eq!(
            flags, 0b0001_0001,
            "expected low-R (bit 0) and high-G (bit 4)"
        );
        assert_eq!(out, [0.0, 1.0, 1.0]);
    }

    #[test]
    fn stages_compose_inverse_exposure_channel_then_matrix() {
        let mut p = identity_profile();
        p.transform.exposure_scale = 2.0;
        p.transform.channel_scale = [1.0, 0.5, 4.0];
        p.transform.matrix[1][2] = 0.5;
        let (out, flags) = apply_transform(&p, [0.4, 0.2, 0.8]);
        // n = [0.2, 0.2, 0.1]; B = n1*0.5 + n2*1 = 0.1 + 0.1 = 0.2.
        let expected = [0.2, 0.2, 0.2];
        for (actual, expected) in out.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-15, "got {actual}");
        }
        assert_eq!(flags, 0);
    }

    #[test]
    fn encode_srgb_u16_boundaries() {
        assert_eq!(encode_srgb_u16([0.0, 1.0, 0.5]), [0, 65535, 32768]);
        assert_eq!(encode_srgb_u16([0.0001, 0.25, 0.75]), [7, 16384, 49151]);
        // 0.25 * 65535 = 16383.75 -> 16384; 0.75 * 65535 = 49151.25 -> 49151.
    }
}
