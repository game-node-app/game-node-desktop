import { beforeEach, describe, expect, it } from 'vitest';
import {
  FLOATING_MODAL_STORAGE_PREFIX,
  clampGeometry,
  createCenteredGeometry,
  fitSize,
  readStoredGeometry,
  writeStoredGeometry,
  type GeometryConstraints,
} from './geometry';

const constraints: GeometryConstraints = {
  defaultSize: { width: 960, height: 720 },
  minSize: { width: 720, height: 480 },
  offset: 24,
  viewport: { width: 1280, height: 800 },
};

describe('floating modal geometry', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it('centers the default size in the viewport', () => {
    expect(createCenteredGeometry(constraints)).toEqual({
      width: 960,
      height: 720,
      x: 160,
      y: 40,
    });
  });

  it('shrinks defaults to fit a small viewport', () => {
    expect(
      createCenteredGeometry({
        ...constraints,
        viewport: { width: 960, height: 600 },
      }),
    ).toEqual({
      width: 912,
      height: 552,
      x: 24,
      y: 24,
    });
  });

  it('respects minimum size and explicit maximum size', () => {
    expect(
      fitSize({ width: 100, height: 100 }, constraints),
    ).toEqual({ width: 720, height: 480 });

    expect(
      fitSize(
        { width: 2000, height: 2000 },
        { ...constraints, maxSize: { width: 900, height: 700 } },
      ),
    ).toEqual({ width: 900, height: 700 });
  });

  it('clamps position so the whole window stays inside the viewport', () => {
    expect(
      clampGeometry(
        { x: -100, y: 900, width: 900, height: 600 },
        constraints,
      ),
    ).toEqual({ x: 24, y: 176, width: 900, height: 600 });
  });

  it('round-trips valid geometry through localStorage', () => {
    writeStoredGeometry('settings', {
      x: 220,
      y: 30,
      width: 880,
      height: 670,
    });

    expect(
      window.localStorage.getItem(`${FLOATING_MODAL_STORAGE_PREFIX}settings`),
    ).toContain('"v":1');
    expect(readStoredGeometry('settings', constraints)).toEqual({
      x: 220,
      y: 30,
      width: 880,
      height: 670,
    });
  });

  it('ignores invalid or out-of-bounds saved geometry', () => {
    window.localStorage.setItem(
      `${FLOATING_MODAL_STORAGE_PREFIX}broken`,
      '{not json',
    );
    expect(readStoredGeometry('broken', constraints)).toBeNull();

    window.localStorage.setItem(
      `${FLOATING_MODAL_STORAGE_PREFIX}negative`,
      JSON.stringify({ v: 1, x: 0, y: 0, width: -1, height: 20 }),
    );
    expect(readStoredGeometry('negative', constraints)).toBeNull();

    window.localStorage.setItem(
      `${FLOATING_MODAL_STORAGE_PREFIX}offscreen`,
      JSON.stringify({ v: 1, x: 5000, y: 5000, width: 900, height: 600 }),
    );
    expect(readStoredGeometry('offscreen', constraints)).toEqual({
      x: 356,
      y: 176,
      width: 900,
      height: 600,
    });
  });
});
