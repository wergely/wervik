import { useCallback, useEffect, useState } from "react";
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
    const refresh = () => {
      getLibrary()
        .then(setSnapshot)
        .catch(() => {});
    };
    const unlistenRetro = listen("retro-artwork-updated", refresh);
    const unlistenBattleNet = listen("battlenet-artwork-updated", refresh);
    const unlistenPlaytime = listen("playtime-updated", refresh);
    const unlistenMetadata = listen("metadata-updated", refresh);
    const unlistenAchievements = listen("retro-achievements-updated", refresh);
    const unlistenSteamAchievements = listen("steam-achievements-updated", refresh);
    const unlistenSteamAchievementSettings = listen("steam-achievements-settings-updated", refresh);
    const unlistenSteamOwnedGames = listen("steam-owned-games-updated", refresh);
    // Fires when a ROM directory scan starts and again when it ends (see
    // `commands::spawn_rom_scan`) — refreshing on both is harmless (the
    // "starts" one is a no-op re-fetch of the same data) and means the
    // library picks up the rescan the moment it's actually done.
    const unlistenRomScan = listen("rom-scan-status", refresh);
    return () => {
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
