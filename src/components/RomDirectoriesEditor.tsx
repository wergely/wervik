import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  addRomDirectory,
  getPlatformStatus,
  getRomDirectories,
  removeRomDirectory,
  rescanRetroLibrary,
  setRomDirectoryPlatform,
} from "../api/tauri";
import type { LibrarySnapshot, PlatformStatus, RomDirectoryConfig } from "../types";
import { Spinner } from "./Spinner";
import styles from "./Settings.module.css";

interface RomDirectoriesEditorProps {
  onLibraryChange: (snapshot: LibrarySnapshot) => void;
}

/** ROM folder configuration: add/remove folders, force a folder to one
 * platform, and rescan. Shared by Settings' "Retro & emulation" category and
 * the Welcome wizard's ROM directory step — both act on the real library, not
 * a preview copy. */
export function RomDirectoriesEditor({ onLibraryChange }: RomDirectoriesEditorProps) {
  const [romDirectories, setRomDirectories] = useState<RomDirectoryConfig[]>([]);
  // Every platform Wervik knows about, for the "force this platform" picker
  // next to each ROM folder — unfiltered (unlike `PlatformsView`, which only
  // shows ones with ROMs already found), since a directory can be tagged
  // before anything's been recognized inside it yet.
  const [allPlatforms, setAllPlatforms] = useState<PlatformStatus[]>([]);
  // Which ROM-folder action is in flight, if any — lets each button show its
  // own "in progress" state instead of one generic flag for all three.
  const [pendingAction, setPendingAction] = useState<"add" | "rescan" | { row: string } | null>(null);
  const romScanning = pendingAction !== null;

  useEffect(() => {
    getRomDirectories().then(setRomDirectories);
    getPlatformStatus().then(setAllPlatforms);
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

  async function handleSetRomDirectoryPlatform(path: string, platformKey: string) {
    setPendingAction({ row: path });
    try {
      const snapshot = await setRomDirectoryPlatform(path, platformKey || null);
      onLibraryChange(snapshot);
      setRomDirectories(await getRomDirectories());
    } finally {
      setPendingAction(null);
    }
  }

  async function handleRemoveRomDirectory(path: string) {
    setPendingAction({ row: path });
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

  return (
    <>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>ROM folders</div>
          <div className={styles.rowDescription}>
            Wervik scans these for supported ROMs and plays them with native emulators, including RetroArch/Libretro
            cores.
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
            const busy =
              typeof pendingAction === "object" && pendingAction !== null && pendingAction.row === dir.path;
            return (
              <div key={dir.path} className={styles.romListItem}>
                <span className={styles.romPath} title={dir.path}>
                  {dir.path}
                </span>
                <select
                  className={styles.romPlatformSelect}
                  data-nav-item
                  value={dir.platformKey ?? ""}
                  disabled={romScanning}
                  aria-label={`Platform override for ${dir.path}`}
                  title="Force every ROM in this folder to one platform, skipping auto-detection — useful for a platform whose games are folders, not single files (e.g. Wii U)"
                  onChange={(e) => handleSetRomDirectoryPlatform(dir.path, e.target.value)}
                >
                  <option value="">Auto-detect</option>
                  {allPlatforms.map((platform) => (
                    <option key={platform.key} value={platform.key}>
                      {platform.displayName}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className={styles.romRemoveButton}
                  data-nav-item
                  onClick={() => handleRemoveRomDirectory(dir.path)}
                  disabled={romScanning}
                  aria-label={`Remove ${dir.path}`}
                >
                  {busy && <Spinner />}
                  {busy ? "Working…" : "Remove"}
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
    </>
  );
}
