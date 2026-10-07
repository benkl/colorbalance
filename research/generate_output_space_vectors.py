"""Independent reference values for the TIFF output spaces.

Converts linear Rec.709 (sRGB primaries) probes to Display P3 and Adobe RGB
(1998) with colour-science, then clamps to [0, 1] in the target. The curves
are applied by colour's own CCTFs, so nothing here shares code with the OCIO
path under test in crates/colorbalance-core/src/output_space.rs.

Both targets are D65, so colour converts by matrix without chromatic
adaptation; OCIO routes through ACES2065-1 and adapts there, which is why the
Rust tests use a tolerance of 5e-4 instead of float epsilon.

Usage: python research/generate_output_space_vectors.py
"""

from __future__ import annotations

import json
from pathlib import Path

import colour
import numpy as np

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "tests" / "fixtures" / "reference" / "output-space-vectors.json"

SOURCE = colour.RGB_COLOURSPACES["sRGB"]
TARGETS = {"display-p3": "Display P3", "adobe-rgb": "Adobe RGB (1998)"}

PROBES = [
    [0.0, 0.0, 0.0],
    [0.18, 0.18, 0.18],
    [0.5, 0.5, 0.5],
    [1.0, 1.0, 1.0],
    [0.5, 0.0, 0.0],
    [0.0, 0.5, 0.0],
    [0.0, 0.0, 0.5],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.8, 0.4, 0.1],
    [0.05, 0.3, 0.6],
    [0.002, 0.001, 0.003],
    # Outside Rec.709: the wide target keeps what sRGB would clip.
    [-0.1, 0.6, 0.2],
    [1.2, 0.3, -0.05],
    [1.5, 1.5, 1.5],
]


def encode(name: str, linear: np.ndarray) -> np.ndarray:
    target = colour.RGB_COLOURSPACES[name]
    clamped = np.clip(linear, 0.0, 1.0)
    return np.clip(target.cctf_encoding(clamped), 0.0, 1.0)


def main() -> None:
    probes = np.array(PROBES)
    spaces = {}
    for key, name in TARGETS.items():
        target = colour.RGB_COLOURSPACES[name]
        linear = colour.RGB_to_RGB(
            probes,
            SOURCE,
            target,
            chromatic_adaptation_transform=None,
            apply_cctf_decoding=False,
            apply_cctf_encoding=False,
        )
        spaces[key] = {
            "colour-space": name,
            "encoded": encode(name, linear).tolist(),
            "linear-unclamped": linear.tolist(),
        }
    OUT.write_text(
        json.dumps(
            {
                "generator": "research/generate_output_space_vectors.py",
                "colour-version": colour.__version__,
                "source": "linear Rec.709 (sRGB primaries, D65), unclamped",
                "probes": PROBES,
                "spaces": spaces,
            },
            indent=1,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
