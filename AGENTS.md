# AGENTS.md

Working guide for AI agents and other contributors entering this repository. Read this file before making changes.

## What this project is

ColorBalance derives a measured color transform from one ColorChecker Classic RAW reference photo and applies it to RAW batches captured with the same camera, lighting, exposure, and decode settings. Correctness of the color pipeline and safety of the user's files outrank features and speed.

Current state: documentation and GitHub roadmap only. There is no implementation yet. The first code lands in milestone 1, issue 1.

## Required reading before working

1. `docs/IMPLEMENTATION_PLAN.md` - the product contract. The color pipeline, quality gates, interchange rules, milestones, and per-issue acceptance criteria live here.
2. `docs/ARCHITECTURE.md` - platform and performance decisions, repository layout, engine API, and the decision log.
3. The GitHub issue you are implementing, including its acceptance criteria.

If code and these documents disagree, the documents win until a decision record changes them. Update the decision log in `docs/ARCHITECTURE.md` and the affected issues in the same change that reverses a decision.

## Non-negotiable invariants

These exist because violating them silently produces wrong color or destroys user data. Never weaken them to make an issue close faster.

1. Fixed decode contract. Every image in a profile's scope is decoded with identical settings: raw colorimetry, linear gamma, unity white balance multipliers, no auto brightening, fixed demosaic, highlight, scaling, and 16-bit depth. The settings travel inside the profile.
2. No scene-inferred exposure or white balance. `apply` uses the stored exposure scalar and neutral channel scaling, or an explicit user stop offset. It never estimates brightness or neutral from arbitrary scene pixels.
3. Clipping is checked in the RAW domain, before demosaicing, using per-channel photosite saturation masks. Demosaiced values hide sparse single-channel clipping.
4. Named chart dataset. The physical ColorChecker revision is selected explicitly and pinned with its dataset version, illuminant, and observer. The patch layout cannot identify the revision.
5. Fail closed. Camera, decoder-contract, or exposure mismatch stops processing by default. Overrides are recorded in the profile and batch report.
6. Never modify input files. Output goes to a new path through a unique temporary file in the destination directory, flushed, then atomically renamed. Existing outputs fail or skip unless overwrite is explicit. Never delete-then-rename.
7. One color implementation. All color math lives in `colorbalance-core` and is shared by CLI, desktop, browser, and server. TypeScript never computes color. Python never runs in production.
8. Exports are honest. CLF and `.cube` files consume normalized linear camera RGB from this tool's decode contract. Documentation and UI must not imply they decode RAW or accept rendered sRGB.

## How work is organized

- One GitHub issue per unit of work, grouped by milestone 1 through 5.
- An issue is complete only when its acceptance criteria are observable. Tests alone are not proof; run the changed path and report what you saw.
- Dependencies between issues are stated in the issue body. Do not start an issue whose dependency is open unless the issue says otherwise.
- Quality gates and thresholds set in milestone 3 issue 13 must have fixed numbers before export work starts.

## Conventions

- Rust: `cargo fmt`, `cargo clippy`, and `cargo test` must pass. No `unsafe` outside the LibRaw FFI layer.
- `colorbalance-core` compiles for native and `wasm32` and must not depend on LibRaw, Tauri, CLI, or UI crates.
- TypeScript: strict mode, no color math, no full-resolution pixel buffers across the UI boundary.
- Python lives only under `research/` and exists to generate independent numerical fixtures and verify formulas.
- Documentation changes ride in the same commit or pull request as the behavior they describe.

## Verification expectations

- Reproduce a bug before fixing it and confirm the fix afterward.
- Numerical work needs fixtures with values computed independently of the code under test. `research/` exists for this.
- Batch safety changes need a failure injection: corrupt input, existing output, cancellation mid-write, commit failure. Completed outputs must survive and temporary files must be cleaned.
- Performance changes need numbers from the benchmark harness before and after.

## Commands

```text
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p colorbalance-cli
cargo build -p colorbalance-core --target wasm32-unknown-unknown
```

The behavioral smoke command is `target/release/colorbalance decode-contract`, which prints the canonical decode contract as JSON. Details, toolchain versions, and LibRaw notes live in `docs/development.md`. CI runs the same gates on Ubuntu, macOS, and Windows.

## Glossary

- Profile: a `*.cbprofile.json` file storing transform stages, decode contract, chart dataset, camera identity, and validation results.
- Decode contract: the exact decoder settings and camera identity a profile is valid for.
- Saturation mask: per-channel flags marking photosites at or above the RAW clipping threshold, taken before demosaicing.
- CLF: Academy Common LUT Format, the interchange export for the transform stages.
- DCP: DNG Camera Profile, the deferred format for RAW-editor interoperability.
- Decode contract digest: hash identifying the decoder and settings recorded in a profile.
