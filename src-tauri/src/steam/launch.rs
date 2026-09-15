//! Hands `steam://` URIs to the Steam client — used both to launch games
//! (never by executing their binaries directly) and to open a game's store
//! page.

#[cfg(not(target_os = "windows"))]
use std::process::Command;

use tauri::AppHandle;

use super::discovery::LaunchKind;
use crate::platform;

pub fn launch(app: &AppHandle, app_id: u32, launch_kind: LaunchKind) -> Result<(), String> {
    open_steam_uri(app, &format!("steam://rungameid/{app_id}"), launch_kind)
}

pub fn open_store_page(
    app: &AppHandle,
    app_id: u32,
    launch_kind: LaunchKind,
) -> Result<(), String> {
    open_steam_uri(app, &format!("steam://store/{app_id}"), launch_kind)
}

#[cfg(target_os = "windows")]
fn open_steam_uri(app: &AppHandle, uri: &str, _launch_kind: LaunchKind) -> Result<(), String> {
    platform::open_url(app, uri)
}

#[cfg(not(target_os = "windows"))]
fn open_steam_uri(_app: &AppHandle, uri: &str, launch_kind: LaunchKind) -> Result<(), String> {
    let result = match launch_kind {
        LaunchKind::Flatpak => Command::new("flatpak")
            .args(["run", "com.valvesoftware.Steam", uri])
            .spawn(),
        LaunchKind::Native => Command::new("steam").arg(uri).spawn(),
        LaunchKind::Windows => {
            return Err("Windows Steam launch is unavailable on this platform.".into())
        }
    };

    match result {
        Ok(_) => Ok(()),
        // Native `steam` binary missing from PATH: fall back to the desktop
        // handler, which Steam registers for the steam:// scheme on install.
        Err(_) if launch_kind == LaunchKind::Native => Command::new("xdg-open")
            .arg(uri)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open Steam: {e}")),
        Err(e) => Err(format!("Could not open Steam: {e}")),
    }
}
