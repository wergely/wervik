import { useCallback, useEffect, useRef, useState } from "react";
import { getRetroAchievements, getSteamAchievements } from "../api/tauri";
import type { Game } from "../types";
import { fromRa, fromSteam, type DisplayAchievements } from "../utils/achievements";

/** Loads and adapts one game's achievements (RetroAchievements or Steam,
 * depending on `game.source`) into the shared `DisplayAchievements` shape.
 * Shared by `GameDetail`'s preview panel and the full `GameAchievements`
 * page so both follow the exact same fetch/race-guard/refresh behavior.
 *
 * `justFinishedGameId`, when it matches `game.id`, forces one fresh
 * (uncached) read — most relevant right after a RetroArch session, whose
 * real achievements engine may have unlocked something meanwhile. */
export function useGameAchievements(game: Game, justFinishedGameId?: string | null) {
  const [achievements, setAchievements] = useState<DisplayAchievements | null>(null);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestRef = useRef(0);

  const load = useCallback(
    (force: boolean) => {
      if (game.source !== "retro" && game.source !== "steam") {
        setAchievements(null);
        return;
      }
      // A monotonic token, not a per-call `cancelled` closure, since this can
      // be triggered from several places (mount/game-change, "just finished
      // playing", a manual refresh) — a stale response from any of them is
      // discarded the same way.
      const requestId = ++requestRef.current;
      if (force) setRefreshing(true);
      else setLoading(true);
      setError(null);
      const request =
        game.source === "retro"
          ? getRetroAchievements(game.id, force).then((value) => (value ? fromRa(value) : null))
          : getSteamAchievements(game.id, force).then((value) => (value ? fromSteam(value) : null));
      request
        .then((value) => {
          if (requestRef.current === requestId) setAchievements(value);
        })
        .catch((err) => {
          if (requestRef.current === requestId) {
            setAchievements(null);
            setError(typeof err === "string" ? err : "Couldn't load achievements.");
          }
        })
        .finally(() => {
          if (requestRef.current === requestId) {
            setLoading(false);
            setRefreshing(false);
          }
        });
    },
    [game.id, game.source],
  );

  useEffect(() => {
    setAchievements(null);
    load(false);
  }, [load]);

  useEffect(() => {
    if (justFinishedGameId && justFinishedGameId === game.id) {
      load(true);
    }
  }, [justFinishedGameId, game.id, load]);

  return { achievements, loading, refreshing, error, refresh: () => load(true) };
}
