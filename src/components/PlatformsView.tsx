import { useEffect, useState } from "react";
import { getPlatformStatus } from "../api/tauri";
import type { PlatformStatus } from "../types";
import { PlatformCard } from "./PlatformCard";
import styles from "./PlatformsView.module.css";

interface PlatformsViewProps {
  onSelectPlatform: (platform: PlatformStatus) => void;
  onBack: () => void;
}

export function PlatformsView({ onSelectPlatform, onBack }: PlatformsViewProps) {
  const [platforms, setPlatforms] = useState<PlatformStatus[] | null>(null);

  useEffect(() => {
    getPlatformStatus().then((list) => setPlatforms(list.filter((p) => p.romCount > 0)));
  }, []);

  return (
    <div className={styles.page}>
      <div className={styles.headerRow}>
        <button type="button" className={styles.backButton} data-nav-item onClick={onBack}>
          ← Back
        </button>
        <h1 className={styles.title}>Platforms</h1>
      </div>

      {platforms === null ? (
        <p className={styles.status}>Loading…</p>
      ) : platforms.length === 0 ? (
        <p className={styles.status}>
          No retro platforms yet — add a ROM folder in Settings to see them here.
        </p>
      ) : (
        <div className={`${styles.grid} wervik-scroll`}>
          {platforms.map((platform) => (
            <PlatformCard key={platform.key} platform={platform} onSelect={onSelectPlatform} />
          ))}
        </div>
      )}
    </div>
  );
}
