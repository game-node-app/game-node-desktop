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
