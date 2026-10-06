use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};
use tauri::{App, Emitter, Manager, WindowEvent};
use tauri_plugin_dialog::{DialogExt, FilePath};

pub struct AppState {
    pub cancellation: colorbalance_core::CancelFlag,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            cancellation: std::sync::Arc::new(AtomicBool::new(false)),
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
            register_native_drag_drop(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::commands::load_reference,
            crate::commands::inspect_reference,
            crate::commands::derive_profile,
            crate::commands::apply_batch,
            crate::commands::cancel_batch,
            crate::commands::export_profile,
            choose_image,
            choose_directory,
            choose_save_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ColorBalance desktop application");
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

#[tauri::command]
fn choose_image(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("Supported images", &["dng", "jpg", "jpeg", "png"])
        .blocking_pick_file()
        .and_then(file_path_to_string)
}

#[tauri::command]
fn choose_directory(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(file_path_to_string)
}

#[tauri::command]
fn choose_save_path(
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
