//! Output color spaces for the 16-bit TIFF export.
//!
//! The fitted profile produces linear Rec.709 (sRGB primaries). This module
//! converts that result into the encoded space the user asks for and supplies
//! the matching ICC profile for the TIFF. No color math lives here: the
//! conversion is an OCIO `ColorSpaceTransform` between two spaces of a pinned
//! built-in config, and the ICC bytes come from `moxcms`. See
//! `docs/ARCHITECTURE.md` D24.
//!
//! Order of operations for a wide target: unclamped linear Rec.709 from the
//! profile, then OCIO to the encoded target, then clamp to `[0, 1]` in the
//! target. Clamping to sRGB first would discard exactly the colors a wide
//! gamut can keep.
//!
//! `sRGB` is the default and stays on [`crate::profile::correct_to_u16`], so
//! its samples are unchanged by this module.

use std::fmt;
use std::str::FromStr;

use moxcms::ColorProfile;
use ocio::config::Config;
use ocio::processor::CpuProcessor;

use crate::profile::{self, Profile};

/// Built-in OCIO config all conversions are resolved in. Pinned by name so a
/// dependency update cannot change which transforms an output space means.
pub const OCIO_CONFIG: &str = "cg-config-v4.0.0_aces-v2.0_ocio-v2.5";

/// OCIO color space holding the profile's output: linear light, Rec.709
/// primaries, D65 white.
pub const SOURCE_SPACE: &str = "Linear Rec.709 (sRGB)";

/// Version of the `ocio` crate that evaluates the conversions.
pub const OCIO_VERSION: &str = ocio::VERSION;

/// Pixels per parallel work item.
const CHUNK_PIXELS: usize = 8192;

/// Encoded output space of a TIFF export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OutputSpace {
    /// sRGB: Rec.709 primaries, IEC 61966-2-1 curve. The default.
    #[default]
    Srgb,
    /// Display P3: P3 primaries, D65 white, sRGB curve.
    DisplayP3,
    /// Adobe RGB (1998): its own primaries, D65 white, gamma 2.19921875.
    AdobeRgb,
}

impl OutputSpace {
    /// Every supported space, default first.
    pub const ALL: [OutputSpace; 3] = [
        OutputSpace::Srgb,
        OutputSpace::DisplayP3,
        OutputSpace::AdobeRgb,
    ];

    /// Stable kebab-case name used on the command line and in reports.
    pub fn as_str(self) -> &'static str {
        match self {
            OutputSpace::Srgb => "srgb",
            OutputSpace::DisplayP3 => "display-p3",
            OutputSpace::AdobeRgb => "adobe-rgb",
        }
    }

    /// Human-readable name for UI and reports.
    pub fn label(self) -> &'static str {
        match self {
            OutputSpace::Srgb => "sRGB",
            OutputSpace::DisplayP3 => "Display P3",
            OutputSpace::AdobeRgb => "Adobe RGB (1998)",
        }
    }

    /// Name of the OCIO color space this output is encoded in. `None` for
    /// sRGB, which is produced by the profile stage itself.
    pub fn ocio_destination(self) -> Option<&'static str> {
        match self {
            OutputSpace::Srgb => None,
            OutputSpace::DisplayP3 => Some("sRGB Encoded P3-D65"),
            OutputSpace::AdobeRgb => Some("Gamma 2.2 Encoded AdobeRGB"),
        }
    }
}

impl fmt::Display for OutputSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OutputSpace {
    type Err = OutputSpaceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        OutputSpace::ALL
            .into_iter()
            .find(|space| space.as_str() == s)
            .ok_or_else(|| OutputSpaceError::Unknown(s.to_string()))
    }
}

/// Failure to resolve an output space. Export stops rather than writing
/// pixels in a space the file does not declare.
#[derive(Debug, thiserror::Error)]
pub enum OutputSpaceError {
    #[error("unknown output space {0:?}; expected one of: srgb, display-p3, adobe-rgb")]
    Unknown(String),
    #[error("OCIO conversion to {space} is unavailable: {message}")]
    Ocio { space: OutputSpace, message: String },
}

/// Converts profile output to one [`OutputSpace`]. Build once per batch and
/// share across worker threads.
#[derive(Clone)]
pub struct OutputConverter {
    space: OutputSpace,
    /// `None` for sRGB.
    processor: Option<CpuProcessor>,
}

impl fmt::Debug for OutputConverter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutputConverter")
            .field("space", &self.space)
            .finish_non_exhaustive()
    }
}

impl OutputConverter {
    /// Resolve `space` in the pinned OCIO config.
    pub fn new(space: OutputSpace) -> Result<Self, OutputSpaceError> {
        let fail = |message: String| OutputSpaceError::Ocio { space, message };
        let processor = match space.ocio_destination() {
            None => None,
            Some(destination) => {
                let config = Config::create_from_builtin_config(OCIO_CONFIG)
                    .map_err(|e| fail(e.to_string()))?;
                let processor = config
                    .get_processor(SOURCE_SPACE, destination)
                    .map_err(|e| fail(e.to_string()))?;
                Some(processor.default_cpu_processor())
            }
        };
        Ok(Self { space, processor })
    }

    pub fn space(&self) -> OutputSpace {
        self.space
    }

    /// ICC profile describing the encoded samples, for TIFF tag 34675.
    ///
    /// `moxcms` stamps the current time into the header; that field is
    /// replaced with the Unix epoch so identical inputs give identical files.
    pub fn icc_profile(&self) -> Result<Vec<u8>, OutputSpaceError> {
        let profile = match self.space {
            OutputSpace::Srgb => ColorProfile::new_srgb(),
            OutputSpace::DisplayP3 => ColorProfile::new_display_p3(),
            OutputSpace::AdobeRgb => ColorProfile::new_adobe_rgb(),
        };
        let mut bytes = profile.encode().map_err(|e| OutputSpaceError::Ocio {
            space: self.space,
            message: format!("ICC profile encoding failed: {e:?}"),
        })?;
        // ICC header bytes 24..36: creation date and time (six big-endian u16).
        let epoch: [u16; 6] = [1970, 1, 1, 0, 0, 0];
        for (slot, value) in bytes[24..36].as_chunks_mut::<2>().0.iter_mut().zip(epoch) {
            *slot = value.to_be_bytes();
        }
        Ok(bytes)
    }

    /// Convert one unclamped linear Rec.709 value to the encoded target,
    /// without clamping. For sRGB the encoding is the IEC curve applied by
    /// the profile stage and is not reproduced here.
    ///
    /// # Panics
    ///
    /// Panics for [`OutputSpace::Srgb`], which has no OCIO processor.
    pub fn convert_linear_rec709(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut out = rgb;
        self.processor
            .as_ref()
            .expect("sRGB output is produced by the profile stage")
            .apply_rgb(&mut out);
        out
    }

    /// Apply the profile to every pixel of an interleaved linear camera RGB
    /// buffer and return the encoded 16-bit samples plus the number of pixels
    /// that did not fit the target gamut.
    ///
    /// On return `rgb` holds clamped linear Rec.709, as with
    /// [`profile::correct_to_u16`], so previews can keep rendering sRGB from
    /// it. A pixel counts as clipped when quantizing it moved a channel, that
    /// is when the unclamped value lies more than half a 16-bit step outside
    /// `[0, 1]` in the target space (for sRGB: when the profile output left
    /// `[0, 1]`).
    ///
    /// # Panics
    ///
    /// Panics if `rgb.len()` is not a multiple of three.
    pub fn correct_to_u16(&self, p: &Profile, rgb: &mut [f32]) -> (Vec<u16>, usize) {
        let Some(processor) = &self.processor else {
            return profile::correct_to_u16(p, rgb);
        };
        assert!(
            rgb.len().is_multiple_of(3),
            "RGB buffer length must be a multiple of 3"
        );
        let mut encoded = vec![0_u16; rgb.len()];
        let clipped = crate::par::zip_chunks_mut_sum(
            rgb,
            CHUNK_PIXELS * 3,
            &mut encoded,
            CHUNK_PIXELS * 3,
            |_, input, output| {
                let mut target = vec![0.0_f32; input.len()];
                for (pixel, wide) in input
                    .as_chunks_mut::<3>()
                    .0
                    .iter_mut()
                    .zip(target.as_chunks_mut::<3>().0.iter_mut())
                {
                    let linear = profile::linear_rec709(
                        p,
                        [
                            f64::from(pixel[0]),
                            f64::from(pixel[1]),
                            f64::from(pixel[2]),
                        ],
                    );
                    for ((slot, kept), value) in pixel.iter_mut().zip(wide.iter_mut()).zip(linear) {
                        *kept = value as f32;
                        *slot = value.clamp(0.0, 1.0) as f32;
                    }
                }
                processor.apply_rgb_slice(&mut target);
                let mut clipped = 0;
                for (value, sample) in target.iter().zip(output.iter_mut()) {
                    *sample = quantize(*value);
                }
                for pixel in target.as_chunks::<3>().0 {
                    if pixel.iter().any(|v| !fits_u16(*v)) {
                        clipped += 1;
                    }
                }
                clipped
            },
        );
        (encoded, clipped)
    }
}

/// Round to 16 bit, clamping to the code range.
fn quantize(value: f32) -> u16 {
    (f64::from(value) * 65535.0).round().clamp(0.0, 65535.0) as u16
}

/// Whether rounding leaves `value` unchanged by the code-range clamp.
fn fits_u16(value: f32) -> bool {
    let scaled = f64::from(value) * 65535.0;
    (-0.5..=65535.5).contains(&scaled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_unknown_fails() {
        for space in OutputSpace::ALL {
            assert_eq!(space.as_str().parse::<OutputSpace>().unwrap(), space);
        }
        assert!(matches!(
            "prophoto".parse::<OutputSpace>(),
            Err(OutputSpaceError::Unknown(name)) if name == "prophoto"
        ));
    }

    #[test]
    fn every_destination_resolves_in_the_pinned_config() {
        for space in OutputSpace::ALL {
            OutputConverter::new(space).unwrap_or_else(|e| panic!("{space}: {e}"));
        }
    }

    #[derive(serde::Deserialize)]
    struct Vectors {
        probes: Vec<[f32; 3]>,
        spaces: std::collections::BTreeMap<String, SpaceVectors>,
    }

    #[derive(serde::Deserialize)]
    struct SpaceVectors {
        encoded: Vec<[f64; 3]>,
        #[serde(rename = "linear-unclamped")]
        linear_unclamped: Vec<[f64; 3]>,
    }

    fn vectors() -> Vectors {
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/reference/output-space-vectors.json"
        ))
        .expect("reference fixture is valid JSON")
    }

    fn identity_profile() -> Profile {
        use crate::calibration::FittedStages;
        use crate::chart::ChartRevision;
        use crate::contract::DecodeContract;
        use crate::decode::CameraIdentity;
        use crate::profile::{ValidationSummary, SCHEMA_VERSION};
        Profile {
            schema_version: SCHEMA_VERSION.to_owned(),
            decode_contract: DecodeContract::canonical("libraw", "0.21.4"),
            camera: CameraIdentity {
                make: "Test".to_owned(),
                model: "T1".to_owned(),
                decoder: "libraw".to_owned(),
                decoder_version: "0.21.4".to_owned(),
            },
            chart_revision: ChartRevision::ClassicFromNovember2014,
            dataset_digest: String::new(),
            reference_digest: String::new(),
            transform: FittedStages {
                exposure_scale: 1.0,
                channel_scale: [1.0; 3],
                matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            },
            validation: ValidationSummary {
                mean_delta_e: 0.0,
                max_delta_e: 0.0,
                p95_delta_e: 0.0,
                neutral_max_delta_e: 0.0,
                skin_max_delta_e: 0.0,
                condition_number: 1.0,
                patch_count: 24,
            },
            quality: None,
            digest: String::new(),
        }
    }

    /// OCIO routes through ACES2065-1 and adapts white there; the reference
    /// converts by matrix. Largest measured difference over the probes is
    /// 2.5e-4 (Display P3 red), so the bound is twice that.
    const REFERENCE_TOLERANCE: f64 = 5e-4;

    /// Near the gamut boundary the reference is the less exact side: Adobe RGB
    /// shares its red and blue primaries with Rec.709, so pure Rec.709 blue has
    /// an Adobe R of exactly 0, but colour's rounded chromaticities give 4.7e-5,
    /// which a 1/2.2 power curve lifts to 0.011. Where the reference linear
    /// value is below `NEAR_BLACK_LINEAR`, compare encoded values to within
    /// the measured worst case (1.1e-2 for Adobe RGB blue).
    const NEAR_BLACK_LINEAR: f64 = 1e-4;
    const NEAR_BLACK_TOLERANCE: f64 = 2e-2;

    #[test]
    fn conversion_matches_independent_reference() {
        let vectors = vectors();
        for (key, reference) in &vectors.spaces {
            let space: OutputSpace = key.parse().unwrap();
            let converter = OutputConverter::new(space).unwrap();
            assert_eq!(reference.encoded.len(), vectors.probes.len());
            for ((probe, expected), linear) in vectors
                .probes
                .iter()
                .zip(&reference.encoded)
                .zip(&reference.linear_unclamped)
            {
                let got = converter.convert_linear_rec709(*probe);
                for channel in 0..3 {
                    let got = f64::from(got[channel]).clamp(0.0, 1.0);
                    let tolerance = if linear[channel].abs() < NEAR_BLACK_LINEAR {
                        NEAR_BLACK_TOLERANCE
                    } else {
                        REFERENCE_TOLERANCE
                    };
                    assert!(
                        (got - expected[channel]).abs() <= tolerance,
                        "{key} probe {probe:?} channel {channel}: got {got}, expected {}",
                        expected[channel]
                    );
                }
            }
        }
    }

    #[test]
    fn wide_target_keeps_colors_srgb_would_clip() {
        // Linear Rec.709 (-0.1, 0.6, 0.2) is outside sRGB but inside P3 and
        // Adobe RGB. The buffer is returned clamped for the preview, while the
        // exported samples carry the out-of-sRGB color.
        let profile = identity_profile();
        for space in [OutputSpace::DisplayP3, OutputSpace::AdobeRgb] {
            let converter = OutputConverter::new(space).unwrap();
            let mut rgb = [-0.1_f32, 0.6, 0.2];
            let (samples, clipped) = converter.correct_to_u16(&profile, &mut rgb);
            assert_eq!(clipped, 0, "{space}");
            assert_eq!(rgb, [0.0, 0.6, 0.2], "{space}: preview buffer is clamped");

            let mut clamped_first = [0.0_f32, 0.6, 0.2];
            let (from_clamped, _) = converter.correct_to_u16(&profile, &mut clamped_first);
            assert_ne!(
                samples, from_clamped,
                "{space}: clamping first loses the color"
            );
        }
        let mut rgb = [-0.1_f32, 0.6, 0.2];
        let (_, clipped) = profile::correct_to_u16(&profile, &mut rgb);
        assert_eq!(clipped, 1, "sRGB path clips the same color");
    }

    #[test]
    fn pixels_outside_the_target_are_counted_and_saturate() {
        let profile = identity_profile();
        let converter = OutputConverter::new(OutputSpace::DisplayP3).unwrap();
        let mut rgb = [0.5_f32, 0.5, 0.5, 1.5, 1.5, 1.5, 0.0, 0.0, 0.0];
        let (samples, clipped) = converter.correct_to_u16(&profile, &mut rgb);
        assert_eq!(clipped, 1);
        assert_eq!(&samples[3..6], &[65535; 3]);
        assert_eq!(&samples[6..9], &[0; 3]);
    }

    #[test]
    fn neutral_axis_stays_neutral() {
        let profile = identity_profile();
        for space in [OutputSpace::DisplayP3, OutputSpace::AdobeRgb] {
            let converter = OutputConverter::new(space).unwrap();
            let mut rgb: Vec<f32> = (0..=100)
                .flat_map(|i| {
                    let v = i as f32 / 100.0;
                    [v, v, v]
                })
                .collect();
            let (samples, clipped) = converter.correct_to_u16(&profile, &mut rgb);
            assert_eq!(clipped, 0);
            for pixel in samples.as_chunks::<3>().0 {
                let spread = pixel.iter().max().unwrap() - pixel.iter().min().unwrap();
                assert!(spread <= 2, "{space}: neutral drifted by {spread} codes");
            }
        }
    }

    #[test]
    fn parallel_chunks_match_a_single_pixel_run() {
        let profile = identity_profile();
        let converter = OutputConverter::new(OutputSpace::AdobeRgb).unwrap();
        let pixels = CHUNK_PIXELS * 2 + 17;
        let rgb: Vec<f32> = (0..pixels * 3)
            .map(|i| ((i * 2654435761usize) % 1000) as f32 / 800.0 - 0.1)
            .collect();
        let (whole, whole_clipped) = converter.correct_to_u16(&profile, &mut rgb.clone());
        let mut one_by_one = Vec::new();
        let mut one_clipped = 0;
        for pixel in rgb.as_chunks::<3>().0 {
            let (samples, clipped) = converter.correct_to_u16(&profile, &mut pixel.to_vec());
            one_by_one.extend(samples);
            one_clipped += clipped;
        }
        assert_eq!(whole, one_by_one);
        assert_eq!(whole_clipped, one_clipped);
    }

    #[test]
    fn icc_profiles_are_deterministic_and_describe_each_space() {
        for space in OutputSpace::ALL {
            let converter = OutputConverter::new(space).unwrap();
            let bytes = converter.icc_profile().unwrap();
            assert_eq!(bytes, converter.icc_profile().unwrap());
            assert_eq!(&bytes[36..40], b"acsp");
            assert_eq!(
                u16::from_be_bytes([bytes[24], bytes[25]]),
                1970,
                "{space}: creation date is fixed"
            );
            ColorProfile::new_from_slice(&bytes)
                .unwrap_or_else(|e| panic!("{space}: profile does not parse: {e:?}"));
        }
        let srgb = OutputConverter::new(OutputSpace::Srgb)
            .unwrap()
            .icc_profile()
            .unwrap();
        let p3 = OutputConverter::new(OutputSpace::DisplayP3)
            .unwrap()
            .icc_profile()
            .unwrap();
        assert_ne!(srgb, p3);
    }

    #[test]
    fn quantize_clamps_and_rounds() {
        assert_eq!(quantize(-0.3), 0);
        assert_eq!(quantize(1.4), 65535);
        assert_eq!(quantize(0.5), 32768);
        assert!(fits_u16(1.0));
        assert!(fits_u16(-0.000_007));
        assert!(!fits_u16(-0.000_008));
        assert!(!fits_u16(1.000_008));
    }
}
