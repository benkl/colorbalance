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
3. **Quick-and-Dirty mode exception**: When calibrating from camera-processed JPEG frames using `--quick-and-dirty`, sRGB non-linearities are inverted to approximate linear scene values. This mode is explicitly flagged with warning badges in the profile and reports.

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

# Apply with parallel workers and explicit overwrite
colorbalance apply studio.cbprofile.json ./shoot -o ./balanced \
  --workers 4 --overwrite --summary summary.json

# Export to Academy Common LUT Format (.clf)
colorbalance export studio.cbprofile.json --format clf -o studio.clf

# Export to 3D LUT (.cube) with custom grid size
colorbalance export studio.cbprofile.json --format cube --size 33 -o studio.cube
```
