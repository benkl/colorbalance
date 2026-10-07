//! The canonical RAW decode contract.
//!
//! Every image in a profile's scope is decoded with exactly these settings.
//! The contract travels inside a profile, and any deviation is rejected
//! before images are processed. This is invariant 1 in `AGENTS.md`.

use serde::{Deserialize, Serialize};

/// Error raised when decoded settings deviate from the pinned contract.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ContractError {
    #[error("output color must be raw, got {0:?}")]
    OutputColor(OutputColor),
    #[error("output depth must be 16 bit, got {0:?}")]
    OutputDepth(OutputDepth),
    #[error("white balance must be unity user multipliers, got {0:?}")]
    WhiteBalance(WhiteBalancePolicy),
    #[error("auto brightening must be disabled")]
    AutoBright,
    #[error("demosaic algorithm is not pinned, got {0:?}")]
    Demosaic(DemosaicAlgorithm),
    #[error("highlight policy is not pinned, got {0:?}")]
    Highlight(HighlightPolicy),
    #[error("orientation policy is not pinned, got {0:?}")]
    Orientation(OrientationPolicy),
}

/// Output color selection. Only `Raw` is valid in the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputColor {
    /// No color conversion, camera raw colorimetry preserved.
    Raw,
}

/// Output bit depth. Only `U16` is valid in the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputDepth {
    U8,
    U16,
}

/// How the decoder is allowed to set white balance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", rename_all_fields = "kebab-case")]
pub enum WhiteBalancePolicy {
    /// Fixed user multipliers, applied identically to every image.
    Unity {
        user_mul: [f32; 4],
    },
    CameraMetadata,
    Auto,
}

/// Pinned demosaic algorithm. Only `Ahd` is valid in the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DemosaicAlgorithm {
    Linear,
    Vng,
    Ppg,
    Ahd,
    Dcb,
}

/// Pinned highlight handling. Only `Clip` is valid in the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HighlightPolicy {
    Clip,
    Unclip,
    Blend,
}

/// How output orientation is derived. Only `AsShot` is valid in the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrientationPolicy {
    /// Keep the orientation recorded in the RAW file.
    AsShot,
    Fixed {
        flip: u16,
    },
}

/// The complete set of decoder settings a profile is valid for.
///
/// Construct it with [`DecodeContract::canonical`]. Deserialized contracts are
/// accepted only if they pass [`DecodeContract::validate`], so profiles from
/// other tools cannot silently relax the decode settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DecodeContract {
    /// Decoder implementation identity, for example `rawler-ahd`.
    pub decoder: String,
    /// Decoder implementation version the contract was pinned against.
    pub decoder_version: String,
    pub output_color: OutputColor,
    pub output_depth: OutputDepth,
    pub gamma: [f32; 2],
    pub white_balance: WhiteBalancePolicy,
    pub no_auto_bright: bool,
    pub demosaic: DemosaicAlgorithm,
    pub highlight: HighlightPolicy,
    pub orientation: OrientationPolicy,
    pub no_auto_scale: bool,
}

/// Apply-time mismatch between a saved profile and the decoder currently in use.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractMismatch {
    #[error("decoder mismatch (profile: {profile}, image: {image})")]
    Decoder { profile: String, image: String },
    #[error("decode settings differ from the profile's pinned contract")]
    Settings,
}

impl DecodeContract {
    /// The one decode contract this tool uses for every image.
    pub fn canonical(decoder: &str, decoder_version: &str) -> Self {
        Self {
            decoder: decoder.to_owned(),
            decoder_version: decoder_version.to_owned(),
            output_color: OutputColor::Raw,
            output_depth: OutputDepth::U16,
            gamma: [1.0, 1.0],
            white_balance: WhiteBalancePolicy::Unity {
                user_mul: [1.0, 1.0, 1.0, 1.0],
            },
            no_auto_bright: true,
            demosaic: DemosaicAlgorithm::Ahd,
            highlight: HighlightPolicy::Clip,
            orientation: OrientationPolicy::AsShot,
            no_auto_scale: false,
        }
    }

    /// Check the actual decoder contract against the profile contract. A version
    /// change alone is allowed, but must be reported to the caller.
    pub fn compare_for_apply(&self, actual: &Self) -> Result<Option<String>, ContractMismatch> {
        if self.decoder != actual.decoder {
            return Err(ContractMismatch::Decoder {
                profile: self.decoder.clone(),
                image: actual.decoder.clone(),
            });
        }
        if self.output_color != actual.output_color
            || self.output_depth != actual.output_depth
            || self.gamma != actual.gamma
            || self.white_balance != actual.white_balance
            || self.no_auto_bright != actual.no_auto_bright
            || self.demosaic != actual.demosaic
            || self.highlight != actual.highlight
            || self.orientation != actual.orientation
            || self.no_auto_scale != actual.no_auto_scale
        {
            return Err(ContractMismatch::Settings);
        }
        Ok((self.decoder_version != actual.decoder_version).then(|| {
            format!(
                "decoder version differs (profile: {} {}, image: {} {})",
                self.decoder, self.decoder_version, actual.decoder, actual.decoder_version
            )
        }))
    }

    /// Reject any deviation from the pinned settings.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.output_color != OutputColor::Raw {
            return Err(ContractError::OutputColor(self.output_color));
        }
        if self.output_depth != OutputDepth::U16 {
            return Err(ContractError::OutputDepth(self.output_depth));
        }
        match self.white_balance {
            WhiteBalancePolicy::Unity { user_mul } => {
                if user_mul != [1.0, 1.0, 1.0, 1.0] {
                    return Err(ContractError::WhiteBalance(self.white_balance));
                }
            }
            policy => return Err(ContractError::WhiteBalance(policy)),
        }
        if self.gamma != [1.0, 1.0] {
            return Err(ContractError::AutoBright);
        }
        if !self.no_auto_bright {
            return Err(ContractError::AutoBright);
        }
        if self.demosaic != DemosaicAlgorithm::Ahd {
            return Err(ContractError::Demosaic(self.demosaic));
        }
        if self.highlight != HighlightPolicy::Clip {
            return Err(ContractError::Highlight(self.highlight));
        }
        if self.orientation != OrientationPolicy::AsShot {
            return Err(ContractError::Orientation(self.orientation));
        }
        if self.no_auto_scale {
            return Err(ContractError::AutoBright);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_contract_passes_validation() {
        let contract = DecodeContract::canonical("rawler-ahd", "0.8.0");
        assert!(contract.validate().is_ok());
    }

    #[test]
    fn canonical_contract_round_trips_through_json() {
        let contract = DecodeContract::canonical("rawler-ahd", "0.8.0");
        let json = serde_json::to_string(&contract).expect("serialize");
        let parsed: DecodeContract = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, contract);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn camera_white_balance_is_rejected() {
        let mut contract = DecodeContract::canonical("rawler-ahd", "0.8.0");
        contract.white_balance = WhiteBalancePolicy::CameraMetadata;
        assert_eq!(
            contract.validate(),
            Err(ContractError::WhiteBalance(
                WhiteBalancePolicy::CameraMetadata
            ))
        );
    }

    #[test]
    fn non_unity_multipliers_are_rejected() {
        let mut contract = DecodeContract::canonical("rawler-ahd", "0.8.0");
        contract.white_balance = WhiteBalancePolicy::Unity {
            user_mul: [1.0, 1.0, 1.0, 1.2],
        };
        assert!(contract.validate().is_err());
    }

    #[test]
    fn srgb_output_and_auto_bright_are_rejected() {
        let mut contract = DecodeContract::canonical("rawler-ahd", "0.8.0");
        contract.no_auto_bright = false;
        assert_eq!(contract.validate(), Err(ContractError::AutoBright));

        let mut contract = DecodeContract::canonical("rawler-ahd", "0.8.0");
        contract.gamma = [1.0 / 2.4, 12.92];
        assert_eq!(contract.validate(), Err(ContractError::AutoBright));
    }

    #[test]
    fn apply_rejects_decoder_name_and_settings_but_reports_version_drift() {
        let saved = DecodeContract::canonical("rawler-ahd", "0.7.9");
        let actual = DecodeContract::canonical("rawler-ahd", "0.8.0");
        assert!(saved
            .compare_for_apply(&actual)
            .unwrap()
            .unwrap()
            .contains("0.7.9"));
        assert_eq!(actual.compare_for_apply(&actual), Ok(None));

        let other = DecodeContract::canonical("libraw", "0.8.0");
        assert!(matches!(
            saved.compare_for_apply(&other),
            Err(ContractMismatch::Decoder { .. })
        ));
        let mut other = actual.clone();
        other.no_auto_scale = true;
        assert_eq!(
            saved.compare_for_apply(&other),
            Err(ContractMismatch::Settings)
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = r#"{
            "decoder": "rawler-ahd",
            "decoder-version": "0.8.0",
            "output-color": "raw",
            "output-depth": "u16",
            "gamma": [1.0, 1.0],
            "white-balance": { "unity": { "user-mul": [1.0, 1.0, 1.0, 1.0] } },
            "no-auto-bright": true,
            "demosaic": "ahd",
            "highlight": "clip",
            "orientation": "as-shot",
            "no-auto-scale": false,
            "mystery-knob": true
        }"#;
        let result: Result<DecodeContract, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
