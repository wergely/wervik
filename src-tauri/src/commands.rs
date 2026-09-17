use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;

use tauri::{Emitter, Manager};
use url::Url;

use crate::battlenet::discovery::BattleNetGame;
use crate::ea::discovery::EaGame;
use crate::game_setup::{self, EnvVar, GamescopeConfig};
use crate::mangohud::{self, MangoHudConfig};
use crate::metadata::{self, GameMetadata, MetadataCache, MetadataProgress};
use crate::nexus::{self, NexusCache, NexusMod};
use crate::platform;
use crate::playtime::{self, FinishedSessionRecord, PlaytimeDb, ProcessTarget};
use crate::retro::discovery::RomGame;
use crate::retro::platform::Platform;
use crate::retro::{
    artwork, bios, discovery as retro_discovery, naming as retro_naming, native as retro_native,
    saves,
};
use crate::retroachievements::{self, GameAchievements, RaProfileSummary, RetroAchievementsCache};
use crate::steam::achievements::{self as steam_achievements, SteamAchievementsCache};
use crate::steam::discovery::{self, LaunchKind, SteamInstall};
use crate::steam::launch;
use crate::steam::launch_options;
use crate::steam::library::{self, Game};
use crate::steam::owned::{self, OwnedGame, SteamOwnedGamesCache};
use crate::steam::SteamCredentials;
use crate::storage::{
    MetadataProvider, PendingWervikConnect, RomDirectoryConfig, Settings, Store, WervikAccount,
};
use crate::window;
use crate::xbox::discovery::XboxGame;

pub struct AppState {
    pub steam_install: Option<SteamInstall>,
    pub library_folders: Vec<PathBuf>,
    pub games: Vec<Game>,
    pub xbox_games: Vec<XboxGame>,
    pub battlenet_games: Vec<BattleNetGame>,
    pub ea_games: Vec<EaGame>,
    pub battlenet_artwork_running: Arc<AtomicBool>,
    pub playtime: PlaytimeDb,
    pub metadata_cache: MetadataCache,
    pub nexus_cache: NexusCache,
    pub retro_achievements_cache: RetroAchievementsCache,
    /// Guards against two RetroAchievements sweeps running at once — same
    /// reasoning as `artwork_fetch_running`.
    pub retro_achievements_fetch_running: Arc<AtomicBool>,
    pub steam_achievements_cache: SteamAchievementsCache,
    /// Guards against two Steam achievements sweeps running at once — same
    /// reasoning as `artwork_fetch_running`.
    pub steam_achievements_fetch_running: Arc<AtomicBool>,
    pub steam_owned_games_cache: SteamOwnedGamesCache,
    pub steam_owned_games: Vec<OwnedGame>,
    /// Guards against two Steam owned-games requests running at once.
    pub steam_owned_games_fetch_running: Arc<AtomicBool>,
    pub metadata_fetch_running: Arc<AtomicBool>,
    pub metadata_progress: Arc<metadata::MetadataProgressState>,
    /// Invalidates an in-flight metadata pass when credentials or the
    /// selected provider changes, so it cannot keep using a stale token.
    pub metadata_fetch_generation: Arc<AtomicUsize>,
    pub store: Store,
    pub game_setup_dir: PathBuf,
    pub app_data_dir: PathBuf,
    pub rom_games: Vec<RomGame>,
    /// Guards against two ROM directory scans running at once — same
    /// reasoning as `artwork_fetch_running`. Also what `get_rom_scan_status`
    /// reports and `rom-scan-status` announces.
    pub rom_scan_running: Arc<AtomicBool>,
    pub app_handle: tauri::AppHandle,
    /// Guards against two artwork-fetch passes running at once — without
    /// it, rescanning a huge library while a previous pass is still
    /// working through the backlog would spawn a second thread hammering
    /// the same host and doubling CPU/network load for no benefit.
    pub artwork_fetch_running: Arc<AtomicBool>,
    /// Local artwork paths already exposed to the frontend. Keeping this
    /// allowlist separate from the library DTO avoids rebuilding the entire
    /// library for every image request.
    pub known_image_paths: HashSet<String>,
    /// How far the current (or most recent) background artwork fetch has
    /// gotten — read by `get_artwork_progress` so the frontend can show a
    /// progress indicator even if it missed the `retro-artwork-progress`
    /// events (e.g. it wasn't mounted yet when the fetch on startup began).
    pub artwork_progress: Arc<ArtworkProgressState>,
    /// Coalesces library, playtime and achievement uploads when several
    /// local cache updates arrive close together.
    pub cloud_sync_running: Arc<AtomicBool>,
    pub cloud_sync_requested: Arc<AtomicBool>,
    /// Invalidates cloud uploads when the connected account changes.
    pub wervik_sync_generation: Arc<AtomicUsize>,
}

#[derive(Default)]
pub struct ArtworkProgressState {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkProgress {
    pub done: usize,
    pub total: usize,
}

pub type AppStateHandle = Mutex<AppState>;

impl AppState {
    pub fn rescan(&mut self) {
        self.xbox_games = crate::xbox::discovery::scan();
        self.battlenet_games = crate::battlenet::discovery::scan();
        self.ea_games = crate::ea::discovery::scan();
        crate::battlenet::artwork::spawn_fetch(
            self.app_handle.clone(),
            self.app_data_dir.clone(),
            self.battlenet_games.clone(),
            self.battlenet_artwork_running.clone(),
        );
        if let Some(install) = &self.steam_install {
            let result = library::scan(&install.root);
            self.library_folders = result.library_folders;
            self.games = result.games;
        }
        self.rebuild_known_image_paths();
        self.seed_provider_playtime();
        self.start_metadata_fetch();
        self.start_achievements_fetch();
        self.start_steam_owned_fetch();
        self.start_steam_achievements_fetch();
    }

    pub(crate) fn rebuild_known_image_paths(&mut self) {
        let mut paths = HashSet::new();
        let add_path = |paths: &mut HashSet<String>, path: &Path| {
            paths.insert(path.to_string_lossy().into_owned());
        };

        for game in &self.games {
            for path in [&game.cover_path, &game.hero_path, &game.logo_path]
                .into_iter()
                .flatten()
            {
                add_path(&mut paths, path);
            }
        }
        for game in &self.xbox_games {
            if let Some(path) = &game.logo_path {
                add_path(&mut paths, path);
            }
        }
        for game in &self.battlenet_games {
            if let Some(path) = crate::battlenet::artwork::cached_cover(&self.app_data_dir, game) {
                add_path(&mut paths, &path);
            }
            if let Some(path) = crate::battlenet::artwork::cached_hero(&self.app_data_dir, game) {
                add_path(&mut paths, &path);
            }
        }
        for game in &self.rom_games {
            if let Some(path) = artwork::cached_cover(&self.app_data_dir, &game.id) {
                add_path(&mut paths, &path);
            }
        }
        for metadata in self.metadata_cache.values() {
            for path in [
                metadata.cover_path.as_deref(),
                metadata.hero_path.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                paths.insert(path.to_owned());
            }
            paths.extend(metadata.screenshot_paths.iter().cloned());
        }

        self.known_image_paths = paths;
    }

    pub(crate) fn metadata_queries(&self) -> Vec<metadata::MetadataQuery> {
        self.games
            .iter()
            .map(|game| metadata::MetadataQuery {
                id: game.app_id.to_string(),
                name: game.name.clone(),
                platform: None,
            })
            .chain(
                self.steam_owned_games
                    .iter()
                    .map(|game| metadata::MetadataQuery {
                        id: game.app_id.to_string(),
                        name: game.name.clone(),
                        platform: None,
                    }),
            )
            .chain(self.xbox_games.iter().map(|game| metadata::MetadataQuery {
                id: game.id.clone(),
                name: game.name.clone(),
                platform: None,
            }))
            .chain(
                self.battlenet_games
                    .iter()
                    .map(|game| metadata::MetadataQuery {
                        id: game.id.clone(),
                        name: game.name.clone(),
                        platform: None,
                    }),
            )
            .chain(self.ea_games.iter().map(|game| metadata::MetadataQuery {
                id: game.id.clone(),
                name: game.name.clone(),
                platform: None,
            }))
            .chain(self.rom_games.iter().map(|game| metadata::MetadataQuery {
                id: game.id.clone(),
                name: game.name.clone(),
                platform: Some(game.platform),
            }))
            .collect()
    }

    pub(crate) fn start_metadata_fetch(&self) {
        self.start_metadata_fetch_with_force(false);
    }

    pub(crate) fn start_metadata_fetch_with_force(&self, force: bool) {
        let generation_id = self
            .metadata_fetch_generation
            .fetch_add(1, Ordering::SeqCst)
            + 1;
        let settings = self.store.settings();
        let wervik_access_token = self
            .store
            .wervik_account()
            .map(|account| account.access_token)
            .filter(|token| !token.trim().is_empty());
        self.metadata_fetch_running.store(false, Ordering::SeqCst);
        metadata::spawn_fetch(
            self.app_handle.clone(),
            self.app_data_dir.clone(),
            self.metadata_queries(),
            self.metadata_fetch_running.clone(),
            self.metadata_progress.clone(),
            self.metadata_fetch_generation.clone(),
            generation_id,
            settings.metadata_provider,
            settings.igdb_client_id,
            settings.igdb_client_secret,
            wervik_access_token,
            force,
        );
    }

    /// RetroAchievements only ever applies to ROMs — Steam/Xbox/Battle.net
    /// have no equivalent here.
    pub(crate) fn achievement_queries(&self) -> Vec<retroachievements::AchievementQuery> {
        self.rom_games
            .iter()
            .map(|game| retroachievements::AchievementQuery {
                id: game.id.clone(),
                name: game.name.clone(),
                platform: game.platform,
            })
            .collect()
    }

    pub(crate) fn start_achievements_fetch(&self) {
        let settings = self.store.settings();
        retroachievements::spawn_fetch(
            self.app_handle.clone(),
            self.achievement_queries(),
            settings.ra_username,
            settings.ra_api_key,
            self.retro_achievements_fetch_running.clone(),
        );
    }

    /// The manual override if set, else whatever's auto-detected from the
    /// local Steam install's own `loginusers.vdf` — `None` when neither is
    /// available (no override, and either no Steam install or no account
    /// has ever signed in locally).
    pub(crate) fn steam_id64(&self) -> Option<String> {
        let settings = self.store.settings();
        if !settings.steam_id64_override.trim().is_empty() {
            return Some(settings.steam_id64_override.trim().to_string());
        }
        steam_achievements::detect_steam_id64(&self.steam_install.as_ref()?.root)
    }

    /// Which Steam Web API credentials are currently in effect: a connected
    /// Wervik Account always wins (it needs no key from the user at all —
    /// see `SteamCredentials`), falling back to the manual key from Settings,
    /// or `None` when neither is configured.
    pub(crate) fn steam_credentials(&self) -> Option<SteamCredentials> {
        if let Some(account) = self.store.wervik_account() {
            let token = account.access_token.trim();
            if !token.is_empty() {
                return Some(SteamCredentials::Wervik(token.to_string()));
            }
        }
        let settings = self.store.settings();
        let api_key = settings.steam_api_key.trim();
        (!api_key.is_empty()).then(|| SteamCredentials::Manual(api_key.to_string()))
    }

    pub(crate) fn start_steam_achievements_fetch(&mut self) {
        let steam_id64 = self.steam_id64();
        if let Some(steam_id64) = steam_id64.as_deref() {
            if let Err(error) = self.steam_achievements_cache.select_account(steam_id64) {
                eprintln!("Could not select the Steam achievements account: {error}");
            }
        }
        let Some(credentials) = self.steam_credentials() else {
            return;
        };
        let app_ids = self
            .games
            .iter()
            .map(|game| (game.app_id.to_string(), game.app_id))
            .collect();
        steam_achievements::spawn_fetch(
            self.app_handle.clone(),
            app_ids,
            credentials,
            steam_id64,
            self.steam_achievements_fetch_running.clone(),
        );
    }

    pub(crate) fn start_steam_owned_fetch(&mut self) {
        let steam_id64 = self.steam_id64();
        self.steam_owned_games = if let Some(steam_id64) = steam_id64.as_deref() {
            if let Err(error) = self.steam_owned_games_cache.select_account(steam_id64) {
                eprintln!("Could not select the Steam owned-games account: {error}");
            }
            self.steam_owned_games_cache
                .get(steam_id64)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let Some(credentials) = self.steam_credentials() else {
            return;
        };
        if let Some(steam_id64) = steam_id64 {
            if self
                .steam_owned_games_cache
                .get_fresh(&steam_id64)
                .is_some()
            {
                return;
            }
            owned::spawn_fetch(
                self.app_handle.clone(),
                credentials,
                steam_id64,
                self.steam_owned_games_fetch_running.clone(),
            );
        }
    }

    /// Keeps metadata attached to a ROM when a rescan gives it a new source
    /// id, which happens for folder-based games after their parent directory
    /// is moved. Matching the local title and platform avoids another lookup
    /// while still letting a platform change produce a fresh result.
    pub(crate) fn preserve_rom_metadata_across_rescan(
        &mut self,
        previous: &[RomGame],
        scanned: &[RomGame],
    ) {
        let mut candidates: HashMap<(Platform, String), Vec<String>> = HashMap::new();
        for game in previous {
            if self.metadata_cache.is_processed(&game.id) {
                candidates
                    .entry((game.platform, game.name.clone()))
                    .or_default()
                    .push(game.id.clone());
            }
        }

        for game in scanned {
            if self.metadata_cache.is_processed(&game.id) {
                if let Some(ids) = candidates.get_mut(&(game.platform, game.name.clone())) {
                    ids.retain(|id| id != &game.id);
                }
                continue;
            }
            let key = (game.platform, game.name.clone());
            let Some(ids) = candidates.get_mut(&key) else {
                continue;
            };
            let Some(index) = ids.iter().position(|id| id != &game.id) else {
                continue;
            };
            let old_id = ids.remove(index);
            if let Err(error) = self.metadata_cache.rekey(&old_id, &game.id) {
                eprintln!(
                    "Could not carry metadata from {old_id} to {}: {error}",
                    game.id
                );
            }
        }
    }

    /// Reconciles ROMs whose localized filenames resolve to the same IGDB
    /// game. The preferred regional dump remains the visible game while the
    /// other files stay available through the existing variant picker.
    pub(crate) fn dedupe_rom_games_by_metadata(&mut self) {
        let igdb_ids = self.metadata_cache.igdb_ids();
        let (rom_games, changed) =
            retro_discovery::dedupe_by_igdb_id(std::mem::take(&mut self.rom_games), &igdb_ids);
        self.rom_games = rom_games;
        if !changed {
            return;
        }

        // A favorite attached to a title that became a variant should remain
        // visible on the single canonical card.
        let favorite_roots: Vec<String> = self
            .rom_games
            .iter()
            .filter(|game| {
                game.variants
                    .iter()
                    .any(|variant| self.store.is_favorite(&variant.id))
            })
            .map(|game| game.id.clone())
            .collect();
        for game_id in favorite_roots {
            self.store.set_favorite(&game_id, true);
        }
        let _ = crate::retro::cache::save(&self.app_data_dir, &self.rom_games);
    }

    pub(crate) fn seed_provider_playtime(&self) {
        for game in &self.games {
            let _ = self.playtime.seed_if_missing(
                &game.app_id.to_string(),
                game.playtime_minutes.unwrap_or(0).saturating_mul(60),
                game.last_played,
            );
        }
        for game in &self.xbox_games {
            let _ =
                self.playtime
                    .seed_if_missing(&game.id, 0, self.store.local_last_played(&game.id));
        }
        for game in &self.battlenet_games {
            let _ =
                self.playtime
                    .seed_if_missing(&game.id, 0, self.store.local_last_played(&game.id));
        }
        for game in &self.ea_games {
            let _ =
                self.playtime
                    .seed_if_missing(&game.id, 0, self.store.local_last_played(&game.id));
        }
    }

    /// Full filesystem walk of every configured ROM directory, in the
    /// background — only called from an explicit user action (adding/
    /// removing a directory, or a "Rescan" button), never on ordinary app
    /// startup, which instead loads the cached results (see
    /// `retro::cache`). Unlike every other background sweep in this file,
    /// this one doesn't hold the state lock for its slow part (the walk
    /// itself): a large ROM library would otherwise freeze every other
    /// command (launching a game, opening Settings, ...) for as long as
    /// the scan takes. The frontend follows along via `rom-scan-status`
    /// (see `get_rom_scan_status`) the same way it does the metadata and
    /// artwork sweeps' own progress events.
    pub(crate) fn spawn_rom_scan(&self) {
        if self
            .rom_scan_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let app_handle = self.app_handle.clone();
        let running = self.rom_scan_running.clone();
        let dirs = self.store.rom_directories();

        std::thread::spawn(move || {
            let _ = app_handle.emit("rom-scan-status", true);
            let (tagged, untagged): (Vec<_>, Vec<_>) =
                dirs.into_iter().partition(|dir| dir.platform_key.is_some());
            let mut scanned = retro_discovery::scan_directories(
                &untagged.into_iter().map(|dir| dir.path).collect::<Vec<_>>(),
            );
            for dir in tagged {
                // `platform_key` is only `Some` here (guaranteed by the
                // `partition` above); an unrecognized key (e.g. a save from
                // a newer Wervik version with a platform this one doesn't
                // know) is silently skipped rather than treated as a fatal
                // scan error — same "don't guess" policy the rest of this
                // module follows.
                if let Some(platform) = dir.platform_key.as_deref().and_then(Platform::from_key) {
                    scanned.extend(retro_discovery::scan_tagged_directory(&dir.path, platform));
                }
            }
            if let Some(state_handle) = app_handle.try_state::<AppStateHandle>() {
                let mut state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
                let previous_rom_games = std::mem::replace(&mut state.rom_games, Vec::new());
                state.preserve_rom_metadata_across_rescan(&previous_rom_games, &scanned);
                state.rom_games = scanned;
                // Built as an owned `Vec` first rather than passed inline:
                // through a `MutexGuard`, `state.store.migrate_legacy_bios_paths(state.rom_games...)`
                // borrows `state` mutably (for `.store`) and immutably (for
                // `.rom_games`) at the same time — the compiler can't prove
                // those are disjoint fields the way it can through a plain
                // `&mut AppState`, since each access goes through a
                // separate `Deref`/`DerefMut` call on the guard.
                let bios_migration_keys: Vec<(String, String)> = state
                    .rom_games
                    .iter()
                    .map(|rom| (rom.id.clone(), rom.platform.key().to_string()))
                    .collect();
                state.store.migrate_legacy_bios_paths(bios_migration_keys);
                state.dedupe_rom_games_by_metadata();
                state.rebuild_known_image_paths();
                let _ = crate::retro::cache::save(&state.app_data_dir, &state.rom_games);
                // The configured metadata provider is the primary cover
                // source for retro games. Keep the old libretro-thumbnails
                // pass only when that provider is unavailable.
                let settings = state.store.settings();
                let metadata_available = match settings.metadata_provider {
                    MetadataProvider::Igdb => metadata::is_configured(
                        &settings.igdb_client_id,
                        &settings.igdb_client_secret,
                    ),
                    MetadataProvider::Wervik => state
                        .store
                        .wervik_account()
                        .is_some_and(|account| !account.access_token.trim().is_empty()),
                };
                if !metadata_available {
                    spawn_artwork_fetch(
                        state.app_handle.clone(),
                        state.app_data_dir.clone(),
                        state.rom_games.clone(),
                        state.artwork_fetch_running.clone(),
                        state.artwork_progress.clone(),
                    );
                }
                state.start_metadata_fetch();
                state.start_achievements_fetch();
            }
            schedule_wervik_cloud_sync(&app_handle);
            running.store(false, Ordering::SeqCst);
            let _ = app_handle.emit("rom-scan-status", false);
        });
    }
}

/// A pause between each artwork download — this is a background job, not
/// something anyone is waiting on, so it has no business racing to finish:
/// keeps CPU/network load low even with a library of thousands of games,
/// and is a courtesy to the free, public thumbnail host rather than
/// hammering it as fast as possible.
const ARTWORK_FETCH_DELAY: Duration = Duration::from_millis(200);

/// Fetches missing box art for `roms` one at a time in the background and
/// emits `retro-artwork-updated` after each successful fetch so the
/// frontend can refresh what it's showing. `running` prevents two passes
/// from ever overlapping (see `AppState::artwork_fetch_running`) — if one
/// is already in flight, this is a silent no-op rather than piling on a
/// second thread doing redundant work.
pub(crate) fn spawn_artwork_fetch(
    app_handle: tauri::AppHandle,
    app_data_dir: PathBuf,
    roms: Vec<RomGame>,
    running: Arc<AtomicBool>,
    progress: Arc<ArtworkProgressState>,
) {
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    std::thread::spawn(move || {
        let agent = artwork::new_agent();
        let total = roms
            .iter()
            .filter(|rom| !artwork::already_resolved(&app_data_dir, &rom.id))
            .count();
        progress.done.store(0, Ordering::SeqCst);
        progress.total.store(total, Ordering::SeqCst);
        let _ = app_handle.emit("retro-artwork-progress", ArtworkProgress { done: 0, total });

        for rom in roms {
            if artwork::already_resolved(&app_data_dir, &rom.id) {
                continue;
            }
            match artwork::fetch_cover(&agent, &app_data_dir, &rom) {
                Ok(Some(path)) => {
                    if let Some(state_handle) = app_handle.try_state::<AppStateHandle>() {
                        let mut state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
                        state
                            .known_image_paths
                            .insert(path.to_string_lossy().into_owned());
                    }
                    let _ = app_handle.emit("retro-artwork-updated", &rom.id);
                }
                Ok(None) => {}
                Err(e) => eprintln!("box art fetch failed for {}: {e}", rom.name),
            }
            let done = progress.done.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = app_handle.emit("retro-artwork-progress", ArtworkProgress { done, total });
            std::thread::sleep(ARTWORK_FETCH_DELAY);
        }
        running.store(false, Ordering::SeqCst);
    });
}

/// Current progress of the background box-art fetch, for a frontend that
/// mounts after the fetch already started (e.g. app startup fires it from
/// Rust before the UI has a chance to subscribe to `retro-artwork-progress`).
#[tauri::command(async)]
pub fn get_artwork_progress(state: State<AppStateHandle>) -> ArtworkProgress {
    let state = state.lock().unwrap_or_else(|e| e.into_inner());
    ArtworkProgress {
        done: state.artwork_progress.done.load(Ordering::SeqCst),
        total: state.artwork_progress.total.load(Ordering::SeqCst),
    }
}

#[tauri::command(async)]
pub fn get_metadata_progress(state: State<AppStateHandle>) -> MetadataProgress {
    let state = state.lock().unwrap_or_else(|e| e.into_inner());
    MetadataProgress {
        done: state.metadata_progress.done.load(Ordering::SeqCst),
        total: state.metadata_progress.total.load(Ordering::SeqCst),
    }
}

/// Wipes the IGDB metadata cache (entries, misses, and downloaded artwork)
/// and re-runs the background fetch for every game, from Settings' "Refresh
/// metadata" button. Unlike a normal rescan, this is the only way to correct
/// a game that already has a *wrong* cached match — normal scans keep the
/// existing result regardless of its age.
#[tauri::command(async)]
pub fn refresh_all_metadata(state: State<AppStateHandle>) -> Result<(), String> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.metadata_cache.clear()?;
    metadata::clear_artwork(&state.app_data_dir)?;
    state.rebuild_known_image_paths();
    state.start_metadata_fetch_with_force(true);
    Ok(())
}

/// Jumps one game to the front of the box-art line — called when the user
/// opens its details, so a game they're actually looking at right now gets
/// a cover sooner than waiting for the background pass (see
/// `spawn_artwork_fetch`) to work through what can be a library thousands
/// of ROMs deep. Returns a cached cover immediately if there already is
/// one; otherwise kicks off the lookup in the background and returns
/// `None` right away — same as the background pass, a hit is reported
/// through a `retro-artwork-updated` event once it lands.
#[tauri::command(async)]
pub fn fetch_cover_now(state: State<AppStateHandle>, id: String) -> Option<String> {
    let (app_handle, app_data_dir, rom) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let settings = state.store.settings();
        let metadata_available = match settings.metadata_provider {
            MetadataProvider::Igdb => {
                metadata::is_configured(&settings.igdb_client_id, &settings.igdb_client_secret)
            }
            MetadataProvider::Wervik => state
                .store
                .wervik_account()
                .is_some_and(|account| !account.access_token.trim().is_empty()),
        };
        if metadata_available {
            return None;
        }
        let rom = state.rom_games.iter().find(|r| r.id == id)?.clone();
        (state.app_handle.clone(), state.app_data_dir.clone(), rom)
    };
    if let Some(path) = artwork::cached_cover(&app_data_dir, &id) {
        if let Some(state) = app_handle.try_state::<AppStateHandle>() {
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            state
                .known_image_paths
                .insert(path.to_string_lossy().into_owned());
        }
        return Some(path.to_string_lossy().to_string());
    }
    std::thread::spawn(move || {
        let agent = artwork::new_agent();
        if let Ok(Some(path)) = artwork::fetch_cover(&agent, &app_data_dir, &rom) {
            if let Some(state) = app_handle.try_state::<AppStateHandle>() {
                let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                state
                    .known_image_paths
                    .insert(path.to_string_lossy().into_owned());
            }
            let _ = app_handle.emit("retro-artwork-updated", &rom.id);
        }
    });
    None
}

pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn start_process_session(
    state: &mut AppState,
    game_id: &str,
    source: &str,
    install_dir: &Path,
) -> Result<(), String> {
    let session_id = state.playtime.start_session(game_id, source, now_unix())?;
    let target = ProcessTarget::from_install_dir(install_dir);
    playtime::spawn_process_monitor(
        state.app_handle.clone(),
        session_id,
        game_id.to_string(),
        target,
        state.store.settings().maximize_games,
    );
    Ok(())
}

/// Completes a session from a process monitor or a retro session shutdown and
/// refreshes the frontend. The old JSON retro total is also updated for
/// compatibility with installations created before the SQLite database.
pub(crate) fn finish_playtime_session(app_handle: &tauri::AppHandle, session_id: i64) {
    let Some(state) = app_handle.try_state::<AppStateHandle>() else {
        return;
    };
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(Some(finished)) = state.playtime.finish_session(session_id, now_unix()) else {
        return;
    };
    if finished.source == "retro" && finished.duration_seconds > 0 {
        state.store.record_retro_session(
            &finished.game_id,
            finished.duration_seconds.div_ceil(60),
            finished.ended_at,
        );
    }
    let game_id = finished.game_id.clone();
    drop(state);
    let _ = app_handle.emit("playtime-updated", &game_id);
    schedule_wervik_cloud_sync(app_handle);
}

/// Where a game came from. The frontend uses this to label/filter games and
/// to decide which source-specific actions (Steam's Setup drawer sections,
/// launch options, vs. a retro game's BIOS/emulator handling) apply.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GameSource {
    Steam,
    Retro,
    Xbox,
    #[serde(rename = "battlenet")]
    BattleNet,
    Ea,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDto {
    /// Unified id across sources: a Steam app id as a string, or a ROM's
    /// stable hash-based id (see `retro::discovery`). Favorites and
    /// playtime are keyed by this, not by `app_id`.
    pub id: String,
    pub source: GameSource,
    pub name: String,
    pub is_installed: bool,
    pub install_dir: String,
    pub library_path: String,
    pub size_on_disk_bytes: u64,
    pub last_played: Option<i64>,
    pub playtime_minutes: Option<u64>,
    pub igdb_score: Option<f64>,
    pub igdb_metadata: Option<GameMetadataSummary>,
    pub reaction: Option<String>,
    pub cover_path: Option<String>,
    pub hero_path: Option<String>,
    pub logo_path: Option<String>,
    /// Local screenshots automatically captured by RetroArch when this retro
    /// game unlocks an achievement.
    pub local_screenshot_paths: Vec<String>,
    pub is_favorite: bool,
    /// Present only for `source == Steam` — kept alongside `id` so the
    /// existing Steam-specific commands (launch, Setup drawer, overflow
    /// menu) keep taking a plain app id. Xbox uses `xbox_app_id` below.
    pub app_id: Option<u32>,
    /// Present only for `source == Xbox` — this is the Windows AppUserModelID
    /// used to launch the title through `shell:AppsFolder`.
    pub xbox_app_id: Option<String>,
    /// Present only for `source == BattleNet` — the product code accepted by
    /// the Battle.net launcher's `--exec="launch ..."` command.
    pub battlenet_product_code: Option<String>,
    /// Present only for `source == Ea` — the content/offer id accepted by
    /// EA App's launch protocol.
    pub ea_game_id: Option<String>,
    /// Short platform label for the source badge (e.g. "SNES"), present
    /// only for `source == Retro`.
    pub platform: Option<String>,
    /// Stable machine key (see `Platform::key`) for joining against
    /// `PlatformStatus.key` — `platform` above is a display label and can
    /// theoretically collide or change wording, this won't.
    pub platform_key: Option<String>,
    pub requires_bios: Option<bool>,
    /// Name of the native emulator this game will actually launch with
    /// (e.g. "gopher64"). `None` means the platform still needs an
    /// emulator installed.
    pub native_emulator: Option<String>,
    /// Region label for the copy of the ROM that plays by default (see
    /// `retro::naming::region_label`) — `None` for Steam games, which have
    /// no such concept.
    pub default_region: Option<String>,
    /// Other regional dumps of this same game the user can choose to play
    /// instead (see `retro::discovery::RomVariant`, `launch_retro_game`'s
    /// `variant_id`) — empty for Steam games.
    pub variants: Vec<GameVariant>,
    /// Achievement completion percentage (earned/total, rounded), from
    /// whatever the relevant background sweep has cached so far —
    /// RetroAchievements for a retro game (`retroachievements::spawn_fetch`),
    /// the Steam Web API for a Steam one (`steam::achievements::spawn_fetch`).
    /// `None` for Xbox/Battle.net (neither has an achievements source
    /// wired up), when the relevant credentials aren't configured, or
    /// before a match has been found/cached yet.
    pub achievement_completion_percent: Option<u8>,
    /// Whether this retro game launches with RetroAchievements Hardcore mode.
    pub hardcore_mode: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameMetadataSummary {
    pub igdb_id: u64,
    pub first_release_date: Option<i64>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub genres: Vec<String>,
    pub platforms: Vec<String>,
    pub rating: Option<f64>,
    pub rating_count: Option<u64>,
}

fn metadata_summary(metadata: &GameMetadata) -> GameMetadataSummary {
    GameMetadataSummary {
        igdb_id: metadata.igdb_id,
        first_release_date: metadata.first_release_date,
        developers: metadata.developers.clone(),
        publishers: metadata.publishers.clone(),
        genres: metadata.genres.clone(),
        platforms: metadata.platforms.clone(),
        rating: metadata.rating,
        rating_count: metadata.rating_count,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameVariant {
    pub id: String,
    /// Original title of the alternate ROM. This makes localized variants
    /// distinguishable even when their filenames have no region tag.
    pub name: String,
    pub region: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    pub steam_found: bool,
    pub steam_root: Option<String>,
    pub xbox_found: bool,
    pub battlenet_found: bool,
    pub ea_found: bool,
    pub library_folders: Vec<String>,
    pub games: Vec<GameDto>,
}

fn resolved_playtime(
    playtime: &PlaytimeDb,
    game_id: &str,
    provider_last_played: Option<i64>,
    provider_minutes: Option<u64>,
) -> (Option<i64>, Option<u64>) {
    if let Ok(Some(summary)) = playtime.summary(game_id) {
        return (summary.last_played, Some(summary.total_minutes()));
    }
    (provider_last_played, provider_minutes)
}

fn steam_to_dto(game: &Game, store: &Store, playtime: &PlaytimeDb) -> GameDto {
    let (last_played, playtime_minutes) = resolved_playtime(
        playtime,
        &game.app_id.to_string(),
        game.last_played,
        game.playtime_minutes,
    );
    GameDto {
        id: game.app_id.to_string(),
        source: GameSource::Steam,
        name: game.name.clone(),
        is_installed: true,
        install_dir: game.install_dir.to_string_lossy().to_string(),
        library_path: game.library_path.to_string_lossy().to_string(),
        size_on_disk_bytes: game.size_on_disk_bytes,
        last_played,
        playtime_minutes,
        igdb_score: None,
        igdb_metadata: None,
        reaction: None,
        cover_path: game
            .cover_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        hero_path: game
            .hero_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        logo_path: game
            .logo_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        local_screenshot_paths: Vec::new(),
        is_favorite: store.is_favorite(&game.app_id.to_string()),
        app_id: Some(game.app_id),
        xbox_app_id: None,
        battlenet_product_code: None,
        ea_game_id: None,
        platform: None,
        platform_key: None,
        requires_bios: None,
        native_emulator: None,
        default_region: None,
        variants: Vec::new(),
        achievement_completion_percent: None,
        hardcore_mode: false,
    }
}

fn steam_owned_to_dto(game: &OwnedGame, store: &Store, playtime: &PlaytimeDb) -> GameDto {
    let id = game.app_id.to_string();
    let (last_played, playtime_minutes) =
        resolved_playtime(playtime, &id, game.last_played, game.playtime_minutes);
    GameDto {
        id,
        source: GameSource::Steam,
        name: game.name.clone(),
        is_installed: false,
        install_dir: String::new(),
        library_path: String::new(),
        size_on_disk_bytes: 0,
        last_played,
        playtime_minutes,
        igdb_score: None,
        igdb_metadata: None,
        reaction: None,
        cover_path: Some(owned::cover_url(game.app_id)),
        hero_path: Some(owned::hero_url(game.app_id)),
        logo_path: game.icon_url.clone(),
        local_screenshot_paths: Vec::new(),
        is_favorite: store.is_favorite(&game.app_id.to_string()),
        app_id: Some(game.app_id),
        xbox_app_id: None,
        battlenet_product_code: None,
        ea_game_id: None,
        platform: None,
        platform_key: None,
        requires_bios: None,
        native_emulator: None,
        default_region: None,
        variants: Vec::new(),
        achievement_completion_percent: None,
        hardcore_mode: false,
    }
}

/// One row in a platform's "Default Emulator" list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EmulatorKind {
    Native,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorOption {
    /// Stable id for a native emulator.
    pub id: String,
    pub display_name: String,
    pub kind: EmulatorKind,
    /// Whether the native emulator is installed and ready to launch.
    pub available: bool,
    /// Whether Wervik can download and install this option itself (see
    /// `retro::native::install`) — drives whether the UI offers a
    /// "Download" action instead of just "Not installed".
    pub managed: bool,
    /// The best default for this platform — exactly one option per platform
    /// is ever marked `true` when the platform has a supported emulator.
    pub recommended: bool,
}

/// Every native emulator Wervik knows how to launch for this platform, in
/// the fixed preference order declared by `native_emulators_for`.
fn emulator_options_for(app_data_dir: &Path, platform: Platform) -> Vec<EmulatorOption> {
    retro_native::native_emulators_for(platform)
        .into_iter()
        .map(|e| EmulatorOption {
            id: e.id.to_string(),
            display_name: e.display_name.to_string(),
            kind: EmulatorKind::Native,
            available: retro_native::is_available(app_data_dir, &e),
            managed: e.is_managed(),
            recommended: e.recommended,
        })
        .collect()
}

/// The user's explicit "Default Emulator" choice for `platform` — always
/// one of `emulator_options_for`'s ids, whether or not that choice is
/// currently available (an unavailable selection is a real, visible state
/// in the UI, not something silently swapped out — see
/// `resolve_effective_native_emulator`). The first time a platform is seen
/// with nothing chosen yet, this picks an installed native emulator or the
/// recommended native option so the UI exposes the correct Download action.
fn selected_emulator_id(app_data_dir: &Path, platform: Platform, store: &Store) -> String {
    if let Some(id) = store.default_emulator(platform.key()) {
        return id;
    }
    let native_options = retro_native::native_emulators_for(platform);
    native_options
        .iter()
        .find(|e| e.recommended && retro_native::is_available(app_data_dir, e))
        .or_else(|| {
            native_options
                .iter()
                .find(|e| retro_native::is_available(app_data_dir, e))
        })
        .map(|e| e.id.to_string())
        .or_else(|| {
            native_options
                .into_iter()
                .find(|e| e.recommended)
                .map(|e| e.id.to_string())
        })
        .unwrap_or_default()
}

/// What will actually run right now for `platform`: the selected native
/// emulator if it is installed. `None` means the user needs to install one.
fn effective_emulator_id(app_data_dir: &Path, platform: Platform, store: &Store) -> Option<String> {
    let options = emulator_options_for(app_data_dir, platform);
    let selected = selected_emulator_id(app_data_dir, platform, store);
    options
        .iter()
        .find(|o| o.id == selected && o.available)
        .map(|o| o.id.clone())
}

/// The concrete native emulator to launch for `platform`, or `None` when the
/// selected option is not installed. Kept separate because callers need the
/// full `NativeEmulator`, not just its id.
fn resolve_effective_native_emulator(
    app_data_dir: &Path,
    platform: Platform,
    store: &Store,
) -> Option<retro_native::NativeEmulator> {
    let selected = selected_emulator_id(app_data_dir, platform, store);
    retro_native::native_emulators_for(platform)
        .into_iter()
        .find(|e| e.id == selected && retro_native::is_available(app_data_dir, e))
}

fn retro_to_dto(
    rom: &RomGame,
    store: &Store,
    playtime_db: &PlaytimeDb,
    app_data_dir: &Path,
    native_emulator: Option<&str>,
) -> GameDto {
    let legacy_playtime = store.retro_playtime(&rom.id);
    let (last_played, playtime_minutes) = resolved_playtime(
        playtime_db,
        &rom.id,
        legacy_playtime.last_played,
        Some(legacy_playtime.total_minutes),
    );
    GameDto {
        id: rom.id.clone(),
        source: GameSource::Retro,
        name: rom.name.clone(),
        is_installed: true,
        install_dir: String::new(),
        library_path: rom.rom_path.clone(),
        size_on_disk_bytes: rom.size_bytes,
        last_played,
        playtime_minutes,
        igdb_score: None,
        igdb_metadata: None,
        reaction: None,
        cover_path: artwork::cached_cover(app_data_dir, &rom.id)
            .map(|p| p.to_string_lossy().to_string()),
        hero_path: None,
        logo_path: None,
        // Achievement screenshots are resolved only when the detail view is
        // opened. Scanning RetroArch's screenshot directory for every ROM
        // on every library refresh made ordinary snapshots unnecessarily
        // expensive on larger libraries.
        local_screenshot_paths: Vec::new(),
        is_favorite: store.is_favorite(&rom.id),
        app_id: None,
        xbox_app_id: None,
        battlenet_product_code: None,
        ea_game_id: None,
        platform: Some(rom.platform.short_name().to_string()),
        platform_key: Some(rom.platform.key().to_string()),
        requires_bios: Some(rom.platform.requires_bios()),
        native_emulator: native_emulator.map(|s| s.to_string()),
        default_region: Some(retro_naming::region_label(&rom.rom_path)),
        variants: rom
            .variants
            .iter()
            .map(|v| GameVariant {
                id: v.id.clone(),
                name: v.name.clone(),
                region: v.region.clone(),
            })
            .collect(),
        // Filled in by `library_snapshot`, once per call, from whatever
        // the background sweep has already cached — this constructor has
        // no access to that cache.
        achievement_completion_percent: None,
        hardcore_mode: store.is_retro_hardcore(&rom.id),
    }
}

fn xbox_to_dto(game: &XboxGame, store: &Store, playtime: &PlaytimeDb) -> GameDto {
    let logo_path = game
        .logo_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let (last_played, playtime_minutes) =
        resolved_playtime(playtime, &game.id, store.local_last_played(&game.id), None);
    GameDto {
        id: game.id.clone(),
        source: GameSource::Xbox,
        name: game.name.clone(),
        is_installed: true,
        install_dir: game.install_dir.to_string_lossy().to_string(),
        library_path: game.install_dir.to_string_lossy().to_string(),
        // Xbox does not expose a cheap, stable local size query. Walking a
        // Game Pass install here could block startup for minutes, so the
        // first integration leaves size unavailable in the detail view.
        size_on_disk_bytes: 0,
        last_played,
        playtime_minutes,
        igdb_score: None,
        igdb_metadata: None,
        reaction: None,
        cover_path: logo_path.clone(),
        hero_path: logo_path.clone(),
        logo_path,
        local_screenshot_paths: Vec::new(),
        is_favorite: store.is_favorite(&game.id),
        app_id: None,
        xbox_app_id: Some(game.aumid.clone()),
        battlenet_product_code: None,
        ea_game_id: None,
        platform: None,
        platform_key: None,
        requires_bios: None,
        native_emulator: None,
        default_region: None,
        variants: Vec::new(),
        achievement_completion_percent: None,
        hardcore_mode: false,
    }
}

fn battlenet_to_dto(
    game: &BattleNetGame,
    store: &Store,
    playtime: &PlaytimeDb,
    app_data_dir: &Path,
) -> GameDto {
    let cover_path = crate::battlenet::artwork::cached_cover(app_data_dir, game)
        .map(|p| p.to_string_lossy().to_string());
    let hero_path = crate::battlenet::artwork::cached_hero(app_data_dir, game)
        .map(|p| p.to_string_lossy().to_string());
    let (last_played, playtime_minutes) =
        resolved_playtime(playtime, &game.id, store.local_last_played(&game.id), None);
    GameDto {
        id: game.id.clone(),
        source: GameSource::BattleNet,
        name: game.name.clone(),
        is_installed: true,
        install_dir: game.install_dir.to_string_lossy().to_string(),
        library_path: game.install_dir.to_string_lossy().to_string(),
        // Battle.net does not expose a cheap, stable local size query. Avoid
        // walking large installs during startup; the detail view omits size
        // for this source until a dedicated asynchronous size pass exists.
        size_on_disk_bytes: 0,
        last_played,
        playtime_minutes,
        igdb_score: None,
        igdb_metadata: None,
        reaction: None,
        cover_path,
        hero_path,
        logo_path: None,
        local_screenshot_paths: Vec::new(),
        is_favorite: store.is_favorite(&game.id),
        app_id: None,
        xbox_app_id: None,
        battlenet_product_code: Some(game.launch_code.clone()),
        ea_game_id: None,
        platform: None,
        platform_key: None,
        requires_bios: None,
        native_emulator: None,
        default_region: None,
        variants: Vec::new(),
        achievement_completion_percent: None,
        hardcore_mode: false,
    }
}

fn ea_to_dto(game: &EaGame, store: &Store, playtime: &PlaytimeDb) -> GameDto {
    let (last_played, playtime_minutes) =
        resolved_playtime(playtime, &game.id, store.local_last_played(&game.id), None);
    GameDto {
        id: game.id.clone(),
        source: GameSource::Ea,
        name: game.name.clone(),
        is_installed: true,
        install_dir: game.install_dir.to_string_lossy().to_string(),
        library_path: game.install_dir.to_string_lossy().to_string(),
        // EA App does not expose a cheap, stable local size query. Avoid
        // walking large installs during startup; the detail view omits size
        // for this source until a dedicated asynchronous size pass exists.
        size_on_disk_bytes: 0,
        last_played,
        playtime_minutes,
        igdb_score: None,
        igdb_metadata: None,
        reaction: None,
        cover_path: None,
        hero_path: None,
        logo_path: None,
        local_screenshot_paths: Vec::new(),
        is_favorite: store.is_favorite(&game.id),
        app_id: None,
        xbox_app_id: None,
        battlenet_product_code: None,
        ea_game_id: Some(game.launch_id.clone()),
        platform: None,
        platform_key: None,
        requires_bios: None,
        native_emulator: None,
        default_region: None,
        variants: Vec::new(),
        achievement_completion_percent: None,
        hardcore_mode: false,
    }
}

fn library_snapshot(state: &AppState) -> LibrarySnapshot {
    // Resolved once per *platform*, not once per ROM — cheap now that
    // `is_available` is just a file-existence check, but still no reason
    // to repeat it once per ROM on the same platform (this used to matter
    // a lot more, back when a Flatpak-packaged native emulator's
    // availability check shelled out to `flatpak info` per call).
    let mut native_by_platform: HashMap<Platform, Option<String>> = HashMap::new();
    for rom in &state.rom_games {
        native_by_platform.entry(rom.platform).or_insert_with(|| {
            resolve_effective_native_emulator(&state.app_data_dir, rom.platform, &state.store)
                .map(|e| e.display_name.to_string())
        });
    }
    let steam_achievements_account = state.steam_id64();
    let steam_achievements_enabled =
        state.steam_credentials().is_some() && steam_achievements_account.is_some();

    let installed_steam_ids: HashSet<u32> = state.games.iter().map(|game| game.app_id).collect();
    let mut games: Vec<GameDto> = state
        .games
        .iter()
        .map(|g| steam_to_dto(g, &state.store, &state.playtime))
        .chain(
            state
                .steam_owned_games
                .iter()
                .filter(|g| !installed_steam_ids.contains(&g.app_id))
                .map(|g| steam_owned_to_dto(g, &state.store, &state.playtime)),
        )
        .chain(
            state
                .xbox_games
                .iter()
                .map(|g| xbox_to_dto(g, &state.store, &state.playtime)),
        )
        .chain(
            state
                .battlenet_games
                .iter()
                .map(|g| battlenet_to_dto(g, &state.store, &state.playtime, &state.app_data_dir)),
        )
        .chain(
            state
                .ea_games
                .iter()
                .map(|g| ea_to_dto(g, &state.store, &state.playtime)),
        )
        .chain(state.rom_games.iter().map(|r| {
            let native = native_by_platform
                .get(&r.platform)
                .and_then(|n| n.as_deref());
            retro_to_dto(
                r,
                &state.store,
                &state.playtime,
                &state.app_data_dir,
                native,
            )
        }))
        .collect();
    for game in &mut games {
        game.reaction = state.store.game_reaction(&game.id);
        if let Some(metadata) = state.metadata_cache.get(&game.id) {
            if let Some(name) = metadata
                .name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
            {
                game.name = name.to_owned();
            }
            game.igdb_score = metadata.rating;
            game.igdb_metadata = Some(metadata_summary(&metadata));
            // IGDB is authoritative for retro artwork. Other providers keep
            // their native cover as the primary image and use IGDB only when
            // they have no local artwork. Either way the DTO exposes one
            // effective path, so the frontend never renders duplicate art.
            if matches!(game.source, GameSource::Retro) {
                if metadata.cover_path.is_some() {
                    game.cover_path = metadata.cover_path;
                }
                if metadata.hero_path.is_some() {
                    game.hero_path = metadata.hero_path;
                }
            } else {
                if game.cover_path.is_none() {
                    game.cover_path = metadata.cover_path;
                }
                if game.hero_path.is_none() {
                    game.hero_path = metadata.hero_path;
                }
            }
        }
        game.achievement_completion_percent = match game.source {
            GameSource::Retro => state
                .retro_achievements_cache
                .get(&game.id)
                .and_then(|achievements| achievements.completion_percent()),
            GameSource::Steam if steam_achievements_enabled => steam_achievements_account
                .as_deref()
                .and_then(|steam_id64| state.steam_achievements_cache.get(&game.id, steam_id64))
                .and_then(|achievements| achievements.completion_percent()),
            GameSource::Steam => None,
            GameSource::Xbox | GameSource::BattleNet | GameSource::Ea => None,
        };
    }

    LibrarySnapshot {
        steam_found: state.steam_install.is_some(),
        steam_root: state
            .steam_install
            .as_ref()
            .map(|i| i.root.to_string_lossy().to_string()),
        xbox_found: !state.xbox_games.is_empty(),
        battlenet_found: !state.battlenet_games.is_empty(),
        ea_found: !state.ea_games.is_empty(),
        library_folders: state
            .library_folders
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        games,
    }
}

#[tauri::command(async)]
pub fn get_library(state: State<AppStateHandle>) -> LibrarySnapshot {
    library_snapshot(&state.lock().unwrap_or_else(|e| e.into_inner()))
}

#[tauri::command(async)]
pub fn get_game_screenshot_paths(
    state: State<AppStateHandle>,
    id: String,
) -> Result<Vec<String>, String> {
    let (app_data_dir, rom_paths, game_name) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let rom = state
            .rom_games
            .iter()
            .find(|rom| rom.id == id)
            .ok_or_else(|| "Retro game not found".to_string())?;
        let rom_paths = std::iter::once(rom.rom_path.clone())
            .chain(rom.variants.iter().map(|variant| variant.rom_path.clone()))
            .collect::<Vec<_>>();
        (state.app_data_dir.clone(), rom_paths, rom.name.clone())
    };
    let paths = retro_native::achievement_screenshot_paths(
        &app_data_dir,
        rom_paths.iter().map(String::as_str),
        &game_name,
    );
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.known_image_paths.extend(paths.iter().cloned());
    Ok(paths)
}

/// Shared tail of `get_game_metadata` and `refresh_game_metadata`: runs the
/// selected metadata lookup and records either the result or a miss in the cache.
/// Callers are responsible for anything that has to happen *before* the
/// lookup (checking for an already-processed cache hit, or clearing it).
fn fetch_and_store_metadata(
    state: &State<AppStateHandle>,
    app_data_dir: &Path,
    game_id: String,
    game_name: &str,
    platform: Option<Platform>,
    settings: &Settings,
    wervik_access_token: Option<&str>,
    force: bool,
) -> Result<Option<GameMetadata>, String> {
    let Some(metadata) = metadata::fetch_game(
        app_data_dir,
        &game_id,
        game_name,
        platform,
        settings.metadata_provider,
        &settings.igdb_client_id,
        &settings.igdb_client_secret,
        wervik_access_token,
        force,
    )?
    else {
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        state.metadata_cache.mark_miss(game_id)?;
        return Ok(None);
    };
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state
        .metadata_cache
        .put(game_id.clone(), metadata.clone())?;
    for path in [
        metadata.cover_path.as_deref(),
        metadata.hero_path.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        state.known_image_paths.insert(path.to_owned());
    }
    state
        .known_image_paths
        .extend(metadata.screenshot_paths.iter().cloned());
    // Same event the background sweep emits on a hit (`metadata::spawn_fetch`)
    // — without it, nothing tells the frontend this game's cached metadata
    // just changed, and `useLibrary`'s snapshot (in particular, whatever
    // page already has this game's *old* name/cover loaded) never refreshes
    // to reflect a corrected match (e.g. after "Refresh metadata" or
    // retagging the ROM directory's platform).
    let app_handle = state.app_handle.clone();
    let _ = app_handle.emit("metadata-updated", &game_id);
    drop(state);
    schedule_wervik_cloud_sync(&app_handle);
    Ok(Some(metadata))
}

/// Returns cached IGDB metadata or performs one best-effort lookup for the
/// selected game. The cache is keyed by Wervik's source-specific game id, so
/// two installations with the same title can still be corrected separately
/// later when manual matching is added.
#[tauri::command(async)]
pub fn get_game_metadata(
    state: State<AppStateHandle>,
    game_id: String,
    game_name: String,
    platform_key: Option<String>,
) -> Result<Option<GameMetadata>, String> {
    let platform = platform_key.as_deref().and_then(Platform::from_key);
    let (cached, processed, app_data_dir, settings, wervik_access_token) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let settings = state.store.settings();
        let wervik_access_token = state
            .store
            .wervik_account()
            .map(|account| account.access_token)
            .filter(|token| !token.trim().is_empty());
        (
            state.metadata_cache.get(&game_id),
            state.metadata_cache.is_processed(&game_id),
            state.app_data_dir.clone(),
            settings,
            wervik_access_token,
        )
    };
    if let Some(metadata) = cached {
        return Ok(Some(metadata));
    }
    if processed {
        return Ok(None);
    }

    fetch_and_store_metadata(
        &state,
        &app_data_dir,
        game_id,
        &game_name,
        platform,
        &settings,
        wervik_access_token.as_deref(),
        false,
    )
}

/// Forces a fresh IGDB lookup for one game, bypassing the cache entirely —
/// the per-game "Refresh metadata" action on the game details page, for when
/// a game is stuck with a wrong cached match (see `refresh_all_metadata` for
/// the whole-library equivalent). Also clears this game's own downloaded
/// artwork first, since `download_image` would otherwise keep serving the
/// old (possibly wrong) cover/screenshots already on disk.
#[tauri::command(async)]
pub fn refresh_game_metadata(
    state: State<AppStateHandle>,
    game_id: String,
    game_name: String,
    platform_key: Option<String>,
) -> Result<Option<GameMetadata>, String> {
    let platform = platform_key.as_deref().and_then(Platform::from_key);
    let (app_data_dir, lookup_name, settings, wervik_access_token) = {
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        state.metadata_cache.remove(&game_id)?;
        metadata::clear_artwork_for(&state.app_data_dir, &game_id)?;
        state.rebuild_known_image_paths();
        // The library DTO may expose a cached metadata title instead of the
        // filename-derived ROM title. For a forced refresh, always prefer
        // the original ROM name so a bad cached match can be corrected.
        let lookup_name = state
            .rom_games
            .iter()
            .find(|rom| rom.id == game_id)
            .map(|rom| rom.name.clone())
            .unwrap_or(game_name);
        let settings = state.store.settings();
        let wervik_access_token = state
            .store
            .wervik_account()
            .map(|account| account.access_token)
            .filter(|token| !token.trim().is_empty());
        (
            state.app_data_dir.clone(),
            lookup_name,
            settings,
            wervik_access_token,
        )
    };

    fetch_and_store_metadata(
        &state,
        &app_data_dir,
        game_id,
        &lookup_name,
        platform,
        &settings,
        wervik_access_token.as_deref(),
        true,
    )
}

/// Returns the cached Nexus Mods trending feed for the selected game, or
/// fetches it on demand when the cache is older than a few hours. A stale
/// result is kept as a fallback when the network is temporarily unavailable.
#[tauri::command(async)]
pub fn get_nexus_mods(
    state: State<AppStateHandle>,
    game_name: String,
) -> Result<Option<Vec<NexusMod>>, String> {
    let Some(domain) = nexus::domain_for_game(&game_name) else {
        return Ok(None);
    };
    let (fresh, stale) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        (
            state.nexus_cache.get_fresh(domain),
            state.nexus_cache.get(domain),
        )
    };
    if let Some(mods) = fresh {
        return Ok(Some(mods));
    }

    match nexus::fetch_trending(domain) {
        Ok(mods) => {
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            state.nexus_cache.put(domain.to_string(), mods.clone())?;
            Ok(Some(mods))
        }
        Err(error) => stale.map_or(Err(error), |mods| Ok(Some(mods))),
    }
}

/// Returns cached RetroAchievements progress for a ROM, or fetches it on
/// demand when the cache is stale (or `force` is set — the manual refresh
/// button, or right after a play session on this game ends). `Ok(None)`
/// covers every "nothing to show" case — the game isn't a ROM, no
/// RetroAchievements credentials are configured in Settings, or
/// RetroAchievements simply has no matching console/title (see
/// `retroachievements::fetch_for_game`) — none of which are user-facing
/// errors.
#[tauri::command(async)]
pub fn get_retro_achievements(
    state: State<AppStateHandle>,
    game_id: String,
    force: bool,
) -> Result<Option<GameAchievements>, String> {
    let (rom, username, api_key, fresh, stale) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(rom) = state.rom_games.iter().find(|r| r.id == game_id).cloned() else {
            return Ok(None);
        };
        let settings = state.store.settings();
        (
            rom,
            settings.ra_username,
            settings.ra_api_key,
            state.retro_achievements_cache.get_fresh(&game_id),
            state.retro_achievements_cache.get(&game_id),
        )
    };
    if username.trim().is_empty() || api_key.trim().is_empty() {
        return Ok(None);
    }
    if !force {
        if let Some(achievements) = fresh {
            return Ok(Some(achievements));
        }
    }

    match retroachievements::fetch_for_game(&username, &api_key, rom.platform, &rom.name) {
        Ok(Some(achievements)) => {
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            state
                .retro_achievements_cache
                .put(game_id, achievements.clone())?;
            let app_handle = state.app_handle.clone();
            drop(state);
            schedule_wervik_cloud_sync(&app_handle);
            Ok(Some(achievements))
        }
        Ok(None) => Ok(None),
        Err(error) => stale.map_or(Err(error), |achievements| Ok(Some(achievements))),
    }
}

/// Steam counterpart to `get_retro_achievements` — matched by the exact
/// Steam app id instead of a fuzzy title/console guess, so `Ok(None)` only
/// ever means "this game has no achievements at all". A real `Err` (most
/// commonly a private Steam profile) is left for the frontend to surface,
/// since — unlike a RetroAchievements mismatch — it's something the user
/// can go fix.
#[tauri::command(async)]
pub fn get_steam_achievements(
    state: State<AppStateHandle>,
    game_id: String,
    force: bool,
) -> Result<Option<steam_achievements::GameAchievements>, String> {
    let (app_id, credentials, steam_id64, fresh, stale) = {
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(app_id) = state
            .games
            .iter()
            .find(|g| g.app_id.to_string() == game_id)
            .map(|g| g.app_id)
        else {
            return Ok(None);
        };
        let credentials = state.steam_credentials();
        let steam_id64 = state.steam_id64();
        if let Some(steam_id64) = steam_id64.as_deref() {
            state.steam_achievements_cache.select_account(steam_id64)?;
        }
        let fresh = steam_id64
            .as_deref()
            .and_then(|id| state.steam_achievements_cache.get_fresh(&game_id, id));
        let stale = steam_id64
            .as_deref()
            .and_then(|id| state.steam_achievements_cache.get(&game_id, id));
        (app_id, credentials, steam_id64, fresh, stale)
    };
    let Some(steam_id64) = steam_id64.filter(|id| !id.trim().is_empty()) else {
        return Ok(None);
    };
    let Some(credentials) = credentials else {
        return Ok(None);
    };
    if !force {
        if let Some(achievements) = fresh {
            return Ok(Some(achievements));
        }
    }

    match steam_achievements::fetch_for_game(&credentials, &steam_id64, app_id) {
        Ok(Some(achievements)) => {
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            state
                .steam_achievements_cache
                .put(game_id, &steam_id64, achievements.clone())?;
            let app_handle = state.app_handle.clone();
            drop(state);
            schedule_wervik_cloud_sync(&app_handle);
            Ok(Some(achievements))
        }
        Ok(None) => {
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            state
                .steam_achievements_cache
                .mark_miss(game_id, &steam_id64)?;
            Ok(None)
        }
        Err(error) => stale.map_or(Err(error), |achievements| Ok(Some(achievements))),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameAchievementSummary {
    pub game_id: String,
    pub game_name: String,
    pub source: String,
    /// The retro platform's short label (e.g. "SNES") — same convention as
    /// `GameDto.platform` (see `retro_to_dto`) — or `None` for a Steam game,
    /// used to group the Platforms breakdown since Steam achievements
    /// aren't tied to any one Wervik `Platform`.
    pub platform: Option<String>,
    /// The same platform's stable `Platform::key()` (e.g. "snes") — lets the
    /// frontend look up a console icon without re-deriving one from
    /// `platform`'s display name.
    pub platform_key: Option<String>,
    pub icon_url: Option<String>,
    pub earned_count: usize,
    pub total_count: usize,
    /// 0 for Steam, which has no points concept.
    pub points_earned: u32,
    pub points_total: u32,
    pub completion_percent: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentUnlock {
    pub game_id: String,
    pub game_name: String,
    pub source: String,
    pub achievement_title: String,
    pub achievement_description: String,
    pub icon_url: String,
    /// 0 for Steam, which has no points concept.
    pub points: u32,
    pub unlocked_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementsOverview {
    pub total_earned: usize,
    pub total_possible: usize,
    pub total_points_earned: u32,
    pub total_points_possible: u32,
    pub games: Vec<GameAchievementSummary>,
    pub recent_unlocks: Vec<RecentUnlock>,
}

/// Newest-first unlocks shown on the Achievements overview — capped since a
/// large library's full unlock history isn't useful past a recent window.
const RECENT_UNLOCKS_LIMIT: usize = 60;

/// Aggregates every game's already-cached achievement progress (RetroAchievements
/// + Steam) into one cross-library view for the Achievements screen. Reads
/// only the existing per-game caches — no network requests here — since
/// both are already kept warm by their own background sweeps (see
/// `retroachievements::spawn_fetch`/`steam_achievements::spawn_fetch`), the
/// same data `library_snapshot` already draws each game's completion badge
/// from.
#[tauri::command(async)]
pub fn get_achievements_overview(state: State<AppStateHandle>) -> AchievementsOverview {
    let state = state.lock().unwrap_or_else(|e| e.into_inner());

    let mut games = Vec::new();
    let mut recent_unlocks = Vec::new();
    let mut total_earned = 0usize;
    let mut total_possible = 0usize;
    let mut total_points_earned = 0u32;
    let mut total_points_possible = 0u32;

    for rom in &state.rom_games {
        let Some(achievements) = state.retro_achievements_cache.get(&rom.id) else {
            continue;
        };
        if achievements.total_count == 0 {
            continue;
        }
        total_earned += achievements.earned_count;
        total_possible += achievements.total_count;
        total_points_earned += achievements.points_earned;
        total_points_possible += achievements.points_total;
        games.push(GameAchievementSummary {
            game_id: rom.id.clone(),
            game_name: rom.name.clone(),
            source: "retro".to_string(),
            platform: Some(rom.platform.short_name().to_string()),
            platform_key: Some(rom.platform.key().to_string()),
            icon_url: achievements.icon_url.clone(),
            earned_count: achievements.earned_count,
            total_count: achievements.total_count,
            points_earned: achievements.points_earned,
            points_total: achievements.points_total,
            completion_percent: achievements.completion_percent().unwrap_or(0),
        });
        for achievement in &achievements.achievements {
            if let Some(unlocked_at) = achievement.unlocked_at {
                recent_unlocks.push(RecentUnlock {
                    game_id: rom.id.clone(),
                    game_name: rom.name.clone(),
                    source: "retro".to_string(),
                    achievement_title: achievement.title.clone(),
                    achievement_description: achievement.description.clone(),
                    icon_url: achievement.badge_url.clone(),
                    points: achievement.points,
                    unlocked_at,
                });
            }
        }
    }

    if let Some(steam_id64) = state.steam_id64() {
        for game in &state.games {
            let game_id = game.app_id.to_string();
            let Some(achievements) = state.steam_achievements_cache.get(&game_id, &steam_id64)
            else {
                continue;
            };
            if achievements.total_count == 0 {
                continue;
            }
            total_earned += achievements.earned_count;
            total_possible += achievements.total_count;
            games.push(GameAchievementSummary {
                game_id: game_id.clone(),
                game_name: game.name.clone(),
                source: "steam".to_string(),
                platform: None,
                platform_key: None,
                icon_url: None,
                earned_count: achievements.earned_count,
                total_count: achievements.total_count,
                points_earned: 0,
                points_total: 0,
                completion_percent: achievements.completion_percent().unwrap_or(0),
            });
            for achievement in &achievements.achievements {
                if let Some(unlocked_at) = achievement.unlocked_at {
                    recent_unlocks.push(RecentUnlock {
                        game_id: game_id.clone(),
                        game_name: game.name.clone(),
                        source: "steam".to_string(),
                        achievement_title: achievement.title.clone(),
                        achievement_description: achievement.description.clone(),
                        icon_url: achievement.icon_url.clone(),
                        points: 0,
                        unlocked_at,
                    });
                }
            }
        }
    }

    games.sort_by(|a, b| {
        b.completion_percent
            .cmp(&a.completion_percent)
            .then_with(|| a.game_name.cmp(&b.game_name))
    });
    recent_unlocks.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
    recent_unlocks.truncate(RECENT_UNLOCKS_LIMIT);

    AchievementsOverview {
        total_earned,
        total_possible,
        total_points_earned,
        total_points_possible,
        games,
        recent_unlocks,
    }
}

/// Account-wide RetroAchievements rank/points for the Achievements screen's
/// header. Unlike `get_achievements_overview`, this is a real network
/// request — RA has no local cache for it — but it's cheap: one request,
/// not one per game.
#[tauri::command(async)]
pub fn get_ra_profile_summary(
    state: State<AppStateHandle>,
) -> Result<Option<RaProfileSummary>, String> {
    let settings = state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .store
        .settings();
    let username = settings.ra_username.trim().to_string();
    let api_key = settings.ra_api_key.trim().to_string();
    if username.is_empty() || api_key.is_empty() {
        return Ok(None);
    }
    retroachievements::fetch_profile_summary(&username, &api_key).map(Some)
}

#[tauri::command(async)]
pub fn rescan_library(state: State<AppStateHandle>) -> LibrarySnapshot {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    // Re-detect Steam in case it was installed/moved since last scan.
    state.steam_install = discovery::find_steam_install();
    state.rescan();
    let app_handle = state.app_handle.clone();
    let snapshot = library_snapshot(&state);
    drop(state);
    schedule_wervik_cloud_sync(&app_handle);
    snapshot
}

#[tauri::command(async)]
pub fn get_rom_directories(state: State<AppStateHandle>) -> Vec<RomDirectoryConfig> {
    state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .store
        .rom_directories()
}

#[tauri::command(async)]
pub fn add_rom_directory(
    state: State<AppStateHandle>,
    path: String,
    platform_key: Option<String>,
) -> LibrarySnapshot {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.store.add_rom_directory(path, platform_key);
    state.spawn_rom_scan();
    library_snapshot(&state)
}

/// Sets (or, with `platform_key: None`, clears) an already-added
/// directory's platform override — see `storage::RomDirectoryConfig`.
/// Always rescans afterward: changing the override changes how every file
/// (or, for a tagged directory, every folder) underneath it is
/// interpreted, not just newly added ones.
#[tauri::command(async)]
pub fn set_rom_directory_platform(
    state: State<AppStateHandle>,
    path: String,
    platform_key: Option<String>,
) -> LibrarySnapshot {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.store.set_rom_directory_platform(&path, platform_key);
    state.spawn_rom_scan();
    library_snapshot(&state)
}

#[tauri::command(async)]
pub fn remove_rom_directory(state: State<AppStateHandle>, path: String) -> LibrarySnapshot {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.store.remove_rom_directory(&path);
    state.spawn_rom_scan();
    library_snapshot(&state)
}

#[tauri::command(async)]
pub fn rescan_retro_library(state: State<AppStateHandle>) -> LibrarySnapshot {
    let state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.spawn_rom_scan();
    library_snapshot(&state)
}

/// Whether a ROM directory scan (see `spawn_rom_scan`) is currently
/// running — polled once on mount the same way `get_metadata_progress`/
/// `get_artwork_progress` are, in case the scan was already underway
/// before the frontend subscribed to the `rom-scan-status` event.
#[tauri::command(async)]
pub fn get_rom_scan_status(state: State<AppStateHandle>) -> bool {
    state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .rom_scan_running
        .load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum RetroLaunch {
    /// Launched as a real, separate OS process (see `retro::native`) —
    /// there's no session/URL for the frontend to do anything with; it
    /// just stays where it is while the emulator's own window runs.
    Native,
}

/// Validates the ROM (and BIOS, if the platform needs one), then launches
/// the selected native emulator as its own process. Wervik tracks that
/// process and records playtime when it exits.
#[tauri::command(async)]
pub fn launch_retro_game(
    state_handle: State<AppStateHandle>,
    id: String,
    variant_id: Option<String>,
) -> Result<RetroLaunch, String> {
    let state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
    let rom = state
        .rom_games
        .iter()
        .find(|g| g.id == id)
        .cloned()
        .ok_or_else(|| {
            "This game's ROM could not be found. Try rescanning your ROM folders.".to_string()
        })?;

    // `variant_id`, when present, picks a different regional dump of this
    // same game to actually play (see `retro::discovery::RomVariant`) —
    // Favorites and playtime still key off `rom`/`id`, while BIOS and
    // emulator settings follow the platform shared by all regional copies.
    let rom_path = match &variant_id {
        Some(variant_id) => rom
            .variants
            .iter()
            .find(|v| &v.id == variant_id)
            .map(|v| v.rom_path.clone())
            .ok_or_else(|| {
                "This version of the game could not be found. Try rescanning.".to_string()
            })?,
        None => rom.rom_path.clone(),
    };

    if !Path::new(&rom_path).exists() {
        return Err(format!(
            "\"{}\" no longer exists on disk. Try rescanning.",
            rom.name
        ));
    }

    let bios_path = state.store.bios_path(rom.platform.key()).map(PathBuf::from);
    if rom.platform.requires_bios() && bios_path.is_none() {
        return Err(format!(
            "{} needs a BIOS file. Add one in this game's or platform's Setup before launching.",
            rom.platform.display_name()
        ));
    }

    if let Some(emulator) =
        resolve_effective_native_emulator(&state.app_data_dir, rom.platform, &state.store)
    {
        let rom_source_path = Path::new(&rom_path);
        // Native emulators (e.g. gopher64) can't read a ROM straight out of
        // a zip themselves — extract it to a scratch dir first and clean it
        // up once the emulator exits.
        let is_zip = rom_source_path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        let (native_rom_path, cleanup_dir) = if is_zip && rom.platform != Platform::Arcade {
            let extract_dir = state.app_data_dir.join("native-rom-cache").join(&id);
            let extracted = retro_discovery::extract_rom_from_zip(rom_source_path, &extract_dir)
                .map_err(|e| format!("Could not extract ROM for the native emulator: {e}"))?;
            (extracted, Some(extract_dir))
        } else {
            (rom_source_path.to_path_buf(), None)
        };

        // A no-op for emulators that don't need a BIOS placed this way
        // (see `retro_native::NativeEmulator::bios_relpath`) — done on
        // every launch, not just once when the BIOS is set, so switching
        // which file is configured always takes effect on the next play.
        if let Some(bios_path) = &bios_path {
            retro_native::install_bios(&state.app_data_dir, &emulator, bios_path)?;
        }

        let app_data_dir = state.app_data_dir.clone();
        let game_setup_dir = state.game_setup_dir.clone();
        let app_handle = state.app_handle.clone();
        let platform_key = rom.platform.key().to_string();
        let settings = state.store.settings();
        let maximize_window = settings.maximize_games;
        let hardcore_mode = state.store.is_retro_hardcore(&id);
        let ra_username = settings.ra_username;
        let ra_password = settings.ra_password;
        let is_retroarch = emulator.id.starts_with("retroarch-");
        let is_pcsx2 = emulator.id == "pcsx2";
        let account_token = state
            .store
            .wervik_account()
            .map(|account| account.access_token)
            .filter(|token| !token.trim().is_empty());
        drop(state);

        let achievement_config = if is_retroarch {
            retro_native::configure_retroarch_achievements(
                &app_data_dir,
                &ra_username,
                &ra_password,
                hardcore_mode,
            )
        } else if is_pcsx2 {
            retro_native::configure_pcsx2_achievements(
                &app_data_dir,
                &ra_username,
                &ra_password,
                hardcore_mode,
            )
        } else {
            Ok(())
        };
        if let Err(error) = achievement_config {
            if let Some(dir) = &cleanup_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            return Err(error);
        }

        if is_retroarch {
            if let Some(token) = account_token {
                if let Err(error) =
                    saves::restore_game(&app_data_dir, &token, &id, &platform_key, &native_rom_path)
                {
                    eprintln!("RetroArch save restore failed for {id}: {error}");
                }
            }
        }

        let state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
        let playtime_session_id = state.playtime.start_session(&id, "retro", now_unix())?;
        let child = match retro_native::spawn(
            &app_data_dir,
            emulator,
            &native_rom_path,
            &game_setup_dir,
            &platform_key,
            maximize_window,
        ) {
            Ok(child) => child,
            Err(error) => {
                let _ = state
                    .playtime
                    .cancel_session(playtime_session_id, now_unix());
                return Err(error);
            }
        };
        if maximize_window {
            window::spawn_maximizer(child.id());
        }
        std::thread::spawn(move || {
            let mut child = child;
            let _ = child.wait();
            if is_retroarch {
                if let Err(error) = saves::sync_game_from_runtime(
                    &app_handle,
                    &app_data_dir,
                    &id,
                    &platform_key,
                    &native_rom_path,
                ) {
                    eprintln!("RetroArch save sync failed for {id}: {error}");
                }
            }
            if let Some(dir) = cleanup_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            finish_playtime_session(&app_handle, playtime_session_id);
        });
        return Ok(RetroLaunch::Native);
    }

    Err(format!(
        "No compatible native emulator is installed for {}. Open Platforms → {} and install one first.",
        rom.platform.display_name(),
        rom.platform.display_name()
    ))
}

/// Opens the folder containing a ROM in the system file manager — the
/// retro-game equivalent of `open_install_folder`.
#[tauri::command(async)]
pub fn open_rom_folder(state: State<AppStateHandle>, id: String) -> Result<(), String> {
    let (app, path) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let path = state
            .rom_games
            .iter()
            .find(|g| g.id == id)
            .and_then(|g| Path::new(&g.rom_path).parent().map(|p| p.to_path_buf()));
        (state.app_handle.clone(), path)
    };
    let Some(path) = path else {
        return Err("ROM not found".into());
    };
    platform::open_path(&app, &path)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformStatus {
    /// Stable key for `set_default_emulator` — see `Platform::key`.
    pub key: String,
    pub display_name: String,
    /// Short label (e.g. "N64", "SNES") — used as the Platforms grid
    /// card's stable monogram, a fixed visual identity rather than
    /// whichever cover a random game on that platform happens to have.
    pub short_name: String,
    pub rom_count: usize,
    /// Every native emulator Wervik knows how to launch for this platform.
    pub emulator_options: Vec<EmulatorOption>,
    /// The user's chosen default (see `selected_emulator_id`) — always one
    /// of `emulator_options`' ids, even if that option is currently
    /// unavailable (the UI shows that state rather than hiding it).
    pub selected_emulator: String,
    /// What will actually be used right now (see `effective_emulator_id`).
    /// `None` when the selected emulator is not installed yet.
    pub effective_emulator: Option<String>,
    /// Whether games on this platform need a user-supplied BIOS file.
    pub requires_bios: bool,
    /// Shared BIOS path configured for this platform, if any.
    pub bios_path: Option<String>,
    /// `"original"` or `"optimized"` (see `retro_native::apply_graphics_preset`)
    /// for the platforms whose native RetroArch core can meaningfully
    /// render above the original hardware's resolution — `None` for every
    /// other platform, which has no such preset to offer at all.
    pub graphics_preset: Option<String>,
}

/// One row per platform Wervik knows about (see `Platform::ALL`), for the
/// Platforms view and Platform Setup — every emulator choice, which one is
/// selected vs. actually in effect, and how many ROMs are currently in the
/// library for it.
#[tauri::command(async)]
pub fn get_platform_status(state: State<AppStateHandle>) -> Vec<PlatformStatus> {
    let state = state.lock().unwrap_or_else(|e| e.into_inner());
    Platform::ALL
        .iter()
        .map(|&platform| PlatformStatus {
            key: platform.key().to_string(),
            display_name: platform.display_name().to_string(),
            short_name: platform.short_name().to_string(),
            rom_count: state
                .rom_games
                .iter()
                .filter(|g| g.platform == platform)
                .count(),
            emulator_options: emulator_options_for(&state.app_data_dir, platform),
            selected_emulator: selected_emulator_id(&state.app_data_dir, platform, &state.store),
            effective_emulator: effective_emulator_id(&state.app_data_dir, platform, &state.store),
            requires_bios: platform.requires_bios(),
            bios_path: state.store.bios_path(platform.key()),
            graphics_preset: retro_native::supports_graphics_preset(platform)
                .then(|| state.store.graphics_preset(platform.key())),
        })
        .collect()
}

/// Sets a platform's graphics preset — see `PlatformStatus.graphicsPreset`
/// and `retro_native::apply_graphics_preset` for what "optimized" actually
/// changes. Errors for a platform with no preset to offer at all.
#[tauri::command(async)]
pub fn set_graphics_preset(
    state: State<AppStateHandle>,
    platform: String,
    optimized: bool,
) -> Result<(), String> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    let platform_enum = Platform::from_key(&platform)
        .ok_or_else(|| format!("\"{platform}\" is not a known platform."))?;
    retro_native::apply_graphics_preset(&state.app_data_dir, platform_enum, optimized)?;
    state
        .store
        .set_graphics_preset(&platform, if optimized { "optimized" } else { "original" });
    Ok(())
}

/// Sets the platform default emulator (see `PlatformStatus.selectedEmulator`).
/// Takes effect immediately — nothing to "Apply", since picking a different
/// default is a simple, safe, instantly-reversible choice, not a batch of
/// settings like the Setup drawer's.
#[tauri::command(async)]
pub fn set_default_emulator(state: State<AppStateHandle>, platform: String, emulator_id: String) {
    state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .store
        .set_default_emulator(&platform, &emulator_id);
}

/// Downloads and installs a `Launcher::Managed` native emulator (see
/// `EmulatorOption.managed`). Blocks
/// the caller for the duration of the download (tens of MB); the frontend
/// shows its own "Downloading…" state rather than Wervik reporting
/// progress, since this is a one-shot action, not a background job.
#[tauri::command(async)]
pub fn install_native_emulator(
    state: State<AppStateHandle>,
    platform: String,
    emulator_id: String,
) -> Result<(), String> {
    let app_data_dir = state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .app_data_dir
        .clone();
    let platform = Platform::from_key(&platform)
        .ok_or_else(|| format!("\"{platform}\" is not a known platform."))?;
    retro_native::install(&app_data_dir, platform, &emulator_id)
}

/// Opens a native emulator's own UI with no ROM loaded, so the user can
/// use its real controller/video/audio settings — functionality Wervik
/// doesn't reimplement or fake a control for (see `retro::native::open_settings`).
/// Fire-and-forget from the caller's perspective: Wervik just reaps the
/// process on exit so it doesn't linger as a zombie, nothing more to track.
#[tauri::command(async)]
pub fn open_native_emulator_settings(
    state: State<AppStateHandle>,
    platform: String,
    emulator_id: String,
) -> Result<(), String> {
    let (app_data_dir, emulator) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let platform_enum = Platform::from_key(&platform)
            .ok_or_else(|| format!("\"{platform}\" is not a known platform."))?;
        let emulator = retro_native::native_emulators_for(platform_enum)
            .into_iter()
            .find(|e| e.id == emulator_id)
            .ok_or_else(|| {
                format!("\"{emulator_id}\" is not a known emulator for this platform.")
            })?;
        (state.app_data_dir.clone(), emulator)
    };
    let mut child = retro_native::open_settings(&app_data_dir, emulator)?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Same shape as `GameSetupStatus` minus the Steam-only Launch Options
/// fields — a native emulator launch is a command Wervik composes and
/// runs itself, so there's no external app to "configure" the way Steam's
/// Launch Options dialog needs to be.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformSetupStatus {
    pub mangohud_installed: bool,
    pub mangohud_enabled: bool,
    pub mangohud_config: MangoHudConfig,
    pub gamemode_installed: bool,
    pub gamemode_enabled: bool,
    pub gamescope_installed: bool,
    pub gamescope_enabled: bool,
    pub gamescope_config: GamescopeConfig,
    pub env_vars: Vec<EnvVar>,
    pub has_custom_config: bool,
}

fn platform_setup_status(dir: &Path, platform_key: &str) -> PlatformSetupStatus {
    let mangohud_enabled = mangohud::is_enabled(dir, platform_key);
    let gamemode_enabled = game_setup::is_gamemode_enabled(dir, platform_key);
    let gamescope_enabled = game_setup::is_gamescope_enabled(dir, platform_key);
    let env_vars = game_setup::read_env_vars(dir, platform_key);

    PlatformSetupStatus {
        mangohud_installed: mangohud::is_installed(),
        mangohud_enabled,
        mangohud_config: mangohud::read_config(dir, platform_key),
        gamemode_installed: game_setup::is_gamemode_installed(),
        gamemode_enabled,
        gamescope_installed: game_setup::is_gamescope_installed(),
        gamescope_enabled,
        gamescope_config: game_setup::read_gamescope_config(dir, platform_key),
        has_custom_config: mangohud_enabled
            || gamemode_enabled
            || gamescope_enabled
            || !env_vars.is_empty(),
        env_vars,
    }
}

#[tauri::command(async)]
pub fn get_platform_setup_status(
    state: State<AppStateHandle>,
    platform: String,
) -> PlatformSetupStatus {
    let state = state.lock().unwrap_or_else(|e| e.into_inner());
    platform_setup_status(&state.game_setup_dir, &platform)
}

/// Persists MangoHud/GameMode/Gamescope/env-var settings that apply to
/// every game on this platform launched through a native emulator (see
/// `retro::native`). Takes effect on the *next* launch — nothing to
/// regenerate or paste anywhere, unlike Steam's wrapper-script flow.
#[tauri::command(async)]
pub fn apply_platform_setup(
    state: State<AppStateHandle>,
    platform: String,
    input: GameSetupInput,
) -> Result<PlatformSetupStatus, String> {
    let state = state.lock().unwrap_or_else(|e| e.into_inner());
    let dir = &state.game_setup_dir;

    mangohud::write_config(dir, &platform, &input.mangohud_config)?;
    mangohud::set_enabled(dir, &platform, input.mangohud_enabled)?;
    game_setup::set_gamemode_enabled(dir, &platform, input.gamemode_enabled)?;
    game_setup::write_gamescope_config(dir, &platform, &input.gamescope_config)?;
    game_setup::set_gamescope_enabled(dir, &platform, input.gamescope_enabled)?;
    game_setup::write_env_vars(dir, &platform, &input.env_vars)?;

    Ok(platform_setup_status(dir, &platform))
}

#[tauri::command(async)]
pub fn get_bios_path(state: State<AppStateHandle>, platform: String) -> Option<String> {
    state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .store
        .bios_path(&platform)
}

/// Copies the user-selected BIOS file into Wervik's own app data directory
/// and remembers it against this platform. Wervik never fetches a BIOS
/// itself — the source path always comes from the user picking a file.
#[tauri::command(async)]
pub fn set_bios_path(
    state: State<AppStateHandle>,
    platform: String,
    source_path: String,
) -> Result<String, String> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    let dest = bios::install(&state.app_data_dir.clone(), Path::new(&source_path))?;
    let dest_str = dest.to_string_lossy().to_string();
    state.store.set_bios_path(&platform, Some(dest_str.clone()));
    Ok(dest_str)
}

#[tauri::command(async)]
pub fn clear_bios_path(state: State<AppStateHandle>, platform: String) {
    state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .store
        .set_bios_path(&platform, None);
}

#[tauri::command(async)]
pub fn launch_game(state: State<AppStateHandle>, app_id: u32) -> Result<(), String> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    if !state.games.iter().any(|game| game.app_id == app_id) {
        return Err("This Steam game is not installed yet. Use Install first.".into());
    }
    let launch_kind = {
        let launch_kind = state
            .steam_install
            .as_ref()
            .map(|i| i.launch_kind)
            .unwrap_or_else(|| {
                #[cfg(target_os = "windows")]
                {
                    LaunchKind::Windows
                }
                #[cfg(not(target_os = "windows"))]
                {
                    LaunchKind::Native
                }
            });
        launch_kind
    };
    launch::launch(&state.app_handle, app_id, launch_kind)?;
    if let Some(game) = state
        .games
        .iter()
        .find(|game| game.app_id == app_id)
        .cloned()
    {
        if let Err(error) =
            start_process_session(&mut state, &app_id.to_string(), "steam", &game.install_dir)
        {
            eprintln!(
                "Could not start Steam playtime tracking for {}: {error}",
                game.name
            );
        }
    }
    Ok(())
}

#[tauri::command(async)]
pub fn install_steam_game(state: State<AppStateHandle>, app_id: u32) -> Result<(), String> {
    let state = state.lock().unwrap_or_else(|e| e.into_inner());
    if state.games.iter().any(|game| game.app_id == app_id) {
        return Err("This Steam game is already installed.".into());
    }
    if !state
        .steam_owned_games
        .iter()
        .any(|game| game.app_id == app_id)
    {
        return Err(
            "Steam game not found in the owned games list. Try rescanning your library.".into(),
        );
    }
    let launch_kind = state
        .steam_install
        .as_ref()
        .map(|install| install.launch_kind)
        .unwrap_or_else(|| {
            #[cfg(target_os = "windows")]
            {
                LaunchKind::Windows
            }
            #[cfg(not(target_os = "windows"))]
            {
                LaunchKind::Native
            }
        });
    launch::install(&state.app_handle, app_id, launch_kind)
}

/// Launches a locally installed Xbox App / PC Game Pass title through its
/// Windows AppUserModelID. The ID comes from the local app registration scan,
/// so this does not need access to the protected game executable directory.
#[tauri::command(async)]
pub fn launch_xbox_game(state: State<AppStateHandle>, id: String) -> Result<(), String> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    let (aumid, install_dir, name) = state
        .xbox_games
        .iter()
        .find(|game| game.id == id)
        .map(|game| {
            (
                game.aumid.clone(),
                game.install_dir.clone(),
                game.name.clone(),
            )
        })
        .ok_or_else(|| "Xbox game not found. Try rescanning your library.".to_string())?;
    crate::xbox::launch::launch(&aumid)?;
    state.store.record_local_launch(&id, now_unix());
    if let Err(error) = start_process_session(&mut state, &id, "xbox", &install_dir) {
        eprintln!("Could not start Xbox playtime tracking for {name}: {error}");
    }
    Ok(())
}

/// Launches an installed Battle.net title through the Battle.net client.
#[tauri::command(async)]
pub fn launch_battlenet_game(state: State<AppStateHandle>, id: String) -> Result<(), String> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    let (product_code, install_dir, name) = state
        .battlenet_games
        .iter()
        .find(|game| game.id == id)
        .map(|game| {
            (
                game.launch_code.clone(),
                game.install_dir.clone(),
                game.name.clone(),
            )
        })
        .ok_or_else(|| "Battle.net game not found. Try rescanning your library.".to_string())?;
    crate::battlenet::launch::launch(&product_code)?;
    state.store.record_local_launch(&id, now_unix());
    if let Err(error) = start_process_session(&mut state, &id, "battlenet", &install_dir) {
        eprintln!("Could not start Battle.net playtime tracking for {name}: {error}");
    }
    Ok(())
}

/// Launches an installed EA App title through the EA client.
#[tauri::command(async)]
pub fn launch_ea_game(state: State<AppStateHandle>, id: String) -> Result<(), String> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    let (launch_id, install_dir, name) = state
        .ea_games
        .iter()
        .find(|game| game.id == id)
        .map(|game| {
            (
                game.launch_id.clone(),
                game.install_dir.clone(),
                game.name.clone(),
            )
        })
        .ok_or_else(|| "EA game not found. Try rescanning your library.".to_string())?;
    crate::ea::launch::launch(&state.app_handle, &launch_id)?;
    state.store.record_local_launch(&id, now_unix());
    if let Err(error) = start_process_session(&mut state, &id, "ea", &install_dir) {
        eprintln!("Could not start EA playtime tracking for {name}: {error}");
    }
    Ok(())
}

#[tauri::command(async)]
pub fn open_steam_page(state: State<AppStateHandle>, app_id: u32) -> Result<(), String> {
    let (app, launch_kind) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let launch_kind = state
            .steam_install
            .as_ref()
            .map(|i| i.launch_kind)
            .unwrap_or_else(|| {
                #[cfg(target_os = "windows")]
                {
                    LaunchKind::Windows
                }
                #[cfg(not(target_os = "windows"))]
                {
                    LaunchKind::Native
                }
            });
        (state.app_handle.clone(), launch_kind)
    };
    launch::open_store_page(&app, app_id, launch_kind)
}

/// `id` is the unified game id (see `GameDto::id`) — works for both Steam
/// and retro games since favorites are tracked across sources.
#[tauri::command(async)]
pub fn set_favorite(state: State<AppStateHandle>, id: String, favorite: bool) {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.store.set_favorite(&id, favorite);
    let app_handle = state.app_handle.clone();
    drop(state);
    schedule_wervik_cloud_sync(&app_handle);
}

/// Stores a local like/dislike preference for a game. Passing `null` clears
/// the current reaction, while the two accepted values are kept deliberately
/// small so saved state remains easy to inspect and migrate.
#[tauri::command(async)]
pub fn set_game_reaction(
    state: State<AppStateHandle>,
    id: String,
    reaction: Option<String>,
) -> Result<(), String> {
    if let Some(value) = &reaction {
        if value != "like" && value != "dislike" {
            return Err("Game reaction must be like or dislike".into());
        }
    }
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.store.set_game_reaction(&id, reaction);
    let app_handle = state.app_handle.clone();
    drop(state);
    schedule_wervik_cloud_sync(&app_handle);
    Ok(())
}

/// Stores RetroAchievements Hardcore mode for one retro game. The setting is
/// applied to the selected emulator immediately before that game's launch,
/// so another retro game can keep normal mode.
#[tauri::command(async)]
pub fn set_retro_hardcore_mode(
    state: State<AppStateHandle>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    if !state.rom_games.iter().any(|game| game.id == id) {
        return Err("This retro game could not be found.".into());
    }
    state.store.set_retro_hardcore(&id, enabled);
    Ok(())
}

const WERVIK_WEB_URL: &str = "https://wervik.wergely.com";
const WERVIK_REDIRECT_SCHEME: &str = if cfg!(debug_assertions) {
    "wervik-dev"
} else {
    "wervik"
};
const WERVIK_ACCOUNT_UPDATED_EVENT: &str = "wervik-account-updated";
const WERVIK_ACCOUNT_ERROR_EVENT: &str = "wervik-account-error";
const WERVIK_SETTINGS_SYNC_ERROR_EVENT: &str = "wervik-settings-sync-error";
const WERVIK_CLOUD_SYNC_ERROR_EVENT: &str = "wervik-cloud-sync-error";
const WERVIK_CONNECT_DURATION_S: i64 = 5 * 60;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopConnectExchangeRequest {
    code: String,
    state: String,
    code_verifier: String,
    installation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopConnectExchangeResponse {
    authenticated: bool,
    name: String,
    email: String,
    installation_id: String,
    access_token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WervikSettingsRequest<'a> {
    settings: &'a Settings,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WervikCloudLibraryGame {
    source: String,
    source_game_id: String,
    igdb_id: Option<u64>,
    name: String,
    platform: Option<String>,
    platform_key: Option<String>,
    is_installed: bool,
    is_favorite: bool,
    reaction: Option<String>,
    playtime_seconds: u64,
    last_played: Option<i64>,
    achievement_completion_percent: Option<u8>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WervikCloudPlaytimeSession {
    local_session_id: i64,
    source: String,
    source_game_id: String,
    started_at: i64,
    ended_at: i64,
    duration_seconds: u64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WervikCloudAchievementSnapshot {
    source: String,
    source_game_id: String,
    igdb_id: Option<u64>,
    name: String,
    platform: Option<String>,
    payload: serde_json::Value,
    fetched_at: i64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WervikCloudLibraryRequest {
    games: Vec<WervikCloudLibraryGame>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WervikCloudPlaytimeRequest {
    sessions: Vec<WervikCloudPlaytimeSession>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WervikCloudAchievementsRequest {
    replace: bool,
    snapshots: Vec<WervikCloudAchievementSnapshot>,
}

#[derive(Debug)]
struct WervikCloudSyncPayload {
    library: WervikCloudLibraryRequest,
    playtime: WervikCloudPlaytimeRequest,
    achievements: WervikCloudAchievementsRequest,
}

fn random_connect_value() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn web_connect_url(
    installation_id: &str,
    state: &str,
    code_challenge: &str,
    redirect_scheme: &str,
    redirect_uri: &str,
) -> Result<String, String> {
    let configured_base =
        std::env::var("WERVIK_WEB_URL").unwrap_or_else(|_| WERVIK_WEB_URL.to_string());
    let mut url =
        Url::parse(&configured_base).map_err(|error| format!("Invalid Wervik web URL: {error}"))?;
    url.set_path("/api/connect/desktop/start");
    url.query_pairs_mut()
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("redirect_scheme", redirect_scheme)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("installation_id", installation_id);
    Ok(url.to_string())
}

fn bind_wervik_callback() -> Result<(TcpListener, String), String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("Could not bind the Wervik Account callback: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("Could not read the Wervik Account callback port: {error}"))?
        .port();
    Ok((listener, format!("http://127.0.0.1:{port}/callback")))
}

fn callback_url_from_request(redirect_uri: &str, request_target: &str) -> Result<Url, String> {
    if !request_target.starts_with('/') || request_target.starts_with("//") {
        return Err("The Wervik Account callback request was invalid".into());
    }
    let request_url = Url::parse(&format!("http://callback.invalid{request_target}"))
        .map_err(|error| format!("The Wervik Account callback request was invalid: {error}"))?;
    if request_url.path() != "/callback" {
        return Err("The Wervik Account callback request was invalid".into());
    }
    let mut callback_url = Url::parse(redirect_uri)
        .map_err(|error| format!("The Wervik Account callback URI was invalid: {error}"))?;
    callback_url.set_path(request_url.path());
    callback_url.set_query(request_url.query());
    Ok(callback_url)
}

fn read_local_callback(stream: &mut TcpStream, redirect_uri: &str) -> Result<Url, String> {
    let mut request = Vec::with_capacity(4096);
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("Could not read the Wervik Account callback: {error}"))?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request.len() > 16 * 1024 {
            return Err("The Wervik Account callback request was too large".into());
        }
    }

    let request_line = request
        .split(|byte| *byte == b'\n')
        .next()
        .and_then(|line| std::str::from_utf8(line).ok())
        .map(str::trim_end)
        .ok_or_else(|| "The Wervik Account callback request was invalid".to_string())?;
    let mut fields = request_line.split_ascii_whitespace();
    let method = fields.next();
    let target = fields.next();
    if method != Some("GET") {
        return Err("The Wervik Account callback must use GET".into());
    }
    let Some(target) = target else {
        return Err("The Wervik Account callback request was invalid".into());
    };
    callback_url_from_request(redirect_uri, target)
}

fn write_local_callback_response(stream: &mut TcpStream, status: &str, message: &str) {
    let body =
        format!("<!doctype html><meta charset=\"utf-8\"><title>Wervik</title><p>{message}</p>");
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn local_callback_target(url: &Url) -> String {
    let mut target = url.clone();
    target.set_query(None);
    target.set_fragment(None);
    target.to_string()
}

fn clear_local_pending_wervik_connect(app: &tauri::AppHandle, redirect_uri: &str) {
    let Some(state_handle) = app.try_state::<AppStateHandle>() else {
        return;
    };
    let mut state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
    let is_current_attempt = state
        .store
        .pending_wervik_connect()
        .is_some_and(|pending| pending.redirect_uri.as_deref() == Some(redirect_uri));
    if is_current_attempt {
        state.store.clear_pending_wervik_connect();
    }
}

fn spawn_wervik_callback_listener(
    app: tauri::AppHandle,
    listener: TcpListener,
    redirect_uri: String,
) {
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(WERVIK_CONNECT_DURATION_S as u64);
        if let Err(error) = listener.set_nonblocking(true) {
            eprintln!("Could not prepare the Wervik Account callback: {error}");
            clear_local_pending_wervik_connect(&app, &redirect_uri);
            return;
        }

        let callback = loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let result = read_local_callback(&mut stream, &redirect_uri);
                    match result {
                        Ok(url) => {
                            write_local_callback_response(
                                &mut stream,
                                "200 OK",
                                "Login received. You can return to Wervik.",
                            );
                            break Some(url);
                        }
                        Err(error) => {
                            write_local_callback_response(
                                &mut stream,
                                "400 Bad Request",
                                "The Wervik Account callback request was invalid.",
                            );
                            eprintln!("Ignoring invalid Wervik Account callback request: {error}");
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        clear_local_pending_wervik_connect(&app, &redirect_uri);
                        let _ = app.emit(
                            WERVIK_ACCOUNT_ERROR_EVENT,
                            "The Wervik Account connection timed out.",
                        );
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => {
                    eprintln!("Could not accept the Wervik Account callback: {error}");
                    clear_local_pending_wervik_connect(&app, &redirect_uri);
                    let _ = app.emit(
                        WERVIK_ACCOUNT_ERROR_EVENT,
                        "Could not receive the Wervik Account callback.",
                    );
                    break None;
                }
            }
        };

        if let Some(url) = callback {
            finish_wervik_connect(app, url);
        }
    });
}

fn exchange_desktop_connect(
    code: String,
    state: String,
    pending: PendingWervikConnect,
) -> Result<WervikAccount, String> {
    let configured_base =
        std::env::var("WERVIK_WEB_URL").unwrap_or_else(|_| WERVIK_WEB_URL.to_string());
    let mut url =
        Url::parse(&configured_base).map_err(|error| format!("Invalid Wervik web URL: {error}"))?;
    url.set_path("/api/connect/desktop/exchange");

    let request = DesktopConnectExchangeRequest {
        code,
        state,
        code_verifier: pending.code_verifier,
        installation_id: pending.installation_id,
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let mut response = agent
        .post(url.as_str())
        .send_json(&request)
        .map_err(|error| format!("Could not connect to Wervik Account: {error}"))?;
    let response = response
        .body_mut()
        .read_json::<DesktopConnectExchangeResponse>()
        .map_err(|error| format!("Could not read Wervik Account response: {error}"))?;

    if !response.authenticated {
        return Err("Wervik Account authentication was not completed".into());
    }

    Ok(WervikAccount {
        name: response.name,
        email: response.email,
        connected_at: now_unix(),
        installation_id: response.installation_id,
        access_token: response.access_token,
    })
}

fn sync_wervik_settings(account: &WervikAccount, settings: &Settings) -> Result<(), String> {
    if account.access_token.is_empty() {
        return Ok(());
    }

    let configured_base =
        std::env::var("WERVIK_WEB_URL").unwrap_or_else(|_| WERVIK_WEB_URL.to_string());
    let mut url =
        Url::parse(&configured_base).map_err(|error| format!("Invalid Wervik web URL: {error}"))?;
    url.set_path("/api/settings");

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    agent
        .put(url.as_str())
        .header("Authorization", &format!("Bearer {}", account.access_token))
        .send_json(WervikSettingsRequest { settings })
        .map_err(|error| format!("Could not sync Wervik Account settings: {error}"))?;

    Ok(())
}

fn game_source_name(source: GameSource) -> &'static str {
    match source {
        GameSource::Steam => "steam",
        GameSource::Retro => "retro",
        GameSource::Xbox => "xbox",
        GameSource::BattleNet => "battlenet",
        GameSource::Ea => "ea",
    }
}

fn build_wervik_cloud_sync_payload(
    state: &AppState,
) -> Option<(WervikAccount, WervikCloudSyncPayload)> {
    let account = state
        .store
        .wervik_account()
        .filter(|account| !account.access_token.trim().is_empty())?;
    let snapshot = library_snapshot(state);
    let game_lookup: HashMap<(String, String), &GameDto> = snapshot
        .games
        .iter()
        .map(|game| {
            (
                (game_source_name(game.source).to_string(), game.id.clone()),
                game,
            )
        })
        .collect();

    let library = snapshot
        .games
        .iter()
        .map(|game| {
            let playtime_seconds = state
                .playtime
                .summary(&game.id)
                .ok()
                .flatten()
                .map(|summary| summary.total_seconds)
                .unwrap_or_else(|| game.playtime_minutes.unwrap_or(0).saturating_mul(60));
            WervikCloudLibraryGame {
                source: game_source_name(game.source).to_string(),
                source_game_id: game.id.clone(),
                igdb_id: game.igdb_metadata.as_ref().map(|metadata| metadata.igdb_id),
                name: game.name.clone(),
                platform: game.platform.clone(),
                platform_key: game.platform_key.clone(),
                is_installed: game.is_installed,
                is_favorite: game.is_favorite,
                reaction: game.reaction.clone(),
                playtime_seconds,
                last_played: game.last_played,
                achievement_completion_percent: game.achievement_completion_percent,
            }
        })
        .collect();

    let playtime = state
        .playtime
        .finished_sessions()
        .unwrap_or_default()
        .into_iter()
        .map(
            |session: FinishedSessionRecord| WervikCloudPlaytimeSession {
                local_session_id: session.session_id,
                source: session.source,
                source_game_id: session.game_id,
                started_at: session.started_at,
                ended_at: session.ended_at,
                duration_seconds: session.duration_seconds,
            },
        )
        .collect();

    let mut achievements = Vec::new();
    for (game_id, game_achievements, fetched_at) in state.retro_achievements_cache.snapshots() {
        let Some(game) = game_lookup.get(&("retro".to_string(), game_id.clone())) else {
            continue;
        };
        let Ok(payload) = serde_json::to_value(game_achievements) else {
            continue;
        };
        achievements.push(WervikCloudAchievementSnapshot {
            source: "retro".to_string(),
            source_game_id: game_id,
            igdb_id: game.igdb_metadata.as_ref().map(|metadata| metadata.igdb_id),
            name: game.name.clone(),
            platform: game.platform.clone(),
            payload,
            fetched_at,
        });
    }
    if let Some(steam_id64) = state.steam_id64() {
        for (game_id, game_achievements, fetched_at) in
            state.steam_achievements_cache.snapshots(&steam_id64)
        {
            let Some(game) = game_lookup.get(&("steam".to_string(), game_id.clone())) else {
                continue;
            };
            let Ok(payload) = serde_json::to_value(game_achievements) else {
                continue;
            };
            achievements.push(WervikCloudAchievementSnapshot {
                source: "steam".to_string(),
                source_game_id: game_id,
                igdb_id: game.igdb_metadata.as_ref().map(|metadata| metadata.igdb_id),
                name: game.name.clone(),
                platform: None,
                payload,
                fetched_at,
            });
        }
    }

    Some((
        account,
        WervikCloudSyncPayload {
            library: WervikCloudLibraryRequest { games: library },
            playtime: WervikCloudPlaytimeRequest { sessions: playtime },
            achievements: WervikCloudAchievementsRequest {
                replace: true,
                snapshots: achievements,
            },
        },
    ))
}

fn post_wervik_sync<T: Serialize>(
    agent: &ureq::Agent,
    base_url: &Url,
    account: &WervikAccount,
    path: &str,
    body: T,
) -> Result<(), String> {
    let mut url = base_url.clone();
    url.set_path(path);
    agent
        .post(url.as_str())
        .header("Authorization", &format!("Bearer {}", account.access_token))
        .send_json(body)
        .map_err(|error| format!("Could not sync Wervik Account data: {error}"))?;
    Ok(())
}

fn sync_wervik_cloud(
    account: &WervikAccount,
    payload: &WervikCloudSyncPayload,
    generation: &AtomicUsize,
    generation_id: usize,
) -> Result<(), String> {
    if generation.load(Ordering::SeqCst) != generation_id {
        return Ok(());
    }
    let configured_base =
        std::env::var("WERVIK_WEB_URL").unwrap_or_else(|_| WERVIK_WEB_URL.to_string());
    let base_url =
        Url::parse(&configured_base).map_err(|error| format!("Invalid Wervik web URL: {error}"))?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    if generation.load(Ordering::SeqCst) != generation_id {
        return Ok(());
    }
    post_wervik_sync(
        &agent,
        &base_url,
        account,
        "/api/sync/library",
        &payload.library,
    )?;
    for sessions in payload.playtime.sessions.chunks(1_000) {
        if generation.load(Ordering::SeqCst) != generation_id {
            return Ok(());
        }
        post_wervik_sync(
            &agent,
            &base_url,
            account,
            "/api/sync/playtime",
            WervikCloudPlaytimeRequest {
                sessions: sessions.to_vec(),
            },
        )?;
    }
    for (index, snapshots) in payload.achievements.snapshots.chunks(50).enumerate() {
        if generation.load(Ordering::SeqCst) != generation_id {
            return Ok(());
        }
        post_wervik_sync(
            &agent,
            &base_url,
            account,
            "/api/sync/achievements",
            WervikCloudAchievementsRequest {
                replace: index == 0,
                snapshots: snapshots.to_vec(),
            },
        )?;
    }
    if payload.achievements.snapshots.is_empty() {
        if generation.load(Ordering::SeqCst) != generation_id {
            return Ok(());
        }
        post_wervik_sync(
            &agent,
            &base_url,
            account,
            "/api/sync/achievements",
            WervikCloudAchievementsRequest {
                replace: true,
                snapshots: Vec::new(),
            },
        )?;
    }
    Ok(())
}

pub(crate) fn schedule_wervik_cloud_sync(app: &tauri::AppHandle) {
    let Some(state_handle) = app.try_state::<AppStateHandle>() else {
        return;
    };
    let (running, requested, generation) = {
        let state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
        let Some(account) = state.store.wervik_account() else {
            return;
        };
        if account.access_token.trim().is_empty() {
            return;
        }
        state.cloud_sync_requested.store(true, Ordering::SeqCst);
        (
            state.cloud_sync_running.clone(),
            state.cloud_sync_requested.clone(),
            state.wervik_sync_generation.clone(),
        )
    };
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            requested.store(false, Ordering::SeqCst);
            let (generation_id, payload) =
                app.try_state::<AppStateHandle>()
                    .map_or((0, None), |state_handle| {
                        let state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
                        (
                            state.wervik_sync_generation.load(Ordering::SeqCst),
                            build_wervik_cloud_sync_payload(&state),
                        )
                    });
            if let Some((account, payload)) = payload {
                if let Err(error) =
                    sync_wervik_cloud(&account, &payload, &generation, generation_id)
                {
                    if generation.load(Ordering::SeqCst) != generation_id {
                        continue;
                    }
                    eprintln!("Wervik cloud sync failed: {error}");
                    let _ = app.emit(WERVIK_CLOUD_SYNC_ERROR_EVENT, error);
                }
            }
            if generation.load(Ordering::SeqCst) != generation_id {
                if requested.load(Ordering::SeqCst) {
                    continue;
                }
                break;
            }
            if !requested.load(Ordering::SeqCst) {
                break;
            }
        }
        running.store(false, Ordering::SeqCst);
        if requested.swap(false, Ordering::SeqCst) {
            schedule_wervik_cloud_sync(&app);
        }
    });
}

pub(crate) fn finish_wervik_connect(app: tauri::AppHandle, url: Url) {
    let is_custom_scheme_callback = matches!(url.scheme(), "wervik" | "wervik-dev")
        && url.host_str() == Some("auth")
        && url.path() == "/callback";
    let is_local_callback =
        url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.path() == "/callback";
    if !is_custom_scheme_callback && !is_local_callback {
        return;
    }

    let callback_state = url
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()));
    let error = url
        .query_pairs()
        .find_map(|(key, value)| (key == "error").then(|| value.into_owned()));
    let code = url
        .query_pairs()
        .find_map(|(key, value)| (key == "code").then(|| value.into_owned()));

    let Some(state_handle) = app.try_state::<AppStateHandle>() else {
        return;
    };
    let pending = {
        let mut state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
        let pending = state.store.pending_wervik_connect();
        let valid = pending.as_ref().is_some_and(|pending| {
            let callback_target_matches = if let Some(expected_uri) = &pending.redirect_uri {
                is_local_callback && expected_uri == &local_callback_target(&url)
            } else {
                is_custom_scheme_callback && pending.redirect_scheme == url.scheme()
            };
            Some(pending.state.as_str()) == callback_state.as_deref()
                && callback_target_matches
                && pending.expires_at > now_unix()
        });
        if !valid {
            state.store.clear_pending_wervik_connect();
            let _ = app.emit(
                WERVIK_ACCOUNT_ERROR_EVENT,
                "The Wervik Account connection expired or is invalid.",
            );
            return;
        }
        state.store.clear_pending_wervik_connect();
        pending.expect("validated pending Wervik connection")
    };

    if let Some(error) = error {
        let mut state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
        state.store.clear_pending_wervik_connect();
        let _ = app.emit(WERVIK_ACCOUNT_ERROR_EVENT, error);
        return;
    }

    let Some(code) = code else {
        let mut state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
        state.store.clear_pending_wervik_connect();
        let _ = app.emit(
            WERVIK_ACCOUNT_ERROR_EVENT,
            "The Wervik Account callback did not include a code.",
        );
        return;
    };

    std::thread::spawn(move || {
        let result = exchange_desktop_connect(code, callback_state.unwrap_or_default(), pending);
        let Some(state_handle) = app.try_state::<AppStateHandle>() else {
            return;
        };
        let mut state = state_handle.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(account) => {
                let mut settings = state.store.settings();
                state.wervik_sync_generation.fetch_add(1, Ordering::SeqCst);
                state.store.set_wervik_account(account.clone());
                // A connected Wervik Account is the authoritative metadata
                // provider. Changing it here also invalidates the current
                // metadata worker, which may still hold the previous token.
                if settings.metadata_provider != MetadataProvider::Wervik {
                    settings.metadata_provider = MetadataProvider::Wervik;
                    state.store.set_settings(settings.clone());
                }
                state.start_metadata_fetch();
                state.start_steam_owned_fetch();
                state.start_steam_achievements_fetch();
                drop(state);
                if let Err(error) = sync_wervik_settings(&account, &settings) {
                    let _ = app.emit(WERVIK_SETTINGS_SYNC_ERROR_EVENT, error);
                }
                schedule_wervik_cloud_sync(&app);
                let _ = app.emit(WERVIK_ACCOUNT_UPDATED_EVENT, account);
            }
            Err(error) => {
                state.store.clear_pending_wervik_connect();
                let _ = app.emit(WERVIK_ACCOUNT_ERROR_EVENT, error);
            }
        }
    });
}

#[tauri::command(async)]
pub fn get_wervik_account(state: State<AppStateHandle>) -> Option<WervikAccount> {
    state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .store
        .wervik_account()
}

#[tauri::command(async)]
pub fn connect_wervik_account(
    app: tauri::AppHandle,
    state: State<AppStateHandle>,
) -> Result<(), String> {
    let (listener, redirect_uri) = bind_wervik_callback()?;
    let url = {
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        let connect_state = random_connect_value();
        let code_verifier = random_connect_value();
        let installation_id = state.store.installation_id();
        let pending = PendingWervikConnect {
            state: connect_state.clone(),
            code_verifier: code_verifier.clone(),
            redirect_scheme: WERVIK_REDIRECT_SCHEME.to_string(),
            redirect_uri: Some(redirect_uri.clone()),
            installation_id: installation_id.clone(),
            expires_at: now_unix() + WERVIK_CONNECT_DURATION_S,
        };
        let url = web_connect_url(
            &installation_id,
            &connect_state,
            &pkce_challenge(&code_verifier),
            WERVIK_REDIRECT_SCHEME,
            &redirect_uri,
        )?;
        state.store.set_pending_wervik_connect(pending.clone());
        url
    };

    if let Err(error) = platform::open_url(&app, &url) {
        state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .store
            .clear_pending_wervik_connect();
        return Err(error);
    }
    spawn_wervik_callback_listener(app, listener, redirect_uri);
    Ok(())
}

#[tauri::command(async)]
pub fn disconnect_wervik_account(state: State<AppStateHandle>) -> Result<(), String> {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    let mut settings = state.store.settings();
    state.wervik_sync_generation.fetch_add(1, Ordering::SeqCst);
    if settings.metadata_provider == MetadataProvider::Wervik {
        settings.metadata_provider = MetadataProvider::Igdb;
        state.store.set_settings(settings);
    }
    state.store.clear_wervik_account();
    state.start_metadata_fetch();
    state.start_steam_owned_fetch();
    state.start_steam_achievements_fetch();
    Ok(())
}

#[tauri::command(async)]
pub fn get_settings(state: State<AppStateHandle>) -> Settings {
    state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .store
        .settings()
}

#[tauri::command(async)]
pub fn set_settings(
    app: tauri::AppHandle,
    state: State<AppStateHandle>,
    settings: Settings,
) -> Result<(), String> {
    if settings.metadata_provider == MetadataProvider::Wervik
        && state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .store
            .wervik_account()
            .map_or(true, |account| account.access_token.trim().is_empty())
    {
        return Err("Connect your Wervik Account before selecting Wervik metadata.".into());
    }

    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let autostart = app.autolaunch();
        let currently_enabled = autostart.is_enabled().unwrap_or(false);
        if settings.launch_on_startup && !currently_enabled {
            autostart.enable().map_err(|e| e.to_string())?;
        } else if !settings.launch_on_startup && currently_enabled {
            autostart.disable().map_err(|e| e.to_string())?;
        }
    }
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    let previous = state.store.settings();
    let metadata_provider_changed = previous.metadata_provider != settings.metadata_provider;
    let credentials_changed = previous.igdb_client_id != settings.igdb_client_id
        || previous.igdb_client_secret != settings.igdb_client_secret;
    let ra_credentials_changed = previous.ra_username != settings.ra_username
        || previous.ra_api_key != settings.ra_api_key
        || previous.ra_password != settings.ra_password;
    let steam_credentials_changed = previous.steam_api_key != settings.steam_api_key
        || previous.steam_id64_override != settings.steam_id64_override;
    let account = state.store.wervik_account();
    if metadata_provider_changed {
        state.metadata_cache.clear()?;
        metadata::clear_artwork(&state.app_data_dir)?;
        state.rebuild_known_image_paths();
    }
    state.store.set_settings(settings.clone());
    // Let metadata enrichment start immediately after credentials or provider
    // changes, rather than only on the next rescan or app restart.
    if credentials_changed || metadata_provider_changed {
        state.start_metadata_fetch();
    }
    if ra_credentials_changed {
        state.start_achievements_fetch();
    }
    if steam_credentials_changed {
        let _ = app.emit("steam-owned-games-updated", ());
        let _ = app.emit("steam-achievements-settings-updated", ());
        state.start_steam_owned_fetch();
        state.start_steam_achievements_fetch();
    }
    drop(state);

    if let Some(account) = account {
        let app_handle = app.clone();
        std::thread::spawn(move || {
            if let Err(error) = sync_wervik_settings(&account, &settings) {
                let _ = app_handle.emit(WERVIK_SETTINGS_SYNC_ERROR_EVENT, error);
            }
        });
    }

    schedule_wervik_cloud_sync(&app);

    Ok(())
}

/// Reads an image file from disk and returns it as a data: URL, so the
/// frontend can display Steam artwork without needing filesystem-scope
/// asset-protocol configuration for arbitrary, runtime-discovered paths.
///
/// `path` must be exactly one of the cover/hero/logo/screenshot paths
/// Wervik itself already surfaced for a known game (see
/// `AppState::known_image_paths`) — this command is a display helper, not a general
/// file-read primitive callable from the webview with an arbitrary path.
#[tauri::command(async)]
pub fn read_image(state: State<AppStateHandle>, path: String) -> Result<String, String> {
    let is_known = state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .known_image_paths
        .contains(&path);
    if !is_known {
        return Err("Not a known game image path".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let mime = match Path::new(&path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

/// Opens the game's install directory in the system file manager.
#[tauri::command(async)]
pub fn open_install_folder(state: State<AppStateHandle>, app_id: u32) -> Result<(), String> {
    let (app, path) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let path = state
            .games
            .iter()
            .find(|g| g.app_id == app_id)
            .map(|g| g.install_dir.clone());
        (state.app_handle.clone(), path)
    };
    let Some(path) = path else {
        return Err("Game not found".into());
    };
    platform::open_path(&app, &path)
}

/// Opens the install directory of a locally discovered Xbox game.
#[tauri::command(async)]
pub fn open_xbox_folder(state: State<AppStateHandle>, id: String) -> Result<(), String> {
    let (app, path) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let path = state
            .xbox_games
            .iter()
            .find(|game| game.id == id)
            .map(|game| game.install_dir.clone());
        (state.app_handle.clone(), path)
    };
    let Some(path) = path else {
        return Err("Xbox game not found".into());
    };
    platform::open_path(&app, &path)
}

/// Opens the install directory of a locally discovered Battle.net game.
#[tauri::command(async)]
pub fn open_battlenet_folder(state: State<AppStateHandle>, id: String) -> Result<(), String> {
    let (app, path) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let path = state
            .battlenet_games
            .iter()
            .find(|game| game.id == id)
            .map(|game| game.install_dir.clone());
        (state.app_handle.clone(), path)
    };
    let Some(path) = path else {
        return Err("Battle.net game not found".into());
    };
    platform::open_path(&app, &path)
}

/// Opens the install directory of a locally discovered EA App game.
#[tauri::command(async)]
pub fn open_ea_folder(state: State<AppStateHandle>, id: String) -> Result<(), String> {
    let (app, path) = {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let path = state
            .ea_games
            .iter()
            .find(|game| game.id == id)
            .map(|game| game.install_dir.clone());
        (state.app_handle.clone(), path)
    };
    let Some(path) = path else {
        return Err("EA game not found".into());
    };
    platform::open_path(&app, &path)
}

/// Opens a Nexus Mods page returned by the official API. Restricting the host
/// here keeps this command useful only for Nexus links even if a malformed
/// or unexpected API response reaches the frontend.
#[tauri::command(async)]
pub fn open_nexus_mod(state: State<AppStateHandle>, url: String) -> Result<(), String> {
    let valid_host =
        url.starts_with("https://www.nexusmods.com/") || url.starts_with("https://nexusmods.com/");
    if !valid_host {
        return Err("Invalid Nexus Mods URL".into());
    }
    let app = state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .app_handle
        .clone();
    platform::open_url(&app, &url)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSetupStatus {
    pub mangohud_installed: bool,
    pub mangohud_enabled: bool,
    pub mangohud_config: MangoHudConfig,
    pub gamemode_installed: bool,
    pub gamemode_enabled: bool,
    pub gamescope_installed: bool,
    pub gamescope_enabled: bool,
    pub gamescope_config: GamescopeConfig,
    pub env_vars: Vec<EnvVar>,
    /// Whether this game's Steam Launch Options already reference our
    /// wrapper script. Wervik never writes this itself (Steam is known to
    /// overwrite externally-made changes to it) — the user pastes
    /// `launch_options_snippet` into Steam's own Properties dialog once,
    /// and this flips to true as soon as Wervik notices it there.
    pub steam_configured: bool,
    pub launch_options_snippet: String,
    /// Drives the small indicator dot on the Setup button and whether the
    /// "active features" row on the details screen renders at all.
    pub has_custom_config: bool,
}

fn game_setup_status(state: &AppState, app_id: u32) -> GameSetupStatus {
    let dir = &state.game_setup_dir;
    let key = app_id.to_string();
    let wrapper_path = game_setup::wrapper_path(dir, &key);
    let current_options = state
        .steam_install
        .as_ref()
        .and_then(|i| launch_options::get_launch_options(&i.root, app_id));
    let mangohud_enabled = mangohud::is_enabled(dir, &key);
    let gamemode_enabled = game_setup::is_gamemode_enabled(dir, &key);
    let gamescope_enabled = game_setup::is_gamescope_enabled(dir, &key);
    let env_vars = game_setup::read_env_vars(dir, &key);

    GameSetupStatus {
        mangohud_installed: mangohud::is_installed(),
        mangohud_enabled,
        mangohud_config: mangohud::read_config(dir, &key),
        gamemode_installed: game_setup::is_gamemode_installed(),
        gamemode_enabled,
        gamescope_installed: game_setup::is_gamescope_installed(),
        gamescope_enabled,
        gamescope_config: game_setup::read_gamescope_config(dir, &key),
        has_custom_config: mangohud_enabled
            || gamemode_enabled
            || gamescope_enabled
            || !env_vars.is_empty(),
        env_vars,
        steam_configured: game_setup::is_wrapper_installed(
            current_options.as_deref(),
            &wrapper_path,
        ),
        launch_options_snippet: game_setup::build_wrapper_launch_options(
            current_options.as_deref().unwrap_or(""),
            &wrapper_path,
        ),
    }
}

/// Also used to re-check whether the user has pasted the Launch Options
/// snippet into Steam yet (a "Check again" button re-calls this).
#[tauri::command(async)]
pub fn get_game_setup_status(state: State<AppStateHandle>, app_id: u32) -> GameSetupStatus {
    game_setup_status(&state.lock().unwrap_or_else(|e| e.into_inner()), app_id)
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSetupInput {
    pub mangohud_enabled: bool,
    pub mangohud_config: MangoHudConfig,
    pub gamemode_enabled: bool,
    pub gamescope_enabled: bool,
    pub gamescope_config: GamescopeConfig,
    pub env_vars: Vec<EnvVar>,
}

/// Persists the whole Game Setup drawer at once (its "Apply" button). Safe
/// to call any time — none of this touches Steam; the wrapper script only
/// takes effect once Launch Options already point at it (see
/// `steam_configured`).
#[tauri::command(async)]
pub fn apply_game_setup(
    state: State<AppStateHandle>,
    app_id: u32,
    input: GameSetupInput,
) -> Result<GameSetupStatus, String> {
    let state = state.lock().unwrap_or_else(|e| e.into_inner());
    let dir = &state.game_setup_dir;
    let key = app_id.to_string();

    mangohud::write_config(dir, &key, &input.mangohud_config)?;
    mangohud::set_enabled(dir, &key, input.mangohud_enabled)?;
    game_setup::set_gamemode_enabled(dir, &key, input.gamemode_enabled)?;
    game_setup::write_gamescope_config(dir, &key, &input.gamescope_config)?;
    game_setup::set_gamescope_enabled(dir, &key, input.gamescope_enabled)?;
    game_setup::write_env_vars(dir, &key, &input.env_vars)?;
    game_setup::regenerate_wrapper(dir, &key)?;

    Ok(game_setup_status(&state, app_id))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wervik-commands-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        (Store::load(&dir), dir)
    }

    #[test]
    fn a_platform_with_no_native_emulators_has_no_default() {
        let (store, dir) = temp_store("no-native-platform");
        assert_eq!(selected_emulator_id(&dir, Platform::Snes, &store), "");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Every native option is now part of the Libretro/native path, so the
    /// platform's recommended choice is never a browser runtime.
    #[test]
    fn native_n64_is_recommended() {
        let dir = std::env::temp_dir().join(format!(
            "wervik-commands-test-recommended-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let n64_options = emulator_options_for(&dir, Platform::Nintendo64);
        let native = n64_options
            .iter()
            .find(|o| o.kind == EmulatorKind::Native)
            .unwrap();
        assert!(native.recommended);

        let snes_options = emulator_options_for(&dir, Platform::Snes);
        assert!(snes_options.is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }

    /// With no explicit choice made yet, the default should be the first
    /// installed native N64 emulator in Wervik's managed directory, else
    /// the recommended native option.
    #[test]
    fn defaults_to_the_first_installed_native_emulator_or_recommended_if_none_are() {
        let (store, dir) = temp_store("n64-default");
        let expected = retro_native::native_emulators_for(Platform::Nintendo64)
            .into_iter()
            .find(|e| retro_native::is_available(&dir, e))
            .map(|e| e.id.to_string())
            .or_else(|| {
                retro_native::native_emulators_for(Platform::Nintendo64)
                    .into_iter()
                    .find(|e| e.recommended)
                    .map(|e| e.id.to_string())
            })
            .unwrap_or_default();
        assert_eq!(
            selected_emulator_id(&dir, Platform::Nintendo64, &store),
            expected
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// An explicit choice that is currently unavailable stays visible, but
    /// there is no hidden browser fallback anymore.
    #[test]
    fn an_explicit_but_unavailable_choice_has_no_effective_emulator() {
        let (mut store, dir) = temp_store("unavailable-choice");
        store.set_default_emulator("nintendo_64", "not-a-real-emulator");

        assert_eq!(
            selected_emulator_id(&dir, Platform::Nintendo64, &store),
            "not-a-real-emulator"
        );

        assert_eq!(
            effective_emulator_id(&dir, Platform::Nintendo64, &store),
            None
        );
        assert!(resolve_effective_native_emulator(&dir, Platform::Nintendo64, &store).is_none());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_is_effective_when_the_selected_native_emulator_is_unavailable() {
        let (mut store, dir) = temp_store("no-runtime-dir");
        store.set_default_emulator("nintendo_64", "not-a-real-emulator");
        assert_eq!(
            effective_emulator_id(&dir, Platform::Nintendo64, &store),
            None
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn local_callback_keeps_the_bound_origin_and_reads_auth_query_parameters() {
        let callback = callback_url_from_request(
            "http://127.0.0.1:43127/callback",
            "/callback?code=one-time-code&state=state-value",
        )
        .unwrap();

        assert_eq!(
            callback.as_str(),
            "http://127.0.0.1:43127/callback?code=one-time-code&state=state-value"
        );
    }

    #[test]
    fn local_callback_rejects_a_different_path() {
        assert!(callback_url_from_request(
            "http://127.0.0.1:43127/callback",
            "/other?code=one-time-code"
        )
        .is_err());
    }
}
