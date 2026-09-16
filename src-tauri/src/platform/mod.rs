//! Small OS integration helpers shared by the Windows, macOS, and Linux builds.
//!
//! Keeping these calls behind one module prevents feature code from reaching
//! for `xdg-open`, `start`, or another platform-specific command directly.

use std::path::Path;

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

pub fn open_path(app: &AppHandle, path: &Path) -> Result<(), String> {
    let path = path.to_string_lossy().into_owned();
    app.opener()
        .open_path(&path, None::<&str>)
        .map_err(|e| format!("Could not open path: {e}"))
}

pub fn open_url(app: &AppHandle, url: &str) -> Result<(), String> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("Could not open URL: {e}"))
}
