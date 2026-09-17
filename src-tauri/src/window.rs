//! Small platform adapters for changing a launched game's window state.
//!
//! This is intentionally best-effort: launchers and some games create their
//! real window asynchronously, and Linux desktops differ in how much window
//! control they expose to another process.

use std::time::Duration;

/// Keeps trying briefly because a process can exist before its first window
/// has been created.
pub fn spawn_maximizer(pid: u32) {
    std::thread::spawn(move || {
        for _ in 0..120 {
            if maximize_process_windows(&[pid]) {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

#[cfg(target_os = "linux")]
pub fn maximize_process_windows(pids: &[u32]) -> bool {
    use std::process::Command;

    let Ok(output) = Command::new("wmctrl").args(["-lp"]).output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }

    let mut maximized = false;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(window_id) = fields.next() else {
            continue;
        };
        let _desktop = fields.next();
        let Some(pid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        if !pids.contains(&pid) {
            continue;
        }
        let Ok(result) = Command::new("wmctrl")
            .args([
                "-i",
                "-r",
                window_id,
                "-b",
                "add,maximized_vert,maximized_horz",
            ])
            .status()
        else {
            continue;
        };
        maximized |= result.success();
    }
    maximized
}

#[cfg(target_os = "windows")]
pub fn maximize_process_windows(pids: &[u32]) -> bool {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, ShowWindow, SW_MAXIMIZE,
    };

    struct EnumState {
        pids: Vec<u32>,
        maximized: bool,
    }

    unsafe extern "system" fn visit_window(hwnd: HWND, data: LPARAM) -> BOOL {
        let state = &mut *(data as *mut EnumState);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != 0 && state.pids.contains(&pid) && IsWindowVisible(hwnd) != 0 {
            ShowWindow(hwnd, SW_MAXIMIZE);
            state.maximized = true;
        }
        1
    }

    let mut state = EnumState {
        pids: pids.to_vec(),
        maximized: false,
    };
    unsafe {
        EnumWindows(Some(visit_window), &mut state as *mut EnumState as LPARAM);
    }
    state.maximized
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn maximize_process_windows(_pids: &[u32]) -> bool {
    false
}
