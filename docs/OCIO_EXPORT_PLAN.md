# OCIO output spaces: investigation and plan

Status: implemented in phases 0 to 5 under D24 in `ARCHITECTURE.md`; that record and `USAGE.md` are authoritative now. This file keeps the investigation notes and the measurements behind the choices. Phase 6 (CLF cross-check) was not done. Two measurements below were open when this was written and are now answered: `v4.0.0` lists encoded targets `sRGB Encoded P3-D65` and `Gamma 2.2 Encoded AdobeRGB`, and `PyOpenColorIO` failed to build with pip, so the independent reference is `colour` 0.4.4 instead.

## Goal

Let `apply` and the desktop export write images in other output color spaces, with OpenColorIO doing the color conversion. No matrices or transfer curves get written by hand for the new spaces.

OCIO only ever sees the output of our fitted transform, which is linear Rec.709 (sRGB primaries, D65). In the built-in OCIO configs that is the space named `Linear Rec.709 (sRGB)`. Camera RGB, the decode contract, exposure and the chart fit stay exactly as they are.

```text
camera RGB -> channel scale -> fitted matrix -> linear Rec.709 -> OCIO processor -> target space -> clamp, count -> 16-bit TIFF + ICC
              (core, unchanged)                                   (new)
```

## What I measured

Spikes ran in scratch crates under `%TEMP%` and are deleted. Nothing touched the repo.

| Question | Result |
| --- | --- |
| Which OCIO crates exist? | `ocio-rs` 0.2.1 (BSD-3, C++ OCIO 2.5.2 over FFI, about 5K downloads, published 2026-07-15) and `ocio` 0.1.0 (MIT AND BSD-3, a pure Rust port, 177 downloads, published 2026-10-04, README says "Work in progress"). `opencolorio` is a placeholder. |
| Does `ocio-rs` build by default? | It builds in **stub mode**: the build prints a warning and APIs "return safe defaults; no real color management". Real OCIO needs `--features bundled` or `OCIO_RS_ENABLE_REAL=1`. A build that forgets this would write unconverted pixels without an error. `ocio_rs::is_stub_build()` exists and must be checked. |
| Does `bundled` build on this Windows machine? | Not as is. CMake found no C++ compiler outside a Visual Studio developer shell. Inside one, the default static link failed with CRT conflicts (`LNK2005`, `LNK2038`, and unresolved `__imp_fread` with `+crt-static`). `OCIO_RS_LINK=dynamic` worked: about 1 minute, output `OpenColorIO_2_5.dll` (5.0 MB) plus `zlib.dll` beside the exe. Linux and macOS untested. |
| Does the C++ OCIO give right numbers? | Yes. Linear Rec.709 to ACEScg matched `colour` 0.4.4 (Bradford) within 7e-5 on four probe colors. P3-D65, Rec.2020 and the sRGB curve agree with `colour` to about 1e-4 or better. |
| Does the pure Rust `ocio` compile for `wasm32-unknown-unknown`? | Yes. `cargo build --release --target wasm32-unknown-unknown` succeeded. |
| Does it agree with C++ OCIO? | On 16 probe colors (4 per target: ACEScg, Rec.2020, P3-D65, `sRGB - Texture`) every printed channel agrees to within 1e-6, the last printed digit. The one visible difference: 1.0 maps to 1.000007 in C++ OCIO and to 1.000000 in the port. I did not find the cause. |
| Does OCIO clamp? | Not in the port. `sRGB - Texture` maps -0.05, 0.5, 1.2 to -0.646, 0.735, 1.083. I did not run this probe on C++ OCIO. Clipping stays our job. |
| Does OCIO write ICC profiles? | No. `FileFormatICC.cpp` has no write or bake code, and the port's `icc.rs` is a description reader. ICC embedding is a separate task. |
| Speed of the pure Rust port | 12.5 MP (3060x4080) `Linear Rec.709 (sRGB)` to `sRGB - Texture`: 531, 554, 588 ms per `apply_rgb_slice`. Matrix-only to P3-D65: 37 ms. I did not check whether `apply_rgb_slice` is single threaded, and I did not time C++ OCIO. |
| Built-in configs | 8 are listed. Config `v4.0.0_aces-v2.0_ocio-v2.5` is marked recommended and loaded in the port with 25 spaces. For `v1.0.0` (14 spaces) I printed 13 names; none is an encoded Display P3 or encoded Rec.2020 space. I have not listed the `v4.0.0` names, so which encoded targets exist there is open (phase 0). |

`PyOpenColorIO` is the intended fixture source below. `pip index` lists `opencolorio` 2.6.0; I did not install it, so whether it installs here is unverified. If it does not, the reference is C++ OCIO through an `ocio-rs` spike, which agreed with `colour` above.

## Backend decision (gate for phase 0)

| | `ocio` (pure Rust) | `ocio-rs` (C++ over FFI) |
| --- | --- | --- |
| wasm32 / browser mode | builds | cannot |
| Native toolchain | none | CMake, MSVC shell on Windows, 1 min build |
| Shipping | static | `OpenColorIO_2_5.dll` + `zlib.dll` in the Tauri bundle on Windows |
| Maturity | published 3 days before this plan, 177 downloads | about 5K downloads |
| Silent failure mode | none seen | stub mode |
| Fidelity to reference OCIO | 6-decimal agreement on 16 probes | is the reference |

Recommendation: pure Rust `ocio`, pinned `=0.1.0`, because invariant 7 wants one implementation across CLI, desktop and browser and only this one reaches wasm. Its immaturity is the risk, so phase 2 checks it against reference OCIO on fixtures, not on my 16 probe colors. If it fails those, fall back to `ocio-rs` behind the same seam. That path is native-only, so browser mode would offer sRGB only.

## Invariants touched

- **Invariant 7** says all color math lives in `colorbalance-core` and the crate must not link LibRaw, Tauri, CLI or UI crates. OCIO is a library, not one of those, but it is still a new math implementation. The decision record must amend invariant 7: output-space conversion is delegated to OCIO, called only from core, and no second implementation exists.
- **Invariant 8:** names in the UI and reports are the real output space. Exports that stay in linear camera RGB (CLF, `.cube`) are unchanged.
- **Clipping order** (`IMPLEMENTATION_PLAN.md` line 123 and 229): today linear sRGB is clipped to [0,1], then encoded. Clipping to sRGB first would throw away exactly the colors a wider target exists to keep. For a monotone curve with f(0)=0 and f(1)=1, clipping after conversion equals clipping before encoding, so one processor from linear Rec.709 to the encoded target, then a clamp in target space, is correct. For sRGB that is mathematically the same as today.
- **Clipped counts** are taken in the target space before the clamp, and need a tolerance. C++ OCIO returns 1.000007 for an exact 1.0. The tolerance comes from phase 2 measurements, not from a guess.

## Phases

Each phase is one GitHub issue. Acceptance means observed, not just green tests.

### 0. Decision record and spike gate

- Write D24 in `docs/ARCHITECTURE.md`: backend, pinned versions, pinned built-in config name, amended invariant 7, scope of "no hand-written color math for output spaces".
- Update `AGENTS.md` invariant 7 in the same change.
- List all space names in config `v4.0.0_aces-v2.0_ocio-v2.5` with the port. Decide the exact v1 targets from that list.
- Acceptance: D24 merged; the list of v1 targets exists with the OCIO space name for each; `cargo build -p colorbalance-core --target wasm32-unknown-unknown` passes with the dependency in.

### 1. Output transform seam in core

- Module `output_space` in `colorbalance-core`: an `OutputSpace` enum, its pinned OCIO name, and a function that builds the processor once per batch.
- Row-parallel apply behind the existing `parallel` feature (rayon off for wasm), clamp, and clipped counts in target space.
- Fail closed if the config, the source space `Linear Rec.709 (sRGB)` or the target name is missing. No stub fallback.
- Acceptance: unit tests on neutral axis, out-of-range inputs and name lookup failure. A 12.5 MP benchmark number, compared with the 531-588 ms figure above. Confirm the processor type can be shared across rayon threads; I did not check.

### 2. Independent fixtures

- `research/` script with `PyOpenColorIO` writes fixtures: a grid of linear Rec.709 inputs including negatives, values above 1, saturated primaries and a gray ramp, converted to each v1 target.
- Rust tests compare against them, and against the existing `srgb_encode` path for the sRGB target, to document the gap between the two. The tolerance is set from the measured maximum difference and written down in `docs/CONTRACTS.md` before export work uses it.
- Acceptance: all targets pass; the maximum observed difference is recorded; the measured difference between OCIO's sRGB and `srgb_encode`, in u16 steps, is recorded.

### 3. `apply` and report

- CLI option `--output-space` (default `srgb`). `srgb` stays on today's code path, so default output is unchanged bit for bit. Routing it through OCIO would move f32 values by about 1e-6 and could flip 16-bit LSBs, so that would be a separate decision.
- Batch report and TIFF description record the output space, OCIO implementation and version, and config name.
- Acceptance: run `apply` on the synthetic fixtures for each target and show the report. Existing-output, corrupt-input and cancellation failure injections still pass.

### 4. ICC embedding (also fixes an existing gap)

- `output.rs` writes no ICC tag today (grep for 34675 finds nothing in that file), although the plan promises "16-bit TIFF in sRGB with an embedded ICC display profile".
- OCIO does not help here. Choose profile bytes per target: shipped standard profiles (license to be checked for each) or generation with a pure Rust crate. `moxcms`, `lcms2` and `tintbox` exist on crates.io; I did not evaluate them.
- Acceptance: an independent reader (for example Pillow `ImageCms`) reports the right profile description and primaries for every target TIFF.

### 5. Desktop

- Output-space selector in export. The preview stays sRGB and says so.
- Acceptance: export a real file in each target, open it in a color-managed viewer and compare against the sRGB export. State what you looked at.

### 6. CLF cross-check (optional but cheap)

- Load our CLF export through OCIO `FileTransform` and compare against our own apply on random camera RGB. This is an independent check that invariant 8's export does what it says.
- Not verified: whether the pure Rust port reads CLF the way C++ OCIO does.

## Out of scope

- User-supplied OCIO configs. They break the reproducibility the profile and report promise. Revisit only with a pinned-config decision record.
- Scene-linear outputs such as ACEScg. A 16-bit integer TIFF is a poor container for them and needs float output (32-bit TIFF or EXR) first.
- Gamut mapping beyond clipping.
- GPU preview shaders.

## Risks

| Risk | Mitigation |
| --- | --- |
| Pure Rust `ocio` is 0.1.0 and may be wrong in places I did not probe | Phase 2 fixtures from reference OCIO; pin `=0.1.0`; keep the seam so the backend can be swapped |
| Stub mode if the C++ fallback is used | Check `is_stub_build()` at startup and fail closed |
| Config names change between built-in config versions | Pin the config name; test every name used |
| ~550 ms single-thread cost at 12.5 MP | Row-parallel apply; benchmark in phase 1 |
