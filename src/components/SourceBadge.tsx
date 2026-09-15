import styles from "./SourceBadge.module.css";
import type { GameSource } from "../types";
import { PlatformIcon } from "./PlatformIcon";

interface SourceBadgeProps {
  label: string;
  source: GameSource;
  /** Only meaningful for `source === "retro"` — which console icon to show. */
  platformKey?: string | null;
  className?: string;
}

export function SourceBadge({ label, source, platformKey, className }: SourceBadgeProps) {
  return (
    <span className={`${styles.badge} ${styles[source]} ${className ?? ""}`}>
      <PlatformIcon source={source} platformKey={platformKey} size={12} className={styles.icon} />
      {label}
    </span>
  );
}
