//! Color conversion and color-difference calculations.

/// D50 reference white in XYZ, with Y normalized to one.
pub const D50_XYZ: [f64; 3] = [0.9642956764295677, 1.0, 0.8251046025104602];
/// D65 reference white in XYZ, with Y normalized to one.
pub const D65_XYZ: [f64; 3] = [0.9504559270516716, 1.0, 1.0890577507598784];

const XYZ_TO_RGB: [[f64; 3]; 3] = [
    [3.2406, -1.5372, -0.4986],
    [-0.9689, 1.8758, 0.0415],
    [0.0557, -0.2040, 1.0570],
];

/// Multiplies a row vector by a 3x3 matrix.
pub fn mat_vec(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        v[0] * m[0][0] + v[1] * m[1][0] + v[2] * m[2][0],
        v[0] * m[0][1] + v[1] * m[1][1] + v[2] * m[2][1],
        v[0] * m[0][2] + v[1] * m[1][2] + v[2] * m[2][2],
    ]
}

fn column_mat_vec(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn inverse(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let determinant = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let cofactors = [
        [
            m[1][1] * m[2][2] - m[1][2] * m[2][1],
            m[0][2] * m[2][1] - m[0][1] * m[2][2],
            m[0][1] * m[1][2] - m[0][2] * m[1][1],
        ],
        [
            m[1][2] * m[2][0] - m[1][0] * m[2][2],
            m[0][0] * m[2][2] - m[0][2] * m[2][0],
            m[0][2] * m[1][0] - m[0][0] * m[1][2],
        ],
        [
            m[1][0] * m[2][1] - m[1][1] * m[2][0],
            m[0][1] * m[2][0] - m[0][0] * m[2][1],
            m[0][0] * m[1][1] - m[0][1] * m[1][0],
        ],
    ];
    [
        [
            cofactors[0][0] / determinant,
            cofactors[0][1] / determinant,
            cofactors[0][2] / determinant,
        ],
        [
            cofactors[1][0] / determinant,
            cofactors[1][1] / determinant,
            cofactors[1][2] / determinant,
        ],
        [
            cofactors[2][0] / determinant,
            cofactors[2][1] / determinant,
            cofactors[2][2] / determinant,
        ],
    ]
}

/// Returns the Bradford D50-to-D65 adaptation matrix.
pub fn bradford_d50_to_d65() -> [[f64; 3]; 3] {
    [
        [0.9554734215, -0.0230984549, 0.0632592432],
        [-0.0283697093, 1.0099953981, 0.0210414412],
        [0.0123140149, -0.0205076493, 1.3303659262],
    ]
}

/// Adapts XYZ from D50 to D65 using Bradford.
pub fn xyz_d50_to_d65(xyz: [f64; 3]) -> [f64; 3] {
    column_mat_vec(bradford_d50_to_d65(), xyz)
}

/// Converts D65 XYZ to linear sRGB.
pub fn xyz_to_linear_srgb(xyz: [f64; 3]) -> [f64; 3] {
    column_mat_vec(XYZ_TO_RGB, xyz)
}

/// Converts linear sRGB to D65 XYZ.
pub fn linear_srgb_to_xyz(rgb: [f64; 3]) -> [f64; 3] {
    column_mat_vec(inverse(XYZ_TO_RGB), rgb)
}

fn lab_f(t: f64) -> f64 {
    let delta: f64 = 6.0 / 29.0;
    if t > delta.powi(3) {
        t.cbrt()
    } else {
        t / (3.0 * delta.powi(2)) + 4.0 / 29.0
    }
}

fn lab_f_inverse(f: f64) -> f64 {
    let delta: f64 = 6.0 / 29.0;
    if f > delta {
        f.powi(3)
    } else {
        3.0 * delta.powi(2) * (f - 4.0 / 29.0)
    }
}

/// Converts D65 XYZ to CIE Lab.
pub fn xyz_d65_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    if xyz == [0.0; 3] {
        return [0.0; 3];
    }
    let fx = lab_f(xyz[0] / D65_XYZ[0]);
    let fy = lab_f(xyz[1] / D65_XYZ[1]);
    let fz = lab_f(xyz[2] / D65_XYZ[2]);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Converts CIE Lab to D65 XYZ.
pub fn lab_d65_to_xyz(lab: [f64; 3]) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    [
        D65_XYZ[0] * lab_f_inverse(fx),
        D65_XYZ[1] * lab_f_inverse(fy),
        D65_XYZ[2] * lab_f_inverse(fz),
    ]
}

fn hue_degrees(a: f64, b: f64) -> f64 {
    let hue = b.atan2(a).to_degrees();
    if hue < 0.0 {
        hue + 360.0
    } else {
        hue
    }
}

/// Computes CIEDE2000 color difference.
pub fn delta_e_2000(a: [f64; 3], b: [f64; 3]) -> f64 {
    let chroma_a = a[1].hypot(a[2]);
    let chroma_b = b[1].hypot(b[2]);
    let mean_chroma = (chroma_a + chroma_b) / 2.0;
    let correction =
        0.5 * (1.0 - (mean_chroma.powi(7) / (mean_chroma.powi(7) + 25.0_f64.powi(7))).sqrt());
    let a_prime = (1.0 + correction) * a[1];
    let b_prime = (1.0 + correction) * b[1];
    let chroma_a_prime = a_prime.hypot(a[2]);
    let chroma_b_prime = b_prime.hypot(b[2]);
    let hue_a = hue_degrees(a_prime, a[2]);
    let hue_b = hue_degrees(b_prime, b[2]);
    let delta_lightness = b[0] - a[0];
    let delta_chroma = chroma_b_prime - chroma_a_prime;
    let hue_difference = if chroma_a_prime * chroma_b_prime == 0.0 {
        0.0
    } else if (hue_b - hue_a).abs() <= 180.0 {
        hue_b - hue_a
    } else if hue_b > hue_a {
        hue_b - hue_a - 360.0
    } else {
        hue_b - hue_a + 360.0
    };
    let delta_hue =
        2.0 * (chroma_a_prime * chroma_b_prime).sqrt() * (hue_difference.to_radians() / 2.0).sin();
    let mean_lightness = (a[0] + b[0]) / 2.0;
    let mean_chroma_prime = (chroma_a_prime + chroma_b_prime) / 2.0;
    let mean_hue = if chroma_a_prime * chroma_b_prime == 0.0 {
        hue_a + hue_b
    } else if (hue_a - hue_b).abs() <= 180.0 {
        (hue_a + hue_b) / 2.0
    } else if hue_a + hue_b < 360.0 {
        (hue_a + hue_b + 360.0) / 2.0
    } else {
        (hue_a + hue_b - 360.0) / 2.0
    };
    let hue_weight = 1.0 - 0.17 * (mean_hue - 30.0).to_radians().cos()
        + 0.24 * (2.0 * mean_hue).to_radians().cos()
        + 0.32 * (3.0 * mean_hue + 6.0).to_radians().cos()
        - 0.20 * (4.0 * mean_hue - 63.0).to_radians().cos();
    let lightness_scale = 1.0
        + 0.015 * (mean_lightness - 50.0).powi(2) / (20.0 + (mean_lightness - 50.0).powi(2)).sqrt();
    let chroma_scale = 1.0 + 0.045 * mean_chroma_prime;
    let hue_scale = 1.0 + 0.015 * mean_chroma_prime * hue_weight;
    let rotation = -2.0
        * (mean_chroma_prime.powi(7) / (mean_chroma_prime.powi(7) + 25.0_f64.powi(7))).sqrt()
        * (60.0_f64.to_radians() * (-((mean_hue - 275.0) / 25.0).powi(2)).exp()).sin();
    ((delta_lightness / lightness_scale).powi(2)
        + (delta_chroma / chroma_scale).powi(2)
        + (delta_hue / hue_scale).powi(2)
        + rotation * (delta_chroma / chroma_scale) * (delta_hue / hue_scale))
        .sqrt()
}

/// Encodes linear sRGB to IEC 61966-2-1 sRGB. No clamping occurs.
pub fn srgb_encode(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Decodes IEC 61966-2-1 sRGB to linear sRGB. No clamping occurs.
pub fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Vectors {
        bradford_d50to_d65: [[f64; 3]; 3],
        bradford_cases: Vec<BradfordCase>,
        xyz_to_srgb_cases: Vec<XyzToSrgbCase>,
        xyz_to_lab_cases: Vec<XyzToLabCase>,
        srgb_encode_cases: Vec<SrgbEncodeCase>,
        delta_e2000_cases: Vec<DeltaECase>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BradfordCase {
        xyz_in: [f64; 3],
        xyz_d65: [f64; 3],
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct XyzToSrgbCase {
        xyz_d65: [f64; 3],
        linear_srgb: [f64; 3],
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct XyzToLabCase {
        xyz_d65: [f64; 3],
        lab: [f64; 3],
    }

    #[derive(Deserialize)]
    struct SrgbEncodeCase {
        linear: f64,
        encoded: f64,
    }

    #[derive(Deserialize)]
    struct DeltaECase {
        lab1: [f64; 3],
        lab2: [f64; 3],
        expected: f64,
    }

    fn vectors() -> Vectors {
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/reference/color-vectors.json"
        ))
        .expect("reference fixture is valid JSON")
    }

    fn assert_close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "expected {expected:.16e}, got {actual:.16e}, tolerance {tolerance:.1e}"
        );
    }

    fn assert_close_vector(actual: [f64; 3], expected: [f64; 3], tolerance: f64) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert_close(actual, expected, tolerance);
        }
    }

    #[test]
    fn bradford_vectors_match_reference() {
        let vectors = vectors();
        for (actual, expected) in bradford_d50_to_d65()
            .into_iter()
            .flatten()
            .zip(vectors.bradford_d50to_d65.into_iter().flatten())
        {
            assert_close(actual, expected, 1e-9);
        }
        for case in vectors.bradford_cases {
            assert_close_vector(xyz_d50_to_d65(case.xyz_in), case.xyz_d65, 1e-9);
        }
    }

    #[test]
    fn xyz_to_srgb_vectors_match_reference() {
        for case in vectors().xyz_to_srgb_cases {
            assert_close_vector(xyz_to_linear_srgb(case.xyz_d65), case.linear_srgb, 1e-9);
        }
    }

    #[test]
    fn xyz_to_lab_vectors_match_reference() {
        for case in vectors().xyz_to_lab_cases {
            assert_close_vector(xyz_d65_to_lab(case.xyz_d65), case.lab, 1e-8);
        }
    }

    #[test]
    fn srgb_encode_vectors_match_reference() {
        for case in vectors().srgb_encode_cases {
            assert_close(srgb_encode(case.linear), case.encoded, 1e-12);
        }
    }

    #[test]
    fn delta_e_2000_vectors_match_reference() {
        for case in vectors().delta_e2000_cases {
            assert_close(delta_e_2000(case.lab1, case.lab2), case.expected, 5e-5);
        }
    }

    #[test]
    fn srgb_round_trip_is_precise() {
        for encoded in [0.0, 0.0005, 0.18, 0.5, 1.0] {
            assert_close(srgb_encode(srgb_decode(encoded)), encoded, 1e-15);
        }
    }

    #[test]
    fn xyz_and_lab_round_trips_are_precise() {
        for xyz in [
            [0.1234, 0.2345, 0.3456],
            [0.9504559270516716, 1.0, 1.0890577507598784],
            [0.2, 0.7, 0.1],
        ] {
            assert_close_vector(linear_srgb_to_xyz(xyz_to_linear_srgb(xyz)), xyz, 1e-12);
            assert_close_vector(lab_d65_to_xyz(xyz_d65_to_lab(xyz)), xyz, 1e-12);
        }
    }
}
