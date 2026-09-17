#![allow(linker_messages)]

mod battlenet;
mod commands;
mod ea;
mod game_setup;
mod mangohud;
mod metadata;
mod nexus;
mod platform;
mod playtime;
mod retro;
mod retroachievements;
mod steam;
mod storage;
mod sysutil;
mod window;
mod xbox;

use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};

use tauri::Manager;
#[cfg(desktop)]
use tauri_plugin_deep_link::DeepLinkExt;

use commands::AppState;
use steam::{discovery, library};
use storage::{MetadataProvider, Store};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            #[cfg(desktop)]
            {
                #[cfg(any(windows, target_os = "linux"))]
                if let Err(error) = app.deep_link().register_all() {
                    eprintln!("Could not register the wervik:// URL scheme: {error}");
                }

                let app_handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        commands::finish_wervik_connect(app_handle.clone(), url);
                    }
                });
            }

            let app_data_dir = app.path().app_data_dir()?;
            let mut store = Store::load(&app_data_dir);
            let playtime =
                playtime::PlaytimeDb::open(&app_data_dir.join("wervik-playtime.sqlite3"))
                    .map_err(std::io::Error::other)?;
            let metadata_cache =
                metadata::MetadataCache::load(&app_data_dir.join("wervik-metadata.json"));
            let nexus_cache = nexus::NexusCache::load(&app_data_dir.join("wervik-nexus.json"));
            let retro_achievements_cache = retroachievements::RetroAchievementsCache::load(
                &app_data_dir.join("wervik-retroachievements.json"),
            );
            let steam_achievements_cache = steam::achievements::SteamAchievementsCache::load(
                &app_data_dir.join("wervik-steam-achievements.json"),
            );
            let steam_owned_games_cache = steam::owned::SteamOwnedGamesCache::load(
                &app_data_dir.join("wervik-steam-owned-games.json"),
            );
            let metadata_fetch_running = Arc::new(AtomicBool::new(false));
            let metadata_progress = Arc::new(metadata::MetadataProgressState::default());
            let metadata_fetch_generation = Arc::new(AtomicUsize::new(0));
            let retro_achievements_fetch_running = Arc::new(AtomicBool::new(false));
            let steam_achievements_fetch_running = Arc::new(AtomicBool::new(false));
            let steam_owned_games_fetch_running = Arc::new(AtomicBool::new(false));
            for (game_id, legacy) in store.legacy_retro_playtime() {
                let _ = playtime.seed_if_missing(
                    &game_id,
                    legacy.total_minutes.saturating_mul(60),
                    legacy.last_played,
                );
            }
            playtime
                .recover_interrupted_sessions()
                .map_err(std::io::Error::other)?;
            // Directory name is historical (predates GameMode/env var
            // support) — kept as-is so already-pasted Launch Options
            // snippets pointing at existing wrapper scripts keep working.
            let game_setup_dir = app.path().app_config_dir()?.join("mangohud");

            let steam_install = discovery::find_steam_install();
            let (library_folders, games) = match &steam_install {
                Some(install) => {
                    let result = library::scan(&install.root);
                    (result.library_folders, result.games)
                }
                None => (Vec::new(), Vec::new()),
            };
            let xbox_games = xbox::discovery::scan();
            let battlenet_games = battlenet::discovery::scan();
            let ea_games = ea::discovery::scan();
            let battlenet_artwork_running = Arc::new(AtomicBool::new(false));

            let mut rom_games = retro::cache::load(&app_data_dir);
            if retro::cache::repair_cached_platforms(&mut rom_games) {
                let _ = retro::cache::save(&app_data_dir, &rom_games);
            }
            store.migrate_legacy_bios_paths(
                rom_games
                    .iter()
                    .map(|rom| (rom.id.clone(), rom.platform.key().to_string()))
                    .collect(),
            );
            let app_handle = app.handle().clone();
            let artwork_fetch_running = Arc::new(AtomicBool::new(false));
            let rom_scan_running = Arc::new(AtomicBool::new(false));
            let artwork_progress = Arc::new(commands::ArtworkProgressState::default());
            let cloud_sync_running = Arc::new(AtomicBool::new(false));
            let cloud_sync_requested = Arc::new(AtomicBool::new(false));
            let cloud_sync_library_requested = Arc::new(AtomicBool::new(false));
            let cloud_sync_achievements_requested = Arc::new(AtomicBool::new(false));
            let wervik_sync_generation = Arc::new(AtomicUsize::new(0));
            // Picks up box art for anything the last scan left uncovered
            // (e.g. Wervik was closed before a previous fetch finished, or
            // this feature is new and the ROM library predates it).
            // When IGDB is configured it is the primary retro cover source;
            // libretro-thumbnails remains available as the no-credentials
            // fallback without downloading a second cover in parallel.
            let metadata_settings = store.settings();
            let metadata_available = match metadata_settings.metadata_provider {
                MetadataProvider::Igdb => metadata::is_configured(
                    &metadata_settings.igdb_client_id,
                    &metadata_settings.igdb_client_secret,
                ),
                MetadataProvider::Wervik => store
                    .wervik_account()
                    .is_some_and(|account| !account.access_token.trim().is_empty()),
            };
            if !metadata_available {
                commands::spawn_artwork_fetch(
                    app_handle.clone(),
                    app_data_dir.clone(),
                    rom_games.clone(),
                    artwork_fetch_running.clone(),
                    artwork_progress.clone(),
                );
            }
            battlenet::artwork::spawn_fetch(
                app_handle.clone(),
                app_data_dir.clone(),
                battlenet_games.clone(),
                battlenet_artwork_running.clone(),
            );

            let mut state = AppState {
                steam_install,
                library_folders,
                games,
                xbox_games,
                battlenet_games,
                ea_games,
                battlenet_artwork_running,
                playtime,
                metadata_cache,
                nexus_cache,
                retro_achievements_cache,
                retro_achievements_fetch_running,
                steam_achievements_cache,
                steam_achievements_fetch_running,
                steam_owned_games_cache,
                steam_owned_games: Vec::new(),
                steam_owned_games_fetch_running,
                metadata_fetch_running,
                metadata_progress,
                metadata_requests: std::collections::HashMap::new(),
                metadata_fetch_generation,
                store,
                game_setup_dir,
                app_data_dir,
                rom_games,
                rom_scan_running,
                app_handle,
                artwork_fetch_running,
                known_image_paths: std::collections::HashSet::new(),
                artwork_progress,
                cloud_sync_running,
                cloud_sync_requested,
                cloud_sync_library_requested,
                cloud_sync_achievements_requested,
                wervik_sync_generation,
            };
            state.seed_provider_playtime();
            state.dedupe_rom_games_by_metadata();
            // Load the account-scoped owned-games cache before building the
            // metadata queue, so cached non-installed titles get enriched on
            // the same startup pass as locally installed games.
            state.start_steam_owned_fetch();
            state.rebuild_known_image_paths();
            let metadata_queries = state.metadata_queries();
            let achievement_queries = state.achievement_queries();
            let metadata_fetch_running = state.metadata_fetch_running.clone();
            let metadata_progress = state.metadata_progress.clone();
            let metadata_fetch_generation = state.metadata_fetch_generation.clone();
            let achievements_fetch_running = state.retro_achievements_fetch_running.clone();
            let metadata_app_handle = state.app_handle.clone();
            let metadata_app_data_dir = state.app_data_dir.clone();
            let metadata_settings = state.store.settings();
            let metadata_wervik_access_token = state
                .store
                .wervik_account()
                .map(|account| account.access_token)
                .filter(|token| !token.trim().is_empty());
            app.manage(Mutex::new(state));
            commands::schedule_wervik_cloud_sync(app.handle());
            #[cfg(desktop)]
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                for url in urls {
                    commands::finish_wervik_connect(app.handle().clone(), url);
                }
            }
            // Start this after the state is managed: the worker checks the
            // current account/configuration before each request and needs to
            // be able to see the managed state immediately.
            if let Some(state) = app.try_state::<Mutex<AppState>>() {
                let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                state.rebuild_known_image_paths();
                state.start_steam_owned_fetch();
                state.start_steam_achievements_fetch();
            }
            metadata::spawn_fetch(
                metadata_app_handle.clone(),
                metadata_app_data_dir,
                metadata_queries,
                metadata_fetch_running,
                metadata_progress,
                metadata_fetch_generation,
                0,
                metadata_settings.metadata_provider,
                metadata_settings.igdb_client_id,
                metadata_settings.igdb_client_secret,
                metadata_wervik_access_token,
                false,
            );
            retroachievements::spawn_fetch(
                metadata_app_handle,
                achievement_queries,
                metadata_settings.ra_username,
                metadata_settings.ra_api_key,
                achievements_fetch_running,
            );

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_library,
            commands::get_game_screenshot_paths,
            commands::get_game_metadata,
            commands::refresh_game_metadata,
            commands::get_nexus_mods,
            commands::get_retro_achievements,
            commands::get_achievements_overview,
            commands::get_ra_profile_summary,
            commands::get_steam_achievements,
            commands::rescan_library,
            commands::get_artwork_progress,
            commands::get_metadata_progress,
            commands::cancel_metadata_fetch,
            commands::refresh_all_metadata,
            commands::fetch_cover_now,
            commands::launch_game,
            commands::install_steam_game,
            commands::launch_xbox_game,
            commands::launch_battlenet_game,
            commands::launch_ea_game,
            commands::set_favorite,
            commands::set_game_reaction,
            commands::set_retro_hardcore_mode,
            commands::get_settings,
            commands::set_settings,
            commands::get_wervik_account,
            commands::connect_wervik_account,
            commands::disconnect_wervik_account,
            commands::read_image,
            commands::open_install_folder,
            commands::open_xbox_folder,
            commands::open_battlenet_folder,
            commands::open_ea_folder,
            commands::open_nexus_mod,
            commands::open_steam_page,
            commands::get_game_setup_status,
            commands::apply_game_setup,
            commands::get_rom_directories,
            commands::add_rom_directory,
            commands::set_rom_directory_platform,
            commands::remove_rom_directory,
            commands::rescan_retro_library,
            commands::get_rom_scan_status,
            commands::launch_retro_game,
            commands::get_platform_status,
            commands::set_default_emulator,
            commands::set_graphics_preset,
            commands::install_native_emulator,
            commands::open_native_emulator_settings,
            commands::get_platform_setup_status,
            commands::apply_platform_setup,
            commands::open_rom_folder,
            commands::get_bios_path,
            commands::set_bios_path,
            commands::clear_bios_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
