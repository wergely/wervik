//! Discovers EA App games registered in the Windows game-install registry.
//!
//! EA App does not expose a local, stable library manifest. Its installer
//! still records installed products under the EA Games/Origin Games registry
//! keys, which is enough to list and launch the games already on this PC.

use std::path::PathBuf;

#[cfg(windows)]
use std::path::Path;

#[derive(Debug, Clone)]
pub struct EaGame {
    /// Stable Wervik id, based on the EA content/offer id.
    pub id: String,
    pub name: String,
    /// Content/offer id accepted by EA App's launch protocol.
    pub launch_id: String,
    pub install_dir: PathBuf,
}

#[cfg(windows)]
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
#[cfg(windows)]
use winreg::RegKey;

#[cfg(windows)]
const REGISTRY_PATHS: [&str; 4] = [
    "Software\\EA Games",
    "Software\\Wow6432Node\\EA Games",
    "Software\\Origin Games",
    "Software\\Wow6432Node\\Origin Games",
];

#[cfg(windows)]
pub fn scan() -> Vec<EaGame> {
    let mut games = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let root = RegKey::predef(hive);
        for registry_path in REGISTRY_PATHS {
            let Ok(games_key) = root.open_subkey(registry_path) else {
                continue;
            };
            let Ok(subkeys) = games_key.enum_keys().collect::<Result<Vec<_>, _>>() else {
                continue;
            };

            for key_name in subkeys {
                let Ok(game_key) = games_key.open_subkey(&key_name) else {
                    continue;
                };
                let Some(install_dir) = registry_string(
                    &game_key,
                    &["Install Dir", "InstallDir", "InstallLocation", "installDir"],
                ) else {
                    continue;
                };
                let install_dir = PathBuf::from(install_dir);
                if !install_dir.is_dir() {
                    continue;
                }

                let name = registry_string(&game_key, &["DisplayName", "displayName", "Name"])
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| key_name.clone());
                if is_ea_support_entry(&name) {
                    continue;
                }

                let launch_id = registry_string(
                    &game_key,
                    &["ContentId", "contentID", "ContentID", "OfferId", "offerId"],
                )
                .or_else(|| installerdata_id(&install_dir))
                .unwrap_or_else(|| key_name.clone());
                let launch_id = launch_id.trim().to_string();
                if launch_id.is_empty() {
                    continue;
                }

                let canonical_dir =
                    std::fs::canonicalize(&install_dir).unwrap_or(install_dir.clone());
                let id = format!("ea:{}", launch_id.to_ascii_lowercase());
                let dedupe_key = format!(
                    "{}|{}",
                    canonical_dir.to_string_lossy().to_ascii_lowercase(),
                    id
                );
                if !seen.insert(dedupe_key) {
                    continue;
                }

                games.push(EaGame {
                    id,
                    name: name.trim().to_string(),
                    launch_id,
                    install_dir: canonical_dir,
                });
            }
        }
    }

    games.sort_by_cached_key(|game| game.name.to_ascii_lowercase());
    games
}

#[cfg(windows)]
fn registry_string(key: &RegKey, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        key.get_value::<String, _>(*name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

#[cfg(windows)]
fn installerdata_id(install_dir: &Path) -> Option<String> {
    [
        install_dir.join("installerdata.xml"),
        install_dir.join("__Installer/installerdata.xml"),
        install_dir.join("__Installer/installerdata/installerdata.xml"),
        install_dir.join("InstallerData/installerdata.xml"),
    ]
    .iter()
    .filter_map(|path| std::fs::read_to_string(path).ok())
    .find_map(|contents| {
        ["contentID", "contentId", "contentid", "offerId", "offerID"]
            .iter()
            .find_map(|tag| xml_tag_value(&contents, tag))
    })
}

#[cfg(windows)]
fn xml_tag_value(contents: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = contents.find(&open)? + open.len();
    let end = contents[start..].find(&close)? + start;
    let value = contents[start..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(windows)]
fn is_ea_support_entry(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "ea app" | "ea desktop" | "origin" | "origin client"
    )
}

#[cfg(not(windows))]
pub fn scan() -> Vec<EaGame> {
    Vec::new()
}
