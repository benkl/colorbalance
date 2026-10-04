//! Deterministic RGB TIFF output.
//!
//! The encoder writes classic little-endian TIFF with one uncompressed RGB
//! strip. It has no platform-dependent metadata, timestamps, padding, or
//! compression, so identical inputs produce identical bytes.

/// Encode interleaved RGB `u16` pixels as a baseline little-endian TIFF.
///
/// Pixels are written in row-major order, with each pixel represented by
/// three little-endian 16-bit samples. The file has one strip,
/// `PhotometricInterpretation=RGB`, `BitsPerSample=[16,16,16]`,
/// `SamplesPerPixel=3`, and no extra samples.
///
/// # Panics
///
/// Panics if either dimension is zero, `pixels` does not contain exactly
/// `width * height * 3` samples, or the pixel byte count cannot be stored
/// in a classic TIFF `LONG` field.
pub fn encode_tiff_rgb_u16(width: u32, height: u32, pixels: &[u16]) -> Vec<u8> {
    assert!(width != 0 && height != 0, "TIFF dimensions must be nonzero");
    let expected_samples = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(3))
        .expect("TIFF dimensions exceed addressable memory");
    assert_eq!(
        pixels.len(),
        expected_samples,
        "pixel count must equal width * height * 3"
    );
    // Header (8) + IFD count (2) + ten 12-byte entries + next-IFD offset (4).
    const IFD_OFFSET: u32 = 8;
    const TAG_COUNT: u16 = 10;
    const IFD_BYTES: u32 = 2 + (TAG_COUNT as u32) * 12 + 4;
    const BITS_PER_SAMPLE_OFFSET: u32 = IFD_OFFSET + IFD_BYTES;
    const BITS_PER_SAMPLE_BYTES: u32 = 6;
    let strip_offset = BITS_PER_SAMPLE_OFFSET + BITS_PER_SAMPLE_BYTES;
    let pixel_byte_count = pixels
        .len()
        .checked_mul(2)
        .expect("TIFF pixel byte count exceeds addressable memory");
    let strip_byte_count =
        u32::try_from(pixel_byte_count).expect("TIFF pixel byte count exceeds classic TIFF LONG");
    let capacity = (strip_offset as usize)
        .checked_add(pixel_byte_count)
        .expect("TIFF file size exceeds addressable memory");
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(b"II");
    put_u16(&mut bytes, 42);
    put_u32(&mut bytes, IFD_OFFSET);

    put_u16(&mut bytes, TAG_COUNT);
    // Entries are sorted by tag number, as required by classic TIFF.
    put_entry(&mut bytes, 256, 4, 1, width);
    put_entry(&mut bytes, 257, 4, 1, height);
    put_entry(&mut bytes, 258, 3, 3, BITS_PER_SAMPLE_OFFSET);
    put_entry(&mut bytes, 259, 3, 1, 1);
    put_entry(&mut bytes, 262, 3, 1, 2);
    put_entry(&mut bytes, 273, 4, 1, strip_offset);
    put_entry(&mut bytes, 277, 3, 1, 3);
    put_entry(&mut bytes, 278, 4, 1, height);
    put_entry(&mut bytes, 279, 4, 1, strip_byte_count);
    put_entry(&mut bytes, 284, 3, 1, 1);
    // No following IFD.
    put_u32(&mut bytes, 0);

    put_u16(&mut bytes, 16);
    put_u16(&mut bytes, 16);
    put_u16(&mut bytes, 16);
    for &sample in pixels {
        put_u16(&mut bytes, sample);
    }
    bytes
}

/// Append one classic TIFF IFD entry. `value` is either an inline scalar or
/// an offset, depending on the entry's type and count.
fn put_entry(bytes: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
    put_u16(bytes, tag);
    put_u16(bytes, field_type);
    put_u32(bytes, count);
    match (field_type, count) {
        // A SHORT scalar occupies the first two bytes of the value field.
        (3, 1) => {
            put_u16(bytes, value as u16);
            put_u16(bytes, 0);
        }
        _ => put_u32(bytes, value),
    }
}

fn put_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TiffView<'a> {
        width: u32,
        height: u32,
        bits_per_sample: [u16; 3],
        strip_offset: usize,
        strip_byte_count: usize,
        bytes: &'a [u8],
    }

    fn read_u16(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    fn read_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    /// Minimal reader for the tags emitted by this module. Keeping this
    /// reader in tests means assertions inspect the encoded structure rather
    /// than relying on offsets that duplicate the writer's arithmetic.
    fn read_tiff(bytes: &[u8]) -> TiffView<'_> {
        assert_eq!(&bytes[0..2], b"II");
        assert_eq!(read_u16(bytes, 2), 42);
        let ifd = read_u32(bytes, 4) as usize;
        let count = read_u16(bytes, ifd) as usize;
        let mut width = None;
        let mut height = None;
        let mut bits = None;
        let mut strip_offset = None;
        let mut strip_byte_count = None;
        for i in 0..count {
            let at = ifd + 2 + i * 12;
            let tag = read_u16(bytes, at);
            let field_type = read_u16(bytes, at + 2);
            let item_count = read_u32(bytes, at + 4);
            let value = at + 8;
            match tag {
                256 => {
                    assert_eq!(field_type, 4);
                    assert_eq!(item_count, 1);
                    width = Some(read_u32(bytes, value));
                }
                257 => {
                    assert_eq!(field_type, 4);
                    assert_eq!(item_count, 1);
                    height = Some(read_u32(bytes, value));
                }
                258 => {
                    assert_eq!(field_type, 3);
                    assert_eq!(item_count, 3);
                    let at = read_u32(bytes, value) as usize;
                    bits = Some([
                        read_u16(bytes, at),
                        read_u16(bytes, at + 2),
                        read_u16(bytes, at + 4),
                    ]);
                }
                259 => {
                    assert_eq!(field_type, 3);
                    assert_eq!(item_count, 1);
                    assert_eq!(read_u16(bytes, value), 1);
                }
                262 => {
                    assert_eq!(field_type, 3);
                    assert_eq!(item_count, 1);
                    assert_eq!(read_u16(bytes, value), 2);
                }
                273 => {
                    assert_eq!(field_type, 4);
                    assert_eq!(item_count, 1);
                    strip_offset = Some(read_u32(bytes, value) as usize);
                }
                277 => {
                    assert_eq!(field_type, 3);
                    assert_eq!(item_count, 1);
                    assert_eq!(read_u16(bytes, value), 3);
                }
                278 => {
                    assert_eq!(field_type, 4);
                    assert_eq!(item_count, 1);
                }
                279 => {
                    assert_eq!(field_type, 4);
                    assert_eq!(item_count, 1);
                    strip_byte_count = Some(read_u32(bytes, value) as usize);
                }
                284 => {
                    assert_eq!(field_type, 3);
                    assert_eq!(item_count, 1);
                    assert_eq!(read_u16(bytes, value), 1);
                }
                tag => panic!("unexpected TIFF tag {tag}"),
            }
        }
        assert_eq!(read_u32(bytes, ifd + 2 + count * 12), 0);
        let strip_offset = strip_offset.expect("StripOffsets");
        let strip_byte_count = strip_byte_count.expect("StripByteCounts");
        TiffView {
            width: width.expect("ImageWidth"),
            height: height.expect("ImageLength"),
            bits_per_sample: bits.expect("BitsPerSample"),
            strip_offset,
            strip_byte_count,
            bytes,
        }
    }

    #[test]
    fn tiff_round_trips_pixel_bytes_and_tags() {
        let pixels = [0u16, 1, 65535, 32768, 1234, 54321];
        let bytes = encode_tiff_rgb_u16(2, 1, &pixels);
        let view = read_tiff(&bytes);
        assert_eq!((view.width, view.height), (2, 1));
        assert_eq!(view.bits_per_sample, [16, 16, 16]);
        assert_eq!(view.strip_byte_count, pixels.len() * 2);
        let expected: Vec<u8> = pixels.iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(&view.bytes[view.strip_offset..], expected.as_slice());
    }

    #[test]
    fn strip_byte_count_matches_data_length() {
        let pixels: Vec<u16> = (0..27).map(|i| i * 257).collect();
        let bytes = encode_tiff_rgb_u16(3, 3, &pixels);
        let view = read_tiff(&bytes);
        assert_eq!(view.strip_byte_count, pixels.len() * 2);
        assert_eq!(view.strip_offset + view.strip_byte_count, bytes.len());
    }

    #[test]
    fn bytes_are_deterministic() {
        let pixels = [9u16, 8, 7, 6, 5, 4];
        assert_eq!(
            encode_tiff_rgb_u16(1, 2, &pixels),
            encode_tiff_rgb_u16(1, 2, &pixels)
        );
    }
}
