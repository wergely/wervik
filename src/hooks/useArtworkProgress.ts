import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getArtworkProgress } from "../api/tauri";
import type { ArtworkProgress } from "../types";

/** Tracks the background box-art fetch (see `retro::artwork` on the Rust
 * side) so the UI can show a progress indicator instead of leaving a huge
 * ROM library's missing covers looking like nothing is happening. Reads
 * the current state once on mount (the fetch on app startup is kicked off
 * from Rust before this can subscribe, so an early event could otherwise
 * be missed) and then follows live updates. */
export function useArtworkProgress(): ArtworkProgress | null {
  const [progress, setProgress] = useState<ArtworkProgress | null>(null);

  useEffect(() => {
    getArtworkProgress()
      .then(setProgress)
      .catch(() => {});

    const unlisten = listen<ArtworkProgress>("retro-artwork-progress", (event) => {
      setProgress(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return progress;
}
