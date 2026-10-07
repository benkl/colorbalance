mod tests {
    use crate::metadata::{ExifEntry, ExifIfd, ExifValue, ExportMetadata};
    use crate::output::{encode_jpeg_rgb_u16, encode_tiff_rgb_u16_with_metadata, JpegSampling};

    fn read_u16(data: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(data[at..at + 2].try_into().unwrap())
    }
    fn read_u32(data: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
    }
    fn entry(data: &[u8], ifd: usize, tag: u16) -> Option<(u16, u32, usize)> {
        let count = read_u16(data, ifd) as usize;
        (0..count)
            .map(|index| ifd + 2 + 12 * index)
            .find(|&p| read_u16(data, p) == tag)
            .map(|p| (read_u16(data, p + 2), read_u32(data, p + 4), p + 8))
    }

    #[test]
    fn rebuilt_ifds_point_to_data_and_ignore_unreviewed_tags() {
        let mut metadata = ExportMetadata::default();
        metadata.exif.extend([
            ExifEntry {
                ifd: ExifIfd::Primary,
                tag: 271,
                value: ExifValue::Ascii(b"Camera\0".to_vec()),
            },
            ExifEntry {
                ifd: ExifIfd::Primary,
                tag: 274,
                value: ExifValue::Short(vec![6]),
            },
            ExifEntry {
                ifd: ExifIfd::Exif,
                tag: 36867,
                value: ExifValue::Ascii(b"2024:02:01 09:11:12\0".to_vec()),
            },
            ExifEntry {
                ifd: ExifIfd::Exif,
                tag: 37500,
                value: ExifValue::Undefined(b"secret".to_vec()),
            },
            ExifEntry {
                ifd: ExifIfd::Gps,
                tag: 1,
                value: ExifValue::Ascii(b"N\0".to_vec()),
            },
            ExifEntry {
                ifd: ExifIfd::Gps,
                tag: 2,
                value: ExifValue::Rational(vec![(51, 1), (30, 1), (0, 1)]),
            },
        ]);
        metadata.xmp = Some(b"<x:xmpmeta/>".to_vec());
        metadata.iptc = Some(vec![0x1c, 2, 5, 0, 1, b'A']);
        let tiff = encode_tiff_rgb_u16_with_metadata(1, 1, &[100, 200, 300], b"profile", &metadata)
            .unwrap();
        assert_eq!(&tiff[..4], b"II*\0");
        let root = read_u32(&tiff, 4) as usize;
        assert!(entry(&tiff, root, 274).is_none());
        let (_, _, make) = entry(&tiff, root, 271).unwrap();
        assert_eq!(&tiff[read_u32(&tiff, make) as usize..][..7], b"Camera\0");
        let (_, _, exif_ptr) = entry(&tiff, root, 34665).unwrap();
        let exif = read_u32(&tiff, exif_ptr) as usize;
        let (_, _, date) = entry(&tiff, exif, 36867).unwrap();
        assert_eq!(
            &tiff[read_u32(&tiff, date) as usize..][..20],
            b"2024:02:01 09:11:12\0"
        );
        assert!(entry(&tiff, exif, 37500).is_none());
        let (_, _, gps_ptr) = entry(&tiff, root, 34853).unwrap();
        let gps = read_u32(&tiff, gps_ptr) as usize;
        let (gps_ref_kind, gps_ref_count, gps_ref_ptr) = entry(&tiff, gps, 1).unwrap();
        assert_eq!(
            (
                gps_ref_kind,
                gps_ref_count,
                &tiff[gps_ref_ptr..gps_ref_ptr + 2]
            ),
            (2, 2, b"N\0".as_slice())
        );
        let (kind, count, iptc) = entry(&tiff, root, 33723).unwrap();
        assert_eq!((kind, count), (4, 2));
        assert_eq!(
            &tiff[read_u32(&tiff, iptc) as usize..][..6],
            &[0x1c, 2, 5, 0, 1, b'A']
        );
        assert!(entry(&tiff, root, 700).is_some());
        let (_, _, strip_ptr) = entry(&tiff, root, 273).unwrap();
        let strip = read_u32(&tiff, strip_ptr) as usize;
        assert_eq!(&tiff[strip..strip + 6], &[100, 0, 200, 0, 44, 1]);
    }

    #[test]
    fn jpeg_has_icc_exif_xmp_and_iptc_app_segments() {
        let metadata = ExportMetadata {
            exif: vec![ExifEntry {
                ifd: ExifIfd::Primary,
                tag: 271,
                value: ExifValue::Ascii(b"Maker\0".to_vec()),
            }],
            xmp: Some(b"<x:xmpmeta/>".to_vec()),
            iptc: Some(vec![0x1c, 2, 5, 0, 1, b'A']),
        };
        let jpeg = encode_jpeg_rgb_u16(
            1,
            1,
            &[65535, 0, 0],
            b"profile",
            95,
            JpegSampling::Yuv444,
            &metadata,
        )
        .unwrap();
        assert_eq!(&jpeg[..2], &[0xff, 0xd8]);
        let mut at = 2;
        let mut icc = false;
        let mut exif = false;
        let mut xmp = false;
        let mut iptc = false;
        while at + 4 <= jpeg.len() && jpeg[at] == 0xff && jpeg[at + 1] != 0xda {
            let marker = jpeg[at + 1];
            let length = u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]) as usize;
            let block = &jpeg[at + 4..at + 2 + length];
            if marker == 0xe2 && block.starts_with(b"ICC_PROFILE\0") {
                icc = true;
            }
            if marker == 0xe1 && block.starts_with(b"Exif\0\0II*\0") {
                exif = true;
            }
            if marker == 0xe1 && block.starts_with(b"http://ns.adobe.com/xap/1.0/\0") {
                xmp = true;
            }
            if marker == 0xed && block.starts_with(b"Photoshop 3.0\0") {
                iptc = true;
            }
            at += 2 + length;
        }
        assert!(icc && exif && xmp && iptc);
    }

    #[test]
    fn empty_metadata_keeps_icc_and_pixels() {
        let metadata = ExportMetadata::default();
        let tiff =
            encode_tiff_rgb_u16_with_metadata(1, 1, &[0, 32768, 65535], b"ICC", &metadata).unwrap();
        let root = read_u32(&tiff, 4) as usize;
        assert!(entry(&tiff, root, 34665).is_none());
        assert!(entry(&tiff, root, 34853).is_none());
        assert!(entry(&tiff, root, 34675).is_some());
        let jpeg = encode_jpeg_rgb_u16(
            1,
            1,
            &[0, 32768, 65535],
            b"ICC",
            95,
            JpegSampling::Yuv444,
            &metadata,
        )
        .unwrap();
        assert_eq!(&jpeg[..2], &[0xff, 0xd8]);
        assert_eq!(&jpeg[jpeg.len() - 2..], &[0xff, 0xd9]);
    }

    #[test]
    fn rejects_invalid_jpeg_dimensions_and_quality() {
        let image = [0, 0, 0];
        assert!(encode_jpeg_rgb_u16(
            1,
            1,
            &image,
            b"icc",
            0,
            JpegSampling::Yuv444,
            &ExportMetadata::default()
        )
        .is_err());
        assert!(encode_jpeg_rgb_u16(
            65536,
            1,
            &image,
            b"icc",
            95,
            JpegSampling::Yuv444,
            &ExportMetadata::default()
        )
        .is_err());
    }
}
