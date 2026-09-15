import { useArtworkProgress } from "../hooks/useArtworkProgress";
import { useMetadataProgress } from "../hooks/useMetadataProgress";
import { Logo } from "./Logo";
import styles from "./Header.module.css";

interface HeaderProps {
  onOpenSettings: () => void;
  onOpenPlatforms: () => void;
}

export function Header({ onOpenSettings, onOpenPlatforms }: HeaderProps) {
  const artworkProgress = useArtworkProgress();
  const fetchingArt = !!artworkProgress && artworkProgress.total > 0 && artworkProgress.done < artworkProgress.total;
  const metadataProgress = useMetadataProgress();
  const fetchingMetadata =
    !!metadataProgress && metadataProgress.total > 0 && metadataProgress.done < metadataProgress.total;

  return (
    <header className={styles.header}>
      <div className={styles.left}>
        <Logo size={30} withWordmark />
        <span className={styles.tagline}>Your games. One library.</span>
      </div>
      <div className={styles.right}>
        {fetchingMetadata && metadataProgress && (
          <div
            className={styles.artworkProgress}
            title={`Updating game details: ${metadataProgress.done} of ${metadataProgress.total}`}
          >
            <span className={styles.artworkProgressLabel}>
              Updating details {metadataProgress.done}/{metadataProgress.total}
            </span>
            <span className={styles.artworkProgressTrack}>
              <span
                className={styles.artworkProgressFill}
                style={{ width: `${Math.min(100, (metadataProgress.done / metadataProgress.total) * 100)}%` }}
              />
            </span>
          </div>
        )}
        {fetchingArt && artworkProgress && (
          <div
            className={styles.artworkProgress}
            title={`Fetching box art: ${artworkProgress.done} of ${artworkProgress.total}`}
          >
            <span className={styles.artworkProgressLabel}>
              Fetching covers {artworkProgress.done}/{artworkProgress.total}
            </span>
            <span className={styles.artworkProgressTrack}>
              <span
                className={styles.artworkProgressFill}
                style={{ width: `${Math.min(100, (artworkProgress.done / artworkProgress.total) * 100)}%` }}
              />
            </span>
          </div>
        )}
        <button
          type="button"
          className={styles.iconButton}
          data-nav-item
          onClick={onOpenPlatforms}
          aria-label="Platforms"
          title="Platforms"
        >
          🎮
        </button>
        <button
          type="button"
          className={styles.iconButton}
          data-nav-item
          onClick={onOpenSettings}
          aria-label="Settings"
          title="Settings"
        >
          ⚙
        </button>
      </div>
    </header>
  );
}
