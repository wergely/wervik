//! Scans Steam library folders for installed games and merges in local
//! playtime/last-played data and artwork.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{artwork, discovery, vdf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub app_id: u32,
    pub name: String,
    pub install_dir: PathBuf,
    pub library_path: PathBuf,
    pub size_on_disk_bytes: u64,
    /// Unix timestamp seconds, if Steam has recorded a last-played time.
    pub last_played: Option<i64>,
    /// Total playtime in minutes, if locally available.
    pub playtime_minutes: Option<u64>,
    pub cover_path: Option<PathBuf>,
    pub hero_path: Option<PathBuf>,
    pub logo_path: Option<PathBuf>,
}

/// StateFlags bit indicating the app is fully installed.
const STATE_FULLY_INSTALLED: u64 = 4;

pub struct ScanResult {
    pub games: Vec<Game>,
    pub library_folders: Vec<PathBuf>,
}

pub fn scan(steam_root: &Path) -> ScanResult {
    let folders = discovery::discover_library_folders(steam_root);
    let playtimes = load_playtimes(steam_root);

    let mut games = Vec::new();
    for folder in &folders {
        let steamapps = folder.path.join("steamapps");
        let librarycache = steam_root.join("appcache/librarycache");
        let Ok(entries) = fs::read_dir(&steamapps) else {
            continue;
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let is_manifest = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"))
                .unwrap_or(false);
            if !is_manifest {
                continue;
            }

            if let Some(game) = parse_manifest(&path, &steamapps, &librarycache, &playtimes) {
                games.push(game);
            }
        }
    }

    let mut games = dedupe_by_app_id(games);
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    ScanResult {
        games,
        library_folders: folders.into_iter().map(|f| f.path).collect(),
    }
}

/// A game can have a fully-installed manifest in more than one library
/// folder at once — e.g. a leftover copy left behind after moving the
/// install to another drive, which Steam itself doesn't always clean up —
/// so scanning every folder can otherwise produce two entries for the same
/// `app_id`. Keeps whichever looks like the one actually being played.
fn dedupe_by_app_id(games: Vec<Game>) -> Vec<Game> {
    let mut by_id: HashMap<u32, Game> = HashMap::new();
    for game in games {
        let app_id = game.app_id;
        match by_id.get(&app_id) {
            Some(existing) if !is_more_current(&game, existing) => {}
            _ => {
                by_id.insert(app_id, game);
            }
        }
    }
    by_id.into_values().collect()
}

fn is_more_current(candidate: &Game, current: &Game) -> bool {
    match (candidate.last_played, current.last_played) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => candidate.size_on_disk_bytes > current.size_on_disk_bytes,
    }
}

fn parse_manifest(
    manifest_path: &Path,
    steamapps: &Path,
    librarycache: &Path,
    playtimes: &HashMap<u32, PlaytimeEntry>,
) -> Option<Game> {
    let contents = fs::read_to_string(manifest_path).ok()?;
    let root = vdf::parse(&contents).ok()?;
    let state = root.get("AppState")?;

    let app_id: u32 = state.get_str("appid")?.parse().ok()?;
    let name = state.get_str("name")?.to_string();
    let install_dir_name = state.get_str("installdir")?.to_string();
    let state_flags = state.get_u64("StateFlags").unwrap_or(0);

    if state_flags & STATE_FULLY_INSTALLED == 0 {
        return None;
    }

    if is_system_tool(&name, &install_dir_name) {
        return None;
    }

    let install_dir = steamapps.join("common").join(&install_dir_name);
    if !install_dir.is_dir() {
        // Manifest exists but the game's files are gone (e.g. removed
        // externally); don't show a broken entry.
        return None;
    }

    let size_on_disk_bytes = state.get_u64("SizeOnDisk").unwrap_or(0);

    // Prefer localconfig.vdf playtime data (kept fresh across sessions);
    // fall back to LastPlayed recorded directly in the manifest.
    let manifest_last_played = state.get_i64("LastPlayed").filter(|&t| t > 0);
    let playtime_entry = playtimes.get(&app_id);
    let last_played = playtime_entry
        .and_then(|p| p.last_played)
        .or(manifest_last_played);
    let playtime_minutes = playtime_entry.and_then(|p| p.playtime_minutes);

    let art = artwork::resolve(librarycache, app_id);

    Some(Game {
        app_id,
        name,
        install_dir,
        library_path: steamapps.parent().unwrap_or(steamapps).to_path_buf(),
        size_on_disk_bytes,
        last_played,
        playtime_minutes,
        cover_path: art.cover,
        hero_path: art.hero,
        logo_path: art.logo,
    })
}

/// Steam's local appmanifest files carry no app-type field (Game vs. Tool
/// lives only in Valve's backend appinfo, which isn't practical to parse
/// locally), so runtimes/compat tools are recognized by Valve's consistent
/// installdir/name conventions instead.
fn is_system_tool(name: &str, install_dir_name: &str) -> bool {
    install_dir_name.starts_with("SteamLinuxRuntime")
        || install_dir_name.starts_with("Proton")
        || install_dir_name == "Steamworks Shared"
        || name == "Steamworks Common Redistributables"
}

struct PlaytimeEntry {
    last_played: Option<i64>,
    playtime_minutes: Option<u64>,
}

fn load_playtimes(steam_root: &Path) -> HashMap<u32, PlaytimeEntry> {
    let mut map = HashMap::new();
    let Some(userdata_dir) = discovery::find_active_userdata_dir(steam_root) else {
        return map;
    };
    let localconfig_path = userdata_dir.join("config/localconfig.vdf");
    let Ok(contents) = fs::read_to_string(&localconfig_path) else {
        return map;
    };
    let Ok(root) = vdf::parse(&contents) else {
        return map;
    };

    let apps = root.path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]);
    let Some(apps) = apps else {
        return map;
    };

    for (app_id_str, app) in apps.entries() {
        let Ok(app_id) = app_id_str.parse::<u32>() else {
            continue;
        };
        let last_played = app.get_i64("LastPlayed").filter(|&t| t > 0);
        let playtime_minutes = app.get_u64("Playtime");
        if last_played.is_some() || playtime_minutes.is_some() {
            map.insert(
                app_id,
                PlaytimeEntry {
                    last_played,
                    playtime_minutes,
                },
            );
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(app_id: u32, last_played: Option<i64>, size_on_disk_bytes: u64) -> Game {
        Game {
            app_id,
            name: format!("Game {app_id}"),
            install_dir: PathBuf::new(),
            library_path: PathBuf::new(),
            size_on_disk_bytes,
            last_played,
            playtime_minutes: None,
            cover_path: None,
            hero_path: None,
            logo_path: None,
        }
    }

    #[test]
    fn dedupes_by_app_id_keeping_most_recently_played() {
        let games = vec![
            game(1, Some(100), 10),
            game(1, Some(200), 10),
            game(2, None, 5),
        ];
        let deduped = dedupe_by_app_id(games);
        assert_eq!(deduped.len(), 2);
        let kept = deduped.iter().find(|g| g.app_id == 1).unwrap();
        assert_eq!(kept.last_played, Some(200));
    }

    /// When neither copy has a recorded last-played time, the larger
    /// install (more likely the real one, not an abandoned leftover) wins.
    #[test]
    fn dedupes_by_app_id_falling_back_to_size_on_disk() {
        let games = vec![game(1, None, 500), game(1, None, 900_000_000)];
        let deduped = dedupe_by_app_id(games);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].size_on_disk_bytes, 900_000_000);
    }
}
