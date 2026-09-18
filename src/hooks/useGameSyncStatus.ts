import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getGameSyncStatus, syncGameSaves } from "../api/tauri";
import type { SaveSyncStatus } from "../types";

interface SaveSyncEvent {
  gameId: string;
  status: SaveSyncStatus;
}

/** Tracks a game's RetroArch save-sync status (see
 * `commands::get_game_sync_status` on the Rust side): read once on mount,
 * then followed live via the `save-sync-updated` event — emitted by the
 * automatic launch/exit hooks as much as by `sync()` below, so this also
 * reflects a sync that started from just playing the game. */
export function useGameSyncStatus(gameId: string) {
  const [status, setStatus] = useState<SaveSyncStatus>({ state: "idle" });

  useEffect(() => {
    let cancelled = false;
    getGameSyncStatus(gameId)
      .then((result) => {
        if (!cancelled) setStatus(result);
      })
      .catch(() => {});

    const unlisten = listen<SaveSyncEvent>("save-sync-updated", (event) => {
      if (event.payload.gameId === gameId) {
        setStatus(event.payload.status);
      }
    });
    return () => {
      cancelled = true;
      unlisten.then((fn) => fn());
    };
  }, [gameId]);

  const sync = useCallback(() => {
    syncGameSaves(gameId).then(setStatus).catch(() => {});
  }, [gameId]);

  return { status, sync };
}
