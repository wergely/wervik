import type { AchievementDetails } from "../components/AchievementDetailsModal";
import type { Game, RaGameAchievements, SteamGameAchievements } from "../types";

/** RetroAchievements and Steam achievements render identically except for
 * points/hardcore, which only RetroAchievements has — this is what both
 * `RaGameAchievements` and `SteamGameAchievements` get adapted into so
 * rendering code never needs to care which source it came from. Shared by
 * `GameDetail`'s achievements panel and the full `GameAchievements` page. */
export interface DisplayAchievements {
  source: "retroachievements" | "steam";
  earnedCount: number;
  totalCount: number;
  pointsEarned: number | null;
  pointsTotal: number | null;
  achievements: DisplayAchievement[];
}

export interface DisplayAchievement {
  id: string;
  title: string;
  description: string;
  iconUrl: string;
  unlocked: boolean;
  unlockedHardcore: boolean;
  points: number | null;
  unlockedAt: number | null;
}

export function toAchievementDetails(
  achievement: DisplayAchievement,
  source: DisplayAchievements["source"],
  gameName: string,
): AchievementDetails {
  return {
    ...achievement,
    source: source === "steam" ? "Steam" : "RetroAchievements",
    gameName,
  };
}

export function fromRa(data: RaGameAchievements): DisplayAchievements {
  return {
    source: "retroachievements",
    earnedCount: data.earnedCount,
    totalCount: data.totalCount,
    pointsEarned: data.pointsEarned,
    pointsTotal: data.pointsTotal,
    achievements: data.achievements.map((achievement) => ({
      id: String(achievement.id),
      title: achievement.title,
      description: achievement.description,
      iconUrl: achievement.badgeUrl,
      unlocked: achievement.unlocked,
      unlockedHardcore: achievement.unlockedHardcore,
      points: achievement.points,
      unlockedAt: achievement.unlockedAt,
    })),
  };
}

export function fromSteam(data: SteamGameAchievements): DisplayAchievements {
  return {
    source: "steam",
    earnedCount: data.earnedCount,
    totalCount: data.totalCount,
    pointsEarned: null,
    pointsTotal: null,
    achievements: data.achievements.map((achievement) => ({
      id: achievement.id,
      title: achievement.title,
      description: achievement.description,
      iconUrl: achievement.iconUrl,
      unlocked: achievement.unlocked,
      unlockedHardcore: false,
      points: null,
      unlockedAt: achievement.unlockedAt,
    })),
  };
}

export function supportsHardcoreMode(game: Game): boolean {
  return (
    game.source === "retro" &&
    (game.nativeEmulator?.startsWith("RetroArch ·") === true || game.nativeEmulator === "PCSX2")
  );
}
