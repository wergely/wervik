import { useEffect, useState } from "react";
import { readImage } from "../api/tauri";

const cache = new Map<string, Promise<string>>();

function load(path: string): Promise<string> {
  let pending = cache.get(path);
  if (!pending) {
    pending = readImage(path);
    cache.set(path, pending);
    pending.catch(() => cache.delete(path));
  }
  return pending;
}

/** Loads a locally-cached Steam artwork file as a displayable data URL. */
export function useArtwork(path: string | null | undefined): {
  url: string | null;
  failed: boolean;
} {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setUrl(null);
    setFailed(false);
    if (!path) {
      return;
    }
    let cancelled = false;
    load(path)
      .then((dataUrl) => {
        if (!cancelled) setUrl(dataUrl);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [path]);

  return { url, failed: failed || !path };
}
