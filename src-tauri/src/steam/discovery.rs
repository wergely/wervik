//! Locates the local Steam installation and its library folders.

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
#[cfg(windows)]
use winreg::RegKey;

use super::vdf;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamInstall {
    pub root: PathBuf,
    /// How Wervik should hand off game launches to this Steam install.
    pub launch_kind: LaunchKind,
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchKind {
    Native,
    Flatpak,
    Windows,
}

#[cfg(not(windows))]
fn candidate_roots() -> Vec<(PathBuf, LaunchKind)> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };

    vec![
        (home.join(".steam/steam"), LaunchKind::Native),
        (home.join(".local/share/Steam"), LaunchKind::Native),
        (home.join(".steam/root"), LaunchKind::Native),
        (
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
            LaunchKind::Flatpak,
        ),
        (
            home.join(".var/app/com.valvesoftware.Steam/.steam/steam"),
            LaunchKind::Flatpak,
        ),
        (
            home.join("snap/steam/common/.local/share/Steam"),
            LaunchKind::Native,
        ),
    ]
}

#[cfg(windows)]
fn candidate_roots() -> Vec<(PathBuf, LaunchKind)> {
    let mut candidates = Vec::new();

    // This override supports portable installs and makes discovery easy to
    // exercise in automated tests. The registry is the normal path below.
    if let Some(path) = std::env::var_os("STEAM_PATH") {
        candidates.push((PathBuf::from(path), LaunchKind::Windows));
    }

    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for subkey in [
            "Software\\Valve\\Steam",
            "Software\\Wow6432Node\\Valve\\Steam",
        ] {
            let Ok(key) = RegKey::predef(hive).open_subkey(subkey) else {
                continue;
            };
            for value_name in ["SteamPath", "InstallPath"] {
                if let Ok(path) = key.get_value::<String, _>(value_name) {
                    candidates.push((PathBuf::from(path), LaunchKind::Windows));
                }
            }
        }
    }

    for variable in ["ProgramFiles(x86)", "ProgramW6432", "ProgramFiles"] {
        if let Some(root) = std::env::var_os(variable) {
            candidates.push((PathBuf::from(root).join("Steam"), LaunchKind::Windows));
        }
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        candidates.push((
            PathBuf::from(local_app_data).join("Programs/Steam"),
            LaunchKind::Windows,
        ));
    }

    candidates
}

/// Locate Steam using the platform's normal install hints. The library
/// parser below is platform-neutral once this root is found.
pub fn find_steam_install() -> Option<SteamInstall> {
    let candidates = candidate_roots();

    let mut seen_canonical = std::collections::HashSet::new();

    for (path, kind) in candidates {
        if !looks_like_steam_root(&path) {
            continue;
        }
        let canonical = fs::canonicalize(&path).unwrap_or(path.clone());
        if !seen_canonical.insert(canonical.clone()) {
            continue;
        }
        return Some(SteamInstall {
            root: canonical,
            launch_kind: kind,
        });
    }

    None
}

fn looks_like_steam_root(path: &Path) -> bool {
    path.join("steamapps").is_dir() || path.join("config").is_dir()
}

/// A discovered Steam library folder (a directory containing a `steamapps`
/// subfolder where games can be installed).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFolder {
    pub path: PathBuf,
}

/// Read `steamapps/libraryfolders.vdf` under the Steam root to discover all
/// library folders (the root's own steamapps dir is always included too).
pub fn discover_library_folders(steam_root: &Path) -> Vec<LibraryFolder> {
    let mut folders = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let root_steamapps = steam_root.join("steamapps");
    if root_steamapps.is_dir() {
        if seen.insert(root_steamapps.clone()) {
            folders.push(LibraryFolder {
                path: steam_root.to_path_buf(),
            });
        }
    }

    let vdf_path = root_steamapps.join("libraryfolders.vdf");
    if let Ok(contents) = fs::read_to_string(&vdf_path) {
        if let Ok(root) = vdf::parse(&contents) {
            if let Some(list) = root.get("libraryfolders") {
                for (_, entry) in list.entries() {
                    if let Some(path_str) = entry.get_str("path") {
                        let path = PathBuf::from(path_str);
                        let steamapps = path.join("steamapps");
                        if steamapps.is_dir() && seen.insert(steamapps) {
                            folders.push(LibraryFolder { path });
                        }
                    }
                }
            }
        }
    }

    folders
}

/// Find the Steam user data directory most likely to belong to the active
/// (or most recently used) local user, for reading playtime/last-played.
pub fn find_active_userdata_dir(steam_root: &Path) -> Option<PathBuf> {
    let loginusers_path = steam_root.join("config/loginusers.vdf");
    let userdata_root = steam_root.join("userdata");
    if !userdata_root.is_dir() {
        return None;
    }

    if let Ok(contents) = fs::read_to_string(&loginusers_path) {
        if let Ok(root) = vdf::parse(&contents) {
            if let Some(users) = root.get("users") {
                let mut best: Option<(i64, PathBuf)> = None;
                for (steamid64, user) in users.entries() {
                    let account_id: u64 = match steamid64.parse::<u64>() {
                        Ok(id) => id.wrapping_sub(76561197960265728),
                        Err(_) => continue,
                    };
                    let dir = userdata_root.join(account_id.to_string());
                    if !dir.is_dir() {
                        continue;
                    }
                    let most_recent = user.get_str("MostRecent") == Some("1");
                    let timestamp = user.get_i64("Timestamp").unwrap_or(0);
                    let score = if most_recent { i64::MAX } else { timestamp };
                    if best.as_ref().map_or(true, |(s, _)| score > *s) {
                        best = Some((score, dir));
                    }
                }
                if let Some((_, dir)) = best {
                    return Some(dir);
                }
            }
        }
    }

    // Fall back to whichever single userdata folder exists.
    fs::read_dir(&userdata_root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
}
