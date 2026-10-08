# ColorBalance implementation plan

## Product goal

ColorBalance creates a reusable color transform from one X-Rite or Calibrite ColorChecker Classic reference frame, then applies that transform to a batch captured with the same camera and lighting setup.

The first release should solve one narrow workflow well:

1. Load a RAW reference frame containing a 24-patch ColorChecker Classic.
2. Detect the chart from a bounded thumbnail when the reference is small enough, or run detection on request; keep manual four-corner selection available for misses and ambiguous results.
3. Reject a bad reference before fitting if patches have RAW photosite clipping, insufficient size, excessive within-patch variation, or a detectable clipped or non-uniform reflection.
4. Fit and validate a transform from deterministic linear, unbalanced camera RGB to linear sRGB D65 using a named chart dataset.
5. Save the transform and a human-readable quality report.
6. Apply the fixed transform to RAW files from the same capture setup. Do not infer exposure or white balance from arbitrary scene content.
7. Write 16-bit TIFF output and a documented transform for hosts that can reproduce the same decoded camera-RGB input.

Rendered JPEG and PNG input can be processed using the `--quick-and-dirty` approximation mode. For camera-processed sources, sRGB gamma is inverted to approximate linear values, and quality gates are relaxed (min 16 pixels per patch, max CV 0.25, unconstrained neutral row) while the HTML report and profile record the approximation. Rendered TIFF input is not supported.

## Product constraints

A profile is valid only when these stay fixed:

- camera body and capture mode;
- illumination spectrum and lighting geometry;
- RAW development settings;
- any filter that changes spectral response.
Exposure must also stay fixed in the first release. A profile stores the scalar derived from its reference and applies that same scalar to every batch image. A later workflow may accept an explicit per-image exposure offset from the user or matched capture metadata. It must never infer a neutral or brightness adjustment from arbitrary scene pixels.
The first release rejects capture-exposure mismatch unless the user supplies an explicit stop offset. The tool cannot correct mixed or spatially varying light, undetectable smooth glare, clipped channels, a chart that occupies too few pixels, or a different camera response. It should report detectable cases rather than produce a confident-looking bad profile.

## Current status (2026-10-08)

The milestone text below is the original plan and its acceptance criteria. This section records what the repository does today.

| Milestone | Issues | State |
| --- | --- | --- |
| 1. Measured calibration core | 1 to 7 | Closed. Decoding uses the `rawler` crate plus an in-repo AHD demosaic, not LibRaw (D18, which supersedes D10). Chart detection is pure Rust (D16) |
| 2. Safe batch workflow | 8 to 12 | Closed |
| 3. Interchange and independent validation | 13 to 16 | Closed. See `docs/model-selection-and-tolerances.md` |
| 4. Desktop release | 17 to 20 | Closed. Tauri 2 app builds and runs; release gate report in `docs/release-gate-1.0.md`. An unsigned Windows x64 NSIS installer and executables are published as the v0.1.1 pre-release; there is no code signing or clean-machine verification |
| 5. Web-capable platform | 21 to 26 | Issue 26 (delivery decision, D11) closed. Issues 21 to 25 open. The core builds for `wasm32` in CI; nothing else in this milestone is built |

Differences from the plan text that readers keep tripping over:

- LibRaw is not used. `rawler` decodes RAW files (D18); the AHD demosaic is in `colorbalance-core`. Only synthetic DNGs are verified so far (uncompressed 16-bit CFA, 12-bit LinearRaw in SOF3 lossless JPEG). Other camera formats are rawler's claim, not tested here.
- JPEG and PNG references work through `--quick-and-dirty` (CLI) or automatic detection of a rendered file (desktop). The profile and report flag the result as approximate.
- The CLI does not run the chart detector. Without `--quad` it samples an 8% inset rectangle. The desktop app runs the detector automatically through 12 MP and on request above that.
- `colorbalance derive --chart` takes `classic-before-nov-2014` or `classic-from-nov-2014`. The CLI defaults to `classic-from-nov-2014`; the desktop app requires an explicit choice before deriving.
- The desktop Library tab (D26) is an enhancement to milestone 4, not a reopening of issues 17–20. It scans one folder of digest-checked profile entries, saves a corrected reference preview and capture metadata, and applies a selected entry to a batch. Library-only camera or decode-contract mismatches proceed with warnings in the batch report; other paths stay fail-closed. GPS is stored by default with an opt-out when saving.

## Platform and technology decision

Build a Rust color engine and deliver it through a command-line interface, a Tauri 2 desktop application with a React UI, and later a browser or hosted mode. The full platform and performance record, including deployment modes and the decision log, lives in `docs/ARCHITECTURE.md`.

Rust is the choice because one core compiles to native desktop, WebAssembly for the browser, and server workers. The product needs local high-performance RAW processing and a credible web path, and shipping two color implementations would let results drift between platforms. Python remains the research and verification stack for independent numerical fixtures. It is never a runtime dependency.

Target 64-bit Windows, macOS, and Linux. Windows is the first packaged target. Build macOS and Linux packages after the reference workflow passes on those systems.

### Proposed stack

| Concern | Library or tool | Reason |
| --- | --- | --- |
| Color engine and fitting | Rust with `nalgebra` | One core for native, WebAssembly, and server; least-squares fitting without a hand-rolled solver |
| RAW decode | `rawler` crate, then AHD demosaic in `colorbalance-core` | Maintained multi-format decoder instead of a hand-written one; no C library or FFI. LGPL-2.1 (see D18) |
| Chart detection | Pure Rust in `colorbalance-core`, using a bounded thumbnail | Avoids OpenCV and bounds the search after thumbnail sampling; uncertain results require manual corners |
| Parallelism | `rayon` | Native per-image and per-tile CPU parallelism |
| TIFF/JPEG output and metadata | In-repo TIFF/EXIF writers, pure-Rust `jpeg-encoder` and `kamadak-exif` | 16-bit TIFF master or 8-bit JPEG, matching ICC, reviewed EXIF; optional XMP/IPTC and GPS removal (D25) |
| Serialization and profiles | `serde`, `serde_json`, `jsonschema` crate | Versioned `*.cbprofile.json` with schema validation |
| Interchange transform | CLF written by the engine, validated with OpenColorIO | Open, checkable interchange for the transform stages |
| CLI | `clap` | Typed subcommands matching the documented contract |
| Desktop shell | Tauri 2 | Web UI in the system webview, native engine, small install size |
| Web UI | React, TypeScript, Vite | One UI for desktop and browser without a server-rendered framework |
| Browser compute | Rust compiled to WebAssembly in Web Workers | Keeps pixel buffers out of JavaScript |
| Hosted mode, if built | Axum, PostgreSQL, S3-compatible storage | Native workers behind an API; image bytes never pass through the API server |
| Research verification | Python with `colour-science` and `colour-checker-detection` | Independent fixtures and formula cross-checks; non-runtime |
| Tests and quality | `cargo test`, `cargo clippy`, `cargo fmt`, GitHub Actions | Conventional Rust toolchain with a three-operating-system matrix |
| Packaging | Cargo release builds | Standalone desktop binaries with native dependencies. Installer bundling is not set up |

Do not ship the first release as a browser-only application. Browser RAW decoding, memory limits, and output writing are measured in milestone 5 before the browser mode is selected. Do not add a hosted server before local modes work end to end, because uploads of RAW batches and image privacy change the product. The React UI is shared by Tauri and the browser so the web path does not require a rewrite.

## Color pipeline

### 1. Decode deterministically

Decode the chart and every batch image with the same settings. The contract fixes raw colorimetry (`output_color` raw), linear gamma, unity `user_mul`, no camera or auto white balance, no auto brightening, the AHD demosaic, clipped highlights, no auto scaling, 16-bit depth, and as-shot orientation. Subtract recorded per-channel black levels, normalize against recorded per-channel saturation levels, and preserve the as-shot orientation. Record the decoder name (`rawler-ahd`), the rawler version, camera make and model, every decoder setting, white and black levels, and channel layout in the profile. Apply stops when the decoder name or the settings differ from the profile; `--force` overrides and the override is recorded. A version-only difference is a warning.

The decoder must also return per-channel photosite saturation masks before demosaicing. The implementation must prove how each supported camera maps RAW channels and masks into the three-channel working array. Unsupported four-color or unusual sensor layouts must fail explicitly in the first release.

### 2. Find and sample the chart

Run the non-ML templated or segmentation detector first. It has a smaller package and deterministic behavior. If detection fails or finds several candidates, ask for four corners. Perspective-warp the chart to a fixed grid, then sample the central area of each patch. Use a robust statistic such as a trimmed mean or median and retain within-patch variance.

Quality gates should include:

- minimum patch dimensions after warping;
- no photosite at or above the defined per-channel RAW clipping threshold inside a sampled patch;
- neutral-patch monotonicity;
- maximum within-patch variation;
- detectable reflections limited to clipped or spatially non-uniform patch samples;
- plausible orientation based on chart patch ordering.

The UI and CLI report must show the detected quadrilateral, sampled regions, and saturation mask. Silent automatic detection is not acceptable for profile creation. A single Classic chart cannot measure spatial illumination falloff because its patches have different reflectances. The capture guide must require even lighting; quantitative falloff correction is deferred until the workflow accepts a uniform-field capture.

### 3. Set exposure and white balance

Estimate one exposure scalar and neutral channel scaling from the chart's neutral row without clipping. Store both in the profile and apply them unchanged to every batch image. Do not estimate either value from chart-free batch images. Keep these stages explicit so the report can distinguish capture-lighting error from camera color error.

### 4. Fit and select a transform

Implement a constrained 3x3 matrix first. Preserve black by omitting an additive offset. Require the user to select the physical chart revision because the 24-patch layout does not identify it reliably. Pin the chart dataset identifier and data version, reference illuminant, 2-degree observer, and Bradford chromatic adaptation from the dataset white to D65. Convert the adapted targets to linear sRGB D65, fit in linear RGB, then evaluate corrected and target values in CIE Lab under D65 with Delta E 2000. An unspecified or unsupported chart revision must fail.

Compare candidate models by leave-one-patch-out or grouped cross-validation before adding complexity. A root-polynomial model may lower training error but can behave badly outside the 24 measured colors. Add it only when held-out results and out-of-gamut tests beat the matrix. Never select a model from training Delta E alone.

Store these validation values:

- mean, median, 95th percentile, and maximum Delta E 2000;
- values for neutral patches and skin-tone patches;
- patch clipping and variance flags;
- matrix condition number;
- held-out error when model selection is enabled;
- source and corrected patch values.

### 5. Apply without changing the contract

The apply path verifies camera identity, decoder settings, and capture exposure metadata before processing. Decode, apply the stored exposure scalar and neutral channel scaling unchanged, apply the fitted matrix, then for sRGB clip linear to `[0, 1]`, count and report clipped pixels, and encode sRGB. For Display P3 and Adobe RGB, convert the unclamped linear Rec.709 result with OCIO, clip in the target space, count, and quantize (D24). A user may supply an explicit stop offset for intentional exposure differences. The tool must not derive one from scene content. A force flag may override camera or metadata mismatch, but the report records it.

Default output is 16-bit TIFF in sRGB with an embedded ICC profile; `--format jpeg` adds 8-bit JPEG at quality 95 and 4:4:4 chroma by default, with user-controlled quality and subsampling. `--output-space` selects Display P3 or Adobe RGB (1998) for either format, and each embeds the matching ICC. Quantize after target-space clipping. Copy reviewed capture fields when present; optional XMP/IPTC copying and GPS removal are explicit. Remove stale orientation, thumbnails, ColorSpace, white-balance fields and maker notes. Missing metadata does not fail export; the report says what was copied or skipped. Never overwrite input files. Existing outputs fail or skip by default. Explicit overwrite writes a unique temporary file in the destination directory, closes and flushes it, then atomically replaces the destination. Never delete the existing destination before rename.

Parallelize by image with a bounded worker count. Keep only active images in memory. Cancellation stops scheduling new files and lets active writes finish or removes their temporary files.

## Interchange formats

### Canonical project profile

Use a versioned `*.cbprofile.json` file as the lossless project record. It contains the exact stages and coefficients used by ColorBalance, input and output color-space contracts, chart type and reference dataset, camera and decoder identity, fit settings, validation statistics, and a SHA-256 digest of the reference image. Publish a JSON Schema and reject unknown major versions.

A custom project file is needed because LUT formats do not carry enough provenance and validation data to decide whether a profile is safe for a batch.

### Common LUT Format

Export Academy Common LUT Format, `.clf`, as the primary transform interchange file. CLF is human-readable, self-contained for its listed pixel operations, supports matrices and 1D or 3D LUTs, and OpenColorIO can validate and apply it. For the matrix model, write the stored scalar, channel scaling, and color matrix as explicit CLF nodes. If a later nonlinear model cannot be represented exactly by those nodes, bake it to a documented 3D LUT and record approximation error.

CLF does not reproduce RAW decoding. This export accepts only normalized linear camera RGB produced with the profile's exact camera and decoder contract. `InputDescriptor` documents that contract but does not enforce it. Interoperability tests must feed the same normalized arrays into each host. A RAW-only, matrix-only DCP export now exists (D28), but its Lightroom/Camera Raw round trip is untested.

### Compatibility exports

- Export a 33-point `.cube` 3D LUT only for hosts that can supply the same normalized linear camera RGB. The format does not carry enough metadata to make it safe for ordinary rendered images or a raw editor. Measure the baked LUT against the exact transform and warn if its maximum error exceeds the published threshold.
- DNG Camera Profile, `.dcp`, is implemented as a RAW-only, matrix-only, single-illuminant export (D28). Relabeling the working-space transform as a full Adobe profile would be wrong, so tone curves, look tables and dual-illuminant data stay out. The mapping and phases are in `LIGHTROOM_EXPORT_PLAN.md`; Lightroom compatibility is untested.
- Do not use ICC as the project format. Embed a standard output ICC profile in rendered files. Input ICC profiles are possible, but application support and camera-RAW semantics do not match this workflow as cleanly as DCP.
- An OCIO configuration is optional packaging around one or more CLF transforms. It is useful for VFX pipelines but too large as the profile itself.

## Package structure

```text
crates/
  colorbalance-core/      # color math, chart sampling and detection, fitting, profiles, quality gates
  colorbalance-raw/       # rawler adapter, JPEG/PNG loading, test-only DNG writer (native only)
  colorbalance-cli/       # clap CLI (native only)
  colorbalance-fixtures/  # shared test fixtures
apps/
  desktop/                # Tauri 2 shell (src-tauri) and React UI (frontend)
  web/                    # browser experiment (milestone 5), not started
research/                 # Python verification notebooks and fixture generators (non-runtime)
tests/                    # cross-crate integration tests and fixtures
docs/                     # implementation plan, architecture, decision log
```

`colorbalance-core` exposes operations such as `inspect_reference`, `derive_profile`, `apply_profile`, `export_clf`, and `export_cube`, and accepts a decoder implementation as a trait. It must compile for native and `wasm32` targets and must not depend on rawler, Tauri, or the CLI. The native CLI and desktop app inject the rawler decoder. Tests inject synthetic and fixture decoders. No color calculation lives in TypeScript.

## Command-line contract

```text
colorbalance inspect reference.dng --chart classic-from-nov-2014 --quad x1,y1,x2,y2,x3,y3,x4,y4
colorbalance derive reference.dng --chart classic-from-nov-2014 --quad x1,y1,x2,y2,x3,y3,x4,y4 --profile studio.cbprofile.json --report studio-report.html
colorbalance apply studio.cbprofile.json ./shoot --output ./balanced --format tiff
colorbalance export studio.cbprofile.json --format clf --output studio.clf
colorbalance export studio.cbprofile.json --format cube --size 33 --output studio.cube
colorbalance export studio.cbprofile.json --format dcp --camera-name "Camera model" --output studio.dcp
```

`derive` exits nonzero when quality gates fail unless the user explicitly records an override. `apply` produces a machine-readable batch summary containing succeeded, skipped, and failed files. One corrupt input must not discard completed outputs.

## Milestones and issue plan

### Milestone 1: Measured calibration core

Exit criterion: a repeatable command derives a validated profile from a supported ColorChecker Classic RAW fixture.

1. **Bootstrap the Rust workspace and CI**
   - Add the Cargo workspace with `colorbalance-core`, `colorbalance-raw`, and `colorbalance-cli`, a `clap` CLI entry point, rustfmt, clippy, and Windows/macOS/Linux CI.
   - Document the supported Rust toolchain version.
   - Acceptance: clean checkout builds; CLI help runs; `colorbalance-core` compiles for native and `wasm32`; CI executes one behavioral smoke command on all three operating systems.

2. **Define licensed reference fixtures and numerical baselines**
   - Select redistributable ColorChecker RAW fixtures for at least two camera models, plus rejected examples with sparse single-channel RAW clipping and detectable high-variance or clipped reflections.
   - Record fixture licenses, physical chart revision, named dataset and version, expected patch coordinates or manual corners, and independent numerical values for at least one non-neutral D50-to-D65 target conversion.
   - Acceptance: tests obtain fixtures reproducibly; an unspecified or unsupported chart revision fails; fixed target RGB and Lab values match an independent calculation rather than values generated by the implementation under test.

3. **Implement deterministic RAW decoding**
   - Decode through the `rawler` crate and the in-repo AHD demosaic with raw colorimetry, linear gamma, unity white balance multipliers, disabled auto brightening, fixed demosaic, highlight, scaling, and 16-bit settings behind a decoder trait. Decision D18 replaced the original LibRaw wrapper.
   - Return normalized linear camera RGB, camera identity, orientation, black and saturation levels, channel layout, complete decode parameters, and pre-demosaic per-channel saturation masks.
   - Acceptance: fixed RAW samples yield expected numeric channel values and masks; repeated decode is identical; sparse source clipping remains flagged after mask mapping; unsupported sensor layouts return a specific error.

4. **Detect ColorChecker Classic and support manual corners**
   - Use a pure-Rust, non-ML detector in `colorbalance-core` on an at-most-roughly-512-pixel-long-side thumbnail. Return a quadrilateral only for a sufficiently distinct 24-patch match; handle orientation and reasonable perspective. Keep four-corner manual input and the diagnostic overlay of the quad and sample regions.
   - In the desktop, run detection after the preview loads only when the decoded image is at most 12 megapixels (`width × height <= 12_000_000`). Provide an explicit Detect chart action for larger images and retries on every image. Reuse the cached decoded reference instead of decoding again.
   - A miss or ambiguous multiple-chart result leaves the corners for manual placement; never substitute a guessed quad. A detected quad does not identify the physical chart revision: the user still selects it explicitly before fitting.
   - Acceptance: supported fixtures produce 24 ordered patch regions; rotated and reasonably perspective-skewed fixtures are detected where the pattern is clear; weak and competing candidates fail closed. Report measured detection latency on named fixtures and hardware after implementation, including misses and large images. Thumbnail resolution, glare, occlusion, unusual charts and perspective can defeat this detector; manual corners remain the fallback.

5. **Sample patches and enforce reference quality gates**
   - Map patch polygons to the RAW saturation masks. Sample central patch areas with robust statistics and calculate RAW clipping, variance, patch size, neutral monotonicity, and detectable reflection flags.
   - Acceptance: good fixtures pass; sparse single-channel clipping is rejected even when demosaiced values hide it; each rejected reflection fixture reports the exact measurable reason and affected patches.

6. **Fit and validate the initial matrix profile**
   - Implement stored exposure and neutral channel scalars, a pinned chart dataset and observer, Bradford adaptation from its stated illuminant to D65, linear-sRGB target conversion, constrained 3x3 fitting, D65 Lab conversion, and Delta E 2000 metrics.
   - Acceptance: identity synthetic data returns identity within tolerance; the independent non-neutral reference target matches fixed D65 RGB and Lab values; held-out synthetic data detects an overfit candidate; reports include every metric listed in this plan.

7. **Implement `inspect` and `derive` commands with a quality report**
   - Compose decode, detection, sampling, fit, and validation into public use cases.
   - Produce JSON diagnostics and an HTML report with overlay, patch table, before and after colors, and warnings.
   - Acceptance: the reference fixture smoke run writes a profile and report; a bad reference exits nonzero without a profile unless override is recorded.

### Milestone 2: Safe batch workflow

Exit criterion: a profile applies to a directory of matching RAW files and produces restartable 16-bit TIFF output without touching inputs.

8. **Version the ColorBalance profile schema**
   - Publish JSON Schema for `*.cbprofile.json`, canonical serialization, profile digest, compatibility checks, and clear major-version errors.
   - Acceptance: round-trip preserves all coefficients and provenance; tampering changes the digest; unsupported major versions fail before decoding images.

9. **Implement profile application and color-space encoding**
   - Verify the decode contract and exposure metadata, apply the stored scalar and channel scaling without scene analysis, apply the exact transform, convert to the chosen output space (sRGB by default; Display P3 and Adobe RGB through OCIO, D24), clip, count clipped pixels, and encode.
   - Acceptance: known synthetic pixels, including negative and over-range results, reproduce exact expected 16-bit encoded values; camera or exposure mismatch fails unless forced or given an explicit stop offset.

10. **Write TIFF or JPEG output with valid metadata**
    - Use a same-directory unique temporary file, close and flush before atomic rename, embed a matching ICC, normalize orientation, and copy only reviewed EXIF fields. TIFF remains 16-bit; JPEG is 8-bit, with configurable quality and subsampling. Existing outputs fail or skip unless overwrite is explicit. Do not use delete-then-rename.
    - Acceptance: an external metadata reader identifies depth, ICC and retained safe capture fields in both formats; stale tags are absent; inputs remain byte-identical; cancellation and injected write or commit failures leave no partial final file; an existing output remains byte-identical when replacement fails.

11. **Add bounded parallel batch processing**
    - Implement recursive input selection, collision-safe output paths, worker limits, cancellation, resume behavior, existing-output policy, and JSON batch summary.
    - Acceptance: mixed good, corrupt, existing-output, and duplicate-name inputs produce deterministic outputs and per-file status; peak resident memory stays within the documented bound for the fixture batch.

12. **Implement the `apply` CLI and end-to-end RAW smoke workflow**
    - Expose batch options, progress, explicit stop offset, overwrite policy, force policy, and exit codes.
    - Acceptance: derive once and apply to a fixed-exposure fixture series; corrected chart validation improves over uncorrected input and every output opens in an independent reader.

### Milestone 3: Interchange and independent validation

Exit criterion: third-party OpenColorIO tools apply the exported transform with measured agreement to ColorBalance.

13. **Measure model choices and set release quality thresholds**
    - Compare the 3x3 model with root-polynomial candidates using held-out patches and separate captured scenes.
    - Publish the dataset, protocol, aggregate results, CLF and LUT error thresholds with metric and corpus, and the decision to keep or replace the default.
    - Acceptance: the decision uses held-out Delta E and behavior outside the chart samples, not training error; export tolerances have fixed numerical values before export work starts.

14. **Export and validate Common LUT Format**
    - Map exact profile stages to CLF, include explicit normalized-camera-RGB input and linear-sRGB output descriptors, and validate with OpenColorIO.
    - Acceptance: `ociochecklut` accepts the file; a second OpenColorIO host and ColorBalance agree within the published tolerance when fed the same normalized linear arrays.

15. **Export a compatibility `.cube` LUT**
    - Bake a configurable 3D LUT, define normalized camera-RGB domain and interpolation, and report approximation error.
    - Acceptance: an independent LUT reader loads the file; the published corpus stays under the fixed maximum error; export refuses a LUT size that misses the threshold unless forced.

16. **Publish capture, interoperability, and limitation documentation**
    - Explain fixed exposure, chart revision selection, even lighting and detectable-reflection limits, profile validity, the normalized camera-RGB CLF and `.cube` input contract, and the limited, untested-in-Lightroom DCP export.
    - Acceptance: a new user can capture, derive, apply, and verify a profile using only released binaries and the documentation; examples never imply that CLF or `.cube` files decode ordinary RAW or accept rendered sRGB input.

### Milestone 4: Desktop release

Exit criterion: a non-developer can finish the measured workflow using a signed Windows package, with macOS and Linux packages following after platform smoke tests.

17. **Build the React reference and batch workflow in Tauri**
    - Add reference selection, chart overlay, editable corners, quality results, profile save, batch selection, output settings, and completion summary in the shared React UI inside the Tauri shell.
    - Acceptance: the UI calls the same engine operations as the CLI; no color calculation lives in TypeScript; no full-resolution pixel buffer crosses the UI boundary; the complete fixture workflow runs without a terminal.

18. **Add responsive progress, cancellation, and recoverable errors**
    - Run decode and writes off the UI thread, expose per-file progress, confirm destructive overwrite choices, and preserve completed outputs on cancellation.
    - Acceptance: the window remains responsive during the fixture batch; cancellation has the same cleanup guarantees as the CLI.

19. **Package and release the desktop application**
    - Build reproducible Windows artifacts first, include native dependencies and licenses, generate checksums, and run the packaged smoke workflow. Add macOS and Linux artifacts after equivalent checks pass.
    - Acceptance: the package runs on a clean machine without a development environment; the installed application derives and applies the fixture profile; release notes state supported cameras, formats, and known limits.

20. **Run a usability and color-quality release gate**
    - Test fresh captures from at least three camera models and two illuminant types with users who did not write the application.
    - Acceptance: publish anonymized task failures and measured Delta E; fix every data-loss, silent-misprofile, and release-threshold failure before 1.0.

### Milestone 5: Web-capable platform

Exit criterion: the browser and hosted execution modes are measured against the native engine, and the delivery decision is recorded in the architecture decision log with evidence.

21. **Compile the color core to WebAssembly and verify numerical parity**
    - Build `colorbalance-core` for `wasm32` with wasm-bindgen, run it in a Web Worker, and compare fitting, Delta E, profile application, and export results against native output on the fixture corpus.
    - Acceptance: native and WebAssembly results agree within the stated tolerance for every fixture; SIMD and scalar variants both pass; the core path requires no WebAssembly threads.

22. **Run a browser RAW decode feasibility spike**
    - Build `rawler` (or the chosen decode path) for WebAssembly, decode the RAW fixtures in a worker, and compare pixels and saturation masks against native decoding while measuring bundle size, memory use, and decode time. The original plan named LibRaw; D18 moved decoding to rawler, which has not been built for `wasm32` here.
    - Acceptance: a written go or no-go record for browser RAW processing with numbers for every fixture camera; the decision is added to the architecture decision log.

23. **Implement browser file handling and job state**
    - Add feature-detected File System Access input and streamed output, file input plus download fallback, transferable buffer transport, and IndexedDB job state for recovery.
    - Acceptance: the fixture derive-and-apply workflow runs in Chrome and Edge without a server; Firefox and Safari behavior is documented; peak memory stays under the published ceiling.

24. **Prototype the hosted processing option**
    - Stand up an Axum API, PostgreSQL job state, presigned multipart uploads to S3-compatible storage, native Rust workers, and SSE progress.
    - Acceptance: a browser session derives and applies the fixture batch end to end; no image bytes pass through the API server; retention and deletion behavior is documented.

25. **Build the performance benchmark harness**
    - Measure native, WebAssembly, and hosted decode, transform, and encode times, memory ceilings, and tile throughput, with reproducible commands and a CI regression gate for the apply path.
    - Acceptance: benchmark commands run from a clean checkout; results are committed; a regression in the apply path fails CI.

26. **Decide and document the web delivery mode**
    - Choose browser-local processing, hosted workers, or desktop-only based on the recorded measurements, and publish the browser and operating system support matrix.
    - Acceptance: the decision record cites the spike and benchmark numbers; `docs/ARCHITECTURE.md` and the support matrix are updated; unsupported paths fail with clear messages.

## Deferred work

- DCP look tables, tone curves and dual-illuminant profiles;
- DCP round trip in Lightroom (plan phase 4);
- ColorChecker Digital SG, Passport Video, and third-party chart definitions;
- rendered JPEG and TIFF reference fitting;
- lens shading and spatial illumination correction;
- GPU processing;
- hosted web operation beyond the milestone 5 prototype;
- plug-ins for editing applications.

Each deferred item needs measurements or a user workflow that justifies its cost. None belongs in the first end-to-end release.

## Sources behind the decisions

- ACES Common LUT Format specification: https://docs.acescentral.com/clf/specification
- OpenColorIO tools and CLF guidance: https://opencolorio.readthedocs.io/en/latest/guides/using_ocio/using_ocio.html
- Colour Checker Detection APIs: https://colour-checker-detection.readthedocs.io/en/develop/colour_checker_detection.detection.html
- Colour correction API and supported fitting methods: https://colour.readthedocs.io/en/develop/generated/colour.colour_correction.html
- rawpy post-processing controls, used while the Python research stack verified decode settings: https://letmaik.github.io/rawpy/api/rawpy.Params.html
- Adobe DNG 1.7.1 specification: https://helpx.adobe.com/content/dam/help/en/camera-raw/digital-negative/jcr_content/root/content/flex/items/position/position-par/download_section_733958301/download-1/DNG_Spec_1_7_1_0.pdf
- LibRaw documentation and supported cameras (the original decoder choice, replaced by rawler in D18): https://www.libraw.org/docs
- Tauri 2 architecture: https://v2.tauri.app/
- WebAssembly and browser capability references: https://developer.mozilla.org/en-US/docs/WebAssembly and https://developer.mozilla.org/en-US/docs/Web/API/FileSystemFileHandle
