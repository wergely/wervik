//! IGDB-backed game metadata with a small local JSON cache.
//!
//! The lookup intentionally lives in Rust instead of the React layer. IGDB
//! does not allow direct browser requests, and keeping the request here also
//! means the UI can use the same path later when credentials move to the
//! Wervik web service.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use ureq::Agent;

use crate::commands::AppStateHandle;
use crate::retro::platform::Platform;
use crate::storage::MetadataProvider;

const MAX_SCREENSHOTS: usize = 4;
const IGDB_REQUEST_DELAY: Duration = Duration::from_millis(300);
const WERVIK_WEB_URL: &str = "https://wervik.wergely.com";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameMetadata {
    #[serde(default)]
    pub name: Option<String>,
    pub igdb_id: u64,
    pub igdb_url: Option<String>,
    pub summary: Option<String>,
    pub first_release_date: Option<i64>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub genres: Vec<String>,
    pub platforms: Vec<String>,
    pub rating: Option<f64>,
    pub rating_count: Option<u64>,
    pub cover_path: Option<String>,
    pub hero_path: Option<String>,
    pub screenshot_paths: Vec<String>,
    pub fetched_at: i64,
}

#[derive(Debug, Clone)]
pub struct MetadataQuery {
    pub id: String,
    pub name: String,
    /// The ROM's known platform, when this query is for a ROM — `None` for
    /// Steam/Xbox/Battle.net/EA titles, which only ever run on one platform
    /// (the current PC) so there's nothing to disambiguate. Lets
    /// `choose_result` reject an IGDB match on the wrong platform (e.g. a
    /// SNES game vs. a same-titled remake on another system).
    pub platform: Option<Platform>,
}

#[derive(Default)]
pub struct MetadataProgressState {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataProgress {
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct MetadataFile {
    #[serde(default)]
    entries: HashMap<String, GameMetadata>,
    #[serde(default)]
    misses: HashMap<String, i64>,
}

#[derive(Debug)]
pub struct MetadataCache {
    path: PathBuf,
    entries: HashMap<String, GameMetadata>,
    misses: HashMap<String, i64>,
}

impl MetadataCache {
    pub fn load(path: &Path) -> Self {
        let (entries, misses) = std::fs::read_to_string(path)
            .ok()
            .and_then(|contents| {
                serde_json::from_str::<MetadataFile>(&contents)
                    .map(|file| (file.entries, file.misses))
                    .or_else(|_| {
                        // The first version of the cache was a bare map. Keep
                        // those entries readable after adding miss backoff.
                        serde_json::from_str::<HashMap<String, GameMetadata>>(&contents)
                            .map(|entries| (entries, HashMap::new()))
                    })
                    .ok()
            })
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            entries,
            misses,
        }
    }

    pub fn get(&self, game_id: &str) -> Option<GameMetadata> {
        self.entries.get(game_id).cloned()
    }

    /// A game is considered processed after either a metadata result or a
    /// recorded miss. Normal library scans never retry processed games;
    /// callers must remove the entry for an explicit refresh.
    pub fn is_processed(&self, game_id: &str) -> bool {
        self.entries.contains_key(game_id) || self.misses.contains_key(game_id)
    }

    /// Every cached entry — used to build the allowlist of image paths
    /// `commands::read_image` may serve (see its doc comment).
    pub fn values(&self) -> impl Iterator<Item = &GameMetadata> {
        self.entries.values()
    }

    /// Returns the stable IGDB identity for every locally enriched game.
    /// Callers use this to reconcile source-specific ROM ids without making
    /// another network request.
    pub fn igdb_ids(&self) -> HashMap<String, u64> {
        self.entries
            .iter()
            .map(|(game_id, metadata)| (game_id.clone(), metadata.igdb_id))
            .collect()
    }

    pub fn put(&mut self, game_id: String, metadata: GameMetadata) -> Result<(), String> {
        self.misses.remove(&game_id);
        self.entries.insert(game_id, metadata);
        self.persist()
    }

    pub fn mark_miss(&mut self, game_id: String) -> Result<(), String> {
        self.misses.insert(game_id, now_unix());
        self.persist()
    }

    /// Drops every cached entry and miss so the next metadata fetch starts
    /// clean — used by "Refresh metadata" in Settings so a game that was
    /// mismatched under old (platform-blind) matching logic gets re-looked-up.
    pub fn clear(&mut self) -> Result<(), String> {
        self.entries.clear();
        self.misses.clear();
        self.persist()
    }

    /// Same as `clear`, but for a single game — the per-game "Refresh
    /// metadata" action on the game details page.
    pub fn remove(&mut self, game_id: &str) -> Result<(), String> {
        self.entries.remove(game_id);
        self.misses.remove(game_id);
        self.persist()
    }

    /// Carries a processed result over when a ROM's source id changes during
    /// a folder move. The metadata itself remains untouched; only its local
    /// cache key changes.
    pub fn rekey(&mut self, old_id: &str, new_id: &str) -> Result<bool, String> {
        if old_id == new_id || self.is_processed(new_id) {
            return Ok(false);
        }
        let mut changed = false;
        if let Some(metadata) = self.entries.remove(old_id) {
            self.entries.insert(new_id.to_string(), metadata);
            changed = true;
        }
        if let Some(fetched_at) = self.misses.remove(old_id) {
            self.misses.insert(new_id.to_string(), fetched_at);
            changed = true;
        }
        if changed {
            self.persist()?;
        }
        Ok(changed)
    }

    fn persist(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&MetadataFile {
            entries: self.entries.clone(),
            misses: self.misses.clone(),
        })
        .map_err(|e| e.to_string())?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&self.path, json).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct TwitchToken {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct IgdbGame {
    id: u64,
    name: Option<String>,
    slug: Option<String>,
    url: Option<String>,
    summary: Option<String>,
    first_release_date: Option<i64>,
    cover: Option<IgdbImage>,
    screenshots: Option<Vec<IgdbImage>>,
    genres: Option<Vec<IgdbNamedObject>>,
    platforms: Option<Vec<IgdbNamedObject>>,
    release_dates: Option<Vec<IgdbReleaseDate>>,
    involved_companies: Option<Vec<IgdbCompany>>,
    rating: Option<f64>,
    rating_count: Option<u64>,
    aggregated_rating: Option<f64>,
    aggregated_rating_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct IgdbReleaseDate {
    /// IGDB's legacy release-region enum: North America is 2 and worldwide
    /// is 8. It remains available on the games response and is enough for
    /// choosing the preferred regional edition without another API request.
    region: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct IgdbImage {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IgdbNamedObject {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IgdbCompany {
    developer: Option<bool>,
    publisher: Option<bool>,
    company: Option<IgdbNamedObject>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WervikMetadataRequest<'a> {
    game_name: &'a str,
    platform_key: Option<&'a str>,
    force: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WervikMetadataResponse {
    metadata: Option<WervikGameMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WervikGameMetadata {
    name: Option<String>,
    igdb_id: u64,
    igdb_url: Option<String>,
    summary: Option<String>,
    first_release_date: Option<i64>,
    developers: Vec<String>,
    publishers: Vec<String>,
    genres: Vec<String>,
    platforms: Vec<String>,
    rating: Option<f64>,
    rating_count: Option<u64>,
    cover_url: Option<String>,
    screenshot_urls: Vec<String>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Prefers the credentials entered in Settings when both fields are set;
/// otherwise falls back to the `WERVIK_IGDB_CLIENT_ID`/
/// `WERVIK_IGDB_CLIENT_SECRET` environment variables documented for
/// developers running from source.
fn credentials(stored_client_id: &str, stored_client_secret: &str) -> Option<(String, String)> {
    if !stored_client_id.trim().is_empty() && !stored_client_secret.trim().is_empty() {
        return Some((
            stored_client_id.to_string(),
            stored_client_secret.to_string(),
        ));
    }
    let client_id = std::env::var("WERVIK_IGDB_CLIENT_ID").ok()?;
    let client_secret = std::env::var("WERVIK_IGDB_CLIENT_SECRET").ok()?;
    if client_id.trim().is_empty() || client_secret.trim().is_empty() {
        return None;
    }
    Some((client_id, client_secret))
}

/// Whether IGDB enrichment is available for this process. Retro artwork uses
/// this to decide whether libretro-thumbnails should remain as a fallback or
/// whether IGDB is the single primary cover source.
pub fn is_configured(stored_client_id: &str, stored_client_secret: &str) -> bool {
    credentials(stored_client_id, stored_client_secret).is_some()
}

fn new_agent() -> Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .build()
        .into()
}

fn twitch_token(agent: &Agent, client_id: &str, client_secret: &str) -> Result<String, String> {
    let mut response = agent
        .post("https://id.twitch.tv/oauth2/token")
        .send_form([
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "client_credentials"),
        ])
        .map_err(|e| format!("IGDB authentication failed: {e}"))?;
    response
        .body_mut()
        .read_json::<TwitchToken>()
        .map(|token| token.access_token)
        .map_err(|e| format!("Could not read the IGDB access token: {e}"))
}

fn igdb_query(name: &str) -> String {
    let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "search \"{escaped}\"; fields id,name,slug,url,summary,first_release_date,cover.url,screenshots.url,genres.name,platforms.name,release_dates.region,involved_companies.developer,involved_companies.publisher,involved_companies.company.name,rating,rating_count,aggregated_rating,aggregated_rating_count; limit 20;"
    )
}

fn search_names(name: &str) -> Vec<String> {
    let original = name.trim();
    let mut names = vec![original.to_string()];
    let lower = original.to_ascii_lowercase();
    for suffix in [
        " - windows",
        " – windows",
        " — windows",
        " (windows)",
        " [windows]",
    ] {
        if lower.ends_with(suffix) {
            let trimmed = original[..original.len() - suffix.len()].trim();
            if !trimmed.is_empty() && trimmed != original {
                names.push(trimmed.to_string());
            }
            break;
        }
    }
    names
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 0 when one of `result_platforms` names is (loosely) this platform, 2 when
/// IGDB reported platforms and none of them are it, 1 when IGDB reported no
/// platforms at all — kept worse than an actual match but no worse than an
/// unconfirmed one, since a missing field isn't evidence of a mismatch.
fn platform_rank(result_platforms: &[IgdbNamedObject], expected: Option<Platform>) -> u8 {
    let Some(expected) = expected else { return 1 };
    if result_platforms.is_empty() {
        return 1;
    }
    let known_names = expected.igdb_names();
    let matches = result_platforms.iter().any(|platform| {
        let Some(name) = platform.name.as_deref() else {
            return false;
        };
        let normalized_name = normalized(name);
        known_names
            .iter()
            .any(|known| normalized(known) == normalized_name)
    });
    if matches {
        0
    } else {
        2
    }
}

/// IGDB's release-region enum uses 2 for North America and 8 for worldwide.
/// Wervik prefers the North American release as the default edition; missing
/// region data remains preferable to a title released only elsewhere because
/// absence is not evidence that the match is wrong.
fn region_rank(game: &IgdbGame) -> u8 {
    let regions: Vec<u8> = game
        .release_dates
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|release| release.region)
        .collect();
    if regions.contains(&2) {
        0
    } else if regions.contains(&8) {
        1
    } else if regions.is_empty() {
        2
    } else {
        3
    }
}

fn choose_result(
    mut results: Vec<IgdbGame>,
    requested_name: &str,
    expected_platform: Option<Platform>,
) -> Option<IgdbGame> {
    if results.is_empty() {
        return None;
    }
    let requested = normalized(requested_name);
    results.sort_by(|left, right| {
        let left_title = left.name.as_deref().map(normalized).unwrap_or_default();
        let right_title = right.name.as_deref().map(normalized).unwrap_or_default();
        let title_rank = |title: &str| {
            if title == requested {
                0
            } else if title.starts_with(&requested) || requested.starts_with(title) {
                1
            } else if !requested.is_empty() && title.contains(&requested) {
                2
            } else {
                3
            }
        };
        let left_key = (
            platform_rank(left.platforms.as_deref().unwrap_or(&[]), expected_platform),
            region_rank(left),
            title_rank(&left_title),
        );
        let right_key = (
            platform_rank(right.platforms.as_deref().unwrap_or(&[]), expected_platform),
            region_rank(right),
            title_rank(&right_title),
        );
        let key_order = left_key.cmp(&right_key);
        if key_order != std::cmp::Ordering::Equal {
            return key_order;
        }

        // When the API returns a base game and a numbered sequel with the
        // same platform/title rank, prefer the base title. The normalized
        // prefix check keeps this tie-breaker narrow, so generic searches
        // still retain the provider's ordering.
        if !left_title.is_empty()
            && !right_title.is_empty()
            && (left_title.starts_with(&right_title) || right_title.starts_with(&left_title))
        {
            return left_title.len().cmp(&right_title.len());
        }
        std::cmp::Ordering::Equal
    });
    results.into_iter().next()
}

fn image_url(url: Option<String>, size: &str) -> Option<String> {
    let url = url?;
    let url = if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url
    };
    Some(url.replace("/t_thumb/", &format!("/{size}/")))
}

fn safe_cache_key(game_id: &str) -> String {
    let key: String = game_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if key.is_empty() {
        "game".to_string()
    } else {
        key
    }
}

fn valid_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xff, 0xd8, 0xff])
        || bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47])
        || bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")
}

fn download_image(
    agent: &Agent,
    url: &str,
    path: &Path,
    bearer_token: Option<&str>,
) -> Option<String> {
    if path.is_file() {
        return Some(path.to_string_lossy().to_string());
    }
    let mut request = agent.get(url);
    if let Some(token) = bearer_token {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let mut response = request.call().ok()?;
    let bytes = response.body_mut().read_to_vec().ok()?;
    if !valid_image(&bytes) {
        return None;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    std::fs::write(path, bytes).ok()?;
    Some(path.to_string_lossy().to_string())
}

fn names(objects: Option<Vec<IgdbNamedObject>>) -> Vec<String> {
    objects
        .unwrap_or_default()
        .into_iter()
        .filter_map(|object| object.name)
        .filter(|name| !name.trim().is_empty())
        .collect()
}

fn should_skip(app_handle: &AppHandle, game_id: &str) -> bool {
    app_handle
        .try_state::<AppStateHandle>()
        .map(|state| {
            let state = state.lock().unwrap_or_else(|e| e.into_inner());
            state.metadata_cache.is_processed(game_id)
        })
        .unwrap_or(false)
}

/// Fetches one game's metadata. Missing credentials are a normal disabled
/// state and return `Ok(None)` so the library remains fully usable without
/// online enrichment.
pub fn fetch_game(
    app_data_dir: &Path,
    game_id: &str,
    game_name: &str,
    platform: Option<Platform>,
    provider: MetadataProvider,
    stored_client_id: &str,
    stored_client_secret: &str,
    wervik_access_token: Option<&str>,
    force: bool,
) -> Result<Option<GameMetadata>, String> {
    let agent = new_agent();
    match provider {
        MetadataProvider::Igdb => {
            let Some((client_id, client_secret)) =
                credentials(stored_client_id, stored_client_secret)
            else {
                return Ok(None);
            };
            let access_token = twitch_token(&agent, &client_id, &client_secret)?;
            fetch_game_with_token(
                &agent,
                &client_id,
                &access_token,
                app_data_dir,
                game_id,
                game_name,
                platform,
            )
        }
        MetadataProvider::Wervik => {
            let Some(access_token) = wervik_access_token.filter(|token| !token.trim().is_empty())
            else {
                return Ok(None);
            };
            fetch_game_from_wervik(
                &agent,
                access_token,
                app_data_dir,
                game_id,
                game_name,
                platform,
                force,
            )
        }
    }
}

fn fetch_game_from_wervik(
    agent: &Agent,
    access_token: &str,
    app_data_dir: &Path,
    game_id: &str,
    game_name: &str,
    platform: Option<Platform>,
    force: bool,
) -> Result<Option<GameMetadata>, String> {
    let configured_base =
        std::env::var("WERVIK_WEB_URL").unwrap_or_else(|_| WERVIK_WEB_URL.to_string());
    let mut url = url::Url::parse(&configured_base)
        .map_err(|error| format!("Invalid Wervik web URL: {error}"))?;
    url.set_path("/api/metadata/igdb");

    let request = WervikMetadataRequest {
        game_name,
        platform_key: platform.map(Platform::key),
        force,
    };
    let mut response = agent
        .post(url.as_str())
        .header("Authorization", &format!("Bearer {access_token}"))
        .send_json(&request)
        .map_err(|error| format!("Wervik metadata lookup failed: {error}"))?;
    let response = response
        .body_mut()
        .read_json::<WervikMetadataResponse>()
        .map_err(|error| format!("Could not read Wervik metadata response: {error}"))?;
    let Some(response) = response.metadata else {
        return Ok(None);
    };

    let artwork_dir = app_data_dir.join("igdb-artwork");
    let key = safe_cache_key(game_id);
    let cover_url = response.cover_url.clone();
    let cover_target = artwork_dir.join(format!("{key}-cover.jpg"));
    let screenshot_targets: Vec<(String, PathBuf)> = response
        .screenshot_urls
        .iter()
        .take(MAX_SCREENSHOTS)
        .enumerate()
        .map(|(index, url)| {
            (
                url.clone(),
                artwork_dir.join(format!("{key}-screenshot-{index}.jpg")),
            )
        })
        .collect();
    let (cover_path, screenshot_paths) = std::thread::scope(|scope| {
        let cover_handle = scope.spawn(|| {
            let image_agent = new_agent();
            cover_url.as_deref().and_then(|url| {
                download_image(&image_agent, url, &cover_target, Some(access_token))
            })
        });
        let screenshot_handles: Vec<_> = screenshot_targets
            .iter()
            .map(|(url, target)| {
                scope.spawn(|| {
                    let image_agent = new_agent();
                    download_image(&image_agent, url, target, Some(access_token))
                })
            })
            .collect();
        let cover_path = cover_handle.join().unwrap();
        let screenshot_paths: Vec<String> = screenshot_handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap())
            .collect();
        (cover_path, screenshot_paths)
    });
    let hero_path = screenshot_paths.first().cloned();

    Ok(Some(GameMetadata {
        name: response.name,
        igdb_id: response.igdb_id,
        igdb_url: response.igdb_url,
        summary: response.summary,
        first_release_date: response.first_release_date,
        developers: response.developers,
        publishers: response.publishers,
        genres: response.genres,
        platforms: response.platforms,
        rating: response.rating,
        rating_count: response.rating_count,
        cover_path,
        hero_path,
        screenshot_paths,
        fetched_at: now_unix(),
    }))
}

fn fetch_game_with_token(
    agent: &Agent,
    client_id: &str,
    access_token: &str,
    app_data_dir: &Path,
    game_id: &str,
    game_name: &str,
    platform: Option<Platform>,
) -> Result<Option<GameMetadata>, String> {
    let mut matched_game = None;
    let search_names = search_names(game_name);
    for (index, search_name) in search_names.iter().enumerate() {
        let query = igdb_query(&search_name);
        let mut response = agent
            .post("https://api.igdb.com/v4/games")
            .header("Client-ID", client_id)
            .header("Authorization", &format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .send(query)
            .map_err(|e| format!("IGDB lookup failed: {e}"))?;
        let results = response
            .body_mut()
            .read_json::<Vec<IgdbGame>>()
            .map_err(|e| format!("Could not read IGDB game data: {e}"))?;
        if let Some(game) = choose_result(results, &search_name, platform) {
            matched_game = Some(game);
            break;
        }
        if index + 1 < search_names.len() {
            std::thread::sleep(IGDB_REQUEST_DELAY);
        }
    }
    let Some(game) = matched_game else {
        return Ok(None);
    };

    let artwork_dir = app_data_dir.join("igdb-artwork");
    let key = safe_cache_key(game_id);
    let cover_url = image_url(game.cover.and_then(|image| image.url), "t_cover_big");
    let screenshot_urls: Vec<String> = game
        .screenshots
        .unwrap_or_default()
        .into_iter()
        .filter_map(|image| image_url(image.url, "t_1080p"))
        .collect();

    let cover_path = cover_url.as_deref().and_then(|url| {
        download_image(
            &agent,
            url,
            &artwork_dir.join(format!("{key}-cover.jpg")),
            None,
        )
    });
    let screenshot_paths: Vec<String> = screenshot_urls
        .iter()
        .take(MAX_SCREENSHOTS)
        .enumerate()
        .filter_map(|(index, url)| {
            download_image(
                &agent,
                url,
                &artwork_dir.join(format!("{key}-screenshot-{index}.jpg")),
                None,
            )
        })
        .collect();
    let hero_path = screenshot_paths.first().cloned();

    let mut developers = Vec::new();
    let mut publishers = Vec::new();
    for entry in game.involved_companies.unwrap_or_default() {
        let Some(company) = entry.company.and_then(|company| company.name) else {
            continue;
        };
        if entry.developer.unwrap_or(false) && !developers.contains(&company) {
            developers.push(company.clone());
        }
        if entry.publisher.unwrap_or(false) && !publishers.contains(&company) {
            publishers.push(company);
        }
    }

    let rating = game.aggregated_rating.or(game.rating);
    let rating_count = game.aggregated_rating_count.or(game.rating_count);
    let igdb_url = game.url.or_else(|| {
        game.slug
            .map(|slug| format!("https://www.igdb.com/games/{slug}"))
    });
    Ok(Some(GameMetadata {
        name: game.name,
        igdb_id: game.id,
        igdb_url,
        summary: game.summary,
        first_release_date: game.first_release_date,
        developers,
        publishers,
        genres: names(game.genres),
        platforms: names(game.platforms),
        rating,
        rating_count,
        cover_path,
        hero_path,
        screenshot_paths,
        fetched_at: now_unix(),
    }))
}

/// Deletes every downloaded cover/screenshot alongside a `MetadataCache::clear`
/// call — otherwise `download_image`'s "file already on disk" shortcut would
/// keep serving art fetched for an old, possibly wrong, IGDB match instead of
/// redownloading it for whatever match "Refresh metadata" finds this time.
pub fn clear_artwork(app_data_dir: &Path) -> Result<(), String> {
    let artwork_dir = app_data_dir.join("igdb-artwork");
    match std::fs::remove_dir_all(&artwork_dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Same as `clear_artwork`, but for a single game's own files — the per-game
/// "Refresh metadata" action, which can't just wipe the whole shared
/// artwork directory.
pub fn clear_artwork_for(app_data_dir: &Path, game_id: &str) -> Result<(), String> {
    let artwork_dir = app_data_dir.join("igdb-artwork");
    let key = safe_cache_key(game_id);
    let mut paths = vec![artwork_dir.join(format!("{key}-cover.jpg"))];
    for index in 0..MAX_SCREENSHOTS {
        paths.push(artwork_dir.join(format!("{key}-screenshot-{index}.jpg")));
    }
    for path in paths {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

/// Updates every installed title in the background. Positive results and
/// misses are persisted, while transport/API errors are left retryable for a
/// later run. The delay keeps the worker below IGDB's public request limit and
/// prevents a large library from making startup or the UI feel busy.
pub fn spawn_fetch(
    app_handle: AppHandle,
    app_data_dir: PathBuf,
    games: Vec<MetadataQuery>,
    running: Arc<AtomicBool>,
    progress: Arc<MetadataProgressState>,
    generation: Arc<AtomicUsize>,
    generation_id: usize,
    provider: MetadataProvider,
    stored_client_id: String,
    stored_client_secret: String,
    wervik_access_token: Option<String>,
    force: bool,
) {
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    std::thread::spawn(move || {
        let igdb_credentials = match provider {
            MetadataProvider::Igdb => credentials(&stored_client_id, &stored_client_secret),
            MetadataProvider::Wervik => None,
        };
        if (provider == MetadataProvider::Igdb && igdb_credentials.is_none())
            || (provider == MetadataProvider::Wervik
                && wervik_access_token
                    .as_deref()
                    .map_or(true, |token| token.trim().is_empty()))
        {
            progress.done.store(0, Ordering::SeqCst);
            progress.total.store(0, Ordering::SeqCst);
            finish_fetch(&running, &generation, generation_id);
            return;
        }
        let agent = new_agent();
        let access_token = match igdb_credentials.as_ref() {
            Some((client_id, client_secret)) => {
                match twitch_token(&agent, client_id, client_secret) {
                    Ok(token) => Some(token),
                    Err(error) => {
                        eprintln!("IGDB metadata worker authentication failed: {error}");
                        finish_fetch(&running, &generation, generation_id);
                        return;
                    }
                }
            }
            None => None,
        };

        let pending: Vec<MetadataQuery> = games
            .into_iter()
            .filter(|game| force || !should_skip(&app_handle, &game.id))
            .collect();
        let total = pending.len();
        progress.done.store(0, Ordering::SeqCst);
        progress.total.store(total, Ordering::SeqCst);
        let _ = app_handle.emit("metadata-progress", MetadataProgress { done: 0, total });
        let mut updated_any = false;

        for game in pending {
            if generation.load(Ordering::SeqCst) != generation_id {
                break;
            }

            let _request_guard = loop {
                if let Some(request_guard) =
                    crate::commands::start_metadata_request(&app_handle, &game.id, generation_id)
                {
                    break Some(request_guard);
                }
                // A request from this same generation already owns the game.
                // Its result will be the authoritative one, so this sweep can
                // leave it alone. A request from an older generation must
                // finish first, otherwise its artwork could be written after
                // a refresh cleared the files and be mistaken for the new
                // result.
                if crate::commands::metadata_request_generation(&app_handle, &game.id)
                    == Some(generation_id)
                {
                    break None;
                }
                let (_, pending) =
                    crate::commands::wait_for_metadata_request(&app_handle, &game.id);
                if generation.load(Ordering::SeqCst) != generation_id || pending {
                    break None;
                }
            };
            let Some(_request_guard) = _request_guard else {
                let done = progress.done.fetch_add(1, Ordering::SeqCst) + 1;
                let _ = app_handle.emit("metadata-progress", MetadataProgress { done, total });
                continue;
            };

            let result = match provider {
                MetadataProvider::Igdb => {
                    let (client_id, _) = igdb_credentials
                        .as_ref()
                        .expect("validated IGDB credentials");
                    fetch_game_with_token(
                        &agent,
                        client_id,
                        access_token
                            .as_deref()
                            .expect("validated IGDB access token"),
                        &app_data_dir,
                        &game.id,
                        &game.name,
                        game.platform,
                    )
                }
                MetadataProvider::Wervik => fetch_game_from_wervik(
                    &agent,
                    wervik_access_token
                        .as_deref()
                        .expect("validated Wervik access token"),
                    &app_data_dir,
                    &game.id,
                    &game.name,
                    game.platform,
                    force,
                ),
            };
            if generation.load(Ordering::SeqCst) != generation_id {
                break;
            }
            match result {
                Ok(Some(metadata)) => {
                    let mut updated = false;
                    if let Some(state) = app_handle.try_state::<AppStateHandle>() {
                        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                        // The generation can change while this worker waits
                        // for the state lock (for example, when the user
                        // starts a per-game refresh). Check again while the
                        // lock is held so an older result cannot overwrite
                        // the newer manual result.
                        if generation.load(Ordering::SeqCst) != generation_id {
                            break;
                        }
                        let image_paths = [metadata.cover_path.clone(), metadata.hero_path.clone()]
                            .into_iter()
                            .flatten()
                            .chain(metadata.screenshot_paths.iter().cloned())
                            .collect::<Vec<_>>();
                        if let Err(error) = state.metadata_cache.put(game.id.clone(), metadata) {
                            eprintln!("Could not save metadata for {}: {error}", game.name);
                        } else {
                            state.known_image_paths.extend(image_paths);
                            state.dedupe_rom_games_by_metadata();
                            let _ = app_handle.emit("metadata-updated", &game.id);
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
                        if generation.load(Ordering::SeqCst) != generation_id {
                            break;
                        }
                        if let Err(error) = state.metadata_cache.mark_miss(game.id.clone()) {
                            eprintln!("Could not save metadata miss for {}: {error}", game.name);
                        }
                    }
                }
                Err(error) => eprintln!("IGDB metadata fetch failed for {}: {error}", game.name),
            }
            let done = progress.done.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = app_handle.emit("metadata-progress", MetadataProgress { done, total });
            if generation.load(Ordering::SeqCst) != generation_id {
                break;
            }
            std::thread::sleep(IGDB_REQUEST_DELAY);
        }
        let completed = generation.load(Ordering::SeqCst) == generation_id;
        finish_fetch(&running, &generation, generation_id);
        if completed && updated_any {
            crate::commands::schedule_wervik_library_sync(&app_handle);
        }
    });
}

fn finish_fetch(running: &AtomicBool, generation: &AtomicUsize, generation_id: usize) {
    if generation.load(Ordering::SeqCst) == generation_id {
        running.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_titles_ignore_punctuation_and_case() {
        assert_eq!(
            normalized("The Witcher 3: Wild Hunt"),
            "thewitcher3wildhunt"
        );
        assert_eq!(normalized("the witcher 3 wild hunt"), "thewitcher3wildhunt");
    }

    #[test]
    fn search_names_falls_back_from_windows_suffix() {
        assert_eq!(
            search_names("A Plague Tale: Requiem - Windows"),
            vec!["A Plague Tale: Requiem - Windows", "A Plague Tale: Requiem"]
        );
    }

    /// The real bug this guards against: IGDB's own search ranked the
    /// Switch "Star Fox" ahead of the SNES original for the query "Star
    /// Fox", and with no platform disambiguation Wervik just took whichever
    /// came first.
    #[test]
    fn choose_result_prefers_the_result_matching_the_roms_platform() {
        let switch_star_fox = IgdbGame {
            id: 1,
            name: Some("Star Fox".to_string()),
            slug: None,
            url: None,
            summary: None,
            first_release_date: None,
            cover: None,
            screenshots: None,
            genres: None,
            platforms: Some(vec![IgdbNamedObject {
                name: Some("Nintendo Switch".to_string()),
            }]),
            release_dates: None,
            involved_companies: None,
            rating: None,
            rating_count: None,
            aggregated_rating: None,
            aggregated_rating_count: None,
        };
        let snes_star_fox = IgdbGame {
            platforms: Some(vec![IgdbNamedObject {
                name: Some("Super Nintendo Entertainment System (SNES)".to_string()),
            }]),
            ..IgdbGame {
                id: 2,
                name: Some("Star Fox".to_string()),
                slug: None,
                url: None,
                summary: None,
                first_release_date: None,
                cover: None,
                screenshots: None,
                genres: None,
                platforms: None,
                release_dates: None,
                involved_companies: None,
                rating: None,
                rating_count: None,
                aggregated_rating: None,
                aggregated_rating_count: None,
            }
        };

        // IGDB returned the Switch game first, ahead of the SNES original.
        let results = vec![switch_star_fox, snes_star_fox];
        let chosen = choose_result(results, "Star Fox", Some(Platform::Snes)).unwrap();
        assert_eq!(
            chosen.id, 2,
            "should have picked the SNES release, not the Switch one"
        );
    }

    fn n64_game(id: u64, name: &str, region: u8) -> IgdbGame {
        IgdbGame {
            id,
            name: Some(name.to_string()),
            slug: None,
            url: None,
            summary: None,
            first_release_date: None,
            cover: None,
            screenshots: None,
            genres: None,
            platforms: Some(vec![IgdbNamedObject {
                name: Some("Nintendo 64".to_string()),
            }]),
            release_dates: Some(vec![IgdbReleaseDate {
                region: Some(region),
            }]),
            involved_companies: None,
            rating: None,
            rating_count: None,
            aggregated_rating: None,
            aggregated_rating_count: None,
        }
    }

    #[test]
    fn choose_result_prefers_base_title_over_numbered_sequel() {
        let sequel = n64_game(2, "Super Mario 64 2", 5);
        let original = n64_game(1, "Super Mario 64", 2);

        let chosen = choose_result(
            vec![sequel, original],
            "Mario 64",
            Some(Platform::Nintendo64),
        )
        .unwrap();
        assert_eq!(chosen.id, 1);
    }

    #[test]
    fn choose_result_prefers_the_north_american_base_over_a_japanese_revision() {
        let chosen = choose_result(
            vec![
                n64_game(2, "Super Mario 64: Shindou Pak Taiou Version", 5),
                n64_game(1, "Super Mario 64", 2),
            ],
            "Mario 64",
            Some(Platform::Nintendo64),
        )
        .unwrap();
        assert_eq!(chosen.id, 1);
    }

    #[test]
    fn image_urls_use_local_igdb_size_variants() {
        assert_eq!(
            image_url(
                Some("//images.igdb.com/igdb/image/upload/t_thumb/co123.jpg".into()),
                "t_cover_big"
            ),
            Some("https://images.igdb.com/igdb/image/upload/t_cover_big/co123.jpg".into())
        );
    }

    #[test]
    fn processed_metadata_does_not_expire_without_an_explicit_refresh() {
        let path = std::env::temp_dir().join(format!(
            "wervik-metadata-processed-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut cache = MetadataCache::load(&path);
        cache
            .put(
                "old-game-id".to_string(),
                GameMetadata {
                    name: None,
                    igdb_id: 42,
                    igdb_url: None,
                    summary: None,
                    first_release_date: None,
                    developers: Vec::new(),
                    publishers: Vec::new(),
                    genres: Vec::new(),
                    platforms: Vec::new(),
                    rating: None,
                    rating_count: None,
                    cover_path: None,
                    hero_path: None,
                    screenshot_paths: Vec::new(),
                    fetched_at: 0,
                },
            )
            .unwrap();

        assert!(cache.is_processed("old-game-id"));
        assert!(cache.rekey("old-game-id", "new-game-id").unwrap());
        assert!(!cache.is_processed("old-game-id"));
        assert!(cache.is_processed("new-game-id"));

        let _ = std::fs::remove_file(path);
    }
}
