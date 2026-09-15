import type { Game } from "../types";
import { GameCard } from "./GameCard";
import styles from "./ContinuePlayingRow.module.css";

interface ContinuePlayingRowProps {
  games: Game[];
  onSelect: (game: Game) => void;
}

export function ContinuePlayingRow({ games, onSelect }: ContinuePlayingRowProps) {
  return (
    <section className={styles.section}>
      <h2 className={styles.heading}>Continue Playing</h2>
      <div className={styles.row}>
        {games.length === 0 && <p>Play a game to start your history.</p>}
        {games.map((game) => (
          <GameCard key={game.id} game={game} size="md" onSelect={onSelect} />
        ))}
      </div>
    </section>
  );
}
