//! Read-only RetroAchievements integration: an achievements/progress viewer
//! for retro games, via the official Web API
//! (<https://api-docs.retroachievements.org>).
//!
//! RetroAchievements has no title-search endpoint — real clients identify a
//! ROM by hashing its contents with a console-specific algorithm and
//! looking that hash up. Implementing every console's hash algorithm is a
//! separate project on its own, so this instead fetches the console's full
//! game list and matches by title using the same best-effort fuzzy
//! matching already used for IGDB (see `metadata::choose_result`) — good
//! enough for a progress viewer, not exact enough for anything that needs
//! to be certain it has the right release.
//!
//! Live achievement *unlocking* (popups, hardcore mode, memory-address
//! triggers) is a different, unrelated integration — see
//! `retro::native::configure_retroarch_achievements` and
//! `retro::native::configure_pcsx2_achievements`, which each point a managed
//! native emulator's own cheevos engine at the user's account. This module
//! doesn't drive either engine itself, but does own [`login`], the one
//! write-shaped call both of them need first (trading a password for a
//! session token) — kept here rather than duplicated in `native` since it's
//! genuinely about talking to retroachievements.org, not about either
//! emulator's own config format.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use ureq::Agent;

use crate::commands::AppStateHandle;
use crate::retro::platform::Platform;

const CACHE_MAX_AGE_SECONDS: i64 = 60 * 60 * 3;
/// How long a "no match" is remembered before retrying — much longer than
/// the progress cache above, since a title/console simply not existing on
/// RetroAchievements is not something that changes from one rescan to the
/// next, unlike a user's own progress.
const MISS_MAX_AGE_SECONDS: i64 = 60 * 60 * 24 * 7;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub badge_url: String,
    pub unlocked: bool,
    pub unlocked_hardcore: bool,
    /// Unix timestamp the achievement was earned, parsed from `DateEarned`
    /// (softcore or hardcore both set it — hardcore just additionally sets
    /// `DateEarnedHardcore`). `#[serde(default)]` so cache entries written
    /// before this field existed still deserialize instead of losing all
    /// cached progress.
    #[serde(default)]
    pub unlocked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameAchievements {
    pub game_title: String,
    pub console_name: String,
    pub icon_url: Option<String>,
    pub earned_count: usize,
    pub total_count: usize,
    pub points_earned: u32,
    pub points_total: u32,
    pub achievements: Vec<Achievement>,
}

impl GameAchievements {
    /// Rounded earned/total percentage — `None` when the game has zero
    /// achievements, which is a degenerate case (a console match with no
    /// achievement set at all) rather than 0%.
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

#[derive(Debug, Serialize, Deserialize)]
struct CacheFile {
    #[serde(default)]
    entries: HashMap<String, CacheEntry>,
    /// Games the background sweep (see `spawn_fetch`) already looked up
    /// and found no RetroAchievements console/title match for, keyed by
    /// Wervik's game id with the timestamp of that attempt — so it isn't
    /// retried on every single rescan.
    #[serde(default)]
    misses: HashMap<String, i64>,
}

#[derive(Debug)]
pub struct RetroAchievementsCache {
    path: PathBuf,
    entries: HashMap<String, CacheEntry>,
    misses: HashMap<String, i64>,
}

impl RetroAchievementsCache {
    pub fn load(path: &Path) -> Self {
        let (entries, misses) = std::fs::read_to_string(path)
            .ok()
            .and_then(|contents| serde_json::from_str::<CacheFile>(&contents).ok())
            .map(|file| (file.entries, file.misses))
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            entries,
            misses,
        }
    }

    pub fn get_fresh(&self, game_id: &str) -> Option<GameAchievements> {
        let entry = self.entries.get(game_id)?;
        (now_unix().saturating_sub(entry.fetched_at) < CACHE_MAX_AGE_SECONDS)
            .then(|| entry.achievements.clone())
    }

    pub fn get(&self, game_id: &str) -> Option<GameAchievements> {
        self.entries
            .get(game_id)
            .map(|entry| entry.achievements.clone())
    }

    pub fn snapshots(&self) -> Vec<(String, GameAchievements, i64)> {
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

    pub fn recent_miss(&self, game_id: &str) -> bool {
        self.misses
            .get(game_id)
            .is_some_and(|fetched_at| now_unix().saturating_sub(*fetched_at) < MISS_MAX_AGE_SECONDS)
    }

    pub fn put(&mut self, game_id: String, achievements: GameAchievements) -> Result<(), String> {
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

    pub fn mark_miss(&mut self, game_id: String) -> Result<(), String> {
        self.misses.insert(game_id, now_unix());
        self.persist()
    }

    fn persist(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&CacheFile {
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

/// Parses RetroAchievements' `DateEarned`/`DateEarnedHardcore` timestamp
/// format ("YYYY-MM-DD HH:MM:SS", always UTC per the Web API docs) into a
/// Unix timestamp. Written by hand (Howard Hinnant's `days_from_civil`)
/// rather than pulling in a date/time crate for one fixed, always-UTC
/// format.
fn parse_ra_date(value: &str) -> Option<i64> {
    let (date, time) = value.split_once(' ')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next()?.parse().ok()?;

    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;

    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Splits into lowercase alphanumeric words. Used for console-name
/// matching instead of `normalized`'s boundary-free string, because
/// stripping every separator would make "NES" a raw substring of "SNES" —
/// a real false-positive `contains()` would happily match.
fn words(value: &str) -> std::collections::HashSet<String> {
    value
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word.to_ascii_lowercase())
        .collect()
}

#[derive(Debug, Deserialize)]
struct RaConsole {
    #[serde(rename = "ID")]
    id: u32,
    #[serde(rename = "Name")]
    name: String,
}

/// Finds the RetroAchievements console whose name shares the most whole
/// words with `platform`'s own name — RA's exact console names aren't part
/// of any stable public contract, so this can't require an exact string.
/// Whole-word overlap (rather than raw substring containment) specifically
/// avoids "NES" being treated as a match for "SNES" just because it's a
/// substring of it.
fn find_console_id(consoles: &[RaConsole], platform: Platform) -> Option<u32> {
    let full_words = words(platform.display_name());
    let short_words = words(platform.short_name());
    consoles
        .iter()
        .filter_map(|console| {
            let name_words = words(&console.name);
            let score = name_words.intersection(&full_words).count()
                + name_words.intersection(&short_words).count();
            (score > 0).then_some((score, console))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, console)| console.id)
}

#[derive(Debug, Deserialize)]
struct RaGameListEntry {
    #[serde(rename = "ID")]
    id: u32,
    #[serde(rename = "Title")]
    title: String,
}

/// Picks the best title match for `rom_title` the same way IGDB lookups do
/// (see `metadata::choose_result`): an exact normalized match first, then
/// either title being a prefix of the other, else the first result.
fn find_game_id(games: &[RaGameListEntry], rom_title: &str) -> Option<u32> {
    if games.is_empty() {
        return None;
    }
    let requested = normalized(rom_title);
    games
        .iter()
        .min_by_key(|game| {
            let title = normalized(&game.title);
            if title == requested {
                0
            } else if title.starts_with(&requested) || requested.starts_with(&title) {
                1
            } else {
                2
            }
        })
        .map(|game| game.id)
}

#[derive(Debug, Deserialize)]
struct RaAchievementEntry {
    #[serde(rename = "ID")]
    id: u64,
    #[serde(rename = "Title")]
    title: String,
    #[serde(rename = "Description")]
    description: String,
    #[serde(rename = "Points")]
    points: u32,
    #[serde(rename = "BadgeName")]
    badge_name: String,
    #[serde(rename = "DateEarned")]
    date_earned: Option<String>,
    #[serde(rename = "DateEarnedHardcore")]
    date_earned_hardcore: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RaGameProgressResponse {
    #[serde(rename = "Title")]
    title: String,
    #[serde(rename = "ConsoleName")]
    console_name: String,
    #[serde(rename = "ImageIcon")]
    image_icon: Option<String>,
    #[serde(rename = "Achievements")]
    achievements: HashMap<String, RaAchievementEntry>,
}

fn fetch_consoles(agent: &Agent, api_key: &str) -> Result<Vec<RaConsole>, String> {
    let url = format!("https://retroachievements.org/API/API_GetConsoleIDs.php?y={api_key}");
    agent
        .get(&url)
        .call()
        .map_err(|error| format!("RetroAchievements request failed: {error}"))?
        .body_mut()
        .read_json::<Vec<RaConsole>>()
        .map_err(|error| format!("Could not read RetroAchievements console list: {error}"))
}

fn fetch_game_list(
    agent: &Agent,
    api_key: &str,
    console_id: u32,
) -> Result<Vec<RaGameListEntry>, String> {
    let url = format!(
        "https://retroachievements.org/API/API_GetGameList.php?y={api_key}&i={console_id}&f=1"
    );
    agent
        .get(&url)
        .call()
        .map_err(|error| format!("RetroAchievements request failed: {error}"))?
        .body_mut()
        .read_json::<Vec<RaGameListEntry>>()
        .map_err(|error| format!("Could not read RetroAchievements game list: {error}"))
}

fn fetch_progress(
    agent: &Agent,
    api_key: &str,
    username: &str,
    game_id: u32,
) -> Result<GameAchievements, String> {
    let url = format!(
        "https://retroachievements.org/API/API_GetGameInfoAndUserProgress.php?y={api_key}&u={username}&g={game_id}"
    );
    let response = agent
        .get(&url)
        .call()
        .map_err(|error| format!("RetroAchievements request failed: {error}"))?
        .body_mut()
        .read_json::<RaGameProgressResponse>()
        .map_err(|error| format!("Could not read RetroAchievements progress: {error}"))?;

    let mut achievements: Vec<Achievement> = response
        .achievements
        .into_values()
        .map(|entry| Achievement {
            id: entry.id,
            title: entry.title,
            description: entry.description,
            points: entry.points,
            badge_url: format!(
                "https://media.retroachievements.org/Badge/{}.png",
                entry.badge_name
            ),
            unlocked: entry.date_earned.is_some(),
            unlocked_hardcore: entry.date_earned_hardcore.is_some(),
            unlocked_at: entry.date_earned.as_deref().and_then(parse_ra_date),
        })
        .collect();
    achievements.sort_by(|a, b| a.id.cmp(&b.id));

    let earned_count = achievements.iter().filter(|a| a.unlocked).count();
    let points_earned = achievements
        .iter()
        .filter(|a| a.unlocked)
        .map(|a| a.points)
        .sum();
    let points_total = achievements.iter().map(|a| a.points).sum();

    Ok(GameAchievements {
        game_title: response.title,
        console_name: response.console_name,
        icon_url: response
            .image_icon
            .map(|path| format!("https://media.retroachievements.org{path}")),
        earned_count,
        total_count: achievements.len(),
        points_earned,
        points_total,
        achievements,
    })
}

/// Shared by the on-demand single-game lookup and the background sweep:
/// resolves `platform`'s RA console id from `consoles`, then that
/// console's game list — cached in `game_lists` since a sweep looks up
/// many ROMs on the same handful of consoles — then the game's RA id
/// within it.
fn resolve_game_id(
    agent: &Agent,
    api_key: &str,
    consoles: &[RaConsole],
    game_lists: &mut HashMap<u32, Vec<RaGameListEntry>>,
    platform: Platform,
    rom_title: &str,
) -> Result<Option<u32>, String> {
    let Some(console_id) = find_console_id(consoles, platform) else {
        return Ok(None);
    };
    if let std::collections::hash_map::Entry::Vacant(entry) = game_lists.entry(console_id) {
        entry.insert(fetch_game_list(agent, api_key, console_id)?);
    }
    Ok(find_game_id(&game_lists[&console_id], rom_title))
}

/// Best-effort achievements lookup for one ROM. Returns `Ok(None)` (not an
/// error) whenever RetroAchievements simply doesn't have a matching
/// console or title — that's the normal case for most of a library, not a
/// failure.
pub fn fetch_for_game(
    username: &str,
    api_key: &str,
    platform: Platform,
    rom_title: &str,
) -> Result<Option<GameAchievements>, String> {
    let agent = new_agent();
    let consoles = fetch_consoles(&agent, api_key)?;
    let mut game_lists = HashMap::new();
    let Some(game_id) = resolve_game_id(
        &agent,
        api_key,
        &consoles,
        &mut game_lists,
        platform,
        rom_title,
    )?
    else {
        return Ok(None);
    };
    fetch_progress(&agent, api_key, username, game_id).map(Some)
}

/// Account-wide RetroAchievements standing — total points and site rank
/// across every game on the account, not just the ones Wervik has matched
/// locally (see `GameAchievements`, which is per-game).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaProfileSummary {
    pub username: String,
    pub avatar_url: Option<String>,
    pub rank: u32,
    pub total_points: u32,
    pub total_true_points: u32,
}

/// RA is known to mix string- and number-typed numeric fields across its
/// API surface, and which one a given field uses isn't part of any stable
/// contract — accepted as either here so a format change degrades to a
/// parsed value instead of a hard deserialize failure.
fn deserialize_flexible_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrNum {
        Str(String),
        Num(u32),
    }
    Ok(match StrOrNum::deserialize(deserializer)? {
        StrOrNum::Str(s) => s.trim().parse().unwrap_or(0),
        StrOrNum::Num(n) => n,
    })
}

#[derive(Debug, Deserialize)]
struct RaUserSummaryResponse {
    #[serde(
        rename = "Rank",
        default,
        deserialize_with = "deserialize_flexible_u32"
    )]
    rank: u32,
    #[serde(
        rename = "TotalPoints",
        default,
        deserialize_with = "deserialize_flexible_u32"
    )]
    total_points: u32,
    #[serde(
        rename = "TotalTruePoints",
        default,
        deserialize_with = "deserialize_flexible_u32"
    )]
    total_true_points: u32,
    #[serde(rename = "UserPic", default)]
    user_pic: Option<String>,
}

/// Fetches the account-wide profile summary via `API_GetUserSummary.php`
/// (`g=0&a=0` skips the recently-played-games/achievements payload Wervik
/// doesn't use, keeping the response small). Field names here aren't part
/// of any stable public contract the way the rest of this module's
/// console/title matching already assumes — parsed defensively
/// (`deserialize_flexible_u32`, every field defaulted) so an unexpected
/// shape degrades to zeroed stats rather than a hard error.
pub fn fetch_profile_summary(username: &str, api_key: &str) -> Result<RaProfileSummary, String> {
    let agent = new_agent();
    let url = format!(
        "https://retroachievements.org/API/API_GetUserSummary.php?y={api_key}&u={username}&g=0&a=0"
    );
    let response: RaUserSummaryResponse = agent
        .get(&url)
        .call()
        .map_err(|error| format!("RetroAchievements request failed: {error}"))?
        .body_mut()
        .read_json()
        .map_err(|error| format!("Could not read the RetroAchievements profile: {error}"))?;

    Ok(RaProfileSummary {
        username: username.to_string(),
        avatar_url: response
            .user_pic
            .map(|path| format!("https://media.retroachievements.org{path}")),
        rank: response.rank,
        total_points: response.total_points,
        total_true_points: response.total_true_points,
    })
}

#[derive(Debug, Deserialize)]
struct RaLoginResponse {
    #[serde(rename = "Success")]
    success: bool,
    #[serde(rename = "Error")]
    error: Option<String>,
    #[serde(rename = "Token")]
    token: Option<String>,
}

/// Trades a RetroAchievements username/password for a durable session
/// token, via the same `r=login2` request every real client (RetroArch,
/// PCSX2, Dolphin...) sends — verified against the open-source `rcheevos`
/// client library's own request builder
/// (`rc_api_init_login_request_hosted` in `rcheevos/src/rapi/rc_api_user.c`)
/// rather than assumed. The password is only ever used for this one
/// request — the caller persists the returned token instead, wherever the
/// target emulator's own config expects it (its own client would replace a
/// stored password with this same token on its next successful login too).
pub fn login(username: &str, password: &str) -> Result<String, String> {
    let agent = new_agent();
    let mut response = agent
        .post("https://retroachievements.org/dorequest.php")
        .send_form([("r", "login2"), ("u", username), ("p", password)])
        .map_err(|error| format!("RetroAchievements request failed: {error}"))?;
    let response: RaLoginResponse = response
        .body_mut()
        .read_json()
        .map_err(|error| format!("Could not read RetroAchievements' response: {error}"))?;

    if !response.success {
        return Err(response
            .error
            .unwrap_or_else(|| "RetroAchievements rejected this login.".to_string()));
    }
    response
        .token
        .ok_or_else(|| "RetroAchievements did not return a session token.".to_string())
}

/// One ROM to look up in the background sweep below.
#[derive(Debug, Clone)]
pub struct AchievementQuery {
    pub id: String,
    pub name: String,
    pub platform: Platform,
}

fn should_skip(app_handle: &AppHandle, game_id: &str) -> bool {
    app_handle
        .try_state::<AppStateHandle>()
        .map(|state| {
            let state = state.lock().unwrap_or_else(|e| e.into_inner());
            state.retro_achievements_cache.get_fresh(game_id).is_some()
                || state.retro_achievements_cache.recent_miss(game_id)
        })
        .unwrap_or(false)
}

/// Kept well below what a single interactive lookup would use, since this
/// runs unattended over a whole library rather than one game a user is
/// actively waiting on.
const SWEEP_REQUEST_DELAY: Duration = Duration::from_millis(400);

/// Populates the achievements cache for every ROM in the background, so
/// the Library grid can show a completion badge without the user having
/// opened each game's page first (see `commands::library_snapshot`). Runs
/// once per rescan or RetroAchievements credentials change — same trigger
/// pattern as `metadata::spawn_fetch` — and skips anything already fresh
/// (or a recent miss) in the cache.
pub fn spawn_fetch(
    app_handle: AppHandle,
    games: Vec<AchievementQuery>,
    username: String,
    api_key: String,
    running: Arc<AtomicBool>,
) {
    if username.trim().is_empty() || api_key.trim().is_empty() {
        return;
    }
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    std::thread::spawn(move || {
        let agent = new_agent();
        let consoles = match fetch_consoles(&agent, &api_key) {
            Ok(consoles) => consoles,
            Err(error) => {
                eprintln!("RetroAchievements worker could not list consoles: {error}");
                running.store(false, Ordering::SeqCst);
                return;
            }
        };
        let mut game_lists: HashMap<u32, Vec<RaGameListEntry>> = HashMap::new();
        let mut updated_any = false;

        for query in games
            .into_iter()
            .filter(|query| !should_skip(&app_handle, &query.id))
        {
            let outcome = resolve_game_id(
                &agent,
                &api_key,
                &consoles,
                &mut game_lists,
                query.platform,
                &query.name,
            )
            .and_then(|found| match found {
                Some(game_id) => fetch_progress(&agent, &api_key, &username, game_id).map(Some),
                None => Ok(None),
            });
            match outcome {
                Ok(Some(achievements)) => {
                    let mut updated = false;
                    if let Some(state) = app_handle.try_state::<AppStateHandle>() {
                        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                        if let Err(error) = state
                            .retro_achievements_cache
                            .put(query.id.clone(), achievements)
                        {
                            eprintln!("Could not save achievements for {}: {error}", query.name);
                        } else {
                            let _ = app_handle.emit("retro-achievements-updated", &query.id);
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
                        if let Err(error) =
                            state.retro_achievements_cache.mark_miss(query.id.clone())
                        {
                            eprintln!(
                                "Could not save achievements miss for {}: {error}",
                                query.name
                            );
                        }
                    }
                }
                Err(error) => {
                    eprintln!("RetroAchievements fetch failed for {}: {error}", query.name)
                }
            }
            std::thread::sleep(SWEEP_REQUEST_DELAY);
        }
        running.store(false, Ordering::SeqCst);
        if updated_any {
            crate::commands::schedule_wervik_achievements_sync(&app_handle);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console(id: u32, name: &str) -> RaConsole {
        RaConsole {
            id,
            name: name.to_string(),
        }
    }

    #[test]
    fn matches_console_names_loosely() {
        let consoles = vec![
            console(1, "Genesis/Mega Drive"),
            console(3, "SNES/Super Famicom"),
            console(7, "NES/Famicom"),
        ];
        assert_eq!(find_console_id(&consoles, Platform::Genesis), Some(1));
        assert_eq!(find_console_id(&consoles, Platform::Snes), Some(3));
        assert_eq!(find_console_id(&consoles, Platform::Nes), Some(7));
    }

    #[test]
    fn returns_none_for_unmatched_console() {
        let consoles = vec![console(1, "Genesis/Mega Drive")];
        assert_eq!(find_console_id(&consoles, Platform::Nintendo64), None);
    }

    /// Regression test: an earlier version matched by raw substring
    /// containment, so "NES" (a literal substring of "SNES") resolved to
    /// the SNES console instead of NES.
    #[test]
    fn does_not_confuse_nes_with_snes() {
        let consoles = vec![console(3, "SNES/Super Famicom"), console(7, "NES/Famicom")];
        assert_eq!(find_console_id(&consoles, Platform::Nes), Some(7));
    }

    fn game(id: u32, title: &str) -> RaGameListEntry {
        RaGameListEntry {
            id,
            title: title.to_string(),
        }
    }

    #[test]
    fn prefers_exact_title_match() {
        let games = vec![
            game(1, "Super Mario World 2: Yoshi's Island"),
            game(2, "Super Mario World"),
        ];
        assert_eq!(find_game_id(&games, "Super Mario World"), Some(2));
    }

    #[test]
    fn parses_ra_date_earned_format() {
        assert_eq!(parse_ra_date("2023-01-15 20:30:45"), Some(1673814645));
        // The epoch itself, as a boundary check.
        assert_eq!(parse_ra_date("1970-01-01 00:00:00"), Some(0));
    }

    #[test]
    fn rejects_a_malformed_ra_date() {
        assert_eq!(parse_ra_date("not-a-date"), None);
        assert_eq!(parse_ra_date(""), None);
    }
}
