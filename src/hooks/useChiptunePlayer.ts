import { useCallback, useEffect, useRef, useState } from "react";
import { createChiptunePlayer, PATTERN_NAMES, type ChiptunePlayer } from "../utils/chiptune";

export interface ChiptuneControls {
  /** Skips to the next track immediately. */
  nextTrack(): void;
  /** The currently playing track's fun name. */
  trackName: string;
}

/** Owns the background chiptune player's lifecycle: creates it once, starts
 * or stops it as `enabled` changes (the caller decides when that is — e.g.
 * only while browsing Home, off everywhere else), keeps its volume in sync,
 * and disposes it on unmount. Browsers only let audio start after a user
 * gesture, so a blocked first `start()` is retried on the next click/key. */
export function useChiptunePlayer(enabled: boolean, volume: number): ChiptuneControls {
  const [trackIndex, setTrackIndex] = useState(0);
  const playerRef = useRef<ChiptunePlayer | null>(null);
  if (!playerRef.current) {
    playerRef.current = createChiptunePlayer(volume, setTrackIndex);
  }

  useEffect(() => {
    const player = playerRef.current;
    return () => player?.dispose();
  }, []);

  useEffect(() => {
    playerRef.current?.setVolume(volume);
  }, [volume]);

  useEffect(() => {
    const player = playerRef.current;
    if (!player) return;
    if (!enabled) {
      player.stop();
      return;
    }
    player.start();
    const retry = () => player.start();
    window.addEventListener("pointerdown", retry, { once: true });
    window.addEventListener("keydown", retry, { once: true });
    return () => {
      window.removeEventListener("pointerdown", retry);
      window.removeEventListener("keydown", retry);
    };
  }, [enabled]);

  const nextTrack = useCallback(() => playerRef.current?.next(), []);

  return { nextTrack, trackName: PATTERN_NAMES[trackIndex] };
}
