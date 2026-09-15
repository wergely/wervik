import { useArtwork } from "../hooks/useArtwork";
import type { GameAchievementSummary } from "../types";
import { PlatformIcon } from "./PlatformIcon";
import styles from "./AchievementGameCard.module.css";

interface AchievementGameCardProps {
  entry: GameAchievementSummary;
  coverPath: string | null | undefined;
  onSelect: () => void;
}

/** A progress-first card for the Achievements screen — distinct from the
 * generic `GameCard` used everywhere else in the library, since this one's
 * whole point is surfacing completion (big percent, a progress bar, the
 * earned/total count) rather than the usual browsing badges (source,
 * favorite, IGDB score...). */
export function AchievementGameCard({ entry, coverPath, onSelect }: AchievementGameCardProps) {
  const { url, failed } = useArtwork(coverPath);

  return (
    <button type="button" className={styles.card} data-nav-item onClick={onSelect} aria-label={entry.gameName}>
      <div className={styles.art}>
        {url && !failed ? (
          <img src={url} alt="" draggable={false} />
        ) : (
          <div className={styles.placeholder}>
            <span>{entry.gameName}</span>
          </div>
        )}
        <div className={styles.scrim} />
        <div className={styles.overlay}>
          <div className={styles.name} title={entry.gameName}>
            {entry.gameName}
          </div>
          <div className={styles.percentRow}>
            <span className={styles.percent}>{entry.completionPercent}%</span>
          </div>
          <div className={styles.progressTrack}>
            <div className={styles.progressFill} style={{ width: `${entry.completionPercent}%` }} />
          </div>
          <div className={styles.footerRow}>
            <span className={styles.fraction}>
              {entry.earnedCount}/{entry.totalCount}
            </span>
            <span className={styles.platformBadge}>
              <PlatformIcon
                source={entry.source === "steam" ? "steam" : "retro"}
                platformKey={entry.platformKey}
                size={13}
              />
              {entry.platform ?? "Steam"}
            </span>
          </div>
        </div>
      </div>
    </button>
  );
}
