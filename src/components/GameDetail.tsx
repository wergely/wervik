import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
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
import { BiosControl } from "./BiosControl";
import { OverflowMenu } from "./OverflowMenu";
import { SourceBadge } from "./SourceBadge";
import { Spinner } from "./Spinner";
import styles from "./GameDetail.module.css";

interface GameDetailProps {
  game: Game;
  onBack: () => void;
  onPlay: (game: Game, variantId?: string | null) => Promise<void>;
  onToggleFavorite: (id: string) => void;
  onSetReaction: (id: string, reaction: GameReaction | null) => void;
  registerBackHandler: (handler: (() => void) | null) => void;
  registerNavScope: (el: HTMLElement | null) => void;
  /** Whether this exact game currently has a tracked play session running
   * (Steam/Xbox/Battle.net or a native retro emulator via a backend
   * process monitor, or an EmulatorJS session — see `App.tsx`). */
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
  achievements: {
    id: string;
    title: string;
    description: string;
    iconUrl: string;
    unlocked: boolean;
    unlockedHardcore: boolean;
    points: number | null;
  }[];
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
    })),
  };
}

function activeFeatures(game: Game): string[] {
  const features: string[] = [];
  if (game.nativeEmulator) features.push(`Native: ${game.nativeEmulator}`);
  return features;
}

function releaseYear(metadata: GameMetadata): string | null {
  if (!metadata.firstReleaseDate) return null;
  return String(new Date(metadata.firstReleaseDate * 1000).getUTCFullYear());
}

function MetadataScreenshot({
  path,
  index,
  onOpen,
}: {
  path: string;
  index: number;
  onOpen: (path: string, trigger: HTMLButtonElement) => void;
}) {
  const image = useArtwork(path);
  if (!image.url || image.failed) return null;
  return (
    <button
      type="button"
      className={styles.screenshotButton}
      data-nav-item
      onClick={(event) => onOpen(path, event.currentTarget)}
      aria-label={`Open screenshot ${index + 1} full screen`}
    >
      <img src={image.url} alt={`Screenshot ${index + 1}`} draggable={false} />
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
  const [nexusMods, setNexusMods] = useState<NexusMod[] | null>(null);
  const [nexusLoading, setNexusLoading] = useState(true);
  const [achievements, setAchievements] = useState<DisplayAchievements | null>(null);
  const [achievementsLoading, setAchievementsLoading] = useState(false);
  const [achievementsRefreshing, setAchievementsRefreshing] = useState(false);
  const [achievementsExpanded, setAchievementsExpanded] = useState(false);
  const [achievementsError, setAchievementsError] = useState<string | null>(null);
  const achievementsRequestRef = useRef(0);
  const [overflowOpen, setOverflowOpen] = useState(false);
  const [lightboxPath, setLightboxPath] = useState<string | null>(null);
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
  const lightboxImage = useArtwork(lightboxPath);
  const hero = gameHero.url && !gameHero.failed ? gameHero : metadataHero;
  const cover = gameCover.url && !gameCover.failed ? gameCover : metadataCover;

  useEffect(() => {
    playButtonRef.current?.focus();
    setOverflowOpen(false);
    setLightboxPath(null);
    setSelectedVariantId(null);
    setLaunching(false);
  }, [game.id, game.source, game.appId]);

  const closeLightbox = useCallback(() => {
    setLightboxPath(null);
    setLightboxZoom(1);
    requestAnimationFrame(() => lightboxTriggerRef.current?.focus());
  }, []);

  useEffect(() => {
    if (!lightboxPath) return;

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
  }, [closeLightbox, lightboxPath, registerBackHandler, registerNavScope]);

  useEffect(() => {
    setLightboxZoom(1);
  }, [lightboxPath]);

  function openLightbox(path: string, trigger: HTMLButtonElement) {
    lightboxTriggerRef.current = trigger;
    setLightboxPath(path);
  }

  useEffect(() => {
    let cancelled = false;
    setMetadata(null);
    setMetadataLoading(true);
    getGameMetadata(game.id, game.name, game.platformKey)
      .then((value) => {
        if (!cancelled) setMetadata(value);
      })
      .catch(() => {
        if (!cancelled) setMetadata(null);
      })
      .finally(() => {
        if (!cancelled) setMetadataLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [game.id, game.name]);

  async function handleRefreshMetadata() {
    setMetadataRefreshing(true);
    try {
      const value = await refreshGameMetadata(game.id, game.name, game.platformKey);
      setMetadata(value);
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
    loadAchievements(false);
  }, [loadAchievements]);

  // Force a fresh read right after a play session on this exact game ends
  // — most relevant for native RetroArch platforms (Arcade, CPS, Neo Geo),
  // whose real achievements engine may have unlocked something meanwhile.
  useEffect(() => {
    if (justFinishedGameId && justFinishedGameId === game.id) {
      loadAchievements(true);
    }
  }, [justFinishedGameId, game.id, loadAchievements]);

  const playtime = formatPlaytime(game.playtimeMinutes);
  const lastPlayed = formatLastPlayed(game.lastPlayed);

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
            {cover.url && !cover.failed && <img src={cover.url} alt="" draggable={false} />}
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
            <button
              type="button"
              className={`${styles.secondaryButton} ${game.isFavorite ? styles.active : ""}`}
              data-nav-item
              onClick={() => onToggleFavorite(game.id)}
              aria-pressed={game.isFavorite}
            >
              {game.isFavorite ? "★ Favorited" : "☆ Favorite"}
            </button>
            <div className={styles.miniActionsRow}>
              <button
                type="button"
                className={`${styles.reactionButton} ${game.reaction === "like" ? styles.likeActive : ""}`}
                data-nav-item
                onClick={() => onSetReaction(game.id, game.reaction === "like" ? null : "like")}
                aria-pressed={game.reaction === "like"}
                aria-label={game.reaction === "like" ? "Remove like" : "Like this game"}
              >
                <span aria-hidden="true">👍</span>
              </button>
              <button
                type="button"
                className={`${styles.reactionButton} ${game.reaction === "dislike" ? styles.dislikeActive : ""}`}
                data-nav-item
                onClick={() => onSetReaction(game.id, game.reaction === "dislike" ? null : "dislike")}
                aria-pressed={game.reaction === "dislike"}
                aria-label={game.reaction === "dislike" ? "Remove dislike" : "Dislike this game"}
              >
                <span aria-hidden="true">👎</span>
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

          {achievementsLoading && <p className={styles.metadataStatus}>Loading achievements…</p>}
          {achievementsError && <p className={styles.launchError}>{achievementsError}</p>}
          {achievements && achievements.totalCount > 0 && (
            <section className={styles.achievementsSection} aria-label="Achievements">
              <div className={styles.achievementsHeading}>
                <h2 className={styles.achievementsTitle}>Achievements</h2>
                <span className={styles.achievementsProgress}>
                  {achievements.earnedCount}/{achievements.totalCount}
                  {achievements.pointsTotal != null &&
                    ` · ${achievements.pointsEarned}/${achievements.pointsTotal} pts`}
                </span>
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
              <div className={styles.achievementsGrid}>
                {(achievementsExpanded
                  ? achievements.achievements
                  : achievements.achievements.slice(0, ACHIEVEMENTS_PREVIEW_COUNT)
                ).map((achievement) => (
                  <div
                    key={achievement.id}
                    className={`${styles.achievementCard} ${achievement.unlocked ? styles.achievementUnlocked : styles.achievementLocked}`}
                    title={achievement.description || achievement.title}
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
                  </div>
                ))}
              </div>
              {achievements.achievements.length > ACHIEVEMENTS_PREVIEW_COUNT && (
                <button
                  type="button"
                  className={styles.achievementsToggle}
                  data-nav-item
                  onClick={() => setAchievementsExpanded((expanded) => !expanded)}
                >
                  {achievementsExpanded ? "Show fewer" : `Show all ${achievements.achievements.length}`}
                </button>
              )}
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

      {lightboxPath && createPortal(
        <div
          ref={lightboxRef}
          className={styles.lightbox}
          role="dialog"
          aria-modal="true"
          aria-label="Screenshot preview"
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            event.preventDefault();
            event.stopPropagation();
            closeLightbox();
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
          <div className={styles.lightboxContent}>
            {lightboxImage.url && !lightboxImage.failed ? (
              <img
                className={styles.lightboxImage}
                src={lightboxImage.url}
                alt="Expanded game screenshot"
                draggable={false}
                style={{ transform: `scale(${lightboxZoom})` }}
              />
            ) : (
              <Spinner />
            )}
          </div>
          <div className={styles.lightboxControls} aria-label="Screenshot zoom controls">
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
