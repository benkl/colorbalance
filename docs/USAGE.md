# Capture, Interoperability, and Limitations Guide

This document describes how to capture reference and batch frames, select the physical chart revision, use the Common LUT Format (CLF) and `.cube` exports, and understand the workflow limitations.

## 1. Capture Guide

### Reference Capture

Photograph an X-Rite or Calibrite ColorChecker Classic target under the setup used for the subject series:

- **Same camera body and capture settings**: identical RAW format, ISO, aperture, shutter speed, picture style, and lens filters.
- **Same illumination**: identical spectrum, lighting positions, and modifiers. Avoid mixed illuminants (e.g., daylight through a window mixed with tungsten lamps).
- **Even lighting across the chart**: verify the chart is lit uniformly without gradient falloff. Spatial falloff cannot be corrected from a 24-patch chart.
- **Avoid reflections and glare**: tilt the chart slightly away from the primary light direction to prevent specular sheen on the matte/semi-gloss patches.
- **Avoid clipping**: verify in-camera histogram shows no channel clipping in the white patch (Patch 19) or bright colors.
- **Sufficient size**: the chart must occupy enough frame area that each individual patch is well-resolved (at least 32×32 pixels per patch).

### Batch Series

- Capture the series without changing exposure, lighting, or camera settings.
- The derived profile is valid **only** for frames shot under the identical setup. Changing camera body, ISO gain mode, lighting spectrum, or RAW development settings invalidates the profile.

## 2. Selecting the Physical Chart Revision

The 24 patches share the same grid layout across manufacturing eras, but the physical pigment formulations and published reference values differ between revisions:

- **`classic-before-nov-2014`**: ColorChecker Classic targets manufactured before November 2014 (ColorChecker 2005 dataset, D50 illuminant, CIE 1931 2° observer).
- **`classic-from-nov-2014`**: ColorChecker Classic targets manufactured from November 2014 onward, including Calibrite rebrands (ColorChecker 2005 after November 2014 dataset, D50 illuminant, CIE 1931 2° observer).

Select the physical revision explicitly in the CLI (`--chart`) or the desktop UI. The application refuses to silently guess the revision from the image.

## 3. Normalized Camera-RGB Input Contract

Profiles derived by ColorBalance (`*.cbprofile.json`), and exported Common LUT Format (`.clf`) or `.cube` 3D LUT files, operate on **normalized linear camera RGB**:

$$\text{input} \in [0.0, 1.0]^3, \quad \text{linear light, camera raw colorimetry}$$

### Important Contract Rules

1. **Not for camera-developed images**: The transform must **not** be applied directly to finished, tone-curved, or sRGB-encoded JPEG/TIFF files. Applying the transform to already-gamma-encoded values produces severe over-saturation and crushed shadows.
2. **Not for RAW editors without contract reproduction**: Exported CLF and `.cube` files do **not** reproduce RAW decoding. They require the host to provide the exact linear camera-RGB arrays defined by the profile's decode contract.
3. **Rendered-source exception**: When the reference is a camera-processed JPEG or PNG, sRGB non-linearities are inverted to approximate linear scene values. The CLI selects this with `--quick-and-dirty`. The desktop app detects it from the file. The profile and report flag the result as approximate.

## 4. Why DNG Camera Profile (DCP) is Deferred

Adobe Camera Raw and Lightroom require camera profiles in the `.dcp` format:

- DCP requires camera-native forward matrices calibrated to two standard illuminants (typically Standard Illuminant A and D65), interpolating between them based on shot white balance.
- DCP handles illuminant-specific hue/saturation maps (`ProfileHueSatMap`) and tone curves in Adobe's proprietary color engine.
- ColorBalance's current model derives an exact single-illuminant matrix for a specific setup. Re-encoding this as a dual-illuminant DCP would misrepresent the single-illuminant derivation. DCP generation is tracked as a future feature once dual-illuminant capture calibration is supported.

## 5. Verified CLI Command Examples

All commands run against the released `colorbalance` binary:

```bash
# Print the canonical RAW decode contract as JSON
colorbalance decode-contract

# Inspect a reference image and view quality gate diagnostics
colorbalance inspect reference.dng --chart classic-from-nov-2014

# Inspect with manual corners (top-left, top-right, bottom-right, bottom-left)
colorbalance inspect reference.dng --chart classic-from-nov-2014 \
  --quad 40,40,440,34,440,280,40,280

# Quick-and-dirty inspection of a compressed JPEG reference
colorbalance inspect reference.jpg --chart classic-from-nov-2014 --quick-and-dirty

# Derive a measured profile, HTML quality report, and SVG overlay
colorbalance derive reference.dng \
  --chart classic-from-nov-2014 \
  --profile studio.cbprofile.json \
  --report studio-report.html \
  --overlay studio-overlay.svg

# Apply the profile to a directory of matching RAW images
colorbalance apply studio.cbprofile.json ./shoot -o ./balanced --summary summary.json

# Write Display P3 or Adobe RGB (1998) TIFFs instead of sRGB
colorbalance apply studio.cbprofile.json ./shoot -o ./balanced --output-space display-p3

# Write 8-bit JPEG in Display P3 at quality 95 with 4:4:4 chroma (defaults)
colorbalance apply studio.cbprofile.json ./shoot -o ./balanced \
  --format jpeg --output-space display-p3

# Smaller JPEGs; omit GPS and opt in to XMP/IPTC copying
colorbalance apply studio.cbprofile.json ./shoot -o ./balanced \
  --format jpeg --jpeg-quality 85 --jpeg-subsampling 420 \
  --strip-gps --copy-xmp-iptc --summary summary.json

# Apply with parallel workers and explicit overwrite
colorbalance apply studio.cbprofile.json ./shoot -o ./balanced \
  --workers 4 --overwrite --summary summary.json

# Export to Academy Common LUT Format (.clf)
colorbalance export studio.cbprofile.json --format clf -o studio.clf

# Export to 3D LUT (.cube) with custom grid size
colorbalance export studio.cbprofile.json --format cube --size 33 -o studio.cube
```

### Output color spaces

`apply` writes 16-bit sRGB TIFF by default. `--format jpeg` writes 8-bit `.jpg` files instead. Either format accepts `--output-space display-p3` or `--output-space adobe-rgb`; the output embeds a matching ICC profile. TIFF remains the lossless 16-bit master.

- The fitted transform still produces linear Rec.709. OpenColorIO converts that to the target (`Linear Rec.709 (sRGB)` to `sRGB Encoded P3-D65` or `Gamma 2.2 Encoded AdobeRGB` in config `cg-config-v4.0.0_aces-v2.0_ocio-v2.5`). The color is not clipped to sRGB first, so a saturated color that sRGB would clip can survive in P3 or Adobe RGB. Clipping happens in the target space, and the clipped-pixel count refers to that space.
- Adobe RGB uses the 2.19921875 exponent from the specification, which is not the same curve as plain gamma 2.2.
- There are no display or view transforms: no tone mapping, no ACES rendering. This is a gamut and encoding change only.
- The batch summary records `output-space`. For non-sRGB output it also records `ocio-config` and `ocio-version`. They are `null` for sRGB, which does not use OCIO.
- An unknown name fails before any file is touched.
- CLF and `.cube` exports are unaffected. They still take normalized linear camera RGB from this tool's decode contract.
- In the desktop app, the output color-space selector applies to saved single images and batches. The on-screen before and after previews are always sRGB.

### JPEG and capture metadata

JPEG uses the same color-space conversion and target-space clipping as TIFF, then rounds the samples to 8 bits. Its default quality is 95 with 4:4:4 chroma subsampling; use `--jpeg-quality 1..100` and `--jpeg-subsampling 444|422|420` to trade color-edge fidelity for size. JPEG dimensions cannot exceed 65,535 pixels per side.

Both export formats carry a reviewed selection of available source metadata: camera make/model, lens, capture time, exposure, ISO, focal length, GPS, artist and copyright. `--strip-gps` drops coordinates. `--copy-xmp-iptc` additionally copies XMP and IPTC where the source and output container support them. No metadata is an ordinary case: export still succeeds, and the batch summary records what was copied or skipped for each image.

The output pixels are already upright. Original Orientation, embedded thumbnails, ColorSpace, white-balance settings and maker notes are not copied: those describe the old RAW or could point at stale data. A JPEG-to-JPEG batch never replaces an input at the same path, even with `--overwrite`. Existing destination files are skipped unless overwrite is explicit; completed outputs survive a later file failure.

### Finding the chart in the desktop app

Load a reference and the preview appears first. For images of 12 megapixels or less the app then looks for the chart on its own, in a few milliseconds. For larger images it skips that step and shows "Large image: automatic detection skipped". Press **FIND CHART** to run it anyway. After a miss the button reads **RETRY**.

What a result means:

- **Found.** Four corners appear on the light table. They are a proposal. Check them and drag any marker that is off. Dragging cancels a detection that is still running and keeps your edit.
- **No chart found.** The corners stay where they were. Place them yourself.
- **Ambiguous.** Two separate regions looked like a chart. The corners stay where they were. Place them yourself.

Detection never picks the chart revision. Choose `classic-before-nov-2014` or `classic-from-nov-2014` yourself before deriving. INSPECT and DERIVE are disabled while a detection is running, so a stale proposal cannot reach the fit.

The detector looks for 24 bright patch squares separated by dark gaps, laid out as a 6 by 4 grid with a descending neutral row. It works on a thumbnail of at most 512 px per side, so it needs the chart to be at least about 48 thumbnail pixels wide and the gaps between patches to be visible. Expect a miss for a small chart, heavy glare, an occluded corner, very low contrast, or patches that blur into each other. A miss costs you four manual clicks. A wrong proposal would cost a bad profile, which is why the detector refuses to guess.

The CLI does not run the detector. Without `--quad`, `inspect` and `derive` sample a rectangle inset 8% from each image edge, which is only right when the chart fills the frame. Pass `--quad` for anything else, using corners read off the desktop app.

### Quality warnings in the desktop app

The desktop app never blocks derivation on chart quality. It derives the profile, then shows what it found. There are no Quick & Dirty or "derive despite failures" checkboxes.

- A rendered JPEG or PNG source is detected from the file contents and gets the relaxed gate set. A RAW source always gets the strict gates.
- The result header reads `CAPTURE QUALITY PASS` or `PROCESSED WITH N WARNING(S)`. Each warning is one line, for example `10 clipped patch(es): re-shoot at lower exposure.` The unabridged per-patch list sits under `DETAILS`.
- The profile stores a `quality` record with `passed`, `overridden`, `quick-and-dirty`, and every failed gate (patch, reason, measured value). `overridden` is true whenever gates failed and the profile was written anyway. Profiles written before this field existed omit it and keep their digests.
- The HTML report repeats the failures and ranks all patches by fit error.

A low fit error does not mean the capture was sound. Clipped patches carry no color information, and noisy or misaligned patches bias the fit. Check the corners and exposure before trusting a profile that came with warnings.

The CLI is unchanged. `derive` still refuses a failing chart unless you pass `--force`, and `--quick-and-dirty` still selects the relaxed gates.

### Correcting single images and comparing before and after

The desktop app has five tabs: **REFERENCE**, **VALIDATE**, **COMPARE**, **EXPORT**, and **LIBRARY**. Switch tabs from the header; no panel button moves you to the next one. The viewport follows the active tab:

- **REFERENCE** shows the light table with the chart corners on a flat background, with no grid, scanlines, or animation.
- **VALIDATE** shows a grid of the 24 chart patches in chart order (6 columns by 4 rows). Each cell shows the corrected color above the chart target, with the patch's ΔE00. The bar under the number is that ΔE relative to the worst patch of this fit. It ranks patches and is not a pass or fail band; the quality gates are reported in the sidebar. Click a cell to highlight its row in the sidebar table.
- **COMPARE** shows the before and after of the last corrected image, captioned with its source file and, if it was written, the saved path. Opening it with a derived profile and no comparison renders the reference automatically. After "Apply & save image" it shows that image instead. With a Library profile active it stays empty until you apply the profile to an image, because the entry's reference may come from another camera. Controls are listed under "Comparison viewer controls".
- **EXPORT** shows what the next write acts on. With no active profile it says so. With a derived profile it says the profile is ready to apply. With a Library profile it shows that entry's card: reference thumbnail, label, notes, tags, quality badges, camera, lens, capture time, GPS, ΔE fit, chart revision, decoder, profile digest and path, plus the camera-mismatch notice when it applies. After "Apply & save image" it names the saved file and points to COMPARE. While a batch runs, and after it ends, it shows a progress bar and one row per file: written, skipped, or failed, with any warning or error.
- **LIBRARY** shows the gallery.

Everything that writes a file lives on EXPORT, and it always acts on one **active profile**: the Library entry you chose with "Use this profile", otherwise the profile derived in this session. Clear the Library selection on EXPORT to go back to the derived profile.

- **Apply & save image** picks any image, corrects it with the active profile, and writes a TIFF or JPEG to the path you choose. COMPARE then shows its before and after.
- **Save reference** (derived profile only) corrects the reference image and writes it in the chosen format.
- **Apply & write batch** corrects a folder into a destination folder, with an overwrite switch and a stop button. The result counts, failed files, and warnings appear below the controls.
- **.CLF** and **.CUBE**, under "Interchange files", export the active profile's transform for other tools.

Both previews are rendered in Rust and downscaled to 1600 px on the long side. The saved file is full resolution. The writer uses a temporary file in the destination folder, flushes it, then renames it, so a failed write leaves no partial output. The save dialog confirms replacing an existing file. For a single image, the camera and decode contract must match the profile, whether the profile is derived or from the Library. A mismatch fails with both camera names and writes nothing, so a profile derived from a rendered JPEG cannot correct a RAW file. Only Library batches continue past a camera or decode-contract mismatch; see the Library section.

The desktop app keeps downscaled PNG previews in a private temporary directory and displays them through Tauri's asset protocol. It waits for replacement images to decode and for subsequent animation frames before removing previews it no longer needs; failed replacements leave the old image available. Closing the app removes any remaining files. Original images and saved TIFFs are not webview asset files.

#### Comparison viewer controls

A toolbar above the image controls what the viewport shows. Both images always share one zoom and pan, so the same pixels stay aligned.

| Control | Effect | Key |
| --- | --- | --- |
| Split | Reveals the original on one side of a draggable line, with the corrected image on the other. | `1` |
| Side by side | Original and corrected in two panes. | `2` |
| Before / After | One image alone. | `3` / `4` |
| Left-right or top-bottom | Direction of the split line. Arrow keys nudge it by 2%. | arrows |
| Zoom `-` `+`, Fit, 100% | The readout is display pixels per preview pixel, up to 800%. From 200% up, pixels render unsmoothed. | `-` `+` |
| Mouse wheel, double-click | Zoom at the cursor. Double-click toggles between fit and 2x. | |
| Drag | Pans while zoomed; the image stays in view. | |
| Hold: Before | Shows the original while held. | Space |
| Backdrop | Dark, mid-gray, or light surround for judging color. | |
| Reset | Fit zoom, no pan, split at 50%. | `0` |

### Why a raw preview looks green

The decode contract uses unity white balance, so a DNG decodes as unbalanced camera RGB. Sensors collect much more green than red or blue, so that data looks green. A Galaxy S25 DNG averaged R 76, G 96, B 70 before balancing. Phone galleries hide this because they apply the camera's recorded neutral.

For display only, the preview and the "before" image divide each channel by the DNG `AsShotNeutral` tag. On the same file that gives R 103, G 96, B 96. Pixel values, calibration, chart measurement, and corrected output never see this gain, and rendered JPEG/PNG sources have none. The "before" image is a white-balanced view of the camera data, not what the contract decodes.

## 6. Desktop library

The Library tab lists calibrations saved in one folder. Pick the folder once. The app rescans it every time you open the tab.

Each card shows the preview (a card shows "No preview" when the reference could not be decoded at save time), the label, camera make, model and lens, capture time, GPS, calibration quality (passed, overridden or quick and dirty, with mean and max delta E), chart revision, decode contract, tags and notes. Entries whose profile fails its digest check appear under "Problems" and cannot be used.

To save an entry, derive a profile, then use "Save to library" on the Validate tab. Enter a label, optional notes and tags. GPS from the reference photo is stored unless you untick "Include GPS". Take care before sharing a library folder, because the location is in `entry.json`.

To share a calibration, copy its folder into another person's library folder. To use a `.cube` or `.clf` file, run `colorbalance export` on the entry's `profile.cbprofile.json`.

"Use this profile" makes an entry the active profile for EXPORT and stays on the Library tab; switch to EXPORT to apply it. The active profile is used for single images, batches, and interchange export. For a batch, if the entry's camera or decode contract differs from the image, the file is still processed and the batch report carries a warning for it. The result will be wrong in color unless the two cameras behave alike. Exposure mismatches still stop processing. Single-image export with a Library entry does not have this exception: a mismatch fails and writes nothing.
