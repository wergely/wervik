import type { Game } from "../types";

export interface Suggestion { game: Game; reason: string }

/** Stable local ranking; history and reactions change the preference profile. */
export function suggestGames(games: Game[], excluded: Set<string>, now = Date.now() / 1000): Suggestion[] {
  const identity = (game: Game) => game.igdbMetadata?.igdbId
    ? `igdb:${game.igdbMetadata.igdbId}` : `game:${game.id}`;
  const blocked = new Set(games.filter(g => excluded.has(g.id) || g.reaction === "dislike").map(identity));
  const seeds = games.filter(g => g.reaction !== "dislike" &&
    ((g.playtimeMinutes ?? 0) > 0 || g.lastPlayed != null || g.reaction === "like"))
    .map(game => ({ game, weight:
      Math.log1p(Math.max(0, game.playtimeMinutes ?? 0) / 60) +
      (game.lastPlayed ? 3 / (1 + Math.max(0, now - game.lastPlayed) / (30 * 86400)) : 0) +
      (game.reaction === "like" ? 3 : 0) }))
    .sort((a, b) => b.weight - a.weight).slice(0, 20);
  const totalWeight = seeds.reduce((sum, seed) => sum + seed.weight, 0) || 1;
  const normalized = (values: string[] = []) => new Set(values.map(v => v.trim().toLowerCase()));
  const ranked = games.filter(g => !blocked.has(identity(g))).map(game => {
    const genres = normalized(game.igdbMetadata?.genres);
    const developers = normalized(game.igdbMetadata?.developers);
    let affinity = 0;
    let strongest = 0;
    let matchReason = "";
    for (const seed of seeds) {
      if (identity(seed.game) === identity(game)) continue;
      const otherGenres = normalized(seed.game.igdbMetadata?.genres);
      const shared = [...genres].filter(g => otherGenres.has(g)).length;
      const union = new Set([...genres, ...otherGenres]).size;
      const sameDeveloper = [...normalized(seed.game.igdbMetadata?.developers)].some(d => developers.has(d));
      const similarity = (union ? shared / union : 0) * 0.8 + (sameDeveloper ? 0.2 : 0);
      const contribution = similarity * seed.weight;
      affinity += contribution;
      if (contribution > strongest) {
        strongest = contribution;
        matchReason = shared ? `Shares genres with ${seed.game.name}` : `Same developer as ${seed.game.name}`;
      }
    }
    const unplayed = (game.playtimeMinutes ?? 0) <= 0 && game.lastPlayed == null;
    const score = game.igdbScore == null ? 0 : Math.max(0, Math.min(100, game.igdbScore));
    const reasons = [matchReason, unplayed ? "Not played yet" : "", score > 0 ? `IGDB ${Math.round(score)}` : ""].filter(Boolean);
    return { game, rank: 60 * affinity / totalWeight + (unplayed ? 25 : 0) + score * 0.15,
      reason: reasons.join(" · ") || "Rediscover a game in your library" };
  }).sort((a, b) => b.rank - a.rank || a.game.name.localeCompare(b.game.name) || a.game.id.localeCompare(b.game.id));
  const seen = new Set<string>();
  return ranked.filter(({ game }) => {
    const key = identity(game);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  }).slice(0, 10).map(({ game, reason }) => ({ game, reason }));
}
