import { describe, expect, it } from 'vitest';
import {
  CONTEXT_MENU_VIEWPORT_MARGIN,
  contextMenuAnchorStyle,
} from './contextMenuAnchor';

const VIEWPORT = { width: 1024, height: 768 };
const MENU_WIDTH = 280;

describe('contextMenuAnchorStyle', () => {
  it('stays at a pointer near the footer instead of correcting for a guessed menu height', () => {
    expect(
      contextMenuAnchorStyle({
        x: 120,
        y: 731,
        menuWidth: MENU_WIDTH,
        viewport: VIEWPORT,
      }),
    ).toEqual({
      position: 'fixed',
      left: '120px',
      top: '731px',
      width: 0,
      height: 0,
    });
  });

  it('clamps a pointer outside the viewport to the eight pixel margin', () => {
    expect(
      contextMenuAnchorStyle({
        x: -20,
        y: -20,
        menuWidth: MENU_WIDTH,
        viewport: VIEWPORT,
      }),
    ).toEqual({
      position: 'fixed',
      left: '8px',
      top: '8px',
      width: 0,
      height: 0,
    });
  });

  it('subtracts the menu width from the horizontal bound so the whole menu stays on screen', () => {
    const horizontalBound = VIEWPORT.width - MENU_WIDTH - CONTEXT_MENU_VIEWPORT_MARGIN;
    expect(horizontalBound).toBe(736);
    expect(
      contextMenuAnchorStyle({
        x: 5000,
        y: 100,
        menuWidth: MENU_WIDTH,
        viewport: VIEWPORT,
      }).left,
    ).toBe('736px');
    expect(
      contextMenuAnchorStyle({
        x: horizontalBound,
        y: 100,
        menuWidth: MENU_WIDTH,
        viewport: VIEWPORT,
      }).left,
    ).toBe('736px');
  });

  it('clamps the vertical bound to the bottom margin without a menu height', () => {
    expect(
      contextMenuAnchorStyle({
        x: 120,
        y: 5000,
        menuWidth: MENU_WIDTH,
        viewport: VIEWPORT,
      }).top,
    ).toBe(`${VIEWPORT.height - CONTEXT_MENU_VIEWPORT_MARGIN}px`);
  });

  it('keeps the eight pixel margin when the viewport is narrower than the menu', () => {
    expect(
      contextMenuAnchorStyle({
        x: 100,
        y: 100,
        menuWidth: MENU_WIDTH,
        viewport: { width: 200, height: 200 },
      }).left,
    ).toBe(`${CONTEXT_MENU_VIEWPORT_MARGIN}px`);
  });

  it('defaults to the runtime window size', () => {
    expect(contextMenuAnchorStyle({ x: 5000, y: 5000, menuWidth: MENU_WIDTH })).toEqual({
      position: 'fixed',
      left: `${window.innerWidth - MENU_WIDTH - CONTEXT_MENU_VIEWPORT_MARGIN}px`,
      top: `${window.innerHeight - CONTEXT_MENU_VIEWPORT_MARGIN}px`,
      width: 0,
      height: 0,
    });
  });
});
