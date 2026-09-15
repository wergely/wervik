import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { addRomDirectory, getRomDirectories, removeRomDirectory, rescanRetroLibrary } from "../api/tauri";
import type { LibrarySnapshot, Settings as SettingsType } from "../types";
import { Spinner } from "./Spinner";
import { Switch } from "./Switch";
import styles from "./Settings.module.css";

interface SettingsProps {
  snapshot: LibrarySnapshot | null;
  settings: SettingsType | null;
  rescanning: boolean;
  onUpdateSettings: (partial: Partial<SettingsType>) => void;
  onRescan: () => void;
  onBack: () => void;
  onLibraryChange: (snapshot: LibrarySnapshot) => void;
}

const SCALE_STEPS = [0.9, 1, 1.1, 1.25];

export function Settings({
  snapshot,
  settings,
  rescanning,
  onUpdateSettings,
  onRescan,
  onBack,
  onLibraryChange,
}: SettingsProps) {
  const scaleIndex = settings ? SCALE_STEPS.indexOf(settings.uiScale) : -1;
  const [romDirectories, setRomDirectories] = useState<string[]>([]);
  // Which ROM-folder action is in flight, if any — lets each button show
  // its own "in progress" state instead of one generic flag for all three.
  const [pendingAction, setPendingAction] = useState<"add" | "rescan" | { remove: string } | null>(null);
  const romScanning = pendingAction !== null;

  useEffect(() => {
    getRomDirectories().then(setRomDirectories);
  }, []);

  async function handleAddRomDirectory() {
    const selected = await open({ directory: true, multiple: false, title: "Select a folder containing ROMs" });
    if (!selected || Array.isArray(selected)) return;
    setPendingAction("add");
    try {
      const snapshot = await addRomDirectory(selected);
      onLibraryChange(snapshot);
      setRomDirectories(await getRomDirectories());
    } finally {
      setPendingAction(null);
    }
  }

  async function handleRemoveRomDirectory(path: string) {
    setPendingAction({ remove: path });
    try {
      const snapshot = await removeRomDirectory(path);
      onLibraryChange(snapshot);
      setRomDirectories(await getRomDirectories());
    } finally {
      setPendingAction(null);
    }
  }

  async function handleRescanRoms() {
    setPendingAction("rescan");
    try {
      onLibraryChange(await rescanRetroLibrary());
    } finally {
      setPendingAction(null);
    }
  }

  function stepScale(delta: number) {
    if (!settings) return;
    const currentIndex = scaleIndex === -1 ? 1 : scaleIndex;
    const nextIndex = Math.min(SCALE_STEPS.length - 1, Math.max(0, currentIndex + delta));
    onUpdateSettings({ uiScale: SCALE_STEPS[nextIndex] });
  }

  return (
    <div className={styles.page}>
      <div className={styles.headerRow}>
        <button type="button" className={styles.backButton} data-nav-item onClick={onBack}>
          ← Back
        </button>
        <h1 className={styles.title}>Settings</h1>
      </div>

      <div className={styles.group}>
        <p className={styles.groupTitle}>Steam</p>
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>Detected installation</div>
            <div className={styles.rowDescription}>The local Steam install Wervik reads from.</div>
          </div>
          <div className={styles.rowValue}>{snapshot?.steamRoot ?? "Not found"}</div>
        </div>
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>Library folders</div>
            <div className={styles.rowDescription}>Where installed games are discovered.</div>
          </div>
          <div className={styles.rowValue}>
            {snapshot && snapshot.libraryFolders.length > 0
              ? snapshot.libraryFolders.join(", ")
              : "None found"}
          </div>
        </div>
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>Rescan library</div>
            <div className={styles.rowDescription}>Look for newly installed or removed games.</div>
          </div>
          <button
            type="button"
            className={styles.rescanButton}
            data-nav-item
            onClick={onRescan}
            disabled={rescanning}
          >
            {rescanning && <Spinner />}
            {rescanning ? "Scanning…" : "Rescan now"}
          </button>
        </div>
      </div>

      <div className={styles.group}>
        <p className={styles.groupTitle}>Battle.net</p>
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>Installed games</div>
            <div className={styles.rowDescription}>Games registered in the local Battle.net Agent database.</div>
          </div>
          <div className={styles.rowValue}>
            {snapshot?.battlenetFound
              ? `${snapshot.games.filter((game) => game.source === "battlenet").length} found`
              : "None found"}
          </div>
        </div>
      </div>

      <div className={styles.group}>
        <p className={styles.groupTitle}>Xbox App &amp; PC Game Pass</p>
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>Installed games</div>
            <div className={styles.rowDescription}>Games registered for the current Windows user.</div>
          </div>
          <div className={styles.rowValue}>
            {snapshot?.xboxFound
              ? `${snapshot.games.filter((game) => game.source === "xbox").length} found`
              : "None found"}
          </div>
        </div>
      </div>

      <div className={styles.group}>
        <p className={styles.groupTitle}>Retro &amp; emulation</p>
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>ROM folders</div>
            <div className={styles.rowDescription}>
              Wervik scans these for supported ROMs and plays them with EmulatorJS.
            </div>
          </div>
          <button
            type="button"
            className={styles.rescanButton}
            data-nav-item
            onClick={handleAddRomDirectory}
            disabled={romScanning}
          >
            {pendingAction === "add" && <Spinner />}
            {pendingAction === "add" ? "Adding…" : "Add folder…"}
          </button>
        </div>
        {romDirectories.length > 0 && (
          <div className={styles.romList}>
            {romDirectories.map((dir) => {
              const removing =
                typeof pendingAction === "object" && pendingAction !== null && pendingAction.remove === dir;
              return (
                <div key={dir} className={styles.romListItem}>
                  <span className={styles.romPath}>{dir}</span>
                  <button
                    type="button"
                    className={styles.romRemoveButton}
                    data-nav-item
                    onClick={() => handleRemoveRomDirectory(dir)}
                    disabled={romScanning}
                    aria-label={`Remove ${dir}`}
                  >
                    {removing && <Spinner />}
                    {removing ? "Removing…" : "Remove"}
                  </button>
                </div>
              );
            })}
          </div>
        )}
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>Rescan ROM library</div>
            <div className={styles.rowDescription}>Look for newly added or removed ROMs.</div>
          </div>
          <button
            type="button"
            className={styles.rescanButton}
            data-nav-item
            onClick={handleRescanRoms}
            disabled={romScanning}
          >
            {pendingAction === "rescan" && <Spinner />}
            {pendingAction === "rescan" ? "Scanning…" : "Rescan now"}
          </button>
        </div>
      </div>

      <div className={styles.group}>
        <p className={styles.groupTitle}>Interface</p>
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>Launch in fullscreen</div>
            <div className={styles.rowDescription}>Start Wervik in fullscreen mode.</div>
          </div>
          <Switch
            checked={!!settings?.fullscreen}
            onChange={(v) => onUpdateSettings({ fullscreen: v })}
            label="Launch in fullscreen"
          />
        </div>
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>Launch on system startup</div>
            <div className={styles.rowDescription}>Open Wervik automatically when you log in.</div>
          </div>
          <Switch
            checked={!!settings?.launchOnStartup}
            onChange={(v) => onUpdateSettings({ launchOnStartup: v })}
            label="Launch on system startup"
          />
        </div>
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>Interface scale</div>
            <div className={styles.rowDescription}>Adjust text and card size.</div>
          </div>
          <div className={styles.scaleControl}>
            <button
              type="button"
              className={styles.scaleButton}
              data-nav-item
              onClick={() => stepScale(-1)}
              aria-label="Decrease interface scale"
            >
              −
            </button>
            <span className={styles.scaleValue}>
              {Math.round((settings?.uiScale ?? 1) * 100)}%
            </span>
            <button
              type="button"
              className={styles.scaleButton}
              data-nav-item
              onClick={() => stepScale(1)}
              aria-label="Increase interface scale"
            >
              +
            </button>
          </div>
        </div>
      </div>

      <p className={styles.about}>Wervik — by Wergely</p>
    </div>
  );
}
