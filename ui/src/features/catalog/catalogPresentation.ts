import type {
  CatalogResult,
  CatalogStatus,
  CatalogView as CatalogViewState,
  Row,
} from '../../api';

/** Fixed game-tile geometry from `components.md` and the catalog reference. */
export const CATALOG_TILE_WIDTH = 172;
export const CATALOG_TILE_HEIGHT = 270;
export const CATALOG_GAP = 16;
/** The results inset is 24 px on each side and 24 below the last line. */
export const CATALOG_PADDING_X = 24;
export const CATALOG_LINE_HEIGHT = CATALOG_TILE_HEIGHT + CATALOG_GAP;

export type CatalogDirection = 'up' | 'down' | 'left' | 'right';

/**
 * A real results-container width to the reference column count. The container
 * has its own 24 px left/right padding, so only the remaining width is usable.
 */
export function catalogColumnsForWidth(width: number): number {
  const usable = width - CATALOG_PADDING_X * 2;
  if (usable <= 0) return 1;
  return Math.max(
    1,
    Math.floor((usable + CATALOG_GAP) / (CATALOG_TILE_WIDTH + CATALOG_GAP)),
  );
}

/** Splits results into the lines the virtualized grid renders. */
export function catalogLines(
  results: CatalogResult[],
  columns: number,
): CatalogResult[][] {
  const width = Math.max(1, Math.floor(columns));
  const lines: CatalogResult[][] = [];
  for (let start = 0; start < results.length; start += width) {
    lines.push(results.slice(start, start + width));
  }
  return lines;
}

/**
 * The state the results area shows. The draft, not the engine echo, decides
 * whether an answer is current: while the user is typing past the previous
 * query, the old answer is hidden instead of flashing stale tiles.
 *
 * `offline` is sticky until the view changes it: the engine sends no request
 * while the session is unverified or the network is down, so Try again is the
 * only action that may start one.
 */
export function catalogDisplayStatus(
  catalog: CatalogViewState | null | undefined,
  draft: string,
): CatalogStatus {
  const trimmed = draft.trim();
  if (trimmed.length < 2) return 'idle';
  if (!catalog) return 'loading';
  if (catalog.status === 'offline') return 'offline';

  const current = catalog.query.trim() === trimmed;
  if (!current) return 'loading';
  // A prefilled query is always searched by its opener; idle with two or more
  // characters only means that request is still on its way.
  if (catalog.status === 'idle') return 'loading';
  return catalog.status;
}

/** The first result matching an engine local row, when it exists. */
export function catalogSelectedResult(
  results: CatalogResult[],
  gameId: number | null,
): CatalogResult | null {
  if (gameId == null) return null;
  return results.find((result) => result.gameId === gameId) ?? null;
}

/**
 * Arrow-key movement over an ungrouped tile grid. Left/right stay on their
 * line; up/down keep the column and clamp to the last line, matching the
 * library grid's `Move` semantics.
 */
export function catalogFocusTarget(
  index: number,
  direction: CatalogDirection,
  columns: number,
  count: number,
): number {
  if (count <= 0) return -1;
  const last = count - 1;
  const width = Math.max(1, Math.floor(columns));
  if (index < 0 || index > last) return direction === 'up' ? last : 0;

  const line = Math.floor(index / width);
  const column = index % width;
  const lineCount = Math.ceil(count / width);

  switch (direction) {
    case 'left':
      return column > 0 ? index - 1 : index;
    case 'right':
      return column + 1 < width && index + 1 <= last ? index + 1 : index;
    case 'up':
      return line > 0 ? index - width : index;
    case 'down': {
      if (line + 1 >= lineCount) return index;
      return Math.min(index + width, last);
    }
  }
}

/**
 * A `Row`-shaped projection for the shared presentational `GameTile`. Catalog
 * results have no local library entry, so every library-only fact stays false;
 * the cached cover arrives through `Row.coverFile`. `status` is
 * only a type-valid placeholder: `CatalogTile` supplies the `meta` slot and the
 * accessible name, so no match dot, match text or playtime is ever rendered.
 */
export function catalogResultRow(result: CatalogResult): Row {
  return {
    playtime: { seconds: null, source: 'internal', lastPlayedAt: null },
    running: false,
    sessionStartedAt: null,
    key: `catalog:${result.gameId}`,
    title: result.title,
    detectedName: result.title,
    matchedName: result.title,
    provider: 'gamenode',
    providerName: '',
    installed: false,
    owned: false,
    shared: false,
    status: {
      matched: { gameId: result.gameId, source: 'automatic' },
    },
    coverFile: result.coverFile ?? null,
    releaseYear: result.year,
    addedAt: 0,
    isFavorite: false,
    hidden: false,
    gamenode: null,
  };
}

/**
 * Controls that own Enter, Space or printable characters. Catalog navigation
 * must not steal them; the detail buttons and the search input keep native
 * behavior.
 */
export function isCatalogInteractiveTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return Boolean(
    target.closest(
      'button, a, input, textarea, select, [contenteditable="true"], [role="button"], [role="menuitem"], [role="menu"], [role="dialog"], [role="alertdialog"], [role="slider"], [role="checkbox"]',
    ),
  );
}
