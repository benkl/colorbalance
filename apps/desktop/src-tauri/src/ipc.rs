//! Tauri command wrappers.
//!
//! The work lives in [`crate::commands`] as plain functions. These wrappers run
//! it on the blocking thread pool so the window event loop and webview stay
//! responsive during a multi-second RAW decode, and turn the stage reports into
//! events the UI renders as progress.
//!
//! Events:
//! - `operation-progress` `{operation, stage, step, steps}` at each stage start
//!   of `load`, `inspect`, `derive` and `correct`.
//! - `batch-progress` `{completed, total, file}` from `apply_batch`; `completed`
//!   counts finished files.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, Window};

use crate::chart_check::{self, ChartCheckResponse};
use crate::commands::{
    self, BackendError, BatchResponse, CorrectResponse, DeriveResponse, DetectResponse,
    ExportOptions, InspectResponse, LoadedReference, MismatchPolicy, QuadPayload,
};
use crate::library::{self, LibraryEntry, LibraryListing};
use crate::preflight::{self, PreflightResponse};
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationProgress<'a> {
    operation: &'a str,
    stage: &'a str,
    step: usize,
    steps: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchProgress {
    completed: usize,
    total: usize,
    file: Option<String>,
}

/// Run `work` on the blocking pool and flatten the join error.
async fn background<T, F>(work: F) -> Result<T, BackendError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, BackendError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| BackendError::Message(format!("background task failed: {error}")))?
}

fn emit_stage(window: &Window, operation: &str, stage: &str, step: usize, steps: usize) {
    let _ = window.emit(
        "operation-progress",
        OperationProgress {
            operation,
            stage,
            step,
            steps,
        },
    );
}

#[tauri::command]
pub async fn load_reference(
    window: Window,
    state: State<'_, AppState>,
    path: String,
) -> Result<LoadedReference, BackendError> {
    let cache = state.reference.clone();
    let previews = state.previews.clone();
    background(move || {
        previews.with_operation(|| {
            commands::load_reference_cached(&cache, &previews, path, &|stage, step, steps| {
                emit_stage(&window, "load", stage, step, steps)
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn detect_chart(
    state: State<'_, AppState>,
    path: String,
) -> Result<DetectResponse, BackendError> {
    let cache = state.reference.clone();
    background(move || commands::detect_chart_cached(&cache, path)).await
}

#[tauri::command]
pub async fn check_chart(
    state: State<'_, AppState>,
    profile_path: String,
    image_path: String,
    quad: Option<QuadPayload>,
) -> Result<ChartCheckResponse, BackendError> {
    let cache = state.reference.clone();
    background(move || chart_check::check_chart(&cache, profile_path, image_path, quad)).await
}

#[tauri::command]
pub async fn inspect_reference(
    window: Window,
    state: State<'_, AppState>,
    path: String,
    chart_revision: String,
    quad: Option<QuadPayload>,
) -> Result<InspectResponse, BackendError> {
    let cache = state.reference.clone();
    let previews = state.previews.clone();
    background(move || {
        previews.with_operation(|| {
            commands::inspect_reference_cached(
                &cache,
                &previews,
                path,
                chart_revision,
                quad,
                &|stage, step, steps| emit_stage(&window, "inspect", stage, step, steps),
            )
        })
    })
    .await
}

#[tauri::command]
pub async fn derive_profile(
    window: Window,
    state: State<'_, AppState>,
    path: String,
    chart_revision: String,
    profile_path: String,
    report_path: Option<String>,
    quad: Option<QuadPayload>,
) -> Result<DeriveResponse, BackendError> {
    let cache = state.reference.clone();
    background(move || {
        commands::derive_profile_cached(
            &cache,
            path,
            chart_revision,
            profile_path,
            report_path,
            quad,
            &|stage, step, steps| emit_stage(&window, "derive", stage, step, steps),
        )
    })
    .await
}

#[tauri::command]
pub async fn correct_image(
    window: Window,
    state: State<'_, AppState>,
    profile_path: String,
    input_path: String,
    output_path: Option<String>,
    export_options: ExportOptions,
    allow_mismatch: bool,
) -> Result<CorrectResponse, BackendError> {
    let export = output_path.map(|path| commands::ImageExport {
        path,
        options: export_options,
    });
    let cache = state.reference.clone();
    let previews = state.previews.clone();
    background(move || {
        previews.with_operation(|| {
            commands::correct_image_cached(
                &cache,
                &previews,
                profile_path,
                input_path,
                export,
                MismatchPolicy::from_allow(allow_mismatch),
                &|stage, step, steps| emit_stage(&window, "correct", stage, step, steps),
            )
        })
    })
    .await
}

#[tauri::command]
pub fn release_previews(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<(), BackendError> {
    state
        .previews
        .with_operation(|| state.previews.release(&paths))
}

#[tauri::command]
pub async fn apply_batch(
    window: Window,
    state: State<'_, AppState>,
    profile_path: String,
    input_path: String,
    output_path: String,
    export_options: ExportOptions,
    allow_mismatch: bool,
) -> Result<BatchResponse, BackendError> {
    let cancellation = state.cancellation.clone();
    background(move || {
        let on_progress =
            std::sync::Arc::new(move |completed: usize, total: usize, file: Option<&Path>| {
                let _ = window.emit(
                    "batch-progress",
                    BatchProgress {
                        completed,
                        total,
                        file: file.map(|path| path.display().to_string()),
                    },
                );
            });
        commands::apply_batch(
            profile_path,
            input_path,
            output_path,
            export_options,
            MismatchPolicy::from_allow(allow_mismatch),
            cancellation,
            on_progress,
        )
    })
    .await
}

#[tauri::command]
pub async fn preflight_batch(
    profile_path: String,
    input_path: String,
    library_path: Option<String>,
) -> Result<PreflightResponse, BackendError> {
    background(move || preflight::scan_batch(input_path, library_path, profile_path)).await
}

/// Ask the running batch to stop scheduling files. Files already in flight
/// finish and are written; nothing partial is left behind.
#[tauri::command]
pub fn cancel_batch(state: State<'_, AppState>) {
    state
        .cancellation
        .store(true, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
pub async fn export_profile(
    profile_path: String,
    format: String,
    output_path: String,
    size: Option<usize>,
    camera_name: Option<String>,
) -> Result<String, BackendError> {
    background(move || {
        commands::export_profile(profile_path, format, output_path, size, camera_name)
    })
    .await
}

/// List the calibration library, and let the webview load its preview images.
#[tauri::command]
pub async fn list_library(
    app: AppHandle,
    library_path: String,
) -> Result<LibraryListing, BackendError> {
    let listing = background({
        let library_path = library_path.clone();
        move || library::list_library(Path::new(&library_path))
    })
    .await?;
    let library_root = PathBuf::from(&library_path);
    for entry in &listing.entries {
        if let Some(preview) = library::preview_grant(&library_root, entry) {
            app.asset_protocol_scope()
                .allow_file(&preview)
                .map_err(|error| BackendError::Message(error.to_string()))?;
        }
    }
    Ok(listing)
}

#[tauri::command]
pub async fn save_to_library(
    library_path: String,
    profile_path: String,
    reference_path: String,
    label: String,
    notes: String,
    tags: Vec<String>,
    include_gps: bool,
) -> Result<LibraryEntry, BackendError> {
    background(move || {
        library::save_to_library(
            Path::new(&library_path),
            Path::new(&profile_path),
            Path::new(&reference_path),
            &label,
            &notes,
            &tags,
            include_gps,
        )
    })
    .await
}
