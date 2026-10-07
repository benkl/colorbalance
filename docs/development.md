# Development guide

How to build, test, and verify ColorBalance locally. See `AGENTS.md` for working rules and `docs/ARCHITECTURE.md` for the platform decisions behind this setup.

## Toolchain

- Rust stable, managed with rustup. `rust-toolchain.toml` selects it automatically.
- Minimum supported Rust version: 1.89 (workspace `rust-version`).
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

RAW files are decoded by the `rawler` crate (pinned 0.8) and demosaiced by the in-repo AHD in `colorbalance-core`. The adapter accepts a 2x2 RGB Bayer CFA or three-component LinearRaw at full frame; other layouts, crops and float samples fail closed. The only camera files checked so far are synthetic DNGs written by `dng_writer.rs` and, in October 2026, a Samsung Galaxy S25 LinearRaw DNG before the switch to rawler; no real camera RAW fixtures are in the repo. `research/samsung_reference_decode.py` can independently decode a local SOF3 strip with `imagecodecs`, `numpy`, and `tifffile`; it is an independent reference, not a production dependency. AHD is tested with analytic cases in `ahd.rs`, not Python fixtures. Profiles recorded with the old `colorbalance-dng` decoder no longer match and must be re-derived.

## Workspace layout

| Crate | Role | wasm32 |
| --- | --- | --- |
| `colorbalance-core` | Decode contract, chart datasets, sampling, chart detection, color math, profiles, batch scheduler, TIFF encoding, CLF and `.cube` export. No rawler, Tauri, CLI, or UI dependencies. `unsafe` forbidden. | yes |
| `colorbalance-raw` | rawler adapter (`rawler_decode.rs`), JPEG/PNG loading, decoder identity, test-only DNG writer. `unsafe` denied. | no |
| `colorbalance-cli` | `clap` command-line interface over the core operations. `unsafe` forbidden. | no |
| `colorbalance-fixtures` | Shared test fixtures. | no |

`apps/desktop/src-tauri` is a separate Cargo project, excluded from the workspace, so `cargo test --workspace` does not build it. `Cargo.lock` is committed. A clean checkout builds from it.

## Desktop app

Frontend (`apps/desktop/frontend`, Node.js 20 or newer):

```text
npm ci
npm run build     # tsc -b and vite build, output in dist/, which the Tauri build embeds
npm test          # node --test over interaction and UI-to-IPC tests
npm run lint      # oxlint
npm run dev       # Vite dev server for the UI alone: browser preview mode with a synthetic demo, no native backend, no DNG decode
```

Backend (`apps/desktop/src-tauri`):

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run         # or cargo build, then run target/debug/colorbalance-desktop
```

Run `npm run build` before building the backend after any frontend change, because the bundle is embedded. On Windows, close the running app first or the build fails with an access-denied error on the exe. The test runs may write `colorbalance_profile.cbprofile.json` and `colorbalance_report.html` into the working directory. Both are gitignored.

CI does not build the desktop app or run the frontend tests. Run both locally before changing them.

### Smoke-testing the real app

`npm test` mocks the Tauri bridge. For UI work, drive the real window too. On Windows, WebView2 exposes the Chrome DevTools Protocol:

```text
cd apps/desktop/src-tauri && cargo build
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9333" ./target/debug/colorbalance-desktop.exe
```

List targets at `http://127.0.0.1:9333/json` and attach to the page over its WebSocket. Load a file by emitting the drop event from the page:

```js
window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
  event: 'native-file-drop',
  payload: { paths: ['C:/path/to/reference.jpg'] },
})
```

Commands such as `load_reference` and `detect_chart` can be called the same way through `__TAURI_INTERNALS__.invoke`. Check the status line, the corner readout, and that dragging a corner keeps your edit.

## Chart detector checks

`cargo test -p colorbalance-core detection` runs the synthetic cases: offset chart, rotated and perspective chart, chart merged with a dark scene, blank image, and two charts (must return `Ambiguous`). On 2026-10-07 the detector also found the chart in the repo sample `20261003_183314.jpg`; patch colors sampled at the returned corners followed ColorChecker order (dark skin first, white to black along the bottom row). Timings are in `docs/ARCHITECTURE.md`. The CLI does not call the detector.

## Decoder notes

`colorbalance-raw` pins the decoder identity `rawler-ahd` and the rawler version in `rawler_decode.rs` (`DECODER_NAME`, `DECODER_VERSION`). Raise `DECODER_VERSION` together with the `rawler` entry in `crates/colorbalance-raw/Cargo.toml`; profiles record both. A version-only difference between profile and run is a warning; a name or settings difference blocks apply unless `--force` is given. rawler is LGPL-2.1 (D18). rawler can panic on unknown file layouts; the adapter catches the panic and reports an unsupported format.

## CI

`.github/workflows/ci.yml` runs on every push to `main` and every pull request:

- format check, clippy with `-D warnings`, tests, release build, and the behavioral smoke command on Ubuntu, macOS, and Windows;
- a `wasm32` job that builds `colorbalance-core` for `wasm32-unknown-unknown`.

PRs that fail any gate do not merge. Do not weaken a gate to make CI pass; fix the code or change the documented decision.
