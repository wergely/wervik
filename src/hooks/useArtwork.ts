import { useEffect, useState } from "react";
import { readImage } from "../api/tauri";

const cache = new Map<string, Promise<string>>();

function load(path: string): Promise<string> {
  if (/^https?:\/\//i.test(path)) {
    return new Promise((resolve, reject) => {
      const image = new Image();
      image.onload = () => resolve(path);
      image.onerror = () => reject(new Error("Artwork request failed"));
      image.src = path;
    });
  }
  let pending = cache.get(path);
  if (!pending) {
    pending = readImage(path);
    cache.set(path, pending);
    pending.catch(() => cache.delete(path));
  }
  return pending;
}

/** Loads local artwork through Tauri, while passing public remote artwork
 * URLs through for Steam titles that are not installed yet. */
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
