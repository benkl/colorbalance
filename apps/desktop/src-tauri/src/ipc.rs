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

use std::path::Path;

use serde::Serialize;
use tauri::{Emitter, State, Window};

use crate::commands::{
    self, BackendError, BatchResponse, CorrectResponse, DeriveResponse, InspectResponse,
    LoadedReference, QuadPayload,
};
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
    background(move || {
        commands::load_reference_cached(&cache, path, &|stage, step, steps| {
            emit_stage(&window, "load", stage, step, steps)
        })
    })
    .await
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
    background(move || {
        commands::inspect_reference_cached(
            &cache,
            path,
            chart_revision,
            quad,
            &|stage, step, steps| emit_stage(&window, "inspect", stage, step, steps),
        )
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
    profile_path: String,
    input_path: String,
    output_path: Option<String>,
    overwrite: bool,
) -> Result<CorrectResponse, BackendError> {
    background(move || {
        commands::correct_image(
            profile_path,
            input_path,
            output_path,
            overwrite,
            &|stage, step, steps| emit_stage(&window, "correct", stage, step, steps),
        )
    })
    .await
}

#[tauri::command]
pub async fn apply_batch(
    window: Window,
    state: State<'_, AppState>,
    profile_path: String,
    input_path: String,
    output_path: String,
    overwrite: bool,
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
            overwrite,
            cancellation,
            on_progress,
        )
    })
    .await
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
) -> Result<String, BackendError> {
    background(move || commands::export_profile(profile_path, format, output_path, size)).await
}
