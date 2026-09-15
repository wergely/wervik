import { useMemo } from "react";
import type { Game, GameReaction, LibrarySnapshot, Settings as SettingsType } from "../types";
import { ContinuePlayingRow } from "./ContinuePlayingRow";
import { EmptyState } from "./EmptyState";
import { LibrarySection } from "./LibrarySection";
import { SuggestedGameCard } from "./SuggestedGameCard";
import { suggestGames } from "../utils/recommendations";
import { useElementWidth } from "../hooks/useElementWidth";
import styles from "./Home.module.css";

const MAX_CONTINUE_PLAYING = 10;

// Continue Playing and Suggested share one visual row (`.topRow`'s two
// columns). Both card types are 190px wide with 20px gaps between cards
// (see ContinuePlayingRow.module.css `.row` and Home.module.css
// `.suggestionRow`), and the columns themselves are separated by a 28px
// gap. This derives how many cards fit across the whole row with no
// horizontal scrolling, then splits that in half between the two
// sections — the odd one out, if any, goes to Continue Playing.
const CARD_WIDTH = 190;
const CARD_GAP = 20;
const COLUMN_GAP = 28;

function rowCapacity(width: number): number {
  return Math.max(0, Math.floor((width - COLUMN_GAP + 2 * CARD_GAP) / (CARD_WIDTH + CARD_GAP)));
}

interface HomeProps {
  snapshot: LibrarySnapshot;
  rescanning: boolean;
  onRescan: () => void;
  onSelectGame: (game: Game) => void;
  onSetReaction: (id: string, reaction: GameReaction | null) => void;
  settings: SettingsType | null;
  onUpdateSettings: (partial: Partial<SettingsType>) => void;
}

export function Home({
  snapshot,
  rescanning,
  onRescan,
  onSelectGame,
  onSetReaction,
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

  const suggestions = useMemo(() => suggestGames(snapshot.games, continuePlayingIds), [snapshot.games, continuePlayingIds]);

  const [topRowRef, topRowWidth] = useElementWidth<HTMLDivElement>();
  const capacity = rowCapacity(topRowWidth);
  const continueSlots = Math.ceil(capacity / 2);
  const suggestedSlots = capacity - continueSlots;
  const displayedContinuePlaying = continuePlaying.slice(0, continueSlots);
  const displayedSuggestions = suggestions.slice(0, suggestedSlots);

  if (
    !snapshot.steamFound &&
    !snapshot.xboxFound &&
    !snapshot.battlenetFound &&
    !snapshot.eaFound &&
    snapshot.games.length === 0
  ) {
    return (
      <div className={styles.page}>
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
        <EmptyState
          icon="📦"
          title="No games found"
          message="Install a game from Steam, Xbox App/Game Pass or Battle.net, or configure the Steam Web API in Settings to show your owned games, then rescan your library."
          actionLabel={rescanning ? "Scanning…" : "Rescan"}
          onAction={onRescan}
          busy={rescanning}
        />
      </div>
    );
  }

  return (
    <div className={styles.page}>
      <div
        className={styles.topRow}
        ref={topRowRef}
        style={{ gridTemplateColumns: `minmax(0, ${continueSlots || 1}fr) minmax(0, ${suggestedSlots || 1}fr)` }}
      >
        <ContinuePlayingRow games={displayedContinuePlaying} onSelect={onSelectGame} />
        <section className={styles.suggestions} aria-label="Suggested games">
          <h2 className={styles.heading}>Suggested</h2>
          <div className={styles.suggestionRow}>
            {displayedSuggestions.map(({ game, reason }) => (
              <SuggestedGameCard key={game.id} game={game} reason={reason}
                onSelect={onSelectGame} onSetReaction={onSetReaction} />
            ))}
            {suggestions.length === 0 && <p>No new suggestions yet.</p>}
          </div>
        </section>
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
