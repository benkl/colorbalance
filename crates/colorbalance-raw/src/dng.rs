//! Classic TIFF/DNG CFA decoder.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;

use colorbalance_core::decode::{CameraIdentity, DecodeError, DecodedImage, RawDecoder};

/// Name recorded for images decoded by this implementation.
pub const DECODER_NAME: &str = "colorbalance-dng";
/// Crate version recorded for images decoded by this implementation.
pub const DECODER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A decoder for uncompressed, classic-TIFF CFA DNG files.
#[derive(Debug, Default, Clone, Copy)]
pub struct DngDecoder;

/// Error returned by the DNG entry point.
#[derive(Debug, thiserror::Error)]
pub enum DngError {
    /// A canonical decoder error.
    #[error(transparent)]
    Decode(#[from] DecodeError),
}

impl DngError {
    fn into_decode_error(self) -> DecodeError {
        match self {
            Self::Decode(error) => error,
        }
    }
}

#[derive(Clone, Copy)]
enum ByteOrder {
    Little,
    Big,
}

impl ByteOrder {
    fn u16(self, bytes: &[u8]) -> u16 {
        let bytes = [bytes[0], bytes[1]];
        match self {
            Self::Little => u16::from_le_bytes(bytes),
            Self::Big => u16::from_be_bytes(bytes),
        }
    }

    fn u32(self, bytes: &[u8]) -> u32 {
        let bytes = [bytes[0], bytes[1], bytes[2], bytes[3]];
        match self {
            Self::Little => u32::from_le_bytes(bytes),
            Self::Big => u32::from_be_bytes(bytes),
        }
    }
}

#[derive(Clone)]
struct IfdEntry {
    field_type: u16,
    count: u32,
    value_bytes: [u8; 4],
}

struct Tiff<'a> {
    bytes: &'a [u8],
    order: ByteOrder,
    entries: BTreeMap<u16, IfdEntry>,
}

fn corrupt(message: impl Into<String>) -> DngError {
    DecodeError::CorruptFile(message.into()).into()
}

fn unsupported_format(message: impl Into<String>) -> DngError {
    DecodeError::UnsupportedFormat(message.into()).into()
}

fn unsupported_layout(message: impl Into<String>) -> DngError {
    DecodeError::UnsupportedSensorLayout(message.into()).into()
}

fn field_size(field_type: u16) -> Option<usize> {
    match field_type {
        1 | 2 | 6 | 7 => Some(1),
        3 | 8 => Some(2),
        4 | 9 | 11 => Some(4),
        5 | 10 | 12 => Some(8),
        _ => None,
    }
}

impl<'a> Tiff<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, DngError> {
        if bytes.len() < 8 {
            return Err(corrupt("TIFF header is shorter than eight bytes"));
        }
        let order = match &bytes[..2] {
            b"II" => ByteOrder::Little,
            b"MM" => ByteOrder::Big,
            _ => return Err(unsupported_format("file is not a TIFF byte stream")),
        };
        if order.u16(&bytes[2..4]) != 42 {
            return Err(unsupported_format("TIFF magic is not 42"));
        }
        let ifd_offset = usize::try_from(order.u32(&bytes[4..8]))
            .map_err(|_| corrupt("first IFD offset does not fit memory"))?;
        let count_end = ifd_offset
            .checked_add(2)
            .ok_or_else(|| corrupt("IFD offset overflows"))?;
        if count_end > bytes.len() {
            return Err(corrupt("first IFD lies outside the file"));
        }
        let entry_count = usize::from(order.u16(&bytes[ifd_offset..count_end]));
        let entries_start = count_end;
        let entries_size = entry_count
            .checked_mul(12)
            .ok_or_else(|| corrupt("IFD entry count overflows"))?;
        let entries_end = entries_start
            .checked_add(entries_size)
            .and_then(|end| end.checked_add(4))
            .ok_or_else(|| corrupt("IFD entries overflow"))?;
        if entries_end > bytes.len() {
            return Err(corrupt("IFD entries lie outside the file"));
        }
        let mut entries = BTreeMap::new();
        for index in 0..entry_count {
            let offset = entries_start + index * 12;
            let tag = order.u16(&bytes[offset..offset + 2]);
            let entry = IfdEntry {
                field_type: order.u16(&bytes[offset + 2..offset + 4]),
                count: order.u32(&bytes[offset + 4..offset + 8]),
                value_bytes: bytes[offset + 8..offset + 12]
                    .try_into()
                    .expect("TIFF entry value field is four bytes"),
            };
            if entries.insert(tag, entry).is_some() {
                return Err(corrupt(format!("duplicate TIFF tag {tag}")));
            }
        }
        Ok(Self {
            bytes,
            order,
            entries,
        })
    }

    fn required(&self, tag: u16) -> Result<&IfdEntry, DngError> {
        self.entries
            .get(&tag)
            .ok_or_else(|| corrupt(format!("missing required TIFF tag {tag}")))
    }

    fn data<'b>(&'b self, entry: &'b IfdEntry) -> Result<Cow<'b, [u8]>, DngError> {
        let size = field_size(entry.field_type).ok_or_else(|| {
            unsupported_format(format!("unsupported TIFF field type {}", entry.field_type))
        })?;
        let count =
            usize::try_from(entry.count).map_err(|_| corrupt("TIFF count does not fit memory"))?;
        let total = size
            .checked_mul(count)
            .ok_or_else(|| corrupt("TIFF value size overflows"))?;
        if total <= 4 {
            return Ok(Cow::Borrowed(&entry.value_bytes[..total]));
        }
        let offset = usize::try_from(self.order.u32(&entry.value_bytes))
            .map_err(|_| corrupt("TIFF value offset does not fit memory"))?;
        let end = offset
            .checked_add(total)
            .ok_or_else(|| corrupt("TIFF value offset overflows"))?;
        self.bytes
            .get(offset..end)
            .map(Cow::Borrowed)
            .ok_or_else(|| corrupt("TIFF value lies outside the file"))
    }

    fn unsigned_values(&self, tag: u16, allowed_types: &[u16]) -> Result<Vec<u32>, DngError> {
        let entry = self.required(tag)?;
        if !allowed_types.contains(&entry.field_type) {
            return Err(corrupt(format!(
                "tag {tag} has unexpected TIFF field type {}",
                entry.field_type
            )));
        }
        let data = self.data(entry)?;
        let size = field_size(entry.field_type).expect("type validated above");
        Ok(data
            .chunks_exact(size)
            .map(|value| match entry.field_type {
                1 => u32::from(value[0]),
                3 => u32::from(self.order.u16(value)),
                4 => self.order.u32(value),
                _ => unreachable!("type validated above"),
            })
            .collect())
    }

    fn scalar(&self, tag: u16, allowed_types: &[u16]) -> Result<u32, DngError> {
        let values = self.unsigned_values(tag, allowed_types)?;
        if values.len() != 1 {
            return Err(corrupt(format!("tag {tag} must have one value")));
        }
        Ok(values[0])
    }

    fn ascii(&self, tag: u16) -> Result<String, DngError> {
        let entry = self.required(tag)?;
        if entry.field_type != 2 || entry.count == 0 {
            return Err(corrupt(format!("tag {tag} is not a nonempty ASCII value")));
        }
        let bytes = self.data(entry)?;
        let content = bytes
            .strip_suffix(&[0])
            .ok_or_else(|| corrupt(format!("tag {tag} is not NUL terminated")))?;
        std::str::from_utf8(content)
            .map(str::to_owned)
            .map_err(|_| corrupt(format!("tag {tag} is not valid UTF-8")))
    }

    fn black_levels(&self) -> Result<[u16; 4], DngError> {
        let entry = self.required(50_714)?;
        let data = self.data(entry)?;
        let levels: Vec<u16> = match entry.field_type {
            3 => data
                .chunks_exact(2)
                .map(|value| self.order.u16(value))
                .collect(),
            5 => data
                .chunks_exact(8)
                .map(|value| {
                    let numerator = self.order.u32(&value[..4]);
                    let denominator = self.order.u32(&value[4..]);
                    if denominator == 0 || numerator % denominator != 0 {
                        return Err(corrupt("BlackLevel contains a nonintegral RATIONAL"));
                    }
                    u16::try_from(numerator / denominator)
                        .map_err(|_| corrupt("BlackLevel RATIONAL is outside u16 range"))
                })
                .collect::<Result<_, _>>()?,
            _ => return Err(corrupt("BlackLevel must be SHORT or RATIONAL")),
        };
        match levels.as_slice() {
            [level] => Ok([*level; 4]),
            [a, b, c, d] => Ok([*a, *b, *c, *d]),
            _ => Err(corrupt("BlackLevel must have one or four values")),
        }
    }
}

fn to_u16(value: u32, tag: u16) -> Result<u16, DngError> {
    u16::try_from(value).map_err(|_| corrupt(format!("tag {tag} is outside u16 range")))
}

fn reject_unsupported_tags(tiff: &Tiff<'_>) -> Result<(), DngError> {
    for (tag, message) in [
        (322, "TileWidth"),
        (323, "TileLength"),
        (324, "TileOffsets"),
        (325, "TileByteCounts"),
        (50_712, "LinearizationTable"),
        (50_720, "DefaultCropOrigin"),
        (50_721, "DefaultCropSize"),
    ] {
        if tiff.entries.contains_key(&tag) {
            return Err(unsupported_layout(format!(
                "unsupported tag {message} ({tag})"
            )));
        }
    }
    Ok(())
}

fn parse_dng(bytes: &[u8]) -> Result<DecodedImage, DngError> {
    let tiff = Tiff::parse(bytes)?;
    reject_unsupported_tags(&tiff)?;

    let width = tiff.scalar(256, &[3, 4])?;
    let height = tiff.scalar(257, &[3, 4])?;
    if width == 0 || height == 0 {
        return Err(corrupt("image dimensions must be nonzero"));
    }
    if tiff.scalar(258, &[3])? != 16 {
        return Err(unsupported_format("BitsPerSample tag 258 must be 16"));
    }
    if tiff.scalar(259, &[3])? != 1 {
        return Err(unsupported_format("Compression tag 259 must be 1"));
    }
    if tiff.scalar(262, &[3])? != 32_803 {
        return Err(unsupported_format(
            "PhotometricInterpretation tag 262 must be CFA (32803)",
        ));
    }
    if tiff.scalar(277, &[3])? != 1 {
        return Err(unsupported_layout("SamplesPerPixel tag 277 must be 1"));
    }
    let repeat = tiff.unsigned_values(33_421, &[3])?;
    if repeat.as_slice() != [2, 2] {
        return Err(unsupported_layout(
            "CFARepeatPatternDim tag 33421 must be [2, 2]",
        ));
    }
    let cfa_values = tiff.unsigned_values(33_422, &[1])?;
    let cfa_pattern: [u8; 4] = cfa_values
        .iter()
        .map(|value| match value {
            0 => Ok(b'R'),
            1 => Ok(b'G'),
            2 => Ok(b'B'),
            _ => Err(unsupported_layout(format!(
                "CFAPattern tag 33422 has color {value}"
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| corrupt("CFAPattern tag 33422 must contain four values"))?;
    for color in *b"RGB" {
        if !cfa_pattern.contains(&color) {
            return Err(unsupported_layout("CFAPattern must contain R, G, and B"));
        }
    }
    let version = tiff.unsigned_values(50_706, &[1])?;
    if version.len() != 4 {
        return Err(corrupt("DNGVersion tag 50706 must contain four bytes"));
    }
    let black_levels = tiff.black_levels()?;
    let white_level = to_u16(tiff.scalar(50_717, &[3])?, 50_717)?;
    let white_levels = [white_level; 4];
    if black_levels.iter().any(|black| *black >= white_level) {
        return Err(corrupt("BlackLevel must be less than WhiteLevel"));
    }

    let orientation = if let Some(entry) = tiff.entries.get(&274) {
        if entry.field_type != 3 || entry.count != 1 {
            return Err(corrupt("Orientation tag 274 must be one SHORT"));
        }
        tiff.order.u16(&tiff.data(entry)?)
    } else {
        1
    };
    if !matches!(orientation, 1 | 3 | 6 | 8) {
        return Err(unsupported_layout(format!(
            "unsupported Orientation tag 274 value {orientation}"
        )));
    }

    if tiff.entries.contains_key(&278) && tiff.scalar(278, &[3, 4])? == 0 {
        return Err(corrupt("RowsPerStrip tag 278 must be nonzero"));
    }
    let strip_offsets = tiff.unsigned_values(273, &[3, 4])?;
    let strip_counts = tiff.unsigned_values(279, &[3, 4])?;
    if strip_offsets.len() != strip_counts.len() || strip_offsets.is_empty() {
        return Err(corrupt(
            "StripOffsets and StripByteCounts must have the same nonzero count",
        ));
    }
    let pixel_count = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| corrupt("image dimensions overflow"))?;
    let expected_bytes = pixel_count
        .checked_mul(2)
        .ok_or_else(|| corrupt("image byte count overflows"))?;
    let mut photosite_bytes = Vec::with_capacity(expected_bytes);
    for (offset, count) in strip_offsets.iter().zip(strip_counts) {
        let offset =
            usize::try_from(*offset).map_err(|_| corrupt("strip offset does not fit memory"))?;
        let count =
            usize::try_from(count).map_err(|_| corrupt("strip byte count does not fit memory"))?;
        let end = offset
            .checked_add(count)
            .ok_or_else(|| corrupt("strip range overflows"))?;
        let strip = bytes
            .get(offset..end)
            .ok_or_else(|| corrupt("strip data lies outside the file"))?;
        photosite_bytes.extend_from_slice(strip);
    }
    if photosite_bytes.len() != expected_bytes {
        return Err(corrupt("strip byte count does not match image dimensions"));
    }
    let photosites: Vec<u16> = photosite_bytes
        .chunks_exact(2)
        .map(|value| tiff.order.u16(value))
        .collect();

    let mut normalized = Vec::with_capacity(pixel_count);
    let mut saturated = Vec::with_capacity(pixel_count);
    for (index, raw) in photosites.iter().copied().enumerate() {
        let x = index % width as usize;
        let y = index / width as usize;
        let cfa = (y % 2) * 2 + x % 2;
        let black = black_levels[cfa];
        let white = white_levels[cfa];
        let value = ((f64::from(raw) - f64::from(black)) / f64::from(white - black)).max(0.0);
        normalized.push(value as f32);
        saturated.push(raw >= white);
    }

    let source_width = width as usize;
    let source_height = height as usize;
    let mut source_rgb = vec![0.0_f32; pixel_count * 3];
    let mut source_clipped = vec![0_u8; pixel_count];
    for y in 0..source_height {
        for x in 0..source_width {
            let pixel = y * source_width + x;
            for (channel, color) in b"RGB".iter().enumerate() {
                let mut sum = 0.0_f64;
                let mut count = 0_usize;
                let mut clipped = false;
                for site_y in y.saturating_sub(1)..=(y + 1).min(source_height - 1) {
                    for site_x in x.saturating_sub(1)..=(x + 1).min(source_width - 1) {
                        let site = site_y * source_width + site_x;
                        let cfa = (site_y % 2) * 2 + site_x % 2;
                        if cfa_pattern[cfa] == *color {
                            sum += f64::from(normalized[site]);
                            count += 1;
                            clipped |= saturated[site];
                        }
                    }
                }
                if count == 0 {
                    return Err(unsupported_layout("CFA has no support for a color channel"));
                }
                source_rgb[pixel * 3 + channel] = (sum / count as f64) as f32;
                if clipped {
                    source_clipped[pixel] |= 1 << channel;
                }
            }
        }
    }

    let (output_width, output_height) = if matches!(orientation, 6 | 8) {
        (height, width)
    } else {
        (width, height)
    };
    let mut rgb = vec![0.0_f32; source_rgb.len()];
    let mut clipped = vec![0_u8; source_clipped.len()];
    for y in 0..source_height {
        for x in 0..source_width {
            let (out_x, out_y) = match orientation {
                1 => (x, y),
                3 => (source_width - 1 - x, source_height - 1 - y),
                6 => (source_height - 1 - y, x),
                8 => (y, source_width - 1 - x),
                _ => unreachable!("orientation validated above"),
            };
            let source = y * source_width + x;
            let output = out_y * output_width as usize + out_x;
            rgb[output * 3..output * 3 + 3]
                .copy_from_slice(&source_rgb[source * 3..source * 3 + 3]);
            clipped[output] = source_clipped[source];
        }
    }

    Ok(DecodedImage {
        width: output_width,
        height: output_height,
        rgb,
        clipped,
        black_levels,
        white_levels,
        cfa_pattern,
        camera: CameraIdentity {
            make: tiff.ascii(271)?,
            model: tiff.ascii(272)?,
            decoder: DECODER_NAME.to_owned(),
            decoder_version: DECODER_VERSION.to_owned(),
        },
    })
}

/// Decode an uncompressed classic-TIFF CFA DNG from a filesystem path.
pub fn decode_dng(path: &Path) -> Result<DecodedImage, DngError> {
    let bytes = std::fs::read(path).map_err(DecodeError::from)?;
    parse_dng(&bytes)
}

impl RawDecoder for DngDecoder {
    fn decode_path(&self, path: &Path) -> Result<DecodedImage, DecodeError> {
        decode_dng(path).map_err(DngError::into_decode_error)
    }
}
