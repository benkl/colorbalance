# ColorBalance architecture

Status: decided, not yet implemented. This document records the platform and performance decisions behind the product. `IMPLEMENTATION_PLAN.md` defines the milestones. When a decision here changes, update the decision log at the end of this file and the affected issues in the same change.

## Product shape

ColorBalance derives a color transform from one ColorChecker Classic RAW reference and applies it to RAW batches captured with the same camera, lighting, exposure, and decode settings. The product must deliver a local, high-performance workflow and stay able to run in a browser without rewriting the color engine.

## Deployment modes

| Mode | Status | Role |
| --- | --- | --- |
| Rust CLI, native | first implementation | proves the decode contract, quality gates, fitting, and output before any UI exists |
| Tauri 2 desktop with React UI | first released application | full local workflow, native LibRaw, unrestricted file access |
| Browser, WebAssembly engine | measured experiment | local processing without install, gated on the LibRaw WebAssembly spike |
| Hosted web service | optional later mode | browser access with native workers, only if uploads and operating cost are acceptable |

Tauri ships first because it reuses the web UI while keeping RAW decoding native. The browser and hosted modes are added behind the same engine API, never as a second color implementation.

## Stack

| Concern | Technology | Reason |
| --- | --- | --- |
| Color engine, fitting, batch logic | Rust | One core compiles to native, WebAssembly, and server workers; predictable memory behavior for large images |
| RAW decode | Built-in pure-Rust DNG decoder (shipped), LibRaw FFI planned for other camera formats | Deterministic normalize-and-flag decode now; broad camera coverage later, cross-checked against the built-in decoder |
| Chart detection | Rust engine using OpenCV bindings | Deterministic templated detection in the shipped product |
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
  colorbalance-core/    # color math, chart sampling, fitting, profiles, quality gates
  colorbalance-raw/     # built-in DNG decoder now; LibRaw FFI planned (native only)
  colorbalance-cli/     # clap CLI (native only)
apps/
  desktop/              # Tauri 2 shell (native only)
  web/                  # browser experiment (milestone 5)
research/               # Python verification notebooks and fixture generators

`colorbalance-core` must compile for native and `wasm32` targets and must not depend on LibRaw, Tauri, or the CLI. Everything RAW-specific lives in `colorbalance-raw` and is injected as a trait implementation.

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

- native CLI and Tauri use the LibRaw decoder;
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
- The desktop backend keeps the most recent decoded reference (and its preview) in `AppState`, so `load_reference`, `inspect_reference` and `derive_profile` on the same file share one decode. An entry is valid only for the same canonical path, byte length and modification time; failures and entries over 1 GiB are never cached; the image is shared as an `Arc` with no pixel copy. Batch and `correct_image` never use it. Stage labels read "Using cached image" / "Using cached preview" on a hit.

## Performance design

### Memory budget

A 45-megapixel RGB image in 32-bit float is about 540 MB before demosaicing buffers, masks, and output. The engine therefore allocates and processes image buffers inside Rust and never copies them into JavaScript. The UI receives paths, measurements, and previews.

### Tiling

Everything after decode is per-pixel independent and runs in tiles or scanlines:

```text
decode tile -> normalize -> channel scaling -> 3x3 matrix -> clip and count -> sRGB encode -> write
```

Demosaicing may require the full frame and stays inside LibRaw initially. The pipeline after decoding must not allocate additional full-size copies.

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
                     -> native Rust workers with LibRaw
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
| D5 | Browser execution is a measured experiment gated on the LibRaw WebAssembly spike | Accepted | 2026-10-03 |
| D6 | Hosted mode is optional and deferred until upload cost and demand are known | Accepted | 2026-10-03 |
| D7 | CPU paths first; WebGPU is optional acceleration, never required | Accepted | 2026-10-03 |
| D8 | Profile format is `*.cbprofile.json` regardless of deployment mode | Accepted | 2026-10-03 |
| D9 | CLF and `.cube` exports require the normalized camera-RGB decode contract and do not replace it | Accepted | 2026-10-03 |
| D10 | Engine ships a built-in pure-Rust DNG decoder (uncompressed 16-bit CFA); LibRaw FFI remains planned for other camera formats and must agree with the built-in decoder on overlapping DNGs | Accepted | 2026-10-03 |
| D11 | Web delivery mode: Tauri 2 desktop is the primary delivery vehicle. Standalone browser preview is supported with local client-side evaluation and simulated demo fixtures. Direct browser-local RAW decode is gated behind WebAssembly memory constraints. Native desktop has unrestricted local filesystem access and multi-threaded parallel batch execution. | Accepted | 2026-10-04 |
| D12 | The built-in DNG decoder also accepts 3-component, 12-bit LinearRaw compressed with lossless JPEG SOF3, when metadata and opcodes preserve unbalanced linear camera RGB. CFA and LinearRaw remain distinct sensor layouts; clipping is flagged from source samples before orientation or preview. Full-frame crop and identity gain maps are allowed, other pixel-changing operations fail closed. LibRaw remains planned for other formats. | Accepted | 2026-10-06 |
| D13 | Desktop commands never run RAW or image work on the main thread: async commands plus `spawn_blocking`, with progress events for UI feedback | Accepted | 2026-10-07 |
| D14 | Desktop caches one decoded reference keyed on path, length and mtime, for load, inspect and derive only. Colour results are unchanged: a hit returns the identical decode, and any file change is a miss | Accepted | 2026-10-07 |

## Platform and Browser Support Matrix

| Platform / Environment | Tier | Capabilities | Notes |
| --- | --- | --- | --- |
| Windows 10/11 x64 (Tauri Desktop) | Tier 1 | Native RAW decode (DNG), rendered image decode (JPEG/PNG), multi-threaded batch, native dialogs, native window drag-and-drop, 16-bit TIFF output | Primary release target |
| macOS 12+ x64 / Apple Silicon (Tauri Desktop) | Tier 1 | Full native feature parity with Windows | Native build target |
| Linux x64 (Tauri Desktop / WebKitGTK) | Tier 1 | Full native feature parity with Windows | Native build target |
| Chrome / Edge (Browser Preview) | Tier 2 | Interactive light-table UI, 4-corner warp alignment, simulation demo, client-side preview rendering | Requires File System Access API for directory writes |
| Firefox / Safari (Browser Preview) | Tier 2 | Interactive light-table UI, 4-corner warp alignment, simulation demo | File upload fallback for single-image inspection |

Earlier planning proposed Python with PySide6. Decisions D2 through D4 supersede that stack because browser delivery became a product requirement. The milestone structure, color pipeline, quality gates, and interchange rules in `IMPLEMENTATION_PLAN.md` are unchanged.
