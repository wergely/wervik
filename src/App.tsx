import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  installSteamGame,
  fetchCoverNow,
  launchBattleNetGame,
  launchEaGame,
  launchRetroGame,
  launchXboxGame,
} from "./api/tauri";
import { AchievementsView } from "./components/AchievementsView";
import { GameAchievements } from "./components/GameAchievements";
import { GameDetail } from "./components/GameDetail";
import { Header, type HeaderView } from "./components/Header";
import { Home } from "./components/Home";
import { PlatformDetail } from "./components/PlatformDetail";
import { PlatformsView } from "./components/PlatformsView";
import { Settings } from "./components/Settings";
import { Welcome } from "./components/Welcome";
import { useChiptunePlayer } from "./hooks/useChiptunePlayer";
import { useLibrary } from "./hooks/useLibrary";
import { useSettings } from "./hooks/useSettings";
import { NAV_ITEM_SELECTOR } from "./navigation/spatialNav";
import { currentNavItem, useControllerNavigation } from "./navigation/useControllerNavigation";
import type { Game } from "./types";
import { playConfirmSound, playSelectSound } from "./utils/sfx";

type View = "home" | "detail" | "settings" | "platforms" | "platformDetail" | "achievements" | "gameAchievements";

interface FocusMemory {
  identity: string;
  index: number;
}

// Buttons without a data-nav-key are matched by their label, since async content can shift indices.
function navIdentity(el: HTMLElement): string {
  return el.dataset.navKey ?? `${el.tagName}:${el.getAttribute("aria-label") ?? el.textContent?.trim() ?? ""}`;
}

function captureFocus(): FocusMemory | null {
  const item = currentNavItem();
  if (!item) return null;
  const index = Array.from(document.querySelectorAll(NAV_ITEM_SELECTOR)).indexOf(item);
  return { identity: navIdentity(item), index };
}

function restoreFocus(memory: FocusMemory): boolean {
  const items = Array.from(document.querySelectorAll<HTMLElement>(NAV_ITEM_SELECTOR));
  // The same game can show up in several rows, so prefer the copy nearest its old position.
  const distance = (el: HTMLElement) => Math.abs(items.indexOf(el) - memory.index);
  const target = items
    .filter((el) => navIdentity(el) === memory.identity)
    .sort((a, b) => distance(a) - distance(b))[0];
  target?.focus();
  return target !== undefined && document.activeElement === target;
}

export default function App() {
  const {
    status,
    snapshot,
    rescanning,
    rescan,
    toggleFavorite,
    updateReaction,
    updateHardcoreMode,
    play,
    applySnapshot,
  } = useLibrary();
  const { settings, update: updateSettings } = useSettings();
  const [view, setView] = useState<View>("home");
  const focusMemory = useRef(new Map<View, FocusMemory>());
  const [selectedGameId, setSelectedGameId] = useState<string | null>(null);
  const [selectedPlatformKey, setSelectedPlatformKey] = useState<string | null>(null);
  // Where "Back" from the full achievements page should land — it's reachable
  // both from the Achievements overview and from a game's own detail page.
  const [achievementsOrigin, setAchievementsOrigin] = useState<"achievements" | "detail">("achievements");
  // Manually reopening the Welcome tour from Settings shouldn't flip the
  // persisted `onboardingCompleted` flag back to false just to show it once
  // more — this is a separate, session-only override.
  const [welcomeForced, setWelcomeForced] = useState(false);
  // Lets a nested overlay (e.g. the Game Setup drawer) intercept Escape/
  // gamepad B to close itself instead of the current view navigating back
  // to Home.
  const [backOverride, setBackOverride] = useState<(() => void) | null>(null);
  // Restricts spatial navigation to an open overlay's subtree, so arrow
  // keys/gamepad can't jump to elements behind it (e.g. Play/Favorite while
  // the Setup drawer is open).
  const [navScope, setNavScope] = useState<HTMLElement | null>(null);
  // The game id currently tracked as running — Steam/Xbox/Battle.net and
  // native retro emulators via a backend process monitor (see
  // `playtime::spawn_process_monitor`). `justFinishedGameId` is a one-shot
  // signal for "this game's
  // session just ended" (see the `playtime-updated` listener below) that
  // `GameDetail` uses to refresh its achievements panel automatically.
  const [playingGameId, setPlayingGameId] = useState<string | null>(null);
  const [justFinishedGameId, setJustFinishedGameId] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = listen<string>("playtime-updated", (event) => {
      setPlayingGameId((current) => (current === event.payload ? null : current));
      setJustFinishedGameId(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (!settings) return;
    getCurrentWindow()
      .setFullscreen(settings.fullscreen)
      .catch(() => {});
  }, [settings?.fullscreen]);

  // Auto-pauses while actually looking at a specific game (its detail
  // screen) — it keeps going in the
  // background across Home, Settings and Platforms, which all share the
  // same persistent top bar.
  const musicControls = useChiptunePlayer(
    Boolean(settings?.musicEnabled) && view !== "detail",
    settings?.musicVolume ?? 0.35,
  );

  // Going back to this view later puts focus back on whatever opened the next one.
  function openView(next: View) {
    const memory = captureFocus();
    if (memory) focusMemory.current.set(view, memory);
    setView(next);
  }

  function openSection(next: View) {
    focusMemory.current.clear();
    setView(next);
  }

  const selectedGame = snapshot?.games.find((g) => g.id === selectedGameId) ?? null;

  function handleSelectGame(game: Game) {
    playSelectSound();
    setSelectedGameId(game.id);
    openView("detail");
    // Retro covers come from a background fetch that can be working
    // through a library thousands of ROMs deep (see `useArtworkProgress`);
    // a game the user is actually looking at right now shouldn't have to
    // wait its turn. Steam games already get their art straight from the
    // local Steam install, so there's nothing to fetch for those.
    if (game.source === "retro" && !game.coverPath) {
      fetchCoverNow(game.id).catch(() => {});
    }
  }

  function handleOpenGameAchievements(game: Game, origin: "achievements" | "detail") {
    playSelectSound();
    setSelectedGameId(game.id);
    setAchievementsOrigin(origin);
    openView("gameAchievements");
  }

  function handleSelectPlatform(platform: { key: string }) {
    setSelectedPlatformKey(platform.key);
    openView("platformDetail");
  }

  async function handlePlay(game: Game, variantId?: string | null) {
    playConfirmSound();
    // Set only once the launch call itself has succeeded — an early throw
    // above (missing id/code) or a rejected launch must never leave
    // `playingGameId` stuck on a game that was never actually started.
    if (game.source === "steam") {
      if (game.appId == null) throw "This game has no Steam app id.";
      if (!game.isInstalled) {
        await installSteamGame(game.appId);
        return;
      }
      await play(game.appId);
      setPlayingGameId(game.id);
      return;
    }
    if (game.source === "xbox") {
      if (game.xboxAppId == null) throw "This game has no Xbox app id.";
      await launchXboxGame(game.id);
      setPlayingGameId(game.id);
      return;
    }
    if (game.source === "battlenet") {
      if (game.battlenetProductCode == null) throw "This game has no Battle.net product code.";
      await launchBattleNetGame(game.id);
      setPlayingGameId(game.id);
      return;
    }
    if (game.source === "ea") {
      if (game.eaGameId == null) throw "This game has no EA App game id.";
      await launchEaGame(game.id);
      setPlayingGameId(game.id);
      return;
    }
    // Runs as its own separate OS window (see `retro::native`) — Wervik
    // doesn't embed it, but a backend process monitor still tracks when it
    // exits (see `playtime::spawn_process_monitor`), same as Steam.
    await launchRetroGame(game.id, variantId);
    setPlayingGameId(game.id);
  }

  function handleBack() {
    if (view === "platformDetail") {
      setView("platforms");
      return;
    }
    if (view === "gameAchievements") {
      setView(achievementsOrigin);
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
  const showingWelcome = Boolean(settings && (!settings.onboardingCompleted || welcomeForced));
  useEffect(() => {
    const id = requestAnimationFrame(() => {
      const memory = focusMemory.current.get(view);
      focusMemory.current.delete(view);
      if (memory && restoreFocus(memory)) return;
      if (
        document.activeElement === document.body ||
        document.activeElement === null
      ) {
        document.querySelector<HTMLElement>(NAV_ITEM_SELECTOR)?.focus();
      }
    });
    return () => cancelAnimationFrame(id);
  }, [view, status, showingWelcome]);

  useControllerNavigation({
    // The Welcome overlay hides the current view, so B mustn't navigate it.
    onBack: showingWelcome ? undefined : backOverride ?? (view === "home" ? undefined : handleBack),
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
            data-nav-item
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

  if (showingWelcome && settings) {
    return (
      <div style={{ zoom: settings.uiScale, height: "100%" }}>
        <Welcome
          settings={settings}
          onUpdateSettings={updateSettings}
          onLibraryChange={applySnapshot}
          onDone={() => {
            setWelcomeForced(false);
            if (!settings.onboardingCompleted) updateSettings({ onboardingCompleted: true });
          }}
        />
      </div>
    );
  }

  const headerView: HeaderView =
    view === "settings"
      ? "settings"
      : view === "platforms" || view === "platformDetail"
        ? "platforms"
        : view === "achievements" || view === "gameAchievements"
          ? "achievements"
          : "home";

  return (
    <div style={{ zoom: settings?.uiScale ?? 1, height: "100%", display: "flex", flexDirection: "column" }}>
      <Header
        activeView={headerView}
        onGoHome={() => openSection("home")}
        onOpenSettings={() => openSection("settings")}
        onOpenPlatforms={() => openSection("platforms")}
        onOpenAchievements={() => openSection("achievements")}
        musicEnabled={Boolean(settings?.musicEnabled)}
        onToggleMusic={() => updateSettings({ musicEnabled: !settings?.musicEnabled })}
        onSkipTrack={musicControls.nextTrack}
        musicTrackName={musicControls.trackName}
        musicVolume={settings?.musicVolume ?? 0.35}
        onChangeMusicVolume={(volume) => updateSettings({ musicVolume: volume })}
      />
      <div style={{ flex: "1 1 auto", minHeight: 0 }}>
        {view === "settings" ? (
          <Settings
            snapshot={snapshot}
            settings={settings}
            rescanning={rescanning}
            onUpdateSettings={updateSettings}
            onRescan={rescan}
            onLibraryChange={applySnapshot}
            onReopenWelcome={() => setWelcomeForced(true)}
          />
        ) : view === "detail" && selectedGame ? (
          <GameDetail
            game={selectedGame}
            onBack={handleBack}
            onPlay={handlePlay}
            onToggleFavorite={toggleFavorite}
            onSetReaction={updateReaction}
            onSetHardcoreMode={updateHardcoreMode}
            onOpenAchievements={(game) => handleOpenGameAchievements(game, "detail")}
            registerBackHandler={setBackOverride}
            registerNavScope={setNavScope}
            isPlaying={playingGameId === selectedGame.id}
            justFinishedGameId={justFinishedGameId}
          />
        ) : view === "gameAchievements" && selectedGame ? (
          <GameAchievements
            game={selectedGame}
            onBack={handleBack}
            onSetHardcoreMode={updateHardcoreMode}
            registerBackHandler={setBackOverride}
            registerNavScope={setNavScope}
            isPlaying={playingGameId === selectedGame.id}
            justFinishedGameId={justFinishedGameId}
          />
        ) : view === "achievements" ? (
          <AchievementsView
            snapshot={snapshot}
            onSelectGame={(game) => handleOpenGameAchievements(game, "achievements")}
            registerBackHandler={setBackOverride}
            registerNavScope={setNavScope}
          />
        ) : view === "platforms" ? (
          <PlatformsView onSelectPlatform={handleSelectPlatform} />
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
            settings={settings}
            onUpdateSettings={updateSettings}
          />
        )}
      </div>
    </div>
  );
}
