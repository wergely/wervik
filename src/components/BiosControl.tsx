import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { clearBiosPath, getBiosPath, setBiosPath } from "../api/tauri";
import { fileName } from "../utils/format";
import styles from "./BiosControl.module.css";

interface BiosControlProps {
  platformKey: string;
  platformName: string;
}

/** BIOS management for platforms that need one (e.g. PlayStation, Neo Geo
 * CD). Wervik never ships or fetches a BIOS itself — the user points at
 * their own dump. The control may appear on a game detail page, but the
 * choice is stored once for the whole platform. For EmulatorJS this is
 * copied into Wervik's own app data dir; for a native emulator that expects
 * its BIOS at a fixed path of its own (see
 * `retro::native::NativeEmulator::bios_relpath`), it's also placed there on
 * the next launch. */
export function BiosControl({ platformKey, platformName }: BiosControlProps) {
  const [biosPath, setBiosPathState] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setLoaded(false);
    getBiosPath(platformKey)
      .then((path) => {
        setBiosPathState(path);
        setLoaded(true);
      })
      .catch(() => setLoaded(true));
  }, [platformKey]);

  async function handlePick() {
    setError(null);
    let selected: string | null;
    try {
      selected = await open({ multiple: false, directory: false, title: `Select ${platformName} BIOS file` });
    } catch {
      return;
    }
    if (!selected) return;
    try {
      const installed = await setBiosPath(platformKey, selected);
      setBiosPathState(installed);
    } catch (err) {
      setError(typeof err === "string" ? err : "Could not add that BIOS file.");
    }
  }

  async function handleClear() {
    await clearBiosPath(platformKey).catch(() => {});
    setBiosPathState(null);
  }

  if (!loaded) return null;

  return (
    <div className={styles.bios}>
      <span className={styles.label}>
        {biosPath ? `BIOS: ${fileName(biosPath)}` : `${platformName} needs a BIOS file to run`}
      </span>
      <div className={styles.actions}>
        <button type="button" className={styles.button} data-nav-item onClick={handlePick}>
          {biosPath ? "Change BIOS" : "Add BIOS file"}
        </button>
        {biosPath && (
          <button type="button" className={styles.button} data-nav-item onClick={handleClear}>
            Remove
          </button>
        )}
      </div>
      {error && <p className={styles.error}>{error}</p>}
    </div>
  );
}
