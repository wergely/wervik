use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use tauri::State;

use tauri::{Emitter, Manager};

use crate::battlenet::discovery::BattleNetGame;
use crate::game_setup::{self, EnvVar, GamescopeConfig};
use crate::mangohud::{self, MangoHudConfig};
use crate::metadata::{self, GameMetadata, MetadataCache, MetadataProgress};
use crate::nexus::{self, NexusCache, NexusMod};
use crate::platform;
use crate::playtime::{self, PlaytimeDb, ProcessTarget};
use crate::retro::discovery::RomGame;
use crate::retro::platform::Platform;
use crate::retro::server::RunningServer;
use crate::retro::session::EmulatorSession;
use crate::retro::{
    artwork, bios, discovery as retro_discovery, naming as retro_naming, native as retro_native,
    server as retro_server, session as retro_session,
};
use crate::steam::discovery::{self, LaunchKind, SteamInstall};
use crate::steam::launch;
use crate::steam::launch_options;
use crate::steam::library::{self, Game};
use crate::storage::{Settings, Store};
use crate::xbox::discovery::XboxGame;

pub struct AppState {
    pub steam_install: Option<SteamInstall>,
    pub library_folders: Vec<PathBuf>,
    pub games: Vec<Game>,
    pub xbox_games: Vec<XboxGame>,
    pub battlenet_games: Vec<BattleNetGame>,
    pub battlenet_artwork_running: Arc<AtomicBool>,
    pub playtime: PlaytimeDb,
    pub metadata_cache: MetadataCache,
    pub nexus_cache: NexusCache,
    pub metadata_fetch_running: Arc<AtomicBool>,
    pub metadata_progress: Arc<metadata::MetadataProgressState>,
    pub store: Store,
    pub game_setup_dir: PathBuf,
    pub app_data_dir: PathBuf,
    pub rom_games: Vec<RomGame>,
    /// Session id -> (game id, running local server, playtime session id).
    /// Dropping the entry
    /// stops the server, since `RunningServer` shuts its thread down on
    /// `Drop`.
    pub retro_sessions: HashMap<String, (String, RunningServer, i64)>,
    /// Where Wervik's bundled copy of the EmulatorJS runtime lives (see
    /// `resources/emulatorjs-runtime`) — `None` only if packaging somehow
    /// left it out, which the session page reports honestly instead of
    /// pretending the emulator can boot.
    pub retro_runtime_dir: Option<PathBuf>,
    pub app_handle: tauri::AppHandle,
    /// Guards against two artwork-fetch passes running at once — without
    /// it, rescanning a huge library while a previous pass is still
    /// working through the backlog would spawn a second thread hammering
    /// the same host and doubling CPU/network load for no benefit.
    pub artwork_fetch_running: Arc<AtomicBool>,
    /// How far the current (or most recent) background artwork fetch has
    /// gotten — read by `get_artwork_progress` so the frontend can show a
    /// progress indicator even if it missed the `retro-artwork-progress`
    /// events (e.g. it wasn't mounted yet when the fetch on startup began).
    pub artwork_progress: Arc<ArtworkProgressState>,
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
        self.seed_provider_playtime();
        self.start_metadata_fetch();
    }

    pub(crate) fn metadata_queries(&self) -> Vec<metadata::MetadataQuery> {
        self.games
            .iter()
            .map(|game| metadata::MetadataQuery {
                id: game.app_id.to_string(),
                name: game.name.clone(),
            })
            .chain(self.xbox_games.iter().map(|game| metadata::MetadataQuery {
                id: game.id.clone(),
                name: game.name.clone(),
            }))
            .chain(
                self.battlenet_games
                    .iter()
                    .map(|game| metadata::MetadataQuery {
                        id: game.id.clone(),
                        name: game.name.clone(),
                    }),
            )
            .chain(self.rom_games.iter().map(|game| metadata::MetadataQuery {
                id: game.id.clone(),
                name: game.name.clone(),
            }))
            .collect()
    }

    pub(crate) fn start_metadata_fetch(&self) {
        metadata::spawn_fetch(
            self.app_handle.clone(),
            self.app_data_dir.clone(),
            self.metadata_queries(),
            self.metadata_fetch_running.clone(),
            self.metadata_progress.clone(),
        );
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
    }

    /// Full filesystem walk of every configured ROM directory. Only called
    /// from an explicit user action (adding/removing a directory, or a
    /// "Rescan" button) — never on ordinary app startup, which instead
    /// loads the cached results (see `retro::cache`). Also kicks off a
    /// background best-effort box art fetch for any newly-seen game (see
    /// `spawn_artwork_fetch`).
    pub fn rescan_retro(&mut self) {
        self.rom_games = retro_discovery::scan_directories(&self.store.rom_directories());
        self.store.migrate_legacy_bios_paths(
            self.rom_games
                .iter()
                .map(|rom| (rom.id.clone(), rom.platform.key().to_string()))
                .collect(),
        );
        self.dedupe_rom_games_by_metadata();
        let _ = crate::retro::cache::save(&self.app_data_dir, &self.rom_games);
        // IGDB is the primary cover source for retro games when configured.
        // Keep the old libretro-thumbnails pass only for installations that
        // have no IGDB credentials, where it remains a useful fallback.
        if !metadata::is_configured() {
            spawn_artwork_fetch(
                self.app_handle.clone(),
                self.app_data_dir.clone(),
                self.rom_games.clone(),
                self.artwork_fetch_running.clone(),
                self.artwork_progress.clone(),
            );
        }
        self.start_metadata_fetch();
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
                Ok(Some(_)) => {
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
    let state = state.lock().unwrap();
    ArtworkProgress {
        done: state.artwork_progress.done.load(Ordering::SeqCst),
        total: state.artwork_progress.total.load(Ordering::SeqCst),
    }
}

#[tauri::command(async)]
pub fn get_metadata_progress(state: State<AppStateHandle>) -> MetadataProgress {
    let state = state.lock().unwrap();
    MetadataProgress {
        done: state.metadata_progress.done.load(Ordering::SeqCst),
        total: state.metadata_progress.total.load(Ordering::SeqCst),
    }
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
    if metadata::is_configured() {
        return None;
    }

    let (app_handle, app_data_dir, rom) = {
        let state = state.lock().unwrap();
        let rom = state.rom_games.iter().find(|r| r.id == id)?.clone();
        (state.app_handle.clone(), state.app_data_dir.clone(), rom)
    };
    if let Some(path) = artwork::cached_cover(&app_data_dir, &id) {
        return Some(path.to_string_lossy().to_string());
    }
    std::thread::spawn(move || {
        let agent = artwork::new_agent();
        if let Ok(Some(_)) = artwork::fetch_cover(&agent, &app_data_dir, &rom) {
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
    playtime::spawn_process_monitor(state.app_handle.clone(), session_id, target);
    Ok(())
}

/// Completes a session from a process monitor or a retro session shutdown and
/// refreshes the frontend. The old JSON retro total is also updated for
/// compatibility with installations created before the SQLite database.
pub(crate) fn finish_playtime_session(app_handle: &tauri::AppHandle, session_id: i64) {
    let Some(state) = app_handle.try_state::<AppStateHandle>() else {
        return;
    };
    let mut state = state.lock().unwrap();
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
    let _ = app_handle.emit("playtime-updated", ());
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
    /// Short platform label for the source badge (e.g. "SNES"), present
    /// only for `source == Retro`.
    pub platform: Option<String>,
    /// Stable machine key (see `Platform::key`) for joining against
    /// `PlatformStatus.key` — `platform` above is a display label and can
    /// theoretically collide or change wording, this won't.
    pub platform_key: Option<String>,
    pub requires_bios: Option<bool>,
    /// Name of the native emulator this game will actually launch with
    /// (e.g. "gopher64"), when one is installed and preferred over
    /// EmulatorJS for its platform — surfaced so switching launch
    /// mechanisms is never silent. `None` means it'll use EmulatorJS.
    pub native_emulator: Option<String>,
    /// Region label for the copy of the ROM that plays by default (see
    /// `retro::naming::region_label`) — `None` for Steam games, which have
    /// no such concept.
    pub default_region: Option<String>,
    /// Other regional dumps of this same game the user can choose to play
    /// instead (see `retro::discovery::RomVariant`, `launch_retro_game`'s
    /// `variant_id`) — empty for Steam games.
    pub variants: Vec<GameVariant>,
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
        is_favorite: store.is_favorite(&game.app_id.to_string()),
        app_id: Some(game.app_id),
        xbox_app_id: None,
        battlenet_product_code: None,
        platform: None,
        platform_key: None,
        requires_bios: None,
        native_emulator: None,
        default_region: None,
        variants: Vec::new(),
    }
}

/// Stable id for the built-in EmulatorJS runtime in the "Default Emulator"
/// selection — sits in the same id space as a native emulator's id (see
/// `retro_native::NativeEmulator::id`) so a platform's default is always
/// just one string, never a separate "prefer native" flag plus a name.
const EMULATORJS_ID: &str = "emulatorjs";

/// One row in a platform's "Default Emulator" list — EmulatorJS itself,
/// always first, plus every native emulator Wervik knows how to launch for
/// that platform (see `retro_native::native_emulators_for`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EmulatorKind {
    BuiltIn,
    Native,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorOption {
    /// `EMULATORJS_ID` for the built-in runtime, or a native emulator's id.
    pub id: String,
    pub display_name: String,
    pub kind: EmulatorKind,
    /// "Ready" (EmulatorJS, when its runtime is bundled) or "Installed"
    /// (a native emulator actually found on the system) — the UI picks the
    /// right word from `kind`, this is just the underlying bool.
    pub available: bool,
    /// Whether Wervik can download and install this option itself (see
    /// `retro::native::install`) — drives whether the UI offers a
    /// "Download" action instead of just "Not installed". Always `false`
    /// for EmulatorJS (nothing to install, it ships with Wervik).
    pub managed: bool,
    /// The best default for this platform — exactly one option per
    /// platform is ever marked `true`. Usually EmulatorJS (the safe,
    /// universal choice), but not when a native emulator measurably runs
    /// the platform's games better (see `retro_native::NativeEmulator::recommended`).
    pub recommended: bool,
}

/// Every emulator choice `platform` offers, in a fixed order (EmulatorJS
/// first, since it's always available whenever Wervik itself is — a
/// consistent, obvious default — then natives in `native_emulators_for`'s
/// preference order).
fn emulator_options_for(
    app_data_dir: &Path,
    platform: Platform,
    retro_runtime_dir: &Option<PathBuf>,
) -> Vec<EmulatorOption> {
    let natives = retro_native::native_emulators_for(platform);
    let has_recommended_native = natives.iter().any(|e| e.recommended);

    // EmulatorJS is never offered at all for a platform it has no
    // vendored core for (Arcade and Neo Geo CD) — a real, unavailable-forever option
    // would just be a broken promise, not a genuine fallback.
    let mut options: Vec<EmulatorOption> = Vec::new();
    if platform.default_core().is_some() {
        options.push(EmulatorOption {
            id: EMULATORJS_ID.to_string(),
            display_name: "EmulatorJS".to_string(),
            kind: EmulatorKind::BuiltIn,
            available: retro_runtime_dir.is_some(),
            managed: false,
            recommended: !has_recommended_native,
        });
    }
    options.extend(natives.into_iter().map(|e| EmulatorOption {
        id: e.id.to_string(),
        display_name: e.display_name.to_string(),
        kind: EmulatorKind::Native,
        available: retro_native::is_available(app_data_dir, &e),
        managed: e.is_managed(),
        recommended: e.recommended,
    }));
    options
}

/// The user's explicit "Default Emulator" choice for `platform` — always
/// one of `emulator_options_for`'s ids, whether or not that choice is
/// currently available (an unavailable selection is a real, visible state
/// in the UI, not something silently swapped out — see
/// `resolve_effective_native_emulator`). The first time a platform is seen
/// with nothing chosen yet, this picks the first native option when the
/// platform has no EmulatorJS fallback, so a newly discovered Arcade or Neo
/// Geo CD library still exposes the correct Download action.
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
            platform
                .default_core()
                .is_none()
                .then(|| native_options)
                .and_then(|options| options.into_iter().find(|e| e.recommended))
                .map(|e| e.id.to_string())
        })
        .unwrap_or_else(|| EMULATORJS_ID.to_string())
}

/// What will actually run right now for `platform`: the selected emulator
/// if it's available, else EmulatorJS as long as its runtime is bundled.
/// `None` only in the packaging-mistake case where even that isn't
/// available — see `EmulatorOption::available`.
fn effective_emulator_id(
    app_data_dir: &Path,
    platform: Platform,
    store: &Store,
    retro_runtime_dir: &Option<PathBuf>,
) -> Option<String> {
    let options = emulator_options_for(app_data_dir, platform, retro_runtime_dir);
    let selected = selected_emulator_id(app_data_dir, platform, store);
    if let Some(option) = options.iter().find(|o| o.id == selected) {
        if option.available {
            return Some(option.id.clone());
        }
    }
    options
        .iter()
        .find(|o| o.id == EMULATORJS_ID && o.available)
        .map(|o| o.id.clone())
}

/// The concrete native emulator to actually launch for `platform` right
/// now, or `None` if this game should go through EmulatorJS instead —
/// either because that's the resolved choice, or because the platform's
/// selected native emulator isn't available and EmulatorJS is the
/// resolved fallback (see `effective_emulator_id`). Kept as its own
/// function (rather than inlining `effective_emulator_id` at each call
/// site) because callers need the full `NativeEmulator` — launcher
/// details included — not just its id.
fn resolve_effective_native_emulator(
    app_data_dir: &Path,
    platform: Platform,
    store: &Store,
) -> Option<retro_native::NativeEmulator> {
    let selected = selected_emulator_id(app_data_dir, platform, store);
    if selected == EMULATORJS_ID {
        return None;
    }
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
        is_favorite: store.is_favorite(&rom.id),
        app_id: None,
        xbox_app_id: None,
        battlenet_product_code: None,
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
        is_favorite: store.is_favorite(&game.id),
        app_id: None,
        xbox_app_id: Some(game.aumid.clone()),
        battlenet_product_code: None,
        platform: None,
        platform_key: None,
        requires_bios: None,
        native_emulator: None,
        default_region: None,
        variants: Vec::new(),
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
        is_favorite: store.is_favorite(&game.id),
        app_id: None,
        xbox_app_id: None,
        battlenet_product_code: Some(game.launch_code.clone()),
        platform: None,
        platform_key: None,
        requires_bios: None,
        native_emulator: None,
        default_region: None,
        variants: Vec::new(),
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

    let mut games: Vec<GameDto> = state
        .games
        .iter()
        .map(|g| steam_to_dto(g, &state.store, &state.playtime))
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
    }

    LibrarySnapshot {
        steam_found: state.steam_install.is_some(),
        steam_root: state
            .steam_install
            .as_ref()
            .map(|i| i.root.to_string_lossy().to_string()),
        xbox_found: !state.xbox_games.is_empty(),
        battlenet_found: !state.battlenet_games.is_empty(),
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
    library_snapshot(&state.lock().unwrap())
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
) -> Result<Option<GameMetadata>, String> {
    let (cached, recent_miss, app_data_dir) = {
        let state = state.lock().unwrap();
        (
            state.metadata_cache.get(&game_id),
            state.metadata_cache.recent_miss(&game_id),
            state.app_data_dir.clone(),
        )
    };
    if let Some(metadata) = cached.filter(metadata::is_fresh) {
        return Ok(Some(metadata));
    }
    if recent_miss {
        return Ok(None);
    }

    let Some(metadata) = metadata::fetch_game(&app_data_dir, &game_id, &game_name)? else {
        let mut state = state.lock().unwrap();
        state.metadata_cache.mark_miss(game_id)?;
        return Ok(None);
    };
    let mut state = state.lock().unwrap();
    state.metadata_cache.put(game_id, metadata.clone())?;
    Ok(Some(metadata))
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
        let state = state.lock().unwrap();
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
            let mut state = state.lock().unwrap();
            state.nexus_cache.put(domain.to_string(), mods.clone())?;
            Ok(Some(mods))
        }
        Err(error) => stale.map_or(Err(error), |mods| Ok(Some(mods))),
    }
}

#[tauri::command(async)]
pub fn rescan_library(state: State<AppStateHandle>) -> LibrarySnapshot {
    let mut state = state.lock().unwrap();
    // Re-detect Steam in case it was installed/moved since last scan.
    state.steam_install = discovery::find_steam_install();
    state.rescan();
    library_snapshot(&state)
}

#[tauri::command(async)]
pub fn get_rom_directories(state: State<AppStateHandle>) -> Vec<String> {
    state.lock().unwrap().store.rom_directories()
}

#[tauri::command(async)]
pub fn add_rom_directory(state: State<AppStateHandle>, path: String) -> LibrarySnapshot {
    let mut state = state.lock().unwrap();
    state.store.add_rom_directory(path);
    state.rescan_retro();
    library_snapshot(&state)
}

#[tauri::command(async)]
pub fn remove_rom_directory(state: State<AppStateHandle>, path: String) -> LibrarySnapshot {
    let mut state = state.lock().unwrap();
    state.store.remove_rom_directory(&path);
    state.rescan_retro();
    library_snapshot(&state)
}

#[tauri::command(async)]
pub fn rescan_retro_library(state: State<AppStateHandle>) -> LibrarySnapshot {
    let mut state = state.lock().unwrap();
    state.rescan_retro();
    library_snapshot(&state)
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum RetroLaunch {
    /// Launched as a real, separate OS process (see `retro::native`) —
    /// there's no session/URL for the frontend to do anything with; it
    /// just stays where it is while the emulator's own window runs.
    Native,
    EmulatorJs {
        session_id: String,
        url: String,
    },
}

/// Validates the ROM (and BIOS, if the platform needs one) still exist,
/// then either launches a native emulator as its own process (when one is
/// installed and preferred for this platform — see `retro::native`) or
/// starts a local-only EmulatorJS session server and hands the frontend a
/// URL to load in an isolated view. For the EmulatorJS path, the frontend
/// is responsible for calling `stop_retro_session` when the player exits;
/// for the native path, Wervik tracks the process itself and records
/// playtime when it exits, with nothing more for the frontend to do.
#[tauri::command(async)]
pub fn launch_retro_game(
    state: State<AppStateHandle>,
    id: String,
    variant_id: Option<String>,
) -> Result<RetroLaunch, String> {
    let mut state = state.lock().unwrap();
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
        // Unlike EmulatorJS, native emulators (e.g. gopher64) can't read a
        // ROM straight out of a zip themselves — extract it to a scratch
        // dir first and clean that up once the emulator exits, same as
        // the wrapper never leaves anything behind.
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

        let playtime_session_id = state.playtime.start_session(&id, "retro", now_unix())?;
        let child = match retro_native::spawn(
            &state.app_data_dir,
            emulator,
            &native_rom_path,
            &state.game_setup_dir,
            rom.platform.key(),
        ) {
            Ok(child) => child,
            Err(error) => {
                let _ = state
                    .playtime
                    .cancel_session(playtime_session_id, now_unix());
                return Err(error);
            }
        };
        let app_handle = state.app_handle.clone();
        std::thread::spawn(move || {
            let mut child = child;
            let _ = child.wait();
            if let Some(dir) = cleanup_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            finish_playtime_session(&app_handle, playtime_session_id);
        });
        return Ok(RetroLaunch::Native);
    }

    let core = rom.platform.default_core().ok_or_else(|| {
        format!(
            "EmulatorJS doesn't support {} — install a native emulator for it instead.",
            rom.platform.display_name()
        )
    })?;
    let session_id = retro_session::generate_session_id();
    let session = EmulatorSession {
        rom_path: PathBuf::from(&rom_path),
        core: core.to_string(),
        bios_path,
    };

    let runtime_dir = state.retro_runtime_dir.clone();
    let playtime_session_id = state.playtime.start_session(&id, "retro", now_unix())?;
    let running = match retro_server::start(session, runtime_dir) {
        Ok(running) => running,
        Err(error) => {
            let _ = state
                .playtime
                .cancel_session(playtime_session_id, now_unix());
            return Err(error);
        }
    };
    let url = running.session_url();
    state
        .retro_sessions
        .insert(session_id.clone(), (id, running, playtime_session_id));

    Ok(RetroLaunch::EmulatorJs { session_id, url })
}

/// Stops a play session's local server and closes its SQLite playtime session.
/// `minutes_played` remains in the command signature for frontend/API
/// compatibility; the persisted duration now comes from the session clock.
#[tauri::command(async)]
pub fn stop_retro_session(
    state: State<AppStateHandle>,
    session_id: String,
    _minutes_played: u64,
) -> Result<(), String> {
    let mut state = state.lock().unwrap();
    let Some((_game_id, running, playtime_session_id)) = state.retro_sessions.remove(&session_id)
    else {
        return Ok(());
    };
    drop(running);
    let Ok(Some(finished)) = state
        .playtime
        .finish_session(playtime_session_id, now_unix())
    else {
        return Ok(());
    };
    if finished.duration_seconds > 0 {
        state.store.record_retro_session(
            &finished.game_id,
            finished.duration_seconds.div_ceil(60),
            finished.ended_at,
        );
    }
    let _ = state.app_handle.emit("playtime-updated", ());
    Ok(())
}

/// Opens the folder containing a ROM in the system file manager — the
/// retro-game equivalent of `open_install_folder`.
#[tauri::command(async)]
pub fn open_rom_folder(state: State<AppStateHandle>, id: String) -> Result<(), String> {
    let (app, path) = {
        let state = state.lock().unwrap();
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
    /// Every emulator choice for this platform — EmulatorJS first, then
    /// any native emulators Wervik knows how to launch for it.
    pub emulator_options: Vec<EmulatorOption>,
    /// The user's chosen default (see `selected_emulator_id`) — always one
    /// of `emulator_options`' ids, even if that option is currently
    /// unavailable (the UI shows that state rather than hiding it).
    pub selected_emulator: String,
    /// What will actually be used right now (see `effective_emulator_id`).
    /// Differs from `selected_emulator` only while the selected choice is
    /// unavailable and Wervik has fallen back to EmulatorJS; `None` only
    /// if nothing at all is usable.
    pub effective_emulator: Option<String>,
    /// Whether games on this platform need a user-supplied BIOS file.
    pub requires_bios: bool,
    /// Shared BIOS path configured for this platform, if any.
    pub bios_path: Option<String>,
}

/// One row per platform Wervik knows about (see `Platform::ALL`), for the
/// Platforms view and Platform Setup — every emulator choice, which one is
/// selected vs. actually in effect, and how many ROMs are currently in the
/// library for it.
#[tauri::command(async)]
pub fn get_platform_status(state: State<AppStateHandle>) -> Vec<PlatformStatus> {
    let state = state.lock().unwrap();
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
            emulator_options: emulator_options_for(
                &state.app_data_dir,
                platform,
                &state.retro_runtime_dir,
            ),
            selected_emulator: selected_emulator_id(&state.app_data_dir, platform, &state.store),
            effective_emulator: effective_emulator_id(
                &state.app_data_dir,
                platform,
                &state.store,
                &state.retro_runtime_dir,
            ),
            requires_bios: platform.requires_bios(),
            bios_path: state.store.bios_path(platform.key()),
        })
        .collect()
}

/// Sets the platform default emulator (see `PlatformStatus.selectedEmulator`).
/// Takes effect immediately — nothing to "Apply", since picking a different
/// default is a simple, safe, instantly-reversible choice, not a batch of
/// settings like the Setup drawer's.
#[tauri::command(async)]
pub fn set_default_emulator(state: State<AppStateHandle>, platform: String, emulator_id: String) {
    state
        .lock()
        .unwrap()
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
    let app_data_dir = state.lock().unwrap().app_data_dir.clone();
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
        let state = state.lock().unwrap();
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
    let state = state.lock().unwrap();
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
    let state = state.lock().unwrap();
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
    state.lock().unwrap().store.bios_path(&platform)
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
    let mut state = state.lock().unwrap();
    let dest = bios::install(&state.app_data_dir.clone(), Path::new(&source_path))?;
    let dest_str = dest.to_string_lossy().to_string();
    state.store.set_bios_path(&platform, Some(dest_str.clone()));
    Ok(dest_str)
}

#[tauri::command(async)]
pub fn clear_bios_path(state: State<AppStateHandle>, platform: String) {
    state.lock().unwrap().store.set_bios_path(&platform, None);
}

#[tauri::command(async)]
pub fn launch_game(state: State<AppStateHandle>, app_id: u32) -> Result<(), String> {
    let mut state = state.lock().unwrap();
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

/// Launches a locally installed Xbox App / PC Game Pass title through its
/// Windows AppUserModelID. The ID comes from the local app registration scan,
/// so this does not need access to the protected game executable directory.
#[tauri::command(async)]
pub fn launch_xbox_game(state: State<AppStateHandle>, id: String) -> Result<(), String> {
    let mut state = state.lock().unwrap();
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
    let mut state = state.lock().unwrap();
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

#[tauri::command(async)]
pub fn open_steam_page(state: State<AppStateHandle>, app_id: u32) -> Result<(), String> {
    let (app, launch_kind) = {
        let state = state.lock().unwrap();
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
    let mut state = state.lock().unwrap();
    state.store.set_favorite(&id, favorite);
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
    state.lock().unwrap().store.set_game_reaction(&id, reaction);
    Ok(())
}

#[tauri::command(async)]
pub fn get_settings(state: State<AppStateHandle>) -> Settings {
    state.lock().unwrap().store.settings()
}

#[tauri::command(async)]
pub fn set_settings(
    app: tauri::AppHandle,
    state: State<AppStateHandle>,
    settings: Settings,
) -> Result<(), String> {
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
    let _ = &app;
    state.lock().unwrap().store.set_settings(settings);
    Ok(())
}

/// Reads an image file from disk and returns it as a data: URL, so the
/// frontend can display Steam artwork without needing filesystem-scope
/// asset-protocol configuration for arbitrary, runtime-discovered paths.
#[tauri::command(async)]
pub fn read_image(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let mime = if path.to_lowercase().ends_with(".png") {
        "image/png"
    } else {
        "image/jpeg"
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

/// Opens the game's install directory in the system file manager.
#[tauri::command(async)]
pub fn open_install_folder(state: State<AppStateHandle>, app_id: u32) -> Result<(), String> {
    let (app, path) = {
        let state = state.lock().unwrap();
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
        let state = state.lock().unwrap();
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
        let state = state.lock().unwrap();
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
    let app = state.lock().unwrap().app_handle.clone();
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
    game_setup_status(&state.lock().unwrap(), app_id)
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
    let state = state.lock().unwrap();
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

#[cfg(all(test, not(target_os = "windows")))]
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
    fn a_platform_with_no_native_emulators_defaults_to_emulatorjs() {
        let (store, dir) = temp_store("no-native-platform");
        assert_eq!(
            selected_emulator_id(&dir, Platform::Snes, &store),
            EMULATORJS_ID
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The native N64 emulator measurably outperforms EmulatorJS's WASM core,
    /// so it — not EmulatorJS — should carry the "Recommended" badge.
    /// Every other platform, with no recommended native, keeps EmulatorJS
    /// as the recommended (safe, universal) choice.
    #[test]
    fn native_n64_is_recommended_instead_of_emulatorjs() {
        let dir = std::env::temp_dir().join(format!(
            "wervik-commands-test-recommended-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let n64_options = emulator_options_for(&dir, Platform::Nintendo64, &None);
        let emulatorjs = n64_options.iter().find(|o| o.id == EMULATORJS_ID).unwrap();
        let native = n64_options
            .iter()
            .find(|o| o.kind == EmulatorKind::Native)
            .unwrap();
        assert!(!emulatorjs.recommended);
        assert!(native.recommended);

        let snes_options = emulator_options_for(&dir, Platform::Snes, &None);
        assert!(
            snes_options
                .iter()
                .find(|o| o.id == EMULATORJS_ID)
                .unwrap()
                .recommended
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    /// With no explicit choice made yet, the default should be the first
    /// installed native N64 emulator in Wervik's managed directory, else
    /// EmulatorJS (mirrors `selected_emulator_id`'s own logic).
    #[test]
    fn defaults_to_the_first_installed_native_emulator_or_emulatorjs_if_none_are() {
        let (store, dir) = temp_store("n64-default");
        let expected = retro_native::native_emulators_for(Platform::Nintendo64)
            .into_iter()
            .find(|e| retro_native::is_available(&dir, e))
            .map(|e| e.id.to_string())
            .unwrap_or_else(|| EMULATORJS_ID.to_string());
        assert_eq!(
            selected_emulator_id(&dir, Platform::Nintendo64, &store),
            expected
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The core fallback-behavior guarantee: an explicit choice that is
    /// currently unavailable must stay exactly what the user picked, while
    /// only the *effective* (actually launched) emulator falls back. Uses
    /// a made-up id rather than a real emulator's, so this holds regardless
    /// of what's actually installed on whichever machine runs this test.
    #[test]
    fn an_explicit_but_unavailable_choice_is_preserved_while_the_effective_emulator_falls_back() {
        let (mut store, dir) = temp_store("unavailable-choice");
        store.set_default_emulator("nintendo_64", "not-a-real-emulator");

        assert_eq!(
            selected_emulator_id(&dir, Platform::Nintendo64, &store),
            "not-a-real-emulator"
        );

        let runtime_dir = Some(std::env::temp_dir());
        assert_eq!(
            effective_emulator_id(&dir, Platform::Nintendo64, &store, &runtime_dir),
            Some(EMULATORJS_ID.to_string())
        );
        assert!(resolve_effective_native_emulator(&dir, Platform::Nintendo64, &store).is_none());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_is_effective_when_even_emulatorjs_is_unavailable() {
        let (mut store, dir) = temp_store("no-runtime-dir");
        store.set_default_emulator("nintendo_64", "not-a-real-emulator");
        assert_eq!(
            effective_emulator_id(&dir, Platform::Nintendo64, &store, &None),
            None
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn selecting_emulatorjs_explicitly_is_respected_over_an_installed_native_emulator() {
        let (mut store, dir) = temp_store("explicit-emulatorjs");
        store.set_default_emulator("nintendo_64", EMULATORJS_ID);

        assert_eq!(
            selected_emulator_id(&dir, Platform::Nintendo64, &store),
            EMULATORJS_ID
        );
        assert!(resolve_effective_native_emulator(&dir, Platform::Nintendo64, &store).is_none());

        let _ = std::fs::remove_dir_all(dir);
    }
}
