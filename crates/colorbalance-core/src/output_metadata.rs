//! Offset-safe TIFF directory serializer for the image IFD and rebuilt EXIF directories.

use crate::metadata::{valid_value, ExifIfd, ExifValue, ExportMetadata};
use crate::output::OutputError;

struct Entry<'a> {
    tag: u16,
    kind: u16,
    data: std::borrow::Cow<'a, [u8]>,
    count: u32,
}

impl<'a> Entry<'a> {
    fn scalar(tag: u16, kind: u16, value: u32) -> Self {
        let data = if kind == 3 {
            (value as u16).to_le_bytes().to_vec()
        } else {
            value.to_le_bytes().to_vec()
        };
        Self {
            tag,
            kind,
            data: std::borrow::Cow::Owned(data),
            count: 1,
        }
    }
    fn bytes(
        tag: u16,
        kind: u16,
        data: impl Into<std::borrow::Cow<'a, [u8]>>,
        count: usize,
    ) -> Result<Self, OutputError> {
        Ok(Self {
            tag,
            kind,
            data: data.into(),
            count: u32::try_from(count).map_err(|_| OutputError::TooLarge)?,
        })
    }
}

fn checked_u32(value: usize) -> Result<u32, OutputError> {
    u32::try_from(value).map_err(|_| OutputError::TooLarge)
}

fn exif_entry<'a>(tag: u16, value: &'a ExifValue) -> Result<Entry<'a>, OutputError> {
    let (kind, data, count) = match value {
        ExifValue::Ascii(v) => (2, std::borrow::Cow::Borrowed(v.as_slice()), v.len()),
        ExifValue::Byte(v) => (1, std::borrow::Cow::Borrowed(v.as_slice()), v.len()),
        ExifValue::Undefined(v) => (7, std::borrow::Cow::Borrowed(v.as_slice()), v.len()),
        ExifValue::Short(v) => (
            3,
            std::borrow::Cow::Owned(v.iter().flat_map(|n| n.to_le_bytes()).collect()),
            v.len(),
        ),
        ExifValue::Long(v) => (
            4,
            std::borrow::Cow::Owned(v.iter().flat_map(|n| n.to_le_bytes()).collect()),
            v.len(),
        ),
        ExifValue::Rational(v) => (
            5,
            std::borrow::Cow::Owned(
                v.iter()
                    .flat_map(|(n, d)| n.to_le_bytes().into_iter().chain(d.to_le_bytes()))
                    .collect(),
            ),
            v.len(),
        ),
        ExifValue::SRational(v) => (
            10,
            std::borrow::Cow::Owned(
                v.iter()
                    .flat_map(|(n, d)| n.to_le_bytes().into_iter().chain(d.to_le_bytes()))
                    .collect(),
            ),
            v.len(),
        ),
    };
    Entry::bytes(tag, kind, data, count)
}

fn append_directory(
    bytes: &mut Vec<u8>,
    entries: &[Entry<'_>],
    offsets: &[u32],
) -> Result<(), OutputError> {
    bytes.extend_from_slice(
        &u16::try_from(entries.len())
            .map_err(|_| OutputError::TooLarge)?
            .to_le_bytes(),
    );
    for (e, &offset) in entries.iter().zip(offsets) {
        bytes.extend_from_slice(&e.tag.to_le_bytes());
        bytes.extend_from_slice(&e.kind.to_le_bytes());
        bytes.extend_from_slice(&e.count.to_le_bytes());
        if e.data.len() <= 4 {
            bytes.extend_from_slice(&e.data);
            bytes.resize(bytes.len() + 4 - e.data.len(), 0);
        } else {
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    Ok(())
}

fn directory_size(n: usize) -> Result<usize, OutputError> {
    n.checked_mul(12)
        .and_then(|v| v.checked_add(6))
        .ok_or(OutputError::TooLarge)
}

/// Returns a rebuilt little-endian TIFF. JPEG EXIF uses this without image entries.
/// For TIFF image output, `image` is Some((width, height, pixels, icc)).
pub(super) fn serialize<'a>(
    metadata: &'a ExportMetadata,
    image: Option<(u32, u32, &[u16], &'a [u8])>,
    include_aux: bool,
) -> Result<Vec<u8>, OutputError> {
    let mut root = Vec::new();
    let mut exif = Vec::new();
    let mut gps = Vec::new();
    if let Some((width, height, pixels, icc)) = image {
        root.extend([
            Entry::scalar(256, 4, width),
            Entry::scalar(257, 4, height),
            Entry::bytes(
                258,
                3,
                [16u16, 16, 16]
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<u8>>(),
                3,
            )?,
            Entry::scalar(259, 3, 1),
            Entry::scalar(262, 3, 2),
            Entry::scalar(273, 4, 0),
            Entry::scalar(277, 3, 3),
            Entry::scalar(278, 4, height),
            Entry::scalar(
                279,
                4,
                checked_u32(pixels.len().checked_mul(2).ok_or(OutputError::TooLarge)?)?,
            ),
            Entry::scalar(284, 3, 1),
            Entry::bytes(34675, 7, icc, icc.len())?,
        ]);
    }
    for entry in &metadata.exif {
        if !valid_value(entry.ifd, entry.tag, &entry.value) {
            continue;
        }
        let target = match entry.ifd {
            ExifIfd::Primary => &mut root,
            ExifIfd::Exif => &mut exif,
            ExifIfd::Gps => &mut gps,
        };
        if !target.iter().any(|e| e.tag == entry.tag) {
            target.push(exif_entry(entry.tag, &entry.value)?);
        }
    }
    if image.is_some() && include_aux {
        if let Some(xmp) = &metadata.xmp {
            if !xmp.is_empty() {
                root.push(Entry::bytes(700, 1, xmp.as_slice(), xmp.len())?);
            }
        }
        if let Some(iptc) = &metadata.iptc {
            if !iptc.is_empty() {
                let count = iptc.len().checked_add(3).ok_or(OutputError::TooLarge)? / 4;
                let mut padded = iptc.clone();
                padded.resize(count * 4, 0);
                root.push(Entry::bytes(33723, 4, padded, count)?);
            }
        }
    }
    if !exif.is_empty() {
        root.push(Entry::scalar(34665, 4, 0));
    }
    if !gps.is_empty() {
        root.push(Entry::scalar(34853, 4, 0));
    }
    root.sort_by_key(|e| e.tag);
    exif.sort_by_key(|e| e.tag);
    gps.sort_by_key(|e| e.tag);

    let root_at = 8usize;
    let exif_at = root_at
        .checked_add(directory_size(root.len())?)
        .ok_or(OutputError::TooLarge)?;
    let gps_at = exif_at
        .checked_add(if exif.is_empty() {
            0
        } else {
            directory_size(exif.len())?
        })
        .ok_or(OutputError::TooLarge)?;
    let mut next = gps_at
        .checked_add(if gps.is_empty() {
            0
        } else {
            directory_size(gps.len())?
        })
        .ok_or(OutputError::TooLarge)?;
    for entry in &mut root {
        if entry.tag == 34665 {
            entry.data = std::borrow::Cow::Owned(checked_u32(exif_at)?.to_le_bytes().to_vec());
        }
        if entry.tag == 34853 {
            entry.data = std::borrow::Cow::Owned(checked_u32(gps_at)?.to_le_bytes().to_vec());
        }
    }
    let mut all_offsets = Vec::new();
    for directory in [&root, &exif, &gps] {
        let mut offsets = Vec::with_capacity(directory.len());
        for entry in directory {
            if entry.data.len() > 4 {
                next = next.checked_add(next & 1).ok_or(OutputError::TooLarge)?;
                offsets.push(checked_u32(next)?);
                next = next
                    .checked_add(entry.data.len())
                    .ok_or(OutputError::TooLarge)?;
            } else {
                offsets.push(0);
            }
        }
        all_offsets.push(offsets);
    }
    if image.is_some() {
        next = next.checked_add(next & 1).ok_or(OutputError::TooLarge)?;
    }
    let pixel_at = checked_u32(next)?;
    let pixel_bytes = image
        .map(|(_, _, p, _)| p.len().checked_mul(2).ok_or(OutputError::TooLarge))
        .transpose()?
        .unwrap_or(0);
    let capacity = next.checked_add(pixel_bytes).ok_or(OutputError::TooLarge)?;
    checked_u32(capacity)?;
    if image.is_some() {
        root.iter_mut()
            .find(|e| e.tag == 273)
            .expect("strip entry")
            .data = std::borrow::Cow::Owned(pixel_at.to_le_bytes().to_vec());
    }
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(b"II");
    bytes.extend_from_slice(&42u16.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());
    append_directory(&mut bytes, &root, &all_offsets[0])?;
    if !exif.is_empty() {
        append_directory(&mut bytes, &exif, &all_offsets[1])?;
    }
    if !gps.is_empty() {
        append_directory(&mut bytes, &gps, &all_offsets[2])?;
    }
    for (directory, offsets) in [
        (&root, &all_offsets[0]),
        (&exif, &all_offsets[1]),
        (&gps, &all_offsets[2]),
    ] {
        for (entry, &offset) in directory.iter().zip(offsets) {
            if entry.data.len() > 4 {
                bytes.resize(offset as usize, 0);
                bytes.extend_from_slice(&entry.data);
            }
        }
    }
    if let Some((_, _, pixels, _)) = image {
        bytes.resize(pixel_at as usize, 0);
        for &sample in pixels {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    Ok(bytes)
}
