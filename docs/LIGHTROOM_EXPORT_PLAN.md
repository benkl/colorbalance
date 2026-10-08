# Lightroom export: DCP camera profile

Status: phases 0 to 3 are implemented (`research/`, core writer, CLI, desktop). Phase 4, the Lightroom round trip, has not been run. Lightroom was never exercised, so nothing here claims it accepts the file. Decision record: D28 in `ARCHITECTURE.md`.

Sources are open only. The serializer is clean-room, written from the public DNG 1.6 specification plus open-source documentation and RawTherapee's reader behavior as facts. No Adobe DNG SDK code or parse logic is in this repository. Claims are tagged **verified** (read in a primary open source, or measured here) or **[INFERENCE]** (reasoned, or recalled from documentation not re-read in this work, not observed).

## Goal

Let a user take the transform fitted from one ColorChecker photo and use it in Adobe Lightroom and Camera Raw as a camera profile, so RAW files from the same camera get the same measured color there.

The vehicle is a DNG Camera Profile (`.dcp`). The `.cube` and CLF exports are not camera profiles.

Scope limit, stated in the UI and docs (invariant 8): a DCP maps camera-native RAW values to XYZ. It applies to RAW files only, and does nothing for JPEG or PNG. Profiles derived from a rendered reference (`quick_and_dirty`) are refused. The profile is matrix-only: no tone curve, no look table.

```text
ours:   RAW -> rawler-ahd decode -> / (exposure_scale * channel_scale) -> M -> linear Rec.709 (D65)

DCP:    RAW -> decode -> / ReferenceNeutral-style white balance -> ForwardMatrix -> XYZ D50
            -> [HueSatMap / LookTable] -> [ProfileToneCurve] -> working space -> output
```

The two chains agree algebraically at one white balance (see Mapping). The decode difference between rawler-ahd and Lightroom's own decode is the main unmeasured risk.

## What is verified

| Question | Result | Source |
| --- | --- | --- |
| File format | TIFF byte-order mark (`II`/`MM`), then `0x4352` instead of 42, then the IFD offset. Little-endian header bytes `49 49 52 43`. **Verified** (spec text). | DNG 1.6 spec, `ExtraCameraProfiles` |
| Tag IDs and types | Table below. **Verified** against the spec. | [DNG 1.6 PDF](http://www.paulbourke.net/dataformats/dng/dng_spec_1_6_0_0.pdf) |
| ForwardMatrix role | Maps white-balanced camera values to XYZ D50. The spec recommends it over ColorMatrix alone. **Verified.** | Spec chapter 6 |
| ColorMatrix role in a one-illuminant profile | Reader derives the white-balance neutral from a temperature/tint through it. **Verified** for RawTherapee's reader (looks up tags 50721, 50778, 50964, 51109). Lightroom's use is **[INFERENCE]**, as is the DCamProf description of it. | RawTherapee `dcp.cc` behavior |
| Single illuminant | A profile with only `ColorMatrix1` is legal under the spec. Lightroom ignoring the illuminant tag in that case is **[INFERENCE]** (recalled from DCamProf docs, page not re-read). | spec; DCamProf docs |
| Matrix identities | `FM·(1,1,1) = D50` and `CM·D50 = N` hold in the written file to about 4e-7 and 5e-5 relative. **Verified** by `research/read_dcp.py` on a real export. | measured |
| No tone curve | RawTherapee's reader adds Adobe's default curve only when `ProfileCopyright` contains "Adobe Systems". With no curve and no copyright it applies none. **Verified** (reader behavior, facts only). What Lightroom does is **[INFERENCE]**: it treats a missing curve as linear. Open item T. | RawTherapee `dcp.cc` behavior |
| `ProfileEmbedPolicy` | 0 allow copying, 1 embed if used, 2 embed never, 3 no restrictions. Policy 0 profiles "may not be used to process non-DNG files". **Verified.** | Spec |
| `BaselineExposureOffset` | Added to `BaselineExposure`, in EV. Spec type is RATIONAL; ExifTool's tag table lists SRATIONAL. The writer uses SRATIONAL so negative values work. **Unresolved:** whether Lightroom reads it as signed. Open item E. | Spec, ExifTool tag docs |
| LightSource codes | D50 = 23, D55 = 20, D65 = 21, A = 17, Other = 255. **Verified.** | Spec |
| Install path | Lightroom: File > Import Profiles & Presets. Windows folder `C:\ProgramData\Adobe\CameraRaw\CameraProfiles`, macOS `~/Library/Application Support/Adobe/CameraRaw/CameraProfiles`. **Verified** for Lightroom (cloud/desktop); Lightroom Classic's path was not checked. | [Adobe FAQ](https://helpx.adobe.com/lightroom/desktop/kb/faq-install-presets-profiles.html) |
| Camera name | The spec calls `UniqueCameraModel` "a unique, non-localized name" that "may be used to index into per-model preferences and replacement profiles". **Verified.** That Lightroom hides a profile whose name differs from its own camera name is **[INFERENCE]** (recalled from DCamProf docs). | spec |
| LUT/XMP route | No documented format. Reverse-engineered by third parties. **Out of scope.** | [xmp2cube](https://github.com/mvimercati/xmp2cube) |
| Validators here | Python `colour` 0.4.4 is installed. ExifTool, DCamProf, dng_validate, RawTherapee are not, and were not installed. | local check |

### Tag table (DNG 1.6 spec)

| Tag | Decimal | Type | Written |
| --- | --- | --- | --- |
| DNGVersion | 50706 | BYTE x4 | 1,6,0,0 |
| DNGBackwardVersion | 50707 | BYTE x4 | 1,4,0,0 |
| UniqueCameraModel | 50708 | ASCII | user-supplied, required |
| ColorMatrix1 | 50721 | SRATIONAL x9 | XYZ(D50) -> camera |
| CalibrationIlluminant1 | 50778 | SHORT | 23 (D50) |
| ProfileName | 50936 | ASCII | `ColorBalance <8 hex of profile digest>` |
| ProfileEmbedPolicy | 50941 | LONG | 0 |
| ForwardMatrix1 | 50964 | SRATIONAL x9 | white-balanced camera -> XYZ D50 |
| BaselineExposureOffset | 51109 | SRATIONAL | `-log2(max w)`, see Mapping |

Not written: ProfileToneCurve, DefaultBlackRender, ProfileCopyright, HueSatMap, LookTable. Rationals use denominator 1,000,000; out-of-range values are refused, not wrapped.

## Mapping

Our fit has three stages: `exposure_scale` (scalar), `channel_scale` (per-channel), and `matrix` M with `out = nᵀ·M`, `n = rgb / (exposure_scale · channel_scale)`.

Closed form, implemented in `dcp.rs` and mirrored independently in `research/generate_dcp_vectors.py`:

```text
T  = Bradford(D65 -> D50) · sRGB->XYZ · Mᵀ · diag(1 / (exposure_scale · channel_scale))
w  = T⁻¹ · D50_white          N = w / max(w)
FM = T · diag(w)              (so FM · 1 = D50 exactly)
CM = diag(N) · FM⁻¹           (so CM · D50 = N)
EV = -log2(max w)             (BaselineExposureOffset)
```

At camera white balance `N` the DCP reproduces the fitted model: measured relative error 5e-5 on the Samsung profile (rational quantization only). Invariant 2 holds: only stored scalars enter.

Away from `N` the DCP and the fit differ, because the DCP applies its own white-balance scaling. Measured CIEDE2000 drift on 24 patches, DCP vs. fitted model, white-patch luminance matched (`research/generate_dcp_vectors.py`):

| Profile | Eyedropper white balance | `channel_scale` white balance |
| --- | --- | --- |
| samsung-raw | mean 0.50, max 1.33 | mean 0.95, max 2.75 |
| rendered-jpeg (not exported; study only) | mean 0.46, max 1.33 | mean 2.00, max 5.91 |

The maximum falls on a neutral patch. Recommended use: set white balance with the eyedropper on a gray chart patch.

Decisions:

- **Tone (T): no `ProfileToneCurve`, no `DefaultBlackRender`.** The fit is linear. Writing an explicit identity curve is untested, so it is not written. Whether Lightroom shows a flat look is open.
- **Exposure (E): `BaselineExposureOffset` is written** from the stored scalars, never from scene pixels. Whether Lightroom honors it, and the RATIONAL/SRATIONAL type, are open.
- **Neutral baked from `w`.** A chart-white-direction alternative was not evaluated.

## Constraints from the invariants

- **7:** serializer in `colorbalance-core` (`interchange/dcp.rs`), pure TIFF IFD writing, no rawler, builds for `wasm32-unknown-unknown` (checked).
- **8:** UI and docs say RAW only, matrix only, measured on this tool's decode, untested in Lightroom.
- **5:** refuse quick-and-dirty profiles, empty/non-ASCII/control-character camera names, non-finite or singular transforms. No default camera name.
- **1:** the fit used rawler-ahd data with unity white balance. Lightroom decodes differently; equivalence is approximate until measured.
- **2:** no scene-inferred exposure or white balance.
- **6:** the desktop writes through a temporary file plus atomic rename. The CLI export uses a plain `fs::write`, as the CLF and `.cube` exports did before.

## Open items for the Lightroom round trip

| ID | Item | Resolved by |
| --- | --- | --- |
| C | Does Lightroom list the profile? `UniqueCameraModel` must equal Lightroom's camera name. ColorBalance stores only rawler `make`/`model`, which can differ. | User |
| T | With no tone curve, does Lightroom show the neutral (linear) look? | User |
| E | Is `BaselineExposureOffset` honored, and does a negative SRATIONAL value read correctly (spec says RATIONAL)? | User |
| P | `ProfileEmbedPolicy` 0. Licensing choice, not technical. | User |
| D | Does a `.dcp` placed in the CameraProfiles folder appear? | User |
| V | Which Lightroom product and version; Classic folder path unverified. | User |

## Phases

- **Phase 0, done:** `research/generate_dcp_vectors.py` writes `tests/fixtures/reference/dcp-vectors.json` (cases samsung-raw, rendered-jpeg, synthetic) and asserts `FM·1 = D50`, `CM·D50 = N`. The drift table above comes from it.
- **Phase 1, done:** `profile_to_dcp` and `dcp_matrices` in core, `bradford_d65_to_d50`, `linear_srgb_to_xyz_matrix`, `InterchangeError::Refused`. Seven unit tests.
- **Phase 2, done:** `colorbalance export --format dcp --camera-name "<name>"`. Smoke on a real profile: exit 0, `research/read_dcp.py` parses it. Quick-and-dirty profile and missing name exit 1 with no file.
- **Phase 3, done:** desktop DCP button, camera-name field prefilled from the profile camera, RAW-only warning, button disabled for rendered-source profiles (Library entries flagged quick-and-dirty, or a reference decoded by the JPEG/PNG decoder). The backend refuses them regardless. Smoked in the real app over CDP: RAW profile exports and parses with `research/read_dcp.py`; rendered profile and missing name are refused with no file.
- **Phase 4, needs Lightroom, not run:** a human with Lightroom runs the procedure below. Measured numbers then go in `ARCHITECTURE.md`. Only then may `AGENTS.md` drop the round trip from "Not done".

### Phase 4 procedure

1. Export: `colorbalance export <profile> --format dcp --camera-name "<name>" -o cb.dcp`. Use the name Lightroom shows for your RAW files (Metadata panel, Camera). Resolves C if the profile then appears.
2. Import: Lightroom, Develop, Profile browser, "Import Profiles", or copy to the CameraProfiles folder and restart. Record the product and version (V) and whether the profile is listed (D).
3. Open a RAW from the reference session. Pick the profile. Set white balance by clicking the neutral chart patch with the eyedropper.
4. Export the result as 16-bit TIFF with Adobe RGB or sRGB and no other adjustments. Run `colorbalance apply` on the same RAW, same profile.
5. Compare the 24 chart patches (sampler on both outputs). Record mean and max CIEDE2000 against the drift table. Large errors suggest tone curve (T), BaselineExposureOffset sign handling (E), or decode differences.
6. Repeat on a RAW with a different white balance to see the single-illuminant drift (P).

## Risks

- Decode mismatch can make Lightroom's result differ from `apply`.
- Lightroom's default look may add contrast on top of a matrix-only profile.
- A camera-name mismatch fails silently (profile not offered).
- A single-illuminant DCP is exact at one white balance only (table above).

## Out of scope

- HueSatMap, LookTable, dual-illuminant profiles. The model has none.
- XMP/preset LUT profiles.
- Embedding the profile into a DNG.
- Profiles for rendered sources.
- Installing into Lightroom's folders from the app.
