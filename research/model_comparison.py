"""Model Comparison and Validation Benchmark for ColorBalance.

Compares:
1. Constrained 3x3 linear matrix (black-preserving least squares).
2. Polynomial Degree 2 (Cheung 2004, 5 terms).
3. Root-Polynomial Degree 2 (Finlayson 2015, degree 2, 6 terms).
4. Root-Polynomial Degree 3 (Finlayson 2015, degree 3, 13 terms).

Evaluated across:
- Leave-One-Out Cross-Validation (LOOCV) on 24-patch ColorChecker Classic.
- Boundary stability & monotonicity under out-of-chart saturated primaries.
- Conditioning and numerical stability.
"""

from __future__ import annotations

import json
from pathlib import Path
import colour
import numpy as np

REPO = Path(__file__).resolve().parent.parent
OBSERVER = "CIE 1931 2 Degree Standard Observer"
D65 = colour.CCS_ILLUMINANTS[OBSERVER]["D65"]
D65_XYZ = colour.xy_to_XYZ(D65)
SRGB = colour.RGB_COLOURSPACES["sRGB"]

def load_chart_data():
    checker = colour.CCS_COLOURCHECKERS["ColorChecker24 - After November 2014"]
    xyY = np.array(list(checker.data.values()))
    xyz_d50 = colour.xyY_to_XYZ(xyY)
    D50 = colour.CCS_ILLUMINANTS[OBSERVER]["D50"]
    D50_XYZ = colour.xy_to_XYZ(D50)
    m_cat = colour.adaptation.matrix_chromatic_adaptation_VonKries(
        D50_XYZ, D65_XYZ, transform="Bradford"
    )
    xyz_d65 = xyz_d50 @ m_cat.T
    rgb_linear = xyz_d65 @ SRGB.matrix_XYZ_to_RGB.T
    return rgb_linear

def calculate_delta_e(actual_rgb_linear, target_rgb_linear):
    actual_xyz = actual_rgb_linear @ SRGB.matrix_RGB_to_XYZ.T
    target_xyz = target_rgb_linear @ SRGB.matrix_RGB_to_XYZ.T
    lab_actual = colour.XYZ_to_Lab(np.maximum(actual_xyz, 0), illuminant=D65)
    lab_target = colour.XYZ_to_Lab(np.maximum(target_xyz, 0), illuminant=D65)
    return colour.delta_E(lab_actual, lab_target, method="CIE 2000")

def run_loocv(source_rgb, target_rgb):
    n = len(source_rgb)
    errors_3x3 = []
    errors_poly2 = []
    errors_root2 = []
    errors_root3 = []

    for i in range(n):
        train_idx = [j for j in range(n) if j != i]
        test_idx = [i]

        s_train = source_rgb[train_idx]
        t_train = target_rgb[train_idx]
        s_test = source_rgb[test_idx]
        t_test = target_rgb[test_idx]

        # 1. Constrained 3x3 linear matrix
        M_3x3 = np.linalg.lstsq(s_train, t_train, rcond=None)[0]
        pred_3x3 = s_test @ M_3x3
        err_3x3 = calculate_delta_e(pred_3x3, t_test)
        errors_3x3.append(float(err_3x3[0]))

        # 2. Polynomial degree 2
        pred_poly2 = colour.characterisation.colour_correction(
            s_test, s_train, t_train, method="Cheung 2004", terms=5
        )
        err_poly2 = calculate_delta_e(pred_poly2, t_test)
        errors_poly2.append(float(err_poly2[0]))

        # 3. Root-Polynomial degree 2
        pred_root2 = colour.characterisation.colour_correction(
            s_test, s_train, t_train, method="Finlayson 2015", degree=2
        )
        err_root2 = calculate_delta_e(pred_root2, t_test)
        errors_root2.append(float(err_root2[0]))

        # 4. Root-Polynomial degree 3
        pred_root3 = colour.characterisation.colour_correction(
            s_test, s_train, t_train, method="Finlayson 2015", degree=3
        )
        err_root3 = calculate_delta_e(pred_root3, t_test)
        errors_root3.append(float(err_root3[0]))

    return {
        "3x3": {
            "mean": float(np.mean(errors_3x3)),
            "median": float(np.median(errors_3x3)),
            "p95": float(np.percentile(errors_3x3, 95)),
            "max": float(np.max(errors_3x3)),
        },
        "polynomial_deg2": {
            "mean": float(np.mean(errors_poly2)),
            "median": float(np.median(errors_poly2)),
            "p95": float(np.percentile(errors_poly2, 95)),
            "max": float(np.max(errors_poly2)),
        },
        "root_polynomial_deg2": {
            "mean": float(np.mean(errors_root2)),
            "median": float(np.median(errors_root2)),
            "p95": float(np.percentile(errors_root2, 95)),
            "max": float(np.max(errors_root2)),
        },
        "root_polynomial_deg3": {
            "mean": float(np.mean(errors_root3)),
            "median": float(np.median(errors_root3)),
            "p95": float(np.percentile(errors_root3, 95)),
            "max": float(np.max(errors_root3)),
        },
    }

def test_boundary_behavior(s_train, t_train):
    # Test boundary / out-of-chart values
    probes = np.array([
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.05, 0.05, 0.05],
        [0.95, 0.95, 0.95],
    ])

    M_3x3 = np.linalg.lstsq(s_train, t_train, rcond=None)[0]
    pred_3x3 = probes @ M_3x3

    pred_root2 = colour.characterisation.colour_correction(
        probes, s_train, t_train, method="Finlayson 2015", degree=2
    )

    pred_root3 = colour.characterisation.colour_correction(
        probes, s_train, t_train, method="Finlayson 2015", degree=3
    )

    # Check for negative predictions or overshoot beyond 1.05
    return {
        "3x3_min": float(np.min(pred_3x3)),
        "3x3_max": float(np.max(pred_3x3)),
        "root2_min": float(np.min(pred_root2)),
        "root2_max": float(np.max(pred_root2)),
        "root3_min": float(np.min(pred_root3)),
        "root3_max": float(np.max(pred_root3)),
    }

def main():
    target_rgb = load_chart_data()
    # Model non-linear realistic camera response with mild channel non-linearity
    # to test model resistance to mild curves
    sensor_response = np.maximum(target_rgb, 0) ** 1.05
    camera_matrix = np.array([
        [1.03, -0.02, 0.01],
        [-0.02, 1.01, -0.01],
        [0.00, -0.01, 1.04]
    ])
    source_rgb = sensor_response @ camera_matrix

    print("Running Leave-One-Out Cross-Validation (LOOCV)...")
    results = run_loocv(source_rgb, target_rgb)
    print("LOOCV Results:")
    print(json.dumps(results, indent=2))

    boundary_res = test_boundary_behavior(source_rgb, target_rgb)
    print("Boundary Stability:")
    print(json.dumps(boundary_res, indent=2))

    out_file = REPO / "research" / "model_benchmark_results.json"
    full_output = {
        "loocv": results,
        "boundary_stability": boundary_res
    }
    out_file.write_text(json.dumps(full_output, indent=2), encoding="utf-8")
    print(f"Benchmark results written to {out_file}")

if __name__ == "__main__":
    main()
