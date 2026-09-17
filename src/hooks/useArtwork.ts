import { useCallback, useEffect, useRef, useState } from "react";
import { readImage } from "../api/tauri";

const cache = new Map<string, Promise<string>>();

function load(path: string): Promise<string> {
  let pending = cache.get(path);
  if (!pending) {
    pending = /^https?:\/\//i.test(path)
      ? new Promise((resolve, reject) => {
          const image = new Image();
          image.onload = () => resolve(path);
          image.onerror = () => reject(new Error("Artwork request failed"));
          image.src = path;
        })
      : readImage(path);
    cache.set(path, pending);
    pending.catch(() => cache.delete(path));
  }
  return pending;
}

/** Loads local artwork through Tauri, while passing public remote artwork
 * URLs through for Steam titles that are not installed yet. */
export function useArtwork(
  path: string | null | undefined,
  options: { lazy?: boolean } = {},
): {
  url: string | null;
  failed: boolean;
  ref: (element: HTMLElement | null) => void;
} {
  const lazy = options.lazy ?? false;
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [nearViewport, setNearViewport] = useState(!lazy);
  const observerRef = useRef<IntersectionObserver | null>(null);

  const ref = useCallback(
    (element: HTMLElement | null) => {
      observerRef.current?.disconnect();
      observerRef.current = null;
      if (!element || !lazy || nearViewport) return;
      if (!("IntersectionObserver" in window)) {
        setNearViewport(true);
        return;
      }
      const observer = new IntersectionObserver(
        ([entry]) => {
          if (!entry?.isIntersecting) return;
          setNearViewport(true);
          observer.disconnect();
          observerRef.current = null;
        },
        { rootMargin: "600px" },
      );
      observer.observe(element);
      observerRef.current = observer;
    },
    [lazy, nearViewport],
  );

  useEffect(() => {
    setUrl(null);
    setFailed(false);
    if (!path || (lazy && !nearViewport)) {
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
  }, [lazy, nearViewport, path]);

  return { url, failed: failed || !path, ref };
}
