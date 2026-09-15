//! Hands an EA App URI to the installed client.

use tauri::AppHandle;

#[cfg(windows)]
use crate::platform;

#[cfg(windows)]
pub fn launch(app: &AppHandle, launch_id: &str) -> Result<(), String> {
    let encoded_id = encode_query_value(launch_id);
    platform::open_url(
        app,
        &format!("origin2://game/launch/?offerIds={encoded_id}"),
    )
}

#[cfg(windows)]
fn encode_query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(not(windows))]
pub fn launch(_app: &AppHandle, _launch_id: &str) -> Result<(), String> {
    Err("EA App games can only be launched on Windows.".to_string())
}
