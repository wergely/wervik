import { invoke } from "@tauri-apps/api/core";
import type {
  ArtworkProgress,
  GameMetadata,
  GameReaction,
  GameSetupInput,
  GameSetupStatus,
  LibrarySnapshot,
  MetadataProgress,
  NexusMod,
  PlatformStatus,
  RetroLaunch,
  Settings,
} from "../types";

export function getLibrary(): Promise<LibrarySnapshot> {
  return invoke("get_library");
}

export function getGameMetadata(gameId: string, gameName: string): Promise<GameMetadata | null> {
  return invoke("get_game_metadata", { gameId, gameName });
}

export function getNexusMods(gameName: string): Promise<NexusMod[] | null> {
  return invoke("get_nexus_mods", { gameName });
}

export function openNexusMod(url: string): Promise<void> {
  return invoke("open_nexus_mod", { url });
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

export function launchXboxGame(id: string): Promise<void> {
  return invoke("launch_xbox_game", { id });
}

export function launchBattleNetGame(id: string): Promise<void> {
  return invoke("launch_battlenet_game", { id });
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

export function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}

export function setSettings(settings: Settings): Promise<void> {
  return invoke("set_settings", { settings });
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

export function getGameSetupStatus(appId: number): Promise<GameSetupStatus> {
  return invoke("get_game_setup_status", { appId });
}

export function applyGameSetup(appId: number, input: GameSetupInput): Promise<GameSetupStatus> {
  return invoke("apply_game_setup", { appId, input });
}

export function getRomDirectories(): Promise<string[]> {
  return invoke("get_rom_directories");
}

export function addRomDirectory(path: string): Promise<LibrarySnapshot> {
  return invoke("add_rom_directory", { path });
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

export function stopRetroSession(sessionId: string, minutesPlayed: number): Promise<void> {
  return invoke("stop_retro_session", { sessionId, minutesPlayed });
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

/** Sets the platform's "Default Emulator" — `"emulatorjs"` or a native
 * emulator's id. Takes effect immediately, no separate apply step. */
export function setDefaultEmulator(platform: string, emulatorId: string): Promise<void> {
  return invoke("set_default_emulator", { platform, emulatorId });
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
