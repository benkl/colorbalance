//! ColorBalance LibRaw decode layer.
//!
//! This crate owns everything LibRaw-specific. The LibRaw FFI bindings and
//! the decode implementation land in milestone 1 issue 3; until then this
//! crate pins the decoder identity and maps the canonical
//! [`DecodeContract`] to the LibRaw parameter values the decode layer must
//! apply. The mapping is testable without linking LibRaw so the contract
//! cannot drift between issue 1 and issue 3.

use colorbalance_core::contract::{ContractError, DecodeContract};

/// Decoder identity recorded in every profile.
pub const DECODER_NAME: &str = "libraw";

/// LibRaw version the decode contract is pinned against.
///
/// Milestone 1 issue 3 vendors and links LibRaw and must fail its build or
/// tests when the linked version differs from this constant.
pub const PINNED_LIBRAW_VERSION: &str = "0.21.0";

/// The canonical decode contract for the pinned LibRaw version.
pub fn canonical_contract() -> DecodeContract {
    DecodeContract::canonical(DECODER_NAME, PINNED_LIBRAW_VERSION)
}

/// LibRaw parameter values that realize a validated [`DecodeContract`].
///
/// Field names follow the LibRaw `libraw_dcraw_data_t` / `dcraw_params`
/// naming so issue 3 can assign them directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LibRawParams {
    /// `0` selects raw colorimetry, no output color conversion.
    pub output_color: i32,
    /// `gamm[0]` power and `gamm[1]` slope, linear.
    pub gamm: [f64; 2],
    /// Fixed white balance multipliers for `[R, G1, B, G2]`.
    pub user_mul: [f32; 4],
    pub use_camera_wb: bool,
    pub use_auto_wb: bool,
    pub no_auto_bright: bool,
    /// 16-bit output.
    pub output_bps: u16,
    /// Demosaic quality selector, `3` is AHD.
    pub user_qual: i32,
    /// `0` clips highlights instead of reconstructing them.
    pub highlight: i32,
    /// `-1` keeps the flip recorded in the RAW file.
    pub user_flip: i32,
}

/// Map a decode contract to the LibRaw parameter values that realize it.
///
/// Returns [`ContractError`] when the contract deviates from the pinned
/// settings, so no caller can construct permissive LibRaw parameters.
pub fn libraw_params(contract: &DecodeContract) -> Result<LibRawParams, ContractError> {
    contract.validate()?;
    Ok(LibRawParams {
        output_color: 0,
        gamm: [1.0, 1.0],
        user_mul: [1.0, 1.0, 1.0, 1.0],
        use_camera_wb: false,
        use_auto_wb: false,
        no_auto_bright: true,
        output_bps: 16,
        user_qual: 3,
        highlight: 0,
        user_flip: -1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use colorbalance_core::contract::WhiteBalancePolicy;

    #[test]
    fn canonical_contract_uses_pinned_decoder_identity() {
        let contract = canonical_contract();
        assert_eq!(contract.decoder, "libraw");
        assert_eq!(contract.decoder_version, PINNED_LIBRAW_VERSION);
        assert!(contract.validate().is_ok());
    }

    #[test]
    fn canonical_contract_maps_to_pinned_libraw_params() {
        let params = libraw_params(&canonical_contract()).expect("valid contract");
        assert_eq!(params.output_color, 0);
        assert_eq!(params.gamm, [1.0, 1.0]);
        assert_eq!(params.user_mul, [1.0, 1.0, 1.0, 1.0]);
        assert!(!params.use_camera_wb);
        assert!(!params.use_auto_wb);
        assert!(params.no_auto_bright);
        assert_eq!(params.output_bps, 16);
        assert_eq!(params.user_qual, 3);
        assert_eq!(params.highlight, 0);
        assert_eq!(params.user_flip, -1);
    }

    #[test]
    fn deviating_contract_yields_no_libraw_params() {
        let mut contract = canonical_contract();
        contract.white_balance = WhiteBalancePolicy::Auto;
        assert!(libraw_params(&contract).is_err());

        let mut contract = canonical_contract();
        contract.no_auto_bright = false;
        assert!(libraw_params(&contract).is_err());
    }
}
