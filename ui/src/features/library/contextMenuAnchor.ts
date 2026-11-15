import { openContextMenu } from '../../store/uiStore';

/** A rectangle in viewport coordinates, as returned by `getBoundingClientRect`. */
export interface ContextMenuRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface ContextMenuPoint {
  x: number;
  y: number;
}

/** Space kept between a pointer-anchored menu and the viewport edges, in CSS pixels. */
export const CONTEXT_MENU_VIEWPORT_MARGIN = 8;

/** Inline style of the fixed, zero-size element the menu is positioned against. */
export interface ContextMenuAnchorStyle {
  position: 'fixed';
  left: string;
  top: string;
  width: 0;
  height: 0;
}

export interface ContextMenuAnchorInput {
  x: number;
  y: number;
  /** Rendered menu width; the menu grows to the right from the anchor. */
  menuWidth: number;
  /** Viewport size; defaults to the runtime window. */
  viewport?: { width: number; height: number };
}

function viewportSize(): { width: number; height: number } {
  return {
    width: typeof window !== 'undefined' ? window.innerWidth : 0,
    height: typeof window !== 'undefined' ? window.innerHeight : 0,
  };
}

/** Keeps `value` inside `[minimum, maximum]`, preferring `minimum` when the bounds cross. */
function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(value, maximum));
}

/**
 * The fixed zero-size anchor style for a menu opened at viewport coordinates `(x, y)`.
 *
 * The dropdown starts at `bottom-start`; the menu's flip and size middlewares choose the
 * side and available height. Never subtract a guessed menu height,
 * because that moves the menu away from the pointer and leaves a gap under tall or short menus.
 * Horizontally the menu grows to the right, so the menu width is subtracted to keep the whole
 * dropdown on screen. Both axes keep a margin from the viewport edges.
 */
export function contextMenuAnchorStyle({
  x,
  y,
  menuWidth,
  viewport = viewportSize(),
}: ContextMenuAnchorInput): ContextMenuAnchorStyle {
  const margin = CONTEXT_MENU_VIEWPORT_MARGIN;
  return {
    position: 'fixed',
    left: `${clamp(x, margin, viewport.width - menuWidth - margin)}px`,
    top: `${clamp(y, margin, viewport.height - margin)}px`,
    width: 0,
    height: 0,
  };
}

/**
 * The context-menu anchor for a focused element: the centre of its rectangle, so a keyboard
 * open (Shift+F10 or the ContextMenu key) never lands at the viewport origin. Without an
 * element the viewport centre is used, which is still a visible position.
 */
export function contextMenuPointFor(
  rect: ContextMenuRect | null | undefined,
  viewport: { width: number; height: number } = {
    width: typeof window !== 'undefined' ? window.innerWidth : 0,
    height: typeof window !== 'undefined' ? window.innerHeight : 0,
  },
): ContextMenuPoint {
  if (!rect) {
    return { x: Math.round(viewport.width / 2), y: Math.round(viewport.height / 2) };
  }
  return {
    x: Math.round(rect.left + rect.width / 2),
    y: Math.round(rect.top + rect.height / 2),
  };
}

/** Reads an element's viewport rectangle, or `null` when there is no element. */
export function contextMenuRectFor(element: Element | null | undefined): ContextMenuRect | null {
  if (!element) return null;
  const rect = element.getBoundingClientRect();
  return { left: rect.left, top: rect.top, width: rect.width, height: rect.height };
}

/**
 * Opens the context menu for a library entry at the centre of `element`. The library screen's
 * focused-container handler calls this for Shift+F10 and the ContextMenu key; the menu itself
 * clamps the point inside the viewport and keeps the shared focus/Escape/outside-click behavior.
 */
export function openContextMenuForElement(
  element: Element | null | undefined,
  entryKey: string,
): ContextMenuPoint {
  const point = contextMenuPointFor(contextMenuRectFor(element));
  openContextMenu(point.x, point.y, entryKey);
  return point;
}
