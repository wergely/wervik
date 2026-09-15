import { Fragment } from "react";
import styles from "./ActiveFeaturesRow.module.css";

interface ActiveFeaturesRowProps {
  features: string[];
}

/** Subtle "MangoHud • GameMode" summary of what's active for this game.
 * Renders nothing at all when there's nothing to show. */
export function ActiveFeaturesRow({ features }: ActiveFeaturesRowProps) {
  if (features.length === 0) return null;

  return (
    <div className={styles.row}>
      {features.map((feature, i) => (
        <Fragment key={feature}>
          {i > 0 && <span className={styles.dot}>•</span>}
          <span>{feature}</span>
        </Fragment>
      ))}
    </div>
  );
}
