import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getMetadataProgress } from "../api/tauri";
import type { MetadataProgress } from "../types";

/** Tracks the background IGDB metadata fetch. Reads the current state once on
 * mount because the startup worker can begin before the UI subscribes, then
 * follows live progress events. */
export function useMetadataProgress(): MetadataProgress | null {
  const [progress, setProgress] = useState<MetadataProgress | null>(null);

  useEffect(() => {
    getMetadataProgress()
      .then(setProgress)
      .catch(() => {});

    const unlisten = listen<MetadataProgress>("metadata-progress", (event) => {
      setProgress(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return progress;
}
