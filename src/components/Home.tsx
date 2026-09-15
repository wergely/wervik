import { useMemo } from "react";
import type { Game, GameReaction, LibrarySnapshot, Settings as SettingsType } from "../types";
import { ContinuePlayingRow } from "./ContinuePlayingRow";
import { EmptyState } from "./EmptyState";
import { Header } from "./Header";
import { LibrarySection } from "./LibrarySection";
import { SuggestedGameCard } from "./SuggestedGameCard";
import styles from "./Home.module.css";

const MAX_CONTINUE_PLAYING = 12;

interface HomeProps {
  snapshot: LibrarySnapshot;
  rescanning: boolean;
  onRescan: () => void;
  onSelectGame: (game: Game) => void;
  onSetReaction: (id: string, reaction: GameReaction | null) => void;
  onOpenSettings: () => void;
  onOpenPlatforms: () => void;
  settings: SettingsType | null;
  onUpdateSettings: (partial: Partial<SettingsType>) => void;
}

export function Home({
  snapshot,
  rescanning,
  onRescan,
  onSelectGame,
  onSetReaction,
  onOpenSettings,
  onOpenPlatforms,
  settings,
  onUpdateSettings,
}: HomeProps) {
  const continuePlaying = useMemo(
    () =>
      snapshot.games
        .filter((g) => g.lastPlayed !== null)
        .sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0))
        .slice(0, MAX_CONTINUE_PLAYING),
    [snapshot.games],
  );
  const continuePlayingIds = useMemo(() => new Set(continuePlaying.map((g) => g.id)), [continuePlaying]);

  if (!snapshot.steamFound && !snapshot.xboxFound && !snapshot.battlenetFound && snapshot.games.length === 0) {
    return (
      <div className={styles.page}>
        <Header onOpenSettings={onOpenSettings} onOpenPlatforms={onOpenPlatforms} />
        <EmptyState
          icon="⚠"
          title="No game sources found"
          message="Install a game through Steam, Xbox App/Game Pass or Battle.net, or add a retro ROM folder, then rescan."
          actionLabel={rescanning ? "Scanning…" : "Rescan"}
          onAction={onRescan}
          busy={rescanning}
        />
      </div>
    );
  }

  if (snapshot.games.length === 0) {
    return (
      <div className={styles.page}>
        <Header onOpenSettings={onOpenSettings} onOpenPlatforms={onOpenPlatforms} />
        <EmptyState
          icon="📦"
          title="No installed games found"
          message="Wervik only shows games that are installed. Install something from Steam, Xbox App/Game Pass or Battle.net, then rescan your library."
          actionLabel={rescanning ? "Scanning…" : "Rescan"}
          onAction={onRescan}
          busy={rescanning}
        />
      </div>
    );
  }

  return (
    <div className={styles.page}>
      <Header onOpenSettings={onOpenSettings} onOpenPlatforms={onOpenPlatforms} />
      <div className={styles.topRow}>
        {continuePlaying.length > 0 && (
          <>
            <ContinuePlayingRow games={continuePlaying} onSelect={onSelectGame} />
            <div className={styles.divider} />
          </>
        )}
        <SuggestedGameCard
          games={snapshot.games}
          excludeIds={continuePlayingIds}
          onSelect={onSelectGame}
          onSetReaction={onSetReaction}
        />
      </div>
      <LibrarySection
        games={snapshot.games}
        onSelect={onSelectGame}
        settings={settings}
        onUpdateSettings={onUpdateSettings}
      />
    </div>
  );
}
