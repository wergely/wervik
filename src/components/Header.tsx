import { useArtworkProgress } from "../hooks/useArtworkProgress";
import { useMetadataProgress } from "../hooks/useMetadataProgress";
import { useRomScanStatus } from "../hooks/useRomScanStatus";
import { Logo } from "./Logo";
import { MusicWidget } from "./MusicWidget";
import { Spinner } from "./Spinner";
import styles from "./Header.module.css";

export type HeaderView = "home" | "platforms" | "achievements" | "settings";

interface HeaderProps {
  activeView: HeaderView;
  onGoHome: () => void;
  onOpenSettings: () => void;
  onOpenPlatforms: () => void;
  onOpenAchievements: () => void;
  musicEnabled: boolean;
  onToggleMusic: () => void;
  onSkipTrack: () => void;
  musicTrackName: string;
  musicVolume: number;
  onChangeMusicVolume: (volume: number) => void;
}

const NAV_ITEMS: { view: HeaderView; icon: string; label: string }[] = [
  { view: "home", icon: "🏠", label: "Library" },
  { view: "achievements", icon: "🏆", label: "Achievements" },
  { view: "platforms", icon: "🎮", label: "Platforms" },
  { view: "settings", icon: "⚙", label: "Settings" },
];

/** The app's persistent top bar — present on every screen except the
 * fullscreen emulator, so navigation (and the background music transport)
 * never disappears behind a page-specific "← Back" button. */
export function Header({
  activeView,
  onGoHome,
  onOpenSettings,
  onOpenPlatforms,
  onOpenAchievements,
  musicEnabled,
  onToggleMusic,
  onSkipTrack,
  musicTrackName,
  musicVolume,
  onChangeMusicVolume,
}: HeaderProps) {
  const artworkProgress = useArtworkProgress();
  const fetchingArt = !!artworkProgress && artworkProgress.total > 0 && artworkProgress.done < artworkProgress.total;
  const metadataProgress = useMetadataProgress();
  const fetchingMetadata =
    !!metadataProgress && metadataProgress.total > 0 && metadataProgress.done < metadataProgress.total;
  const scanningRoms = useRomScanStatus();

  const handlers: Record<HeaderView, () => void> = {
    home: onGoHome,
    achievements: onOpenAchievements,
    platforms: onOpenPlatforms,
    settings: onOpenSettings,
  };

  return (
    <header className={styles.header}>
      <div className={styles.left}>
        <Logo size={30} withWordmark />
      </div>

      <nav className={styles.nav} aria-label="Main navigation">
        {NAV_ITEMS.map((item) => (
          <button
            key={item.view}
            type="button"
            className={`${styles.navItem} ${activeView === item.view ? styles.navItemActive : ""}`}
            data-nav-item
            onClick={handlers[item.view]}
            aria-current={activeView === item.view}
          >
            <span aria-hidden="true">{item.icon}</span>
            {item.label}
          </button>
        ))}
      </nav>

      <div className={styles.right}>
        {scanningRoms && (
          <div className={styles.artworkProgress} title="Scanning ROM folders for new or removed games">
            <Spinner />
            <span className={styles.artworkProgressLabel}>Loading ROMs</span>
          </div>
        )}
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
        <MusicWidget
          enabled={musicEnabled}
          trackName={musicTrackName}
          volume={musicVolume}
          onToggle={onToggleMusic}
          onNext={onSkipTrack}
          onVolumeChange={onChangeMusicVolume}
        />
      </div>
    </header>
  );
}
