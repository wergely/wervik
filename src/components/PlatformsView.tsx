import { useEffect, useState } from "react";
import { getPlatformStatus } from "../api/tauri";
import type { PlatformStatus } from "../types";
import { PlatformCard } from "./PlatformCard";
import styles from "./PlatformsView.module.css";

interface PlatformsViewProps {
  onSelectPlatform: (platform: PlatformStatus) => void;
}

export function PlatformsView({ onSelectPlatform }: PlatformsViewProps) {
  const [platforms, setPlatforms] = useState<PlatformStatus[] | null>(null);

  useEffect(() => {
    getPlatformStatus().then((list) =>
      setPlatforms(
        [...list].sort((a, b) => b.romCount - a.romCount || a.displayName.localeCompare(b.displayName)),
      ),
    );
  }, []);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Platforms</h1>

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
