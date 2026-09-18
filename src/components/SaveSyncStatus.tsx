import { useGameSyncStatus } from "../hooks/useGameSyncStatus";
import { formatLastPlayed } from "../utils/format";
import { Spinner } from "./Spinner";
import styles from "./SaveSyncStatus.module.css";

interface SaveSyncStatusProps {
  gameId: string;
}

/** Cloud-save-sync indicator for the game page, shown only when a Wervik
 * Account is connected — see `App.tsx`'s use of `useWervikAccount`. Renders
 * nothing when sync doesn't apply to this game (no account, ROM not found,
 * or its platform doesn't launch through a RetroArch core). */
export function SaveSyncStatus({ gameId }: SaveSyncStatusProps) {
  const { status, sync } = useGameSyncStatus(gameId);

  if (status.state === "notApplicable") return null;

  const label =
    status.state === "synced"
      ? `Save synced ${formatLastPlayed(status.at) ?? "recently"}`
      : status.state === "error"
        ? "Save sync failed"
        : status.state === "syncing"
          ? "Syncing save…"
          : "Save not synced yet";
  const syncing = status.state === "syncing";

  return (
    <div
      className={`${styles.chip} ${styles[status.state] ?? ""}`}
      title={status.state === "error" ? status.message : undefined}
    >
      <span className={styles.dot} />
      <span>{label}</span>
      <button
        type="button"
        className={styles.syncButton}
        data-nav-item
        onClick={sync}
        disabled={syncing}
        aria-label="Sync save now"
        title="Sync save now"
      >
        {syncing ? <Spinner /> : "↻"}
      </button>
    </div>
  );
}
