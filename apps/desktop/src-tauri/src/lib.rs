use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};
use tauri::{App, Emitter, Manager, WindowEvent};
use tauri_plugin_dialog::{DialogExt, FilePath};

pub struct AppState {
    pub cancellation: colorbalance_core::CancelFlag,
    /// Most recent decoded reference, shared by load, inspect and derive.
    pub reference: std::sync::Arc<reference_cache::ReferenceCache>,
    pub previews: std::sync::Arc<preview_files::PreviewFiles>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            cancellation: std::sync::Arc::new(AtomicBool::new(false)),
            reference: std::sync::Arc::default(),
            previews: std::sync::Arc::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeFileDrop {
    pub paths: Vec<String>,
    pub position: DropPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DropPosition {
    pub x: f64,
    pub y: f64,
}

pub fn file_path_to_string(path: FilePath) -> Option<String> {
    path.into_path()
        .ok()
        .map(|value| value.to_string_lossy().into_owned())
}

pub fn dropped_paths(paths: &[PathBuf], x: f64, y: f64) -> NativeFileDrop {
    NativeFileDrop {
        paths: paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        position: DropPosition { x, y },
    }
}

pub fn run_app() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            let previews = &app.state::<AppState>().previews;
            let directory = previews.directory()?;
            app.asset_protocol_scope()
                .allow_directory(&directory, false)?;
            register_native_drag_drop(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::ipc::load_reference,
            crate::ipc::detect_chart,
            crate::ipc::inspect_reference,
            crate::ipc::derive_profile,
            crate::ipc::correct_image,
            crate::ipc::release_previews,
            crate::ipc::apply_batch,
            crate::ipc::cancel_batch,
            crate::ipc::export_profile,
            choose_image,
            choose_directory,
            choose_save_path,
        ])
        .build(tauri::generate_context!())
        .expect("error while building ColorBalance desktop application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>().previews.cleanup();
            }
        });
}

fn register_native_drag_drop(app: &mut App) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let events_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::DragDrop(tauri::DragDropEvent::Enter { .. })
        | WindowEvent::DragDrop(tauri::DragDropEvent::Over { .. }) => {
            let _ = events_window.emit("native-file-drop-hover", true);
        }
        WindowEvent::DragDrop(tauri::DragDropEvent::Leave) => {
            let _ = events_window.emit("native-file-drop-hover", false);
        }
        WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, position }) => {
            let _ = events_window.emit("native-file-drop-hover", false);
            let _ = events_window.emit(
                "native-file-drop",
                dropped_paths(paths, position.x, position.y),
            );
        }
        _ => {}
    });
}

// Dialog commands are `async` on purpose: Tauri runs sync commands on the main
// thread, and `blocking_*` dialog calls there stall the window event loop.
#[tauri::command]
async fn choose_image(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("Supported images", &["dng", "jpg", "jpeg", "png"])
        .blocking_pick_file()
        .and_then(file_path_to_string)
}

#[tauri::command]
async fn choose_directory(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(file_path_to_string)
}

#[tauri::command]
async fn choose_save_path(
    app: tauri::AppHandle,
    default_path: String,
    extension: String,
) -> Option<String> {
    app.dialog()
        .file()
        .set_file_name(&default_path)
        .add_filter(extension.to_uppercase(), &[extension.as_str()])
        .blocking_save_file()
        .and_then(file_path_to_string)
}

pub mod commands;
pub mod ipc;
pub mod preview_files;
pub mod reference_cache;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn dropped_paths_preserve_all_files_and_coordinates() {
        let event = dropped_paths(
            &[
                PathBuf::from("C:/images/reference.dng"),
                PathBuf::from("C:/images/second.jpg"),
            ],
            128.5,
            64.25,
        );
        assert_eq!(event.paths.len(), 2);
        assert!(event.paths[0].ends_with("reference.dng"));
        assert_eq!(event.position, DropPosition { x: 128.5, y: 64.25 });
    }
}
