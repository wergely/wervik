import { useId } from "react";
import { useArtwork } from "../hooks/useArtwork";
import type { Game, GameReaction } from "../types";
import { sourceLabel } from "../utils/format";
import { SourceBadge } from "./SourceBadge";
import styles from "./SuggestedGameCard.module.css";

interface SuggestedGameCardProps {
  game: Game;
  reason: string;
  onSelect: (game: Game) => void;
  onSetReaction: (id: string, reaction: GameReaction | null) => void;
}

function releaseYear(game: Game): string | null {
  const timestamp = game.igdbMetadata?.firstReleaseDate;
  return timestamp ? String(new Date(timestamp * 1000).getUTCFullYear()) : null;
}

export function SuggestedGameCard({ game: displayedSuggestion, reason, onSelect, onSetReaction }: SuggestedGameCardProps) {
  const reasonId = useId();
  const { url, failed, ref: artworkRef } = useArtwork(displayedSuggestion.coverPath, { lazy: true });
  return (
      <div className={styles.cardWrap}>
        <button
          type="button"
          className={styles.card}
          data-nav-item
          data-nav-key={displayedSuggestion.id}
          onClick={() => onSelect(displayedSuggestion)}
          aria-label={`${displayedSuggestion.name}${displayedSuggestion.isInstalled ? "" : " — Not installed"}`}
        >
          <div className={styles.art} ref={artworkRef}>
            {url && !failed ? (
              <img src={url} alt="" draggable={false} loading="lazy" decoding="async" />
            ) : (
              <div className={styles.placeholder}>
                <span>{displayedSuggestion.name}</span>
              </div>
            )}
            <SourceBadge
              label={sourceLabel(displayedSuggestion)}
              source={displayedSuggestion.source}
              platformKey={displayedSuggestion.platformKey}
              className={styles.sourceBadge}
            />
            {!displayedSuggestion.isInstalled && <div className={styles.notInstalledBadge}>Not installed</div>}
            {releaseYear(displayedSuggestion) && <div className={styles.releaseYear}>{releaseYear(displayedSuggestion)}</div>}
            {displayedSuggestion.igdbScore != null && (
              <div className={styles.scoreBadge} aria-label={`IGDB score ${Math.round(displayedSuggestion.igdbScore)}`}>
                ★ {Math.round(displayedSuggestion.igdbScore)}
              </div>
            )}
          </div>
          <div className={styles.title}>{displayedSuggestion.name}</div>
        </button>
        <div className={styles.reactionOverlay} aria-label="Game reaction">
          <button
            type="button"
            className={`${styles.reactionButton} ${displayedSuggestion.reaction === "like" ? styles.likeActive : ""}`}
            data-nav-item
            onClick={() => onSetReaction(displayedSuggestion.id, displayedSuggestion.reaction === "like" ? null : "like")}
            aria-pressed={displayedSuggestion.reaction === "like"}
            aria-label={displayedSuggestion.reaction === "like" ? "Remove like" : "Like this game"}
            title={displayedSuggestion.reaction === "like" ? "Remove like" : "Like this game"}
          >
            👍
          </button>
          <button
            type="button"
            className={`${styles.reactionButton} ${displayedSuggestion.reaction === "dislike" ? styles.dislikeActive : ""}`}
            data-nav-item
            onClick={() => onSetReaction(displayedSuggestion.id, displayedSuggestion.reaction === "dislike" ? null : "dislike")}
            aria-pressed={displayedSuggestion.reaction === "dislike"}
            aria-label={displayedSuggestion.reaction === "dislike" ? "Remove dislike" : "Dislike this game"}
            title={displayedSuggestion.reaction === "dislike" ? "Remove dislike" : "Dislike this game"}
          >
            👎
          </button>
        </div>
        <div className={styles.reasonInfo}>
          <button type="button" className={styles.infoButton} data-nav-item
            aria-label="Why this game is suggested" aria-describedby={reasonId}>
            <span aria-hidden="true">ⓘ</span>
          </button>
          <div id={reasonId} role="tooltip" className={styles.reasonTooltip}>{reason}</div>
        </div>
      </div>
  );
}
