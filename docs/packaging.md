# Desktop Application Release and Packaging Guide

This document defines the build, verification, and distribution process for standalone `colorbalance-desktop` releases on Windows, macOS, and Linux.

## 1. Supported Platform Tiers

| Platform | Arch | Target | Status |
| --- | --- | --- | --- |
| Windows 10/11 | x64 | `x86_64-pc-windows-msvc` | Supported (Primary) |
| macOS 12+ | x64 / ARM64 | `x86_64-apple-darwin` / `aarch64-apple-darwin` | Supported |
| Linux | x64 | `x86_64-unknown-linux-gnu` | Supported |

## 2. Prerequisites

- **Rust toolchain**: 1.89+ stable with MSVC toolchain on Windows.
- **Node.js**: 20+ with npm.
- **WebView Runtime**: Evergreen Microsoft Edge WebView2 on Windows (pre-installed on Windows 10/11).
- **System Python is NOT required**: the binary is standalone native Rust and statically bundles all dependencies.

## 3. Building the Release Binary

### Step 1: Build the production frontend bundle

```bash
cd apps/desktop/frontend
npm ci
npm run build
```

This compiles TypeScript and packages the React SPA assets to `apps/desktop/frontend/dist`.

### Step 2: Build the standalone release binary

From the repository root or `apps/desktop/src-tauri`:

```bash
cargo build --release --manifest-path apps/desktop/src-tauri/Cargo.toml
```

The output executable is generated at:

```text
apps/desktop/src-tauri/target/release/colorbalance-desktop.exe (Windows)
apps/desktop/src-tauri/target/release/colorbalance-desktop (macOS / Linux)
```

## 4. Generating Artifact Checksums

```bash
# Windows PowerShell
Get-FileHash apps/desktop/src-tauri/target/release/colorbalance-desktop.exe -Algorithm SHA256

# Bash
sha256sum apps/desktop/src-tauri/target/release/colorbalance-desktop.exe > colorbalance-desktop.sha256
```

## 5. Verification on Clean Machines

1. Transfer `colorbalance-desktop.exe` to a clean Windows environment without developer tools installed.
2. Verify checksum matches published hash.
3. Launch `colorbalance-desktop.exe`.
4. Drag and drop a ColorChecker frame (e.g. `20261003_183314.jpg` or a RAW `.dng`).
5. Select physical chart revision and run **DERIVE COLOR PROFILE**.
6. Select input directory and destination, and execute batch calibration.
7. Verify generated 16-bit TIFF files open in standard image viewers (Windows Photos, IrfanView, Adobe Photoshop).

## 6. Code Signing & Packaging (CI Pipeline)

- No installer is produced. The release artifact is the bare executable from `cargo build --release`. Installer bundling (`.msi`, `.dmg`, AppImage) would need the Tauri CLI, which is not a project dependency, and the bundler configuration was removed with it.
- Code signing utilizes Windows Authenticode with EV Certificate or Azure Trusted Signing during GitHub Actions release workflows.
