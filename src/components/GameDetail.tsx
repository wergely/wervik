import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  getGameScreenshotPaths,
  getGameMetadata,
  getNexusMods,
  getRetroAchievements,
  getSteamAchievements,
  openInstallFolder,
  openNativeEmulatorSettings,
  openNexusMod,
  openRomFolder,
  openSteamPage,
  openBattleNetFolder,
  openEaFolder,
  openXboxFolder,
  refreshGameMetadata,
} from "../api/tauri";
import { useArtwork } from "../hooks/useArtwork";
import type {
  Game,
  GameMetadata,
  GameReaction,
  NexusMod,
  RaGameAchievements,
  SteamGameAchievements,
} from "../types";
import { formatLastPlayed, formatPlaytime, formatSize, sourceLabel } from "../utils/format";
import { ActiveFeaturesRow } from "./ActiveFeaturesRow";
import { AchievementDetailsModal, type AchievementDetails } from "./AchievementDetailsModal";
import { BiosControl } from "./BiosControl";
import { OverflowMenu } from "./OverflowMenu";
import { SourceBadge } from "./SourceBadge";
import { Spinner } from "./Spinner";
import { Switch } from "./Switch";
import styles from "./GameDetail.module.css";

interface GameDetailProps {
  game: Game;
  onBack: () => void;
  onPlay: (game: Game, variantId?: string | null) => Promise<void>;
  onToggleFavorite: (id: string) => void;
  onSetReaction: (id: string, reaction: GameReaction | null) => void;
  onSetHardcoreMode: (id: string, enabled: boolean) => void;
  registerBackHandler: (handler: (() => void) | null) => void;
  registerNavScope: (el: HTMLElement | null) => void;
  /** Whether this exact game currently has a tracked play session running
   * (Steam/Xbox/Battle.net or a native retro emulator via a backend
   * process monitor — see `App.tsx`). */
  isPlaying: boolean;
  /** The id of whichever game's play session just ended, or `null` — a
   * one-shot signal (not necessarily this game) used to force-refresh the
   * achievements panel right after a session on this game ends. */
  justFinishedGameId: string | null;
}

// A game with a full achievement set can easily be 30-50 entries —
// collapsed to this many by default so the page isn't dominated by it.
const ACHIEVEMENTS_PREVIEW_COUNT = 6;

/** RetroAchievements and Steam achievements render identically except for
 * points/hardcore, which only RetroAchievements has — this is what both
 * `RaGameAchievements` and `SteamGameAchievements` get adapted into so the
 * rendering below doesn't need to care which source it came from. */
interface DisplayAchievements {
  source: "retroachievements" | "steam";
  earnedCount: number;
  totalCount: number;
  pointsEarned: number | null;
  pointsTotal: number | null;
  achievements: DisplayAchievement[];
}

interface DisplayAchievement {
  id: string;
  title: string;
  description: string;
  iconUrl: string;
  unlocked: boolean;
  unlockedHardcore: boolean;
  points: number | null;
  unlockedAt: number | null;
}

function toAchievementDetails(
  achievement: DisplayAchievement,
  source: DisplayAchievements["source"],
  gameName: string,
): AchievementDetails {
  return {
    ...achievement,
    source: source === "steam" ? "Steam" : "RetroAchievements",
    gameName,
  };
}

function fromRa(data: RaGameAchievements): DisplayAchievements {
  return {
    source: "retroachievements",
    earnedCount: data.earnedCount,
    totalCount: data.totalCount,
    pointsEarned: data.pointsEarned,
    pointsTotal: data.pointsTotal,
    achievements: data.achievements.map((achievement) => ({
      id: String(achievement.id),
      title: achievement.title,
      description: achievement.description,
      iconUrl: achievement.badgeUrl,
      unlocked: achievement.unlocked,
      unlockedHardcore: achievement.unlockedHardcore,
      points: achievement.points,
      unlockedAt: achievement.unlockedAt,
    })),
  };
}

function fromSteam(data: SteamGameAchievements): DisplayAchievements {
  return {
    source: "steam",
    earnedCount: data.earnedCount,
    totalCount: data.totalCount,
    pointsEarned: null,
    pointsTotal: null,
    achievements: data.achievements.map((achievement) => ({
      id: achievement.id,
      title: achievement.title,
      description: achievement.description,
      iconUrl: achievement.iconUrl,
      unlocked: achievement.unlocked,
      unlockedHardcore: false,
      points: null,
      unlockedAt: achievement.unlockedAt,
    })),
  };
}

function activeFeatures(game: Game): string[] {
  const features: string[] = [];
  if (game.nativeEmulator) features.push(`Native: ${game.nativeEmulator}`);
  return features;
}

function supportsHardcoreMode(game: Game): boolean {
  return (
    game.source === "retro" &&
    (game.nativeEmulator?.startsWith("RetroArch ·") === true || game.nativeEmulator === "PCSX2")
  );
}

function releaseYear(metadata: GameMetadata): string | null {
  if (!metadata.firstReleaseDate) return null;
  return String(new Date(metadata.firstReleaseDate * 1000).getUTCFullYear());
}

function MetadataScreenshot({
  path,
  index,
  gallery,
  onOpen,
}: {
  path: string;
  index: number;
  gallery: string[];
  onOpen: (path: string, index: number, gallery: string[], trigger: HTMLButtonElement) => void;
}) {
  const image = useArtwork(path);
  if (!image.url || image.failed) return null;
  return (
    <button
      type="button"
      className={styles.screenshotButton}
      data-nav-item
      onClick={(event) => onOpen(path, index, gallery, event.currentTarget)}
      aria-label={`Open screenshot ${index + 1} full screen`}
    >
      <img
        src={image.url}
        alt={`Screenshot ${index + 1}`}
        draggable={false}
        loading="lazy"
        decoding="async"
      />
    </button>
  );
}

function NexusModCard({ mod, onOpen }: { mod: NexusMod; onOpen: (url: string) => void }) {
  const [imageFailed, setImageFailed] = useState(false);
  return (
    <button
      type="button"
      className={styles.nexusModCard}
      data-nav-item
      onClick={() => onOpen(mod.modPageUrl)}
      title={`Open ${mod.name} on Nexus Mods`}
    >
      {mod.pictureUrl && !imageFailed && (
        <img
          className={styles.nexusModImage}
          src={mod.pictureUrl}
          alt=""
          loading="lazy"
          onError={() => setImageFailed(true)}
        />
      )}
      <span className={styles.nexusModBody}>
        <strong className={styles.nexusModName}>{mod.name}</strong>
        {mod.summary && <span className={styles.nexusModSummary}>{mod.summary}</span>}
        {mod.author && <span className={styles.nexusModAuthor}>by {mod.author}</span>}
      </span>
    </button>
  );
}

export function GameDetail({
  game,
  onBack,
  onPlay,
  onToggleFavorite,
  onSetReaction,
  onSetHardcoreMode,
  registerBackHandler,
  registerNavScope,
  isPlaying,
  justFinishedGameId,
}: GameDetailProps) {
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [launching, setLaunching] = useState(false);
  const [metadata, setMetadata] = useState<GameMetadata | null>(null);
  const [metadataLoading, setMetadataLoading] = useState(true);
  const [metadataRefreshing, setMetadataRefreshing] = useState(false);
  const [localScreenshotPaths, setLocalScreenshotPaths] = useState(game.localScreenshotPaths);
  const [nexusMods, setNexusMods] = useState<NexusMod[] | null>(null);
  const [nexusLoading, setNexusLoading] = useState(true);
  const [achievements, setAchievements] = useState<DisplayAchievements | null>(null);
  const [achievementsLoading, setAchievementsLoading] = useState(false);
  const [achievementsRefreshing, setAchievementsRefreshing] = useState(false);
  const [achievementsExpanded, setAchievementsExpanded] = useState(false);
  const [selectedAchievement, setSelectedAchievement] = useState<DisplayAchievement | null>(null);
  const [achievementsError, setAchievementsError] = useState<string | null>(null);
  const metadataRequestRef = useRef(0);
  const achievementsRequestRef = useRef(0);
  const [overflowOpen, setOverflowOpen] = useState(false);
  const [lightboxGallery, setLightboxGallery] = useState<string[]>([]);
  const [lightboxIndex, setLightboxIndex] = useState(0);
  const [lightboxZoom, setLightboxZoom] = useState(1);
  // `null` means "play the default version" — see `Game.variants`.
  const [selectedVariantId, setSelectedVariantId] = useState<string | null>(null);
  const playButtonRef = useRef<HTMLButtonElement>(null);
  const lightboxRef = useRef<HTMLDivElement>(null);
  const lightboxTriggerRef = useRef<HTMLButtonElement>(null);
  const gameHero = useArtwork(game.heroPath ?? game.coverPath);
  const gameCover = useArtwork(game.coverPath);
  const metadataHero = useArtwork(metadata?.heroPath ?? metadata?.coverPath);
  const metadataCover = useArtwork(metadata?.coverPath);
  const lightboxPath = lightboxGallery[lightboxIndex] ?? null;
  const lightboxImage = useArtwork(lightboxPath);
  const hero = gameHero.url && !gameHero.failed ? gameHero : metadataHero;
  const cover = gameCover.url && !gameCover.failed ? gameCover : metadataCover;

  useEffect(() => {
    playButtonRef.current?.focus();
    setOverflowOpen(false);
    setLightboxGallery([]);
    setLightboxIndex(0);
    setSelectedVariantId(null);
    setLaunching(false);
  }, [game.id, game.source, game.appId]);

  useEffect(() => {
    let cancelled = false;
    setLocalScreenshotPaths([]);
    if (game.source !== "retro") return;
    getGameScreenshotPaths(game.id)
      .then((paths) => {
        if (!cancelled) setLocalScreenshotPaths(paths);
      })
      .catch(() => {
        if (!cancelled) setLocalScreenshotPaths([]);
      });
    return () => {
      cancelled = true;
    };
  }, [game.id, game.source]);

  const closeLightbox = useCallback(() => {
    setLightboxGallery([]);
    setLightboxIndex(0);
    setLightboxZoom(1);
    requestAnimationFrame(() => lightboxTriggerRef.current?.focus());
  }, []);

  const moveLightbox = useCallback((direction: -1 | 1) => {
    setLightboxIndex((index) => {
      if (lightboxGallery.length < 2) return index;
      return (index + direction + lightboxGallery.length) % lightboxGallery.length;
    });
    setLightboxZoom(1);
  }, [lightboxGallery.length]);

  useEffect(() => {
    if (lightboxGallery.length === 0) return;

    // App passes a React state setter: wrap the callback so React stores
    // it instead of invoking it as a state updater and closing the preview.
    registerBackHandler(() => closeLightbox);
    registerNavScope(lightboxRef.current);
    requestAnimationFrame(() => lightboxRef.current?.querySelector<HTMLElement>("[data-nav-item]")?.focus());

    const lightbox = lightboxRef.current;
    if (!lightbox) return;
    function handleWheel(event: WheelEvent) {
      event.preventDefault();
      setLightboxZoom((zoom) => Math.min(3, Math.max(1, zoom + (event.deltaY < 0 ? 0.1 : -0.1))));
    }
    lightbox.addEventListener("wheel", handleWheel, { passive: false });

    return () => {
      lightbox.removeEventListener("wheel", handleWheel);
      registerBackHandler(null);
      registerNavScope(null);
    };
  }, [closeLightbox, lightboxGallery.length, registerBackHandler, registerNavScope]);

  useEffect(() => {
    setLightboxZoom(1);
  }, [lightboxPath]);

  function openLightbox(
    _path: string,
    index: number,
    gallery: string[],
    trigger: HTMLButtonElement,
  ) {
    lightboxTriggerRef.current = trigger;
    setLightboxGallery(gallery);
    setLightboxIndex(index);
  }

  useEffect(() => {
    const requestId = ++metadataRequestRef.current;
    let cancelled = false;
    setMetadata(null);
    setMetadataLoading(true);
    getGameMetadata(game.id, game.name, game.platformKey)
      .then((value) => {
        if (!cancelled && requestId === metadataRequestRef.current) setMetadata(value);
      })
      .catch(() => {
        if (!cancelled && requestId === metadataRequestRef.current) setMetadata(null);
      })
      .finally(() => {
        if (!cancelled && requestId === metadataRequestRef.current) setMetadataLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [game.id, game.name, game.platformKey]);

  async function handleRefreshMetadata() {
    const requestId = ++metadataRequestRef.current;
    setMetadataRefreshing(true);
    try {
      const value = await refreshGameMetadata(game.id, game.name, game.platformKey);
      if (requestId === metadataRequestRef.current) setMetadata(value);
    } catch {
      // Keep whatever was already shown — the credit line's button just
      // stops spinning, same as a failed background fetch would.
    } finally {
      setMetadataRefreshing(false);
    }
  }

  useEffect(() => {
    let cancelled = false;
    setNexusMods(null);
    setNexusLoading(true);
    getNexusMods(game.name)
      .then((value) => {
        if (!cancelled) setNexusMods(value);
      })
      .catch(() => {
        if (!cancelled) setNexusMods(null);
      })
      .finally(() => {
        if (!cancelled) setNexusLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [game.id, game.name]);

  const loadAchievements = useCallback(
    (force: boolean) => {
      if (game.source !== "retro" && game.source !== "steam") {
        setAchievements(null);
        return;
      }
      // A monotonic token, not a per-call `cancelled` closure, because this
      // is now called from three places (mount/game-change, the "just
      // finished playing" effect, and the manual refresh button) — a stale
      // response from any of them is discarded the same way.
      const requestId = ++achievementsRequestRef.current;
      if (force) setAchievementsRefreshing(true);
      else setAchievementsLoading(true);
      setAchievementsError(null);
      const request =
        game.source === "retro"
          ? getRetroAchievements(game.id, force).then((value) => (value ? fromRa(value) : null))
          : getSteamAchievements(game.id, force).then((value) => (value ? fromSteam(value) : null));
      request
        .then((value) => {
          if (achievementsRequestRef.current === requestId) setAchievements(value);
        })
        .catch((err) => {
          if (achievementsRequestRef.current === requestId) {
            setAchievements(null);
            setAchievementsError(typeof err === "string" ? err : "Couldn't load achievements.");
          }
        })
        .finally(() => {
          if (achievementsRequestRef.current === requestId) {
            setAchievementsLoading(false);
            setAchievementsRefreshing(false);
          }
        });
    },
    [game.id, game.source],
  );

  useEffect(() => {
    setAchievements(null);
    setAchievementsExpanded(false);
    setSelectedAchievement(null);
    loadAchievements(false);
  }, [loadAchievements]);

  // Force a fresh read right after a play session on this exact game ends
  // — most relevant for games launched through a native RetroArch core,
  // whose real achievements engine may have unlocked something meanwhile.
  useEffect(() => {
    if (justFinishedGameId && justFinishedGameId === game.id) {
      loadAchievements(true);
    }
  }, [justFinishedGameId, game.id, loadAchievements]);

  const playtime = formatPlaytime(game.playtimeMinutes);
  const lastPlayed = formatLastPlayed(game.lastPlayed);
  const orderedAchievements = achievements
    ? [...achievements.achievements].sort((a, b) => Number(b.unlocked) - Number(a.unlocked))
    : [];

  async function handlePlay() {
    if (launching) return;
    setLaunchError(null);
    setLaunching(true);
    const startedAt = Date.now();
    try {
      await onPlay(game, selectedVariantId);
    } catch (err) {
      setLaunchError(typeof err === "string" ? err : "Could not launch the game.");
    } finally {
      // Steam, Xbox and Battle.net report success after handing the request
      // to their own process, while the game window may still be opening.
      // Keep the visual feedback briefly so that handoff is not perceived as
      // an unresponsive Play button.
      const remaining = 450 - (Date.now() - startedAt);
      if (remaining > 0) await new Promise((resolve) => setTimeout(resolve, remaining));
      setLaunching(false);
    }
  }

  async function handleOpenEmulatorSettings() {
    if (!game.platformKey || !game.nativeEmulator) return;
    setLaunchError(null);
    try {
      await openNativeEmulatorSettings(game.platformKey, game.nativeEmulator);
    } catch (err) {
      setLaunchError(typeof err === "string" ? err : "Couldn't open the emulator's settings.");
    }
  }

  function handleOpenNexusMod(url: string) {
    openNexusMod(url).catch(() => {});
  }

  return (
    <div className={styles.detail}>
      {hero.url && !hero.failed ? (
        <div className={styles.heroLayer} style={{ backgroundImage: `url(${hero.url})` }} />
      ) : (
        <div className={styles.heroFallback} />
      )}
      <div className={styles.scrim} />

      <button
        type="button"
        className={styles.backButton}
        data-nav-item
        onClick={onBack}
        aria-label="Back to library"
      >
        ← Library
      </button>

      <div className={styles.content}>
        <div className={styles.sidebar}>
          <div className={styles.cover}>
            {cover.url && !cover.failed && <img src={cover.url} alt="" draggable={false} decoding="async" />}
          </div>

          {/* Play (and everything needed to configure it) lives right below
              the cover art, outside `.info`'s own scroll area — so reading a
              long description or achievements list never scrolls the most
              important control on this screen out of view. */}
          {game.variants.length > 0 && (
            <div className={styles.variantRow}>
              <label className={styles.variantLabel} htmlFor="game-variant-select">
                Version
              </label>
              <select
                id="game-variant-select"
                className={styles.variantSelect}
                data-nav-item
                value={selectedVariantId ?? ""}
                onChange={(e) => setSelectedVariantId(e.target.value || null)}
              >
                <option value="">{game.defaultRegion ?? "Default"}</option>
                {game.variants.map((variant) => (
                  <option key={variant.id} value={variant.id}>
                    {variant.name || variant.region}
                    {variant.name && variant.region !== "Original" ? ` · ${variant.region}` : ""}
                  </option>
                ))}
              </select>
            </div>
          )}

          <div className={styles.actions}>
            <button
              type="button"
              ref={playButtonRef}
              className={`${styles.playButton} ${isPlaying ? styles.playButtonPlaying : ""}`}
              data-nav-item
              onClick={handlePlay}
              disabled={launching || isPlaying}
              aria-busy={launching}
            >
              {launching ? (
                <>
                  <Spinner />
                  {game.isInstalled ? "Launching…" : "Installing…"}
                </>
              ) : isPlaying ? (
                "● Playing"
              ) : !game.isInstalled ? (
                "↓ Install"
              ) : (
                "▶ Play"
              )}
            </button>
            {game.nativeEmulator && game.platformKey && (
              <button
                type="button"
                className={styles.secondaryButton}
                data-nav-item
                onClick={handleOpenEmulatorSettings}
                aria-label={`Open ${game.nativeEmulator} settings — controller, video, audio`}
              >
                ⚙ {game.nativeEmulator} Settings
              </button>
            )}
            <div className={styles.miniActionsRow}>
              <button
                type="button"
                className={`${styles.reactionButton} ${styles.favoriteAction} ${game.isFavorite ? styles.favoriteActive : ""}`}
                data-nav-item
                onClick={() => onToggleFavorite(game.id)}
                aria-pressed={game.isFavorite}
                aria-label={game.isFavorite ? "Remove favorite" : "Add favorite"}
                title={game.isFavorite ? "Remove favorite" : "Add favorite"}
              >
                <svg className={styles.actionIcon} viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    fill="currentColor"
                    d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z"
                  />
                </svg>
              </button>
              <button
                type="button"
                className={`${styles.reactionButton} ${styles.dislikeAction} ${game.reaction === "dislike" ? styles.dislikeActive : ""}`}
                data-nav-item
                onClick={() => onSetReaction(game.id, game.reaction === "dislike" ? null : "dislike")}
                aria-pressed={game.reaction === "dislike"}
                aria-label={game.reaction === "dislike" ? "Remove dislike" : "Dislike this game"}
                title={game.reaction === "dislike" ? "Remove dislike" : "Dislike this game"}
              >
                <svg
                  className={styles.actionIcon}
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <path d="M10 15v4a3 3 0 0 0 3 3l4-8V2H5.72a2 2 0 0 0-2 1.7l-1.38 9A2 2 0 0 0 4.32 15H10Z" />
                  <path d="M18 2h2a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-2" />
                </svg>
              </button>
              <OverflowMenu
                open={overflowOpen}
                onOpen={() => setOverflowOpen(true)}
                onClose={() => setOverflowOpen(false)}
                onShowFiles={
                  game.isInstalled
                    ? () =>
                        (game.source === "steam" && game.appId != null
                          ? openInstallFolder(game.appId)
                          : game.source === "xbox"
                            ? openXboxFolder(game.id)
                            : game.source === "battlenet"
                              ? openBattleNetFolder(game.id)
                              : game.source === "ea"
                                ? openEaFolder(game.id)
                              : openRomFolder(game.id)
                        ).catch(() => {})
                    : undefined
                }
                onOpenSteamPage={
                  game.source === "steam" && game.appId != null
                    ? () => openSteamPage(game.appId!).catch(() => {})
                    : undefined
                }
                registerBackHandler={registerBackHandler}
                registerNavScope={registerNavScope}
              />
            </div>
          </div>

          {game.source === "retro" && game.requiresBios && game.platformKey && (
            <BiosControl platformKey={game.platformKey} platformName={game.platform ?? "This platform"} />
          )}

          {launchError && <p className={styles.launchError}>{launchError}</p>}
        </div>

        <div className={styles.info}>
          <SourceBadge
            label={sourceLabel(game)}
            source={game.source}
            platformKey={game.platformKey}
            className={styles.sourceBadge}
          />
          {!game.isInstalled && <div className={styles.notInstalled}>Not installed</div>}
          {/* Prefers the freshly-fetched IGDB name over `game.name` (from
              the library snapshot) — otherwise a corrected match (e.g.
              after retagging a ROM folder's platform, or hitting "refresh
              metadata") shows the right description/cover immediately but
              keeps displaying the old title until the whole library
              snapshot happens to reload, which nothing here triggers on
              its own. Mirrors the same preference the backend's own
              `library_snapshot` already applies. */}
          <h1 className={styles.title}>{metadata?.name || game.name}</h1>

          <div className={styles.metaRow}>
            {playtime && (
              <div className={styles.metaItem}>
                <span className={styles.metaLabel}>Playtime</span>
                <span className={styles.metaValue}>{playtime}</span>
              </div>
            )}
            {lastPlayed && (
              <div className={styles.metaItem}>
                <span className={styles.metaLabel}>Last played</span>
                <span className={styles.metaValue}>{lastPlayed}</span>
              </div>
            )}
            {game.isInstalled && game.source !== "xbox" && game.source !== "battlenet" && game.source !== "ea" && (
              <div className={styles.metaItem}>
                <span className={styles.metaLabel}>Size</span>
                <span className={styles.metaValue}>{formatSize(game.sizeOnDiskBytes)}</span>
              </div>
            )}
          </div>

          {metadataLoading && <p className={styles.metadataStatus}>Loading game details…</p>}
          {metadata && (
            <section className={styles.metadata} aria-label="Game information">
              {metadata.summary && <p className={`${styles.summary} wervik-scroll`}>{metadata.summary}</p>}
              <div className={styles.metadataFacts}>
                {releaseYear(metadata) && (
                  <div className={styles.metadataFact}>
                    <span className={styles.metaLabel}>Released</span>
                    <span className={styles.metaValue}>{releaseYear(metadata)}</span>
                  </div>
                )}
                {metadata.developers.length > 0 && (
                  <div className={styles.metadataFact}>
                    <span className={styles.metaLabel}>Developer</span>
                    <span className={styles.metaValue}>{metadata.developers.join(", ")}</span>
                  </div>
                )}
                {metadata.publishers.length > 0 && (
                  <div className={styles.metadataFact}>
                    <span className={styles.metaLabel}>Publisher</span>
                    <span className={styles.metaValue}>{metadata.publishers.join(", ")}</span>
                  </div>
                )}
                {metadata.rating != null && (
                  <div className={styles.metadataFact}>
                    <span className={styles.metaLabel}>IGDB rating</span>
                    <span className={styles.metaValue}>
                      {metadata.rating.toFixed(0)}{metadata.ratingCount ? ` (${metadata.ratingCount})` : ""}
                    </span>
                  </div>
                )}
              </div>
              {metadata.genres.length > 0 && (
                <div className={styles.tags} aria-label="Genres">
                  {metadata.genres.map((genre) => (
                    <span key={genre} className={styles.tag}>{genre}</span>
                  ))}
                </div>
              )}
              {metadata.platforms.length > 0 && (
                <div className={styles.metadataPlatforms}>
                  <span className={styles.metaLabel}>Platforms</span>
                  <span className={styles.metaValue}>{metadata.platforms.join(", ")}</span>
                </div>
              )}
              {metadata.screenshotPaths.length > 0 && (
                <div className={styles.screenshots} aria-label="Screenshots">
                  {metadata.screenshotPaths.map((path, index) => (
                    <MetadataScreenshot
                      key={path}
                      path={path}
                      index={index}
                      gallery={metadata.screenshotPaths}
                      onOpen={openLightbox}
                    />
                  ))}
                </div>
              )}
              {metadata.igdbUrl && (
                <div className={styles.metadataCredit}>
                  <span>Metadata from IGDB</span>
                  <button
                    type="button"
                    className={styles.metadataRefreshButton}
                    data-nav-item
                    onClick={handleRefreshMetadata}
                    disabled={metadataRefreshing}
                    aria-label="Wrong match? Refresh metadata for this game"
                    title="Wrong match? Refresh metadata for this game"
                  >
                    {metadataRefreshing ? <Spinner /> : "↻"}
                  </button>
                </div>
              )}
            </section>
          )}

          {localScreenshotPaths.length > 0 && (
            <section className={styles.localScreenshotsSection} aria-label="Achievement screenshots">
              <div className={styles.localScreenshotsHeading}>
                <h2 className={styles.localScreenshotsTitle}>Achievement screenshots</h2>
                <span className={styles.localScreenshotsCredit}>RetroArch</span>
              </div>
              <div className={styles.screenshots}>
                {localScreenshotPaths.map((path, index) => (
                  <MetadataScreenshot
                    key={path}
                    path={path}
                    index={index}
                    gallery={localScreenshotPaths}
                    onOpen={openLightbox}
                  />
                ))}
              </div>
            </section>
          )}

          {achievementsLoading && <p className={styles.metadataStatus}>Loading achievements…</p>}
          {achievementsError && <p className={styles.launchError}>{achievementsError}</p>}
          {achievements && achievements.totalCount > 0 && (
            <section className={styles.achievementsSection} aria-label="Achievements">
              <div className={styles.achievementsHeading}>
                <div className={styles.achievementsTitleGroup}>
                  <h2 className={styles.achievementsTitle}>Achievements</h2>
                  {orderedAchievements.length > ACHIEVEMENTS_PREVIEW_COUNT && (
                    <button
                      type="button"
                      className={styles.achievementsToggle}
                      data-nav-item
                      onClick={() => setAchievementsExpanded((expanded) => !expanded)}
                    >
                      {achievementsExpanded ? "Collapse" : "View all"}
                    </button>
                  )}
                </div>
                <span className={styles.achievementsProgress}>
                    {achievements.earnedCount}/{achievements.totalCount}
                    {achievements.pointsTotal != null &&
                      ` · ${achievements.pointsEarned}/${achievements.pointsTotal} pts`}
                  </span>
                <div className={styles.achievementsActions}>
                  {supportsHardcoreMode(game) && (
                    <div
                      className={styles.hardcoreInline}
                      title="Disables save states, rewind and cheats for this game. Applies on the next launch."
                    >
                      <span>Hardcore mode</span>
                      <Switch
                        checked={game.hardcoreMode}
                        onChange={(enabled) => onSetHardcoreMode(game.id, enabled)}
                        label={`Hardcore mode for ${game.name}`}
                        disabled={isPlaying}
                      />
                    </div>
                  )}
                  <button
                    type="button"
                    className={styles.achievementsRefreshButton}
                    data-nav-item
                    onClick={() => loadAchievements(true)}
                    disabled={achievementsRefreshing}
                    aria-label="Refresh achievements"
                    title="Refresh achievements"
                  >
                    {achievementsRefreshing ? <Spinner /> : "↻"}
                  </button>
                </div>
              </div>
              <div className={styles.achievementsGrid}>
                {(achievementsExpanded
                  ? orderedAchievements
                  : orderedAchievements.slice(0, ACHIEVEMENTS_PREVIEW_COUNT)
                ).map((achievement) => (
                  <button
                    key={achievement.id}
                    type="button"
                    className={`${styles.achievementCard} ${achievement.unlocked ? styles.achievementUnlocked : styles.achievementLocked}`}
                    title={achievement.description || achievement.title}
                    data-nav-item
                    onClick={() => setSelectedAchievement(achievement)}
                    aria-label={`Show details for ${achievement.title || "Unnamed achievement"}`}
                  >
                    <img className={styles.achievementBadge} src={achievement.iconUrl} alt="" loading="lazy" />
                    <div className={styles.achievementInfo}>
                      <div className={styles.achievementTitle} title={achievement.title}>
                        {achievement.title || "Unnamed achievement"}
                      </div>
                      {achievement.description && (
                        <div className={styles.achievementDescription}>{achievement.description}</div>
                      )}
                      <div className={styles.achievementPoints}>
                        {achievement.points != null
                          ? `${achievement.points} pts${achievement.unlockedHardcore ? " · Hardcore" : ""}`
                          : achievement.unlocked
                            ? "Unlocked"
                            : "Locked"}
                      </div>
                    </div>
                  </button>
                ))}
              </div>
              <div className={styles.achievementsCredit}>
                Achievements from {achievements.source === "steam" ? "Steam" : "RetroAchievements"}
              </div>
            </section>
          )}

          {nexusLoading && <p className={styles.metadataStatus}>Loading popular mods…</p>}
          {nexusMods && nexusMods.length > 0 && (
            <section className={styles.nexusSection} aria-label="Popular Nexus Mods">
              <div className={styles.nexusHeading}>
                <h2 className={styles.nexusTitle}>Popular mods</h2>
                <span className={styles.nexusCredit}>Nexus Mods</span>
              </div>
              <div className={styles.nexusGrid}>
                {nexusMods.slice(0, 3).map((mod) => (
                  <NexusModCard key={mod.modPageUrl} mod={mod} onOpen={handleOpenNexusMod} />
                ))}
              </div>
            </section>
          )}

          <ActiveFeaturesRow features={activeFeatures(game)} />
        </div>
      </div>

      {selectedAchievement && achievements && (
        <AchievementDetailsModal
          achievement={toAchievementDetails(selectedAchievement, achievements.source, game.name)}
          onClose={() => setSelectedAchievement(null)}
          registerBackHandler={registerBackHandler}
          registerNavScope={registerNavScope}
        />
      )}

      {lightboxPath && createPortal(
        <div
          ref={lightboxRef}
          className={styles.lightbox}
          role="dialog"
          aria-modal="true"
          aria-label="Screenshot preview"
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              closeLightbox();
            } else if (event.key === "ArrowLeft" && lightboxGallery.length > 1) {
              event.preventDefault();
              event.stopPropagation();
              moveLightbox(-1);
            } else if (event.key === "ArrowRight" && lightboxGallery.length > 1) {
              event.preventDefault();
              event.stopPropagation();
              moveLightbox(1);
            }
          }}
          onClick={(event) => {
            if (event.target === event.currentTarget) closeLightbox();
          }}
        >
          <button
            type="button"
            className={styles.lightboxClose}
            data-nav-item
            onClick={closeLightbox}
            aria-label="Close screenshot preview"
          >
            ×
          </button>
          {lightboxGallery.length > 1 && (
            <>
              <button
                type="button"
                className={`${styles.lightboxNavButton} ${styles.lightboxNavPrevious}`}
                data-nav-item
                onClick={() => moveLightbox(-1)}
                aria-label="Previous screenshot"
              >
                ‹
              </button>
              <button
                type="button"
                className={`${styles.lightboxNavButton} ${styles.lightboxNavNext}`}
                data-nav-item
                onClick={() => moveLightbox(1)}
                aria-label="Next screenshot"
              >
                ›
              </button>
            </>
          )}
          <div className={styles.lightboxContent}>
            {lightboxImage.url && !lightboxImage.failed ? (
              <img
                className={styles.lightboxImage}
                src={lightboxImage.url}
                alt={`Expanded game screenshot ${lightboxIndex + 1}`}
                draggable={false}
                decoding="async"
                style={{ transform: `scale(${lightboxZoom})` }}
              />
            ) : (
              <Spinner />
            )}
          </div>
          <div className={styles.lightboxControls} aria-label="Screenshot zoom controls">
            <span className={styles.lightboxCounter} aria-live="polite">
              {lightboxIndex + 1} / {lightboxGallery.length}
            </span>
            <button
              type="button"
              className={styles.zoomButton}
              data-nav-item
              onClick={() => setLightboxZoom((zoom) => Math.max(1, zoom - 0.1))}
              aria-label="Zoom out"
            >
              −
            </button>
            <span className={styles.zoomValue} aria-live="polite">{Math.round(lightboxZoom * 100)}%</span>
            <button
              type="button"
              className={styles.zoomButton}
              data-nav-item
              onClick={() => setLightboxZoom((zoom) => Math.min(3, zoom + 0.1))}
              aria-label="Zoom in"
            >
              +
            </button>
          </div>
        </div>,
        document.body,
      )}

    </div>
  );
}
