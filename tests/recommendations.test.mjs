import { test } from 'node:test';
import assert from 'node:assert/strict';
import { suggestGames } from '../src/utils/recommendations.ts';
const game = (id, overrides = {}) => ({ id, name: id, playtimeMinutes: null, lastPlayed: null, reaction: null, igdbScore: null, igdbMetadata: null, ...overrides });
const meta = (id, genres) => ({ igdbId: id, genres, developers: [] });
test('ranks affinity, unplayed status and score, with an actual matching reason', () => {
  const seed = game('favorite', { playtimeMinutes: 600, lastPlayed: 1000, igdbMetadata: meta(1, ['RPG']) });
  const related = game('related', { igdbMetadata: meta(2, ['RPG']), igdbScore: 85 });
  const lower = game('lower', { igdbMetadata: meta(3, ['RPG']), igdbScore: 60 });
  const played = game('played', { igdbMetadata: meta(4, ['RPG']), playtimeMinutes: 1, igdbScore: 85 });
  const unrelated = game('unrelated', { igdbMetadata: meta(5, ['Sport']), igdbScore: 95 });
  const result = suggestGames([seed, unrelated, lower, played, related], new Set(['favorite']), 1000);
  assert.equal(result[0].game.id, 'related');
  assert.match(result[0].reason, /favorite.*Not played yet.*85/);
  assert.ok(result.findIndex(r => r.game.id === 'lower') < result.findIndex(r => r.game.id === 'played'));
});
test('excludes history and disliked identities across sources and deduplicates results', () => {
  const games = [game('recent', { igdbMetadata: meta(1, []) }), game('copy', { igdbMetadata: meta(1, []) }),
    game('bad', { reaction: 'dislike', igdbMetadata: meta(2, []) }), game('bad-copy', { igdbMetadata: meta(2, []) }),
    game('a', { igdbMetadata: meta(3, []) }), game('b', { igdbMetadata: meta(3, []) })];
  assert.equal(suggestGames(games, new Set(['recent'])).length, 1);
});
test('cold start uses score, stays deterministic and caps at ten', () => {
  const games = Array.from({ length: 15 }, (_, i) => game(String(i), { igdbScore: i * 5 }));
  const result = suggestGames(games, new Set());
  assert.equal(result.length, 10);
  assert.equal(result[0].game.id, '14');
  assert.deepEqual(result, suggestGames([...games].reverse(), new Set()));
  assert.deepEqual(suggestGames([], new Set()), []);
});
test('likes supply affinity even without playtime', () => {
  const result = suggestGames([game('liked', { reaction: 'like', igdbMetadata: meta(1, ['RPG']) }),
    game('match', { igdbMetadata: meta(2, ['RPG']) }), game('other', { igdbScore: 100 })], new Set(['liked']));
  assert.equal(result[0].game.id, 'match');
});
