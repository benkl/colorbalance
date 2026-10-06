//! Lossless JPEG (ITU-T T.81, process SOF3) decoder.
//!
//! This is the compression DNG uses for lossless-compressed raw data, including
//! the 3-component linear-raw images written by Samsung phones. It supports
//! Huffman coding, predictors 1 through 7, a point transform, 2 to 16 bit
//! precision, byte stuffing, and restart intervals.
//!
//! Restart intervals matter: the Samsung files carry one every 16 image rows,
//! and a decoder that ignores them silently repeats the first interval.

use colorbalance_core::decode::DecodeError;

/// A fully decoded lossless-JPEG frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LosslessImage {
    pub width: usize,
    pub height: usize,
    pub components: usize,
    pub precision: u8,
    /// SOF3 component IDs in the same order as interleaved `samples`.
    pub component_ids: Vec<u8>,
    /// Interleaved samples, `width * height * components` long, row-major.
    pub samples: Vec<u16>,
}

fn corrupt(message: impl Into<String>) -> DecodeError {
    DecodeError::CorruptFile(format!("lossless JPEG: {}", message.into()))
}

fn unsupported(message: impl Into<String>) -> DecodeError {
    DecodeError::UnsupportedFormat(format!("lossless JPEG: {}", message.into()))
}

/// A canonical Huffman table decoded bit by bit.
#[derive(Clone, Default)]
struct HuffmanTable {
    /// Smallest code of each length, indexed 1..=16.
    min_code: [i32; 17],
    /// Largest code of each length, or -1 when there is none.
    max_code: [i32; 17],
    /// Index into `values` of the first symbol of each length.
    first_index: [usize; 17],
    values: Vec<u8>,
}

impl HuffmanTable {
    fn new(counts: &[u8; 16], values: &[u8]) -> Result<Self, DecodeError> {
        let total: usize = counts.iter().map(|&count| usize::from(count)).sum();
        if total != values.len() || total == 0 {
            return Err(corrupt(
                "Huffman table symbol count does not match its lengths",
            ));
        }
        if values.iter().any(|&category| category > 16) {
            return Err(corrupt(
                "lossless Huffman table has an invalid difference category",
            ));
        }
        let mut table = Self {
            values: values.to_vec(),
            ..Self::default()
        };
        let mut code = 0i32;
        let mut index = 0usize;
        for length in 1..=16 {
            let count = i32::from(counts[length - 1]);
            table.first_index[length] = index;
            table.min_code[length] = code;
            code += count;
            table.max_code[length] = if count == 0 { -1 } else { code - 1 };
            index += usize::from(counts[length - 1]);
            if code > (1 << length) {
                return Err(corrupt("Huffman table is over-subscribed"));
            }
            code <<= 1;
        }
        Ok(table)
    }
}

/// Reads only entropy bytes. A marker or EOF before a complete code is an error.
struct BitReader<'a> {
    data: &'a [u8],
    position: usize,
    byte: u8,
    bits: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], position: usize) -> Self {
        Self {
            data,
            position,
            byte: 0,
            bits: 0,
        }
    }

    fn bit(&mut self) -> Result<u32, DecodeError> {
        if self.bits == 0 {
            let byte = *self
                .data
                .get(self.position)
                .ok_or_else(|| corrupt("truncated entropy data"))?;
            self.position += 1;
            self.byte = if byte == 0xFF {
                match self.data.get(self.position) {
                    Some(&0x00) => {
                        self.position += 1;
                        0xFF
                    }
                    _ => return Err(corrupt("unexpected marker in entropy data")),
                }
            } else {
                byte
            };
            self.bits = 8;
        }
        self.bits -= 1;
        Ok(u32::from((self.byte >> self.bits) & 1))
    }

    fn read(&mut self, count: u32) -> Result<u32, DecodeError> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }
        Ok(value)
    }

    fn symbol(&mut self, table: &HuffmanTable) -> Result<u8, DecodeError> {
        let mut code = 0i32;
        for length in 1..=16 {
            code = (code << 1) | self.bit()? as i32;
            if table.max_code[length] >= 0
                && code >= table.min_code[length]
                && code <= table.max_code[length]
            {
                let index = table.first_index[length] + (code - table.min_code[length]) as usize;
                return table
                    .values
                    .get(index)
                    .copied()
                    .ok_or_else(|| corrupt("Huffman symbol index is out of range"));
            }
        }
        Err(corrupt("invalid Huffman code"))
    }

    /// Discard only the unused bits of the last entropy byte, not arbitrary bytes.
    fn marker(&mut self, expected: u8) -> Result<(), DecodeError> {
        self.bits = 0;
        if self.data.get(self.position) != Some(&0xFF) {
            return Err(corrupt("expected marker at entropy boundary"));
        }
        while self.data.get(self.position) == Some(&0xFF) {
            self.position += 1;
        }
        if self.data.get(self.position) != Some(&expected) {
            return Err(corrupt(format!(
                "expected marker FF {expected:02X} at entropy boundary"
            )));
        }
        self.position += 1;
        Ok(())
    }
}

struct Component {
    table: usize,
    index: usize,
}

/// Decode a complete lossless-JPEG stream (SOI through EOI).
pub fn decode(data: &[u8]) -> Result<LosslessImage, DecodeError> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err(corrupt("stream does not start with SOI"));
    }
    let mut position = 2;
    let mut tables: [Option<HuffmanTable>; 4] = [None, None, None, None];
    let mut restart_interval = 0usize;
    let mut frame: Option<(u8, usize, usize, usize)> = None;
    let mut frame_ids = [0u8; 4];

    loop {
        if data.get(position) != Some(&0xFF) {
            return Err(corrupt("expected a marker before the scan"));
        }
        while data.get(position) == Some(&0xFF) {
            position += 1;
        }
        let marker = *data
            .get(position)
            .ok_or_else(|| corrupt("truncated marker"))?;
        position += 1;
        if marker == 0xD9 {
            return Err(corrupt("end of image before the scan"));
        }
        if marker == 0 || marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            return Err(corrupt("unexpected marker before the scan"));
        }
        let length_bytes: [u8; 2] = data
            .get(position..position + 2)
            .ok_or_else(|| corrupt("truncated segment length"))?
            .try_into()
            .expect("two bytes");
        let length = usize::from(u16::from_be_bytes(length_bytes));
        if length < 2 {
            return Err(corrupt("invalid segment length"));
        }
        let body_start = position + 2;
        let body_end = position + length;
        let body = data
            .get(body_start..body_end)
            .ok_or_else(|| corrupt("segment runs past the end of the data"))?;
        match marker {
            // SOF0-SOF2 and friends are the lossy/extended processes.
            0xC0..=0xC2 | 0xC5..=0xCF if marker != 0xC4 && marker != 0xC8 && marker != 0xCC => {
                return Err(unsupported(format!(
                    "frame type SOF{} is not lossless (expected SOF3)",
                    marker - 0xC0
                )));
            }
            0xC3 => {
                if frame.is_some() {
                    return Err(corrupt("multiple frame headers"));
                }
                if body.len() < 6 {
                    return Err(corrupt("frame header is too short"));
                }
                let precision = body[0];
                let height = usize::from(u16::from_be_bytes([body[1], body[2]]));
                let width = usize::from(u16::from_be_bytes([body[3], body[4]]));
                let components = usize::from(body[5]);
                if !(2..=16).contains(&precision) {
                    return Err(unsupported(format!("{precision}-bit precision")));
                }
                if width == 0 || height == 0 || components == 0 || components > 4 {
                    return Err(corrupt("frame dimensions or component count are invalid"));
                }
                if body.len() != 6 + 3 * components {
                    return Err(corrupt("frame header length does not match its components"));
                }
                for index in 0..components {
                    let component_id = body[6 + 3 * index];
                    if frame_ids[..index].contains(&component_id) {
                        return Err(corrupt("duplicate frame component ID"));
                    }
                    frame_ids[index] = component_id;
                    if body[7 + 3 * index] != 0x11 {
                        return Err(unsupported("subsampled components"));
                    }
                }
                frame = Some((precision, width, height, components));
            }
            0xC4 => {
                let mut cursor = 0;
                while cursor < body.len() {
                    let info = body[cursor];
                    let class = info >> 4;
                    let id = usize::from(info & 15);
                    if class != 0 || id > 3 {
                        return Err(corrupt("unexpected Huffman table class or id"));
                    }
                    let counts: [u8; 16] = body
                        .get(cursor + 1..cursor + 17)
                        .ok_or_else(|| corrupt("Huffman table is truncated"))?
                        .try_into()
                        .expect("sixteen bytes");
                    let total: usize = counts.iter().map(|&count| usize::from(count)).sum();
                    let values = body
                        .get(cursor + 17..cursor + 17 + total)
                        .ok_or_else(|| corrupt("Huffman values are truncated"))?;
                    tables[id] = Some(HuffmanTable::new(&counts, values)?);
                    cursor += 17 + total;
                }
            }
            0xDD => {
                if body.len() != 2 {
                    return Err(corrupt("restart interval segment must have two bytes"));
                }
                restart_interval = usize::from(u16::from_be_bytes([body[0], body[1]]));
            }
            0xDA => {
                let (precision, width, height, components) =
                    frame.ok_or_else(|| corrupt("scan appears before the frame header"))?;
                let scan_components =
                    usize::from(*body.first().ok_or_else(|| corrupt("empty scan"))?);
                if scan_components != components {
                    return Err(unsupported("scans that do not cover every component"));
                }
                if body.len() != 4 + 2 * scan_components {
                    return Err(corrupt("scan header length does not match its components"));
                }
                let mut selectors = Vec::with_capacity(components);
                let mut seen = [false; 256];
                for index in 0..components {
                    let component_id = body[1 + 2 * index];
                    let frame_index = frame_ids[..components]
                        .iter()
                        .position(|&id| id == component_id)
                        .ok_or_else(|| corrupt("scan has an unknown component"))?;
                    if seen[usize::from(component_id)] {
                        return Err(corrupt("scan has a duplicate component"));
                    }
                    seen[usize::from(component_id)] = true;
                    if body[2 + 2 * index] & 15 != 0 {
                        return Err(corrupt("lossless scan uses a nonzero AC selector"));
                    }
                    let table = usize::from(body[2 + 2 * index] >> 4);
                    if table > 3 || tables[table].is_none() {
                        return Err(corrupt("scan references a missing Huffman table"));
                    }
                    selectors.push(Component {
                        table,
                        index: frame_index,
                    });
                }
                let predictor = body[1 + 2 * scan_components];
                let point_transform = u32::from(body[3 + 2 * scan_components] & 15);
                if body[2 + 2 * scan_components] != 0 || body[3 + 2 * scan_components] & 0xF0 != 0 {
                    return Err(corrupt("invalid lossless scan approximation bytes"));
                }
                if point_transform >= u32::from(precision) {
                    return Err(corrupt("point transform exceeds sample precision"));
                }
                if !(1..=7).contains(&predictor) {
                    return Err(unsupported(format!("predictor {predictor}")));
                }
                return decode_scan(
                    data,
                    body_end,
                    (precision, width, height, components),
                    &tables,
                    &selectors,
                    &frame_ids[..components],
                    predictor,
                    point_transform,
                    restart_interval,
                );
            }
            // Metadata and unused quantization tables do not contribute to SOF3 pixels.
            0xE0..=0xEF | 0xFE | 0xDB => {}
            _ => {
                return Err(unsupported(format!(
                    "marker FF {marker:02X} before the scan"
                )))
            }
        }
        position = body_end;
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_scan(
    data: &[u8],
    start: usize,
    frame: (u8, usize, usize, usize),
    tables: &[Option<HuffmanTable>; 4],
    selectors: &[Component],
    component_ids: &[u8],
    predictor: u8,
    point_transform: u32,
    restart_interval: usize,
) -> Result<LosslessImage, DecodeError> {
    let (precision, width, height, components) = frame;
    let sample_count = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(components))
        .ok_or_else(|| corrupt("image dimensions overflow"))?;
    if restart_interval != 0 && restart_interval < width * height && restart_interval % width != 0 {
        return Err(unsupported(
            "restart intervals must start at row boundaries",
        ));
    }
    let mut samples = vec![0u16; sample_count];
    let mut reader = BitReader::new(data, start);
    let initial = 1i32 << (u32::from(precision) - point_transform - 1);

    let mut pixels_in_interval = 0usize;
    let mut next_restart = 0u8;
    // The first sample after each restart uses the initial predictor.
    let mut interval_first_row = 0usize;
    for y in 0..height {
        for x in 0..width {
            if restart_interval > 0 && pixels_in_interval == restart_interval {
                reader.marker(0xD0 + next_restart)?;
                next_restart = (next_restart + 1) & 7;
                pixels_in_interval = 0;
            }
            if pixels_in_interval == 0 {
                interval_first_row = y;
            }
            let interval_start = pixels_in_interval == 0;
            for component in 0..components {
                let table = tables[selectors[component].table]
                    .as_ref()
                    .expect("table presence checked while parsing the scan header");
                let category = reader.symbol(table)?;
                let difference = match category {
                    0 => 0,
                    16 => 32768,
                    category @ 1..=15 => {
                        let bits = reader.read(u32::from(category))? as i32;
                        if bits < (1 << (category - 1)) {
                            bits - ((1 << category) - 1)
                        } else {
                            bits
                        }
                    }
                    _ => return Err(corrupt("lossless difference category exceeds 16")),
                };

                let frame_component = selectors[component].index;
                let index = (y * width + x) * components + frame_component;
                let at = |dx: usize, dy: usize| -> i32 {
                    i32::from(samples[((y - dy) * width + (x - dx)) * components + frame_component])
                        >> point_transform
                };
                // T.81 H.1.1: the first sample of a scan or restart interval predicts
                // from mid-grey, and every other sample of that first line predicts
                // from its left neighbour. Neighbours above the interval do not exist.
                let prediction = if interval_start {
                    initial
                } else if y == interval_first_row || y == 0 {
                    at(1, 0)
                } else if x == 0 {
                    at(0, 1)
                } else {
                    let a = at(1, 0);
                    let b = at(0, 1);
                    let c = at(1, 1);
                    match predictor {
                        1 => a,
                        2 => b,
                        3 => c,
                        4 => a + b - c,
                        5 => a + ((b - c) >> 1),
                        6 => b + ((a - c) >> 1),
                        _ => (a + b) >> 1,
                    }
                };
                // Lossless JPEG arithmetic wraps negative differences at the
                // declared precision. Samsung's 12-bit DNG also contains valid
                // positive samples above 4095: keep those values so clipping
                // remains visible instead of wrapping highlights to black.
                let value = (i64::from(prediction) + i64::from(difference)) << point_transform;
                let modulus = 1i64 << u32::from(precision);
                let value = if value < 0 || value > i64::from(u16::MAX) {
                    value.rem_euclid(modulus)
                } else {
                    value
                };
                samples[index] = u16::try_from(value)
                    .map_err(|_| corrupt("decoded sample is outside the u16 range"))?;
            }
            pixels_in_interval += 1;
        }
    }
    reader.marker(0xD9)?;
    if reader.position != data.len() {
        return Err(corrupt("trailing bytes after EOI"));
    }
    Ok(LosslessImage {
        width,
        height,
        components,
        component_ids: component_ids.to_vec(),
        precision,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) {
        out.extend_from_slice(&[0xFF, marker]);
        out.extend_from_slice(&u16::try_from(body.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(body);
    }

    struct Bits {
        bytes: Vec<u8>,
        value: u8,
        count: u8,
    }

    impl Bits {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                value: 0,
                count: 0,
            }
        }

        fn push(&mut self, value: u32, length: u8) {
            for shift in (0..length).rev() {
                self.value = (self.value << 1) | (((value >> shift) & 1) as u8);
                self.count += 1;
                if self.count == 8 {
                    self.bytes.push(self.value);
                    if self.value == 0xFF {
                        self.bytes.push(0);
                    }
                    self.count = 0;
                    self.value = 0;
                }
            }
        }

        fn flush(&mut self, out: &mut Vec<u8>) {
            if self.count != 0 {
                self.push((1 << (8 - self.count)) - 1, 8 - self.count);
            }
            out.append(&mut self.bytes);
        }
    }

    // Fixed five-bit canonical codes for categories 0..16; deliberately simple
    // so expected pixels are independent of the decoder's Huffman logic.
    #[allow(clippy::too_many_arguments)] // Test fixture: every JPEG header dimension is explicit.
    fn encoded(
        width: usize,
        height: usize,
        channels: usize,
        precision: u8,
        pt: u8,
        predictor: u8,
        restart: usize,
        samples: &[u16],
    ) -> Vec<u8> {
        let mut jpeg = vec![0xFF, 0xD8];
        let mut frame = vec![
            precision,
            (height >> 8) as u8,
            height as u8,
            (width >> 8) as u8,
            width as u8,
            channels as u8,
        ];
        for c in 0..channels {
            frame.extend_from_slice(&[(c + 1) as u8, 0x11, 0]);
        }
        segment(&mut jpeg, 0xC3, &frame);
        let mut dht = vec![0];
        dht.extend_from_slice(&[0, 0, 0, 0, 17, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        dht.extend(0..=16);
        segment(&mut jpeg, 0xC4, &dht);
        if restart != 0 {
            segment(&mut jpeg, 0xDD, &(restart as u16).to_be_bytes());
        }
        let mut scan = vec![channels as u8];
        for c in 0..channels {
            scan.extend_from_slice(&[(c + 1) as u8, 0]);
        }
        scan.extend_from_slice(&[predictor, 0, pt]);
        segment(&mut jpeg, 0xDA, &scan);

        let mut bits = Bits::new();
        let initial = 1i32 << (precision - pt - 1);
        let modulus = 1i32 << (precision - pt);
        let mut rst = 0;
        for pixel in 0..width * height {
            if restart != 0 && pixel != 0 && pixel % restart == 0 {
                bits.flush(&mut jpeg);
                jpeg.extend_from_slice(&[0xFF, 0xD0 + rst]);
                rst = (rst + 1) & 7;
            }
            let x = pixel % width;
            let y = pixel / width;
            for c in 0..channels {
                let at = |dx: usize, dy: usize| -> i32 {
                    i32::from(samples[((y - dy) * width + x - dx) * channels + c]) >> pt
                };
                let prediction = if pixel == 0 || (restart != 0 && pixel % restart == 0) {
                    initial
                } else if y == 0 || (restart != 0 && y == (pixel / restart * restart) / width) {
                    at(1, 0)
                } else if x == 0 {
                    at(0, 1)
                } else {
                    let (a, b, z) = (at(1, 0), at(0, 1), at(1, 1));
                    match predictor {
                        1 => a,
                        2 => b,
                        3 => z,
                        4 => a + b - z,
                        5 => a + ((b - z) >> 1),
                        6 => b + ((a - z) >> 1),
                        _ => (a + b) >> 1,
                    }
                };
                let target = i32::from(samples[pixel * channels + c]) >> pt;
                let diff = (target - prediction + modulus / 2).rem_euclid(modulus) - modulus / 2;
                let category = if diff == 0 {
                    0
                } else if diff == -32768 {
                    16
                } else {
                    (32 - diff.unsigned_abs().leading_zeros()) as u8
                };
                bits.push(u32::from(category), 5);
                if (1..=15).contains(&category) {
                    let raw = if diff < 0 {
                        diff + (1 << category) - 1
                    } else {
                        diff
                    };
                    bits.push(raw as u32, category);
                }
            }
        }
        bits.flush(&mut jpeg);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn predictors_and_point_transform() {
        for predictor in 1..=7 {
            for pt in [0, 2] {
                let samples: Vec<u16> = (0..4 * 5)
                    .flat_map(|pixel| {
                        (0..3).map(move |channel| {
                            let x = pixel % 4;
                            let y = pixel / 4;
                            ((100 + channel * 13 + x * x * 3 + y * y * 4 + x * y * 2) << pt) as u16
                        })
                    })
                    .collect();
                let jpeg = encoded(4, 5, 3, 10, pt, predictor, 0, &samples);
                let image = decode(&jpeg).unwrap();
                assert_eq!(image.component_ids, [1, 2, 3]);
                assert_eq!(image.samples, samples, "predictor {predictor}, Pt {pt}");
            }
        }
    }

    #[test]
    fn positive_values_above_precision_are_not_wrapped_to_black() {
        let samples = [4095, 4096, 4128, 4094];
        let jpeg = encoded(4, 1, 1, 12, 0, 1, 0, &samples);
        assert_eq!(decode(&jpeg).unwrap().samples, samples);
    }

    #[test]
    fn mid_row_restart_interval_is_rejected() {
        let jpeg = encoded(5, 2, 1, 12, 0, 1, 3, &[2048; 10]);
        assert!(decode(&jpeg).is_err());
    }

    #[test]
    fn restart_sequence_wraps_and_does_not_skip_bytes() {
        let samples: Vec<u16> = (0..4 * 11 * 3)
            .map(|index| (100 + (index / 3 % 4) * 3 + (index / 12) * 5 + index % 3) as u16)
            .collect();
        for restart in [4, 8, 12, 16] {
            let jpeg = encoded(4, 11, 3, 12, 0, 1, restart, &samples);
            assert_eq!(decode(&jpeg).unwrap().samples, samples, "restart {restart}");
        }
        let jpeg = encoded(4, 11, 3, 12, 0, 1, 4, &samples);
        let marker = jpeg
            .windows(2)
            .position(|pair| pair == [0xFF, 0xD0])
            .unwrap();
        for replacement in [[0xFF, 0xD1], [0x00, 0xD0], [0xFF, 0xD9]] {
            let mut bad = jpeg.clone();
            bad[marker..marker + 2].copy_from_slice(&replacement);
            assert!(decode(&bad).is_err(), "accepted restart {replacement:?}");
        }
        let mut injected = jpeg.clone();
        injected.insert(marker, 0);
        assert!(decode(&injected).is_err());
        let mut missing = jpeg.clone();
        missing.drain(marker..marker + 2);
        assert!(decode(&missing).is_err());
    }

    #[test]
    fn stuffed_ff_categories_and_truncation() {
        let category_sixteen = encoded(2, 1, 1, 16, 0, 1, 0, &[0, 65535]);
        assert_eq!(decode(&category_sixteen).unwrap().samples, [0, 65535]);
        let samples = [65535, 65534];
        let jpeg = encoded(2, 1, 1, 16, 0, 1, 0, &samples);
        let stuffed = jpeg
            .windows(2)
            .position(|pair| pair == [0xFF, 0x00])
            .unwrap();
        assert_eq!(decode(&jpeg).unwrap().samples, samples);
        let mut bad = jpeg.clone();
        bad[stuffed + 1] = 0xD9;
        assert!(decode(&bad).is_err());
        let mut bad = jpeg.clone();
        bad.remove(stuffed + 1);
        assert!(decode(&bad).is_err());
        let mut bad = jpeg.clone();
        bad.truncate(stuffed);
        assert!(decode(&bad).is_err());
        let mut bad = jpeg.clone();
        bad.truncate(bad.len() - 2);
        assert!(decode(&bad).is_err());
        let mut bad = jpeg.clone();
        bad.extend_from_slice(&[0]);
        assert!(decode(&bad).is_err());
        // A marker inserted inside a difference must not supply fake zero bits.
        let mut bad = jpeg.clone();
        let scan_start = jpeg
            .windows(2)
            .position(|pair| pair == [0xFF, 0xDA])
            .unwrap();
        let header_len = usize::from(u16::from_be_bytes([
            jpeg[scan_start + 2],
            jpeg[scan_start + 3],
        ]));
        bad.splice(
            scan_start + 2 + header_len..scan_start + 2 + header_len + 2,
            [0xFF, 0xD9],
        );
        assert!(decode(&bad).is_err());
    }

    #[test]
    fn rejects_malformed_headers_and_scan_categories() {
        let jpeg = encoded(2, 1, 1, 12, 0, 1, 0, &[2048, 2049]);
        let mut bad = jpeg.clone();
        let sof = bad
            .windows(2)
            .position(|pair| pair == [0xFF, 0xC3])
            .unwrap();
        bad[sof + 2] = 0;
        bad[sof + 3] = 1;
        assert!(decode(&bad).is_err());
        let mut bad = jpeg.clone();
        let sos = bad
            .windows(2)
            .position(|pair| pair == [0xFF, 0xDA])
            .unwrap();
        bad[sos + 9] = 12; // Pt must be below precision.
        assert!(decode(&bad).is_err());
        let mut bad = jpeg.clone();
        let dht = bad
            .windows(2)
            .position(|pair| pair == [0xFF, 0xC4])
            .unwrap();
        bad[dht + 4 + 17 + 16] = 17;
        assert!(decode(&bad).is_err());
        let mut bad = jpeg.clone();
        bad.insert(bad.len() - 2, 0);
        assert!(decode(&bad).is_err());
        let mut bad = jpeg.clone();
        let sof = bad
            .windows(2)
            .position(|pair| pair == [0xFF, 0xC3])
            .unwrap();
        bad[sof + 10] = 9;
        assert!(decode(&bad).is_err());
    }
}
