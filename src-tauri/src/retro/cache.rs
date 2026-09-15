//! Persists the last ROM discovery so Wervik never has to walk (or block
//! on walking) the filesystem on every startup — only on an explicit
//! rescan, or the very first run.

use std::path::{Path, PathBuf};

use super::discovery::is_arcade_directory_name;
use super::discovery::RomGame;
use super::platform::Platform;

fn cache_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("retro-library-cache.json")
}

pub fn load(app_data_dir: &Path) -> Vec<RomGame> {
    std::fs::read_to_string(cache_path(app_data_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app_data_dir: &Path, games: &[RomGame]) -> Result<(), String> {
    std::fs::create_dir_all(app_data_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(games).map_err(|e| e.to_string())?;
    std::fs::write(cache_path(app_data_dir), json).map_err(|e| e.to_string())
}

/// Repairs the cache entries affected by the old ZIP classification rule.
/// A full rescan is still explicit because ROM folders can be large, but a
/// cached Genesis entry whose own parent is clearly a CPS/Neo Geo/arcade
/// collection can be corrected immediately at startup without touching the
/// ROM files.
pub fn repair_cached_platforms(games: &mut [RomGame]) -> bool {
    let mut changed = false;
    for game in games {
        if game.platform != Platform::Genesis {
            continue;
        }
        let Some(directory_name) = Path::new(&game.rom_path)
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
        else {
            continue;
        };
        if is_arcade_directory_name(directory_name) {
            game.platform = Platform::Arcade;
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_cached_cps_and_neo_geo_entries_without_touching_real_genesis() {
        let mut games = vec![
            RomGame {
                id: "cps".to_string(),
                name: "Street Fighter".to_string(),
                platform: Platform::Genesis,
                rom_path: "D:/Games/CapcomCPS2ByDataghost/sfz3.zip".to_string(),
                size_bytes: 1,
                modified_at: 0,
                variants: Vec::new(),
            },
            RomGame {
                id: "neo-geo".to_string(),
                name: "King of Fighters".to_string(),
                platform: Platform::Genesis,
                rom_path: "D:/Games/neo-geo-aes-romset/kof97.zip".to_string(),
                size_bytes: 1,
                modified_at: 0,
                variants: Vec::new(),
            },
            RomGame {
                id: "genesis".to_string(),
                name: "Sonic".to_string(),
                platform: Platform::Genesis,
                rom_path: "D:/Games/MegaDrive/sonic.bin".to_string(),
                size_bytes: 1,
                modified_at: 0,
                variants: Vec::new(),
            },
        ];

        assert!(repair_cached_platforms(&mut games));
        assert_eq!(games[0].platform, Platform::Arcade);
        assert_eq!(games[1].platform, Platform::Arcade);
        assert_eq!(games[2].platform, Platform::Genesis);
    }
}
