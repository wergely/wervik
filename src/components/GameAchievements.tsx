import { useMemo, useState } from "react";
import { useArtwork } from "../hooks/useArtwork";
import { useGameAchievements } from "../hooks/useGameAchievements";
import type { Game } from "../types";
import { sourceLabel } from "../utils/format";
import { supportsHardcoreMode, toAchievementDetails, type DisplayAchievement } from "../utils/achievements";
import { AchievementDetailsModal } from "./AchievementDetailsModal";
import { PlatformIcon } from "./PlatformIcon";
import { SourceBadge } from "./SourceBadge";
import { Spinner } from "./Spinner";
import { Switch } from "./Switch";
import styles from "./GameAchievements.module.css";

interface GameAchievementsProps {
  game: Game;
  onBack: () => void;
  onSetHardcoreMode: (id: string, enabled: boolean) => void;
  registerBackHandler: (handler: (() => void) | null) => void;
  registerNavScope: (el: HTMLElement | null) => void;
  isPlaying: boolean;
  justFinishedGameId: string | null;
}

type Filter = "all" | "unlocked" | "locked";
type Sort = "default" | "recent" | "points" | "alpha";

function formatUnlockDate(unixSeconds: number): string {
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium" }).format(new Date(unixSeconds * 1000));
}

/** A donut built from a plain CSS `conic-gradient` — no SVG needed for a
 * single sweep. `--ring-deg` is how far around the violet→cyan sweep goes
 * before giving way to the empty track color. */
function CompletionRing({ percent }: { percent: number }) {
  const clamped = Math.min(100, Math.max(0, percent));
  return (
    <div
      className={styles.ring}
      style={{ "--ring-deg": `${clamped * 3.6}deg` } as React.CSSProperties}
      role="img"
      aria-label={`${clamped}% complete`}
    >
      <div className={styles.ringHole}>
        <span className={styles.ringValue}>{clamped}%</span>
      </div>
    </div>
  );
}

export function GameAchievements({
  game,
  onBack,
  onSetHardcoreMode,
  registerBackHandler,
  registerNavScope,
  isPlaying,
  justFinishedGameId,
}: GameAchievementsProps) {
  const { achievements, loading, refreshing, error, refresh } = useGameAchievements(game, justFinishedGameId);
  const [selectedAchievement, setSelectedAchievement] = useState<DisplayAchievement | null>(null);
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("default");

  const hero = useArtwork(game.heroPath ?? game.coverPath);

  const unlockedCount = achievements?.achievements.filter((a) => a.unlocked).length ?? 0;
  const lockedCount = achievements ? achievements.achievements.length - unlockedCount : 0;
  const hardcoreCount = achievements?.achievements.filter((a) => a.unlockedHardcore).length ?? 0;
  const percent =
    achievements && achievements.totalCount > 0
      ? Math.round((achievements.earnedCount / achievements.totalCount) * 100)
      : 0;

  const recentUnlocks = useMemo(() => {
    if (!achievements) return [];
    return achievements.achievements
      .filter((a) => a.unlocked && a.unlockedAt != null)
      .sort((a, b) => (b.unlockedAt ?? 0) - (a.unlockedAt ?? 0))
      .slice(0, 6);
  }, [achievements]);

  const visibleAchievements = useMemo(() => {
    if (!achievements) return [];
    let list = achievements.achievements;
    if (filter === "unlocked") list = list.filter((a) => a.unlocked);
    if (filter === "locked") list = list.filter((a) => !a.unlocked);
    const query = search.trim().toLowerCase();
    if (query) {
      list = list.filter(
        (a) => a.title.toLowerCase().includes(query) || a.description.toLowerCase().includes(query),
      );
    }
    const sorted = [...list];
    if (sort === "recent") sorted.sort((a, b) => (b.unlockedAt ?? 0) - (a.unlockedAt ?? 0));
    else if (sort === "points") sorted.sort((a, b) => (b.points ?? 0) - (a.points ?? 0));
    else if (sort === "alpha") sorted.sort((a, b) => a.title.localeCompare(b.title));
    else sorted.sort((a, b) => Number(b.unlocked) - Number(a.unlocked));
    return sorted;
  }, [achievements, filter, search, sort]);

  function openAchievement(achievement: DisplayAchievement) {
    setSelectedAchievement(achievement);
  }

  return (
    <div className={styles.page}>
      <button type="button" className={styles.backButton} data-nav-item onClick={onBack} aria-label="Back">
        ← Back
      </button>

      <div className={styles.hero}>
        {hero.url && !hero.failed ? (
          <img className={styles.heroImage} src={hero.url} alt="" draggable={false} loading="eager" decoding="async" />
        ) : (
          <div className={styles.heroFallbackIcon}>
            <PlatformIcon source={game.source} platformKey={game.platformKey} size={120} />
          </div>
        )}
        <div className={styles.heroScrim} />
        <div className={styles.heroContent}>
          <SourceBadge
            label={sourceLabel(game)}
            source={game.source}
            platformKey={game.platformKey}
            className={styles.sourceBadge}
          />
          <h1 className={styles.title}>{game.name}</h1>
          <p className={styles.eyebrow}>Achievements</p>

          {achievements && achievements.totalCount > 0 && (
            <div className={styles.heroStats}>
              <CompletionRing percent={percent} />
              <div className={styles.heroStatList}>
                <div className={styles.heroStat}>
                  <span className={styles.heroStatValue}>
                    {achievements.earnedCount}/{achievements.totalCount}
                  </span>
                  <span className={styles.heroStatLabel}>Unlocked</span>
                </div>
                {achievements.pointsTotal != null && (
                  <div className={styles.heroStat}>
                    <span className={styles.heroStatValue}>
                      {achievements.pointsEarned}/{achievements.pointsTotal}
                    </span>
                    <span className={styles.heroStatLabel}>Points</span>
                  </div>
                )}
                {hardcoreCount > 0 && (
                  <div className={styles.heroStat}>
                    <span className={styles.heroStatValue}>{hardcoreCount}</span>
                    <span className={styles.heroStatLabel}>Hardcore</span>
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </div>

      {loading && (
        <p className={styles.status}>
          <Spinner /> Loading achievements…
        </p>
      )}
      {error && <p className={styles.statusError}>{error}</p>}

      {achievements && achievements.totalCount === 0 && (
        <p className={styles.status}>
          No achievements found for {game.name} yet — Wervik hasn't matched it to a tracked game, or it has none.
        </p>
      )}

      {!loading && !achievements && !error && (
        <p className={styles.status}>
          {game.source === "retro"
            ? "Configure RetroAchievements in Settings to track this game."
            : "This game doesn't have achievement tracking."}
        </p>
      )}

      {achievements && achievements.totalCount > 0 && (
        <>
          <div className={styles.toolbar}>
            <input
              type="search"
              className={styles.search}
              data-nav-item
              placeholder="Search achievements…"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              aria-label="Search achievements"
            />
            <div className={styles.filterChips} role="tablist" aria-label="Filter achievements">
              {(
                [
                  ["all", `All ${achievements.totalCount}`],
                  ["unlocked", `Unlocked ${unlockedCount}`],
                  ["locked", `Locked ${lockedCount}`],
                ] as [Filter, string][]
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  role="tab"
                  aria-selected={filter === id}
                  className={`${styles.filterChip} ${filter === id ? styles.filterChipActive : ""}`}
                  data-nav-item
                  onClick={() => setFilter(id)}
                >
                  {label}
                </button>
              ))}
            </div>
            <select
              className={styles.sortSelect}
              data-nav-item
              value={sort}
              onChange={(event) => setSort(event.target.value as Sort)}
              aria-label="Sort achievements"
            >
              <option value="default">Unlocked first</option>
              <option value="recent">Recently unlocked</option>
              {achievements.pointsTotal != null && <option value="points">Points</option>}
              <option value="alpha">Alphabetical</option>
            </select>
            <div className={styles.toolbarActions}>
              {supportsHardcoreMode(game) && (
                <div
                  className={styles.hardcoreInline}
                  title="Disables save states, rewind and cheats for this game. Applies on the next launch."
                >
                  <span>Hardcore</span>
                  <Switch
                    checked={game.hardcoreMode}
                    onChange={(enabled) => onSetHardcoreMode(game.id, enabled)}
                    label={`Hardcore mode for ${game.name}`}
                    disabled={isPlaying}
                  />
                </div>
              )}
              <button
                type="button"
                className={styles.refreshButton}
                data-nav-item
                onClick={refresh}
                disabled={refreshing}
                aria-label="Refresh achievements"
                title="Refresh achievements"
              >
                {refreshing ? <Spinner /> : "↻"}
              </button>
            </div>
          </div>

          {recentUnlocks.length > 0 && (
            <section className={styles.recentSection} aria-label="Recently unlocked">
              <h2 className={styles.sectionTitle}>Recently unlocked</h2>
              <div className={`${styles.recentStrip} wervik-scroll`}>
                {recentUnlocks.map((achievement) => (
                  <button
                    key={achievement.id}
                    type="button"
                    className={styles.recentCard}
                    data-nav-item
                    onClick={() => openAchievement(achievement)}
                    title={achievement.title}
                  >
                    <img className={styles.recentBadge} src={achievement.iconUrl} alt="" loading="lazy" />
                    <span className={styles.recentTitle}>{achievement.title || "Unnamed achievement"}</span>
                    <span className={styles.recentDate}>
                      {achievement.unlockedAt != null ? formatUnlockDate(achievement.unlockedAt) : ""}
                    </span>
                  </button>
                ))}
              </div>
            </section>
          )}

          <section className={styles.gridSection} aria-label="All achievements">
            {visibleAchievements.length === 0 ? (
              <p className={styles.status}>No achievements match your filters.</p>
            ) : (
              <div className={styles.grid}>
                {visibleAchievements.map((achievement) => (
                  <button
                    key={achievement.id}
                    type="button"
                    className={`${styles.card} ${achievement.unlocked ? styles.cardUnlocked : styles.cardLocked}`}
                    data-nav-item
                    onClick={() => openAchievement(achievement)}
                    aria-label={`Show details for ${achievement.title || "Unnamed achievement"}`}
                  >
                    <div className={styles.cardBadgeWrap}>
                      <img className={styles.cardBadge} src={achievement.iconUrl} alt="" loading="lazy" />
                      {!achievement.unlocked && (
                        <span className={styles.lockOverlay} aria-hidden="true">
                          🔒
                        </span>
                      )}
                    </div>
                    <div className={styles.cardInfo}>
                      <div className={styles.cardTitle}>{achievement.title || "Unnamed achievement"}</div>
                      {achievement.description && (
                        <div className={styles.cardDescription}>{achievement.description}</div>
                      )}
                      <div className={styles.cardMeta}>
                        {achievement.points != null && <span>{achievement.points} pts</span>}
                        {achievement.unlockedHardcore && <span className={styles.hardcoreTag}>Hardcore</span>}
                        {achievement.unlocked && achievement.unlockedAt != null && (
                          <span>{formatUnlockDate(achievement.unlockedAt)}</span>
                        )}
                      </div>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </section>

          <p className={styles.credit}>
            Achievements from {achievements.source === "steam" ? "Steam" : "RetroAchievements"}
          </p>
        </>
      )}

      {selectedAchievement && achievements && (
        <AchievementDetailsModal
          achievement={toAchievementDetails(selectedAchievement, achievements.source, game.name)}
          onClose={() => setSelectedAchievement(null)}
          registerBackHandler={registerBackHandler}
          registerNavScope={registerNavScope}
        />
      )}
    </div>
  );
}
