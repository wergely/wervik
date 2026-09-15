import { useEffect, useState } from "react";
import { useArtwork } from "../hooks/useArtwork";
import type { Game, GameReaction } from "../types";
import { sourceLabel } from "../utils/format";
import { SourceBadge } from "./SourceBadge";
import styles from "./SuggestedGameCard.module.css";

interface SuggestedGameCardProps {
  games: Game[];
  /** Games to avoid suggesting when possible (e.g. what's already showing
   * in "Continue Playing") — falls back to the full list if excluding them
   * would leave nothing to pick from. */
  excludeIds?: Set<string>;
  onSelect: (game: Game) => void;
  onSetReaction: (id: string, reaction: GameReaction | null) => void;
}

function releaseYear(game: Game): string | null {
  const timestamp = game.igdbMetadata?.firstReleaseDate;
  return timestamp ? String(new Date(timestamp * 1000).getUTCFullYear()) : null;
}

function pickRandom(games: Game[], excludeIds: Set<string> | undefined, avoidId: string | null): Game | null {
  const pool = excludeIds ? games.filter((g) => !excludeIds.has(g.id)) : games;
  const source = pool.length > 0 ? pool : games;
  if (source.length === 0) return null;
  if (source.length > 1 && avoidId) {
    const withoutLast = source.filter((g) => g.id !== avoidId);
    if (withoutLast.length > 0) {
      return withoutLast[Math.floor(Math.random() * withoutLast.length)];
    }
  }
  return source[Math.floor(Math.random() * source.length)];
}

export function SuggestedGameCard({ games, excludeIds, onSelect, onSetReaction }: SuggestedGameCardProps) {
  const [suggestion, setSuggestion] = useState<Game | null>(() => pickRandom(games, excludeIds, null));
  // Metadata and artwork arrive through a refreshed library snapshot. Keep
  // the selected id stable while replacing the object, otherwise a suggested
  // card picked before the IGDB worker finishes would keep its old score.
  const currentSuggestion = suggestion
    ? games.find((game) => game.id === suggestion.id) ?? null
    : null;
  const displayedSuggestion = currentSuggestion ?? suggestion;
  const { url, failed } = useArtwork(displayedSuggestion?.coverPath ?? null);

  // Re-roll if the current pick falls out of the library (e.g. a ROM
  // directory was removed) rather than pointing at a game that's gone.
  useEffect(() => {
    if (suggestion && !games.some((g) => g.id === suggestion.id)) {
      setSuggestion(pickRandom(games, excludeIds, suggestion.id));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [games]);

  if (!displayedSuggestion) return null;

  return (
    <section className={styles.panel}>
      <div className={styles.header}>
        <span className={styles.heading}>Suggested</span>
        <button
          type="button"
          className={styles.shuffleButton}
          data-nav-item
          onClick={() => setSuggestion((prev) => pickRandom(games, excludeIds, prev?.id ?? null))}
          aria-label="Suggest another game"
          title="Suggest another game"
        >
          🎲
        </button>
      </div>
      <div className={styles.cardWrap}>
        <button
          type="button"
          className={styles.card}
          data-nav-item
          onClick={() => onSelect(displayedSuggestion)}
          aria-label={displayedSuggestion.name}
        >
          <div className={styles.art}>
            {url && !failed ? (
              <img src={url} alt="" draggable={false} />
            ) : (
              <div className={styles.placeholder}>
                <span>{displayedSuggestion.name}</span>
              </div>
            )}
            <SourceBadge label={sourceLabel(displayedSuggestion)} className={styles.sourceBadge} />
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
      </div>
    </section>
  );
}
