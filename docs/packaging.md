# Building and distributing the desktop app

## What is shipped

The `v0.1.1` pre-release contains a **Windows x64** NSIS installer (per user, with uninstaller), the portable CLI and desktop executables, a licenses zip, and `SHA256SUMS.txt`. All are unsigned. WebView2 is needed for the desktop app. Builds are made on Windows 11; the installer was tested by a silent install, a launch with one backend command, and a silent uninstall on the build machine, but not on a clean machine. The workspace CI builds the CLI on Windows, macOS and Linux, but it does **not** build the desktop app or installer. No macOS or Linux release binaries are published.

## Build from source

Rust 1.89+ (`apps/desktop/src-tauri` currently declares 1.90), Node.js 20+ with npm, and platform Tauri prerequisites are needed. On Windows use the MSVC Rust toolchain and WebView2. Python is not needed to run the app.

```bash
cd apps/desktop/frontend
npm ci
npm run build
cd ../src-tauri
cargo build --release
```

The desktop binary lands at `apps/desktop/src-tauri/target/release/colorbalance-desktop.exe` on Windows, or `colorbalance-desktop` on macOS/Linux. The `build.rs` script also calls `npm run build`; prebuilding with `npm ci` locks the frontend dependencies.

The CLI is a workspace member and builds separately:

```bash
cargo build --release -p colorbalance-cli
```

## Build the installer

The frontend dev dependency `@tauri-apps/cli` drives the bundler. `bundle` in `tauri.conf.json` targets NSIS only, installs per user, and copies `LICENSE` and the third-party notices into the app's `licenses/` folder. The first run downloads NSIS and a Tauri helper DLL from GitHub.

```bash
cd apps/desktop/frontend
npm ci
cd ../src-tauri
node ../frontend/node_modules/@tauri-apps/cli/tauri.js build --ci
```

The output is `target/release/bundle/nsis/ColorBalance Light-Table_<version>_x64-setup.exe` under the Cargo target directory. Stop any running `colorbalance-desktop.exe` first; Windows locks it and the build fails.

## Verify a downloaded binary

Download both the binary and `SHA256SUMS.txt` from the same release. On Windows:

```powershell
Get-FileHash .\colorbalance-desktop-windows-x64.exe -Algorithm SHA256
Get-FileHash .\colorbalance-cli-windows-x64.exe -Algorithm SHA256
```

Compare the printed hashes with `SHA256SUMS.txt`. Checksums detect accidental corruption; they do not authenticate an unsigned binary. Prefer downloading from the release page over mirrors.

Before wider distribution, a separate Windows machine should run the complete flow: load a RAW DNG and a JPEG, detect/check corners, inspect, derive, apply to a separate folder, verify TIFF and JPEG in an independent viewer, test cancellation and existing output, and confirm the Library can save/reopen a profile. That check has **not** been reported for this release. Signed installers and macOS/Linux packaging are not available.

See [third-party notices](../THIRD_PARTY_NOTICES.md) for rawler's LGPL-2.1 license and rebuild instructions.
