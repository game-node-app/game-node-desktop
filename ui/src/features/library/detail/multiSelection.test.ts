import { describe, expect, it } from 'vitest';
import type { Row } from '../../../api';
import { summarizeSelection } from './multiSelection';

function row(overrides: Partial<Row> = {}): Row {
  return {
    key: 'steam:1',
    title: 'Game',
    detectedName: 'Game',
    matchedName: 'Game',
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    running: false,
    status: { matched: { gameId: 1, source: 'automatic' } },
    coverFile: null,
    playtime: { seconds: 3600, source: 'store', lastPlayedAt: null },
    releaseYear: 2020,
    addedAt: 0,
    isFavorite: false,
    hidden: false,
    gamenode: null,
    ...overrides,
  };
}

describe('summarizeSelection', () => {
  it('keeps selection order, ignores missing keys and separates known playtime', () => {
    const summary = summarizeSelection(
      ['steam:2', 'steam:1', 'gone'],
      [
        row({ key: 'steam:1', playtime: { seconds: 120, source: 'store', lastPlayedAt: null } }),
        row({
          key: 'steam:2',
          installed: false,
          playtime: { seconds: null, source: 'store', lastPlayedAt: null },
          gamenode: { status: 'planned', collectionIds: [], entryId: 'entry' },
        }),
      ],
    );

    expect(summary.rows.map((entry) => entry.key)).toEqual(['steam:2', 'steam:1']);
    expect(summary.totalSeconds).toBe(120);
    expect(summary.installedCount).toBe(1);
    expect(summary.allInstalled).toBe(false);
    expect(summary.inGameNode).toBe(1);
    expect(summary.notOnGameNode).toBe(1);
    expect(summary.matched).toBe(2);
    expect(summary.allFavorites).toBe(false);
  });

  it('reports unknown total time and Unfavorite all only over matched favorites', () => {
    const summary = summarizeSelection(
      ['steam:1', 'steam:2', 'steam:3'],
      [
        row({ key: 'steam:1', isFavorite: true, playtime: { seconds: null, source: 'store', lastPlayedAt: null } }),
        row({ key: 'steam:2', isFavorite: true, playtime: { seconds: null, source: 'store', lastPlayedAt: null } }),
        row({ key: 'steam:3', status: 'unmatched', isFavorite: true, playtime: { seconds: null, source: 'store', lastPlayedAt: null } }),
      ],
    );

    expect(summary.totalSeconds).toBeNull();
    expect(summary.favorites).toBe(2);
    expect(summary.matched).toBe(2);
    expect(summary.allFavorites).toBe(true);
  });
});
