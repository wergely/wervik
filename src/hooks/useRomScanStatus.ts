import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getRomScanStatus } from "../api/tauri";

/** Tracks the background ROM directory scan (see `commands::spawn_rom_scan`
 * on the Rust side) so adding a folder or hitting Rescan shows up as a
 * top-bar job, the same as the metadata/artwork sweeps, instead of just a
 * button spinner. Reads the current state once on mount — the scan can
 * already be running before this subscribes — then follows live updates. */
export function useRomScanStatus(): boolean {
  const [scanning, setScanning] = useState(false);

  useEffect(() => {
    getRomScanStatus()
      .then(setScanning)
      .catch(() => {});

    const unlisten = listen<boolean>("rom-scan-status", (event) => {
      setScanning(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return scanning;
}
