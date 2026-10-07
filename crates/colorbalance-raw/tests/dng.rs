use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use colorbalance_raw::{decode_raw, write_dng, DngWriteSpec};

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
    let image = decode_raw(&path).unwrap();
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
    // One saturated red photosite at (8, 8). AHD support radius is 5, so the
    // red flag covers x, y in 3..=13 and nothing else; green and blue stay clear.
    let path = temp_path("clip");
    let mut photosites = vec![600; 16 * 16];
    photosites[8 * 16 + 8] = 1100;
    write_dng(&path, &base_spec(16, 16, photosites)).unwrap();
    let image = decode_raw(&path).unwrap();
    fs::remove_file(path).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let expected = if (3..=13).contains(&x) && (3..=13).contains(&y) {
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
    let image = decode_raw(&path).unwrap();
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
    let error = decode_raw(&path).unwrap_err().to_string();
    fs::remove_file(path).unwrap();
    assert!(error.contains("PhotometricInterpretation"));
}

#[test]
fn full_frame_crop_is_accepted_but_smaller_crop_is_rejected() {
    let path = temp_path("crop");
    write_dng(&path, &base_spec(4, 4, vec![600; 16])).unwrap();
    let original = fs::read(&path).unwrap();
    let ifd_count = u16::from_le_bytes([original[8], original[9]]) as usize;
    let insertion = 10 + 12 * ifd_count;
    let mut bytes = original.clone();
    bytes.splice(
        insertion..insertion,
        [
            0x1f, 0xc6, 0x03, 0x00, 0x02, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0x20, 0xc6, 0x03, 0x00,
            0x02, 0x00, 0x00, 0x00, 4, 0, 4, 0,
        ],
    );
    bytes[8..10].copy_from_slice(&((ifd_count + 2) as u16).to_le_bytes());
    // Inserting IFD entries shifts all out-of-line values and the strip.
    for index in 0..ifd_count + 2 {
        let offset = 10 + index * 12;
        let tag = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        if matches!(tag, 271 | 272 | 273 | 50_714) {
            let old = u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap());
            bytes[offset + 8..offset + 12].copy_from_slice(&(old + 24).to_le_bytes());
        }
    }
    fs::write(&path, &bytes).unwrap();
    assert!(decode_raw(&path).is_ok());
    // No crop may silently masquerade as the full camera frame.
    bytes[insertion + 20..insertion + 22].copy_from_slice(&3_u16.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    let error = decode_raw(&path).unwrap_err().to_string();
    fs::remove_file(path).unwrap();
    assert!(error.contains("not the full frame"), "{error}");
}

/// Write a three-component LinearRaw DNG around `jpeg`, a single lossless JPEG strip.
/// Channel black levels are 0, 1024, 0 and white levels 4095, 3072, 2048.
fn write_linear_raw(path: &std::path::Path, jpeg: &[u8], width: u32, height: u32) {
    let mut data = b"II*\0\x08\0\0\0".to_vec();
    let entries: &[(u16, u16, u32, u32)] = &[
        (254, 4, 1, 0),
        (256, 4, 1, width),
        (257, 4, 1, height),
        (258, 3, 3, 256),
        (259, 3, 1, 7),
        (262, 3, 1, 34_892),
        (271, 2, 5, 262),
        (272, 2, 6, 267),
        (273, 4, 1, 285),
        (274, 3, 1, 6),
        (277, 3, 1, 3),
        (278, 4, 1, height),
        (279, 4, 1, jpeg.len() as u32),
        (284, 3, 1, 1),
        (50_706, 1, 4, 0x0000_0601),
        (50_714, 3, 3, 273),
        (50_717, 3, 3, 279),
        (50_719, 3, 2, 0),
        (50_720, 3, 2, width | (height << 16)),
    ];
    data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for &(tag, ty, count, value) in entries {
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&ty.to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.resize(256, 0);
    for _ in 0..3 {
        data.extend_from_slice(&12_u16.to_le_bytes());
    }
    data.extend_from_slice(b"Test\0Phone\0");
    data.resize(273, 0);
    for level in [0_u16, 1024, 0, 4095, 3072, 2048] {
        data.extend_from_slice(&level.to_le_bytes());
    }
    data.resize(285, 0);
    data.extend_from_slice(jpeg);
    fs::write(path, data).unwrap();
}

#[test]
fn linear_raw_jpeg_uses_measured_channels_not_a_cfa() {
    use colorbalance_core::decode::SensorLayout;

    // A single SOF3 pixel with three zero differences from the 12-bit
    // initial predictor (2048). Frame component IDs are R=0, G=1, B=2.
    let jpeg: &[u8] = &[
        0xff, 0xd8, 0xff, 0xc3, 0, 17, 12, 0, 1, 0, 1, 3, 0, 0x11, 0, 1, 0x11, 0, 2, 0x11, 0, 0xff,
        0xc4, 0, 20, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xda, 0, 12, 3, 0,
        0, 1, 0, 2, 0, 1, 0, 0, 0x1f, 0xff, 0xd9,
    ];
    let path = temp_path("linear-raw");
    write_linear_raw(&path, jpeg, 1, 1);
    let result = colorbalance_raw::decode_any(&path).unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(result.sensor_layout, SensorLayout::LinearRaw);
    assert_eq!((result.width, result.height), (1, 1));
    assert_eq!(result.cfa_pattern, [0; 4]);
    assert_eq!(result.black_levels, [0, 1024, 0, 0]);
    assert_eq!(result.white_levels, [4095, 3072, 2048, 0]);
    assert_eq!(result.clipped_at(0, 0), 4);
    let pixel = result.rgb_at(0, 0);
    assert!((pixel[0] - 2048.0 / 4095.0).abs() < 1e-6);
    assert!((pixel[1] - 1024.0 / 2048.0).abs() < 1e-6);
    assert_eq!(pixel[2], 1.0);
}

/// Three-component 12-bit lossless JPEG with a two-code Huffman table: `0` is a
/// zero difference and `1` is category 1, followed by a sign bit (1 is +1, 0 is -1).
fn restart_jpeg(lines: u16, restart_pixels: u16, entropy: &[u8]) -> Vec<u8> {
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xc3, 0, 17, 12];
    jpeg.extend_from_slice(&lines.to_be_bytes());
    jpeg.extend_from_slice(&[0, 1, 3, 0, 0x11, 0, 1, 0x11, 0, 2, 0x11, 0]);
    jpeg.extend_from_slice(&[0xff, 0xc4, 0, 21, 0, 2]);
    jpeg.extend_from_slice(&[0; 15]);
    jpeg.extend_from_slice(&[0, 1]);
    jpeg.extend_from_slice(&[0xff, 0xdd, 0, 4]);
    jpeg.extend_from_slice(&restart_pixels.to_be_bytes());
    jpeg.extend_from_slice(&[0xff, 0xda, 0, 12, 3, 0, 0, 1, 0, 2, 0, 1, 0, 0]);
    jpeg.extend_from_slice(entropy);
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    jpeg
}

#[test]
fn linear_raw_restart_intervals_reset_the_predictor() {
    // Two rows of one pixel, one restart interval per row. Each interval codes
    // +1, -1, 0 against the 12-bit initial predictor 2048, bits 11 10 0 plus
    // one-padding (0xe7). Both rows must decode to (2049, 2047, 2048). A decoder
    // that ignores the restart marker predicts row 1 from row 0 instead.
    let jpeg = restart_jpeg(2, 1, &[0xe7, 0xff, 0xd0, 0xe7]);
    let path = temp_path("linear-raw-restart");
    write_linear_raw(&path, &jpeg, 1, 2);
    let result = colorbalance_raw::decode_any(&path).unwrap();
    fs::remove_file(path).unwrap();
    // Orientation 6 turns the two rows into two columns.
    assert_eq!((result.width, result.height), (2, 1));
    for x in 0..2 {
        let pixel = result.rgb_at(x, 0);
        assert!((pixel[0] - 2049.0 / 4095.0).abs() < 1e-6, "{pixel:?}");
        assert!((pixel[1] - 1023.0 / 2048.0).abs() < 1e-6, "{pixel:?}");
        assert_eq!(pixel[2], 1.0, "{pixel:?}");
    }
}

#[test]
fn linear_raw_restart_marker_out_of_order_is_rejected() {
    let jpeg = restart_jpeg(2, 1, &[0xe7, 0xff, 0xd1, 0xe7]);
    let path = temp_path("linear-raw-restart-order");
    write_linear_raw(&path, &jpeg, 1, 2);
    let error = colorbalance_raw::decode_any(&path).unwrap_err().to_string();
    fs::remove_file(path).unwrap();
    assert!(error.contains("restart"), "{error}");
}

#[test]
fn linear_raw_samples_far_above_white_fail_closed() {
    // 16-bit precision starts every component at 32768, more than twice any
    // white level this fixture declares (4095, 3072, 2048), as a decode that
    // lost its place would produce.
    let jpeg: &[u8] = &[
        0xff, 0xd8, 0xff, 0xc3, 0, 17, 16, 0, 1, 0, 1, 3, 0, 0x11, 0, 1, 0x11, 0, 2, 0x11, 0, 0xff,
        0xc4, 0, 20, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xda, 0, 12, 3, 0,
        0, 1, 0, 2, 0, 1, 0, 0, 0x1f, 0xff, 0xd9,
    ];
    let path = temp_path("linear-raw-garbage");
    write_linear_raw(&path, jpeg, 1, 1);
    let error = colorbalance_raw::decode_any(&path).unwrap_err().to_string();
    fs::remove_file(path).unwrap();
    assert!(error.contains("twice the white level"), "{error}");
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
    let first = decode_raw(&path).unwrap();
    let second = decode_raw(&path).unwrap();
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
    let error = decode_raw(&path).unwrap_err().to_string();
    fs::remove_file(path).unwrap();
    assert!(
        error.contains("RGB Bayer"),
        "expected non-RGB CFA rejection, got: {error}"
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

        assert_eq!(sniff_source(&path).unwrap(), SourceKind::Raw);
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
            error.contains("expected a DNG, JPEG, PNG, or camera RAW"),
            "got: {error}"
        );
    }
}
