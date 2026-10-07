//! Reviewed export metadata. Tags carry typed values, never source offsets or IFD pointers.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ExifIfd {
    Primary,
    Exif,
    Gps,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExifValue {
    Ascii(Vec<u8>),
    Byte(Vec<u8>),
    Short(Vec<u16>),
    Long(Vec<u32>),
    Rational(Vec<(u32, u32)>),
    SRational(Vec<(i32, i32)>),
    Undefined(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExifEntry {
    pub ifd: ExifIfd,
    pub tag: u16,
    pub value: ExifValue,
}

/// Metadata from a source image, independent of its original byte order and offsets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportMetadata {
    pub exif: Vec<ExifEntry>,
    /// UTF-8 XMP packet, without the JPEG APP1 namespace prefix.
    pub xmp: Option<Vec<u8>>,
    /// IPTC IIM dataset bytes, without a JPEG Photoshop APP13 wrapper.
    pub iptc: Option<Vec<u8>>,
}

impl ExportMetadata {
    pub fn is_empty(&self) -> bool {
        self.exif.is_empty() && self.xmp.is_none() && self.iptc.is_none()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataReport {
    pub copied: Vec<String>,
    pub skipped: Vec<String>,
}

impl MetadataReport {
    pub fn summary(&self) -> String {
        format!(
            "metadata: copied [{}]; skipped [{}]",
            self.copied.join(", "),
            self.skipped.join(", ")
        )
    }
}

/// Review list. The serializer checks this list again even for caller-created metadata.
pub fn allowed_tag(ifd: ExifIfd, tag: u16) -> bool {
    match ifd {
        ExifIfd::Primary => matches!(tag, 271 | 272 | 306 | 315 | 33432),
        ExifIfd::Exif => matches!(
            tag,
            33434 | 33437 | 34855 | 36867 | 36868 | 37386 | 42034 | 42035 | 42036
        ),
        ExifIfd::Gps => matches!(tag, 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 18 | 29),
    }
}

pub fn valid_value(ifd: ExifIfd, tag: u16, value: &ExifValue) -> bool {
    use ExifValue::*;
    if !allowed_tag(ifd, tag) {
        return false;
    }
    match (ifd, tag, value) {
        (ExifIfd::Primary, _, Ascii(v))
        | (ExifIfd::Exif, 36867 | 36868 | 42035 | 42036, Ascii(v))
        | (ExifIfd::Gps, 1 | 3 | 18 | 29, Ascii(v)) => {
            !v.is_empty()
                && v.len() <= 65535
                && v.last() == Some(&0)
                && (ifd == ExifIfd::Primary && tag == 33432 || !v[..v.len() - 1].contains(&0))
        }
        (ExifIfd::Exif, 34855, Short(v)) => v.len() == 1,
        (ExifIfd::Exif, 34855, Long(v)) => v.len() == 1,
        (ExifIfd::Exif, 33434 | 33437 | 37386, Rational(v)) => v.len() == 1 && v[0].1 != 0,
        (ExifIfd::Exif, 42034, Rational(v)) => v.len() == 4 && v.iter().all(|r| r.1 != 0),
        (ExifIfd::Gps, 0, Byte(v)) => v.len() == 4,
        (ExifIfd::Gps, 2 | 4 | 7, Rational(v)) => v.len() == 3 && v.iter().all(|r| r.1 != 0),
        (ExifIfd::Gps, 6, Rational(v)) => v.len() == 1 && v[0].1 != 0,
        (ExifIfd::Gps, 5, Byte(v)) => v.len() == 1,
        _ => false,
    }
}
