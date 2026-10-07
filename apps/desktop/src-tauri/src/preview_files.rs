//! Per-process PNG files served only through Tauri's asset protocol.
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use crate::commands::BackendError;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Files {
    directory: Option<tempfile::TempDir>,
    closed: bool,
    published: HashSet<PathBuf>,
}

#[derive(Default)]
pub struct PreviewFiles {
    files: Mutex<Files>,
    operation: Mutex<()>,
}

impl PreviewFiles {
    /// Keep reference invalidation, rendering and publication ordered across IPC calls.
    pub fn with_operation<T>(&self, work: impl FnOnce() -> T) -> T {
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        work()
    }
    /// Create a private directory. The asset scope is granted for this exact directory.
    pub fn directory(&self) -> Result<PathBuf, BackendError> {
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(directory) = &files.directory {
            return Ok(directory.path().to_path_buf());
        }
        if files.closed {
            return Err(BackendError::Message(
                "preview session has ended".to_owned(),
            ));
        }
        let directory = tempfile::Builder::new()
            .prefix("colorbalance-preview-")
            .tempdir()?;
        let path = directory.path().to_path_buf();
        files.directory = Some(directory);
        Ok(path)
    }

    fn publish(&self, kind: &str, png: &[u8]) -> Result<PathBuf, BackendError> {
        self.publish_with_id(kind, png, NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    fn publish_with_id(&self, kind: &str, png: &[u8], id: u64) -> Result<PathBuf, BackendError> {
        let directory = self.directory()?;
        let path = directory.join(format!("{kind}-{id}.png"));
        let partial = directory.join(format!(".{kind}-{id}.partial"));
        let mut created = false;
        let result = (|| {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&partial)?;
            created = true;
            file.write_all(png)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&partial, &path)
        })();
        if let Err(error) = result {
            if created {
                let _ = fs::remove_file(&partial);
            }
            return Err(error.into());
        }
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        if files.closed {
            let _ = fs::remove_file(&path);
            return Err(BackendError::Message(
                "preview session has ended".to_owned(),
            ));
        }
        files.published.insert(path.clone());
        Ok(path)
    }

    pub fn replace_reference(&self, png: &[u8]) -> Result<String, BackendError> {
        Ok(self
            .publish("reference", png)?
            .to_string_lossy()
            .into_owned())
    }

    /// Publish a corrected preview only for a reference PNG from this session.
    pub fn replace_comparison(
        &self,
        before: &Path,
        after: &[u8],
    ) -> Result<(String, String), BackendError> {
        self.replace_comparison_with_id(before, after, NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    fn replace_comparison_with_id(
        &self,
        before: &Path,
        after: &[u8],
        id: u64,
    ) -> Result<(String, String), BackendError> {
        if !self
            .files
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .published
            .contains(before)
        {
            return Err(BackendError::Message(
                "before preview is not in this session".to_owned(),
            ));
        }
        let after_path = self.publish_with_id("corrected", after, id)?;
        Ok((
            before.to_string_lossy().into_owned(),
            after_path.to_string_lossy().into_owned(),
        ))
    }

    /// Only files published by this session may be retired by the frontend.
    pub fn release(&self, paths: &[String]) -> Result<(), BackendError> {
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        let mut failure = None;
        for path in paths {
            let path = Path::new(path);
            if files.published.contains(path) {
                match fs::remove_file(path) {
                    Ok(()) => {
                        files.published.remove(path);
                    }
                    Err(error) => {
                        failure = Some(error);
                    }
                }
            }
        }
        failure.map_or(Ok(()), |error| Err(error.into()))
    }

    /// Unreleased files remain owned by the private directory until app exit.
    pub fn cleanup(&self) {
        let _operation = self
            .operation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        files.closed = true;
        files.published.clear();
        files.directory.take();
    }
}

impl Drop for PreviewFiles {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_and_exit_remove_only_session_files() {
        let previews = PreviewFiles::default();
        let directory = previews.directory().unwrap();
        let outside = directory.parent().unwrap().join(format!(
            "outside-{}",
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&outside, b"not a preview").unwrap();
        let first = previews.replace_reference(b"first").unwrap();
        let second = previews.replace_reference(b"second").unwrap();
        assert!(
            Path::new(&first).exists(),
            "old URL stays valid until the UI releases it"
        );
        assert!(Path::new(&second).exists());
        let (before, after) = previews
            .replace_comparison(Path::new(&second), b"after")
            .unwrap();
        assert_eq!(before, second);
        previews.release(&[first.clone(), after.clone()]).unwrap();
        assert!(!Path::new(&first).exists());
        assert!(!Path::new(&after).exists());
        assert!(Path::new(&second).exists());
        previews.cleanup();
        assert!(!Path::new(&second).exists());
        assert!(!directory.exists());
        assert_eq!(fs::read(&outside).unwrap(), b"not a preview");
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn failed_publish_leaves_no_partial_png_and_preserves_previous_preview() {
        let previews = PreviewFiles::default();
        let previous = previews.replace_reference(b"valid").unwrap();
        // Occupy this test-only ID, independent of concurrent tests' counter updates.
        let id = u64::MAX;
        let blocker = previews
            .directory()
            .unwrap()
            .join(format!(".reference-{id}.partial"));
        fs::write(&blocker, b"blocker").unwrap();
        assert!(previews.publish_with_id("reference", b"bad", id).is_err());
        assert_eq!(fs::read(&previous).unwrap(), b"valid");
        assert_eq!(fs::read(&blocker).unwrap(), b"blocker");

        previews.cleanup();
    }
    #[test]
    fn failed_corrected_write_keeps_existing_comparison_and_reference() {
        let previews = PreviewFiles::default();
        let reference = previews.replace_reference(b"before").unwrap();
        let (_, previous_after) = previews
            .replace_comparison(Path::new(&reference), b"after")
            .unwrap();
        let id = u64::MAX;
        let blocker = previews
            .directory()
            .unwrap()
            .join(format!(".corrected-{id}.partial"));
        fs::write(&blocker, b"blocker").unwrap();
        assert!(previews
            .replace_comparison_with_id(Path::new(&reference), b"replacement", id)
            .is_err());
        assert_eq!(fs::read(&reference).unwrap(), b"before");
        assert_eq!(fs::read(&previous_after).unwrap(), b"after");
        assert!(!previews
            .directory()
            .unwrap()
            .join(format!("corrected-{id}.png"))
            .exists());
        assert_eq!(fs::read(&blocker).unwrap(), b"blocker");
    }

    #[test]
    fn release_refuses_unpublished_files_and_comparison_refuses_unowned_before() {
        let previews = PreviewFiles::default();
        let directory = previews.directory().unwrap();
        let outsider = directory.join("not-published.png");
        fs::write(&outsider, b"untouched").unwrap();
        previews
            .release(&[outsider.to_string_lossy().into_owned()])
            .unwrap();
        assert_eq!(fs::read(&outsider).unwrap(), b"untouched");
        assert!(previews
            .replace_comparison(&outsider, b"corrected")
            .is_err());
    }
}
