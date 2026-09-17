import type { PlatformStatus } from "../types";
import { PlatformIcon } from "./PlatformIcon";
import styles from "./PlatformCard.module.css";

interface PlatformCardProps {
  platform: PlatformStatus;
  onSelect: (platform: PlatformStatus) => void;
}

export function PlatformCard({ platform, onSelect }: PlatformCardProps) {
  const effective = platform.emulatorOptions.find((e) => e.id === platform.effectiveEmulator);

  return (
    <button
      type="button"
      className={styles.card}
      data-nav-item
      onClick={() => onSelect(platform)}
      aria-label={platform.displayName}
    >
      {/* A stable visual identity — not a random cover pulled from one of the
          player's own games, which would make the same platform look
          different every rescan. */}
      <div className={styles.art}>
        <PlatformIcon source="retro" platformKey={platform.key} size={56} art artVariant="thumb" />
      </div>
      <div className={styles.info}>
        <span className={styles.name}>{platform.displayName}</span>
        <span className={styles.count}>
          {platform.romCount} {platform.romCount === 1 ? "game" : "games"}
        </span>
        <span className={`${styles.emulator} ${!effective ? styles.emulatorWarning : ""}`}>
          {effective ? effective.displayName : "No emulator available"}
        </span>
      </div>
    </button>
  );
}
