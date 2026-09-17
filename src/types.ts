export type GameSource = "steam" | "retro" | "xbox" | "battlenet" | "ea";
export type GameReaction = "like" | "dislike";

export interface Game {
  /** Unified id: a Steam app id as a string, or a ROM's stable id. Use this
   * for favorites/navigation; use `appId` only for Steam-specific actions. */
  id: string;
  source: GameSource;
  name: string;
  /** `false` only for Steam games owned by the account but not installed. */
  isInstalled: boolean;
  installDir: string;
  libraryPath: string;
  sizeOnDiskBytes: number;
  lastPlayed: number | null;
  playtimeMinutes: number | null;
  igdbScore: number | null;
  igdbMetadata: GameMetadataSummary | null;
  reaction: GameReaction | null;
  coverPath: string | null;
  heroPath: string | null;
  logoPath: string | null;
  /** Local RetroArch screenshots captured when this game unlocks an achievement. */
  localScreenshotPaths: string[];
  isFavorite: boolean;
  /** Present only when `source === "steam"`. */
  appId: number | null;
  /** Present only when `source === "xbox"`; Windows AppUserModelId used to launch it. */
  xboxAppId: string | null;
  /** Present only when `source === "battlenet"`; Battle.net product code. */
  battlenetProductCode: string | null;
  /** Present only when `source === "ea"`; EA App content/offer id. */
  eaGameId: string | null;
  /** Short platform label (e.g. "SNES"), present only when `source === "retro"`. */
  platform: string | null;
  /** Stable machine key (see `PlatformStatus.key`) for joining against the
   * Platforms view — `platform` above is a display label, this isn't. */
  platformKey: string | null;
  requiresBios: boolean | null;
  /** Name of the native emulator this game will actually launch with (e.g.
   * "gopher64"). `null` means the platform still needs an emulator
   * installed. */
  nativeEmulator: string | null;
  /** Region label for the copy that plays by default (e.g. "USA") — `null`
   * for Steam games. */
  defaultRegion: string | null;
  /** Other regional dumps of this same game the user can play instead
   * (see `launchRetroGame`'s `variantId`) — empty for Steam games. */
  variants: GameVariant[];
  /** Achievement completion percentage — RetroAchievements for a retro
   * game, the Steam Web API for a Steam one — from whatever the relevant
   * background sweep has cached so far. `null` for Xbox/Battle.net, when
   * the relevant credentials aren't configured, or before a match/result
   * is cached yet. */
  achievementCompletionPercent: number | null;
  /** Whether this retro game launches with RetroAchievements Hardcore mode. */
  hardcoreMode: boolean;
}

export interface GameVariant {
  id: string;
  /** Original ROM title, useful when localized names are grouped together. */
  name: string;
  region: string;
}

export interface GameMetadata {
  name: string | null;
  igdbId: number;
  igdbUrl: string | null;
  summary: string | null;
  firstReleaseDate: number | null;
  developers: string[];
  publishers: string[];
  genres: string[];
  platforms: string[];
  rating: number | null;
  ratingCount: number | null;
  coverPath: string | null;
  heroPath: string | null;
  screenshotPaths: string[];
  fetchedAt: number;
}

export interface NexusMod {
  name: string;
  author: string | null;
  summary: string | null;
  pictureUrl: string | null;
  modPageUrl: string;
}

export interface RaAchievement {
  id: number;
  title: string;
  description: string;
  points: number;
  badgeUrl: string;
  unlocked: boolean;
  unlockedHardcore: boolean;
  unlockedAt: number | null;
}

export interface RaGameAchievements {
  gameTitle: string;
  consoleName: string;
  iconUrl: string | null;
  earnedCount: number;
  totalCount: number;
  pointsEarned: number;
  pointsTotal: number;
  achievements: RaAchievement[];
}

/** Steam's own achievements have no point value or hardcore-mode concept
 * — unlike `RaAchievement`, this is the complete shape. */
export interface SteamAchievement {
  id: string;
  title: string;
  description: string;
  iconUrl: string;
  unlocked: boolean;
  unlockedAt: number | null;
}

export interface SteamGameAchievements {
  earnedCount: number;
  totalCount: number;
  achievements: SteamAchievement[];
}

export type AchievementSource = "retro" | "steam";

export interface GameAchievementSummary {
  gameId: string;
  gameName: string;
  source: AchievementSource;
  platform: string | null;
  platformKey: string | null;
  iconUrl: string | null;
  earnedCount: number;
  totalCount: number;
  pointsEarned: number;
  pointsTotal: number;
  completionPercent: number;
}

export interface RecentUnlock {
  gameId: string;
  gameName: string;
  source: AchievementSource;
  achievementTitle: string;
  achievementDescription: string;
  iconUrl: string;
  points: number;
  unlockedAt: number;
}

export interface AchievementsOverview {
  totalEarned: number;
  totalPossible: number;
  totalPointsEarned: number;
  totalPointsPossible: number;
  games: GameAchievementSummary[];
  recentUnlocks: RecentUnlock[];
}

export interface RaProfileSummary {
  username: string;
  avatarUrl: string | null;
  rank: number;
  totalPoints: number;
  totalTruePoints: number;
}

export interface LibrarySnapshot {
  steamFound: boolean;
  steamRoot: string | null;
  xboxFound: boolean;
  battlenetFound: boolean;
  eaFound: boolean;
  libraryFolders: string[];
  games: Game[];
}

export interface Settings {
  fullscreen: boolean;
  /** Ask supported game windows to open maximized without entering fullscreen. */
  maximizeGames: boolean;
  launchOnStartup: boolean;
  uiScale: number;
  metadataProvider: MetadataProvider;
  /** The Library grid's own sort/filter selections — see `LibrarySection`. */
  librarySortMode: SortMode;
  /** The Library grid's current search text. */
  librarySearch: string;
  /** Empty means all sources; otherwise the selected source badges. */
  librarySourceFilter: GameSource[];
  /** Empty means all retro platforms; otherwise the selected platforms. */
  libraryPlatformFilter: string[];
  libraryFavoritesOnly: boolean;
  /** Advanced filters sourced from locally cached IGDB metadata. */
  libraryIgdbGenreFilter: string[];
  libraryIgdbPlatformFilter: string[];
  libraryIgdbDeveloperFilter: string[];
  libraryIgdbPublisherFilter: string[];
  libraryIgdbMinScore: number | null;
  libraryIgdbYearFrom: number | null;
  libraryIgdbYearTo: number | null;
  libraryIgdbMetadataOnly: boolean;
  /** Background chiptune player heard while browsing Home — see
   * `useChiptunePlayer`. Generated locally; no audio files or network. */
  musicEnabled: boolean;
  /** 0–1. */
  musicVolume: number;
  /** IGDB credentials entered in Settings, as an alternative to the
   * WERVIK_IGDB_CLIENT_ID/SECRET environment variables — see the Rust
   * `metadata::credentials`, which prefers these when both are set. */
  igdbClientId: string;
  igdbClientSecret: string;
  /** RetroAchievements credentials. The password is also used by managed
   * emulators for real-time achievements and is persisted with the settings. */
  raUsername: string;
  raApiKey: string;
  raPassword: string;
  /** Steam Web API key (from steamcommunity.com/dev/apikey) for the same
   * kind of read-only achievements viewer, but for Steam games — matched
   * by the exact Steam app id, no fuzzy title guess needed. The account's
   * SteamID64 is detected automatically from the local Steam install;
   * `steamId64Override` is only a manual escape hatch if auto-detection
   * ever picks the wrong one on a machine with more than one local
   * account. */
  steamApiKey: string;
  steamId64Override: string;
}

export type MetadataProvider = "igdb" | "wervik";

export interface WervikAccount {
  name: string;
  email: string;
  connectedAt: number;
  installationId: string;
  /** Opaque credential used by the desktop client to sync account settings. */
  accessToken: string;
}

export type SortMode = "recent" | "alphabetical" | "playtime" | "score" | "completion";

export interface GameMetadataSummary {
  igdbId: number;
  firstReleaseDate: number | null;
  developers: string[];
  publishers: string[];
  genres: string[];
  platforms: string[];
  rating: number | null;
  ratingCount: number | null;
}

export type OverlayPosition =
  | "top-left"
  | "top-right"
  | "bottom-left"
  | "bottom-right";

export interface MangoHudConfig {
  fps: boolean;
  frameTiming: boolean;
  gpuStats: boolean;
  cpuStats: boolean;
  ram: boolean;
  vram: boolean;
  position: OverlayPosition;
  fpsLimit: number;
  toggleHud: string;
  noDisplay: boolean;
}

export interface EnvVar {
  key: string;
  value: string;
}

export type Upscaler = "none" | "fsr" | "nis";

export interface GamescopeConfig {
  /** 0 means "not set" — gamescope runs at its default/native resolution. */
  internalWidth: number;
  internalHeight: number;
  outputWidth: number;
  outputHeight: number;
  upscaler: Upscaler;
}

export interface GameSetupStatus {
  mangohudInstalled: boolean;
  mangohudEnabled: boolean;
  mangohudConfig: MangoHudConfig;
  gamemodeInstalled: boolean;
  gamemodeEnabled: boolean;
  gamescopeInstalled: boolean;
  gamescopeEnabled: boolean;
  gamescopeConfig: GamescopeConfig;
  envVars: EnvVar[];
  /** Whether the game's Steam Launch Options already reference our wrapper.
   * Absent for platform-level setup (native emulator launches), which has
   * no external app to configure — see `PlatformSetupStatus` in Rust. */
  steamConfigured?: boolean;
  /** The exact Launch Options value to paste into Steam's Properties dialog.
   * Absent for platform-level setup — see `steamConfigured`. */
  launchOptionsSnippet?: string;
  hasCustomConfig: boolean;
}

export interface GameSetupInput {
  mangohudEnabled: boolean;
  mangohudConfig: MangoHudConfig;
  gamemodeEnabled: boolean;
  gamescopeEnabled: boolean;
  gamescopeConfig: GamescopeConfig;
  envVars: EnvVar[];
}

export type RetroLaunch = { mode: "native" };

export type EmulatorKind = "native";

export interface EmulatorOption {
  /** Stable id for a native emulator. */
  id: string;
  displayName: string;
  kind: EmulatorKind;
  /** Whether the native emulator is installed and ready to launch. */
  available: boolean;
  /** Whether Wervik can download and install this option itself — drives
   * whether to show a "Download" action instead of just "Not installed". */
  managed: boolean;
  /** The best native default for this platform. */
  recommended: boolean;
}

export interface RomDirectoryConfig {
  path: string;
  /** Explicit platform override for every ROM under this directory — see
   * `Platform::key()` on the Rust side for the exact string. `null` keeps
   * today's auto-detection by file extension/content. */
  platformKey: string | null;
}

export interface PlatformStatus {
  key: string;
  displayName: string;
  /** Short label (e.g. "N64", "SNES") — used as the Platforms grid card's
   * stable monogram. */
  shortName: string;
  romCount: number;
  /** Every native emulator Wervik knows how to launch for this platform. */
  emulatorOptions: EmulatorOption[];
  /** The user's chosen default (see `EmulatorOption.id`) — always one of
   * `emulatorOptions`' ids, even when that option is currently
   * unavailable (the UI shows that state rather than hiding it). */
  selectedEmulator: string;
  /** What will actually be used right now; `null` when the selected native
   * emulator is not installed. */
  effectiveEmulator: string | null;
  /** Whether games on this platform need a user-supplied BIOS file. */
  requiresBios: boolean;
  /** Shared BIOS path configured for this platform, if any. */
  biosPath: string | null;
  /** `"original"` or `"optimized"` — only for the platforms whose native
   * RetroArch core can meaningfully render above the original hardware's
   * resolution (N64, PlayStation). `null` means this platform has no such
   * preset to offer at all. */
  graphicsPreset: string | null;
}

/** Progress of the background box-art fetch (see `retro::artwork`).
 * `total` is how many ROMs needed a cover when the current pass started;
 * `done` is how many of those have been attempted (found or not) so far. */
export interface ArtworkProgress {
  done: number;
  total: number;
}

/** Progress of the background IGDB metadata fetch. */
export interface MetadataProgress {
  done: number;
  total: number;
}
