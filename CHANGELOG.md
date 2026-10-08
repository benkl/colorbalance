# Changelog

## v0.1.0 — pre-release (2026-10-08)

First public Windows x64 CLI and desktop binaries. Unsigned, no installer, no clean-machine verification.

- Derive a measured ColorChecker transform from RAW; JPEG/PNG is a flagged quick-and-dirty approximation. Quality gates, reports and a tamper-evident profile keep the fit inspectable.
- Batch apply to new 16-bit TIFF or 8-bit JPEG files in sRGB, Display P3 or Adobe RGB (1998). Inputs are never modified; output is committed atomically.
- Desktop: light-table corners, chart detection, reference inspection, per-patch validation, before/after comparison, Library, batch preflight and chart check.
- CLI: `inspect`, `derive`, `apply`, `export`, `decode-contract`.
- Interchange: CLF, `.cube`, and RAW-only matrix `.dcp` export. Lightroom/Camera Raw import has not been tested.
- Color core builds for wasm32; there is no browser app or hosted service.

**Known limits:** real RAW verification is one Samsung Galaxy S25 LinearRaw DNG plus synthetic fixtures; other camera formats are untested here. CLI chart detection is not wired. DCP is single-illuminant and matrix-only; it has not been round-tripped in Lightroom. The desktop release is Windows x64 only, unsigned, and requires WebView2. See [README](README.md#reality-check) and [Lightroom export plan](docs/LIGHTROOM_EXPORT_PLAN.md).

The release includes `LICENSE-MIT`, `LICENSE-APACHE` and `THIRD_PARTY_NOTICES.md` in the source repository. The binaries include `rawler` under LGPL-2.1; see those notices before redistribution.
