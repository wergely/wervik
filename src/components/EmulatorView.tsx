import styles from "./EmulatorView.module.css";

interface EmulatorViewProps {
  url: string;
  gameName: string;
  onExit: () => void;
}

/** A dedicated top-level view (not nested inside Game Details) that hosts
 * an EmulatorJS play session in an isolated iframe pointed at Wervik's own
 * local, session-scoped server (see `retro::server` on the Rust side).
 * `sandbox` keeps the emulator's own script execution boxed in even though
 * the content is same-machine, as defense in depth. */
export function EmulatorView({ url, gameName, onExit }: EmulatorViewProps) {
  return (
    <div className={styles.view}>
      <iframe
        title={gameName}
        src={url}
        className={styles.frame}
        allow="gamepad; fullscreen; autoplay"
        sandbox="allow-scripts allow-same-origin allow-pointer-lock allow-forms allow-modals"
      />
      <button type="button" className={styles.exitButton} data-nav-item onClick={onExit} aria-label="Exit game">
        ✕ Exit
      </button>
    </div>
  );
}
