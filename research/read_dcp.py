"""Independent DCP reader used to check what colorbalance writes.

Written from the DNG 1.6 spec only. Parses the header and the single IFD, then
checks the fitted-model identities against the profile JSON:

  * header is 'II', 0x4352, IFD offset
  * IFD entries are in ascending tag order, out-of-line data is word aligned
  * ForwardMatrix1 maps camera (1,1,1) to D50 white
  * ColorMatrix1 * D50 white = the neutral implied by ForwardMatrix1
  * at white balance N the DCP reproduces the fitted model for chart patches

Usage: python research/read_dcp.py file.dcp [profile.cbprofile.json]
"""

from __future__ import annotations

import json
import struct
import sys
from pathlib import Path

import colour
import numpy as np

SIZES = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 10: 8}
NAMES = {
    50706: "DNGVersion",
    50707: "DNGBackwardVersion",
    50708: "UniqueCameraModel",
    50721: "ColorMatrix1",
    50778: "CalibrationIlluminant1",
    50936: "ProfileName",
    50941: "ProfileEmbedPolicy",
    50964: "ForwardMatrix1",
    51109: "BaselineExposureOffset",
}


def parse(data: bytes) -> dict[int, tuple[int, int, bytes]]:
    assert data[:2] == b"II", "not little endian"
    magic, ifd = struct.unpack_from("<HI", data, 2)
    assert magic == 0x4352, hex(magic)
    (n,) = struct.unpack_from("<H", data, ifd)
    entries, last = {}, 0
    for i in range(n):
        tag, kind, count, field = struct.unpack_from("<HHI4s", data, ifd + 2 + 12 * i)
        assert tag > last, "tags must ascend"
        last = tag
        size = SIZES[kind] * count
        if size <= 4:
            raw = field[:size]
        else:
            (off,) = struct.unpack("<I", field)
            assert off % 2 == 0, "unaligned offset"
            raw = data[off : off + size]
            assert len(raw) == size, "truncated"
        entries[tag] = (kind, count, raw)
    (nxt,) = struct.unpack_from("<I", data, ifd + 2 + 12 * n)
    assert nxt == 0
    return entries


def rationals(raw: bytes) -> np.ndarray:
    vals = struct.unpack(f"<{len(raw) // 4}i", raw)
    return np.array([vals[i] / vals[i + 1] for i in range(0, len(vals), 2)])


def main() -> None:
    dcp = Path(sys.argv[1]).read_bytes()
    e = parse(dcp)
    for tag, (kind, count, raw) in e.items():
        name = NAMES.get(tag, str(tag))
        if kind == 2:
            shown = raw[:-1].decode()
        elif kind == 10:
            shown = np.round(rationals(raw), 6).tolist()
        else:
            shown = raw.hex()
        print(f"{tag:>6} {name:<24} type {kind:<2} count {count:<2} {shown}")

    d50 = colour.xy_to_XYZ(colour.CCS_ILLUMINANTS["CIE 1931 2 Degree Standard Observer"]["D50"])
    fm = rationals(e[50964][2]).reshape(3, 3)
    cm = rationals(e[50721][2]).reshape(3, 3)
    ev = rationals(e[51109][2])[0]
    print("FM*1 - D50:", np.abs(fm @ np.ones(3) - d50).max())
    n = cm @ d50
    print("neutral from CM:", n.round(6), "max", n.max().round(6))
    assert abs(n.max() - 1.0) < 1e-5

    if len(sys.argv) > 2:
        p = json.loads(Path(sys.argv[2]).read_text())
        t = p["transform"]
        exposure = t["exposure-scale"]
        channel = np.array(t["channel-scale"])
        m = np.array(t["matrix"])
        s2x = colour.RGB_COLOURSPACES["sRGB"].matrix_RGB_to_XYZ
        d65 = colour.xy_to_XYZ(colour.CCS_ILLUMINANTS["CIE 1931 2 Degree Standard Observer"]["D65"])
        b = colour.adaptation.matrix_chromatic_adaptation_VonKries(d65, d50, transform="Bradford")
        rng = np.random.default_rng(1)
        worst = 0.0
        for _ in range(200):
            raw = rng.uniform(0.01, 0.9, 3)
            model = b @ s2x @ m.T @ (raw / (exposure * channel))
            reader = fm @ (raw / n) * 2.0**ev
            worst = max(worst, np.abs(model - reader).max() / max(model.max(), 1e-9))
        print(f"max relative model-vs-DCP error at white balance N: {worst:.2e}")
        assert worst < 2e-3


if __name__ == "__main__":
    main()
