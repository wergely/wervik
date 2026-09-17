import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  getLibrary,
  launchGame,
  rescanLibrary,
  setFavorite,
  setGameReaction,
  setRetroHardcoreMode,
} from "../api/tauri";
import type { Game, GameReaction, LibrarySnapshot } from "../types";

export type LibraryStatus = "loading" | "ready" | "error";

export function useLibrary() {
  const [status, setStatus] = useState<LibraryStatus>("loading");
  const [snapshot, setSnapshot] = useState<LibrarySnapshot | null>(null);
  const [rescanning, setRescanning] = useState(false);
  const refreshTimerRef = useRef<number | null>(null);
  const refreshInFlightRef = useRef(false);
  const refreshPendingRef = useRef(false);

  useEffect(() => {
    getLibrary()
      .then((s) => {
        setSnapshot(s);
        setStatus("ready");
      })
      .catch(() => setStatus("error"));
  }, []);

  // Artwork is fetched in the background (see `retro::artwork` and
  // `battlenet::artwork` on the Rust side); each successful fetch emits an
  // event so the library picks it up without the user having to rescan.
  useEffect(() => {
    let disposed = false;

    const scheduleRefresh = () => {
      if (disposed) return;
      if (refreshInFlightRef.current) {
        refreshPendingRef.current = true;
        return;
      }
      if (refreshTimerRef.current !== null) return;
      refreshTimerRef.current = window.setTimeout(async () => {
        refreshTimerRef.current = null;
        if (disposed) return;
        refreshInFlightRef.current = true;
        try {
          const next = await getLibrary();
          if (!disposed) setSnapshot(next);
        } catch {
          // A background refresh is best effort; the current snapshot remains usable.
        } finally {
          refreshInFlightRef.current = false;
          if (refreshPendingRef.current) {
            refreshPendingRef.current = false;
            scheduleRefresh();
          }
        }
      }, 150);
    };

    const unlistenRetro = listen("retro-artwork-updated", scheduleRefresh);
    const unlistenBattleNet = listen("battlenet-artwork-updated", scheduleRefresh);
    const unlistenPlaytime = listen("playtime-updated", scheduleRefresh);
    const unlistenMetadata = listen("metadata-updated", scheduleRefresh);
    const unlistenAchievements = listen("retro-achievements-updated", scheduleRefresh);
    const unlistenSteamAchievements = listen("steam-achievements-updated", scheduleRefresh);
    const unlistenSteamAchievementSettings = listen("steam-achievements-settings-updated", scheduleRefresh);
    const unlistenSteamOwnedGames = listen("steam-owned-games-updated", scheduleRefresh);
    // Fires when a ROM directory scan starts and again when it ends (see
    // `commands::spawn_rom_scan`) — only the completed scan needs a new
    // snapshot.
    const unlistenRomScan = listen<boolean>("rom-scan-status", (event) => {
      if (!event.payload) scheduleRefresh();
    });
    return () => {
      disposed = true;
      if (refreshTimerRef.current !== null) {
        window.clearTimeout(refreshTimerRef.current);
        refreshTimerRef.current = null;
      }
      refreshPendingRef.current = false;
      unlistenRetro.then((fn) => fn());
      unlistenBattleNet.then((fn) => fn());
      unlistenPlaytime.then((fn) => fn());
      unlistenMetadata.then((fn) => fn());
      unlistenAchievements.then((fn) => fn());
      unlistenSteamAchievements.then((fn) => fn());
      unlistenSteamAchievementSettings.then((fn) => fn());
      unlistenSteamOwnedGames.then((fn) => fn());
      unlistenRomScan.then((fn) => fn());
    };
  }, []);

  const rescan = useCallback(async () => {
    setRescanning(true);
    try {
      const s = await rescanLibrary();
      setSnapshot(s);
      setStatus("ready");
    } catch {
      setStatus("error");
    } finally {
      setRescanning(false);
    }
  }, []);

  const toggleFavorite = useCallback((id: string) => {
    setSnapshot((prev) => {
      if (!prev) return prev;
      const next = !prev.games.find((g) => g.id === id)?.isFavorite;
      setFavorite(id, next).catch(() => {});
      return {
        ...prev,
        games: prev.games.map((g): Game => (g.id === id ? { ...g, isFavorite: next } : g)),
      };
    });
  }, []);

  const updateReaction = useCallback((id: string, reaction: GameReaction | null) => {
    setSnapshot((prev) => {
      if (!prev) return prev;
      setGameReaction(id, reaction).catch(() => {});
      return {
        ...prev,
        games: prev.games.map((game): Game => (game.id === id ? { ...game, reaction } : game)),
      };
    });
  }, []);

  const updateHardcoreMode = useCallback((id: string, enabled: boolean) => {
    setSnapshot((prev) => {
      if (!prev) return prev;
      setRetroHardcoreMode(id, enabled).catch(() => {});
      return {
        ...prev,
        games: prev.games.map((game): Game => (game.id === id ? { ...game, hardcoreMode: enabled } : game)),
      };
    });
  }, []);

  const play = useCallback((appId: number) => {
    return launchGame(appId);
  }, []);

  return {
    status,
    snapshot,
    rescanning,
    rescan,
    toggleFavorite,
    updateReaction,
    updateHardcoreMode,
    play,
    applySnapshot: setSnapshot,
  };
}
