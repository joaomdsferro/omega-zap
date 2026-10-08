//! Files dropped on the window from the desktop. The system hands their paths
//! to this side, not to the page, so the page can only read a file that was
//! really dropped, and never any other path it names.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{DragDropEvent, Manager, State, WindowEvent};

use crate::account::{MAX_SEND_BYTES, lock};

/// The paths of the last drop that have not been read yet.
#[derive(Default)]
pub struct Dropped(Mutex<HashSet<PathBuf>>);

/// Remembers the files of a drop, in place of the last drop's.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
        let dropped = window.state::<Dropped>();
        let mut known = lock(&dropped.0);
        known.clear();
        known.extend(paths.iter().cloned());
    }
}

/// The contents of a dropped file, once.
#[tauri::command]
pub async fn read_dropped_file(dropped: State<'_, Dropped>, path: String) -> Result<tauri::ipc::Response, String> {
    let path = PathBuf::from(path);
    if !lock(&dropped.0).remove(&path) {
        return Err("That file was not dropped here.".into());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let meta = std::fs::metadata(&path).map_err(|e| format!("{name} could not be read: {e}"))?;
        if meta.is_dir() {
            return Err(format!("{name} is a folder; only files can be sent."));
        }
        if meta.len() > MAX_SEND_BYTES as u64 {
            return Err(format!("{name} is larger than {} MB.", MAX_SEND_BYTES / 1024 / 1024));
        }
        std::fs::read(&path).map_err(|e| format!("{name} could not be read: {e}"))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}
