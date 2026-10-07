# ColorBalance architecture

Status: milestones 1 to 4 are implemented (CLI, rawler-based RAW decode with an in-repo AHD demosaic, calibration core, batch apply, CLF and `.cube` export, Tauri 2 desktop app). Milestone 5 is open: `colorbalance-core` builds for `wasm32`, but there is no wasm-bindgen glue, worker, browser app, or hosted mode yet. This document records the platform and performance decisions behind the product. `IMPLEMENTATION_PLAN.md` defines the milestones. When a decision here changes, update the decision log at the end of this file and the affected issues in the same change.

## Product shape

ColorBalance derives a color transform from one ColorChecker Classic RAW reference and applies it to RAW batches captured with the same camera, lighting, exposure, and decode settings. The product must deliver a local, high-performance workflow and stay able to run in a browser without rewriting the color engine.

## Deployment modes

| Mode | Status | Role |
| --- | --- | --- |
| Rust CLI, native | shipped | `decode-contract`, `inspect`, `derive`, `apply`, `export`. No automatic chart detection; manual `--quad` or an 8% inset rectangle |
| Tauri 2 desktop with React UI | shipped, unsigned, no published release | Full local workflow with rawler RAW decode, JPEG/PNG decode, automatic chart finding, unrestricted file access |
| Browser, WebAssembly engine | not started | The core builds for `wasm32` in CI (it does not depend on rawler). Parity tests, worker, and RAW decode are open (issues 21 to 23). Browser RAW decode needs a spike on whether rawler builds and fits for `wasm32` (issue 22) |
| Hosted web service | not started | Optional, deferred until upload cost and demand are known (issue 24) |

Tauri ships first because it reuses the web UI while keeping RAW decoding native. The browser and hosted modes are added behind the same engine API, never as a second color implementation.

## Stack

| Concern | Technology | Reason |
| --- | --- | --- |
| Color engine, fitting, batch logic | Rust | One core compiles to native, WebAssembly, and server workers; predictable memory behavior for large images |
| RAW decode | `rawler` crate returns undemosaiced photosites; AHD demosaic lives in `colorbalance-core` | One maintained decoder instead of a hand-written one. The decoder name and rawler version are recorded in every profile (D18) |
| Chart detection | Pure Rust in `colorbalance-core`, on a bounded thumbnail | One deterministic detector for native and potential WebAssembly use, without OpenCV; weak or competing matches return no automatic corners |
| Matrix math | `nalgebra` | Least squares and 3x3 fitting without a hand-rolled solver |
| Parallelism | `rayon` | Native per-image and per-tile CPU parallelism |
| Serialization | `serde`, `serde_json`, `jsonschema` crate | Versioned `*.cbprofile.json` with schema validation |
| TIFF output and metadata | `tiff` crate or libtiff binding, plus `kamadak-exif` or Exiv2 | 16-bit output, embedded ICC, reviewed EXIF copying |
| CLI | `clap` | Typed subcommands matching the documented contract |
| Desktop shell | Tauri 2 | Web UI in the system webview, native engine, small install size |
| Web UI | React, TypeScript, Vite | One UI for Tauri and browser; no server-rendered framework needed |
| Browser compute | Rust compiled to WebAssembly, run in Web Workers | Keeps pixel buffers out of JavaScript |
| Optional GPU | WebGPU | Only after CPU paths are measured; never required for correctness |
| Hosted mode, if built | Axum, PostgreSQL, S3-compatible storage, SSE | Native workers behind an API; image bytes never pass through the API server |
| Research verification | Python with `colour-science` and `colour-checker-detection` | Generates independent numerical fixtures and cross-checks formulas; never a runtime dependency |

Python is the reference and verification stack, not the production pixel path. It cannot become part of the browser application, and shipping two color implementations would let desktop and web results drift.

## Repository layout

```text
crates/
  colorbalance-core/      # color math, chart sampling and detection, fitting, profiles, batch scheduler, exports
  colorbalance-raw/       # rawler adapter, JPEG/PNG loading, test-only DNG writer (native only)
  colorbalance-cli/       # clap CLI (native only)
  colorbalance-fixtures/  # shared test fixtures
apps/
  desktop/
    src-tauri/            # Tauri 2 shell, separate Cargo project (native only)
    frontend/             # React, Vite, Tailwind UI
  web/                    # planned browser experiment (milestone 5), does not exist yet
research/                 # Python fixture generators and cross-checks (non-runtime)
tests/                    # cross-crate fixtures
docs/                     # plan, architecture, usage, development, packaging, release gate
```

`colorbalance-core` must compile for native and `wasm32` targets and must not depend on rawler, Tauri, or the CLI. Everything file-format-specific lives in `colorbalance-raw` and is injected as a trait implementation.

## Engine API

The core exposes operations, not files or UI concepts:

```rust
inspect_reference(...) -> ReferenceInspection
derive_profile(...) -> DerivedProfile
apply_profile(...) -> ApplySummary
export_clf(...) -> ClfArtifact
export_cube(...) -> CubeArtifact
```

Each caller supplies a decoder implementation:

- native CLI and Tauri use the rawler adapter in `colorbalance-raw`;
- the browser uses the WebAssembly decoder selected in milestone 5;
- tests use synthetic and fixture decoders.

This is the boundary that keeps desktop, browser, and server modes on one color implementation.

## UI and IPC rules

The React UI implements the four-step workflow: reference, validation, batch, processing.

Rules that apply to every UI host:

- No color calculation lives in TypeScript. The UI renders state and calls engine operations.
- Full-resolution pixel buffers never cross the UI boundary. Tauri commands receive paths and return compact results, measurements, and progress events.
- Previews are encoded low-resolution images produced by the engine, not raw arrays serialized through JSON IPC.
- Long operations report progress and remain cancellable. Cancellation stops scheduling new files and finishes or removes active temporary files.
- Tauri commands that decode or process images are `async` and run their work on the blocking thread pool (`spawn_blocking`); native dialog commands are `async` too. Synchronous commands run on the main thread and freeze the window. Stage progress arrives as the `operation-progress` event (`{operation, stage, step, steps}`) and batch progress as `batch-progress` (`{completed, total, file}`). The frontend listens through `core:event:default`, which `capabilities/default.json` must grant: without it `listen` is rejected and the progress strip never updates.
- The desktop backend keeps the most recent decoded reference (and its preview) in `AppState`, so `load_reference`, `inspect_reference`, `derive_profile` and `correct_image` (BEFORE / AFTER and SAVE REFERENCE) on the same file share one decode and one "before" preview. An entry is valid only for the same canonical path, byte length and modification time; failures and entries over 1 GiB are never cached; the image is shared as an immutable `Arc` with no pixel copy. `correct_image` transforms a copy of the cached pixels, so the cache is never mutated. Batch never uses it. Stage labels read "Using cached image" / "Using cached preview" on a hit.
- Previews are downscaled before the sRGB encode: a linear-light box average to at most 1600 px on the longest side, then encode and PNG. Never encode the full frame to preview it.
- Chart discovery samples a bounded thumbnail (roughly 512 px on the longest side), not a full-resolution detection buffer. After the preview loads, the desktop automatically detects only when `width × height <= 12_000_000`; an explicit Detect chart action runs on any image and can retry a miss. The backend uses the cached decoded reference on a hit. Return a visible quadrilateral for a clear match; a miss or competing chart candidates leave the user to place four corners. Neither automatic detection nor manual corners select the physical chart revision; that selection is explicit before fitting.

## Performance design

### Memory budget

A 45-megapixel RGB image in 32-bit float is about 540 MB before demosaicing buffers, masks, and output. The engine therefore allocates and processes image buffers inside Rust and never copies them into JavaScript. The UI receives paths, measurements, and previews.

### Detection cost and limits

Detection work is bounded by the thumbnail after decoding, but decoding the source and collecting thumbnail samples still costs time on large files. Measured on 2026-10-07 (release build, `detect_chart` on an already decoded image, one run each): the 1398x1864 sample JPEG with a chart found in 6.7 ms; a synthetic 4200x3200 (13.4 MP) frame with that chart pasted in found it in 9.5 ms; a 1200x900 frame without a chart returned `Missing` in 10.6 ms. These are single runs on one development machine, not a latency target, and the detector has not been run on RAW fixtures. The detector finds the 24 bright patches as separate squares and fits the lattice, so a chart whose dark backing touches other dark scene content is still found. Charts below roughly 48 thumbnail pixels wide, heavy skew, glare, occlusion, low contrast, or patches that merge with the gaps may be missed. Failing closed is better than placing a plausible but wrong quad.

### Tiling

Everything after decode is per-pixel independent and runs in tiles or scanlines:

```text
decode tile -> normalize -> channel scaling -> 3x3 matrix -> clip and count -> sRGB encode -> write
```

Demosaicing needs the full frame. It runs in `colorbalance-core::ahd` on the whole decoded mosaic. The pipeline after decoding must not allocate additional full-size copies.

### CPU before GPU

The shipped transform is one scalar, three channel multipliers, one 3x3 matrix, clipping, and the sRGB transfer function. Native SIMD and threads handle this. Decode, demosaicing, and TIFF compression dominate runtime. WebGPU remains an optional acceleration path for previews or nonlinear 3D LUT work after measurements justify it.

### WebAssembly constraints

- Run the engine in a Web Worker, never on the UI thread.
- Move image data with transferable `ArrayBuffer` objects, not JSON.
- Enable WebAssembly SIMD where available and keep a scalar fallback.
- Add WebAssembly threads only after the single-worker path works, because threads require cross-origin isolation headers (COOP and COEP).
- Keep only active images in memory and process the batch one file at a time.

## Browser file handling

- Use the File System Access API where available for directory input and streamed output.
- Fall back to file input plus downloaded outputs on browsers without it.
- Store recoverable job state in IndexedDB or origin-private storage.
- Publish a support matrix by browser and operating system instead of assuming uniform behavior.

## Hosted mode, if built

```text
React UI -> Axum API -> PostgreSQL job state
                     -> S3-compatible storage, presigned multipart uploads
                     -> native Rust workers with the rawler decoder
                     -> SSE progress, signed download URLs
```

Rules:

- RAW files upload directly to object storage with presigned multipart URLs. Image bytes never pass through the API server.
- Workers are stateless and read job definitions from the queue.
- PostgreSQL can serve as both database and queue for the first deployment.
- Retention and deletion policies are explicit and documented, because the local-first product does not send images anywhere.

## Decision log

| ID | Decision | Status | Date |
| --- | --- | --- | --- |
| D1 | Product is local-first; no server required for the core workflow | Accepted | 2026-10-03 |
| D2 | Color engine is Rust, shared by CLI, desktop, browser, and server | Accepted | 2026-10-03 |
| D3 | Python is research and verification only, not a runtime dependency | Accepted | 2026-10-03 |
| D4 | Desktop ships as Tauri 2 with the React UI | Accepted | 2026-10-03 |
| D5 | Browser execution is a measured experiment gated on a browser RAW decode spike (issue 22) | Accepted | 2026-10-03 |
| D6 | Hosted mode is optional and deferred until upload cost and demand are known | Accepted | 2026-10-03 |
| D7 | CPU paths first; WebGPU is optional acceleration, never required | Accepted | 2026-10-03 |
| D8 | Profile format is `*.cbprofile.json` regardless of deployment mode | Accepted | 2026-10-03 |
| D9 | CLF and `.cube` exports require the normalized camera-RGB decode contract and do not replace it | Accepted | 2026-10-03 |
| D10 | ~~Built-in pure-Rust DNG decoder with LibRaw FFI planned~~ Superseded by D18 | Superseded | 2026-10-07 |
| D11 | Web delivery mode: Tauri 2 desktop is the primary delivery vehicle. Standalone browser preview is supported with local client-side evaluation and simulated demo fixtures. Direct browser-local RAW decode is gated behind WebAssembly memory constraints. Native desktop has unrestricted local filesystem access and multi-threaded parallel batch execution. | Accepted | 2026-10-04 |
| D12 | The decoder accepts 3-component, 12-bit LinearRaw compressed with lossless JPEG SOF3, when metadata preserves unbalanced linear camera RGB. CFA and LinearRaw remain distinct sensor layouts; clipping is flagged from source samples before orientation or preview. Full-frame crop only; other crops fail closed. Since D18 rawler performs the decode and the pixel-changing DNG opcodes are no longer checked by this project. | Accepted (amended by D18) | 2026-10-06 |
| D13 | Desktop commands never run RAW or image work on the main thread: async commands plus `spawn_blocking`, with progress events for UI feedback | Accepted | 2026-10-07 |
| D14 | Desktop caches one decoded reference keyed on path, length and mtime, for load, inspect, derive and correct. Colour results are unchanged: a hit returns the identical decode, any file change is a miss, and correction never mutates the shared image | Accepted | 2026-10-07 |
| D15 | Previews average the linear image down to the preview size first, then sRGB-encode only the output. Replaces encode-full-frame then Triangle resize. Release, 45 MP: 3.1-3.5 s to 0.24 s | Accepted | 2026-10-07 |
| D16 | Replace planned OpenCV-bound chart detection with a pure-Rust detector in `colorbalance-core` over a bounded thumbnail. Automatic detection runs after preview only through 12 MP; larger images and retries use an explicit action. Misses and ambiguous candidates use manual corners; physical chart revision always requires user selection. Detector latency and limits must be measured, not assumed. | Accepted | 2026-10-07 |
| D17 | Documentation states shipped behavior only. The CLI keeps manual `--quad` input and does not call the detector until a decision record changes that. Support-matrix tiers describe intent; the Verified column records what has actually been run. | Accepted | 2026-10-07 |
| D18 | RAW decode uses the `rawler` 0.8 crate for every camera format; the hand-written DNG decoder and its lossless-JPEG code are deleted. The demosaic is a real AHD (directional green interpolation, colour-difference interpolation, CIELab homogeneity vote) in `colorbalance-core/src/ahd.rs`, so the contract's `demosaic: ahd` is true. Clipping flags are computed per photosite before demosaicing and then spread over the 11x11 neighbourhood (support radius 5) of each clipped photosite. Contract identity is `rawler-ahd` plus the rawler version. Apply blocks on a decoder-name or settings mismatch and `--force` records the override; a version-only difference is a warning in the batch result. The gate coefficient of variation divides by `max(mean, 0.01)` so near-zero out-of-gamut channels are not judged on quantization noise. The workspace minimum Rust version is 1.89 because rawler requires it. rawler is LGPL-2.1 and the project has no licence yet, so distribution terms must be settled before release. `dng_writer.rs` stays for test fixtures only. Profiles with decoder `colorbalance-dng` do not match and must be re-derived. | Accepted | 2026-10-07 |
| D19 | AHD demosaic and the per-pixel apply loop run row-parallel on `rayon`, behind the `parallel` feature of `colorbalance-core`. The CLI, `colorbalance-raw` and the desktop crate enable it; the `wasm32` build leaves it off and runs the same closures in order. Every row writes only its own slice and reads shared input, so output is bit-identical with and without the feature. Batch scheduling claims inputs from a shared atomic counter instead of the fixed `index % workers` stride, so one slow file no longer holds back the files assigned behind it. `BatchOptions::workers` still bounds images in flight, so memory use is unchanged. Synthetic 6000x4000 mosaic, release, 24 logical CPUs, best of 3 runs each: AHD 3.9 s to 0.54 s, apply and encode 1.47 s to 0.10 s, output hash identical before and after. Measured with a throwaway example; the harness is issue 25. `clipping` stays sequential (sliding window across rows). The TIFF writer, 3x3 solver and Lab/CIEDE2000 stay hand-written | Accepted | 2026-10-07 |

## Platform and Browser Support Matrix

Tier is the support goal. Verified is what has been run by hand or in CI as of 2026-10-07. CI builds and tests the Rust workspace on Ubuntu, macOS, and Windows. It does not build the desktop app or run the frontend tests.

| Platform / Environment | Tier | Capabilities | Verified |
| --- | --- | --- | --- |
| Windows 10/11 x64 (Tauri Desktop) | Tier 1 | Native DNG decode, JPEG/PNG decode, multi-threaded batch, native dialogs, native file drop, automatic chart finding, 16-bit TIFF output | Desktop built and driven by hand, including the chart-finding paths. Primary target |
| macOS 12+ x64 / Apple Silicon (Tauri Desktop) | Tier 1 goal | Same feature set as Windows | Core and CLI tests only. Desktop not built |
| Linux x64 (Tauri Desktop / WebKitGTK) | Tier 1 goal | Same feature set as Windows | Core and CLI tests only. Desktop not built |
| Chrome / Edge (browser preview via `npm run dev`) | Tier 2 | Light-table UI, 4-corner alignment, synthetic demo, preview of formats the browser can display. No DNG decode, no automatic chart finding, no derive or apply | Not exercised in a real browser. Frontend tests mock the Tauri bridge. Directory writes through the File System Access API are not implemented (issue 23) |
| Firefox / Safari (browser preview) | Tier 2 | Same as above | Not exercised. File upload is the only input path |

Earlier planning proposed Python with PySide6. Decisions D2 through D4 supersede that stack because browser delivery became a product requirement. The milestone structure, color pipeline, quality gates, and interchange rules in `IMPLEMENTATION_PLAN.md` are unchanged.
