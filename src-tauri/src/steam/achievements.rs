//! Steam achievements — an official, exact alternative to
//! `retroachievements` for Steam games: matched by the real Steam AppID
//! (see `Game.app_id`), no fuzzy title matching needed, via the public
//! Steam Web API (<https://steamcommunity.com/dev/apikey> for a free,
//! user-generated key).
//!
//! Valve only serves a user's own achievement progress through this API
//! when that Steam account's "Game details" privacy setting is Public —
//! this is entirely outside Wervik's control, so a private profile is
//! surfaced as a real, actionable error rather than silently showing
//! nothing (see `fetch_for_game`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use ureq::Agent;

use super::vdf;
use super::SteamCredentials;
use crate::commands::AppStateHandle;

const CACHE_MAX_AGE_SECONDS: i64 = 60 * 60 * 3;
const WERVIK_WEB_URL: &str = "https://wervik.wergely.com";
/// A game genuinely having no achievements at all doesn't change from one
/// sweep to the next, so — same reasoning as `retroachievements` — that's
/// remembered for much longer than actual progress.
const MISS_MAX_AGE_SECONDS: i64 = 60 * 60 * 24 * 7;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub id: String,
    pub title: String,
    pub description: String,
    pub icon_url: String,
    pub unlocked: bool,
    /// Unix timestamp from Steam's own `unlocktime` — 0 means not unlocked,
    /// normalized to `None` here. `#[serde(default)]` so cache entries
    /// written before this field existed still deserialize.
    #[serde(default)]
    pub unlocked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameAchievements {
    pub earned_count: usize,
    pub total_count: usize,
    pub achievements: Vec<Achievement>,
}

impl GameAchievements {
    pub fn completion_percent(&self) -> Option<u8> {
        (self.total_count > 0)
            .then(|| (self.earned_count as f64 / self.total_count as f64 * 100.0).round() as u8)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    achievements: GameAchievements,
    fetched_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct CacheFile {
    /// Progress belongs to a Steam account, not just to an app id. Keeping
    /// this alongside the cache prevents a shared Windows install from
    /// showing the previous account's progress after the active account
    /// changes.
    #[serde(default)]
    steam_id64: Option<String>,
    #[serde(default)]
    entries: HashMap<String, CacheEntry>,
    #[serde(default)]
    misses: HashMap<String, i64>,
}

#[derive(Debug)]
pub struct SteamAchievementsCache {
    path: PathBuf,
    steam_id64: Option<String>,
    entries: HashMap<String, CacheEntry>,
    misses: HashMap<String, i64>,
}

impl SteamAchievementsCache {
    pub fn load(path: &Path) -> Self {
        let file = std::fs::read_to_string(path)
            .ok()
            .and_then(|contents| serde_json::from_str::<CacheFile>(&contents).ok())
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            steam_id64: file.steam_id64,
            entries: file.entries,
            misses: file.misses,
        }
    }

    /// Switches the cache to one Steam account. Old entries are discarded
    /// before any new request can read them, which also handles an account
    /// change detected automatically from `loginusers.vdf`.
    pub fn select_account(&mut self, steam_id64: &str) -> Result<(), String> {
        let steam_id64 = steam_id64.trim();
        if steam_id64.is_empty() || self.steam_id64.as_deref() == Some(steam_id64) {
            return Ok(());
        }
        self.steam_id64 = Some(steam_id64.to_string());
        self.entries.clear();
        self.misses.clear();
        self.persist()
    }

    /// Keyed by the game's unified Wervik id — a Steam app id as a string,
    /// same as `GameDto.id` for a Steam game.
    pub fn get_fresh(&self, game_id: &str, steam_id64: &str) -> Option<GameAchievements> {
        if self.steam_id64.as_deref() != Some(steam_id64) {
            return None;
        }
        let entry = self.entries.get(game_id)?;
        (now_unix().saturating_sub(entry.fetched_at) < CACHE_MAX_AGE_SECONDS)
            .then(|| entry.achievements.clone())
    }

    pub fn get(&self, game_id: &str, steam_id64: &str) -> Option<GameAchievements> {
        if self.steam_id64.as_deref() != Some(steam_id64) {
            return None;
        }
        self.entries
            .get(game_id)
            .map(|entry| entry.achievements.clone())
    }

    pub fn snapshots(&self, steam_id64: &str) -> Vec<(String, GameAchievements, i64)> {
        if self.steam_id64.as_deref() != Some(steam_id64) {
            return Vec::new();
        }
        self.entries
            .iter()
            .map(|(game_id, entry)| {
                (
                    game_id.clone(),
                    entry.achievements.clone(),
                    entry.fetched_at,
                )
            })
            .collect()
    }

    pub fn recent_miss(&self, game_id: &str, steam_id64: &str) -> bool {
        if self.steam_id64.as_deref() != Some(steam_id64) {
            return false;
        }
        self.misses
            .get(game_id)
            .is_some_and(|fetched_at| now_unix().saturating_sub(*fetched_at) < MISS_MAX_AGE_SECONDS)
    }

    pub fn put(
        &mut self,
        game_id: String,
        steam_id64: &str,
        achievements: GameAchievements,
    ) -> Result<(), String> {
        if self.steam_id64.as_deref() != Some(steam_id64) {
            return Err(
                "Steam achievements account changed while the request was in flight".to_string(),
            );
        }
        self.misses.remove(&game_id);
        self.entries.insert(
            game_id,
            CacheEntry {
                achievements,
                fetched_at: now_unix(),
            },
        );
        self.persist()
    }

    pub fn mark_miss(&mut self, game_id: String, steam_id64: &str) -> Result<(), String> {
        if self.steam_id64.as_deref() != Some(steam_id64) {
            return Err(
                "Steam achievements account changed while the request was in flight".to_string(),
            );
        }
        self.entries.remove(&game_id);
        self.misses.insert(game_id, now_unix());
        self.persist()
    }

    fn persist(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&CacheFile {
            steam_id64: self.steam_id64.clone(),
            entries: self.entries.clone(),
            misses: self.misses.clone(),
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
        .timeout_global(Some(Duration::from_secs(12)))
        .build()
        .into()
}

/// Reads the SteamID64 of whichever local account most recently signed
/// into this Steam install, from its own `config/loginusers.vdf` — the
/// same file Steam itself uses to remember local accounts. `None` if no
/// account has ever signed in here (a fresh/portable install) or the file
/// is missing/unreadable.
pub fn detect_steam_id64(steam_root: &Path) -> Option<String> {
    let contents =
        std::fs::read_to_string(steam_root.join("config").join("loginusers.vdf")).ok()?;
    let root = vdf::parse(&contents).ok()?;
    let users = root.get("users")?;
    let mut fallback = None;
    for (id, user) in users.entries() {
        if user.get_str("MostRecent") == Some("1") {
            return Some(id.clone());
        }
        fallback.get_or_insert_with(|| id.clone());
    }
    fallback
}

#[derive(Debug, Deserialize)]
struct SchemaResponse {
    #[serde(default)]
    game: Option<SchemaGame>,
}

#[derive(Debug, Deserialize, Default)]
struct SchemaGame {
    #[serde(rename = "availableGameStats", default)]
    available_game_stats: Option<AvailableGameStats>,
}

#[derive(Debug, Deserialize, Default)]
struct AvailableGameStats {
    #[serde(default)]
    achievements: Vec<SchemaAchievement>,
}

#[derive(Debug, Deserialize, Clone)]
struct SchemaAchievement {
    name: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(default)]
    description: String,
    icon: String,
    icongray: String,
}

fn fetch_schema(
    agent: &Agent,
    api_key: &str,
    app_id: u32,
) -> Result<Vec<SchemaAchievement>, String> {
    let url = format!(
        "https://api.steampowered.com/ISteamUserStats/GetSchemaForGame/v2/?key={api_key}&appid={app_id}"
    );
    let response: SchemaResponse = agent
        .get(&url)
        .call()
        .map_err(|error| format!("Steam request failed: {error}"))?
        .body_mut()
        .read_json()
        .map_err(|error| format!("Could not read the Steam achievement schema: {error}"))?;
    Ok(response
        .game
        .unwrap_or_default()
        .available_game_stats
        .unwrap_or_default()
        .achievements)
}

#[derive(Debug, Deserialize)]
struct PlayerAchievementsResponse {
    playerstats: PlayerStats,
}

#[derive(Debug, Deserialize)]
struct PlayerStats {
    /// Steam omits this field on successful responses and only includes it
    /// with `false` in many error responses.
    #[serde(default)]
    success: Option<bool>,
    error: Option<String>,
    #[serde(default)]
    achievements: Vec<PlayerAchievement>,
}

#[derive(Debug, Deserialize)]
struct PlayerAchievement {
    apiname: String,
    achieved: u8,
    #[serde(default)]
    unlocktime: i64,
}

fn fetch_unlocked(
    agent: &Agent,
    api_key: &str,
    steam_id64: &str,
    app_id: u32,
) -> Result<HashMap<String, i64>, String> {
    let url = format!(
        "https://api.steampowered.com/ISteamUserStats/GetPlayerAchievements/v1/?key={api_key}&steamid={steam_id64}&appid={app_id}"
    );
    let response: PlayerAchievementsResponse = agent
        .get(&url)
        .call()
        .map_err(|error| format!("Steam request failed: {error}"))?
        .body_mut()
        .read_json()
        .map_err(|error| format!("Could not read Steam achievement progress: {error}"))?;
    if response.playerstats.success == Some(false) {
        return Err(response.playerstats.error.unwrap_or_else(|| {
            "Steam refused this request — check that \"Game details\" is set to Public in your \
             Steam profile's privacy settings."
                .to_string()
        }));
    }
    Ok(response
        .playerstats
        .achievements
        .into_iter()
        .filter(|achievement| achievement.achieved != 0)
        .map(|achievement| (achievement.apiname, achievement.unlocktime))
        .collect())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WervikAchievementsRequest<'a> {
    steam_id64: &'a str,
    app_id: u32,
}

#[derive(Debug, Deserialize, Default)]
struct WervikAchievementsResponse {
    #[serde(default)]
    achievements: Option<GameAchievements>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Same lookup as `fetch_for_game_with_key`, but through Wervik's own proxy —
/// available once a Wervik Account is connected, using Wervik's own Steam Web
/// API key instead of one the user has to generate and paste in Settings.
fn fetch_for_game_from_wervik(
    access_token: &str,
    steam_id64: &str,
    app_id: u32,
) -> Result<Option<GameAchievements>, String> {
    let configured_base =
        std::env::var("WERVIK_WEB_URL").unwrap_or_else(|_| WERVIK_WEB_URL.to_string());
    let mut url = url::Url::parse(&configured_base)
        .map_err(|error| format!("Invalid Wervik web URL: {error}"))?;
    url.set_path("/api/steam/achievements");

    let mut response = new_agent()
        .post(url.as_str())
        .header("Authorization", &format!("Bearer {access_token}"))
        .send_json(&WervikAchievementsRequest { steam_id64, app_id })
        .map_err(|error| format!("Wervik Steam achievements request failed: {error}"))?;
    let response = response
        .body_mut()
        .read_json::<WervikAchievementsResponse>()
        .map_err(|error| format!("Could not read the Wervik Steam achievements response: {error}"))?;
    if let Some(error) = response.error {
        return Err(response.message.unwrap_or(error));
    }
    Ok(response.achievements)
}

/// Looks up one Steam game's achievements and the caller's progress on them,
/// through whichever `SteamCredentials` are currently in effect (the user's
/// own key, or Wervik's proxy once an account is connected).
pub fn fetch_for_game(
    credentials: &SteamCredentials,
    steam_id64: &str,
    app_id: u32,
) -> Result<Option<GameAchievements>, String> {
    match credentials {
        SteamCredentials::Manual(api_key) => fetch_for_game_with_key(api_key, steam_id64, app_id),
        SteamCredentials::Wervik(access_token) => {
            fetch_for_game_from_wervik(access_token, steam_id64, app_id)
        }
    }
}

/// `Ok(None)` means the game genuinely has no achievements at all — not an
/// error. A private profile, an invalid key, or a network failure are real
/// errors, surfaced as `Err` rather than hidden as "nothing to show", since —
/// unlike a RetroAchievements title/console mismatch — they're something the
/// user can actually go fix.
fn fetch_for_game_with_key(
    api_key: &str,
    steam_id64: &str,
    app_id: u32,
) -> Result<Option<GameAchievements>, String> {
    let agent = new_agent();
    let schema = fetch_schema(&agent, api_key, app_id)?;
    if schema.is_empty() {
        return Ok(None);
    }
    let unlocked = fetch_unlocked(&agent, api_key, steam_id64, app_id)?;

    let achievements: Vec<Achievement> = schema
        .into_iter()
        .map(|entry| {
            let unlocked_at = unlocked.get(&entry.name).copied().filter(|&time| time > 0);
            let is_unlocked = unlocked.contains_key(&entry.name);
            Achievement {
                id: entry.name,
                title: entry.display_name,
                description: entry.description,
                icon_url: if is_unlocked {
                    entry.icon
                } else {
                    entry.icongray
                },
                unlocked: is_unlocked,
                unlocked_at,
            }
        })
        .collect();
    let earned_count = achievements.iter().filter(|a| a.unlocked).count();

    Ok(Some(GameAchievements {
        earned_count,
        total_count: achievements.len(),
        achievements,
    }))
}

fn should_skip(
    app_handle: &AppHandle,
    game_id: &str,
    credentials: &SteamCredentials,
    steam_id64: &str,
) -> bool {
    app_handle
        .try_state::<AppStateHandle>()
        .map(|state| {
            let state = state.lock().unwrap_or_else(|e| e.into_inner());
            if state.steam_credentials().as_ref() != Some(credentials)
                || state.steam_id64().as_deref() != Some(steam_id64)
            {
                return true;
            }
            state
                .steam_achievements_cache
                .get_fresh(game_id, steam_id64)
                .is_some()
                || state
                    .steam_achievements_cache
                    .recent_miss(game_id, steam_id64)
        })
        .unwrap_or(false)
}

/// Same delay used for the RetroAchievements sweep — kept well below what
/// a single interactive lookup would use, since this runs unattended over
/// a whole library rather than one game a user is actively waiting on.
const SWEEP_REQUEST_DELAY: Duration = Duration::from_millis(400);

/// Populates the achievements cache for every Steam game in the
/// background, so the Library grid can show a completion badge without
/// the user opening each game's page first — same trigger pattern and
/// cache-skip logic as `retroachievements::spawn_fetch`. `app_ids` pairs
/// each game's Wervik id (its app id as a string) with the numeric app id
/// the Steam API itself needs. A `None` `steam_id64` (couldn't be
/// auto-detected and no manual override is set) skips the sweep entirely —
/// same as missing credentials.
pub fn spawn_fetch(
    app_handle: AppHandle,
    app_ids: Vec<(String, u32)>,
    credentials: SteamCredentials,
    steam_id64: Option<String>,
    running: Arc<AtomicBool>,
) {
    let Some(steam_id64) = steam_id64.filter(|id| !id.trim().is_empty()) else {
        return;
    };
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    std::thread::spawn(move || {
        let mut updated_any = false;
        for (game_id, app_id) in app_ids
            .into_iter()
            .filter(|(id, _)| !should_skip(&app_handle, id, &credentials, &steam_id64))
        {
            match fetch_for_game(&credentials, &steam_id64, app_id) {
                Ok(Some(achievements)) => {
                    let mut updated = false;
                    if let Some(state) = app_handle.try_state::<AppStateHandle>() {
                        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                        if let Err(error) = state.steam_achievements_cache.put(
                            game_id.clone(),
                            &steam_id64,
                            achievements,
                        ) {
                            eprintln!(
                                "Could not save Steam achievements for app {app_id}: {error}"
                            );
                        } else {
                            let _ = app_handle.emit("steam-achievements-updated", &game_id);
                            updated = true;
                        }
                    }
                    if updated {
                        updated_any = true;
                    }
                }
                Ok(None) => {
                    if let Some(state) = app_handle.try_state::<AppStateHandle>() {
                        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                        if let Err(error) = state
                            .steam_achievements_cache
                            .mark_miss(game_id.clone(), &steam_id64)
                        {
                            eprintln!(
                                "Could not save Steam achievements miss for app {app_id}: {error}"
                            );
                        }
                    }
                }
                Err(error) => {
                    // A single game's failure (most commonly a private
                    // profile) would otherwise repeat for every remaining
                    // game in the sweep for no new information — log once
                    // and stop instead of spamming the same error N times.
                    eprintln!("Steam achievements sweep stopped: {error}");
                    break;
                }
            }
            std::thread::sleep(SWEEP_REQUEST_DELAY);
        }
        running.store(false, Ordering::SeqCst);
        if updated_any {
            crate::commands::schedule_wervik_achievements_sync(&app_handle);
        }
        // A settings change can race an in-flight HTTP request. The old
        // worker deliberately refuses to write under the new account's
        // cache identity; restart once here so the new configuration is not
        // left waiting for another rescan.
        if let Some(state) = app_handle.try_state::<AppStateHandle>() {
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            if state.steam_credentials().as_ref() != Some(&credentials)
                || state.steam_id64().as_deref() != Some(steam_id64.as_str())
            {
                state.start_steam_achievements_fetch();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_most_recent_account() {
        let dir = std::env::temp_dir().join(format!(
            "wervik-steam-achievements-test-{}",
            std::process::id()
        ));
        let config_dir = dir.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("loginusers.vdf"),
            r#"
            "users"
            {
                "76561197960287930"
                {
                    "AccountName"       "old_account"
                    "MostRecent"        "0"
                }
                "76561198012345678"
                {
                    "AccountName"       "current_account"
                    "MostRecent"        "1"
                }
            }
            "#,
        )
        .unwrap();

        assert_eq!(
            detect_steam_id64(&dir),
            Some("76561198012345678".to_string())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_to_any_account_when_none_marked_most_recent() {
        let dir = std::env::temp_dir().join(format!(
            "wervik-steam-achievements-test-fallback-{}",
            std::process::id()
        ));
        let config_dir = dir.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("loginusers.vdf"),
            r#"
            "users"
            {
                "76561197960287930"
                {
                    "AccountName"       "only_account"
                }
            }
            "#,
        )
        .unwrap();

        assert_eq!(
            detect_steam_id64(&dir),
            Some("76561197960287930".to_string())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn returns_none_without_a_loginusers_file() {
        let dir = std::env::temp_dir().join(format!(
            "wervik-steam-achievements-test-missing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(detect_steam_id64(&dir), None);
    }

    #[test]
    fn accepts_successful_player_response_without_a_success_field() {
        let response: PlayerAchievementsResponse = serde_json::from_str(
            r#"{
                "playerstats": {
                    "steamID": "76561198012345678",
                    "gameName": "Example",
                    "achievements": [
                        { "apiname": "FIRST", "achieved": 1 }
                    ]
                }
            }"#,
        )
        .unwrap();

        assert_ne!(response.playerstats.success, Some(false));
        assert_eq!(response.playerstats.achievements.len(), 1);
    }

    #[test]
    fn cache_does_not_reuse_progress_for_another_account() {
        let path = std::env::temp_dir().join(format!(
            "wervik-steam-achievements-cache-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut cache = SteamAchievementsCache::load(&path);
        let achievements = GameAchievements {
            earned_count: 1,
            total_count: 2,
            achievements: Vec::new(),
        };

        cache.select_account("76561198012345678").unwrap();
        cache
            .put("10".to_string(), "76561198012345678", achievements.clone())
            .unwrap();
        assert!(cache.get("10", "76561198012345678").is_some());

        cache.select_account("76561198087654321").unwrap();
        assert!(cache.get("10", "76561198087654321").is_none());
        assert!(cache.get("10", "76561198012345678").is_none());

        let _ = std::fs::remove_file(&path);
    }
}
