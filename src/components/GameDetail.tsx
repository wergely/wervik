import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  getGameMetadata,
  getNexusMods,
  openInstallFolder,
  openNativeEmulatorSettings,
  openNexusMod,
  openRomFolder,
  openSteamPage,
  openBattleNetFolder,
  openXboxFolder,
} from "../api/tauri";
import { useArtwork } from "../hooks/useArtwork";
import type { Game, GameMetadata, GameReaction, NexusMod } from "../types";
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
}: GameDetailProps) {
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [launching, setLaunching] = useState(false);
  const [metadata, setMetadata] = useState<GameMetadata | null>(null);
  const [metadataLoading, setMetadataLoading] = useState(true);
  const [nexusMods, setNexusMods] = useState<NexusMod[] | null>(null);
  const [nexusLoading, setNexusLoading] = useState(true);
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

    registerBackHandler(closeLightbox);
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
    getGameMetadata(game.id, game.name)
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
        <div className={styles.cover}>
          {cover.url && !cover.failed && <img src={cover.url} alt="" draggable={false} />}
        </div>

        <div className={styles.info}>
          <SourceBadge label={sourceLabel(game)} source={game.source} className={styles.sourceBadge} />
          <h1 className={styles.title}>{game.name}</h1>

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
            {game.source !== "xbox" && game.source !== "battlenet" && (
              <div className={styles.metaItem}>
                <span className={styles.metaLabel}>Size</span>
                <span className={styles.metaValue}>{formatSize(game.sizeOnDiskBytes)}</span>
              </div>
            )}
          </div>

          {metadataLoading && <p className={styles.metadataStatus}>Loading game details…</p>}
          {metadata && (
            <section className={styles.metadata} aria-label="Game information">
              {metadata.summary && <p className={styles.summary}>{metadata.summary}</p>}
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
              {metadata.igdbUrl && <div className={styles.metadataCredit}>Metadata from IGDB</div>}
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
                {nexusMods.map((mod) => (
                  <NexusModCard key={mod.modPageUrl} mod={mod} onOpen={handleOpenNexusMod} />
                ))}
              </div>
            </section>
          )}

          <ActiveFeaturesRow features={activeFeatures(game)} />

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
              className={styles.playButton}
              data-nav-item
              onClick={handlePlay}
              disabled={launching}
              aria-busy={launching}
            >
              {launching ? (
                <>
                  <Spinner />
                  Launching…
                </>
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
              onShowFiles={() =>
                (game.source === "steam" && game.appId != null
                  ? openInstallFolder(game.appId)
                  : game.source === "xbox"
                    ? openXboxFolder(game.id)
                    : game.source === "battlenet"
                      ? openBattleNetFolder(game.id)
                      : openRomFolder(game.id)
                ).catch(() => {})
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

          {game.source === "retro" && game.requiresBios && game.platformKey && (
            <BiosControl platformKey={game.platformKey} platformName={game.platform ?? "This platform"} />
          )}

          {launchError && <p className={styles.launchError}>{launchError}</p>}
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
