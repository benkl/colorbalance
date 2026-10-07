//! One-entry cache for the decoded reference image.
//!
//! The light-table calls `load_reference`, then `inspect_reference` on every
//! chart-corner change, then `derive_profile`, all on the same file. Each call
//! used to re-decode a multi-megapixel RAW. This cache keeps the most recent
//! decode and its on-disk preview path so those calls share one decode.
//!
//! Rules:
//! - An entry is valid only for the same canonical path, byte length and
//!   modification time it was decoded from. Anything else is a miss and drops
//!   the old entry before the new decode starts, so two images never coexist.
//! - The lock is held only to look up or store, never during a decode. The file
//!   is stat-ed before and after the decode; if it changed in between, the
//!   result is returned to the caller but not cached.
//! - Failures are never cached. Entries over the byte limit are never cached.
//! - The image is shared as an immutable [`Arc`]; hits do not copy pixels.
//! - Preview PNG bytes live in the session directory, outside this memory limit.
//! - A released preview path is a miss even when the decoded image still hits.
//!
//! The cache lives in `AppState`, not in a global, and plain `commands` calls
//! pass a [`ReferenceCache::disabled`] one.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use colorbalance_core::decode::DecodedImage;

use crate::commands::BackendError;

/// Default budget for one cached reference's pixels and clip flags.
/// One entry only. 1 GiB holds a 45-megapixel decode (~585 MB with flags); a
/// smaller limit would silently exclude the large RAW frames this exists for.
pub const DEFAULT_LIMIT_BYTES: usize = 1024 * 1024 * 1024;

/// What identifies the bytes a decode came from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileKey {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
}

impl FileKey {
    /// `None` when the file cannot be fingerprinted (missing, not a regular
    /// file, or no modification time); such files are decoded uncached.
    fn of(path: &Path) -> Option<Self> {
        let path = fs::canonicalize(path).ok()?;
        let metadata = fs::metadata(&path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        Some(Self {
            path,
            len: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    }
}

struct Entry {
    key: FileKey,
    image: Arc<DecodedImage>,
    preview: Option<String>,
}

fn image_bytes(image: &DecodedImage) -> usize {
    std::mem::size_of_val(image.rgb.as_slice()) + image.clipped.len()
}

/// A decoded image and whether it came from the cache.
pub struct CachedImage {
    pub image: Arc<DecodedImage>,
    pub hit: bool,
}

pub struct ReferenceCache {
    limit: usize,
    slot: Mutex<Option<Entry>>,
}

impl Default for ReferenceCache {
    fn default() -> Self {
        Self::with_limit(DEFAULT_LIMIT_BYTES)
    }
}

impl ReferenceCache {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            slot: Mutex::new(None),
        }
    }
    /// Compare the canonical file identity, size and modification time to the cached decode.
    pub fn matches(&self, path: &Path) -> bool {
        let key = FileKey::of(path);
        self.slot()
            .as_ref()
            .is_some_and(|entry| Some(&entry.key) == key.as_ref())
    }

    /// A cache that never stores anything: every lookup decodes.
    pub fn disabled() -> Self {
        Self::with_limit(0)
    }

    fn slot(&self) -> MutexGuard<'_, Option<Entry>> {
        // The slot is replaced wholesale, so a panic elsewhere cannot leave it
        // half-written.
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Return the cached decode of `path`, or run `decode` and cache its result.
    ///
    /// `before_decode` runs only on a miss, just before the decode starts, so
    /// callers can report the stage that is about to take the time.
    pub fn get_or_decode(
        &self,
        path: &Path,
        before_decode: impl FnOnce(),
        decode: impl FnOnce(&Path) -> Result<DecodedImage, BackendError>,
    ) -> Result<CachedImage, BackendError> {
        let before = if self.limit == 0 {
            None
        } else {
            FileKey::of(path)
        };
        let Some(before) = before else {
            before_decode();
            return Ok(CachedImage {
                image: Arc::new(decode(path)?),
                hit: false,
            });
        };

        {
            let mut slot = self.slot();
            if let Some(entry) = slot.as_ref().filter(|entry| entry.key == before) {
                return Ok(CachedImage {
                    image: Arc::clone(&entry.image),
                    hit: true,
                });
            }
            *slot = None;
        }

        before_decode();
        let image = Arc::new(decode(path)?);
        if image_bytes(&image) <= self.limit && FileKey::of(path).as_ref() == Some(&before) {
            *self.slot() = Some(Entry {
                key: before,
                image: Arc::clone(&image),
                preview: None,
            });
        }
        Ok(CachedImage { image, hit: false })
    }

    /// Return the preview for `image` and whether it was already cached,
    /// rendering it with `render` on a miss.
    ///
    /// The preview is stored only against the entry that still holds this exact
    /// image, so it can never be attached to a different or replaced file.
    pub fn preview(
        &self,
        image: &Arc<DecodedImage>,
        render: impl FnOnce(&DecodedImage) -> Result<String, BackendError>,
    ) -> Result<(String, bool), BackendError> {
        if let Some(entry) = self.slot().as_ref() {
            if Arc::ptr_eq(&entry.image, image) {
                if let Some(preview) = &entry.preview {
                    if Path::new(preview).is_file() {
                        return Ok((preview.clone(), true));
                    }
                }
            }
        }
        let preview = render(image.as_ref())?;
        if let Some(entry) = self.slot().as_mut() {
            if Arc::ptr_eq(&entry.image, image) && image_bytes(image) <= self.limit {
                entry.preview = Some(preview.clone());
            }
        }
        Ok((preview, false))
    }
}
