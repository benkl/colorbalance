//! ColorBalance command-line interface.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use colorbalance_core::DecodeContract;

#[derive(Parser)]
#[command(
    name = "colorbalance",
    version,
    about = "Derive a ColorChecker color transform and apply it to RAW batches",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the canonical RAW decode contract as JSON
    DecodeContract,
}

/// Render the canonical decode contract as pretty JSON.
fn decode_contract_json(contract: &DecodeContract) -> String {
    serde_json::to_string_pretty(contract).expect("decode contract serializes")
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::DecodeContract => {
            let contract = colorbalance_raw::canonical_contract();
            println!("{}", decode_contract_json(&contract));
            ExitCode::SUCCESS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_contract_json_contains_every_pinned_setting() {
        let json = decode_contract_json(&colorbalance_raw::canonical_contract());
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");

        assert_eq!(value["decoder"], "libraw");
        assert_eq!(value["output-color"], "raw");
        assert_eq!(value["output-depth"], "u16");
        assert_eq!(value["gamma"], serde_json::json!([1.0, 1.0]));
        assert_eq!(
            value["white-balance"]["unity"]["user-mul"],
            serde_json::json!([1.0, 1.0, 1.0, 1.0])
        );
        assert_eq!(value["no-auto-bright"], true);
        assert_eq!(value["demosaic"], "ahd");
        assert_eq!(value["highlight"], "clip");
        assert_eq!(value["orientation"], "as-shot");
        assert_eq!(value["no-auto-scale"], false);
    }

    #[test]
    fn decode_contract_json_parses_back_into_a_valid_contract() {
        let contract = colorbalance_raw::canonical_contract();
        let json = decode_contract_json(&contract);
        let parsed: DecodeContract = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, contract);
        assert!(parsed.validate().is_ok());
    }
}
