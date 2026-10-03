"""Generate independently computed reference data for ColorBalance tests.

Runs with colour-science and writes JSON files under tests/fixtures/reference/.
The Rust implementation must match these values within the tolerances stated
in docs/CONTRACTS.md. This script is the authority; the Rust code under test
never generates its own expected values.

Usage: python research/generate_reference_data.py
"""

from __future__ import annotations

import json
from pathlib import Path

import colour
import numpy as np
REPO = Path(__file__).resolve().parent.parent
OUT_DATASETS = REPO / "crates" / "colorbalance-core" / "data"
OUT_VECTORS = REPO / "tests" / "fixtures" / "reference"


OBSERVER = "CIE 1931 2 Degree Standard Observer"
ILLUMINANTS = colour.CCS_ILLUMINANTS[OBSERVER]
D50 = ILLUMINANTS["D50"]
D65 = ILLUMINANTS["D65"]
SRGB = colour.RGB_COLOURSPACES["sRGB"]
D50_XYZ = colour.xy_to_XYZ(D50)
D65_XYZ = colour.xy_to_XYZ(D65)

PATCH_ORDER = [
    "dark-skin", "light-skin", "blue-sky", "foliage", "blue-flower",
    "bluish-green", "orange", "purplish-blue", "moderate-red", "purple",
    "yellow-green", "orange-yellow", "blue", "green", "red", "yellow",
    "magenta", "cyan", "white", "neutral-8", "neutral-6-5", "neutral-5",
    "neutral-3-5", "black",
]

DATASETS = {
    "classic-before-november-2014": "ColorChecker24 - Before November 2014",
    "classic-from-november-2014": "ColorChecker24 - After November 2014",
}

# Sharma, Wu, Dalal (2005) CIEDE2000 test pairs (a1..a? subset with exact
# published values), used as an external cross-check of our own implementation.
SHARMA_PAIRS = [
    # (Lab1, Lab2, expected dE00)
    ((50.0, 2.6772, -79.7751), (50.0, 0.0, -82.7485), 2.0425),
    ((50.0, 3.1571, -77.2803), (50.0, 0.0, -82.7485), 2.8615),
    ((50.0, 2.8361, -74.02), (50.0, 0.0, -82.7485), 3.4412),
    ((50.0, -1.3802, -84.2814), (50.0, 0.0, -82.7485), 1.0),
    ((50.0, -1.1848, -84.8006), (50.0, 0.0, -82.7485), 1.0),
    ((50.0, -0.9009, -85.5211), (50.0, 0.0, -82.7485), 1.0),
    ((50.0, 0.0, 0.0), (50.0, -1.0, 2.0), 2.3669),
    ((50.0, -1.0, 2.0), (50.0, 0.0, 0.0), 2.3669),
    ((50.0, 2.49, -0.001), (50.0, -2.49, 0.0009), 7.1792),
    ((50.0, 2.49, -0.001), (50.0, -2.49, 0.001), 7.1792),
    ((50.0, 2.49, -0.001), (50.0, -2.49, 0.0011), 7.2195),
    ((50.0, 2.49, -0.001), (50.0, -2.49, 0.0012), 7.2195),
    ((50.0, -0.001, 2.49), (50.0, 0.0009, -2.49), 4.8045),
    ((50.0, -0.001, 2.49), (50.0, 0.001, -2.49), 4.8045),
    ((50.0, -0.001, 2.49), (50.0, 0.0011, -2.49), 4.7461),
    ((50.0, 2.5, 0.0), (50.0, 0.0, -2.5), 4.3065),
    ((50.0, 2.5, 0.0), (73.0, 25.0, -18.0), 27.1492),
    ((50.0, 2.5, 0.0), (61.0, -5.0, 29.0), 22.8977),
    ((50.0, 2.5, 0.0), (56.0, -27.0, -3.0), 31.9030),
    ((50.0, 2.5, 0.0), (58.0, 24.0, 15.0), 19.4535),
    ((50.0, 2.5, 0.0), (50.0, 3.1736, 0.5854), 1.0),
    ((50.0, 2.5, 0.0), (50.0, 3.2972, 0.0), 1.0),
    ((50.0, 2.5, 0.0), (50.0, 1.8634, 0.5757), 1.0),
    ((50.0, 2.5, 0.0), (50.0, 3.2592, 0.335), 1.0),
    ((60.2574, -34.0099, 36.2677), (60.4626, -34.1751, 39.4387), 1.2644),
    ((63.0109, -31.0961, -5.8663), (62.8187, -29.7946, -4.0864), 1.2630),
    ((61.2901, 3.7196, -5.3901), (61.4292, 2.248, -4.962), 1.8731),
    ((35.0831, -44.1164, 3.7933), (35.0232, -40.0716, 1.5901), 1.8645),
    ((22.7233, 20.0904, -46.694), (23.0331, 14.973, -42.5619), 2.0373),
    ((36.4612, 47.858, 18.3852), (36.2715, 50.5065, 21.2231), 1.4146),
    ((90.8027, -2.0831, 1.441), (91.1528, -1.6435, 0.0447), 1.4441),
    ((90.9257, -0.5406, -0.9208), (88.6381, -0.8985, -0.7239), 1.5381),
    ((6.7747, -0.2908, -2.4247), (5.8714, -0.0985, -2.2286), 0.6377),
    ((2.0776, 0.0795, -1.135), (0.9033, -0.0636, -0.5514), 0.9082),
]


def as_list(a: np.ndarray) -> list[float]:
    return [round(float(x), 10) for x in np.asarray(a).ravel()]


def chart_dataset(name: str) -> dict:
    checker = colour.CCS_COLOURCHECKERS[name]
    sd_order = list(checker.data.keys())
    assert len(sd_order) == 24, name
    xyY = np.array([checker.data[k] for k in sd_order])
    xyz_d50 = colour.xyY_to_XYZ(xyY)
    # Colour-science orders ColorChecker data in the standard reading order,
    # but verify by the characteristic neutral row once mapped.
    m = colour.adaptation.matrix_chromatic_adaptation_VonKries(
        D50_XYZ, D65_XYZ, transform="Bradford"
    )
    xyz_d65 = xyz_d50 @ m.T
    rgb_linear = xyz_d65 @ SRGB.matrix_XYZ_to_RGB.T
    lab = colour.XYZ_to_Lab(xyz_d65, illuminant=D65)
    return {
        "source": name,
        "illuminant": "D50",
        "observer": OBSERVER,
        "adaptation": "Bradford",
        "targetWhitePoint": "D65",
        "patches": [
            {
                "name": PATCH_ORDER[i],
                "sdName": sd_order[i],
                "xyY": as_list(xyY[i]),
                "xyzD50": as_list(xyz_d50[i]),
                "xyzD65": as_list(xyz_d65[i]),
                "linearSrgbD65": as_list(rgb_linear[i]),
                "labD65": as_list(lab[i]),
            }
            for i in range(24)
        ],
    }


def main() -> None:
    OUT_DATASETS.mkdir(parents=True, exist_ok=True)

    datasets = {
        revision: chart_dataset(source)
        for revision, source in DATASETS.items()
    }
    (OUT_DATASETS / "chart-datasets.json").write_text(
        json.dumps(datasets, indent=2) + "\n", encoding="utf-8"
    )

    # Cross-check the CIEDE2000 implementation against the published pairs.
    de00 = []
    for i, (lab1, lab2, expected) in enumerate(SHARMA_PAIRS):
        got = float(
            colour.delta_E(np.array(lab1), np.array(lab2), method="CIE 2000")
        )
        assert abs(got - expected) < 1e-4, (i, got, expected)
        de00.append({"lab1": list(lab1), "lab2": list(lab2), "expected": expected})

    # sRGB encoding boundaries and a spread of linear values.
    linear = [
        0.0, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.02, 0.04045 / 12.92,
        0.04045 / 12.92 + 1e-9, 0.05, 0.1, 0.18, 0.25, 0.33, 0.5, 0.75, 0.9,
        0.99, 1.0,
    ]
    encoded = [float(colour.cctf_encoding(v)) for v in linear]

    # Bradford adaptation spot vectors.
    xyz_inputs = [
        [0.0, 0.0, 0.0],
        [0.5, 0.5, 0.5],
        [0.9504559270516716, 1.0, 1.089057750759878],
        [0.1234, 0.2345, 0.3456],
        [1.0, 0.25, 0.1],
        [0.2, 0.7, 0.1],
    ]
    m = colour.adaptation.matrix_chromatic_adaptation_VonKries(
        D50_XYZ, D65_XYZ, transform="Bradford"
    )
    adapted = [as_list(np.array(x) @ m.T) for x in xyz_inputs]
    srgb_targets = [
        as_list(np.array(x) @ SRGB.matrix_XYZ_to_RGB.T) for x in xyz_inputs
    ]
    labs = [
        as_list(colour.XYZ_to_Lab(np.array(x), illuminant=D65))
        for x in xyz_inputs
    ]

    vectors = {
        "observer": OBSERVER,
        "d50": as_list(D50),
        "d65": as_list(D65),
        "bradfordD50toD65": [as_list(row) for row in m],
        "xyzToLinearSrgbD65": [as_list(row) for row in SRGB.matrix_XYZ_to_RGB],
        "linearSrgbToXyzD65": [as_list(row) for row in SRGB.matrix_RGB_to_XYZ],
        "bradfordCases": [
            {"xyzIn": x, "xyzD65": a}
            for x, a in zip(xyz_inputs, adapted)
        ],
        "xyzToSrgbCases": [
            {"xyzD65": x, "linearSrgb": s} for x, s in zip(xyz_inputs, srgb_targets)
        ],
        "xyzToLabCases": [
            {"xyzD65": x, "lab": l} for x, l in zip(xyz_inputs, labs)
        ],
        "srgbEncodeCases": [
            {"linear": v, "encoded": e} for v, e in zip(linear, encoded)
        ],
        "deltaE2000Cases": de00,
    }
    OUT_VECTORS.mkdir(parents=True, exist_ok=True)
    (OUT_VECTORS / "color-vectors.json").write_text(
        json.dumps(vectors, indent=2) + "\n", encoding="utf-8"
    )
    print(f"wrote {OUT_DATASETS / 'chart-datasets.json'}")
    print(f"wrote {OUT_VECTORS / 'color-vectors.json'}")


if __name__ == "__main__":
    main()
