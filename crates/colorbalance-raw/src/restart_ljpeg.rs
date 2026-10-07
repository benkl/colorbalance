//! Restart-interval aware lossless JPEG decode for single-strip LinearRaw DNGs.
//!
//! rawler 0.8.0 ignores the JPEG DRI segment and RSTn markers. It decodes the
//! entropy data straight through, so the Huffman state and the predictor are
//! wrong after the first restart interval and every later row is garbage. A
//! Samsung Galaxy S25 DNG carries one restart interval per 16 rows.
//!
//! This module splits the strip at its RSTn markers and decodes each interval
//! as an independent JPEG with rawler's own lossless decoder. Per T.81 the
//! predictor and bit state reset at a restart, so the intervals are
//! independent. `research/samsung_reference_decode.py` does the same split with
//! libjpeg and is the independent check for this code.

use rawler::decompressors::ljpeg::LjpegDecompressor;
use rayon::prelude::*;

const SOF3: u8 = 0xC3;
const DHT: u8 = 0xC4;
const DRI: u8 = 0xDD;
const SOS: u8 = 0xDA;
const RST0: u8 = 0xD0;

/// A strip cut into restart intervals, ready to decode.
struct Plan<'a> {
    /// SOI through SOS with the DRI segment removed.
    head: Vec<u8>,
    /// Offset of the 16-bit SOF3 line count inside `head`.
    height_at: usize,
    rows_per_interval: usize,
    intervals: Vec<&'a [u8]>,
}

fn be16(bytes: &[u8], at: usize) -> Result<usize, String> {
    bytes
        .get(at..at + 2)
        .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
        .ok_or_else(|| "truncated JPEG header".to_owned())
}

/// Parse the header and cut the entropy data at RSTn markers.
///
/// Returns `Ok(None)` when the strip has no restart interval, so rawler's own
/// decode is already correct for it.
fn plan(
    stream: &[u8],
    width: usize,
    height: usize,
    cpp: usize,
) -> Result<Option<Plan<'_>>, String> {
    if stream.get(..2) != Some(&[0xFF, 0xD8]) {
        return Err("strip does not start with a JPEG SOI marker".to_owned());
    }
    let mut head = vec![0xFF, 0xD8];
    let mut height_at = None;
    let mut restart = 0_usize;
    let mut at = 2;
    let header_end = loop {
        if stream.get(at) != Some(&0xFF) {
            return Err("malformed JPEG header".to_owned());
        }
        let marker = *stream.get(at + 1).ok_or("truncated JPEG header")?;
        let length = be16(stream, at + 2)?;
        let end = at + 2 + length;
        let segment = stream.get(at..end).ok_or("truncated JPEG header")?;
        if length < 2 {
            return Err("malformed JPEG segment length".to_owned());
        }
        match marker {
            SOF3 => {
                let precision = segment.get(4).copied().ok_or("truncated SOF3")?;
                let lines = be16(segment, 5)?;
                let samples = be16(segment, 7)?;
                let components = usize::from(segment.get(9).copied().ok_or("truncated SOF3")?);
                if !(10..=16).contains(&precision)
                    || lines != height
                    || samples != width
                    || components != cpp
                {
                    return Err(format!(
                        "SOF3 {samples}x{lines} with {components} components does not match the TIFF {width}x{height} with {cpp}"
                    ));
                }
                height_at = Some(head.len() + 5);
                head.extend_from_slice(segment);
            }
            DRI => {
                if length != 4 {
                    return Err("malformed DRI segment".to_owned());
                }
                restart = be16(segment, 4)?;
            }
            DHT => head.extend_from_slice(segment),
            SOS => {
                head.extend_from_slice(segment);
                break end;
            }
            // Any other segment (COM, APPn, ...) cannot affect lossless entropy
            // decoding, but a second frame header or an unknown SOF would.
            0xC0..=0xCF => return Err(format!("unsupported JPEG frame marker {marker:#04x}")),
            _ => head.extend_from_slice(segment),
        }
        at = end;
    };
    if restart == 0 {
        return Ok(None);
    }
    let height_at = height_at.ok_or("JPEG header has no SOF3 segment")?;
    if !restart.is_multiple_of(width) {
        return Err(format!(
            "restart interval of {restart} samples does not span whole {width}-sample rows"
        ));
    }
    let rows_per_interval = restart / width;

    let entropy = stream[header_end..]
        .strip_suffix(&[0xFF, 0xD9])
        .ok_or("strip does not end with an EOI marker")?;
    let mut intervals = Vec::new();
    let mut start = 0;
    let mut cursor = 0;
    while let Some(offset) = entropy[cursor..].iter().position(|&b| b == 0xFF) {
        let marker_at = cursor + offset;
        match entropy.get(marker_at + 1) {
            Some(0x00) => cursor = marker_at + 2,
            Some(&m) if (RST0..=RST0 + 7).contains(&m) => {
                let expected = RST0 + (intervals.len() % 8) as u8;
                if m != expected {
                    return Err(format!(
                        "restart marker {m:#04x} where {expected:#04x} was expected"
                    ));
                }
                intervals.push(&entropy[start..marker_at]);
                start = marker_at + 2;
                cursor = start;
            }
            Some(&m) => return Err(format!("unexpected marker {m:#04x} inside entropy data")),
            None => return Err("entropy data ends inside a marker".to_owned()),
        }
    }
    intervals.push(&entropy[start..]);
    let expected = height.div_ceil(rows_per_interval);
    if intervals.len() != expected {
        return Err(format!(
            "{} restart intervals found, {expected} expected for {height} rows of {rows_per_interval}",
            intervals.len()
        ));
    }
    Ok(Some(Plan {
        head,
        height_at,
        rows_per_interval,
        intervals,
    }))
}

/// Decode a single-strip lossless JPEG that uses restart intervals.
///
/// Returns interleaved samples, `width * height * cpp` of them, or `Ok(None)`
/// when the strip has no restart interval. Any inconsistency is an error:
/// a partial or shifted decode must never reach color math.
pub fn decode_restart_strip(
    stream: &[u8],
    width: usize,
    height: usize,
    cpp: usize,
) -> Result<Option<Vec<u16>>, String> {
    let Some(plan) = plan(stream, width, height, cpp)? else {
        return Ok(None);
    };
    let row_samples = width * cpp;
    let mut out = vec![0_u16; row_samples * height];
    out.par_chunks_mut(plan.rows_per_interval * row_samples)
        .zip(plan.intervals.par_iter())
        .enumerate()
        .try_for_each(|(index, (chunk, entropy))| {
            let rows = chunk.len() / row_samples;
            let mut piece = Vec::with_capacity(plan.head.len() + entropy.len() + 2);
            piece.extend_from_slice(&plan.head);
            piece[plan.height_at..plan.height_at + 2].copy_from_slice(
                &u16::try_from(rows)
                    .map_err(|_| "interval too tall")?
                    .to_be_bytes(),
            );
            piece.extend_from_slice(entropy);
            piece.extend_from_slice(&[0xFF, 0xD9]);
            let decoder = LjpegDecompressor::new(&piece)
                .map_err(|e| format!("restart interval {index}: {e}"))?;
            if decoder.width() != row_samples || decoder.height() != rows {
                return Err(format!(
                    "restart interval {index} decoded to the wrong size"
                ));
            }
            decoder
                .decode(chunk, 0, row_samples, row_samples, rows, false)
                .map_err(|e| format!("restart interval {index}: {e}"))
        })?;
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Single-component 12-bit lossless JPEG. The Huffman table has two
    /// one-bit codes: `0` is a zero difference, `1` is category 1 followed by
    /// one sign bit (`1` is +1, `0` is -1).
    fn jpeg(width: u16, lines: u16, restart: Option<u16>, entropy: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xC3, 0, 11, 12];
        out.extend_from_slice(&lines.to_be_bytes());
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&[1, 0, 0x11, 0]);
        out.extend_from_slice(&[0xFF, 0xC4, 0, 21, 0, 2]);
        out.extend_from_slice(&[0; 15]);
        out.extend_from_slice(&[0, 1]);
        if let Some(interval) = restart {
            out.extend_from_slice(&[0xFF, 0xDD, 0, 4]);
            out.extend_from_slice(&interval.to_be_bytes());
        }
        out.extend_from_slice(&[0xFF, 0xDA, 0, 8, 1, 0, 0, 1, 0, 0]);
        out.extend_from_slice(entropy);
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    /// One interval is two 4-sample rows. The first sample is predicted from
    /// 2048 and the rest from their neighbours, with one-bit padding:
    /// `0xC0 0x7F` is +1 then seven zero differences (all 2049), `0x80 0x7F`
    /// is -1 then zeros (all 2047), and `0x00 0x7F` is eight zeros (all 2048).
    const UP: [u8; 2] = [0xC0, 0x7F];
    const DOWN: [u8; 2] = [0x80, 0x7F];
    const FLAT: [u8; 2] = [0x00, 0x7F];

    fn entropy(parts: [[u8; 2]; 3], markers: [u8; 2]) -> Vec<u8> {
        let mut out = parts[0].to_vec();
        out.extend_from_slice(&[0xFF, markers[0]]);
        out.extend_from_slice(&parts[1]);
        out.extend_from_slice(&[0xFF, markers[1]]);
        out.extend_from_slice(&parts[2]);
        out
    }

    #[test]
    fn a_strip_without_a_restart_interval_is_left_to_rawler() {
        let stream = jpeg(4, 4, None, &FLAT);
        assert_eq!(decode_restart_strip(&stream, 4, 4, 1).unwrap(), None);
    }

    #[test]
    fn each_interval_restarts_the_predictor_and_rows_land_in_order() {
        let stream = jpeg(4, 6, Some(8), &entropy([UP, DOWN, FLAT], [0xD0, 0xD1]));
        let samples = decode_restart_strip(&stream, 4, 6, 1).unwrap().unwrap();
        let expected: Vec<u16> = [2049, 2047, 2048]
            .into_iter()
            .flat_map(|v| [v; 8])
            .collect();
        assert_eq!(samples, expected);
    }

    #[test]
    fn restart_markers_out_of_order_fail_closed() {
        let stream = jpeg(4, 6, Some(8), &entropy([UP, DOWN, FLAT], [0xD1, 0xD0]));
        let error = decode_restart_strip(&stream, 4, 6, 1).unwrap_err();
        assert!(error.contains("was expected"), "{error}");
    }

    #[test]
    fn a_missing_interval_fails_closed() {
        let mut data = UP.to_vec();
        data.extend_from_slice(&[0xFF, 0xD0]);
        data.extend_from_slice(&DOWN);
        let stream = jpeg(4, 6, Some(8), &data);
        let error = decode_restart_strip(&stream, 4, 6, 1).unwrap_err();
        assert!(error.contains("restart intervals found"), "{error}");
    }

    #[test]
    fn an_interval_that_splits_a_row_fails_closed() {
        let stream = jpeg(4, 6, Some(6), &entropy([UP, DOWN, FLAT], [0xD0, 0xD1]));
        let error = decode_restart_strip(&stream, 4, 6, 1).unwrap_err();
        assert!(error.contains("whole"), "{error}");
    }

    #[test]
    fn a_header_that_disagrees_with_the_tiff_dimensions_fails_closed() {
        let stream = jpeg(4, 6, Some(8), &entropy([UP, DOWN, FLAT], [0xD0, 0xD1]));
        let error = decode_restart_strip(&stream, 4, 8, 1).unwrap_err();
        assert!(error.contains("does not match"), "{error}");
    }
}
