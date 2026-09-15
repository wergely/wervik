import styles from "./SourceBadge.module.css";
import type { GameSource } from "../types";

interface SourceBadgeProps {
  label: string;
  source: GameSource;
  className?: string;
}

export function SourceBadge({ label, source, className }: SourceBadgeProps) {
  return <span className={`${styles.badge} ${styles[source]} ${className ?? ""}`}>{label}</span>;
}
