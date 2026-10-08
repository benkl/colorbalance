//! Read-only batch metadata scan. A matching camera or EXIF value cannot
//! establish matching illumination, exposure, decode settings, or filter use.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use colorbalance_core::{collect_inputs, profile, DEFAULT_EXTENSIONS};
use colorbalance_raw::{read_capture_info, sniff_source, CaptureInfo, SourceKind};
use serde::Serialize;

use crate::commands::BackendError;
use crate::library::{self, LibraryEntry};

const MAX_FILES: usize = 200;
const MAX_DISTINCT: usize = 100;
const MAX_SUGGESTIONS: usize = 50;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightFile {
    pub path: String,
    pub has_capture_metadata: bool,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub captured_at: Option<String>,
    /// `None` means the camera identity cannot be compared from EXIF.
    pub camera_mismatch: Option<bool>,
    /// `None` means either the file or the active library reference lacks a lens.
    pub lens_mismatch: Option<bool>,
    /// `None` means either ISO value is unavailable.
    pub iso_mismatch: Option<bool>,
    /// A different local clock reading, not a measured lighting difference.
    pub capture_time_different: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingFields {
    pub capture_metadata: usize,
    pub camera_make: usize,
    pub camera_model: usize,
    pub lens: usize,
    pub iso: usize,
    pub captured_at: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IsoRange {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTimeRange {
    /// Wall-clock time with no timezone; this is not an elapsed duration.
    pub earliest: String,
    pub latest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveLibraryEntry {
    pub id: String,
    pub label: String,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub captured_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySuggestion {
    pub entry_id: String,
    pub label: String,
    pub camera_make: String,
    pub camera_model: String,
    pub matching_files: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightResponse {
    pub total_files: usize,
    /// The first 200 files in input-path order; totals include every input.
    pub files: Vec<PreflightFile>,
    pub omitted_files: usize,
    pub missing: MissingFields,
    pub camera_mismatch_count: usize,
    pub lens_mismatch_count: usize,
    pub iso_mismatch_count: usize,
    pub capture_time_different_count: usize,
    /// First 100 distinct values in sorted order, with full counts alongside.
    pub lenses: Vec<String>,
    pub lens_count: usize,
    pub iso_values: Vec<u32>,
    pub iso_count: usize,
    pub iso_range: Option<IsoRange>,
    pub capture_time_range: Option<CaptureTimeRange>,
    /// Present only when the active profile is the saved file in this library.
    pub active_library_entry: Option<ActiveLibraryEntry>,
    /// Camera make and model matches only; not a claim of usable calibration.
    pub suggestions: Vec<LibrarySuggestion>,
}

/// Scan EXIF only. No image is decoded or written, and no mismatch is inferred
/// from missing metadata. Batch collection follows the apply command exactly.
pub fn scan_batch(
    input_path: String,
    library_path: Option<String>,
    profile_path: String,
) -> Result<PreflightResponse, BackendError> {
    let loaded = profile::from_json(&fs::read_to_string(&profile_path)?)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    let extensions = DEFAULT_EXTENSIONS
        .iter()
        .map(|extension| (*extension).to_owned())
        .collect::<Vec<_>>();
    let inputs =
        collect_inputs(Path::new(&input_path), &extensions).map_err(BackendError::Message)?;
    let entries = library_path
        .as_deref()
        .map(|path| library::list_library(Path::new(path)))
        .transpose()?
        .map_or_else(Vec::new, |listing| listing.entries);
    let active = active_entry(&entries, Path::new(&profile_path));
    let camera_comparable = loaded.camera.decoder != colorbalance_raw::JPEG_DECODER_NAME;
    let mut scan = BatchScan::new(
        &loaded.camera.make,
        &loaded.camera.model,
        camera_comparable,
        active,
        &entries,
    );
    for path in inputs {
        let info = read_capture_info(&path);
        let source_kind = sniff_source(&path).ok();
        scan.add_with_kind(path, info, source_kind);
    }
    Ok(scan.finish())
}

fn active_entry<'a>(entries: &'a [LibraryEntry], profile_path: &Path) -> Option<&'a LibraryEntry> {
    let active_path = fs::canonicalize(profile_path).ok()?;
    entries
        .iter()
        .find(|entry| fs::canonicalize(&entry.profile_path).is_ok_and(|path| path == active_path))
}

/// Case-insensitive exact comparison. No substring matching: "R5" must not equal "R50".
fn same_name(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    !a.is_empty() && a.eq_ignore_ascii_case(b)
}

fn same_camera(make: &str, model: &str, other_make: &str, other_model: &str) -> bool {
    same_name(make, other_make) && same_name(model, other_model)
}

struct BatchScan<'a> {
    camera_make: &'a str,
    camera_model: &'a str,
    /// False for rendered-image profiles, whose stored identity is synthetic
    /// and says nothing about the EXIF camera.
    camera_comparable: bool,
    active: Option<&'a LibraryEntry>,
    entries: &'a [LibraryEntry],
    total_files: usize,
    files: Vec<PreflightFile>,
    missing: MissingFields,
    camera_mismatch_count: usize,
    lens_mismatch_count: usize,
    iso_mismatch_count: usize,
    capture_time_different_count: usize,
    lenses: BTreeSet<String>,
    iso_values: BTreeSet<u32>,
    times: BTreeSet<String>,
    camera_counts: BTreeMap<(String, String), usize>,
}

impl<'a> BatchScan<'a> {
    fn new(
        camera_make: &'a str,
        camera_model: &'a str,
        camera_comparable: bool,
        active: Option<&'a LibraryEntry>,
        entries: &'a [LibraryEntry],
    ) -> Self {
        Self {
            camera_make,
            camera_model,
            camera_comparable,
            active,
            entries,
            total_files: 0,
            files: Vec::new(),
            missing: MissingFields::default(),
            camera_mismatch_count: 0,
            lens_mismatch_count: 0,
            iso_mismatch_count: 0,
            capture_time_different_count: 0,
            lenses: BTreeSet::new(),
            iso_values: BTreeSet::new(),
            times: BTreeSet::new(),
            camera_counts: BTreeMap::new(),
        }
    }

    #[cfg(test)]
    fn add(&mut self, path: PathBuf, info: CaptureInfo) {
        self.add_with_kind(path, info, Some(SourceKind::Raw));
    }

    fn add_with_kind(&mut self, path: PathBuf, info: CaptureInfo, source_kind: Option<SourceKind>) {
        self.total_files += 1;
        let has_capture_metadata = info.make.is_some()
            || info.model.is_some()
            || info.lens.is_some()
            || info.iso.is_some()
            || info.date_time.is_some()
            || info.gps.is_some();
        self.missing.capture_metadata += usize::from(!has_capture_metadata);
        self.missing.camera_make += usize::from(info.make.is_none());
        self.missing.camera_model += usize::from(info.model.is_none());
        self.missing.lens += usize::from(info.lens.is_none());
        self.missing.iso += usize::from(info.iso.is_none());
        self.missing.captured_at += usize::from(info.date_time.is_none());

        if let (Some(make), Some(model)) = (&info.make, &info.model) {
            *self
                .camera_counts
                .entry((make.clone(), model.clone()))
                .or_default() += 1;
        }
        let camera_mismatch = match source_kind {
            None => None,
            Some(SourceKind::Rendered) if self.camera_comparable => Some(true),
            Some(SourceKind::Raw) if !self.camera_comparable => Some(true),
            Some(SourceKind::Rendered) => None, // Rendered profiles have synthetic camera identities.
            Some(SourceKind::Raw) => match (&info.make, &info.model) {
                (Some(make), Some(model)) => Some(!same_camera(
                    make,
                    model,
                    self.camera_make,
                    self.camera_model,
                )),
                (Some(make), None) if !same_name(make, self.camera_make) => Some(true),
                (None, Some(model)) if !same_name(model, self.camera_model) => Some(true),
                _ => None,
            },
        };
        let lens_mismatch = self
            .active
            .and_then(|entry| Some(info.lens.as_ref()? != entry.lens.as_ref()?));
        let iso_mismatch = self.active.and_then(|entry| Some(info.iso? != entry.iso?));
        let capture_time_different = self
            .active
            .and_then(|entry| Some(info.date_time.as_ref()? != entry.captured_at.as_ref()?));
        self.camera_mismatch_count += usize::from(camera_mismatch == Some(true));
        self.lens_mismatch_count += usize::from(lens_mismatch == Some(true));
        self.iso_mismatch_count += usize::from(iso_mismatch == Some(true));
        self.capture_time_different_count += usize::from(capture_time_different == Some(true));
        if let Some(lens) = &info.lens {
            self.lenses.insert(lens.clone());
        }
        if let Some(iso) = info.iso {
            self.iso_values.insert(iso);
        }
        if let Some(time) = &info.date_time {
            self.times.insert(time.clone());
        }
        if self.files.len() < MAX_FILES {
            self.files.push(PreflightFile {
                path: path.to_string_lossy().into_owned(),
                has_capture_metadata,
                camera_make: info.make,
                camera_model: info.model,
                lens: info.lens,
                iso: info.iso,
                captured_at: info.date_time,
                camera_mismatch,
                lens_mismatch,
                iso_mismatch,
                capture_time_different,
            });
        }
    }

    fn finish(self) -> PreflightResponse {
        let mut suggestions = self
            .entries
            .iter()
            .filter_map(|entry| {
                let matching_files: usize = self
                    .camera_counts
                    .iter()
                    .filter(|((make, model), _)| {
                        same_camera(make, model, &entry.camera_make, &entry.camera_model)
                    })
                    .map(|(_, count)| *count)
                    .sum();
                if matching_files == 0 {
                    return None;
                }
                Some(LibrarySuggestion {
                    entry_id: entry.id.clone(),
                    label: entry.label.clone(),
                    camera_make: entry.camera_make.clone(),
                    camera_model: entry.camera_model.clone(),
                    matching_files,
                })
            })
            .collect::<Vec<_>>();
        suggestions.sort_by(|a, b| {
            b.matching_files
                .cmp(&a.matching_files)
                .then_with(|| a.label.cmp(&b.label))
                .then_with(|| a.entry_id.cmp(&b.entry_id))
        });
        suggestions.truncate(MAX_SUGGESTIONS);
        let iso_range = self
            .iso_values
            .first()
            .zip(self.iso_values.last())
            .map(|(min, max)| IsoRange {
                min: *min,
                max: *max,
            });
        let capture_time_range =
            self.times
                .first()
                .zip(self.times.last())
                .map(|(earliest, latest)| CaptureTimeRange {
                    earliest: earliest.clone(),
                    latest: latest.clone(),
                });
        PreflightResponse {
            total_files: self.total_files,
            omitted_files: self.total_files - self.files.len(),
            files: self.files,
            missing: self.missing,
            camera_mismatch_count: self.camera_mismatch_count,
            lens_mismatch_count: self.lens_mismatch_count,
            iso_mismatch_count: self.iso_mismatch_count,
            capture_time_different_count: self.capture_time_different_count,
            lens_count: self.lenses.len(),
            lenses: self.lenses.into_iter().take(MAX_DISTINCT).collect(),
            iso_count: self.iso_values.len(),
            iso_values: self.iso_values.into_iter().take(MAX_DISTINCT).collect(),
            iso_range,
            capture_time_range,
            active_library_entry: self.active.map(|entry| ActiveLibraryEntry {
                id: entry.id.clone(),
                label: entry.label.clone(),
                lens: entry.lens.clone(),
                iso: entry.iso,
                captured_at: entry.captured_at.clone(),
            }),
            suggestions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, make: &str, model: &str) -> LibraryEntry {
        LibraryEntry {
            id: id.into(),
            label: id.into(),
            notes: String::new(),
            tags: vec![],
            profile_path: String::new(),
            preview_path: None,
            digest: String::new(),
            camera_make: make.into(),
            camera_model: model.into(),
            lens: Some("35mm".into()),
            iso: Some(200),
            captured_at: Some("2024-01-01 10:00:00".into()),
            gps: None,
            chart_revision: String::new(),
            decoder: String::new(),
            decoder_version: String::new(),
            quality_passed: true,
            quality_overridden: false,
            quick_and_dirty: false,
            mean_delta_e: 0.0,
            max_delta_e: 0.0,
            patch_count: 0,
        }
    }

    fn info(
        make: Option<&str>,
        model: Option<&str>,
        lens: Option<&str>,
        iso: Option<u32>,
        time: Option<&str>,
    ) -> CaptureInfo {
        CaptureInfo {
            make: make.map(str::to_owned),
            model: model.map(str::to_owned),
            lens: lens.map(str::to_owned),
            iso,
            date_time: time.map(str::to_owned),
            gps: None,
        }
    }

    #[test]
    fn baseline_requires_same_saved_profile_path() {
        let dir = tempfile::tempdir().unwrap();
        let saved = dir.path().join("saved.json");
        let copy = dir.path().join("copy.json");
        std::fs::write(&saved, b"saved").unwrap();
        std::fs::write(&copy, b"saved").unwrap();
        let mut entry = entry("saved", "Canon", "R5");
        entry.profile_path = saved.to_string_lossy().into_owned();
        let entries = [entry];
        assert!(active_entry(&entries, &saved).is_some());
        assert!(active_entry(&entries, &copy).is_none());
    }

    #[test]
    fn rendered_profile_camera_is_unknown_not_mismatched() {
        let mut scan = BatchScan::new("Rendered Image (Quick & Dirty)", "JPG", false, None, &[]);
        scan.add_with_kind(
            "a.jpg".into(),
            info(Some("samsung"), Some("Galaxy S25"), None, None, None),
            Some(SourceKind::Rendered),
        );
        let response = scan.finish();
        assert_eq!(response.camera_mismatch_count, 0);
        assert_eq!(response.files[0].camera_mismatch, None);
    }

    #[test]
    fn mixed_source_kinds_are_camera_mismatches() {
        let camera = info(Some("Canon"), Some("R5"), None, None, None);
        let mut raw_profile = BatchScan::new("Canon", "R5", true, None, &[]);
        raw_profile.add_with_kind("a.jpg".into(), camera.clone(), Some(SourceKind::Rendered));
        assert_eq!(raw_profile.finish().files[0].camera_mismatch, Some(true));

        let mut rendered_profile =
            BatchScan::new("Rendered Image (Quick & Dirty)", "JPG", false, None, &[]);
        rendered_profile.add_with_kind("a.dng".into(), camera, Some(SourceKind::Raw));
        assert_eq!(
            rendered_profile.finish().files[0].camera_mismatch,
            Some(true)
        );
    }

    #[test]
    fn camera_names_compare_exactly_ignoring_case() {
        let mut scan = BatchScan::new("Canon", "R5", true, None, &[]);
        scan.add(
            "a".into(),
            info(Some("CANON"), Some("r5"), None, None, None),
        );
        scan.add(
            "b".into(),
            info(Some("Canon"), Some("R50"), None, None, None),
        );
        let response = scan.finish();
        assert_eq!(response.files[0].camera_mismatch, Some(false));
        assert_eq!(response.files[1].camera_mismatch, Some(true));
    }

    #[test]
    fn aggregates_missing_values_and_active_reference_mismatches() {
        let entries = [entry("active", "Canon", "R5")];
        let mut scan = BatchScan::new("Canon", "R5", true, Some(&entries[0]), &entries);
        scan.add(
            "a.dng".into(),
            info(
                Some("Canon"),
                Some("R5"),
                Some("35mm"),
                Some(200),
                Some("2024-01-01 10:00:00"),
            ),
        );
        scan.add(
            "b.dng".into(),
            info(
                Some("Nikon"),
                None,
                Some("50mm"),
                Some(400),
                Some("2024-01-02 10:00:00"),
            ),
        );
        scan.add("c.dng".into(), CaptureInfo::default());
        let result = scan.finish();
        assert_eq!(result.total_files, 3);
        assert_eq!(result.missing.capture_metadata, 1);
        assert_eq!(result.missing.camera_make, 1);
        assert_eq!(result.missing.camera_model, 2);
        assert_eq!(result.missing.iso, 1);
        assert_eq!(result.camera_mismatch_count, 1);
        assert_eq!(result.lens_mismatch_count, 1);
        assert_eq!(result.iso_mismatch_count, 1);
        assert_eq!(result.capture_time_different_count, 1);
        assert_eq!(result.files[1].camera_mismatch, Some(true));
        assert_eq!(result.files[2].lens_mismatch, None);
        assert_eq!(result.iso_range, Some(IsoRange { min: 200, max: 400 }));
        assert_eq!(
            result.capture_time_range,
            Some(CaptureTimeRange {
                earliest: "2024-01-01 10:00:00".into(),
                latest: "2024-01-02 10:00:00".into(),
            })
        );
    }

    #[test]
    fn bare_profile_reports_spread_without_reference_comparisons() {
        let entries = [entry("other", "Canon", "R5")];
        let mut scan = BatchScan::new("Canon", "R5", true, None, &entries);
        scan.add(
            "a.dng".into(),
            info(
                Some("Canon"),
                Some("R5"),
                Some("50mm"),
                Some(400),
                Some("2024-03-01 11:00:00"),
            ),
        );
        let result = scan.finish();
        assert!(result.active_library_entry.is_none());
        assert_eq!(result.files[0].lens_mismatch, None);
        assert_eq!(result.files[0].iso_mismatch, None);
        assert_eq!(result.files[0].capture_time_different, None);
        assert_eq!(result.lenses, ["50mm"]);
        assert_eq!(result.iso_values, [400]);
    }

    #[test]
    fn suggestions_require_complete_camera_identity_and_are_ranked() {
        let entries = [
            entry("a", "Canon", "R5"),
            entry("b", "Nikon", "Z7"),
            entry("c", "Canon", "R5"),
        ];
        let mut scan = BatchScan::new("Canon", "R5", true, None, &entries);
        scan.add(
            "a".into(),
            info(Some("Canon"), Some("R5"), None, None, None),
        );
        scan.add(
            "b".into(),
            info(Some("Canon"), Some("R5"), None, None, None),
        );
        scan.add(
            "c".into(),
            info(Some("Nikon"), Some("Z7"), None, None, None),
        );
        scan.add("d".into(), info(Some("Nikon"), None, None, None, None));
        let result = scan.finish();
        assert_eq!(
            result
                .suggestions
                .iter()
                .map(|s| (s.entry_id.as_str(), s.matching_files))
                .collect::<Vec<_>>(),
            [("a", 2), ("c", 2), ("b", 1)]
        );
        assert_eq!(result.camera_mismatch_count, 2);
    }

    #[test]
    fn large_batch_keeps_full_totals_but_bounds_details() {
        let mut scan = BatchScan::new("Canon", "R5", true, None, &[]);
        for n in 0..250 {
            scan.add(
                format!("{n}.dng").into(),
                info(None, None, Some(&format!("lens-{n}")), Some(n + 1), None),
            );
        }
        let result = scan.finish();
        assert_eq!(result.total_files, 250);
        assert_eq!(result.files.len(), MAX_FILES);
        assert_eq!(result.omitted_files, 50);
        assert_eq!(result.lens_count, 250);
        assert_eq!(result.lenses.len(), MAX_DISTINCT);
        assert_eq!(result.iso_count, 250);
        assert_eq!(result.iso_values.len(), MAX_DISTINCT);
        assert_eq!(result.iso_range, Some(IsoRange { min: 1, max: 250 }));
    }
}
