use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use colorbalance_raw::{decode_dng, write_dng, DngWriteSpec};

fn temp_path(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "colorbalance-{name}-{}-{nonce}.dng",
        std::process::id()
    ))
}

fn base_spec(width: u32, height: u32, photosites: Vec<u16>) -> DngWriteSpec {
    DngWriteSpec {
        width,
        height,
        cfa_pattern: *b"RGGB",
        black_levels: [100, 200, 300, 400],
        white_levels: [1100; 4],
        make: "Test Make".to_owned(),
        model: "Test Model".to_owned(),
        orientation: 1,
        photosites,
    }
}

#[test]
fn photosite_round_trip_preserves_levels_and_cfa_mapping() {
    let path = temp_path("round-trip");
    let raw = [350, 425, 500, 575];
    let mut photosites = Vec::new();
    for y in 0..4 {
        for x in 0..4 {
            photosites.push(raw[(y % 2) * 2 + x % 2]);
        }
    }
    write_dng(&path, &base_spec(4, 4, photosites)).unwrap();
    let image = decode_dng(&path).unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(image.black_levels, [100, 200, 300, 400]);
    assert_eq!(image.white_levels, [1100; 4]);
    assert_eq!(image.cfa_pattern, [b'R', b'G', b'G', b'B']);
    let expected = [0.25_f32, 0.25, 0.25];
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(image.rgb_at(x, y), expected);
            assert_eq!(image.clipped_at(x, y), 0);
        }
    }
}

#[test]
fn sparse_single_channel_clipping_only_marks_its_support() {
    let path = temp_path("clip");
    let mut photosites = vec![600; 36];
    photosites[2 * 6 + 2] = 1100;
    write_dng(&path, &base_spec(6, 6, photosites)).unwrap();
    let image = decode_dng(&path).unwrap();
    fs::remove_file(path).unwrap();
    for y in 0..6 {
        for x in 0..6 {
            let expected = if (1..=3).contains(&x) && (1..=3).contains(&y) {
                1
            } else {
                0
            };
            assert_eq!(image.clipped_at(x, y), expected, "at ({x}, {y})");
        }
    }
}

#[test]
fn orientation_six_rotates_clockwise_and_swaps_dimensions() {
    let path = temp_path("orientation");
    let mut photosites = vec![600; 24];
    photosites[0] = 100;
    let mut spec = base_spec(4, 6, photosites);
    spec.orientation = 6;
    write_dng(&path, &spec).unwrap();
    let image = decode_dng(&path).unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!((image.width, image.height), (6, 4));
    assert_eq!(image.rgb_at(5, 0)[0], 0.0);
    assert!(image.rgb_at(0, 0)[0] > 0.0);
}

#[test]
fn unsupported_photometric_is_rejected() {
    let path = temp_path("photometric");
    write_dng(&path, &base_spec(4, 4, vec![600; 16])).unwrap();
    let mut bytes = fs::read(&path).unwrap();
    let ifd_count = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    for index in 0..ifd_count {
        let offset = 10 + index * 12;
        if u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) == 262 {
            bytes[offset + 8..offset + 10].copy_from_slice(&2_u16.to_le_bytes());
        }
    }
    fs::write(&path, bytes).unwrap();
    let error = decode_dng(&path).unwrap_err().to_string();
    fs::remove_file(path).unwrap();
    assert!(error.contains("PhotometricInterpretation"));
}

#[test]
fn decode_is_deterministic_across_repeated_decodes() {
    // The same DNG must decode to bit-identical normalized RGB, clip masks,
    // and levels on every run.
    let path = temp_path("determinism");
    let mut photosites = Vec::new();
    for y in 0..6 {
        for x in 0..6 {
            photosites.push(100 + ((y * 6 + x) * 37) as u16 % 1000);
        }
    }
    write_dng(&path, &base_spec(6, 6, photosites)).unwrap();
    let first = decode_dng(&path).unwrap();
    let second = decode_dng(&path).unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(first.width, second.width);
    assert_eq!(first.height, second.height);
    assert_eq!(first.black_levels, second.black_levels);
    assert_eq!(first.white_levels, second.white_levels);
    assert_eq!(first.cfa_pattern, second.cfa_pattern);
    assert_eq!(first.rgb.len(), second.rgb.len());
    assert!(first
        .rgb
        .iter()
        .zip(second.rgb.iter())
        .all(|(a, b)| a.to_bits() == b.to_bits()));
    assert_eq!(first.clipped, second.clipped);
}

#[test]
fn four_color_cfa_is_rejected() {
    // Bayer (RGGB) is the only supported layout; a fourth CFA color value
    // must fail with a clear error, not decode.
    let path = temp_path("four-color");
    write_dng(&path, &base_spec(4, 4, vec![600; 16])).unwrap();
    let mut bytes = fs::read(&path).unwrap();
    // IFD entry signature for CFAPattern: tag 33422 (0x828E), BYTE (1), count 4.
    let signature: &[u8] = &[0x8E, 0x82, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00];
    let pos = bytes
        .windows(8)
        .position(|w| w == signature)
        .expect("CFAPattern tag present in fixture");
    // Inline value holds [R, G, G, B]; set the fourth color to 3.
    bytes[pos + 8 + 3] = 3;
    fs::write(&path, &bytes).unwrap();
    let error = decode_dng(&path).unwrap_err().to_string();
    fs::remove_file(path).unwrap();
    assert!(
        error.contains("CFAPattern"),
        "expected CFAPattern rejection, got: {error}"
    );
}

// Phones and gallery apps hand out JPEGs named `.dng`, so the file's bytes, not its
// extension, must pick the decoder.
mod decode_by_content {
    use super::*;
    use colorbalance_raw::{decode_any, sniff_source, SourceKind};

    fn scratch(name: &str, extension: &str) -> PathBuf {
        temp_path(name).with_extension(extension)
    }

    #[test]
    fn jpeg_named_dng_decodes_as_a_rendered_image() {
        let path = scratch("jpeg-as-dng", "dng");
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            8,
            6,
            image::Rgb([90, 120, 60]),
        ))
        .write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
        fs::write(&path, jpeg).unwrap();

        assert_eq!(sniff_source(&path).unwrap(), SourceKind::Rendered);
        let decoded = decode_any(&path).expect("a JPEG must decode whatever its extension says");
        fs::remove_file(path).unwrap();
        assert_eq!((decoded.width, decoded.height), (8, 6));
    }

    #[test]
    fn real_dng_with_a_jpg_extension_still_decodes_as_raw() {
        let path = scratch("dng-as-jpg", "jpg");
        write_dng(&path, &base_spec(4, 4, vec![600; 16])).unwrap();

        assert_eq!(sniff_source(&path).unwrap(), SourceKind::Tiff);
        let decoded = decode_any(&path).expect("a DNG must decode whatever its extension says");
        fs::remove_file(path).unwrap();
        assert_eq!((decoded.width, decoded.height), (4, 4));
    }

    #[test]
    fn unrecognised_data_is_rejected_with_a_clear_message() {
        let path = scratch("garbage", "dng");
        fs::write(&path, b"this is not an image at all").unwrap();
        let error = decode_any(&path).unwrap_err().to_string();
        fs::remove_file(path).unwrap();
        assert!(
            error.contains("expected a DNG, JPEG, or PNG"),
            "got: {error}"
        );
    }
}
