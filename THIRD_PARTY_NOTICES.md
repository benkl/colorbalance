# Third-party notices

ColorBalance's own code is licensed `MIT OR Apache-2.0` (see `LICENSE-MIT` and `LICENSE-APACHE`). The binaries link third-party crates under their own licenses. The full list is `cargo metadata` over `Cargo.lock` and `apps/desktop/src-tauri/Cargo.lock`. This file records the ones that are not plain MIT/Apache-2.0/BSD/Zlib.

License texts shipped with the source: [`rawler` LGPL-2.1](third_party/licenses/rawler-LGPL-2.1.txt), [MPL-2.0](third_party/licenses/cssparser-MPL-2.0.txt), [Independent JPEG Group](third_party/licenses/jpeg-encoder-IJG.txt).

## rawler 0.8.0: LGPL-2.1

RAW decoding uses the [`rawler`](https://crates.io/crates/rawler) crate, licensed LGPL-2.1. It is statically linked into `colorbalance` (CLI) and `colorbalance-desktop`.

- Exact source: crates.io `rawler 0.8.0`, pinned by `Cargo.lock`.
- To rebuild the binaries against a modified `rawler`, clone this repository, point the `rawler` dependency in `crates/colorbalance-raw/Cargo.toml` at your version (a `[patch.crates-io]` entry also works), and run `cargo build --release`. The complete corresponding source of this project is the repository you are reading.
- `colorbalance-core`, the color engine, does not depend on `rawler`.

## MPL-2.0 (desktop app only)

`cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors` come in through Tauri/wry. They are unmodified crates.io releases. Their source is available at crates.io under the versions in `apps/desktop/src-tauri/Cargo.lock`.

## Other notable licenses

| Crate | License | Note |
| --- | --- | --- |
| `jpeg-encoder` | (MIT OR Apache-2.0) AND IJG | JPEG output. The IJG notice applies: this software is based in part on the work of the Independent JPEG Group. |
| `icu_*`, `unicode-ident` and relatives | Unicode-3.0 | Permissive. |
| `xxhash-rust` | BSL-1.0 | Permissive. |
| `r-efi` | MIT OR Apache-2.0 OR LGPL-2.1-or-later | Used under MIT/Apache-2.0. |

The desktop UI loads fonts from Google Fonts at runtime (see `tauri.conf.json` CSP). Those fonts carry their own licenses and are not bundled.

## Not included

- Adobe, Lightroom and Camera Raw are trademarks of Adobe. The DCP writer is written from the public DNG specification and contains no Adobe SDK code.
- ColorChecker is a trademark of X-Rite / Calibrite. This project is not affiliated with them. The chart reference values in `crates/colorbalance-core/data/chart-datasets.json` are measured colorimetric data (xyY under D50) taken from the `colour-science` 0.4.4 package's `CCS_COLOURCHECKERS` tables by `research/generate_reference_data.py`, then converted. The `source` field of each dataset names the table it came from. `colour-science` is BSD-3-Clause and is a research-time tool only; it is not linked into any binary.
