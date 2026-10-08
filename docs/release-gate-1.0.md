# Milestone 4: historical desktop gate assessment (2026-10-04)

Date: 2026-10-04  
Scope: `colorbalance` 0.1.0-alpha / Milestone 4. This is a historical assessment, not proof that the 2026-10-08 Windows pre-release ran on a clean machine. See [packaging](packaging.md) and [changelog](../CHANGELOG.md) for the shipped status.

## 1. Executive Summary

ColorBalance Milestone 4 delivers a fully functional, local-first color calibration and batch processing tool with both a CLI engine and a standalone desktop application (Tauri 2 + React + Planck Light-Table UI).

All non-negotiable invariants defined in `AGENTS.md` and `docs/IMPLEMENTATION_PLAN.md` are enforced:
- Deterministic RAW and rendered image decoding.
- Fixed decode contracts embedded in tamper-evident SHA-256 sealed profiles.
- Prohibition of scene-inferred exposure or neutral balance.
- Non-destructive batch application with temporary atomic file replacement.
- Fail-closed metadata verification on camera mismatch.

## 2. Tested Cameras, Formats, and Illuminants

| Capture Source | Capture Mode | Format | Resolution | Tested Illuminant | Status | Measured ΔE2000 (Mean / Max) | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Synthetic Sensor Model A | Linear Raw CFA | DNG 1.4 | 480×320 | D50 Bradford Adapted | **PASS** | 0.203 / 1.996 | Clean reference, condition number 1.15 |
| Synthetic Sensor Model B | Linear Raw CFA | DNG 1.4 | 480×320 | D65 Linear Target | **PASS** | 0.842 / 2.145 | Multi-shot batch series |
| Samsung Galaxy S25 | Camera ISP Rendered | JPEG (sRGB) | 1864×1398 | Natural Indoor / Ambient | **SUPPORTED (Tier 2)** | 26.949 / 56.633 | Quick & Dirty mode: inverts sRGB gamma; high CV from non-uniform field lighting correctly flagged by quality gates |
| Synthetic High-Glare Model | Raw CFA with Gradient | DNG 1.4 | 480×320 | D50 | **REJECTED (As Intended)** | N/A | Quality gate tripped: CV > 0.05 on affected patch, preventing bad profile derivation |
| Synthetic Clipped Model | Saturated White Photosite | DNG 1.4 | 480×320 | D50 | **REJECTED (As Intended)** | N/A | Pre-demosaic saturation mask flagged highlight clipping |

## 3. Data Safety and Batch Integrity Verification

- **Atomic File Writing**: All outputs write to an exclusively created `.tmp-<pid>-<nanos>-<seq>.<ext>` file in the destination directory, are flushed, then renamed into place.
- **Overwrite Safety**: Existing files are skipped by default (`summary.skipped` count incremented). Overwrite succeeds only when `--overwrite` is explicitly specified.
- **Crash Recovery**: A failed write removes only the temporary file that write created. Startup never sweeps `.tmp-*` files, since they may belong to a concurrent run.
- **Non-destructive Invariant**: Source files are opened read-only and remain byte-identical after processing.
- **Failure Isolation**: A corrupted file in a batch logs an individual failure and continues processing remaining images without aborting or discarding successful outputs.

## 4. Gate assessment at the time

The author marked Milestone 4 GO on 2026-10-04 based on local builds and tests. The following are the recorded checks; no clean-machine run is documented, and packaging an installer has not been done:
- Desktop and CLI ran locally without a Python runtime in the application path.
- CLI and desktop derived and applied profiles using synthetic RAW DNGs and a rendered JPEG.
- The then-current regression suite passed locally. The suite has changed since this assessment; use current CI for current results.
