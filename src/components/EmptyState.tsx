import { Spinner } from "./Spinner";
import styles from "./EmptyState.module.css";

interface EmptyStateProps {
  icon?: string;
  title: string;
  message: string;
  actionLabel?: string;
  onAction?: () => void;
  busy?: boolean;
}

export function EmptyState({ icon = "🎮", title, message, actionLabel, onAction, busy }: EmptyStateProps) {
  return (
    <div className={styles.wrapper}>
      <div className={styles.icon}>{icon}</div>
      <h1 className={styles.title}>{title}</h1>
      <p className={styles.message}>{message}</p>
      {actionLabel && onAction && (
        <button
          type="button"
          className={styles.action}
          data-nav-item
          onClick={onAction}
          disabled={busy}
        >
          {busy && <Spinner className={styles.actionSpinner} />}
          {actionLabel}
        </button>
      )}
    </div>
  );
}
