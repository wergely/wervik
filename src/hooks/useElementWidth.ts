import { useCallback, useRef, useState } from "react";

/** Tracks an element's content-box width via ResizeObserver. Returns a ref
 * callback to attach to the element and its current width in pixels (0
 * before the element has mounted). */
export function useElementWidth<T extends HTMLElement>(): [(node: T | null) => void, number] {
  const [width, setWidth] = useState(0);
  const observerRef = useRef<ResizeObserver | null>(null);

  const ref = useCallback((node: T | null) => {
    observerRef.current?.disconnect();
    observerRef.current = null;
    if (!node) return;
    setWidth(node.getBoundingClientRect().width);
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    observer.observe(node);
    observerRef.current = observer;
  }, []);

  return [ref, width];
}
