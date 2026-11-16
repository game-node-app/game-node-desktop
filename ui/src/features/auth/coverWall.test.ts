import { describe, expect, it } from 'vitest';
import { readFileSync, statSync } from 'node:fs';
import {
  CURATED_SIGN_IN_COVERS,
  resolveMuralCovers,
  SIGN_IN_MURAL_TILES,
} from './coverWall';

const manifest = JSON.parse(
  readFileSync('src/assets/signin-covers/manifest.json', 'utf8'),
) as {
  count: number;
  source: string;
  covers: Array<{
    slug: string;
    title: string;
    localFile: string;
    gameId: number;
    releaseYear: number;
    igdbImageUrl: string;
  }>;
};

describe('sign-in cover wall', () => {
  it('keeps a manifest with the title and IGDB origin of every bundled cover', () => {
    expect(manifest.source).toContain('IGDB');
    expect(manifest.count).toBe(SIGN_IN_MURAL_TILES);
    expect(manifest.covers).toHaveLength(SIGN_IN_MURAL_TILES);
    expect(CURATED_SIGN_IN_COVERS).toHaveLength(SIGN_IN_MURAL_TILES);

    for (const [index, cover] of CURATED_SIGN_IN_COVERS.entries()) {
      const recorded = manifest.covers[index]!;
      expect(cover).toMatchObject({
        slug: recorded.slug,
        title: recorded.title,
        localFile: recorded.localFile,
        gameId: recorded.gameId,
        releaseYear: recorded.releaseYear,
        igdbImageUrl: recorded.igdbImageUrl,
      });
      expect(cover.title.trim()).not.toBe('');
      expect(cover.igdbImageUrl).toMatch(
        /^https:\/\/images\.igdb\.com\/igdb\/image\/upload\/t_cover_big\/[a-z0-9_]+\.jpg$/,
      );
      expect(cover.src).toContain('signin-covers');
      const file = `src/assets/signin-covers/${cover.localFile}`;
      expect(statSync(file).size).toBeGreaterThan(1_000);
      const bytes = readFileSync(file);
      expect([bytes[0], bytes[1]]).toEqual([0xff, 0xd8]);
      expect([bytes.at(-2), bytes.at(-1)]).toEqual([0xff, 0xd9]);
    }

    expect(CURATED_SIGN_IN_COVERS.map((cover) => cover.slug)).toContain('elden-ring');
    expect(CURATED_SIGN_IN_COVERS.map((cover) => cover.slug)).toContain('hades');
    expect(CURATED_SIGN_IN_COVERS.map((cover) => cover.slug)).toContain(
      'baldurs-gate-iii',
    );
  });

  it('preserves cached covers first and fills the remaining slots with IGDB samples', () => {
    const covers = resolveMuralCovers(['119133.jpg', 'local-game-0123456789abcdef.webp']);

    expect(covers).toHaveLength(SIGN_IN_MURAL_TILES);
    expect(covers[0]).toMatchObject({
      source: 'cache',
      src: 'gamenode-asset://localhost/cover/119133.jpg',
    });
    expect(covers[1]).toMatchObject({
      source: 'cache',
      src: 'gamenode-asset://localhost/cover/local-game-0123456789abcdef.webp',
    });
    expect(covers.slice(2).every((cover) => cover.source === 'curated')).toBe(true);
    expect(new Set(covers.slice(2).map((cover) => cover.src)).size).toBe(23);
  });

  it('replaces the mock host placeholders with the bundled real artwork', () => {
    const files = Array.from({ length: 12 }, (_, index) => `mock-${index}.svg`);
    const covers = resolveMuralCovers(files);

    expect(covers).toHaveLength(SIGN_IN_MURAL_TILES);
    expect(covers.every((cover) => cover.source === 'curated')).toBe(true);
    expect(covers.some((cover) => cover.src.includes('mock-'))).toBe(false);
    expect(covers.slice(12).every((cover) => cover.source === 'curated')).toBe(true);
    expect(new Set(covers.map((cover) => cover.src)).size).toBe(SIGN_IN_MURAL_TILES);
  });

  it('caps a full cache at 25 without adding samples', () => {
    const files = Array.from({ length: 30 }, (_, index) => `${index}.jpg`);
    const covers = resolveMuralCovers(files);

    expect(covers).toHaveLength(SIGN_IN_MURAL_TILES);
    expect(covers.every((cover) => cover.source === 'cache')).toBe(true);
    expect(covers[24]?.src).toContain('/24.jpg');
  });

  it('replaces invalid cover names with bundled real IGDB artwork', () => {
    const covers = resolveMuralCovers(['../escape.jpg', '.hidden.jpg', '']);

    expect(covers).toHaveLength(SIGN_IN_MURAL_TILES);
    expect(covers.every((cover) => cover.source === 'curated')).toBe(true);
  });
});
