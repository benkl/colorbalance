//! Calibration library: one user-chosen folder, one sub-folder per entry.
//!
//! The folder is the source of truth and is rescanned on every listing. An
//! entry directory holds:
//!
//! - `profile.cbprofile.json`: the profile, byte for byte as it was derived.
//!   Loading verifies its digest, so a damaged or edited profile is a problem,
//!   never a usable entry.
//! - `entry.json`: label, notes, tags and capture facts, plus the digest of
//!   the profile it belongs to.
//! - `preview.png`: the corrected reference, optional.
//!
//! Entries are built in a `.tmp-*` sibling directory and renamed into place,
//! so a scan never sees a half-written entry, and saving never touches an
//! existing one. Directories starting with `.` and loose files are ignored.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use colorbalance_core::profile::{self, Profile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::commands::{corrected_preview_png, BackendError};

const PROFILE_FILE: &str = "profile.cbprofile.json";
const ENTRY_FILE: &str = "entry.json";
const PREVIEW_FILE: &str = "preview.png";
const ENTRY_SCHEMA_VERSION: &str = "1.0";
/// Longest side, in pixels, of an entry's preview.
const PREVIEW_MAX_DIM: u32 = 320;
const SLUG_MAX_CHARS: usize = 40;
const MAX_NAME_ATTEMPTS: u32 = 10_000;

/// A GPS position in decimal degrees, south and west negative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryGps {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: Option<f64>,
}

/// One usable library entry, as the gallery shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    /// The entry's directory name, unique within the library.
    pub id: String,
    pub label: String,
    pub notes: String,
    pub tags: Vec<String>,
    pub profile_path: String,
    /// `None` when the entry has no readable preview.
    pub preview_path: Option<String>,
    pub digest: String,
    pub camera_make: String,
    pub camera_model: String,
    pub lens: Option<String>,
    /// Reference ISO, when present in EXIF. Older entries do not have this field.
    pub iso: Option<u32>,
    /// `YYYY-MM-DD HH:MM:SS`, as read from the reference's EXIF.
    pub captured_at: Option<String>,
    pub gps: Option<LibraryGps>,
    /// The profile's chart revision as serialized, for example
    /// `classic-from-november2014`.
    pub chart_revision: String,
    pub decoder: String,
    pub decoder_version: String,
    pub quality_passed: bool,
    pub quality_overridden: bool,
    pub quick_and_dirty: bool,
    pub mean_delta_e: f64,
    pub max_delta_e: f64,
    pub patch_count: u32,
}

/// A library directory that could not be loaded as an entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryProblem {
    pub id: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryListing {
    pub entries: Vec<LibraryEntry>,
    pub problems: Vec<LibraryProblem>,
}

/// `entry.json`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct EntryFile {
    schema_version: String,
    label: String,
    notes: String,
    tags: Vec<String>,
    profile_digest: String,
    capture: CaptureFile,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CaptureFile {
    make: Option<String>,
    model: Option<String>,
    lens: Option<String>,
    iso: Option<u32>,
    date_time: Option<String>,
    gps: Option<LibraryGps>,
}

/// Scan `library` and load every entry directory. One bad entry becomes a
/// [`LibraryProblem`]; it never fails the scan. A library folder that does not
/// exist yet lists as empty.
pub fn list_library(library: &Path) -> Result<LibraryListing, BackendError> {
    let mut listing = LibraryListing::default();
    if !library.exists() {
        return Ok(listing);
    }
    for item in fs::read_dir(library)?.flatten() {
        let id = item.file_name().to_string_lossy().into_owned();
        let path = item.path();
        if id.starts_with('.') || !path.is_dir() {
            continue;
        }
        match load_entry(library, &path, id.clone()) {
            Ok(entry) => listing.entries.push(entry),
            Err(message) => listing.problems.push(LibraryProblem { id, message }),
        }
    }
    listing
        .entries
        .sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.id.cmp(&b.id)));
    listing.problems.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(listing)
}

/// The entry's `preview.png` as a canonical path, or `None` unless it is a
/// regular file that really lives at `<library>/<entry>/preview.png` once every
/// symlink is resolved. A preview that links elsewhere, or an entry folder that
/// links out of the library, yields nothing.
fn verified_preview(library: &Path, dir: &Path) -> Option<PathBuf> {
    let library = fs::canonicalize(library).ok()?;
    let dir = fs::canonicalize(dir).ok()?;
    if dir.parent()? != library {
        return None;
    }
    let expected = dir.join(PREVIEW_FILE);
    let preview = fs::canonicalize(&expected).ok()?;
    (preview == expected && preview.is_file()).then_some(preview)
}

/// The one path the webview may be allowed to load for `entry`: its verified
/// `preview.png`. `None` when the entry has no preview or the preview fails
/// verification.
pub fn preview_grant(library: &Path, entry: &LibraryEntry) -> Option<PathBuf> {
    entry.preview_path.as_ref()?;
    verified_preview(library, &library.join(&entry.id))
}

fn load_entry(library: &Path, dir: &Path, id: String) -> Result<LibraryEntry, String> {
    let profile_path = dir.join(PROFILE_FILE);
    let profile_text = fs::read_to_string(&profile_path)
        .map_err(|error| format!("cannot read {PROFILE_FILE}: {error}"))?;
    let profile =
        profile::from_json(&profile_text).map_err(|error| format!("invalid profile: {error}"))?;
    let entry_text = fs::read_to_string(dir.join(ENTRY_FILE))
        .map_err(|error| format!("cannot read {ENTRY_FILE}: {error}"))?;
    let file: EntryFile = serde_json::from_str(&entry_text)
        .map_err(|error| format!("invalid {ENTRY_FILE}: {error}"))?;
    let major = |version: &str| version.split('.').next().unwrap_or_default().to_owned();
    if major(&file.schema_version) != major(ENTRY_SCHEMA_VERSION) {
        return Err(format!(
            "unsupported {ENTRY_FILE} schema version {}",
            file.schema_version
        ));
    }
    if file.profile_digest != profile.digest {
        return Err("entry does not match its profile".to_owned());
    }
    let preview = dir.join(PREVIEW_FILE);
    let has_preview = verified_preview(library, dir).is_some();
    let (quality_passed, quality_overridden, quick_and_dirty) = profile
        .quality
        .as_ref()
        .map_or((true, false, false), |quality| {
            (quality.passed, quality.overridden, quality.quick_and_dirty)
        });
    Ok(LibraryEntry {
        id,
        label: file.label,
        notes: file.notes,
        tags: file.tags,
        profile_path: profile_path.to_string_lossy().into_owned(),
        preview_path: has_preview.then(|| preview.to_string_lossy().into_owned()),
        digest: profile.digest.clone(),
        camera_make: profile.camera.make.clone(),
        camera_model: profile.camera.model.clone(),
        lens: file.capture.lens,
        iso: file.capture.iso,
        captured_at: file.capture.date_time,
        gps: file.capture.gps,
        chart_revision: chart_revision(&profile),
        decoder: profile.camera.decoder.clone(),
        decoder_version: profile.camera.decoder_version.clone(),
        quality_passed,
        quality_overridden,
        quick_and_dirty,
        mean_delta_e: profile.validation.mean_delta_e,
        max_delta_e: profile.validation.max_delta_e,
        patch_count: profile.validation.patch_count,
    })
}

fn chart_revision(profile: &Profile) -> String {
    match serde_json::to_value(profile.chart_revision) {
        Ok(serde_json::Value::String(name)) => name,
        _ => format!("{:?}", profile.chart_revision),
    }
}

/// Add a calibration to `library` and return the new entry.
///
/// The reference must hash to the profile's `reference_digest`; any other file
/// is refused before the library is touched. The preview is the corrected
/// reference. If it cannot be decoded or rendered the entry is still saved,
/// without a preview. Capture facts come from the reference's EXIF, GPS only
/// when `include_gps` is set. An existing entry is never overwritten: a taken
/// name gets a numeric suffix.
pub fn save_to_library(
    library: &Path,
    profile_path: &Path,
    reference_path: &Path,
    label: &str,
    notes: &str,
    tags: &[String],
    include_gps: bool,
) -> Result<LibraryEntry, BackendError> {
    let label = label.trim();
    if label.is_empty() {
        return Err(BackendError::Message(
            "the library entry needs a label".to_owned(),
        ));
    }
    let profile_text = fs::read_to_string(profile_path)?;
    let profile = profile::from_json(&profile_text)
        .map_err(|error| BackendError::Message(error.to_string()))?;
    let reference_digest = sha256_file(reference_path)?;
    if !reference_digest.eq_ignore_ascii_case(&profile.reference_digest) {
        return Err(BackendError::Message(format!(
            "the reference does not belong to this profile: its digest is {reference_digest}, the profile was derived from {}",
            profile.reference_digest
        )));
    }

    let capture = colorbalance_raw::read_capture_info(reference_path);
    let file = EntryFile {
        schema_version: ENTRY_SCHEMA_VERSION.to_owned(),
        label: label.to_owned(),
        notes: notes.to_owned(),
        tags: clean_tags(tags),
        profile_digest: profile.digest.clone(),
        capture: CaptureFile {
            make: capture.make,
            model: capture.model,
            lens: capture.lens,
            iso: capture.iso,
            date_time: capture.date_time,
            gps: capture.gps.filter(|_| include_gps).map(|gps| LibraryGps {
                latitude: gps.latitude,
                longitude: gps.longitude,
                altitude: gps.altitude,
            }),
        },
    };
    let entry_json = serde_json::to_string_pretty(&file)
        .map_err(|error| BackendError::Message(error.to_string()))?;

    fs::create_dir_all(library)?;
    // Dropped on any early return, which removes only this call's directory.
    let staging = tempfile::Builder::new()
        .prefix(".tmp-")
        .tempdir_in(library)?;
    write_synced(&staging.path().join(PROFILE_FILE), profile_text.as_bytes())?;
    write_synced(&staging.path().join(ENTRY_FILE), entry_json.as_bytes())?;
    if let Ok(png) = corrected_preview_png(&profile, reference_path, PREVIEW_MAX_DIM) {
        write_synced(&staging.path().join(PREVIEW_FILE), &png)?;
    }

    let stem = format!(
        "{}-{}",
        slug(label),
        profile.digest.chars().take(8).collect::<String>()
    );
    let staged = staging.keep();
    let target = match publish(&staged, library, &stem) {
        Ok(target) => target,
        Err(error) => {
            let _ = fs::remove_dir_all(&staged);
            return Err(error);
        }
    };
    let id = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    load_entry(library, &target, id).map_err(BackendError::Message)
}

/// Lowercase hex SHA-256 of a file, read in chunks.
fn sha256_file(path: &Path) -> Result<String, BackendError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Trim tags, drop empty ones and repeats, keep the order given.
fn clean_tags(tags: &[String]) -> Vec<String> {
    let mut cleaned: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if !tag.is_empty() && !cleaned.iter().any(|seen| seen == tag) {
            cleaned.push(tag.to_owned());
        }
    }
    cleaned
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), BackendError> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Rename `staged` to the first free name `stem`, `stem-2`, `stem-3`, ...
/// A name that exists is skipped, so no entry is ever replaced.
fn publish(staged: &Path, library: &Path, stem: &str) -> Result<PathBuf, BackendError> {
    for attempt in 1..=MAX_NAME_ATTEMPTS {
        let name = if attempt == 1 {
            stem.to_owned()
        } else {
            format!("{stem}-{attempt}")
        };
        let target = library.join(name);
        if target.exists() {
            continue;
        }
        match fs::rename(staged, &target) {
            Ok(()) => return Ok(target),
            // Another writer took the name between the check and the rename.
            Err(_) if target.exists() => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(BackendError::Message(
        "no free name for this library entry".to_owned(),
    ))
}

/// Lowercase ASCII letters and digits of `label` joined by single dashes, at
/// most [`SLUG_MAX_CHARS`] long; `entry` when nothing is left.
fn slug(label: &str) -> String {
    let mut slug = String::new();
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug: String = slug.chars().take(SLUG_MAX_CHARS).collect();
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "entry".to_owned()
    } else {
        slug.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_short_ascii_and_never_empty() {
        assert_eq!(slug("Café / Studio #1"), "caf-studio-1");
        assert_eq!(slug("  --  "), "entry");
        assert_eq!(slug("日本"), "entry");
        let long = slug(&format!("{} tail", "a".repeat(SLUG_MAX_CHARS - 1)));
        assert_eq!(long, "a".repeat(SLUG_MAX_CHARS - 1));
    }

    #[test]
    fn tags_are_trimmed_and_deduplicated_in_order() {
        let tags = ["  street ", "", "indoor", "street", "indoor "].map(String::from);
        assert_eq!(clean_tags(&tags), ["street", "indoor"]);
    }
}
