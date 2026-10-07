//! Minimal uncompressed DNG writer used by deterministic fixtures.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

/// Parameters for one uncompressed, single-strip 16-bit CFA DNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DngWriteSpec {
    pub width: u32,
    pub height: u32,
    /// CFA colors in row-major 2 by 2 order, using `b'R'`, `b'G'`, and `b'B'`.
    pub cfa_pattern: [u8; 4],
    pub black_levels: [u16; 4],
    pub white_levels: [u16; 4],
    pub make: String,
    pub model: String,
    /// TIFF orientation. The fixture writer accepts the four orientations the reader supports.
    pub orientation: u16,
    /// Row-major 16-bit mosaic samples.
    pub photosites: Vec<u16>,
}

#[derive(Clone, Copy)]
struct Entry {
    tag: u16,
    field_type: u16,
    count: u32,
    value: u32,
}

fn invalid_input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn append_c_string(data: &mut Vec<u8>, value: &str) -> io::Result<(u32, u32)> {
    if value.as_bytes().contains(&0) {
        return Err(invalid_input("DNG strings cannot contain NUL bytes"));
    }
    let offset = u32::try_from(data.len()).map_err(|_| invalid_input("DNG is too large"))?;
    data.extend_from_slice(value.as_bytes());
    data.push(0);
    let count =
        u32::try_from(value.len() + 1).map_err(|_| invalid_input("DNG string is too long"))?;
    Ok((offset, count))
}

fn write_entry(output: &mut Vec<u8>, entry: Entry) {
    output.extend_from_slice(&entry.tag.to_le_bytes());
    output.extend_from_slice(&entry.field_type.to_le_bytes());
    output.extend_from_slice(&entry.count.to_le_bytes());
    output.extend_from_slice(&entry.value.to_le_bytes());
}

/// Write a little-endian classic TIFF/DNG with one IFD and one uncompressed strip.
pub fn write_dng(path: &Path, spec: &DngWriteSpec) -> io::Result<()> {
    if spec.width == 0 || spec.height == 0 {
        return Err(invalid_input("DNG dimensions must be nonzero"));
    }
    let pixel_count = (spec.width as usize)
        .checked_mul(spec.height as usize)
        .ok_or_else(|| invalid_input("DNG dimensions overflow"))?;
    if spec.photosites.len() != pixel_count {
        return Err(invalid_input("photosite count does not match dimensions"));
    }
    if !matches!(spec.orientation, 1 | 3 | 6 | 8) {
        return Err(invalid_input("orientation must be 1, 3, 6, or 8"));
    }
    if spec
        .cfa_pattern
        .iter()
        .any(|color| !matches!(color, b'R' | b'G' | b'B'))
    {
        return Err(invalid_input("CFA pattern contains an unknown color"));
    }
    if spec
        .black_levels
        .iter()
        .zip(spec.white_levels)
        .any(|(black, white)| *black >= white)
    {
        return Err(invalid_input(
            "each black level must be below its white level",
        ));
    }
    if !spec
        .white_levels
        .iter()
        .all(|level| *level == spec.white_levels[0])
    {
        return Err(invalid_input(
            "minimal DNG writer requires one shared white level",
        ));
    }

    const ENTRY_COUNT: u16 = 18;
    const HEADER_SIZE: u32 = 8;
    let ifd_size = 2_u32 + u32::from(ENTRY_COUNT) * 12 + 4;
    let extra_base = HEADER_SIZE + ifd_size;
    let mut extra = Vec::new();

    let make_relative = append_c_string(&mut extra, &spec.make)?;
    let model_relative = append_c_string(&mut extra, &spec.model)?;
    if extra.len() % 2 != 0 {
        extra.push(0);
    }
    let black_relative =
        u32::try_from(extra.len()).map_err(|_| invalid_input("DNG is too large"))?;
    for level in spec.black_levels {
        extra.extend_from_slice(&level.to_le_bytes());
    }
    let strip_relative =
        u32::try_from(extra.len()).map_err(|_| invalid_input("DNG is too large"))?;
    let strip_byte_count = u32::try_from(
        pixel_count
            .checked_mul(2)
            .ok_or_else(|| invalid_input("DNG strip size overflows"))?,
    )
    .map_err(|_| invalid_input("DNG strip exceeds classic TIFF limits"))?;

    let color_code = |color| match color {
        b'R' => 0_u8,
        b'G' => 1_u8,
        b'B' => 2_u8,
        _ => unreachable!("CFA validated above"),
    };
    let cfa_inline = u32::from_le_bytes(spec.cfa_pattern.map(color_code));
    let dimensions_inline = u32::from_le_bytes([2, 0, 2, 0]);
    let dng_version_inline = u32::from_le_bytes([1, 4, 0, 0]);
    let make_offset = extra_base + make_relative.0;
    let model_offset = extra_base + model_relative.0;
    let black_offset = extra_base + black_relative;
    let strip_offset = extra_base + strip_relative;

    let entries = [
        Entry {
            tag: 256,
            field_type: 4,
            count: 1,
            value: spec.width,
        },
        Entry {
            tag: 257,
            field_type: 4,
            count: 1,
            value: spec.height,
        },
        Entry {
            tag: 258,
            field_type: 3,
            count: 1,
            value: 16,
        },
        Entry {
            tag: 259,
            field_type: 3,
            count: 1,
            value: 1,
        },
        Entry {
            tag: 262,
            field_type: 3,
            count: 1,
            value: 32_803,
        },
        Entry {
            tag: 271,
            field_type: 2,
            count: make_relative.1,
            value: make_offset,
        },
        Entry {
            tag: 272,
            field_type: 2,
            count: model_relative.1,
            value: model_offset,
        },
        Entry {
            tag: 273,
            field_type: 4,
            count: 1,
            value: strip_offset,
        },
        Entry {
            tag: 274,
            field_type: 3,
            count: 1,
            value: u32::from(spec.orientation),
        },
        Entry {
            tag: 277,
            field_type: 3,
            count: 1,
            value: 1,
        },
        Entry {
            tag: 278,
            field_type: 4,
            count: 1,
            value: spec.height,
        },
        Entry {
            tag: 279,
            field_type: 4,
            count: 1,
            value: strip_byte_count,
        },
        Entry {
            tag: 33_421,
            field_type: 3,
            count: 2,
            value: dimensions_inline,
        },
        Entry {
            tag: 33_422,
            field_type: 1,
            count: 4,
            value: cfa_inline,
        },
        Entry {
            tag: 50_706,
            field_type: 1,
            count: 4,
            value: dng_version_inline,
        },
        Entry {
            tag: 50_713,
            field_type: 3,
            count: 2,
            value: dimensions_inline,
        },
        Entry {
            tag: 50_714,
            field_type: 3,
            count: 4,
            value: black_offset,
        },
        Entry {
            tag: 50_717,
            field_type: 3,
            count: 1,
            value: u32::from(spec.white_levels[0]),
        },
    ];

    let capacity = usize::try_from(strip_offset)
        .ok()
        .and_then(|offset| offset.checked_add(strip_byte_count as usize))
        .ok_or_else(|| invalid_input("DNG output size overflows"))?;
    let mut output = Vec::with_capacity(capacity);
    output.extend_from_slice(b"II");
    output.extend_from_slice(&42_u16.to_le_bytes());
    output.extend_from_slice(&HEADER_SIZE.to_le_bytes());
    output.extend_from_slice(&ENTRY_COUNT.to_le_bytes());
    for entry in entries {
        write_entry(&mut output, entry);
    }
    output.extend_from_slice(&0_u32.to_le_bytes());
    output.extend_from_slice(&extra);
    debug_assert_eq!(output.len(), strip_offset as usize);
    for sample in &spec.photosites {
        output.extend_from_slice(&sample.to_le_bytes());
    }

    let mut file = File::create(path)?;
    file.write_all(&output)
}
