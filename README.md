# ColorBalance

Photograph a ColorChecker Classic once. ColorBalance measures how your camera, light, and exposure rendered it, fits a color transform, and applies that transform to every other frame from the same setup.

It works from a RAW reference (DNG), or from a JPEG/PNG as a flagged approximation. It reports quality problems instead of hiding them. It never modifies an input file, and it refuses to process images that don't match the profile unless you override that on purpose.

Version 0.1.0, pre-release. No license has been chosen yet.

## Status

Milestones 1 to 4 are done and their GitHub issues are closed. Milestone 5 (browser and hosted delivery) is open and mostly unstarted.

| Area | State |
| --- | --- |
| Calibration core: chart sampling, quality gates, matrix fit, validation | Done |
| Batch apply with bounded parallelism, cancellation, 16-bit TIFF output | Done |
| CLF and `.cube` export | Done |
| Desktop app (Tauri 2, React) with light table, editable corners, before and after | Done |
| Automatic chart finding in the desktop app | Done, pure Rust, no OpenCV |
| RAW decode through the `rawler` crate, then this project's AHD demosaic | Done. Verified on synthetic DNGs (uncompressed CFA, 12-bit LinearRaw) and on one Samsung Galaxy S25 LinearRaw DNG, which loads and previews. Other camera formats are rawler's, untested here |
| `colorbalance-core` compiles for `wasm32-unknown-unknown` | Builds, checked in CI. No wasm-bindgen glue, worker, or parity tests yet (issue 21) |
| Browser app, hosted mode, benchmark harness | Not started (issues 22 to 25) |
| Chart auto-detection in the CLI | Not wired. The CLI uses a manual `--quad` or an 8% inset rectangle |
| Signed installers, macOS and Linux packages | Release process is documented in `docs/packaging.md`. Nothing is published |

## What it does

1. Load a reference frame containing a ColorChecker Classic.
2. Find the chart. The desktop app proposes four corners automatically for images of 12 MP or less and on request for larger ones. A miss or an ambiguous result leaves you to place the corners.
3. Choose the physical chart revision yourself. Layout alone cannot identify it.
4. Check the reference. Patches with RAW photosite clipping, too few pixels, or too much variation fail the quality gates, and the report says which patch and which measured value.
5. Fit one exposure scalar, neutral channel scaling, and a 3x3 matrix from linear camera RGB to linear sRGB, and save a `*.cbprofile.json` plus an HTML report.
6. Apply the profile to a folder. Output is 16-bit TIFF, written through a temporary file and renamed atomically. Existing outputs are skipped or fail unless you pass overwrite.
7. Optionally export the transform as CLF or `.cube`. These take normalized linear camera RGB from this tool's decode contract. They do not decode RAW and they are wrong for already-rendered sRGB images.

Apply never estimates brightness or white balance from the scene. It uses the stored scalar and neutral scaling, or an explicit stop offset.

## Quick start

Needs Rust 1.89 or newer (the `rawler` dependency requires it). The desktop app also needs Node.js 20 or newer and, on Windows, the WebView2 runtime.

### CLI

```text
cargo build --release -p colorbalance-cli
target/release/colorbalance decode-contract

target/release/colorbalance inspect reference.dng --chart classic-from-nov-2014 --quad x1,y1,x2,y2,x3,y3,x4,y4
target/release/colorbalance derive reference.dng --chart classic-from-nov-2014 --quad ... \
    --profile look.cbprofile.json --report look.html --overlay look.svg
target/release/colorbalance apply look.cbprofile.json ./batch --output ./out --summary summary.json
target/release/colorbalance export look.cbprofile.json --format clf --output look.clf
```

Corners are top-left, top-right, bottom-right, bottom-left, in image pixels. Without `--quad` the CLI samples a rectangle inset 8% from the image edges, which only works if the chart fills the frame. For JPEG or PNG references add `--quick-and-dirty`. Run any subcommand with `--help` for every option.

### Desktop app

```text
cd apps/desktop/frontend
npm ci
npm run build
cd ../src-tauri
cargo run
```

Drop a reference image on the window or click the light table to pick one. Packaging, installers, and release checks are in [docs/packaging.md](docs/packaging.md).

## Documentation

| Document | Contents |
| --- | --- |
| [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) | Product contract: color pipeline, quality gates, interchange rules, milestones, per-issue acceptance criteria |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Platform and performance decisions, detection cost and limits, decision log D1 to D18 |
| [docs/USAGE.md](docs/USAGE.md) | Capture guide, chart revision, finding the chart, input contract for exports, CLI examples |
| [docs/development.md](docs/development.md) | Toolchain, build and test commands, smoke tests, desktop and frontend workflow |
| [docs/CONTRACTS.md](docs/CONTRACTS.md) | Module interface contracts used during the first implementation |
| [docs/model-selection-and-tolerances.md](docs/model-selection-and-tolerances.md) | Model benchmark, chosen fit, quality thresholds, export tolerances |
| [docs/packaging.md](docs/packaging.md) | Desktop build, platform tiers, packaged smoke workflow |
| [docs/release-gate-1.0.md](docs/release-gate-1.0.md) | Milestone 4 release gate report |
| [AGENTS.md](AGENTS.md) | Working guide for AI agents and contributors |

## Repository layout

```text
crates/
  colorbalance-core/      color math, chart sampling and detection, fitting, profiles,
                          batch scheduling, CLF/cube export. Native and wasm32. No unsafe.
  colorbalance-raw/       rawler decode adapter, rendered-image loading, test-only DNG writer
  colorbalance-cli/       clap CLI: decode-contract, inspect, derive, apply, export
  colorbalance-fixtures/  shared test fixtures
apps/desktop/
  src-tauri/              Tauri 2 shell and IPC commands (separate Cargo workspace)
  frontend/               React, Vite, Tailwind UI. No color math
research/                 Python fixture generators and cross-checks. Never runs in production
tests/                    cross-crate fixtures
docs/                     plan, architecture, usage, development, packaging, release gate
.github/workflows/        CI on Ubuntu, macOS, Windows, plus a wasm32 build of the core
```

## Roadmap

Work is tracked as GitHub issues grouped by milestone. An issue counts as done when its acceptance criteria are observable, not when the code compiles.

| Milestone | State |
| --- | --- |
| [1. Measured calibration core](https://github.com/benkl/colorbalance/milestone/1) | Closed |
| [2. Safe batch workflow](https://github.com/benkl/colorbalance/milestone/2) | Closed |
| [3. Interchange and independent validation](https://github.com/benkl/colorbalance/milestone/3) | Closed |
| [4. Desktop release](https://github.com/benkl/colorbalance/milestone/4) | Closed |
| [5. Web-capable platform](https://github.com/benkl/colorbalance/milestone/5) | Open: WebAssembly parity (#21), browser decode spike (#22, now about rawler, see D18), browser file handling (#23), hosted prototype (#24), benchmark harness (#25). The delivery decision (#26) is closed |

Known gaps outside milestone 5: CLI chart detection, DNG Camera Profile export (deferred on purpose, see `docs/USAGE.md`), real camera RAW fixtures.

## For AI agents

Read [AGENTS.md](AGENTS.md) first. It lists the invariants you must not weaken, the crate boundaries, the commands that gate a change, and how to smoke-test the desktop app. Then read `docs/IMPLEMENTATION_PLAN.md` and `docs/ARCHITECTURE.md`, and the GitHub issue you are working on. If code and those documents disagree, the documents win until a decision record changes them.

Short version of the rules:

- One color implementation, in `colorbalance-core`. TypeScript never computes color.
- Fixed decode contract. No scene-inferred exposure or white balance.
- Clipping is checked per photosite in the RAW domain.
- The chart revision is chosen explicitly. Detection never chooses it.
- Fail closed on camera, decoder, or exposure mismatch. Never modify inputs. Never delete-then-rename outputs.
- Report what you ran and saw. Tests alone are not proof.

## License

Not chosen yet. Decide before the first public release. Note that `rawler` is LGPL-2.1, which constrains how the binaries can be distributed (see decision D18 in `docs/ARCHITECTURE.md`).
