import { useCallback, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import styles from "./AchievementDetailsModal.module.css";

export interface AchievementDetails {
  title: string;
  description: string;
  iconUrl: string | null;
  unlocked: boolean;
  unlockedAt: number | null;
  points: number | null;
  unlockedHardcore: boolean;
  source: "RetroAchievements" | "Steam";
  gameName?: string;
  /** RA's "beaten game" category — `"progression"`, `"win_condition"`,
   * `"missable"`, or absent/null for a regular achievement or a Steam one. */
  achievementType?: string | null;
}

const ACHIEVEMENT_TYPE_LABELS: Record<string, string> = {
  progression: "Progression",
  win_condition: "Win condition",
  missable: "Missable",
};

interface AchievementDetailsModalProps {
  achievement: AchievementDetails;
  onClose: () => void;
  registerBackHandler?: (handler: (() => void) | null) => void;
  registerNavScope?: (el: HTMLElement | null) => void;
}

function formatUnlockDate(unixSeconds: number): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(unixSeconds * 1000));
}

export function AchievementDetailsModal({
  achievement,
  onClose,
  registerBackHandler,
  registerNavScope,
}: AchievementDetailsModalProps) {
  const modalRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const triggerRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  const close = useCallback(() => {
    onCloseRef.current();
    requestAnimationFrame(() => triggerRef.current?.focus());
  }, []);

  useEffect(() => {
    triggerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      close();
    };
    document.addEventListener("keydown", handleKeyDown);
    registerBackHandler?.(() => close);
    registerNavScope?.(modalRef.current);
    const focusId = requestAnimationFrame(() => closeButtonRef.current?.focus());

    return () => {
      cancelAnimationFrame(focusId);
      document.removeEventListener("keydown", handleKeyDown);
      registerBackHandler?.(null);
      registerNavScope?.(null);
    };
  }, [close, registerBackHandler, registerNavScope]);

  return createPortal(
    <div
      className={styles.backdrop}
      onClick={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <div
        ref={modalRef}
        className={styles.modal}
        role="dialog"
        aria-modal="true"
        aria-labelledby="achievement-details-title"
      >
        <div className={styles.header}>
          <div className={styles.headerTitles}>
            <span className={styles.eyebrow}>{achievement.source}</span>
            {achievement.gameName && <span className={styles.gameName}>{achievement.gameName}</span>}
          </div>
          <button
            ref={closeButtonRef}
            type="button"
            className={styles.closeButton}
            data-nav-item
            onClick={close}
            aria-label="Close achievement details"
          >
            ×
          </button>
        </div>

        <div className={styles.content}>
          <div className={styles.achievementHeader}>
            {achievement.iconUrl ? (
              <img className={styles.icon} src={achievement.iconUrl} alt="" />
            ) : (
              <div className={styles.iconPlaceholder} aria-hidden="true">
                🏆
              </div>
            )}
            <div className={styles.titleGroup}>
              <h2 id="achievement-details-title" className={styles.title}>
                {achievement.title || "Unnamed achievement"}
              </h2>
              <span className={`${styles.status} ${achievement.unlocked ? styles.statusUnlocked : styles.statusLocked}`}>
                {achievement.unlocked ? "Unlocked" : "Locked"}
              </span>
            </div>
          </div>

          <p className={styles.description}>{achievement.description || "No description available."}</p>

          <div className={styles.details}>
            {achievement.achievementType && ACHIEVEMENT_TYPE_LABELS[achievement.achievementType] && (
              <div className={styles.detailRow}>
                <span>Category</span>
                <strong>{ACHIEVEMENT_TYPE_LABELS[achievement.achievementType]}</strong>
              </div>
            )}
            {achievement.points != null && (
              <div className={styles.detailRow}>
                <span>Points</span>
                <strong>{achievement.points}</strong>
              </div>
            )}
            {achievement.unlockedAt != null && (
              <div className={styles.detailRow}>
                <span>Unlocked</span>
                <strong>{formatUnlockDate(achievement.unlockedAt)}</strong>
              </div>
            )}
            {achievement.unlockedHardcore && (
              <div className={styles.detailRow}>
                <span>Mode</span>
                <strong>Hardcore</strong>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
