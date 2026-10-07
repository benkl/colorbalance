//! Capture facts (camera, lens, time, GPS) read from a file's EXIF for the
//! calibration library. Unlike [`crate::read_export_metadata`] this keeps
//! decoded values, not raw EXIF entries, and never reports an error: a file
//! without readable EXIF yields an empty [`CaptureInfo`].

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use exif::{Exif, In, Tag, Value};

/// A GPS position in decimal degrees. South and west are negative.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptureGps {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres above sea level; negative below. `None` when absent.
    pub altitude: Option<f64>,
}

/// Capture facts from the primary IFD and the Exif and GPS sub-IFDs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaptureInfo {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    /// `DateTimeOriginal` as `YYYY-MM-DD HH:MM:SS`, local time with no zone.
    pub date_time: Option<String>,
    /// Present only when both latitude and longitude are valid.
    pub gps: Option<CaptureGps>,
}

/// Read the capture facts of `path`. Unreadable files, missing EXIF and
/// malformed fields give `None` for the affected values.
pub fn read_capture_info(path: &Path) -> CaptureInfo {
    let Ok(file) = File::open(path) else {
        return CaptureInfo::default();
    };
    let mut reader = BufReader::new(file);
    let mut parser = exif::Reader::new();
    parser.continue_on_error(true);
    let Ok(exif) = parser
        .read_from_container(&mut reader)
        .or_else(|error| error.distill_partial_result(|_| {}))
    else {
        return CaptureInfo::default();
    };
    CaptureInfo {
        make: text(&exif, Tag::Make),
        model: text(&exif, Tag::Model),
        lens: text(&exif, Tag::LensModel),
        date_time: date_time(&exif),
        gps: gps(&exif),
    }
}

fn text(exif: &Exif, tag: Tag) -> Option<String> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let Value::Ascii(values) = &field.value else {
        return None;
    };
    let text = String::from_utf8_lossy(values.first()?);
    let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    (!text.is_empty()).then(|| text.to_owned())
}

fn date_time(exif: &Exif) -> Option<String> {
    let field = exif.get_field(Tag::DateTimeOriginal, In::PRIMARY)?;
    let Value::Ascii(values) = &field.value else {
        return None;
    };
    let stamp = exif::DateTime::from_ascii(values.first()?).ok()?;
    let valid = (1..=12).contains(&stamp.month)
        && (1..=31).contains(&stamp.day)
        && stamp.hour < 24
        && stamp.minute < 60
        && stamp.second < 60;
    valid.then(|| {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            stamp.year, stamp.month, stamp.day, stamp.hour, stamp.minute, stamp.second
        )
    })
}

fn gps(exif: &Exif) -> Option<CaptureGps> {
    let latitude = coordinate(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S', 90.0)?;
    let longitude = coordinate(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W', 180.0)?;
    Some(CaptureGps {
        latitude,
        longitude,
        altitude: altitude(exif),
    })
}

/// Degrees, minutes and seconds with a hemisphere reference to signed degrees.
fn coordinate(exif: &Exif, tag: Tag, reference: Tag, negative: u8, limit: f64) -> Option<f64> {
    let Value::Rational(parts) = &exif.get_field(tag, In::PRIMARY)?.value else {
        return None;
    };
    let [degrees, minutes, seconds] = parts.as_slice() else {
        return None;
    };
    let magnitude = degrees.to_f64() + minutes.to_f64() / 60.0 + seconds.to_f64() / 3600.0;
    let Value::Ascii(reference) = &exif.get_field(reference, In::PRIMARY)?.value else {
        return None;
    };
    let hemisphere = reference.first()?.first()?.to_ascii_uppercase();
    let value = if hemisphere == negative {
        -magnitude
    } else {
        magnitude
    };
    (value.is_finite() && value.abs() <= limit).then_some(value)
}

fn altitude(exif: &Exif) -> Option<f64> {
    let Value::Rational(values) = &exif.get_field(Tag::GPSAltitude, In::PRIMARY)?.value else {
        return None;
    };
    let metres = values.first()?.to_f64();
    // GPSAltitudeRef: 0 above sea level (also the default), 1 below.
    let below = matches!(
        exif.get_field(Tag::GPSAltitudeRef, In::PRIMARY).map(|f| &f.value),
        Some(Value::Byte(bytes)) if bytes.first() == Some(&1)
    );
    metres
        .is_finite()
        .then_some(if below { -metres } else { metres })
}

#[cfg(test)]
mod tests {
    use super::*;
    use colorbalance_core::metadata::{ExifEntry, ExifIfd, ExifValue, ExportMetadata};
    use colorbalance_core::output::encode_tiff_rgb_u16_with_metadata;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn ascii(text: &str) -> ExifValue {
        ExifValue::Ascii(format!("{text}\0").into_bytes())
    }

    fn entry(ifd: ExifIfd, tag: u16, value: ExifValue) -> ExifEntry {
        ExifEntry { ifd, tag, value }
    }

    fn dms(degrees: u32, minutes: u32, seconds: u32) -> ExifValue {
        ExifValue::Rational(vec![(degrees, 1), (minutes, 1), (seconds, 1)])
    }

    /// Write a TIFF carrying `exif` and read its capture info back.
    fn capture(exif: Vec<ExifEntry>) -> CaptureInfo {
        let bytes = encode_tiff_rgb_u16_with_metadata(
            1,
            1,
            &[0, 0, 0],
            b"ICC",
            &ExportMetadata {
                exif,
                ..Default::default()
            },
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "colorbalance-capture-{}-{}.tiff",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, bytes).unwrap();
        let info = read_capture_info(&path);
        std::fs::remove_file(path).unwrap();
        info
    }

    #[test]
    fn reads_camera_lens_time_and_gps() {
        let info = capture(vec![
            entry(ExifIfd::Primary, 271, ascii("Canon")),
            entry(ExifIfd::Primary, 272, ascii("EOS R5")),
            entry(ExifIfd::Exif, 36867, ascii("2024:03:09 14:05:59")),
            entry(ExifIfd::Exif, 42036, ascii("RF24-70mm F2.8")),
            entry(ExifIfd::Gps, 1, ascii("N")),
            entry(ExifIfd::Gps, 2, dms(48, 30, 36)),
            entry(ExifIfd::Gps, 3, ascii("E")),
            entry(ExifIfd::Gps, 4, dms(2, 21, 0)),
            entry(ExifIfd::Gps, 6, ExifValue::Rational(vec![(355, 10)])),
        ]);
        assert_eq!(info.make.as_deref(), Some("Canon"));
        assert_eq!(info.model.as_deref(), Some("EOS R5"));
        assert_eq!(info.lens.as_deref(), Some("RF24-70mm F2.8"));
        assert_eq!(info.date_time.as_deref(), Some("2024-03-09 14:05:59"));
        let gps = info.gps.unwrap();
        assert!((gps.latitude - 48.51).abs() < 1e-9);
        assert!((gps.longitude - 2.35).abs() < 1e-9);
        assert_eq!(gps.altitude, Some(35.5));
    }

    #[test]
    fn south_west_and_below_sea_level_are_negative() {
        let info = capture(vec![
            entry(ExifIfd::Gps, 1, ascii("S")),
            entry(ExifIfd::Gps, 2, dms(33, 0, 0)),
            entry(ExifIfd::Gps, 3, ascii("W")),
            entry(ExifIfd::Gps, 4, dms(70, 30, 0)),
            entry(ExifIfd::Gps, 5, ExifValue::Byte(vec![1])),
            entry(ExifIfd::Gps, 6, ExifValue::Rational(vec![(12, 1)])),
        ]);
        let gps = info.gps.unwrap();
        assert_eq!(gps.latitude, -33.0);
        assert_eq!(gps.longitude, -70.5);
        assert_eq!(gps.altitude, Some(-12.0));
    }

    #[test]
    fn incomplete_or_invalid_gps_is_dropped() {
        // No longitude.
        let partial = capture(vec![
            entry(ExifIfd::Gps, 1, ascii("N")),
            entry(ExifIfd::Gps, 2, dms(10, 0, 0)),
        ]);
        assert_eq!(partial.gps, None);
        // Latitude outside the valid range.
        let out_of_range = capture(vec![
            entry(ExifIfd::Gps, 1, ascii("N")),
            entry(ExifIfd::Gps, 2, dms(95, 0, 0)),
            entry(ExifIfd::Gps, 3, ascii("E")),
            entry(ExifIfd::Gps, 4, dms(1, 0, 0)),
        ]);
        assert_eq!(out_of_range.gps, None);
    }

    #[test]
    fn blank_or_invalid_values_are_absent() {
        let info = capture(vec![
            entry(ExifIfd::Primary, 271, ascii("  ")),
            entry(ExifIfd::Exif, 36867, ascii("2024:13:40 99:99:99")),
        ]);
        assert_eq!(info, CaptureInfo::default());
    }

    #[test]
    fn a_missing_or_exif_free_file_gives_empty_info() {
        let missing = std::env::temp_dir().join("colorbalance-capture-does-not-exist.tiff");
        assert_eq!(read_capture_info(&missing), CaptureInfo::default());
        assert_eq!(capture(Vec::new()), CaptureInfo::default());
    }
}
