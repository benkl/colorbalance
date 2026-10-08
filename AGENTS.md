# AGENTS.md

Working guide for AI agents and other contributors entering this repository. Read this file before making changes.

## What this project is

ColorBalance derives a measured color transform from one ColorChecker Classic reference photo and applies it to batches captured with the same camera, lighting, exposure, and decode settings. Correctness of the color pipeline and safety of the user's files outrank features and speed.

## Current state

Milestones 1 to 4 are implemented and their issues are closed. Milestone 5 (issues 21 to 25: WebAssembly parity, browser decode spike, browser file handling, hosted prototype, benchmark harness) is open and has no implementation beyond a CI check that `colorbalance-core` builds for `wasm32-unknown-unknown`.

What exists:

- `colorbalance-core`: decode contract, chart datasets, patch sampling, quality gates, matrix fit, profiles, 16-bit TIFF encoding, bounded batch scheduler, CLF and `.cube` export, and `detection.rs`, the chart locator.
- `colorbalance-raw`: RAW decoding through the `rawler` crate (`rawler_decode.rs`), JPEG/PNG loading for the quick-and-dirty approximation, and `dng_writer.rs`, a DNG writer used only by test fixtures. rawler returns undemosaiced photosites. The AHD demosaic is in `colorbalance-core/src/ahd.rs`. Decoder identity is `rawler-ahd` plus the pinned rawler version. Verified only on synthetic DNGs; there are no real camera RAW fixtures yet.
- `colorbalance-cli`: `decode-contract`, `inspect`, `derive`, `apply`, `export`. The CLI does not call the detector. Without `--quad` it samples an 8% inset rectangle.
- `apps/desktop`: Tauri 2 shell (`src-tauri`) and React UI (`frontend`). Commands: `load_reference`, `detect_chart`, `check_chart`, `inspect_reference`, `derive_profile`, `correct_image`, `apply_batch`, `preflight_batch`, `cancel_batch`, `export_profile`, Library list and save, and three native pickers. One decoded reference is cached by path, length, and mtime.

Not done: CLI auto-detection, DCP export, signed installers, any browser or hosted mode, real camera RAW fixtures. Do not describe these as working.

## Required reading before working

1. `docs/IMPLEMENTATION_PLAN.md` - the product contract. The color pipeline, quality gates, interchange rules, milestones, and per-issue acceptance criteria live here.
2. `docs/ARCHITECTURE.md` - platform and performance decisions, repository layout, engine API, detection cost and limits, and the decision log (D1 to D16).
3. The GitHub issue you are implementing, including its acceptance criteria.
4. `docs/USAGE.md` and `docs/development.md` when you touch user-visible behavior or build steps.

If code and these documents disagree, the documents win until a decision record changes them. Update the decision log in `docs/ARCHITECTURE.md` and the affected issues in the same change that reverses a decision.

## Non-negotiable invariants

These exist because violating them silently produces wrong color or destroys user data. Never weaken them to make an issue close faster.

1. Fixed decode contract. Every image in a profile's scope is decoded with identical settings: raw colorimetry, linear gamma, unity white balance multipliers, no auto brightening, fixed demosaic, highlight, scaling, and 16-bit depth. The settings travel inside the profile.
2. No scene-inferred exposure or white balance. `apply` uses the stored exposure scalar and neutral channel scaling, or an explicit user stop offset. It never estimates brightness or neutral from arbitrary scene pixels.
3. Clipping is checked in the RAW domain, before demosaicing, using per-channel photosite saturation masks. Demosaiced values hide sparse single-channel clipping.
4. Named chart dataset. The physical ColorChecker revision is selected explicitly and pinned with its dataset version, illuminant, and observer. The patch layout cannot identify the revision. The chart detector finds corners only. It never selects a revision and never derives a profile.
5. Fail closed. Camera, decoder-contract, or exposure mismatch stops processing by default. Overrides are recorded in the profile and batch report. A detector miss or ambiguous result leaves the corners untouched for manual placement. One sanctioned exception (D26): applying a desktop Library entry may proceed across a camera or decode-contract mismatch. The batch report records a warning for every such file. Every other path, and exposure mismatches everywhere, still fail closed.
6. Never modify input files. Output goes to a new path through a unique temporary file in the destination directory, flushed, then atomically renamed. Existing outputs fail or skip unless overwrite is explicit. Never delete-then-rename.
7. One color implementation. All color math lives in `colorbalance-core` and is shared by CLI, desktop, browser, and server. Conversion into output spaces other than sRGB is delegated to OCIO, called only from core; there is no second implementation and no hand-written matrix or curve for those spaces (D24). TypeScript never computes color. Python never runs in production.
8. Exports are honest. CLF and `.cube` files consume normalized linear camera RGB from this tool's decode contract. Documentation and UI must not imply they decode RAW or accept rendered sRGB.

## Repository map

```text
crates/colorbalance-core/    native + wasm32. No rawler, Tauri, CLI, or UI dependencies. unsafe forbidden.
crates/colorbalance-raw/     rawler adapter and image loading (native only). unsafe denied.
crates/colorbalance-cli/     clap CLI
crates/colorbalance-fixtures/ shared test fixtures
apps/desktop/src-tauri/      Tauri 2 shell. Excluded from the workspace: build it from its own directory.
apps/desktop/frontend/       React, Vite, Tailwind. Strict TypeScript. No color math.
research/                    Python fixture generators. Never a runtime dependency.
tests/                       cross-crate fixtures
docs/                        plan, architecture, usage, development, packaging, release gate
```

Desktop UI flow lives in `frontend/src/App.tsx`, the corner overlay in `components/LightTableOverlay.tsx`, and the Tauri bridge in `tauri.ts`. IPC commands are in `src-tauri/src/ipc.rs` and `commands.rs`.

## Chart detector notes

`crates/colorbalance-core/src/detection.rs`, `detect_chart(&DecodedImage) -> Detection` with `Found(ChartQuad)`, `Missing`, or `Ambiguous`.

- Samples a thumbnail of at most 512 px per side. Finds bright patch squares at several luminance levels, groups them into a lattice, and validates the whole grid (dark gaps, descending neutral row, limited chroma in neutrals, enough colorful patches). Both reading directions are tried.
- A second distinct valid region returns `Ambiguous`.
- The desktop runs it automatically only for images of 12 million pixels or fewer, after the preview shows. The FIND CHART button runs it at any size.
- Frontend rules that keep it safe: generation refs discard stale results, dragging a corner cancels a pending result, and INSPECT and DERIVE stay disabled while detection runs.
- Known misses: charts narrower than about 48 thumbnail pixels, heavy glare, occlusion, low contrast, merged patches. Prefer a miss over a wrong quad. If you tune thresholds, re-run the regression tests and the real sample (`20261003_183314.jpg`, repo root).
- Measured timings and their conditions are in `docs/ARCHITECTURE.md`. Do not quote numbers you did not measure.

## How work is organized

- One GitHub issue per unit of work, grouped by milestone 1 through 5.
- An issue is complete only when its acceptance criteria are observable. Tests alone are not proof; run the changed path and report what you saw.
- Dependencies between issues are stated in the issue body. Do not start an issue whose dependency is open unless the issue says otherwise.
- Quality gates and thresholds from milestone 3 issue 13 have fixed numbers in `docs/model-selection-and-tolerances.md`. Do not change them without a decision record (D18 records the CV mean floor).
- When behavior changes, update the affected docs and issues in the same commit.

## Conventions

- Rust 1.89 or newer (rawler requires it). `cargo fmt`, `cargo clippy`, and `cargo test` must pass. No `unsafe` in the workspace crates; `colorbalance-core` forbids it and `colorbalance-raw` denies it.
- `colorbalance-core` compiles for native and `wasm32` and must not depend on rawler, Tauri, CLI, or UI crates.
- TypeScript: strict mode, no color math, no full-resolution pixel buffers across the UI boundary.
- Desktop commands never run image work on the main thread (D13).
- Python lives only under `research/` and exists to generate independent numerical fixtures and verify formulas.
- Documentation changes ride in the same commit or pull request as the behavior they describe.
- Do not commit generated output. `colorbalance_profile.cbprofile.json` and `colorbalance_report.html` are gitignored.

## Verification expectations

- Reproduce a bug before fixing it and confirm the fix afterward.
- Numerical work needs fixtures with values computed independently of the code under test. `research/` exists for this.
- Batch safety changes need a failure injection: corrupt input, existing output, cancellation mid-write, commit failure. Completed outputs must survive and temporary files must be cleaned.
- Performance changes need numbers from a before and after run. There is no benchmark harness yet (issue 25), so state how you measured.
- UI changes need a run of the real app, not only `npm test`. The recipe is in `docs/development.md`.

## Commands

```text
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p colorbalance-cli
cargo build -p colorbalance-core --target wasm32-unknown-unknown

# desktop backend, from apps/desktop/src-tauri
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test

# frontend, from apps/desktop/frontend
npm run build && npm test && npm run lint
```

The behavioral smoke command is `target/release/colorbalance decode-contract`, which prints the canonical decode contract as JSON. Details, toolchain versions, and the desktop smoke recipe live in `docs/development.md`. CI runs the workspace gates on Ubuntu, macOS, and Windows, plus the wasm32 build. CI does not build the desktop app or run the frontend tests.

Windows notes: stop a running desktop exe before `cargo build` or `cargo test` in `apps/desktop/src-tauri`, otherwise the linker fails with an access-denied error. Git warns that LF will become CRLF. That is harmless.

## Glossary

- Profile: a `*.cbprofile.json` file storing transform stages, decode contract, chart dataset, camera identity, and validation results.
- Decode contract: the exact decoder settings and camera identity a profile is valid for.
- Saturation mask: per-channel flags marking photosites at or above the RAW clipping threshold, taken before demosaicing. After AHD each flag covers the 11x11 neighbourhood around a clipped photosite (support radius 5).
- CLF: Academy Common LUT Format, the interchange export for the transform stages.
- DCP: DNG Camera Profile, the deferred format for RAW-editor interoperability.
- Decode contract digest: hash identifying the decoder and settings recorded in a profile.
- Quick-and-dirty: approximate calibration from a rendered JPEG/PNG. Gates are relaxed and the result is flagged in the profile and report.
