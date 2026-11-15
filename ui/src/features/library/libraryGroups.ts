import {
  IconBookmark,
  IconCalendar,
  IconDeviceDesktop,
  IconLink,
  type TablerIcon,
} from '@tabler/icons-react';
import type { GroupBy, Row, RowGroup } from '../../api';
import { GAME_ROW_LAYOUT } from '../../components/ui/GameRow';
import { formatI18n } from './formatters';
import { providerIcon } from './libraryControlIcons';

/**
 * Shared geometry and item builders for the virtualized grid and list.
 *
 * The engine owns the grouping: `groups` arrives in its fixed order and
 * `firstIndex` points into the already collapsed-filtered `rows`. These builders
 * only turn that shape into virtual items, so a line never mixes two groups and
 * a collapsed group mounts no games while keeping its header and engine count.
 */

export const GRID_TILE_WIDTH = 172;
export const GRID_TILE_HEIGHT = 270;
/** Horizontal and vertical gap between grid lines from the reference dump. */
export const GRID_GAP = 16;
/** The grid content inset from `raw/02-L1-main-grid.txt` (24 px each side). */
export const GRID_PADDING_X = 24;
export const GRID_GROUP_HEADER_HEIGHT = 36;
export const GRID_LINE_HEIGHT = GRID_TILE_HEIGHT + GRID_GAP;
export const LIST_ROW_HEIGHT = GAME_ROW_LAYOUT.height;
export const LIST_GROUP_HEADER_HEIGHT = 36;

/** Real container width to a column count, using the 24 px inset and 16 px gap. */
export function columnsForWidth(width: number): number {
  const usable = width - GRID_PADDING_X * 2;
  if (usable <= 0) return 1;
  return Math.max(
    1,
    Math.floor((usable + GRID_GAP) / (GRID_TILE_WIDTH + GRID_GAP)),
  );
}

export interface GroupSlice {
  group: RowGroup;
  rows: Row[];
}

/**
 * Slices the engine's visible rows by the group boundaries it publishes.
 * A collapsed group keeps its header but yields no rows. The caller never has
 * to guess where a group ends: the next group's `firstIndex` (or the end of
 * `rows`) is authoritative, which also handles a collapsed sibling correctly.
 */
export function groupSlices(rows: Row[], groups: RowGroup[]): GroupSlice[] {
  return groups.map((group, index) => {
    if (group.collapsed) return { group, rows: [] };
    const start = Math.min(Math.max(group.firstIndex, 0), rows.length);
    const next =
      index + 1 < groups.length ? groups[index + 1].firstIndex : rows.length;
    const end = Math.min(Math.max(next, start), rows.length);
    return { group, rows: rows.slice(start, end) };
  });
}

export interface GridGroupItem {
  kind: 'group';
  key: string;
  group: RowGroup;
}

export interface GridLineItem {
  kind: 'line';
  key: string;
  /** `null` for an ungrouped view; otherwise the group the whole line belongs to. */
  groupId: string | null;
  rows: Row[];
}

export type GridItem = GridGroupItem | GridLineItem;

/**
 * Virtual items for the grid: one full-width header per rendered group, then
 * lines chunked inside that group only. An ungrouped view has no headers.
 */
export function buildGridItems(
  rows: Row[],
  groups: RowGroup[],
  columns: number,
): GridItem[] {
  const width = Math.max(1, Math.floor(columns));
  const slices: Array<{ group: RowGroup | null; rows: Row[] }> =
    groups.length > 0 ? groupSlices(rows, groups) : [{ group: null, rows }];
  const items: GridItem[] = [];

  for (const slice of slices) {
    const groupId = slice.group?.id ?? null;
    if (slice.group) {
      items.push({ kind: 'group', key: `group:${slice.group.id}`, group: slice.group });
      if (slice.group.collapsed) continue;
    }
    for (let start = 0; start < slice.rows.length; start += width) {
      const line = slice.rows.slice(start, start + width);
      items.push({
        kind: 'line',
        // The row composition is part of the key so a line keeps its exact
        // virtualizer measurement only while it holds the same games.
        key: `line:${groupId ?? 'all'}:${start}:${line.map((row) => row.key).join('|')}`,
        groupId,
        rows: line,
      });
    }
  }

  return items;
}

export interface ListGroupItem {
  kind: 'group';
  key: string;
  group: RowGroup;
}

export interface ListRowItem {
  kind: 'row';
  key: string;
  row: Row;
}

export type ListItem = ListGroupItem | ListRowItem;

/** Virtual items for the list: group headers and single rows in engine order. */
export function buildListItems(rows: Row[], groups: RowGroup[]): ListItem[] {
  if (groups.length === 0) {
    return rows.map((row) => ({ kind: 'row', key: `row:${row.key}`, row }));
  }

  const items: ListItem[] = [];
  for (const slice of groupSlices(rows, groups)) {
    items.push({ kind: 'group', key: `group:${slice.group.id}`, group: slice.group });
    if (slice.group.collapsed) continue;
    for (const row of slice.rows) {
      items.push({ kind: 'row', key: `row:${row.key}`, row });
    }
  }
  return items;
}

export function gridItemHeight(item: GridItem | undefined): number {
  if (!item) return GRID_LINE_HEIGHT;
  return item.kind === 'group' ? GRID_GROUP_HEADER_HEIGHT : GRID_LINE_HEIGHT;
}

export function listItemHeight(item: ListItem | undefined): number {
  if (!item) return LIST_ROW_HEIGHT;
  return item.kind === 'group' ? LIST_GROUP_HEADER_HEIGHT : LIST_ROW_HEIGHT;
}

export type Translate = (key: string) => string;

/**
 * Group headers carry either an i18n key (`Library::Playing`, `Library::{}s`)
 * or, for a store group, the provider's display name verbatim.
 */
export function groupLabel(group: RowGroup, t: Translate): string {
  if (!group.labelKey.includes('::')) return group.labelKey;
  return formatI18n(t(group.labelKey), group.id);
}

export function groupCountLabel(count: number, t: Translate): string {
  if (count === 1) return t('Library::1 game');
  return formatI18n(t('Library::{} games'), count);
}

/** The group header glyph from `components.md`'s store icon / status / match rule. */
export function groupIcon(
  group: GroupBy,
  id: string,
): TablerIcon | undefined {
  switch (group) {
    case 'store':
      return providerIcon(id);
    case 'gameNodeStatus':
      return IconBookmark;
    case 'matchState':
      return IconLink;
    case 'installation':
      return IconDeviceDesktop;
    case 'releaseDecade':
      return IconCalendar;
    default:
      return undefined;
  }
}
