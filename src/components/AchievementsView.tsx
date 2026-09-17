import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { getAchievementsOverview, getRaProfileSummary } from "../api/tauri";
import type { AchievementsOverview, AchievementSource, Game, LibrarySnapshot, RaProfileSummary } from "../types";
import { formatLastPlayed } from "../utils/format";
import { AchievementDetailsModal, type AchievementDetails } from "./AchievementDetailsModal";
import { AchievementGameCard } from "./AchievementGameCard";
import { PlatformIcon } from "./PlatformIcon";
import { Spinner } from "./Spinner";
import styles from "./AchievementsView.module.css";

type Tab = "overview" | "games" | "platforms" | "recent";

const TABS: { id: Tab; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "games", label: "Games" },
  { id: "platforms", label: "Platforms" },
  { id: "recent", label: "Recently Unlocked" },
];

/** Maximum number of "closest to 100%" cards the Overview tab shows — a
 * shortlist, not a full browser (that's what the Games tab is for). */
const OVERVIEW_GAMES_LIMIT = 10;
const OVERVIEW_UNLOCKS_LIMIT = 6;
const OVERVIEW_CARD_WIDTH = 190;
const OVERVIEW_CARD_GAP = 20;

interface AchievementsViewProps {
  snapshot: LibrarySnapshot;
  onSelectGame: (game: Game) => void;
  registerBackHandler: (handler: (() => void) | null) => void;
  registerNavScope: (el: HTMLElement | null) => void;
}

interface PlatformGroup {
  label: string;
  source: AchievementSource;
  platformKey: string | null;
  earnedCount: number;
  totalCount: number;
  gameCount: number;
}

function groupByPlatform(overview: AchievementsOverview): PlatformGroup[] {
  const groups = new Map<string, PlatformGroup>();
  for (const game of overview.games) {
    const label = game.platform ?? "Steam";
    const group =
      groups.get(label) ??
      ({
        label,
        source: game.source,
        platformKey: game.platformKey,
        earnedCount: 0,
        totalCount: 0,
        gameCount: 0,
      } satisfies PlatformGroup);
    group.earnedCount += game.earnedCount;
    group.totalCount += game.totalCount;
    group.gameCount += 1;
    groups.set(label, group);
  }
  return Array.from(groups.values()).sort((a, b) => b.totalCount - a.totalCount);
}

export function AchievementsView({
  snapshot,
  onSelectGame,
  registerBackHandler,
  registerNavScope,
}: AchievementsViewProps) {
  const [tab, setTab] = useState<Tab>("overview");
  const [overview, setOverview] = useState<AchievementsOverview | null>(null);
  const [profile, setProfile] = useState<RaProfileSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [overviewCardCount, setOverviewCardCount] = useState(1);
  const [selectedUnlock, setSelectedUnlock] = useState<AchievementsOverview["recentUnlocks"][number] | null>(null);
  const overviewCardRowRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    Promise.all([getAchievementsOverview(), getRaProfileSummary().catch(() => null)])
      .then(([overviewValue, profileValue]) => {
        if (cancelled) return;
        setOverview(overviewValue);
        setProfile(profileValue);
      })
      .catch(() => {
        if (!cancelled) setOverview(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Real box art comes from the library snapshot Wervik already has loaded
  // (same `Game` objects `Home`/`LibrarySection` render), not from RA/Steam
  // icon URLs — those are small achievement/game icons, not cover art.
  const gamesById = useMemo(() => new Map(snapshot.games.map((game) => [game.id, game])), [snapshot.games]);

  function selectGame(gameId: string) {
    const game = gamesById.get(gameId);
    if (game) onSelectGame(game);
  }

  // Already sorted completion-desc (then by name) by `get_achievements_overview`.
  // A game with 0% is one Wervik matched but the player hasn't actually
  // earned anything in yet — noise in a card browser, so it's hidden from
  // both the shortlist and the full Games tab (below).
  const startedGames = useMemo(
    () => (overview ? overview.games.filter((game) => game.completionPercent > 0) : []),
    [overview],
  );
  const inProgress = useMemo(() => startedGames.filter((game) => game.completionPercent < 100), [startedGames]);
  const overviewGames = inProgress.slice(0, Math.min(OVERVIEW_GAMES_LIMIT, overviewCardCount));
  const platformGroups = useMemo(() => (overview ? groupByPlatform(overview) : []), [overview]);
  const completedGamesCount = useMemo(
    () => (overview ? overview.games.filter((game) => game.completionPercent === 100).length : 0),
    [overview],
  );

  const overallPercent =
    overview && overview.totalPossible > 0
      ? Math.round((overview.totalEarned / overview.totalPossible) * 100)
      : null;
  const completedGamesPercent =
    overview && overview.games.length > 0 ? Math.round((completedGamesCount / overview.games.length) * 100) : null;

  useLayoutEffect(() => {
    if (tab !== "overview") return;
    const row = overviewCardRowRef.current;
    if (!row) return;

    const updateCardCount = () => {
      const count = Math.max(
        1,
        Math.floor((row.clientWidth + OVERVIEW_CARD_GAP) / (OVERVIEW_CARD_WIDTH + OVERVIEW_CARD_GAP)),
      );
      setOverviewCardCount(count);
    };

    updateCardCount();
    const observer = new ResizeObserver(updateCardCount);
    observer.observe(row);
    return () => observer.disconnect();
  }, [inProgress.length, tab]);

  function openUnlock(unlock: AchievementsOverview["recentUnlocks"][number]) {
    setSelectedUnlock(unlock);
  }

  const selectedUnlockDetails: AchievementDetails | null = selectedUnlock
    ? {
        title: selectedUnlock.achievementTitle,
        description: selectedUnlock.achievementDescription,
        iconUrl: selectedUnlock.iconUrl,
        unlocked: true,
        unlockedAt: selectedUnlock.unlockedAt,
        points: selectedUnlock.points > 0 ? selectedUnlock.points : null,
        unlockedHardcore: false,
        source: selectedUnlock.source === "steam" ? "Steam" : "RetroAchievements",
        gameName: selectedUnlock.gameName,
      }
    : null;

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Achievements</h1>

      {loading ? (
        <p className={styles.status}>
          <Spinner /> Loading…
        </p>
      ) : !overview || overview.games.length === 0 ? (
        <p className={styles.status}>
          No achievements tracked yet — configure RetroAchievements or Steam in Settings, then play a game.
        </p>
      ) : (
        <>
          <div className={styles.statsRow}>
            <StatCard
              icon="🏆"
              accent="yellow"
              label="Total Achievements"
              value={`${overview.totalEarned.toLocaleString()} / ${overview.totalPossible.toLocaleString()}`}
              percent={overallPercent}
              barPercent={overallPercent}
            />
            <StatCard
              icon="💯"
              accent="green"
              label="Games 100%"
              value={`${completedGamesCount} of ${overview.games.length} games`}
              percent={completedGamesPercent}
              barPercent={completedGamesPercent}
            />
            {overview.totalPointsPossible > 0 && (
              <StatCard
                icon="🎯"
                accent="violet"
                label="RetroAchievements"
                value={`${overview.totalPointsEarned.toLocaleString()} / ${overview.totalPointsPossible.toLocaleString()}`}
                percent={Math.round((overview.totalPointsEarned / overview.totalPointsPossible) * 100)}
                barPercent={Math.round((overview.totalPointsEarned / overview.totalPointsPossible) * 100)}
              />
            )}
            {profile && (
              <StatCard
                icon="🏅"
                accent="cyan"
                label="RA Site Rank"
                value={profile.rank > 0 ? `#${profile.rank}` : "—"}
                subtitle={`${profile.username} · ${profile.totalPoints.toLocaleString()} pts`}
              />
            )}
            <StatCard
              icon="🎮"
              accent="violet"
              label="Games Tracked"
              value={overview.games.length.toLocaleString()}
              subtitle={`${platformGroups.length} ${platformGroups.length === 1 ? "platform" : "platforms"}`}
            />
          </div>

          <div className={styles.tabs} role="tablist">
            {TABS.map((entry) => (
              <button
                key={entry.id}
                type="button"
                role="tab"
                aria-selected={tab === entry.id}
                className={`${styles.tab} ${tab === entry.id ? styles.tabActive : ""}`}
                data-nav-item
                onClick={() => setTab(entry.id)}
              >
                {entry.label}
              </button>
            ))}
          </div>

          <div className={`${styles.tabContent} wervik-scroll`}>
            {tab === "overview" && (
              <>
                {inProgress.length > 0 && (
                  <section className={styles.section}>
                    <h2 className={styles.sectionTitle}>Closest to 100%</h2>
                    <div ref={overviewCardRowRef} className={styles.cardRow}>
                      {overviewGames.map((entry) => (
                        <AchievementGameCard
                          key={`${entry.source}-${entry.gameId}`}
                          entry={entry}
                          coverPath={gamesById.get(entry.gameId)?.coverPath}
                          onSelect={() => selectGame(entry.gameId)}
                        />
                      ))}
                    </div>
                  </section>
                )}
                {overview.recentUnlocks.length > 0 && (
                  <section className={styles.section}>
                    <div className={styles.sectionHeading}>
                      <h2 className={styles.sectionTitle}>Recently unlocked</h2>
                      {overview.recentUnlocks.length > OVERVIEW_UNLOCKS_LIMIT && (
                        <button
                          type="button"
                          className={styles.viewAllButton}
                          data-nav-item
                          onClick={() => setTab("recent")}
                        >
                          View all →
                        </button>
                      )}
                    </div>
                    <UnlockList
                      unlocks={overview.recentUnlocks.slice(0, OVERVIEW_UNLOCKS_LIMIT)}
                      onSelect={openUnlock}
                    />
                  </section>
                )}
              </>
            )}

            {tab === "games" && (
              <section className={styles.section}>
                <div className={styles.cardGrid}>
                  {startedGames.map((entry) => (
                    <AchievementGameCard
                      key={`${entry.source}-${entry.gameId}`}
                      entry={entry}
                      coverPath={gamesById.get(entry.gameId)?.coverPath}
                      onSelect={() => selectGame(entry.gameId)}
                    />
                  ))}
                </div>
              </section>
            )}

            {tab === "platforms" && (
              <section className={styles.section}>
                <div className={styles.platformList}>
                  {platformGroups.map((group) => {
                    const percent = group.totalCount > 0 ? Math.round((group.earnedCount / group.totalCount) * 100) : 0;
                    return (
                      <div key={group.label} className={styles.platformRow}>
                        <div className={styles.platformHeading}>
                          <span className={styles.platformLabel}>
                            <PlatformIcon source={group.source} platformKey={group.platformKey} size={18} />
                            {group.label}
                          </span>
                          <span className={styles.platformMeta}>
                            {group.earnedCount}/{group.totalCount} · {group.gameCount}{" "}
                            {group.gameCount === 1 ? "game" : "games"}
                          </span>
                        </div>
                        <div className={styles.progressTrack}>
                          <div className={styles.progressFill} style={{ width: `${percent}%` }} />
                        </div>
                      </div>
                    );
                  })}
                </div>
              </section>
            )}

            {tab === "recent" && (
              <section className={styles.section}>
                <UnlockList unlocks={overview.recentUnlocks} onSelect={openUnlock} />
              </section>
            )}
          </div>
        </>
      )}

      {selectedUnlockDetails && (
        <AchievementDetailsModal
          achievement={selectedUnlockDetails}
          onClose={() => setSelectedUnlock(null)}
          registerBackHandler={registerBackHandler}
          registerNavScope={registerNavScope}
        />
      )}
    </div>
  );
}

interface StatCardProps {
  icon: string;
  accent: "yellow" | "violet" | "cyan" | "green";
  label: string;
  value: string;
  /** Shown as a small colored badge next to the value — omit when the stat
   * has no natural ratio (e.g. a rank or a plain count). */
  percent?: number | null;
  /** Renders a progress bar instead of `subtitle` when set. */
  barPercent?: number | null;
  subtitle?: string;
}

function StatCard({ icon, accent, label, value, percent, barPercent, subtitle }: StatCardProps) {
  return (
    <div className={styles.statCard}>
      <div className={`${styles.statIcon} ${styles[`accent${accent[0].toUpperCase()}${accent.slice(1)}`]}`}>
        <span aria-hidden="true">{icon}</span>
      </div>
      <div className={styles.statBody}>
        <span className={styles.statLabel}>{label}</span>
        <div className={styles.statValueRow}>
          <span className={styles.statValue}>{value}</span>
          {percent != null && <span className={styles.statPercent}>{percent}%</span>}
        </div>
        {barPercent != null ? (
          <div className={styles.progressTrack}>
            <div
              className={`${styles.progressFill} ${styles[`accentFill${accent[0].toUpperCase()}${accent.slice(1)}`]}`}
              style={{ width: `${Math.min(100, Math.max(0, barPercent))}%` }}
            />
          </div>
        ) : subtitle ? (
          <span className={styles.statSubtitle}>{subtitle}</span>
        ) : null}
      </div>
    </div>
  );
}

function UnlockList({
  unlocks,
  onSelect,
}: {
  unlocks: AchievementsOverview["recentUnlocks"];
  onSelect: (unlock: AchievementsOverview["recentUnlocks"][number]) => void;
}) {
  return (
    <div className={styles.unlockList}>
      {unlocks.map((unlock, index) => (
        <button
          key={`${unlock.gameId}-${unlock.achievementTitle}-${index}`}
          type="button"
          className={styles.unlockRow}
          data-nav-item
          onClick={() => onSelect(unlock)}
          aria-label={`Show details for ${unlock.achievementTitle}`}
        >
          <img className={styles.unlockIcon} src={unlock.iconUrl} alt="" loading="lazy" />
          <div className={styles.unlockInfo}>
            <span className={styles.unlockTitle} title={unlock.achievementDescription}>
              {unlock.achievementTitle}
            </span>
            <span className={styles.unlockGame}>
              {unlock.gameName}
              {unlock.points > 0 ? ` · ${unlock.points} pts` : ""}
            </span>
          </div>
          <span className={styles.unlockTime}>{formatLastPlayed(unlock.unlockedAt)}</span>
        </button>
      ))}
    </div>
  );
}
