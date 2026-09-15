import styles from "./SourceBadge.module.css";

interface SourceBadgeProps {
  label: string;
  className?: string;
}

export function SourceBadge({ label, className }: SourceBadgeProps) {
  return <span className={`${styles.badge} ${className ?? ""}`}>{label}</span>;
}
