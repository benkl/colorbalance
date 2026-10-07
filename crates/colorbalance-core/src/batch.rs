//! Bounded parallel batch scheduling shared by the CLI and desktop hosts.
//!
//! The core owns input enumeration (recursive), collision-safe output names,
//! the bounded worker pool, and cancellation semantics. Hosts supply the
//! per-file processing closure so decode, transform, and encoding stay in the
//! native layer. At most `workers` full images are decoded in memory at once;
//! the default bound of 2 matches the documented memory budget
//! (`docs/ARCHITECTURE.md`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use serde::{Deserialize, Serialize};

/// File extensions accepted as batch inputs by default.
pub const DEFAULT_EXTENSIONS: &[&str] = &["dng", "raw", "cr3", "nef", "jpg", "jpeg", "png"];

/// Default in-flight image bound. Two full 540 MB buffers is the documented
/// worst case; the bound is configurable through [`BatchOptions`].
pub const DEFAULT_WORKERS: usize = 2;

/// Cancellation flag understood by the worker pool.
pub type CancelFlag = Arc<AtomicBool>;

/// Options for one batch run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchOptions {
    /// Directory created for outputs if missing.
    pub output: PathBuf,
    /// When false, inputs whose output already exists are skipped.
    pub overwrite: bool,
    /// Number of in-flight images. Clamped to at least 1.
    pub workers: usize,
    /// Extensions accepted as inputs, in lowercase.
    pub extensions: Vec<String>,
    /// Output filename extension, without the dot.
    pub output_extension: &'static str,
}

impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            output: PathBuf::new(),
            overwrite: false,
            workers: DEFAULT_WORKERS,
            extensions: DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
            output_extension: "tiff",
        }
    }
}

/// Result of one input file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchFileResult {
    /// Input path as presented to the batch.
    pub input: String,
    /// Final output path, present only when the file succeeded.
    pub output: Option<String>,
    /// Error message, present only when the file failed.
    pub message: Option<String>,
}

/// Cancellation status of the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CancelState {
    /// Every scheduled input finished.
    Completed,
    /// Remaining inputs were left unscheduled by request.
    Cancelled(usize),
}

/// Deterministic per-file batch summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchSummary {
    pub total: usize,
    pub succeeded: Vec<BatchFileResult>,
    pub skipped: Vec<String>,
    pub failed: Vec<BatchFileResult>,
    pub cancel: CancelState,
}

impl BatchSummary {
    /// Number of files that produced a valid output.
    pub fn succeeded_count(&self) -> usize {
        self.succeeded.len()
    }
}

/// Collect input files: a single file, or every image in a directory tree.
///
/// The result is sorted by input path so output naming and summary order are
/// independent of filesystem enumeration order.
pub fn collect_inputs(path: &Path, extensions: &[String]) -> Result<Vec<PathBuf>, String> {
    let files: Vec<PathBuf> = if path.is_file() {
        vec![path.to_path_buf()]
    } else if path.is_dir() {
        walk(path, extensions)?
    } else {
        return Err(format!("input path not found: {}", path.display()));
    };
    Ok(dedup_sorted(files))
}

fn dedup_sorted(mut files: Vec<PathBuf>) -> Vec<PathBuf> {
    files.sort();
    files.dedup();
    files
}

fn walk(dir: &Path, extensions: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("cannot read directory entry: {e}"))?
            .path();
        if path.is_dir() {
            files.extend(walk(&path, extensions)?);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
            .is_some_and(|e| extensions.iter().any(|x| x == e))
        {
            files.push(path);
        }
    }
    Ok(files)
}

/// Progress callback invoked before each file starts.
pub type ProgressFn = Arc<dyn Fn(&Path, usize, usize) + Send + Sync>;
/// Collision-safe output path for `input` inside `output_dir`.
///
/// `seen` tracks outputs already assigned in this run; a second input with the
/// same stem is suffixed `-1`, `-2`, ... so recursive selection never
/// overwrites an earlier file of the run. The returned path is registered in
/// `seen`.
pub fn unique_output_path(
    output_dir: &Path,
    input: &Path,
    extension: &str,
    seen: &mut Vec<String>,
) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .to_string();
    let mut candidate = output_dir.join(format!("{stem}.{extension}"));
    let mut counter = 1usize;
    while seen.contains(&candidate.to_string_lossy().into_owned()) {
        candidate = output_dir.join(format!("{stem}-{counter}.{extension}"));
        counter += 1;
    }
    seen.push(candidate.to_string_lossy().into_owned());
    candidate
}

/// Run `process` over `inputs` with at most `options.workers` in flight.
///
/// Workers claim the next unprocessed input as they become free, so a slow
/// file delays only its own worker.
///
/// Cancellation: when `cancel` is set, no further file is scheduled; already
/// running files finish. The summary is always sorted by input path, so it is
/// deterministic regardless of completion order.
///
/// `process` returns `Ok(Some(output))` for success, `Ok(None)` for a skipped
/// file (existing output without overwrite), or `Err(message)` for failure.
pub fn run_batch<F>(
    inputs: Vec<PathBuf>,
    options: &BatchOptions,
    progress: Option<ProgressFn>,
    cancel: CancelFlag,
    process: F,
) -> Result<BatchSummary, String>
where
    F: Fn(&Path, PathBuf) -> Result<Option<PathBuf>, String> + Send + Sync + 'static,
{
    std::fs::create_dir_all(&options.output).map_err(|e| e.to_string())?;
    let total = inputs.len();
    let workers = options.workers.max(1).min(total.max(1));
    let (tx, rx) = mpsc::channel::<(usize, Result<Option<PathBuf>, String>)>();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let process = Arc::new(process);

    let inputs = Arc::new(inputs);
    // Next unclaimed input. Workers claim indices as they become free, so a
    // slow file delays only its own worker instead of a fixed slice.
    let next = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let tx = tx.clone();
        let inputs = Arc::clone(&inputs);
        let next = Arc::clone(&next);
        let cancel = cancel.clone();
        let seen = Arc::clone(&seen);
        let options = options.clone();
        let progress = progress.clone();
        let process = Arc::clone(&process);
        handles.push(std::thread::spawn(move || {
            loop {
                // Check cancellation before claiming so a cancelled run
                // leaves the remaining inputs unclaimed.
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(input) = inputs.get(index) else {
                    return;
                };
                let output = {
                    let mut lock = seen.lock().expect("output name lock poisoned");
                    unique_output_path(&options.output, input, options.output_extension, &mut lock)
                };
                if let Some(cb) = &progress {
                    cb(input, index, inputs.len());
                }
                let result = process(input, output);
                let _ = tx.send((index, result));
            }
        }));
    }
    drop(tx);

    let mut results: Vec<(usize, Result<Option<PathBuf>, String>)> = Vec::with_capacity(total);
    for message in rx {
        results.push(message);
    }
    for handle in handles {
        handle
            .join()
            .map_err(|_| "batch worker panicked".to_string())?;
    }
    results.sort_by_key(|(index, _)| *index);

    let mut summary = BatchSummary {
        total,
        succeeded: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
        cancel: CancelState::Completed,
    };
    for (index, result) in results {
        let input = inputs[index].to_string_lossy().into_owned();
        match result {
            Ok(Some(output)) => summary.succeeded.push(BatchFileResult {
                input,
                output: Some(output.to_string_lossy().into_owned()),
                message: None,
            }),
            Ok(None) => summary.skipped.push(input),
            Err(message) => summary.failed.push(BatchFileResult {
                input,
                output: None,
                message: Some(message),
            }),
        }
    }
    if cancel.load(Ordering::Relaxed) {
        let done = summary.succeeded.len() + summary.skipped.len() + summary.failed.len();
        summary.cancel = CancelState::Cancelled(total - done);
    }
    summary.succeeded.sort_by(|a, b| a.input.cmp(&b.input));
    summary.skipped.sort();
    summary.failed.sort_by(|a, b| a.input.cmp(&b.input));
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "colorbalance-batch-{name}-{}-{nonce}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn collect_inputs_recurses_and_sorts() {
        let root = temp_dir("collect");
        write(&root.join("z.dng"), b"z");
        write(&root.join("sub").join("a.dng"), b"a");
        write(&root.join("sub").join("b.nef"), b"b");
        write(&root.join("notes.txt"), b"no");

        let extensions = vec!["dng".to_string(), "nef".to_string()];
        let files = collect_inputs(&root, &extensions).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.dng", "b.nef", "z.dng"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_names_get_colliding_suffixes() {
        let out = temp_dir("names");
        let mut seen: Vec<String> = Vec::new();
        let p1 = unique_output_path(&out, Path::new("a/cr-0001.dng"), "tiff", &mut seen);
        let p2 = unique_output_path(&out, Path::new("b/cr-0001.dng"), "tiff", &mut seen);
        assert_eq!(p1.file_name().unwrap(), "cr-0001.tiff");
        assert_eq!(p2.file_name().unwrap(), "cr-0001-1.tiff");
        let _ = fs::remove_dir_all(&out);
    }

    #[test]
    fn summary_is_deterministic_and_sorted() {
        let root = temp_dir("deterministic");
        let out = root.join("out");
        for i in 0..12 {
            write(&root.join(format!("f{i:02}.dng")), &[i as u8]);
        }
        let options = BatchOptions {
            output: out,
            ..Default::default()
        };
        let inputs = collect_inputs(&root, &options.extensions).unwrap();
        let process = |input: &Path, output: PathBuf| -> Result<Option<PathBuf>, String> {
            if input
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("f05"))
            {
                return Err("synthetic corrupt file".to_string());
            }
            fs::write(&output, b"ok").unwrap();
            Ok(Some(output))
        };
        let summary = run_batch(
            inputs,
            &options,
            None,
            Arc::new(AtomicBool::new(false)),
            process,
        )
        .unwrap();
        assert_eq!(summary.total, 12);
        assert_eq!(summary.succeeded.len(), 11);
        assert_eq!(summary.failed.len(), 1);
        assert_eq!(
            summary.failed[0].message.as_deref(),
            Some("synthetic corrupt file")
        );
        assert!(matches!(summary.cancel, CancelState::Completed));
        // Successes are input-sorted regardless of completion order.
        let names: Vec<String> = summary
            .succeeded
            .iter()
            .map(|r| r.output.as_deref().unwrap().to_string())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cancellation_leaves_unscheduled_files_untouched() {
        let root = temp_dir("cancel");
        let out = root.join("out");
        for i in 0..16 {
            write(&root.join(format!("f{i:02}.dng")), &[i as u8]);
        }
        let options = BatchOptions {
            output: out.clone(),
            workers: 4,
            ..Default::default()
        };
        let inputs = collect_inputs(&root, &options.extensions).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let marker = Arc::new(AtomicBool::new(false));
        let marker_in = Arc::clone(&marker);
        let process = move |_input: &Path, output: PathBuf| -> Result<Option<PathBuf>, String> {
            std::thread::sleep(std::time::Duration::from_millis(3));
            marker_in.store(true, Ordering::Relaxed);
            fs::write(&output, b"ok").unwrap();
            Ok(Some(output))
        };
        let cancel_flag = Arc::clone(&cancel);
        let flag_probe = Arc::clone(&marker);
        std::thread::spawn(move || {
            while !flag_probe.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            cancel_flag.store(true, Ordering::Relaxed);
        });
        let summary = run_batch(inputs, &options, None, cancel, process).unwrap();
        assert!(matches!(summary.cancel, CancelState::Cancelled(_)));
        assert!(summary.succeeded.len() < 16);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_slow_file_does_not_hold_back_files_behind_it() {
        // Under a fixed stride, with two workers, f00 would own f02 and f04
        // and they could not start until f00 returned. Here f00 only returns
        // once every other file has finished, so the run completes only if
        // the idle worker takes over the rest.
        let root = temp_dir("steal");
        let out = root.join("out");
        for i in 0..5 {
            write(&root.join(format!("f{i:02}.dng")), &[i as u8]);
        }
        let options = BatchOptions {
            output: out,
            workers: 2,
            ..Default::default()
        };
        let inputs = collect_inputs(&root, &options.extensions).unwrap();
        let others_done = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&others_done);
        let process = move |input: &Path, output: PathBuf| -> Result<Option<PathBuf>, String> {
            let slow = input.file_name().is_some_and(|n| n == "f00.dng");
            if slow {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while counter.load(Ordering::Relaxed) < 4 {
                    if std::time::Instant::now() > deadline {
                        return Err("other files were stuck behind the slow one".to_string());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            } else {
                counter.fetch_add(1, Ordering::Relaxed);
            }
            fs::write(&output, b"ok").unwrap();
            Ok(Some(output))
        };
        let summary = run_batch(
            inputs,
            &options,
            None,
            Arc::new(AtomicBool::new(false)),
            process,
        )
        .unwrap();
        assert_eq!(summary.succeeded.len(), 5, "{:?}", summary.failed);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skip_existing_output_without_overwrite_is_host_policy() {
        // The pool itself is storage-agnostic; the skip policy is exercised
        // through the process closure as hosts use it.
        let root = temp_dir("skip");
        let out = root.join("out");
        fs::create_dir_all(&out).unwrap();
        write(&root.join("a.dng"), b"a");
        write(&out.join("a.tiff"), b"existing");
        let options = BatchOptions {
            output: out.clone(),
            overwrite: false,
            ..Default::default()
        };
        let inputs = collect_inputs(&root, &options.extensions).unwrap();
        let overwrite = options.overwrite;
        let process = move |_input: &Path, output: PathBuf| -> Result<Option<PathBuf>, String> {
            if output.exists() && !overwrite {
                return Ok(None);
            }
            fs::write(&output, b"ok").unwrap();
            Ok(Some(output))
        };
        let summary = run_batch(
            inputs,
            &options,
            None,
            Arc::new(AtomicBool::new(false)),
            process,
        )
        .unwrap();
        assert_eq!(summary.succeeded.len(), 0);
        assert_eq!(
            summary.skipped,
            vec![root.join("a.dng").to_string_lossy().into_owned()]
        );
        let _ = fs::remove_dir_all(&root);
    }
}
