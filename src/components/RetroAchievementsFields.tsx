import { useEffect, useState } from "react";
import type { Settings as SettingsType } from "../types";
import styles from "./Settings.module.css";

interface RetroAchievementsFieldsProps {
  settings: SettingsType | null;
  onUpdateSettings: (partial: Partial<SettingsType>) => void;
}

/** The shared RetroAchievements credentials used by both the read-only
 * viewer and the managed emulators' live achievements engine. Shared by
 * Settings' RetroAchievements category and the Welcome wizard's setup step —
 * both act on the real settings, not a wizard-only draft. */
export function RetroAchievementsFields({ settings, onUpdateSettings }: RetroAchievementsFieldsProps) {
  const [username, setUsername] = useState(settings?.raUsername ?? "");
  const [apiKey, setApiKey] = useState(settings?.raApiKey ?? "");
  const [password, setPassword] = useState(settings?.raPassword ?? "");
  const [revealApiKey, setRevealApiKey] = useState(false);
  const [revealPassword, setRevealPassword] = useState(false);

  useEffect(() => {
    setUsername(settings?.raUsername ?? "");
    setApiKey(settings?.raApiKey ?? "");
    setPassword(settings?.raPassword ?? "");
  }, [settings?.raUsername, settings?.raApiKey, settings?.raPassword]);

  const dirty =
    username.trim() !== (settings?.raUsername ?? "") ||
    apiKey.trim() !== (settings?.raApiKey ?? "") ||
    password !== (settings?.raPassword ?? "");
  const configured = !!settings?.raUsername && !!settings?.raApiKey;
  const liveConfigured = !!settings?.raUsername && !!settings?.raPassword;

  function save() {
    onUpdateSettings({
      raUsername: username.trim(),
      raApiKey: apiKey.trim(),
      raPassword: password,
    });
  }

  return (
    <>
      <p className={styles.groupDescription}>
        Optional. Configure your RetroAchievements account once for the viewer and for real-time achievements in
        managed RetroArch/PCSX2 games. The game page controls Hardcore mode per game. Get a Web API key from your{" "}
        <a href="https://retroachievements.org/settings" target="_blank" rel="noreferrer">
          RetroAchievements control panel
        </a>{" "}
        ("Keys" section).
      </p>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Username</div>
        </div>
        <input
          className={styles.textInput}
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder="Username"
          spellCheck={false}
          data-nav-item
        />
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Web API Key</div>
          <div className={styles.rowDescription}>Used for the read-only achievements viewer.</div>
        </div>
        <div className={styles.secretField}>
          <input
            className={styles.textInput}
            type={revealApiKey ? "text" : "password"}
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="Web API Key"
            spellCheck={false}
            data-nav-item
          />
          <button
            type="button"
            className={styles.revealButton}
            data-nav-item
            onClick={() => setRevealApiKey((v) => !v)}
            aria-label={revealApiKey ? "Hide API key" : "Show API key"}
            title={revealApiKey ? "Hide API key" : "Show API key"}
          >
            {revealApiKey ? "🙈" : "👁"}
          </button>
        </div>
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Password</div>
          <div className={styles.rowDescription}>
            Saved with your settings and used to sign in managed emulators for real-time achievements.
          </div>
        </div>
        <div className={styles.secretField}>
          <input
            className={styles.textInput}
            type={revealPassword ? "text" : "password"}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="Password"
            spellCheck={false}
            data-nav-item
          />
          <button
            type="button"
            className={styles.revealButton}
            data-nav-item
            onClick={() => setRevealPassword((v) => !v)}
            aria-label={revealPassword ? "Hide password" : "Show password"}
            title={revealPassword ? "Hide password" : "Show password"}
          >
            {revealPassword ? "🙈" : "👁"}
          </button>
        </div>
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Status</div>
          <div className={styles.rowDescription}>
            {configured || liveConfigured
              ? `${configured ? "Viewer configured" : "Live credentials saved"}${configured && liveConfigured ? " · live credentials saved for managed emulators" : ""}.`
              : "Not configured — Wervik works fine without it."}
          </div>
        </div>
        <button type="button" className={styles.rescanButton} data-nav-item onClick={save} disabled={!dirty}>
          Save
        </button>
      </div>
    </>
  );
}
