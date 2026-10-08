"""Independent reference values for the DNG Camera Profile (DCP) export.

Maps ColorBalance's fitted stages onto DCP matrices using only numpy and
colour-science, so nothing here shares code with
crates/colorbalance-core/src/interchange/dcp.rs.

Fitted model (profile.rs): n = rgb / (exposure_scale * channel_scale), then
out = n @ M, linear Rec.709 (sRGB primaries, D65). In column form the
camera-to-linear-sRGB map is M^T.

DCP model (DNG 1.6, chapter 6): CameraToXYZ_D50 = FM * D * inverse(AB * CC),
D = inverse(diag(ReferenceNeutral)). With AB = CC = identity and the user's
white balance equal to ReferenceNeutral, camera values map by FM * diag(1/N).

Closed form used by the export:
  T  = Bradford(D65 -> D50) * sRGB_to_XYZ * M^T * diag(1 / (exposure * channel))
       raw camera values -> XYZ D50, exactly the fitted model
  w  = inverse(T) * D50_white        the raw vector the model maps to D50 white
  N  = w / max(w)                     ReferenceNeutral
  FM = T * diag(w)                    camera (1,1,1) -> D50 white exactly
  CM = diag(N) * inverse(FM)          ColorMatrix1, XYZ D50 -> camera, CM*D50 = N
  EV = -log2(max(w))                  BaselineExposureOffset: FM*(r/N) = max(w)*T*r,
                                      so this EV restores the fitted model's scale

FM * 1 = D50 holds by construction, so a reader that renormalises the
ForwardMatrix so camera ones map to D50 white leaves it unchanged. At
white balance N the DCP reproduces the fitted model exactly. The residual to
measure is the user's real white balance: the eyedropper on the chart gray
gives raw proportional to channel_scale, not to N.

Usage: python research/generate_dcp_vectors.py
Writes tests/fixtures/reference/dcp-vectors.json and prints, per sample
profile, the CIEDE2000 gap between the fitted model and the DCP when white
balance is taken from the chart gray (white patch luminance matched).
"""

from __future__ import annotations

import json
from pathlib import Path

import colour
import numpy as np

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "tests" / "fixtures" / "reference" / "dcp-vectors.json"
DATASETS = REPO / "crates" / "colorbalance-core" / "data" / "chart-datasets.json"

SRGB = colour.RGB_COLOURSPACES["sRGB"]
S2X = SRGB.matrix_RGB_to_XYZ
ILLUM = colour.CCS_ILLUMINANTS["CIE 1931 2 Degree Standard Observer"]
D65_XY = ILLUM["D65"]
D50_XY = ILLUM["D50"]
D65 = colour.xy_to_XYZ(D65_XY)
D50 = colour.xy_to_XYZ(D50_XY)
B65_TO_50 = colour.adaptation.matrix_chromatic_adaptation_VonKries(D65, D50, transform="Bradford")

PROFILES = {
    "samsung-raw": REPO / "colorbalance_profile.cbprofile.json",
    "rendered-jpeg": REPO / "apps" / "desktop" / "src-tauri" / "colorbalance_profile.cbprofile.json",
}


def stages(profile: dict) -> tuple[float, np.ndarray, np.ndarray]:
    t = profile["transform"]
    return (
        float(t["exposure-scale"]),
        np.array(t["channel-scale"], dtype=float),
        np.array(t["matrix"], dtype=float),
    )


def dcp_matrices(exposure: float, channel: np.ndarray, matrix: np.ndarray) -> dict:
    t = B65_TO_50 @ S2X @ matrix.T @ np.diag(1.0 / (exposure * channel))
    w = np.linalg.solve(t, D50)
    n = w / w.max()
    fm = t @ np.diag(w)
    cm = np.diag(n) @ np.linalg.inv(fm)
    return {
        "t": t,
        "w": w,
        "reference_neutral": n,
        "forward_matrix": fm,
        "color_matrix": cm,
        "baseline_exposure_offset": float(-np.log2(w.max())),
    }


def chart_refs(profile: dict) -> np.ndarray:
    datasets = json.loads(DATASETS.read_text())
    key = profile["chart-revision"]
    data = datasets[key] if key in datasets else next(iter(datasets.values()))
    return np.array([p["linearSrgbD65"] for p in data["patches"]], dtype=float)


def lab(xyz_d50: np.ndarray) -> np.ndarray:
    return colour.XYZ_to_Lab(xyz_d50, D50_XY)


def drift(profile: dict) -> dict:
    """Gap between model and DCP at the white balance a user gets from the gray.

    Two white-balance sources: the white patch raw direction (eyedropper on
    the chart) and channel_scale (what the fit calls neutral).
    """
    exposure, channel, matrix = stages(profile)
    m = dcp_matrices(exposure, channel, matrix)
    refs = chart_refs(profile)
    raws = np.array([exposure * channel * np.linalg.solve(matrix.T, r) for r in refs])
    model = np.array([m["t"] @ r for r in raws])
    out = {}
    for label, rho in (
        ("eyedropper", raws[18] / raws[18].max()),
        ("channel-scale", channel / channel.max()),
    ):
        dcp = np.array([m["forward_matrix"] @ (r / rho) for r in raws])
        # Match the white patch luminance, as an exposure slider would.
        dcp = dcp * (model[18][1] / dcp[18][1])
        de = colour.delta_E(lab(model), lab(dcp), method="CIE 2000")
        out[label] = {
            "mean": float(de.mean()),
            "max": float(de.max()),
            "neutral_max": float(de[18:24].max()),
        }
    return out


def case(name: str, exposure: float, channel: np.ndarray, matrix: np.ndarray) -> dict:
    m = dcp_matrices(exposure, channel, matrix)
    return {
        "name": name,
        "exposure_scale": exposure,
        "channel_scale": channel.tolist(),
        "matrix": matrix.tolist(),
        "reference_neutral": m["reference_neutral"].tolist(),
        "forward_matrix": m["forward_matrix"].tolist(),
        "color_matrix": m["color_matrix"].tolist(),
        "baseline_exposure_offset": m["baseline_exposure_offset"],
    }


def main() -> None:
    cases = []
    for name, path in PROFILES.items():
        if not path.exists():
            continue
        profile = json.loads(path.read_text())
        exposure, channel, matrix = stages(profile)
        d = drift(profile)
        for label, v in d.items():
            print(
                f"{name} [{label}]: dE00 mean {v['mean']:.4f}  max {v['max']:.4f}  "
                f"neutral max {v['neutral_max']:.4f}"
            )
        c = case(name, exposure, channel, matrix)
        c["drift_de00_eyedropper_max"] = d["eyedropper"]["max"]
        c["drift_de00_channel_scale_max"] = d["channel-scale"]["max"]
        cases.append(c)

    exposure = 0.5
    channel = np.array([0.8, 1.0, 0.7])
    matrix = np.array([[1.2, -0.1, -0.1], [-0.2, 1.3, -0.1], [-0.05, -0.15, 1.2]])
    cases.append(case("synthetic", exposure, channel, matrix))

    for c in cases:
        fm = np.array(c["forward_matrix"])
        assert np.allclose(fm @ np.ones(3), D50, atol=1e-12), c["name"]
        cm = np.array(c["color_matrix"])
        assert np.allclose(cm @ D50, c["reference_neutral"], atol=1e-12), c["name"]

    OUT.write_text(json.dumps({"d50_xyz": D50.tolist(), "cases": cases}, indent=2) + "\n")
    print("wrote", OUT.relative_to(REPO))


if __name__ == "__main__":
    main()
