//! Container metadata is read independently of pixels. A bad optional block never prevents export.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use colorbalance_core::metadata::{
    valid_value, ExifEntry, ExifIfd, ExifValue, ExportMetadata, MetadataReport,
};
use exif::{Context, Value};
use flate2::read::ZlibDecoder;

const MAX_BLOCK: usize = 16 * 1024 * 1024;
const XMP_APP1: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const PHOTOSHOP: &[u8] = b"Photoshop 3.0\0";
const MAX_XMP_JPEG: usize = 65533 - XMP_APP1.len();
const MAX_IPTC_JPEG: usize = 65533 - PHOTOSHOP.len() - 12 - 1;

#[derive(Debug, Default)]
pub struct MetadataRead {
    pub metadata: ExportMetadata,
    pub report: MetadataReport,
}

impl MetadataRead {
    fn xmp(&mut self, data: &[u8]) {
        if self.metadata.xmp.is_some() {
            return;
        }
        if data.len() > MAX_XMP_JPEG {
            self.report
                .skipped
                .push("XMP (too large for JPEG APP1)".into());
            return;
        }
        if data.is_empty() || std::str::from_utf8(data).is_err() {
            self.report.skipped.push("XMP (invalid packet)".into());
            return;
        }
        self.metadata.xmp = Some(data.to_vec());
        self.report.copied.push("XMP".into());
    }

    fn iptc(&mut self, data: &[u8]) {
        if self.metadata.iptc.is_some() {
            return;
        }
        if data.len() > MAX_IPTC_JPEG {
            self.report
                .skipped
                .push("IPTC (too large for JPEG APP13)".into());
            return;
        }
        if !valid_iptc(data) {
            self.report
                .skipped
                .push("IPTC (invalid IIM dataset)".into());
            return;
        }
        self.metadata.iptc = Some(data.to_vec());
        self.report.copied.push("IPTC".into());
    }
}

/// Read allowed EXIF and optional XMP/IPTC. Failures are recorded, not propagated.
pub fn read_export_metadata(path: &Path, include_xmp_iptc: bool, strip_gps: bool) -> MetadataRead {
    let mut output = MetadataRead::default();
    match File::open(path) {
        Ok(file) => {
            let mut reader = BufReader::new(file);
            let mut parser = exif::Reader::new();
            parser.continue_on_error(true);
            match parser.read_from_container(&mut reader).or_else(|e| {
                e.distill_partial_result(|errors| {
                    output
                        .report
                        .skipped
                        .extend(errors.into_iter().map(|e| format!("EXIF parse: {e}")));
                })
            }) {
                Ok(exif) => collect_exif(&exif, strip_gps, &mut output),
                Err(exif::Error::NotFound(_)) => {
                    output.report.skipped.push("EXIF (not present)".into())
                }
                Err(e) => output.report.skipped.push(format!("EXIF ({e})")),
            }
            if include_xmp_iptc {
                if let Err(e) = read_aux(&mut reader, &mut output) {
                    output.report.skipped.push(format!("XMP/IPTC ({e})"));
                }
            }
        }
        Err(e) => output.report.skipped.push(format!("metadata source ({e})")),
    }
    output
}

fn collect_exif(exif: &exif::Exif, strip_gps: bool, output: &mut MetadataRead) {
    // Reserve the TIFF header, root and sub-IFD pointers in JPEG's single APP1.
    let mut exif_budget = 60_000usize;
    for field in exif.fields() {
        if field.ifd_num != exif::In::PRIMARY {
            continue;
        } // No source thumbnails or alternate IFDs.
        let ifd = match field.tag.0 {
            Context::Tiff => ExifIfd::Primary,
            Context::Exif => ExifIfd::Exif,
            Context::Gps => ExifIfd::Gps,
            Context::Interop => continue,
            _ => continue,
        };
        let tag = field.tag.1;
        let name = field.tag.to_string();
        if ifd == ExifIfd::Gps && strip_gps {
            output.report.skipped.push(format!("{name} (GPS stripped)"));
            continue;
        }
        if !colorbalance_core::metadata::allowed_tag(ifd, tag) {
            // Ignore image layout and ordinary raw-processing metadata, but report reviewed exclusions.
            if matches!(tag, 274 | 37500 | 40961 | 41987 | 513 | 514) || ifd == ExifIfd::Gps {
                output.report.skipped.push(name);
            }
            continue;
        }
        let source_size = match &field.value {
            Value::Ascii(parts) => parts.iter().map(Vec::len).sum::<usize>(),
            Value::Byte(v) | Value::Undefined(v, _) => v.len(),
            Value::Short(v) => v.len().saturating_mul(2),
            Value::Long(v) => v.len().saturating_mul(4),
            Value::Rational(v) => v.len().saturating_mul(8),
            Value::SRational(v) => v.len().saturating_mul(8),
            _ => 0,
        };
        if source_size > exif_budget || source_size > 65500 {
            output
                .report
                .skipped
                .push(format!("{name} (too large for JPEG EXIF)"));
            continue;
        }
        let value = match &field.value {
            Value::Ascii(v) if v.len() == 1 => {
                let mut s = v[0].clone();
                if !s.ends_with(&[0]) {
                    s.push(0);
                }
                ExifValue::Ascii(s)
            }
            Value::Byte(v) => ExifValue::Byte(v.clone()),
            Value::Short(v) => ExifValue::Short(v.clone()),
            Value::Long(v) => ExifValue::Long(v.clone()),
            Value::Rational(v) => ExifValue::Rational(v.iter().map(|r| (r.num, r.denom)).collect()),
            Value::SRational(v) => {
                ExifValue::SRational(v.iter().map(|r| (r.num, r.denom)).collect())
            }
            Value::Undefined(v, _) => ExifValue::Undefined(v.clone()),
            _ => {
                output
                    .report
                    .skipped
                    .push(format!("{name} (unsupported value)"));
                continue;
            }
        };
        if !valid_value(ifd, tag, &value) {
            output
                .report
                .skipped
                .push(format!("{name} (unsupported value)"));
            continue;
        }
        let size = match &value {
            ExifValue::Ascii(v) | ExifValue::Byte(v) | ExifValue::Undefined(v) => v.len(),
            ExifValue::Short(v) => v.len() * 2,
            ExifValue::Long(v) => v.len() * 4,
            ExifValue::Rational(v) => v.len() * 8,
            ExifValue::SRational(v) => v.len() * 8,
        };
        let cost = size.max(4).saturating_add(14);
        if cost > exif_budget {
            output
                .report
                .skipped
                .push(format!("{name} (too large for JPEG EXIF)"));
            continue;
        }
        if output
            .metadata
            .exif
            .iter()
            .any(|e| e.ifd == ifd && e.tag == tag)
        {
            output.report.skipped.push(format!("{name} (duplicate)"));
            continue;
        }
        exif_budget -= cost;
        output.metadata.exif.push(ExifEntry { ifd, tag, value });
        output.report.copied.push(name);
    }
}

fn read_aux(reader: &mut BufReader<File>, output: &mut MetadataRead) -> std::io::Result<()> {
    reader.seek(SeekFrom::Start(0))?;
    let mut magic = [0; 8];
    let n = reader.read(&mut magic)?;
    reader.seek(SeekFrom::Start(0))?;
    if n >= 3 && magic[..3] == [0xff, 0xd8, 0xff] {
        read_jpeg(reader, output)
    } else if n == 8 && magic == *b"\x89PNG\r\n\x1a\n" {
        read_png(reader, output)
    } else if n >= 4 && (&magic[..4] == b"II*\0" || &magic[..4] == b"MM\0*") {
        read_tiff_aux(reader, output)
    } else {
        Ok(())
    }
}

fn read_jpeg(reader: &mut BufReader<File>, output: &mut MetadataRead) -> std::io::Result<()> {
    reader.seek(SeekFrom::Start(2))?;
    loop {
        let mut marker = [0; 1];
        if reader.read_exact(&mut marker).is_err() {
            break;
        }
        if marker[0] != 0xff {
            break;
        }
        reader.read_exact(&mut marker)?;
        while marker[0] == 0xff {
            reader.read_exact(&mut marker)?;
        }
        if marker[0] == 0xda || marker[0] == 0xd9 {
            break;
        }
        if marker[0] == 0x01 || (0xd0..=0xd7).contains(&marker[0]) {
            continue;
        }
        let mut len = [0; 2];
        reader.read_exact(&mut len)?;
        let size = u16::from_be_bytes(len) as usize;
        if size < 2 {
            break;
        }
        if marker[0] == 0xe1 || marker[0] == 0xed {
            let mut block = vec![0; size - 2];
            reader.read_exact(&mut block)?;
            if marker[0] == 0xe1 {
                if let Some(packet) = block.strip_prefix(XMP_APP1) {
                    output.xmp(packet);
                }
            } else if let Some(body) = block.strip_prefix(PHOTOSHOP) {
                read_photoshop_iptc(body, output);
            }
        } else {
            reader.seek(SeekFrom::Current((size - 2) as i64))?;
        }
    }
    Ok(())
}

fn read_photoshop_iptc(mut body: &[u8], output: &mut MetadataRead) {
    while body.len() >= 12 && body.starts_with(b"8BIM") {
        let resource = u16::from_be_bytes([body[4], body[5]]);
        let name_size = body[6] as usize;
        let Some(name_len) = (name_size + 1).checked_add(1).map(|n| n & !1) else {
            return;
        };
        let Some(header_len) = 6usize.checked_add(name_len).and_then(|n| n.checked_add(4)) else {
            return;
        };
        if body.len() < header_len {
            return;
        }
        let at = header_len - 4;
        let size = u32::from_be_bytes(body[at..header_len].try_into().unwrap()) as usize;
        let Some(end) = header_len.checked_add(size) else {
            return;
        };
        if end > body.len() {
            return;
        }
        if resource == 0x0404 {
            output.iptc(&body[header_len..end]);
        }
        let Some(padded) = end.checked_add(size & 1) else {
            return;
        };
        if padded > body.len() {
            return;
        }
        body = &body[padded..];
    }
}

fn valid_iptc(data: &[u8]) -> bool {
    let mut pos = 0;
    let mut count = 0;
    while pos < data.len() {
        if data.len() - pos < 5 || data[pos] != 0x1c {
            return false;
        }
        let length = u16::from_be_bytes([data[pos + 3], data[pos + 4]]);
        if length & 0x8000 != 0 {
            return false;
        } // Extended-size datasets cannot be checked here.
        pos += 5;
        let Some(next) = pos.checked_add(length as usize) else {
            return false;
        };
        if next > data.len() {
            return false;
        }
        pos = next;
        count += 1;
    }
    count > 0
}

fn read_png(reader: &mut BufReader<File>, output: &mut MetadataRead) -> std::io::Result<()> {
    reader.seek(SeekFrom::Start(8))?;
    loop {
        let mut header = [0; 8];
        if reader.read_exact(&mut header).is_err() {
            break;
        }
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        if &header[4..] == b"IEND" {
            break;
        }
        if size > MAX_BLOCK && (&header[4..] == b"iTXt" || &header[4..] == b"tEXt") {
            output
                .report
                .skipped
                .push("XMP (PNG chunk too large)".into());
            reader.seek(SeekFrom::Current(size as i64 + 4))?;
            continue;
        }
        if (&header[4..] == b"iTXt" || &header[4..] == b"tEXt") && size <= MAX_BLOCK {
            let mut block = vec![0; size];
            reader.read_exact(&mut block)?;
            let mut crc = [0; 4];
            reader.read_exact(&mut crc)?;
            if !valid_png_crc(&header[4..], &block, &crc) {
                output
                    .report
                    .skipped
                    .push("XMP (PNG chunk CRC mismatch)".into());
                continue;
            }
            if &header[4..] == b"iTXt" {
                read_png_itxt(&block, output);
            }
            if &header[4..] == b"tEXt" {
                if let Some((key, value)) = split_zero(&block) {
                    if key == b"XML:com.adobe.xmp" {
                        output.xmp(value);
                    }
                }
            }
        } else {
            reader.seek(SeekFrom::Current(size as i64 + 4))?;
        }
    }
    Ok(())
}

fn valid_png_crc(kind: &[u8], block: &[u8], expected: &[u8; 4]) -> bool {
    let mut crc = !0u32;
    for &byte in kind.iter().chain(block) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320u32 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    (!crc).to_be_bytes() == *expected
}

fn split_zero(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let pos = bytes.iter().position(|&c| c == 0)?;
    Some((&bytes[..pos], &bytes[pos + 1..]))
}

fn read_png_itxt(data: &[u8], output: &mut MetadataRead) {
    let Some((key, rest)) = split_zero(data) else {
        return;
    };
    if key != b"XML:com.adobe.xmp" || rest.len() < 2 {
        return;
    }
    let compressed = rest[0] == 1 && rest[1] == 0;
    if rest[0] > 1 || rest[1] != 0 {
        output
            .report
            .skipped
            .push("XMP (unsupported PNG compression)".into());
        return;
    }
    let Some((_, rest)) = split_zero(&rest[2..]) else {
        return;
    };
    let Some((_, text)) = split_zero(rest) else {
        return;
    };
    if compressed {
        let mut decoded = Vec::new();
        match ZlibDecoder::new(text)
            .take((MAX_XMP_JPEG + 1) as u64)
            .read_to_end(&mut decoded)
        {
            Ok(_) => output.xmp(&decoded),
            Err(_) => output.report.skipped.push("XMP (invalid PNG zlib)".into()),
        }
    } else {
        output.xmp(text);
    }
}

fn read_tiff_aux(reader: &mut BufReader<File>, output: &mut MetadataRead) -> std::io::Result<()> {
    reader.seek(SeekFrom::Start(0))?;
    let mut head = [0; 8];
    reader.read_exact(&mut head)?;
    let little = &head[..2] == b"II";
    let number = |bytes: &[u8]| -> u32 {
        if little {
            u32::from_le_bytes(bytes.try_into().unwrap())
        } else {
            u32::from_be_bytes(bytes.try_into().unwrap())
        }
    };
    let short = |bytes: &[u8]| -> u16 {
        if little {
            u16::from_le_bytes(bytes.try_into().unwrap())
        } else {
            u16::from_be_bytes(bytes.try_into().unwrap())
        }
    };
    let offset = number(&head[4..8]) as u64;
    let file_size = reader.get_ref().metadata()?.len();
    if offset < 8 || offset.checked_add(2).is_none_or(|end| end > file_size) {
        return Ok(());
    }
    reader.seek(SeekFrom::Start(offset))?;
    let mut count = [0; 2];
    reader.read_exact(&mut count)?;
    let count = short(&count).min(1024);
    for index in 0..count {
        let Some(at) = offset.checked_add(2 + u64::from(index) * 12) else {
            break;
        };
        if at.checked_add(12).is_none_or(|end| end > file_size) {
            break;
        }
        reader.seek(SeekFrom::Start(at))?;
        let mut entry = [0; 12];
        reader.read_exact(&mut entry)?;
        let tag = short(&entry[..2]);
        if tag != 700 && tag != 33723 && tag != 34377 {
            continue;
        }
        let kind = short(&entry[2..4]);
        if (tag == 33723 && kind != 4 && kind != 1) || (tag != 33723 && kind != 1 && kind != 7) {
            output
                .report
                .skipped
                .push(format!("TIFF tag {tag} (unsupported type)"));
            continue;
        }
        let count = number(&entry[4..8]) as usize;
        let size = if kind == 4 {
            count.saturating_mul(4)
        } else {
            count
        };
        if size == 0 || size > MAX_BLOCK {
            output
                .report
                .skipped
                .push(format!("TIFF tag {tag} (invalid size)"));
            continue;
        }
        let block = if size <= 4 {
            entry[8..8 + size].to_vec()
        } else {
            let value_at = number(&entry[8..12]) as u64;
            if value_at
                .checked_add(size as u64)
                .is_none_or(|end| end > file_size)
            {
                output
                    .report
                    .skipped
                    .push(format!("TIFF tag {tag} (invalid offset)"));
                continue;
            }
            reader.seek(SeekFrom::Start(value_at))?;
            let mut bytes = vec![0; size];
            reader.read_exact(&mut bytes)?;
            bytes
        };
        if tag == 700 {
            output.xmp(&block);
        } else if tag == 33723 {
            if valid_iptc(&block) {
                output.iptc(&block);
            } else {
                // LONG values may have up to three zero padding bytes.
                for pad in 1..=3 {
                    if block.len() >= pad
                        && block[block.len() - pad..].iter().all(|b| *b == 0)
                        && valid_iptc(&block[..block.len() - pad])
                    {
                        output.iptc(&block[..block.len() - pad]);
                        break;
                    }
                }
            }
        } else if let Some(body) = block.strip_prefix(PHOTOSHOP) {
            read_photoshop_iptc(body, output);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use colorbalance_core::metadata::{ExifEntry, ExifIfd, ExifValue};
    use colorbalance_core::output::encode_tiff_rgb_u16_with_metadata;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fixture(data: &[u8], extension: &str) -> std::path::PathBuf {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "colorbalance-metadata-{}-{id}.{extension}",
            std::process::id()
        ));
        std::fs::write(&path, data).unwrap();
        path
    }

    #[test]
    fn reads_reviewed_ifds_and_optional_tiff_blocks() {
        let metadata = ExportMetadata {
            exif: vec![
                ExifEntry {
                    ifd: ExifIfd::Primary,
                    tag: 271,
                    value: ExifValue::Ascii(b"Brand\0".to_vec()),
                },
                ExifEntry {
                    ifd: ExifIfd::Exif,
                    tag: 36867,
                    value: ExifValue::Ascii(b"2024:01:01 12:00:00\0".to_vec()),
                },
                ExifEntry {
                    ifd: ExifIfd::Gps,
                    tag: 1,
                    value: ExifValue::Ascii(b"N\0".to_vec()),
                },
                ExifEntry {
                    ifd: ExifIfd::Gps,
                    tag: 2,
                    value: ExifValue::Rational(vec![(51, 1), (0, 1), (0, 1)]),
                },
            ],
            xmp: Some(b"<x:xmpmeta/>".to_vec()),
            iptc: Some(vec![0x1c, 2, 5, 0, 1, b'A']),
        };
        let bytes = encode_tiff_rgb_u16_with_metadata(1, 1, &[0, 0, 0], b"ICC", &metadata).unwrap();
        let path = fixture(&bytes, "tiff");
        let found = read_export_metadata(&path, true, false);
        assert!(found
            .metadata
            .exif
            .iter()
            .any(|e| e.ifd == ExifIfd::Exif && e.tag == 36867));
        assert!(found
            .metadata
            .exif
            .iter()
            .any(|e| e.ifd == ExifIfd::Gps && e.tag == 2));
        assert_eq!(found.metadata.xmp, metadata.xmp);
        assert_eq!(found.metadata.iptc, metadata.iptc);
        let filtered = read_export_metadata(&path, false, true);
        assert!(filtered.metadata.exif.iter().all(|e| e.ifd != ExifIfd::Gps));
        assert!(filtered.metadata.xmp.is_none() && filtered.metadata.iptc.is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reads_jpeg_exif_xmp_and_photoshop_iptc() {
        use colorbalance_core::output::{encode_jpeg_rgb_u16, JpegSampling};
        let source = ExportMetadata {
            exif: vec![ExifEntry {
                ifd: ExifIfd::Primary,
                tag: 271,
                value: ExifValue::Ascii(b"Brand\0".to_vec()),
            }],
            xmp: Some(b"<x:xmpmeta/>".to_vec()),
            iptc: Some(vec![0x1c, 2, 5, 0, 1, b'A']),
        };
        let bytes =
            encode_jpeg_rgb_u16(1, 1, &[0, 0, 0], b"ICC", 95, JpegSampling::Yuv444, &source)
                .unwrap();
        let path = fixture(&bytes, "jpg");
        let found = read_export_metadata(&path, true, false);
        assert!(found.metadata.exif.iter().any(|e| e.tag == 271));
        assert_eq!(found.metadata.xmp, source.xmp);
        assert_eq!(found.metadata.iptc, source.iptc);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reads_png_itxt_xmp_and_ignores_corrupt_crc() {
        fn chunk(bytes: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
            bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            bytes.extend_from_slice(kind);
            bytes.extend_from_slice(payload);
            let mut crc = !0u32;
            for &byte in kind.iter().chain(payload) {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xedb88320u32 & (0u32.wrapping_sub(crc & 1)));
                }
            }
            bytes.extend_from_slice(&(!crc).to_be_bytes());
        }
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(
            &mut png,
            b"iTXt",
            b"XML:com.adobe.xmp\0\0\0\0\0<x:xmpmeta/>",
        );
        chunk(&mut png, b"IEND", b"");
        let path = fixture(&png, "png");
        assert_eq!(
            read_export_metadata(&path, true, false)
                .metadata
                .xmp
                .as_deref(),
            Some(b"<x:xmpmeta/>".as_slice())
        );
        std::fs::remove_file(&path).unwrap();
        let crc_at = 8 + 8 + b"XML:com.adobe.xmp\0\0\0\0\0<x:xmpmeta/>".len();
        png[crc_at] ^= 1;
        std::fs::write(&path, png).unwrap();
        let read = read_export_metadata(&path, true, false);
        assert!(read.metadata.xmp.is_none());
        assert!(read.report.skipped.iter().any(|s| s.contains("CRC")));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_exif_and_auxiliary_data_never_fail() {
        let path = fixture(b"II*\0\xff\xff\xff\xff", "dng");
        let result = read_export_metadata(&path, true, false);
        assert!(result.metadata.is_empty());
        assert!(!result.report.skipped.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reads_photoshop_iptc_without_trusting_malformed_resource_length() {
        let mut block = b"Photoshop 3.0\0".to_vec();
        block.extend_from_slice(b"8BIM\x04\x04\0\0\0\0\0\x06\x1c\x02\x05\0\x01A");
        let mut output = MetadataRead::default();
        read_photoshop_iptc(block.strip_prefix(PHOTOSHOP).unwrap(), &mut output);
        assert_eq!(output.metadata.iptc.unwrap(), &[0x1c, 2, 5, 0, 1, b'A']);
        let mut output = MetadataRead::default();
        read_photoshop_iptc(b"8BIM\x04\x04\0\0\xff\xff\xff\xff", &mut output);
        assert!(output.metadata.iptc.is_none());
    }
}
