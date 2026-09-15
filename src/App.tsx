import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  fetchCoverNow,
  getLibrary,
  launchBattleNetGame,
  launchRetroGame,
  launchXboxGame,
  stopRetroSession,
} from "./api/tauri";
import { EmulatorView } from "./components/EmulatorView";
import { GameDetail } from "./components/GameDetail";
import { Home } from "./components/Home";
import { PlatformDetail } from "./components/PlatformDetail";
import { PlatformsView } from "./components/PlatformsView";
import { Settings } from "./components/Settings";
import { useLibrary } from "./hooks/useLibrary";
import { useSettings } from "./hooks/useSettings";
import { useControllerNavigation } from "./navigation/useControllerNavigation";
import type { Game } from "./types";

type View = "home" | "detail" | "settings" | "emulator" | "platforms" | "platformDetail";

interface ActiveSession {
  game: Game;
  sessionId: string;
  url: string;
  startedAt: number;
}

export default function App() {
  const { status, snapshot, rescanning, rescan, toggleFavorite, updateReaction, play, applySnapshot } = useLibrary();
  const { settings, update: updateSettings } = useSettings();
  const [view, setView] = useState<View>("home");
  const [selectedGameId, setSelectedGameId] = useState<string | null>(null);
  const [selectedPlatformKey, setSelectedPlatformKey] = useState<string | null>(null);
  const [activeSession, setActiveSession] = useState<ActiveSession | null>(null);
  const activeSessionRef = useRef<ActiveSession | null>(null);
  activeSessionRef.current = activeSession;
  // Lets a nested overlay (e.g. the Game Setup drawer) intercept Escape/
  // gamepad B to close itself instead of the current view navigating back
  // to Home.
  const [backOverride, setBackOverride] = useState<(() => void) | null>(null);
  // Restricts spatial navigation to an open overlay's subtree, so arrow
  // keys/gamepad can't jump to elements behind it (e.g. Play/Favorite while
  // the Setup drawer is open).
  const [navScope, setNavScope] = useState<HTMLElement | null>(null);

  useEffect(() => {
    if (!settings) return;
    getCurrentWindow()
      .setFullscreen(settings.fullscreen)
      .catch(() => {});
  }, [settings?.fullscreen]);

  const selectedGame = snapshot?.games.find((g) => g.id === selectedGameId) ?? null;

  function handleSelectGame(game: Game) {
    setSelectedGameId(game.id);
    setView("detail");
    // Retro covers come from a background fetch that can be working
    // through a library thousands of ROMs deep (see `useArtworkProgress`);
    // a game the user is actually looking at right now shouldn't have to
    // wait its turn. Steam games already get their art straight from the
    // local Steam install, so there's nothing to fetch for those.
    if (game.source === "retro" && !game.coverPath) {
      fetchCoverNow(game.id).catch(() => {});
    }
  }

  function handleSelectPlatform(platform: { key: string }) {
    setSelectedPlatformKey(platform.key);
    setView("platformDetail");
  }

  async function handlePlay(game: Game, variantId?: string | null) {
    if (game.source === "steam") {
      if (game.appId == null) throw "This game has no Steam app id.";
      await play(game.appId);
      return;
    }
    if (game.source === "xbox") {
      if (game.xboxAppId == null) throw "This game has no Xbox app id.";
      await launchXboxGame(game.id);
      return;
    }
    if (game.source === "battlenet") {
      if (game.battlenetProductCode == null) throw "This game has no Battle.net product code.";
      await launchBattleNetGame(game.id);
      return;
    }
    const launch = await launchRetroGame(game.id, variantId);
    if (launch.mode === "native") {
      // Runs as its own separate OS window (see `retro::native`) — Wervik
      // has nothing to embed or track here, unlike an EmulatorJS session.
      return;
    }
    setActiveSession({ game, sessionId: launch.sessionId, url: launch.url, startedAt: Date.now() });
    setView("emulator");
  }

  async function handleExitEmulator() {
    const session = activeSessionRef.current;
    if (!session) {
      setView("home");
      return;
    }
    const minutesPlayed = Math.round((Date.now() - session.startedAt) / 60000);
    setActiveSession(null);
    setView("detail");
    try {
      await stopRetroSession(session.sessionId, minutesPlayed);
    } finally {
      // Playtime/last-played was just recorded on the Rust side — refresh
      // so Continue Playing, "Recently played" sort, and this game's own
      // detail screen reflect it immediately instead of only after the
      // next unrelated rescan.
      getLibrary().then(applySnapshot).catch(() => {});
    }
  }

  function handleBack() {
    if (view === "emulator") {
      handleExitEmulator();
      return;
    }
    if (view === "platformDetail") {
      setView("platforms");
      return;
    }
    setView("home");
  }

  // Drop any stale overlay-back handler/nav scope whenever the visible view
  // changes.
  useEffect(() => {
    setBackOverride(null);
    setNavScope(null);
  }, [view]);

  // Seed keyboard/controller focus whenever the visible view changes, so
  // the first arrow press or D-pad nudge has somewhere to move from.
  useEffect(() => {
    // The emulator view wants the iframe to keep initial keyboard focus for
    // the game itself, not the Exit button.
    if (view === "emulator") return;
    const id = requestAnimationFrame(() => {
      if (
        document.activeElement === document.body ||
        document.activeElement === null
      ) {
        document.querySelector<HTMLElement>("[data-nav-item]")?.focus();
      }
    });
    return () => cancelAnimationFrame(id);
  }, [view, status]);

  useControllerNavigation({
    onBack: backOverride ?? (view === "home" ? undefined : handleBack),
    scope: navScope,
  });

  if (status === "loading" || !snapshot) {
    return (
      <div style={{ zoom: settings?.uiScale ?? 1, height: "100%" }}>
        <div
          style={{
            height: "100%",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--text-secondary)",
          }}
        >
          Loading your library…
        </div>
      </div>
    );
  }

  if (status === "error") {
    return (
      <div style={{ zoom: settings?.uiScale ?? 1, height: "100%" }}>
        <div
          style={{
            height: "100%",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 16,
            color: "var(--text-secondary)",
            textAlign: "center",
            padding: 40,
          }}
        >
          <h1 style={{ color: "var(--text)", margin: 0 }}>Something went wrong</h1>
          <p>Wervik couldn't read your Steam library. Try rescanning.</p>
          <button
            type="button"
            onClick={rescan}
            style={{
              background: "var(--gradient)",
              color: "#090b10",
              fontWeight: 700,
              border: "none",
              borderRadius: 10,
              padding: "12px 26px",
            }}
          >
            Rescan
          </button>
        </div>
      </div>
    );
  }

  if (view === "emulator" && activeSession) {
    // Rendered outside the UI-scale wrapper: the emulator should always
    // fill the real screen 1:1, regardless of Wervik's own interface scale.
    return (
      <EmulatorView url={activeSession.url} gameName={activeSession.game.name} onExit={handleExitEmulator} />
    );
  }

  return (
    <div style={{ zoom: settings?.uiScale ?? 1, height: "100%" }}>
      {view === "settings" ? (
        <Settings
          snapshot={snapshot}
          settings={settings}
          rescanning={rescanning}
          onUpdateSettings={updateSettings}
          onRescan={rescan}
          onBack={handleBack}
          onLibraryChange={applySnapshot}
        />
      ) : view === "detail" && selectedGame ? (
        <GameDetail
          game={selectedGame}
          onBack={handleBack}
          onPlay={handlePlay}
          onToggleFavorite={toggleFavorite}
          onSetReaction={updateReaction}
          registerBackHandler={setBackOverride}
          registerNavScope={setNavScope}
        />
      ) : view === "platforms" ? (
        <PlatformsView onSelectPlatform={handleSelectPlatform} onBack={handleBack} />
      ) : view === "platformDetail" && selectedPlatformKey ? (
        <PlatformDetail
          platformKey={selectedPlatformKey}
          onBack={handleBack}
        />
      ) : (
        <Home
          snapshot={snapshot}
          rescanning={rescanning}
          onRescan={rescan}
          onSelectGame={handleSelectGame}
          onSetReaction={updateReaction}
          onOpenSettings={() => setView("settings")}
          onOpenPlatforms={() => setView("platforms")}
          settings={settings}
          onUpdateSettings={updateSettings}
        />
      )}
    </div>
  );
}
