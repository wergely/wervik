import type { Game } from "../types";
import { GameCard } from "./GameCard";
import styles from "./ContinuePlayingRow.module.css";

interface ContinuePlayingRowProps {
  games: Game[];
  onSelect: (game: Game) => void;
}

export function ContinuePlayingRow({ games, onSelect }: ContinuePlayingRowProps) {
  if (games.length === 0) return null;

  return (
    <section className={styles.section}>
      <h2 className={styles.heading}>Continue Playing</h2>
      <div className={`${styles.row} wervik-scroll`}>
        {games.map((game) => (
          <GameCard key={game.id} game={game} size="md" onSelect={onSelect} />
        ))}
      </div>
    </section>
  );
}
