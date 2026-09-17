//! Resolves and caches artwork for the Battle.net products Wervik discovers.
//!
//! Battle.net's product database contains install and launch information, but
//! not the cover art shown by its client. The first pass therefore uses
//! official Blizzard Press Center assets and stores them in Wervik's app data.
//! The network request runs in the background; once cached, the UI reads the
//! files locally just like Steam and retro artwork.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use ureq::Agent;

use super::discovery::BattleNetGame;

struct ArtworkUrls {
    cover: &'static str,
    hero: &'static str,
}

fn artwork_urls(launch_code: &str) -> Option<ArtworkUrls> {
    // These URLs point to Blizzard's official Press Center image CDN. The
    // `sky` parameter is the CDN's signed transformation token; keeping the
    // complete URL is necessary because the host rejects unsigned variants.
    match launch_code {
        "WoW" => Some(ArtworkUrls {
            cover: "https://imguscdn.gamespress.com/cdn/files/BlizzardLive/2024/08/170146-fa2a03b1/World_of_Warcraft_The_War_Within_EA_Key_Art_1_Logo_1080x1920.jpg?w=250&mode=max&otf=y&quality=90&format=jpg&bgcolor=black&sky=d0b9ee1268640dc86b241350973aa0165b7e7ed7db677e43f6d99ae545807655",
            hero: "https://imguscdn.gamespress.com/cdn/files/BlizzardLive/2024/08/170146-fa2a03b1/World_of_Warcraft_The_War_Within_EA_Key_Art_1_Logo_1920x1080.jpg?w=250&mode=max&otf=y&quality=90&format=jpg&bgcolor=black&sky=2685ceb26014b8b9239962f6901faf06e53fcb681b31e371032664084ac119b5",
        }),
        "WoW_wow_classic" => Some(ArtworkUrls {
            cover: "https://imguscdn.gamespress.com/cdn/files/BlizzardLive/2018/09/26173006-789f632f-c43a-45b4-abd9-f3a380bb474e/WOW_Classic.jpg?w=250&mode=max&otf=y&quality=90&format=jpg&bgcolor=black&sky=99530a15803a4c29b9110b422af174745f7af3c1e08f7792f9349b6e5ff23641",
            hero: "https://imguscdn.gamespress.com/cdn/files/BlizzardLive/2019/05/10015647-250e3f31-10e8-4002-9210-426169a53703/ClassicLaunch_WoW_Andorhal_3840x2160.jpg?w=250&mode=max&otf=y&quality=90&format=jpg&bgcolor=black&sky=af177efaf5a4d2b776ac622651aae67d937d59ff32dc1fd9d21aaad2b22820c8",
        }),
        _ => None,
    }
}

fn cache_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("battlenet-artwork")
}

fn cache_key(game: &BattleNetGame) -> String {
    game.id
        .strip_prefix("battlenet:")
        .unwrap_or(&game.id)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn cache_path(app_data_dir: &Path, game: &BattleNetGame, kind: &str) -> PathBuf {
    cache_dir(app_data_dir).join(format!("{}-{kind}.jpg", cache_key(game)))
}

fn is_valid_image(path: &Path) -> bool {
    use std::io::Read;

    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 3];
    if file.read_exact(&mut magic).is_err() {
        return false;
    }
    // The current Battle.net sources are JPEGs. Accept PNG as well so a
    // future asset can be added without changing the cache reader.
    magic == [0xff, 0xd8, 0xff] || magic == [0x89, 0x50, 0x4e]
}

fn valid_cached_path(path: PathBuf) -> Option<PathBuf> {
    if !path.exists() {
        return None;
    }
    if is_valid_image(&path) {
        return Some(path);
    }
    let _ = std::fs::remove_file(path);
    None
}

pub fn cached_cover(app_data_dir: &Path, game: &BattleNetGame) -> Option<PathBuf> {
    valid_cached_path(cache_path(app_data_dir, game, "cover"))
}

pub fn cached_hero(app_data_dir: &Path, game: &BattleNetGame) -> Option<PathBuf> {
    valid_cached_path(cache_path(app_data_dir, game, "hero"))
}

fn download(agent: &Agent, url: &str) -> Result<Option<Vec<u8>>, String> {
    match agent.get(url).call() {
        Ok(mut response) => {
            let bytes = response
                .body_mut()
                .read_to_vec()
                .map_err(|e| e.to_string())?;
            if bytes.starts_with(&[0xff, 0xd8, 0xff])
                || bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47])
            {
                Ok(Some(bytes))
            } else {
                Ok(None)
            }
        }
        Err(ureq::Error::StatusCode(_)) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn fetch_missing(
    agent: &Agent,
    app_data_dir: &Path,
    game: &BattleNetGame,
    urls: &ArtworkUrls,
) -> Result<bool, String> {
    let dir = cache_dir(app_data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut downloaded = false;

    for (kind, url) in [("cover", urls.cover), ("hero", urls.hero)] {
        let destination = cache_path(app_data_dir, game, kind);
        if valid_cached_path(destination.clone()).is_some() {
            continue;
        }
        if let Some(bytes) = download(agent, url)? {
            std::fs::write(destination, bytes).map_err(|e| e.to_string())?;
            downloaded = true;
        }
    }

    Ok(downloaded)
}

pub fn new_agent() -> Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into()
}

/// Fetches known Battle.net artwork without delaying startup. A successful
/// pass emits an event so the frontend can request a fresh library snapshot.
pub fn spawn_fetch(
    app_handle: AppHandle,
    app_data_dir: PathBuf,
    games: Vec<BattleNetGame>,
    running: Arc<AtomicBool>,
) {
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    std::thread::spawn(move || {
        let agent = new_agent();
        for game in games {
            let Some(urls) = artwork_urls(&game.launch_code) else {
                continue;
            };
            match fetch_missing(&agent, &app_data_dir, &game, &urls) {
                Ok(true) => {
                    if let Some(state) = app_handle.try_state::<crate::commands::AppStateHandle>() {
                        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(path) = cached_cover(&app_data_dir, &game) {
                            state
                                .known_image_paths
                                .insert(path.to_string_lossy().into_owned());
                        }
                        if let Some(path) = cached_hero(&app_data_dir, &game) {
                            state
                                .known_image_paths
                                .insert(path.to_string_lossy().into_owned());
                        }
                    }
                    let _ = app_handle.emit("battlenet-artwork-updated", &game.id);
                }
                Ok(false) => {}
                Err(error) => {
                    eprintln!("Battle.net artwork fetch failed for {}: {error}", game.name)
                }
            }
        }
        running.store(false, Ordering::SeqCst);
    });
}
