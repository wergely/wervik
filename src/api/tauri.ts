import { invoke } from "@tauri-apps/api/core";
import type {
  AchievementsOverview,
  ArtworkProgress,
  GameMetadata,
  GameReaction,
  GameSetupInput,
  GameSetupStatus,
  LibrarySnapshot,
  MetadataProgress,
  NexusMod,
  PlatformStatus,
  RaGameAchievements,
  RaProfileSummary,
  RetroLaunch,
  RomDirectoryConfig,
  SaveSyncStatus,
  Settings,
  SteamGameAchievements,
  WervikAccount,
} from "../types";

export function getLibrary(): Promise<LibrarySnapshot> {
  return invoke("get_library");
}

export function getGameScreenshotPaths(id: string): Promise<string[]> {
  return invoke("get_game_screenshot_paths", { id });
}

export function getGameMetadata(
  gameId: string,
  gameName: string,
  platformKey?: string | null,
): Promise<GameMetadata | null> {
  return invoke("get_game_metadata", { gameId, gameName, platformKey: platformKey ?? null });
}

/** Bypasses the cache entirely — for a game stuck with a wrong IGDB match. */
export function refreshGameMetadata(
  gameId: string,
  gameName: string,
  platformKey?: string | null,
): Promise<GameMetadata | null> {
  return invoke("refresh_game_metadata", { gameId, gameName, platformKey: platformKey ?? null });
}

export function getNexusMods(gameName: string): Promise<NexusMod[] | null> {
  return invoke("get_nexus_mods", { gameName });
}

export function openNexusMod(url: string): Promise<void> {
  return invoke("open_nexus_mod", { url });
}

/** `Ok(None)` (returned as `null`) covers "nothing to show" — not a ROM,
 * no RetroAchievements credentials configured, or no matching game — none
 * of which are errors the UI needs to surface. `force` bypasses the
 * server-side cache (the refresh button, or right after a play session on
 * this game ends). */
export function getRetroAchievements(gameId: string, force = false): Promise<RaGameAchievements | null> {
  return invoke("get_retro_achievements", { gameId, force });
}

/** Steam counterpart to `getRetroAchievements` — matched by the exact
 * Steam app id, so `null` only ever means "this game has no achievements
 * at all". A rejected promise (most commonly a private Steam profile) is
 * left for the caller to surface, since it's something the user can fix. */
export function getSteamAchievements(gameId: string, force = false): Promise<SteamGameAchievements | null> {
  return invoke("get_steam_achievements", { gameId, force });
}

/** Cross-library achievements overview — reads only what's already cached
 * from each game's own background sweep, so this never triggers a fresh
 * per-game network fetch. */
export function getAchievementsOverview(): Promise<AchievementsOverview> {
  return invoke("get_achievements_overview");
}

/** Account-wide RetroAchievements rank/points. `null` when RA credentials
 * aren't configured in Settings. */
export function getRaProfileSummary(): Promise<RaProfileSummary | null> {
  return invoke("get_ra_profile_summary");
}

export function rescanLibrary(): Promise<LibrarySnapshot> {
  return invoke("rescan_library");
}

export function getArtworkProgress(): Promise<ArtworkProgress> {
  return invoke("get_artwork_progress");
}

export function getMetadataProgress(): Promise<MetadataProgress> {
  return invoke("get_metadata_progress");
}

export function cancelMetadataFetch(): Promise<void> {
  return invoke("cancel_metadata_fetch");
}

export function refreshAllMetadata(): Promise<void> {
  return invoke("refresh_all_metadata");
}

export function getRomScanStatus(): Promise<boolean> {
  return invoke("get_rom_scan_status");
}

/** Jumps one game to the front of the box-art queue — a cache hit (or an
 * eventual `retro-artwork-updated` event, if it wasn't already cached)
 * updates the library, so callers don't need to do anything with the
 * returned path themselves. */
export function fetchCoverNow(id: string): Promise<string | null> {
  return invoke("fetch_cover_now", { id });
}

export function launchGame(appId: number): Promise<void> {
  return invoke("launch_game", { appId });
}

export function installSteamGame(appId: number): Promise<void> {
  return invoke("install_steam_game", { appId });
}

export function launchXboxGame(id: string): Promise<void> {
  return invoke("launch_xbox_game", { id });
}

export function launchBattleNetGame(id: string): Promise<void> {
  return invoke("launch_battlenet_game", { id });
}

export function launchEaGame(id: string): Promise<void> {
  return invoke("launch_ea_game", { id });
}

export function openSteamPage(appId: number): Promise<void> {
  return invoke("open_steam_page", { appId });
}

/** `id` is the unified game id (`Game.id`) — works for Steam and retro games. */
export function setFavorite(id: string, favorite: boolean): Promise<void> {
  return invoke("set_favorite", { id, favorite });
}

export function setGameReaction(id: string, reaction: GameReaction | null): Promise<void> {
  return invoke("set_game_reaction", { id, reaction });
}

/** Stores Hardcore mode for this individual retro game. */
export function setRetroHardcoreMode(id: string, enabled: boolean): Promise<void> {
  return invoke("set_retro_hardcore_mode", { id, enabled });
}

export function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}

export function setSettings(settings: Settings): Promise<void> {
  return invoke("set_settings", { settings });
}

export function getWervikAccount(): Promise<WervikAccount | null> {
  return invoke("get_wervik_account");
}

export function connectWervikAccount(): Promise<void> {
  return invoke("connect_wervik_account");
}

export function disconnectWervikAccount(): Promise<void> {
  return invoke("disconnect_wervik_account");
}

export function readImage(path: string): Promise<string> {
  return invoke("read_image", { path });
}

export function openInstallFolder(appId: number): Promise<void> {
  return invoke("open_install_folder", { appId });
}

export function openXboxFolder(id: string): Promise<void> {
  return invoke("open_xbox_folder", { id });
}

export function openBattleNetFolder(id: string): Promise<void> {
  return invoke("open_battlenet_folder", { id });
}

export function openEaFolder(id: string): Promise<void> {
  return invoke("open_ea_folder", { id });
}

export function getGameSetupStatus(appId: number): Promise<GameSetupStatus> {
  return invoke("get_game_setup_status", { appId });
}

export function applyGameSetup(appId: number, input: GameSetupInput): Promise<GameSetupStatus> {
  return invoke("apply_game_setup", { appId, input });
}

export function getRomDirectories(): Promise<RomDirectoryConfig[]> {
  return invoke("get_rom_directories");
}

export function addRomDirectory(path: string, platformKey?: string | null): Promise<LibrarySnapshot> {
  return invoke("add_rom_directory", { path, platformKey: platformKey ?? null });
}

/** `platformKey: null` reverts the directory to auto-detection. */
export function setRomDirectoryPlatform(path: string, platformKey: string | null): Promise<LibrarySnapshot> {
  return invoke("set_rom_directory_platform", { path, platformKey });
}

export function removeRomDirectory(path: string): Promise<LibrarySnapshot> {
  return invoke("remove_rom_directory", { path });
}

export function rescanRetroLibrary(): Promise<LibrarySnapshot> {
  return invoke("rescan_retro_library");
}

/** `variantId` picks a different regional dump of this same game to play
 * instead of the default (see `Game.variants`); omit for the default. */
export function launchRetroGame(id: string, variantId?: string | null): Promise<RetroLaunch> {
  return invoke("launch_retro_game", { id, variantId: variantId ?? null });
}

export function getGameSyncStatus(gameId: string): Promise<SaveSyncStatus> {
  return invoke("get_game_sync_status", { gameId });
}

/** Pulls newer remote saves down, then pushes local changes up, without
 * requiring the game to be running. */
export function syncGameSaves(gameId: string): Promise<SaveSyncStatus> {
  return invoke("sync_game_saves", { gameId });
}

export function getBiosPath(platform: string): Promise<string | null> {
  return invoke("get_bios_path", { platform });
}

export function setBiosPath(platform: string, sourcePath: string): Promise<string> {
  return invoke("set_bios_path", { platform, sourcePath });
}

export function clearBiosPath(platform: string): Promise<void> {
  return invoke("clear_bios_path", { platform });
}

export function openRomFolder(id: string): Promise<void> {
  return invoke("open_rom_folder", { id });
}

export function getPlatformStatus(): Promise<PlatformStatus[]> {
  return invoke("get_platform_status");
}

/** Sets the platform's "Default Emulator" — a native emulator's id. Takes
 * effect immediately, no separate apply step. */
export function setDefaultEmulator(platform: string, emulatorId: string): Promise<void> {
  return invoke("set_default_emulator", { platform, emulatorId });
}

/** Sets a platform's graphics preset (see `PlatformStatus.graphicsPreset`) —
 * only valid for a platform that has one at all. Rejects if that
 * platform's RetroArch core isn't installed yet. */
export function setGraphicsPreset(platform: string, optimized: boolean): Promise<void> {
  return invoke("set_graphics_preset", { platform, optimized });
}

export function getPlatformSetupStatus(platform: string): Promise<GameSetupStatus> {
  return invoke("get_platform_setup_status", { platform });
}

export function applyPlatformSetup(platform: string, input: GameSetupInput): Promise<GameSetupStatus> {
  return invoke("apply_platform_setup", { platform, input });
}

/** Downloads and installs a managed native emulator (see
 * `EmulatorOption.managed`) into Wervik's own app data directory. */
export function installNativeEmulator(platform: string, emulatorId: string): Promise<void> {
  return invoke("install_native_emulator", { platform, emulatorId });
}

/** Opens a native emulator's own UI with no ROM loaded, so the user can
 * use its real controller/video/audio settings. */
export function openNativeEmulatorSettings(platform: string, emulatorId: string): Promise<void> {
  return invoke("open_native_emulator_settings", { platform, emulatorId });
}
