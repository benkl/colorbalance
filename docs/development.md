# Development guide

How to build, test, and verify ColorBalance locally. See `AGENTS.md` for working rules and `docs/ARCHITECTURE.md` for the platform decisions behind this setup.

## Toolchain

- Rust stable, managed with rustup. `rust-toolchain.toml` selects it automatically.
- Minimum supported Rust version: 1.85 (workspace `rust-version`).
- The `wasm32-unknown-unknown` target is required to check `colorbalance-core` for the browser build:

```text
rustup target add wasm32-unknown-unknown
```

## Commands

```text
cargo fmt --all                     # format
cargo fmt --all --check             # CI format gate
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p colorbalance-cli
cargo build -p colorbalance-core --target wasm32-unknown-unknown
```

The build writes the `colorbalance` binary to `target/release/colorbalance` (`.exe` on Windows).

## Behavioral smoke test

CI and local verification run the same command:

```text
colorbalance decode-contract
```

It prints the canonical RAW decode contract as JSON. The pinned settings are raw colorimetry, linear gamma, unity white balance multipliers, disabled auto brightening, AHD demosaic, clipped highlights, as-shot orientation, and 16-bit output. Any change to these values is a contract change and must update `crates/colorbalance-core/src/contract.rs`, its tests, the CLI test, and the CI grep list together.

The built-in DNG decoder accepts uncompressed 16-bit CFA and a narrow three-component 12-bit LinearRaw/SOF3 lossless-JPEG layout (including the Samsung Galaxy S25 file checked in October 2026). Other camera RAW formats still need LibRaw. The LinearRaw path uses the DNG RGB components directly, flags saturation from those samples, and refuses unsupported crops or pixel-changing opcodes. `research/samsung_reference_decode.py` can independently decode a local SOF3 strip with `imagecodecs`, `numpy`, and `tifffile`; it is not a production dependency. It splits restart intervals because a single libjpeg decode can silently repeat the first interval on this file.

## Workspace layout

| Crate | Role | wasm32 |
| --- | --- | --- |
| `colorbalance-core` | Decode contract, chart model, color math, profiles. No LibRaw, Tauri, CLI, or UI dependencies. `unsafe` forbidden. | yes |
| `colorbalance-raw` | LibRaw layer: decoder identity, contract-to-parameter mapping, later the FFI decode implementation. `unsafe` only inside the FFI module. | no |
| `colorbalance-cli` | `clap` command-line interface over the core operations. `unsafe` forbidden. | no |

`Cargo.lock` is committed. A clean checkout builds from it.

## LibRaw notes

`colorbalance-raw` pins the decoder identity (`libraw`) and version (`PINNED_LIBRAW_VERSION`). The FFI bindings, vendored LibRaw build, and deterministic decode implementation are milestone 1 issue 3. When that issue lands, this document must describe how LibRaw is vendored and built per platform, and the build or tests must fail when the linked version differs from the pin.

## CI

`.github/workflows/ci.yml` runs on every push to `main` and every pull request:

- format check, clippy with `-D warnings`, tests, release build, and the behavioral smoke command on Ubuntu, macOS, and Windows;
- a `wasm32` job that builds `colorbalance-core` for `wasm32-unknown-unknown`.

PRs that fail any gate do not merge. Do not weaken a gate to make CI pass; fix the code or change the documented decision.
