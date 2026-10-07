# Module contracts

Binding interface specification for parallel implementation. Every agent implements exactly these signatures against these rules. Deviations must be reported, not silently made. Tolerances are absolute unless stated. All math is `f64`; image pixels are `f32` after decode; all serialization is deterministic.

Shared rules:

- No new dependencies without listing them in the report. Approved shared deps: `serde`, `serde_json`, `thiserror`, `sha2`, `clap`.
- `unsafe` is forbidden in `colorbalance-core` and `colorbalance-cli`, denied in `colorbalance-raw` (no `unsafe` code is present there).
- Every public function gets doc comments. Tests load expected values from committed JSON; the code under test never generates its own expected values.
- Keep files formatted with `cargo fmt` and clippy-clean with `-D warnings`.

## Reference data (already generated, do not regenerate)

- `research/generate_reference_data.py` is the authority. It uses colour-science 0.4.4.
- `crates/colorbalance-core/data/chart-datasets.json` - both chart revisions, 24 patches each, fields per patch: `name`, `xyY`, `xyzD50`, `xyzD65`, `linearSrgbD65`, `labD65`. Dataset fields: `source`, `illuminant` ("D50"), `observer` ("CIE 1931 2 Degree Standard Observer"), `adaptation` ("Bradford"), `targetWhitePoint` ("D65").
- `tests/fixtures/reference/color-vectors.json` - test vectors: matrices `bradfordD50toD65`, `xyzToLinearSrgbD65`, `linearSrgbToXyzD65`, chromaticity `d50`, `d65`, and case lists `bradfordCases`, `xyzToSrgbCases`, `xyzToLabCases`, `srgbEncodeCases`, `deltaE2000Cases` (34 published Sharma pairs).

## Module `colorbalance-core::color` (file `crates/colorbalance-core/src/color.rs`)

Owner: agent ColorMath. No deps beyond std.

```rust
pub const D50_XYZ: [f64; 3]; // = xy_to_xyz([0.3457, 0.3585]); Y=1
pub const D65_XYZ: [f64; 3]; // = xy_to_xyz([0.3127, 0.3290]); Y=1
pub fn bradford_d50_to_d65() -> [[f64; 3]; 3];
pub fn xyz_d50_to_d65(xyz: [f64; 3]) -> [f64; 3];
pub fn xyz_to_linear_srgb(xyz: [f64; 3]) -> [f64; 3];
pub fn linear_srgb_to_xyz(rgb: [f64; 3]) -> [f64; 3];
pub fn xyz_d65_to_lab(xyz: [f64; 3]) -> [f64; 3];
pub fn lab_d65_to_xyz(lab: [f64; 3]) -> [f64; 3];
pub fn delta_e_2000(a: [f64; 3], b: [f64; 3]) -> f64;
pub fn srgb_encode(v: f64) -> f64;   // piecewise IEC 61966-2-1, no clamp
pub fn srgb_decode(v: f64) -> f64;   // inverse, no clamp
pub fn mat_vec(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3];
```

Definitions:

- Bradford CAT matrix `M_A` from cone response
  `[[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]]`;
  `M = M_A^-1 · diag(ρ_wrk/ρ_src) · M_A` with src=D50_XYZ, dst=D65_XYZ. Row-vector convention: `out = v · M` matches the JSON (vectors are row-multiplied). Verify against `bradfordD50toD65` to 1e-9.
- XYZ↔linear sRGB uses the D65 sRGB matrix from the JSON (`xyzToLinearSrgbD65` / transpose inverse) to 1e-9.
- Lab: D65, 2-deg. `f(t) = t^(1/3)` if `t > (6/29)^3` else `t/(3·(6/29)^2) + 4/29`. `L = 116·f_y - 16`, `a = 500(f_x - f_y)`, `b = 200(f_y - f_z)`, `X_n=D65_XYZ[0]` etc. `xyz_d65_to_lab([0,0,0])` must be `[0,0,0]`. Match JSON cases to 1e-8.
- `delta_e_2000` implements CIE 224 exactly as colour-science `method="CIE 2000"` (hue angle handling with atan2, `h'` continuity). Match all 34 Sharma pairs to 1e-6.
- `srgb_encode(v) = 12.92·v` for `v <= 0.0031308` else `1.055·v^(1/2.4) - 0.055`. Match JSON to 1e-12. `srgb_decode` is its exact inverse; round-trip property test to 1e-15.
- Tests read `tests/fixtures/reference/color-vectors.json` via `include_str!("../../../tests/fixtures/reference/color-vectors.json")` and a small serde_json parse. Also test `linear_srgb_to_xyz(xyz_to_linear_srgb(x)) == x` to 1e-12 and same for Lab round trip (excluding near-zero Y where L degenerates; use inputs with Y > 0.01).

## Module `colorbalance-core::decode` (file `src/decode.rs`, already created, owner: lead)

Types shared by every decoder and the calibration engine:

```rust
pub struct CameraIdentity { pub make: String, pub model: String,
    pub decoder: String, pub decoder_version: String }
pub struct DecodedImage {
    pub width: u32, pub height: u32,
    pub rgb: Vec<f32>,        // interleaved RGB, length w*h*3, row-major, upright
    pub clipped: Vec<u8>,     // per pixel bitmask: 1=R, 2=G, 4=B; length w*h
    pub black_levels: [u16; 4], // per CFA position [0]=row0col0, [1]=row0col1, [2]=row1col0, [3]=row1col1
    pub white_levels: [u16; 4],
    pub cfa_pattern: [u8; 4],  // b'R' | b'G' | b'B' per CFA position
    pub camera: CameraIdentity,
}
pub trait RawDecoder {
    fn decode_path(&self, path: &std::path::Path)
        -> Result<DecodedImage, DecodeError>;
}
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("io error: {0}")] Io(#[from] std::io::Error),
    #[error("unsupported format: {0}")] UnsupportedFormat(String),
    #[error("unsupported sensor layout: {0}")] UnsupportedSensorLayout(String),
    #[error("corrupt file: {0}")] CorruptFile(String),
}
```

Normalization rule (decoders must follow exactly): `v_norm = max(0, (raw - black_c) / (white_c - black_c))` computed in f64 from each RAW-domain sample, then stored as f32. Values above white are NOT clamped; the source sample is flagged in `clipped`. For CFA, `c` is the CFA position; a photosite is saturated when `raw >= white_c`. The AHD demosaic flags a pixel's channel when a saturated photosite of that channel lies within a (2r+1)x(2r+1) window around it, with support radius r = 5. For LinearRaw, `c` is the RGB component and each pixel's channel flag comes from that component before orientation or preview rendering. `sensor_layout` distinguishes CFA, LinearRaw, and rendered inputs; the CFA pattern is not meaningful for LinearRaw.

## Module `colorbalance-raw::rawler_decode` (file `crates/colorbalance-raw/src/rawler_decode.rs`)

```rust
pub const DECODER_NAME: &str = "rawler-ahd";
pub const DECODER_VERSION: &str = "0.8.0"; // must match the pinned rawler dependency
pub struct RawlerDecoder;
impl colorbalance_core::decode::RawDecoder for RawlerDecoder { ... }
pub fn decode_raw(path: &std::path::Path)
    -> Result<colorbalance_core::decode::DecodedImage, colorbalance_core::decode::DecodeError>;
```

Adapter requirements:

- rawler (`get_decoder`, `raw_image` with default parameters) reads the file. Panics inside rawler, such as `todo!()` on an unknown PhotometricInterpretation, are caught and reported as an unsupported format.
- Only a 2x2 RGB Bayer CFA (one sample per pixel) or three-component LinearRaw is accepted. Float samples, other CFA layouts, and unsupported black-level grids fail closed.
- Only full-frame images are accepted. A crop area or active area smaller than the image fails closed with the rectangles in the message.
- Black and white levels come from rawler's reading of the file. `white <= black` is rejected.
- CFA data is normalized with the rule above, then demosaiced by `colorbalance_core::ahd::demosaic_ahd`. LinearRaw samples are normalized per component with no demosaic and no scene-inferred white balance.
- All eight rawler orientations are applied to pixels and flags.
- `display_neutral` is `1 / wb_coeffs[..3]` when all three are finite and positive, otherwise absent.
- The decoder does not apply opcodes, gain maps, or linearization beyond what rawler does. For DNG, rawler 0.8.0 reads black and white levels from the file (read from its source: the camera catalog supplies only clean names, hints and params); if the WhiteLevel tag is absent it falls back to the bit depth. For non-DNG formats the levels come from rawler's camera catalog, which is not checked against real camera files here.

Writer (fixture support, same file or `dng_writer.rs` in this crate, exported for the fixtures crate):

```rust
pub struct DngWriteSpec {
    pub width: u32, pub height: u32,
    pub cfa_pattern: [u8; 4],
    pub black_levels: [u16; 4], pub white_levels: [u16; 4],
    pub make: String, pub model: String,
    pub orientation: u16, // 1
    pub photosites: Vec<u16>, // row-major mosaic, length w*h
}
pub fn write_dng(path: &std::path::Path, spec: &DngWriteSpec) -> std::io::Result<()>;
```

Writes little-endian classic TIFF with exactly the tags above (BlackLevel as SHORT×4, WhiteLevel as SHORT, single strip, one IFD). Round-trip test: write a spec with known mosaic, decode, assert photosite-normalized values and flags match exactly (write → read at photosite level by demosaicing a constant-mosaic image). Test clipping: set some photosites to white level for one channel only, assert only that channel's bit is set after demosaic and only in the affected window. Test orientation: encode orientation 6 and assert pixel positions flip. Test unsupported: craft a variant with PhotometricInterpretation=2 (RGB) and assert UnsupportedFormat.

## Fixture crate `colorbalance-fixtures` (new, `crates/colorbalance-fixtures`)

Owner: agent DngCodec (same task). Test-only crate, never a runtime dependency of core/cli.

```rust
pub struct ChartScene {
    pub width: u32, pub height: u32,
    pub quad: [[f64; 2]; 4],       // chart corners TL,TR,BR,BL in pixels
    pub revision: colorbalance_core::ChartRevision,
    pub exposure: f64,             // multiplies all camera rgb before quantize
    pub defect: SceneDefect,
}
pub enum SceneDefect { Clean, ClipWhitePatch, ClipSingleChannel { patch: usize }, Glare { patch: usize } }
pub fn render_chart_dng(path: &std::path::Path, scene: &ChartScene) -> std::io::Result<()>;
pub const CAMERA_MATRIX: [[f64; 3]; 3]; // fixed synthetic camera response, defined below
```

Rendering model (deterministic):

- For each patch `i` (reading order, 6 columns × 4 rows inside the quad, cell = bilinear map of the quad), fill the central 100% of the cell (patch boundaries are the cell boundaries; add a 2% dark gap between cells to model chart borders) with flat color.
- Target camera RGB for patch `i`: `t = dataset(i).xyz_d50` (from embedded dataset), `cam = (t · CAMERA_MATRIX) · exposure`, quantize per CFA site: the mosaic samples channel `C` at its sites with value `clamp(round(cam_C · white_C + black_C), black_C, white_C)` where the clamp to white marks saturation (flagged by decoder). CAMERA_MATRIX = `M_sRGB_from_XYZ · M_bradford` with column gains `[1.05, 0.98, 1.06]` and cross-terms: `M = [[1.02, 0.03, 0.00], [0.02, 0.99, 0.02], [0.00, 0.04, 1.04]] · (M_sRGB_from_XYZ · M_bradford)` — exact constants computed in code from `colorbalance_core::color` functions (compose at runtime; no hand-copied numbers).
- `ClipWhitePatch`: exposure chosen so the white patch exceeds white level in all channels (quantizer clamps, flags set).
- `ClipSingleChannel`: after normal quantize, force patch `i`'s R sites to white level (camera value chosen so only R clips: use exposure * 1.5 for that patch's R channel pre-quantize).
- `Glare`: patch `i` gets a radial gradient added to all channels peaking at +40% center, no clipping (must trip the within-patch variation gate, not the clipping gate).
- Outside the quad: flat 18% gray with the same camera transform applied to XYZ of sRGB 0.18 gray (use linear 0.18 → XYZ via color module), so the surround is neutral.
- Choose `width=480, height=320`, quad = chart rectangle inset ~40 px with a slight perspective (TR 6 px up) so perspective mapping is exercised. Default exposure 1.0 uses the dataset's own scale; verify the white patch lands between 0.5 and 0.95 of white level for the Clean scene (test asserts this from decoded values).
- Include a test: decode the Clean fixture through `RawlerDecoder`, sample patches with the calibration module's quad grid (integration test may live in the fixtures crate or tests/; coordinate via lead if calibration is not integrated yet — then assert only patch means approximately match the analytic camera rgb to 1e-3).

## Module `colorbalance-core::dataset` (file `src/dataset.rs`)

Owner: agent ChartData.

```rust
pub struct PatchReference { pub patch: ChartPatch,
    pub xy_y: [f64; 3], pub xyz_d50: [f64; 3], pub xyz_d65: [f64; 3],
    pub linear_srgb_d65: [f64; 3], pub lab_d65: [f64; 3] }
pub struct ChartDataset { pub revision: ChartRevision, pub source: String,
    pub illuminant: String, pub observer: String, pub adaptation: String,
    pub patches: Vec<PatchReference> /* len 24 */ }
pub fn load(revision: ChartRevision) -> Result<ChartDataset, DatasetError>;
pub fn dataset_digest() -> String; // sha256 hex of the embedded JSON bytes
```

- Parse `include_str!("../data/chart-datasets.json")` with serde (kebab-case names map to `ChartRevision` serde form; patch `name` maps to `ChartPatch`).
- Validate: 24 patches, names exactly in `ChartModel::new(revision).patches` order, illuminant D50, observer "CIE 1931 2 Degree Standard Observer", adaptation Bradford. Any mismatch → `DatasetError::InvalidDataset(String)` naming the problem.
- Unknown revision string in JSON → error.
- Tests: both revisions load; digest is 64 hex chars and stable across calls; tampering a parsed struct field is impossible (no mutation API), but a `load_from_str` helper (pub for tests) rejects wrong patch order and wrong count.

## Module `colorbalance-core::calibration` (file `src/calibration.rs`)

Owner: agent Calibration. Uses `color`, `decode`, `dataset`, `chart` modules.

```rust
pub struct ChartQuad { pub corners: [[f64; 2]; 4] } // TL, TR, BR, BL
pub struct PatchSample { pub patch: ChartPatch,
    pub mean_rgb: [f64; 3], pub variance: [f64; 3],
    pub clipped_mask: u8, pub sample_pixels: usize }
pub struct GateConfig { pub min_patch_pixels: usize,      // default 64
    pub max_cv: f64,                                       // default 0.05
    pub cv_mean_floor: f64,                                // default 0.01; denominator floor in normalized camera RGB
    pub require_neutral_row: bool }                        // default true
pub struct GateFailure { pub patch: Option<ChartPatch>, pub reason: String,
    pub measured: String }
pub fn sample_patches(image: &DecodedImage, quad: &ChartQuad)
    -> Result<Vec<PatchSample>, CalibrationError>; // 24 samples, reading order
pub fn evaluate_quality(samples: &[PatchSample], cfg: &GateConfig)
    -> Result<(), Vec<GateFailure>>; // Ok(()) or all failures
pub struct PatchValidation { pub patch: ChartPatch,
    pub source_rgb: [f64; 3], pub corrected_rgb: [f64; 3], pub target_rgb: [f64; 3],
    pub corrected_lab: [f64; 3], pub target_lab: [f64; 3], pub delta_e: f64 }
pub struct ValidationReport { pub per_patch: Vec<PatchValidation>,
    pub mean_delta_e: f64, pub median_delta_e: f64, pub p95_delta_e: f64,
    pub max_delta_e: f64, pub neutral_max_delta_e: f64, pub skin_max_delta_e: f64,
    pub condition_number: f64 }
pub struct FittedStages { pub exposure_scale: f64, pub channel_scale: [f64; 3],
    pub matrix: [[f64; 3]; 3] }
pub fn fit(samples: &[PatchSample], dataset: &ChartDataset)
    -> Result<(FittedStages, ValidationReport), CalibrationError>;
```

Definitions:

- Grid mapping: bilinear interpolation of the quad. Cell `(col, row)` with `col∈[0,6)`, `row∈[0,4)`: corners at `u=(col)/6…(col+1)/6`, `v=row/4…`. Sampling region: central 60% of each cell (20% margin each side) to avoid patch borders and demosaic blending. `sample_pixels` is the pixel count of that region; require ≥ `min_patch_pixels` measured per patch.
- `mean_rgb` is the mean of `f64::from(pixel)` over the region per channel; `variance` is population variance per channel; `clipped_mask` ORs the decode masks in the region.
- Gates: any `clipped_mask != 0` (reason names the channel bits); `sqrt(variance)/max(mean, cv_mean_floor) > max_cv` per channel (reason names patch, channel, measured cv); patch pixel count; neutral row monotonicity: the luminance `Y = 0.2126R+0.7152G+0.0722B` of the six neutral patches must be strictly increasing in reading order White→Black (reversed order means the quad is upside down: report "neutral row reversed; rotate corners"); `require_neutral_row` also checks the last row chroma `max(|a*|,|b*|)`… use simple chroma proxy `max-min channel / mean` of last-row patches < 0.12 while rows 0-2 contain patches exceeding it (else "last row is not the neutral row; check corner order").
- Fit steps: `y_k` = target luminance of neutral patch `k` (from `dataset` `linear_srgb_d65` Y). `e = median_k(meanY_k / y_k)`. `c_j = median_k(mean_kj / (e · y_k))`. Normalized source `n_ij = m_ij / (e · c_j)`. Least squares per output channel `j`: solve `A^T A x = A^T b_j` with `A` the 24×3 matrix of `n_i`, `b_j` the target `t_ij`; 3×3 solve via Gaussian elimination with partial pivoting (write it in this module; no external linear algebra dep). `matrix` is the row-vector convention `corrected = n · M`.
- Validation: `corrected = n · M`; ΔE00 in Lab (D65) between corrected and target per patch; mean, median, p95 (linear interpolation between order statistics at 0.95·23), max; neutral subset max (patches White..Black); skin subset max (DarkSkin, LightSkin); condition number = `‖M‖_∞ · ‖M⁻¹‖_∞` (3×3 inverse analytically; if singular → CalibrationError::SingularMatrix).
- Errors: `#[error("chart sampling failed: {0}")] Sampling(String)`, `SingularMatrix`, `InsufficientPatches`.
- Tests: identity fixture — construct `PatchSample`s directly from a dataset's targets passed through a known matrix `M_t` and gains; `fit` must recover `M_t` to 1e-6 and report max ΔE < 1e-6. Held-out check: fit on 23 patches, predict the 24th, ΔE < 1e-6. Leave-one-out API is not required this round; the test constructs the subsets itself.

## Module `colorbalance-core::profile` (file `src/profile.rs`)

Owner: agent ProfileApply. Uses calibration, contract, decode types.

```rust
pub const SCHEMA_VERSION: &str = "1.0";
pub struct Profile { pub schema_version: String,
    pub decode_contract: DecodeContract, pub camera: CameraIdentity,
    pub chart_revision: ChartRevision, pub dataset_digest: String,
    pub reference_digest: String, pub transform: FittedStages,
    pub validation: ValidationSummary }
pub struct ValidationSummary { pub mean_delta_e: f64, pub max_delta_e: f64,
    pub p95_delta_e: f64, pub neutral_max_delta_e: f64,
    pub skin_max_delta_e: f64, pub condition_number: f64, pub patch_count: u32 }
pub fn to_json(p: &Profile) -> String;   // canonical field order, pretty
pub fn digest(p: &Profile) -> String;    // sha256 of to_json with digest field set to "" 
pub fn from_json(s: &str) -> Result<Profile, ProfileError>;
// verifies: schema major matches SCHEMA_VERSION, digest matches, contract validates
pub fn apply_transform(p: &Profile, rgb: [f64; 3]) -> ([f64; 3], u8);
// exposure, channel scale, matrix, then clip to [0,1]; u8 bits 1=R low,2=G,4=B high? NO:
// bit 1 = R below 0 or above 1? Return two flags per channel packed: bits 0..2 low-clip R,G,B; bits 3..5 high-clip R,G,B.
pub fn encode_srgb_u16(rgb: [f64; 3]) -> [u16; 3]; // expects already clamped; round(v*65535), half away from zero
```

- JSON uses kebab-case, `deny_unknown_fields`, floats via serde default (shortest round-trip). `reference_digest` is the sha256 of the reference image file, provided by the caller (CLI computes it; tests pass a fixed hex).
- `from_json` recomputes the digest over the loaded struct and compares with the embedded `digest` field: add `pub digest: String` to `Profile` serialized last; `digest(p)` serializes with that field as empty string and hashes the result. Mismatch → `ProfileError::DigestMismatch`.
- Tests: round-trip preserves everything bit-exactly; tampering any float fails digest; wrong major version (edit "2.0" and fix nothing else) fails before any decode; `apply_transform` on the identity profile is identity within 1e-12 with no clip bits; a matrix producing -0.1 and 1.4 sets low bit R / high bit G exactly; `encode_srgb_u16` boundary: 0.0→0, 1.0→65535, 0.5→ round(32767.5)=32768, 0.0001→ round(6.5535)=7.

## Module `colorbalance-core::output` (file `src/output.rs`)

Owner: agent ProfileApply.

```rust
pub fn encode_tiff_rgb_u16(width: u32, height: u32, pixels: &[u16], icc_profile: &[u8]) -> Vec<u8>;
```

Baseline TIFF, little-endian, single strip, uncompressed, PhotometricInterpretation=2 (RGB), BitsPerSample=[16,16,16], SamplesPerPixel=3, plus tag 34675 (InterColorProfile, type UNDEFINED) holding `icc_profile` verbatim; an empty profile panics. Deterministic bytes. Test: decode with a minimal reader in the test (reuse the tag parser you write) and compare pixel data; plus property test that output length matches header strip byte count. A local Python/Pillow check happens in integration (lead).

## Module `colorbalance-core::output_space` (file `src/output_space.rs`)

Decision D24. `OutputSpace` is `Srgb` (default), `DisplayP3` or `AdobeRgb`, parsed from and printed as `srgb`, `display-p3`, `adobe-rgb`. `OutputConverter::new(space)` builds an OCIO processor from `Linear Rec.709 (sRGB)` to the target in the built-in config `cg-config-v4.0.0_aces-v2.0_ocio-v2.5`. Construction fails with `OutputSpaceError::Ocio` rather than falling back.

```rust
impl OutputConverter {
    pub fn new(space: OutputSpace) -> Result<Self, OutputSpaceError>;
    pub fn icc_profile(&self) -> Result<Vec<u8>, OutputSpaceError>;
    pub fn convert_linear_rec709(&self, rgb: [f32; 3]) -> [f32; 3];
    pub fn correct_to_u16(&self, p: &Profile, rgb: &mut [f32]) -> (Vec<u16>, usize);
}
```

- `correct_to_u16` returns interleaved 16-bit samples and the number of pixels with any channel clipped. For `Srgb` it is `profile::correct_to_u16` unchanged. For the other spaces it applies `profile::linear_rec709` without clamping, converts with OCIO, then clamps and quantizes in the target space. `rgb` is left holding the clamped linear Rec.709 values so previews keep working.
- `icc_profile` is authored by `moxcms` and has its header creation date fixed to 1970-01-01 00:00:00, so equal inputs give equal bytes.
- Fixture: `tests/fixtures/reference/output-space-vectors.json`, generated by `research/generate_output_space_vectors.py` with the Python `colour` package (an implementation independent of OCIO). Regenerate with `python research/generate_output_space_vectors.py` after changing probes. Tolerance against it is 5e-4 of encoded range, and 2e-2 where the reference linear value is below 1e-4: OCIO adapts the white point through ACES2065-1 while the reference converts by matrix, and the slope of a power curve near black amplifies the difference.

## Module `colorbalance-core::interchange` (files `src/interchange/clf.rs`, `src/interchange/cube.rs`, `src/interchange/mod.rs`)

Owner: agent Interchange. Uses profile + color.

```rust
pub fn profile_to_clf(p: &Profile) -> String;
pub fn clf_to_matrices(clf: &str) -> Result<([[[f64; 3]; 3]]; 2), InterchangeError>; // parse own subset
pub fn profile_to_cube(p: &Profile, size: usize) -> String;
pub fn cube_max_error(p: &Profile, size: usize, probe_count: usize) -> f64;
```

- CLF: exactly two `Matrix` ProcessNodes inside one `ProcessList` (`compCLFversion="3.0"`, `id="colorbalance-<digest12>"`): node 1 diagonal matrix of `channel_scale`, node 2 the color `matrix` (row-vector convention CLF uses row-times-matrix: Array rows are the matrix rows; document in Description that input is row-vector · matrix). Include `InputDescriptor` "normalized linear camera RGB, decode contract digest <digest16>, camera <make model>", `OutputDescriptor` "linear sRGB D65", `Description` naming ColorBalance and profile digest. `inBitDepth/outBitDepth="32f"`. Numbers with `format!("{:.10e}")`… use `{:.10}` trimmed of trailing zeros is NOT required; plain `{}` of f64 is fine and round-trips. LF newlines, UTF-8, starts with `<?xml version="1.0" encoding="UTF-8"?>`.
- `clf_to_matrices` parses only our generated subset (simple string scanning is acceptable; no XML dep). Test: `profile_to_clf` → parse → applying the two matrices in sequence to sample vectors equals `apply_transform` without clipping to 1e-9 (inputs chosen in range so no clipping occurs).
- `.cube`: `TITLE`, `DOMAIN_MIN 0 0 0`, `DOMAIN_MAX 1 1 1`, `LUT_3D_SIZE N`, then N³ lines "r g b" of transform outputs clamped to [0,1], red fastest. `cube_max_error` trilinearly interpolates the cube at deterministic probe points (LCG `state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)` starting 0x5eed, take 3 draws scaled to [0,1]) and compares with direct `apply_transform`; returns max abs component error. Test: size 33 on the identity profile has error < 1e-12; on a mild fixed matrix profile, error < 5e-3 and monotone with size (33 → error decreases or stays within 10% of size 17 error).

## CLI (owner: lead, later wave)

Not part of this delegation.

## Repository and integration rules

- Do not edit `Cargo.toml` files other than adding your crate's approved deps; report needed changes instead if they touch workspace deps.
- Do not edit files you do not own; the lead integrates.
- Each agent reports: files created, public API as implemented (with any deviation and why), test names, and `cargo test` output summary for their crate.
