# ColorBalance implementation plan

## Product goal

ColorBalance creates a reusable color transform from one X-Rite or Calibrite ColorChecker Classic reference frame, then applies that transform to a batch captured with the same camera and lighting setup.

The first release should solve one narrow workflow well:

1. Load a RAW reference frame containing a 24-patch ColorChecker Classic.
2. Detect the chart, with manual four-corner selection as a fallback.
3. Reject a bad reference before fitting if patches have RAW photosite clipping, insufficient size, excessive within-patch variation, or a detectable clipped or non-uniform reflection.
4. Fit and validate a transform from deterministic linear, unbalanced camera RGB to linear sRGB D65 using a named chart dataset.
5. Save the transform and a human-readable quality report.
6. Apply the fixed transform to RAW files from the same capture setup. Do not infer exposure or white balance from arbitrary scene content.
7. Write 16-bit TIFF output and a documented transform for hosts that can reproduce the same decoded camera-RGB input.

Rendered TIFF and JPEG input can follow once the RAW path is measured and stable. Fitting a profile against camera-processed JPEG is useful, but it is less predictable because tone curves, white balance, local processing, and gamut clipping have already changed the patch values.

## Product constraints

A profile is valid only when these stay fixed:

- camera body and capture mode;
- illumination spectrum and lighting geometry;
- RAW development settings;
- any filter that changes spectral response.
Exposure must also stay fixed in the first release. A profile stores the scalar derived from its reference and applies that same scalar to every batch image. A later workflow may accept an explicit per-image exposure offset from the user or matched capture metadata. It must never infer a neutral or brightness adjustment from arbitrary scene pixels.
The first release rejects capture-exposure mismatch unless the user supplies an explicit stop offset. The tool cannot correct mixed or spatially varying light, undetectable smooth glare, clipped channels, a chart that occupies too few pixels, or a different camera response. It should report detectable cases rather than produce a confident-looking bad profile.

## Platform and technology decision

Build a local, cross-platform desktop application with a reusable Python core. Deliver the core and command-line interface first, then put a PySide6 interface over the same use cases.

Python is the practical choice here. The maintained libraries for ColorChecker detection, color-science calculations, RAW decoding, and OpenColorIO already expose NumPy arrays. A Rust or C++ implementation would spend the first releases rebuilding or binding those parts without improving the fitting model. Keep pixel operations vectorized or in native libraries so Python does not run per-pixel loops.

Target Python 3.12 on Windows, macOS, and Linux. Windows is the first packaged target. Build macOS and Linux packages after the reference workflow passes on those systems.

### Proposed stack

| Concern | Library or tool | Reason |
| --- | --- | --- |
| Numeric arrays and fitting | NumPy and SciPy | Stable least-squares, robust optimization, native vectorized operations |
| Color models, chart datasets, Delta E | `colour-science` | Published chart datasets and tested color conversions and correction algorithms |
| Chart detection | `colour-checker-detection` with OpenCV | Supports ColorChecker Classic segmentation, templated detection, and inference APIs |
| RAW decode | `rawpy` and LibRaw | Broad camera support and explicit controls for white balance, gamma, bit depth, and automatic brightening |
| TIFF/JPEG I/O and metadata | OpenImageIO | 16-bit and floating-point image I/O, metadata access, and native image operations |
| Interchange transform | OpenColorIO Python bindings | Read, write, apply, and validate CLF transforms; bake `.cube` files when needed |
| CLI | Typer | Typed subcommands and usable help without a custom parser layer |
| Desktop UI | PySide6 | One Python process, native desktop widgets, worker threads, and supported packaging tools |
| Models and serialization | Standard-library dataclasses plus JSON Schema | Versioned files without a runtime model framework |
| Tests and quality | pytest, Ruff, mypy | Small, conventional Python toolchain |
| Packaging | `uv` for development, `pyside6-deploy` or Nuitka for desktop builds | Locked Python dependencies and standalone desktop binaries |
| CI and releases | GitHub Actions | Test matrix, fixtures, packaged artifacts, and checksums |

Do not start with a web application. Browser RAW support and multi-gigabyte batch upload make the workflow worse, while a server adds storage, privacy, and operating cost. Do not split the first release into a TypeScript UI and Python service. Qt keeps the UI and image engine in one process and removes a packaging and IPC boundary.

## Color pipeline

### 1. Decode deterministically

Decode the chart and every batch image with the same settings. Require rawpy `output_color=raw`, `gamma=(1, 1)`, unity `user_wb`, `no_auto_bright=True`, a fixed demosaic method, a fixed highlight policy, fixed scaling behavior, and 16-bit output. Subtract recorded per-channel black levels, normalize against recorded per-channel saturation levels, and preserve the as-shot orientation. Record the LibRaw version, camera make and model, every decoder setting, white and black levels, and channel layout in the profile.

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

The apply path verifies camera identity, decoder settings, and capture exposure metadata before processing. Decode, apply the stored exposure scalar and neutral channel scaling unchanged, apply the fitted matrix, clip linear sRGB to `[0, 1]` for integer TIFF, count and report low and high clipped pixels, then encode sRGB. A user may supply an explicit stop offset for intentional exposure differences. The tool must not derive one from scene content. A force flag may override camera or metadata mismatch, but the report records it.

Default output is 16-bit TIFF in sRGB with an embedded ICC display profile. Preserve capture metadata where the output format supports it, but remove or rewrite tags that would falsely describe transformed pixel data. Never overwrite input files. Existing outputs fail or skip by default. Explicit overwrite writes a unique temporary file in the destination directory, closes and flushes it, then atomically replaces the destination. Never delete the existing destination before rename.

Parallelize by image with a bounded worker count. Keep only active images in memory. Cancellation stops scheduling new files and lets active writes finish or removes their temporary files.

## Interchange formats

### Canonical project profile

Use a versioned `*.cbprofile.json` file as the lossless project record. It contains the exact stages and coefficients used by ColorBalance, input and output color-space contracts, chart type and reference dataset, camera and decoder identity, fit settings, validation statistics, and a SHA-256 digest of the reference image. Publish a JSON Schema and reject unknown major versions.

A custom project file is needed because LUT formats do not carry enough provenance and validation data to decide whether a profile is safe for a batch.

### Common LUT Format

Export Academy Common LUT Format, `.clf`, as the primary transform interchange file. CLF is human-readable, self-contained for its listed pixel operations, supports matrices and 1D or 3D LUTs, and OpenColorIO can validate and apply it. For the matrix model, write the stored scalar, channel scaling, and color matrix as explicit CLF nodes. If a later nonlinear model cannot be represented exactly by those nodes, bake it to a documented 3D LUT and record approximation error.

CLF does not reproduce RAW decoding. This export accepts only normalized linear camera RGB produced with the profile's exact camera and decoder contract. `InputDescriptor` documents that contract but does not enforce it. Interoperability tests must feed the same normalized arrays into each host. General RAW-editor interoperability is deferred to DCP.

### Compatibility exports

- Export a 33-point `.cube` 3D LUT only for hosts that can supply the same normalized linear camera RGB. The format does not carry enough metadata to make it safe for ordinary rendered images or a raw editor. Measure the baked LUT against the exact transform and warn if its maximum error exceeds the published threshold.
- Treat DNG Camera Profile, `.dcp`, as a later feature. DCP is the right format for Adobe Camera Raw and Lightroom RAW workflows, but it requires camera-native color matrices, illuminant handling, and DNG-specific semantics. Relabeling the initial working-space transform as DCP would be wrong.
- Do not use ICC as the project format. Embed a standard output ICC profile in rendered files. Input ICC profiles are possible, but application support and camera-RAW semantics do not match this workflow as cleanly as DCP.
- An OCIO configuration is optional packaging around one or more CLF transforms. It is useful for VFX pipelines but too large as the profile itself.

## Proposed package structure

```text
src/colorbalance/
  application/       # derive and apply use cases
  calibration/       # patch sampling, fitting, validation
  color/             # transfer functions, spaces, Delta E adapters
  imageio/           # RAW and rendered-image adapters
  profiles/          # schema, migration, CLF and cube export
  cli.py
  gui/               # PySide6, added after CLI behavior is stable
tests/
  fixtures/          # licensed or generated small images and profile fixtures
  integration/
  unit/
docs/
  IMPLEMENTATION_PLAN.md
  capture-guide.md
  profile-format.md
```

Domain code should accept arrays and immutable metadata records. It should not import Typer or PySide6. CLI and GUI call the same `derive_profile` and `apply_profile` use cases. Decoder, detector, profile writer, and output writer are explicit adapters so tests can exercise behavior without mocking the color math.

## Command-line contract

```text
colorbalance inspect reference.CR3
colorbalance derive reference.CR3 --chart classic-24 --profile studio.cbprofile.json --report studio-report.html
colorbalance apply studio.cbprofile.json ./shoot --output ./balanced --format tiff
colorbalance export studio.cbprofile.json --format clf --output studio.clf
colorbalance export studio.cbprofile.json --format cube --size 33 --output studio.cube
```

`derive` exits nonzero when quality gates fail unless the user explicitly records an override. `apply` produces a machine-readable batch summary containing succeeded, skipped, and failed files. One corrupt input must not discard completed outputs.

## Milestones and issue plan

### Milestone 1: Measured calibration core

Exit criterion: a repeatable command derives a validated profile from a supported ColorChecker Classic RAW fixture.

1. **Bootstrap the Python package and CI**
   - Add `pyproject.toml`, `src` layout, CLI entry point, Ruff, mypy, pytest, dependency lock, and Windows/macOS/Linux CI.
   - Document supported Python versions and native dependency installation.
   - Acceptance: clean checkout installs; CLI help runs; CI executes one behavioral smoke command on all three operating systems.

2. **Define licensed reference fixtures and numerical baselines**
   - Select redistributable ColorChecker RAW fixtures for at least two camera models, plus rejected examples with sparse single-channel RAW clipping and detectable high-variance or clipped reflections.
   - Record fixture licenses, physical chart revision, named dataset and version, expected patch coordinates or manual corners, and independent numerical values for at least one non-neutral D50-to-D65 target conversion.
   - Acceptance: tests obtain fixtures reproducibly; an unspecified or unsupported chart revision fails; fixed target RGB and Lab values match an independent calculation rather than values generated by the implementation under test.

3. **Implement deterministic RAW decoding**
   - Wrap rawpy with `output_color=raw`, `gamma=(1, 1)`, unity `user_wb`, disabled auto brightening, fixed demosaic, highlight, scaling, and 16-bit settings.
   - Return normalized linear camera RGB, camera identity, orientation, black and saturation levels, channel layout, complete decode parameters, and pre-demosaic per-channel saturation masks.
   - Acceptance: fixed RAW samples yield expected numeric channel values and masks; repeated decode is identical; sparse source clipping remains flagged after mask mapping; unsupported sensor layouts return a specific error.

4. **Detect ColorChecker Classic and support manual corners**
   - Integrate deterministic chart detection, chart orientation, perspective warp, four-corner fallback input, and explicit chart-revision selection.
   - Save a diagnostic overlay.
   - Acceptance: supported fixtures produce 24 ordered patch regions; a detection miss requests corners rather than guessing; no chart dataset is selected silently.

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
   - Verify the decode contract and exposure metadata, apply the stored scalar and channel scaling without scene analysis, apply the exact transform, clip linear sRGB to `[0, 1]`, count clipped pixels, and encode sRGB output.
   - Acceptance: known synthetic pixels, including negative and over-range results, reproduce exact expected 16-bit encoded values; camera or exposure mismatch fails unless forced or given an explicit stop offset.

10. **Write 16-bit TIFF output with valid metadata**
    - Use a same-directory unique temporary file, close and flush before atomic rename, embed sRGB ICC, normalize orientation, and copy only reviewed EXIF fields. Existing outputs fail or skip unless overwrite is explicit. Do not use delete-then-rename.
    - Acceptance: an external metadata reader identifies 16-bit TIFF and embedded sRGB; inputs remain byte-identical; cancellation and injected write or commit failures leave no partial final file; an existing output remains byte-identical when replacement fails.

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
    - Explain fixed exposure, chart revision selection, even lighting and detectable-reflection limits, profile validity, the normalized camera-RGB CLF and `.cube` input contract, and why general RAW-editor support requires the deferred DCP work.
    - Acceptance: a new user can capture, derive, apply, and verify a profile using only released binaries and the documentation; examples never imply that CLF or `.cube` files decode ordinary RAW or accept rendered sRGB input.

### Milestone 4: Desktop release

Exit criterion: a non-developer can finish the measured workflow using a signed Windows package, with macOS and Linux packages following after platform smoke tests.

17. **Build the PySide6 reference and batch workflow**
    - Add reference selection, chart overlay, editable corners, quality results, profile save, batch selection, output settings, and completion summary.
    - Acceptance: the UI calls the same application use cases as the CLI; no color calculation lives in widget code; the complete fixture workflow runs without a terminal.

18. **Add responsive progress, cancellation, and recoverable errors**
    - Run decode and writes off the UI thread, expose per-file progress, confirm destructive overwrite choices, and preserve completed outputs on cancellation.
    - Acceptance: the window remains responsive during the fixture batch; cancellation has the same cleanup guarantees as the CLI.

19. **Package and release the desktop application**
    - Build reproducible Windows artifacts first, include native dependencies and licenses, generate checksums, and run the packaged smoke workflow. Add macOS and Linux artifacts after equivalent checks pass.
    - Acceptance: the package runs on a clean machine without Python; the installed application derives and applies the fixture profile; release notes state supported cameras, formats, and known limits.

20. **Run a usability and color-quality release gate**
    - Test fresh captures from at least three camera models and two illuminant types with users who did not write the application.
    - Acceptance: publish anonymized task failures and measured Delta E; fix every data-loss, silent-misprofile, and release-threshold failure before 1.0.

## Deferred work

- DCP generation for Lightroom and Adobe Camera Raw;
- dual-illuminant camera profiles;
- ColorChecker Digital SG, Passport Video, and third-party chart definitions;
- rendered JPEG and TIFF reference fitting;
- lens shading and spatial illumination correction;
- GPU processing;
- network or cloud operation;
- plug-ins for editing applications.

Each deferred item needs measurements or a user workflow that justifies its cost. None belongs in the first end-to-end release.

## Sources behind the decisions

- ACES Common LUT Format specification: https://docs.acescentral.com/clf/specification
- OpenColorIO tools and CLF guidance: https://opencolorio.readthedocs.io/en/latest/guides/using_ocio/using_ocio.html
- Colour Checker Detection APIs: https://colour-checker-detection.readthedocs.io/en/develop/colour_checker_detection.detection.html
- Colour correction API and supported fitting methods: https://colour.readthedocs.io/en/develop/generated/colour.colour_correction.html
- rawpy post-processing controls: https://letmaik.github.io/rawpy/api/rawpy.Params.html
- Adobe DNG 1.7.1 specification: https://helpx.adobe.com/content/dam/help/en/camera-raw/digital-negative/jcr_content/root/content/flex/items/position/position-par/download_section_733958301/download-1/DNG_Spec_1_7_1_0.pdf
- Qt for Python deployment: https://doc.qt.io/qtforpython-6/deployment/deployment-pyside6-deploy.html
