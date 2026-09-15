import { useArtwork } from "../hooks/useArtwork";
import type { Game } from "../types";
import { sourceLabel } from "../utils/format";
import { SourceBadge } from "./SourceBadge";
import styles from "./GameCard.module.css";

interface GameCardProps {
  game: Game;
  size?: "md" | "lg";
  onSelect: (game: Game) => void;
}

function releaseYear(game: Game): string | null {
  const timestamp = game.igdbMetadata?.firstReleaseDate;
  return timestamp ? String(new Date(timestamp * 1000).getUTCFullYear()) : null;
}

export function GameCard({ game, size = "md", onSelect }: GameCardProps) {
  const { url, failed } = useArtwork(game.coverPath);

  return (
    <button
      type="button"
      className={`${styles.card} ${size === "lg" ? styles.lg : ""}`}
      data-nav-item
      onClick={() => onSelect(game)}
      aria-label={`${game.name}${game.isInstalled ? "" : " — Not installed"}`}
    >
      <div className={styles.art}>
        {url && !failed ? (
          <img src={url} alt="" draggable={false} />
        ) : (
          <div className={styles.placeholder}>
            <span>{game.name}</span>
          </div>
        )}
        <SourceBadge
          label={sourceLabel(game)}
          source={game.source}
          platformKey={game.platformKey}
          className={styles.sourceBadge}
        />
        {!game.isInstalled && <div className={styles.notInstalledBadge}>Not installed</div>}
        {releaseYear(game) && <div className={styles.releaseYear}>{releaseYear(game)}</div>}
        {game.igdbScore != null && (
          <div className={styles.scoreBadge} aria-label={`IGDB score ${Math.round(game.igdbScore)}`}>
            ★ {Math.round(game.igdbScore)}
          </div>
        )}
        {(game.achievementCompletionPercent != null || game.isFavorite) && (
          <div className={styles.topRightBadges}>
            {game.achievementCompletionPercent != null && (
              <div
                className={styles.completionBadge}
                aria-label={`Achievement completion: ${game.achievementCompletionPercent}%`}
              >
                🏆 {game.achievementCompletionPercent}%
              </div>
            )}
            {game.isFavorite && <div className={styles.favoriteBadge}>★</div>}
          </div>
        )}
      </div>
      <div className={styles.title}>{game.name}</div>
    </button>
  );
}
