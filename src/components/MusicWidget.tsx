import styles from "./MusicWidget.module.css";

interface MusicWidgetProps {
  enabled: boolean;
  trackName: string;
  volume: number;
  onToggle: () => void;
  onNext: () => void;
  onVolumeChange: (volume: number) => void;
}

/** Compact background-music transport in the top bar: play/pause, the
 * current generated track's name (with a little "now playing" equalizer),
 * skip to another track, and a volume slider. */
export function MusicWidget({ enabled, trackName, volume, onToggle, onNext, onVolumeChange }: MusicWidgetProps) {
  return (
    <div className={styles.widget}>
      <button
        type="button"
        className={styles.playButton}
        data-nav-item
        onClick={onToggle}
        aria-pressed={enabled}
        aria-label={enabled ? "Pause background music" : "Play background music"}
        title={enabled ? "Pause background music" : "Play background music"}
      >
        {enabled ? "⏸" : "▶"}
      </button>
      <div className={styles.info}>
        <div className={styles.status}>
          {enabled && (
            <span className={styles.equalizer} aria-hidden="true">
              <span />
              <span />
              <span />
            </span>
          )}
          <span className={styles.statusLabel}>{enabled ? "Now playing" : "Paused"}</span>
        </div>
        <div className={styles.trackName} title={trackName}>
          {trackName}
        </div>
      </div>
      <button
        type="button"
        className={styles.nextButton}
        data-nav-item
        onClick={onNext}
        disabled={!enabled}
        aria-label="Next track"
        title="Next track"
      >
        ⏭
      </button>
      <input
        type="range"
        className={styles.volume}
        data-nav-item
        min={0}
        max={1}
        step={0.05}
        value={volume}
        disabled={!enabled}
        onChange={(e) => onVolumeChange(Number(e.target.value))}
        aria-label="Music volume"
      />
    </div>
  );
}
