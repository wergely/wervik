//! Steam games owned by the active account, including titles that are not
//! currently installed. The list is deliberately cached per SteamID64 so a
//! shared Windows installation can never display another account's library.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use ureq::Agent;

use crate::commands::AppStateHandle;

const CACHE_MAX_AGE_SECONDS: i64 = 60 * 60 * 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedGame {
    pub app_id: u32,
    pub name: String,
    pub playtime_minutes: Option<u64>,
    pub last_played: Option<i64>,
    /// The small icon URL returned by Steam. It is kept as a fallback for
    /// titles whose library artwork is not available on the CDN.
    pub icon_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct CacheFile {
    #[serde(default)]
    steam_id64: Option<String>,
    #[serde(default)]
    fetched_at: i64,
    #[serde(default)]
    games: Vec<OwnedGame>,
}

#[derive(Debug)]
pub struct SteamOwnedGamesCache {
    path: PathBuf,
    steam_id64: Option<String>,
    fetched_at: i64,
    games: Vec<OwnedGame>,
}

impl SteamOwnedGamesCache {
    pub fn load(path: &Path) -> Self {
        let file = std::fs::read_to_string(path)
            .ok()
            .and_then(|contents| serde_json::from_str::<CacheFile>(&contents).ok())
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            steam_id64: file.steam_id64,
            fetched_at: file.fetched_at,
            games: file.games,
        }
    }

    /// Switches the cache to one Steam account. A changed account starts
    /// with an empty list until that account's own list is fetched.
    pub fn select_account(&mut self, steam_id64: &str) -> Result<(), String> {
        let steam_id64 = steam_id64.trim();
        if steam_id64.is_empty() || self.steam_id64.as_deref() == Some(steam_id64) {
            return Ok(());
        }
        self.steam_id64 = Some(steam_id64.to_string());
        self.fetched_at = 0;
        self.games.clear();
        self.persist()
    }

    pub fn get(&self, steam_id64: &str) -> Option<Vec<OwnedGame>> {
        (self.steam_id64.as_deref() == Some(steam_id64)).then(|| self.games.clone())
    }

    pub fn get_fresh(&self, steam_id64: &str) -> Option<Vec<OwnedGame>> {
        if self.steam_id64.as_deref() != Some(steam_id64) {
            return None;
        }
        (now_unix().saturating_sub(self.fetched_at) < CACHE_MAX_AGE_SECONDS)
            .then(|| self.games.clone())
    }

    pub fn put(&mut self, steam_id64: &str, games: Vec<OwnedGame>) -> Result<(), String> {
        if self.steam_id64.as_deref() != Some(steam_id64) {
            return Err("Steam account changed while the owned games request was in flight".into());
        }
        self.games = games;
        self.fetched_at = now_unix();
        self.persist()
    }

    fn persist(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&CacheFile {
            steam_id64: self.steam_id64.clone(),
            fetched_at: self.fetched_at,
            games: self.games.clone(),
        })
        .map_err(|error| error.to_string())?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&self.path, json).map_err(|error| error.to_string())
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn new_agent() -> Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into()
}

#[derive(Debug, Deserialize)]
struct OwnedGamesResponse {
    #[serde(default)]
    response: OwnedGames,
}

#[derive(Debug, Deserialize, Default)]
struct OwnedGames {
    #[serde(default)]
    game_count: Option<usize>,
    #[serde(default)]
    games: Vec<ApiOwnedGame>,
}

#[derive(Debug, Deserialize)]
struct ApiOwnedGame {
    appid: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    playtime_forever: u64,
    #[serde(default)]
    rtime_last_played: i64,
    #[serde(default)]
    img_icon_url: String,
}

fn fetch_owned_games(api_key: &str, steam_id64: &str) -> Result<Vec<OwnedGame>, String> {
    let url = format!(
        "https://api.steampowered.com/IPlayerService/GetOwnedGames/v0001/?key={api_key}&steamid={steam_id64}&format=json&include_appinfo=1&include_played_free_games=1"
    );
    let response: OwnedGamesResponse = new_agent()
        .get(&url)
        .call()
        .map_err(|error| format!("Steam owned games request failed: {error}"))?
        .body_mut()
        .read_json()
        .map_err(|error| format!("Could not read the Steam owned games list: {error}"))?;
    if response.response.game_count.is_none() {
        return Err(
            "Steam did not return the owned games list. Check the API key and Steam profile privacy settings."
                .into(),
        );
    }

    let mut games: Vec<OwnedGame> = response
        .response
        .games
        .into_iter()
        .filter(|game| game.appid != 0 && !game.name.trim().is_empty())
        .map(|game| OwnedGame {
            app_id: game.appid,
            name: game.name,
            playtime_minutes: (game.playtime_forever > 0).then_some(game.playtime_forever),
            last_played: (game.rtime_last_played > 0).then_some(game.rtime_last_played),
            icon_url: (!game.img_icon_url.trim().is_empty()).then(|| {
                format!(
                    "https://media.steampowered.com/steamcommunity/public/images/apps/{}/{}.jpg",
                    game.appid, game.img_icon_url
                )
            }),
        })
        .collect();
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(games)
}

/// Steam's store artwork is public and can be loaded directly by the
/// frontend. The icon URL in `OwnedGame` remains a fallback for apps without
/// a library portrait.
pub fn cover_url(app_id: u32) -> String {
    format!(
        "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{app_id}/library_600x900_2x.jpg"
    )
}

pub fn hero_url(app_id: u32) -> String {
    format!(
        "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{app_id}/library_hero.jpg"
    )
}

fn configuration_changed(app_handle: &AppHandle, api_key: &str, steam_id64: &str) -> bool {
    app_handle
        .try_state::<AppStateHandle>()
        .map(|state| {
            let state = state.lock().unwrap_or_else(|error| error.into_inner());
            state.store.settings().steam_api_key.trim() != api_key
                || state.steam_id64().as_deref() != Some(steam_id64)
        })
        .unwrap_or(false)
}

/// Refreshes the account-owned list in the background. Cached entries are
/// loaded by `AppState::start_steam_owned_fetch`, so the first screen does
/// not wait for this network request.
pub fn spawn_fetch(
    app_handle: AppHandle,
    api_key: String,
    steam_id64: String,
    running: Arc<AtomicBool>,
) {
    if api_key.trim().is_empty()
        || steam_id64.trim().is_empty()
        || running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return;
    }

    std::thread::spawn(move || {
        let result = fetch_owned_games(&api_key, &steam_id64);
        let changed = configuration_changed(&app_handle, &api_key, &steam_id64);
        let mut restart = false;
        if let Some(state_handle) = app_handle.try_state::<AppStateHandle>() {
            let mut state = state_handle
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if changed {
                restart = true;
            } else {
                match result {
                    Ok(games) => {
                        if let Err(error) = state
                            .steam_owned_games_cache
                            .put(&steam_id64, games.clone())
                        {
                            eprintln!("Could not save Steam owned games: {error}");
                        }
                        state.steam_owned_games = games;
                        let _ = app_handle.emit("steam-owned-games-updated", ());
                    }
                    Err(error) => {
                        eprintln!("Steam owned games fetch failed: {error}");
                    }
                }
            }
        }
        running.store(false, Ordering::SeqCst);
        if restart {
            if let Some(state_handle) = app_handle.try_state::<AppStateHandle>() {
                let mut state = state_handle
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                state.start_steam_owned_fetch();
            }
        }
    });
}
