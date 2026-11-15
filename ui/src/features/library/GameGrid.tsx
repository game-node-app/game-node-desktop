import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { observeElementRect, useVirtualizer } from '@tanstack/react-virtual';
import { commands, useLibraryView, type Row, type RowGroup } from '../../api';
import {
  GameTile,
  type GameTilePrimaryAction,
} from '../../components/ui/GameTile';
import { GroupHeader } from '../../components/ui/GroupHeader';
import {
  formatSessionTime,
  sessionElapsedSeconds,
  useNowSeconds,
} from '../../hooks/useSessionClock';
import { useI18n } from '../../i18n';
import { providerIcon } from './libraryControlIcons';
import {
  GRID_GAP,
  GRID_TILE_WIDTH,
  buildGridItems,
  columnsForWidth,
  gridItemHeight,
  groupCountLabel,
  groupIcon,
  groupLabel,
} from './libraryGroups';
import { useLibrarySelection } from './useRowHandlers';

export interface GameGridProps {
  className?: string;
  style?: React.CSSProperties;
  /**
   * Explicit row to reveal even when the engine focus did not change. The
   * catalog's "Show in library" action selects the local row, which already
   * updates the focus this grid follows; pass the key here when a caller needs
   * an explicit scroll request. Group headers expose `data-group-header` and
   * rows expose `data-row-key` for key handlers in `LibraryScreen`.
   */
  scrollToKey?: string | null;
}

const INITIAL_WIDTH = 800;
const INITIAL_HEIGHT = 600;
const SET_COLUMNS_DEBOUNCE_MS = 150;
const EMPTY_ROWS: Row[] = [];
const EMPTY_GROUPS: RowGroup[] = [];

/**
 * The tile's primary hover action. Play needs an installed launch target;
 * Install is offered for an owned or family-shared game that is not installed.
 * A running game gets no primary action, matching the engine's `Reason::Running`.
 */
export function tilePrimaryAction(row: Row): GameTilePrimaryAction | undefined {
  if (row.running) return undefined;
  if (row.installed) return 'play';
  if (
    (row.owned || row.shared) &&
    row.provider !== 'manual' &&
    row.provider !== 'drm-free'
  ) {
    return 'install';
  }
  return undefined;
}

function revealIndex(
  parent: HTMLDivElement | null,
  scrollToIndex: (index: number, options: { align: 'auto' }) => void,
  index: number,
) {
  if (!parent) return;
  try {
    scrollToIndex(index, { align: 'auto' });
  } catch {
    // JSDOM has no Element.scrollTo; the real webview always does.
  }
}

/**
 * Virtualized cover grid on the fixed 172×270 `GameTile`. Group headers are
 * full-width virtual items and lines are chunked inside a group, so a line
 * never crosses a group boundary. The engine owns rows, group order and
 * counts; this view only renders them and sends inputs.
 */
export const GameGrid: React.FC<GameGridProps> = ({
  className,
  style,
  scrollToKey,
}) => {
  const library = useLibraryView();
  const { t } = useI18n();
  const parentRef = useRef<HTMLDivElement>(null);

  const rows: Row[] = library?.rows ?? EMPTY_ROWS;
  const groups = library?.groups ?? EMPTY_GROUPS;
  const group = library?.layout?.group ?? 'none';
  const { selectedKeys, handlers } = useLibrarySelection();
  const focusedKey = library?.focused;

  // One wall clock for the whole view: a running tile must never start its own
  // timer. Unknown session starts stay null instead of pretending to be zero.
  const nowSeconds = useNowSeconds(
    1000,
    rows.some((row) => row.running),
  );

  // Track the column count (not the raw width): ResizeObserver fires every
  // frame while resizing, but React bails out when the count is unchanged.
  const [columns, setColumns] = useState<number>(() =>
    columnsForWidth(INITIAL_WIDTH),
  );

  useEffect(() => {
    const el = parentRef.current;
    if (!el) return;

    const measure = () => {
      if (el.clientWidth > 0) setColumns(columnsForWidth(el.clientWidth));
    };
    measure();

    // Window resizes are cheap to apply directly and cover environments where
    // ResizeObserver is missing (web preview, older webviews).
    const onWindowResize = () => measure();
    window.addEventListener('resize', onWindowResize);

    let frame = 0;
    let pendingWidth = 0;
    let ro: ResizeObserver | undefined;
    if (typeof ResizeObserver !== 'undefined') {
      ro = new ResizeObserver((entries) => {
        for (const entry of entries) {
          const width = entry.contentRect.width || entry.target.clientWidth;
          if (width > 0) pendingWidth = width;
        }
        if (pendingWidth > 0 && !frame) {
          frame = requestAnimationFrame(() => {
            frame = 0;
            setColumns(columnsForWidth(pendingWidth));
          });
        }
      });
      ro.observe(el);
    }

    return () => {
      window.removeEventListener('resize', onWindowResize);
      ro?.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }, []);

  // Tell the engine the real column count, debounced. The engine starts at one
  // column, so the first measured count is always sent; later calls only happen
  // on a real change.
  const sentColumnsRef = useRef<number | null>(null);
  useEffect(() => {
    if (sentColumnsRef.current === columns) return;
    const timer = setTimeout(() => {
      sentColumnsRef.current = columns;
      commands.libraryInput({ setColumns: columns });
    }, SET_COLUMNS_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [columns]);

  const items = useMemo(
    () => buildGridItems(rows, groups, columns),
    [rows, groups, columns],
  );
  const estimateSize = useCallback(
    (index: number) => gridItemHeight(items[index]),
    [items],
  );
  const getItemKey = useCallback(
    (index: number) => items[index]?.key ?? index,
    [items],
  );

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize,
    getItemKey,
    // Two virtual items above and below keep scrolling smooth while bounding
    // mounted covers (a 160×214 decoded bitmap is ~137 KB each).
    overscan: 2,
    initialRect: { width: INITIAL_WIDTH, height: INITIAL_HEIGHT },
    observeElementRect: (instance, cb) =>
      observeElementRect(instance, (rect) =>
        cb({
          width: rect.width || INITIAL_WIDTH,
          height: rect.height || INITIAL_HEIGHT,
        }),
      ),
  });

  // Reveal the requested or engine-focused row only when its position or the
  // layout changes, so an unrelated snapshot never fights the user's scrolling.
  const itemsRef = useRef(items);
  itemsRef.current = items;
  const revealKey = scrollToKey ?? focusedKey ?? null;
  const scrolledToRef = useRef<string | null>(null);
  useEffect(() => {
    if (!revealKey) {
      scrolledToRef.current = null;
      return;
    }
    const index = itemsRef.current.findIndex(
      (item) =>
        item.kind === 'line' &&
        item.rows.some((row) => row.key === revealKey),
    );
    if (index < 0) return; // Rows may arrive later; retried when `items` changes.
    const token = `${revealKey}|${index}|${columns}|${group}`;
    if (scrolledToRef.current === token) return;
    scrolledToRef.current = token;
    revealIndex(parentRef.current, virtualizer.scrollToIndex, index);
  }, [revealKey, columns, group, virtualizer, items]);

  const tiles: React.ReactNode[] = [];
  for (const virtualItem of virtualizer.getVirtualItems()) {
    const item = items[virtualItem.index];
    if (!item) continue;
    if (item.kind === 'group') {
      const Icon = groupIcon(group, item.group.id);
      tiles.push(
        <div
          key={item.key}
          data-testid={`group-header-${item.group.id}`}
          data-group-header={item.group.id}
          className="absolute left-0 top-0 w-full"
          style={{ transform: `translateY(${virtualItem.start}px)` }}
        >
          <GroupHeader
            label={groupLabel(item.group, t)}
            count={item.group.count}
            countLabel={groupCountLabel(item.group.count, t)}
            icon={Icon}
            collapsed={item.group.collapsed}
            onToggle={() =>
              commands.libraryInput({ toggleGroup: { id: item.group.id } })
            }
          />
        </div>,
      );
      continue;
    }

    item.rows.forEach((row, column) => {
      const primaryAction = tilePrimaryAction(row);
      const elapsed = sessionElapsedSeconds(row.sessionStartedAt, nowSeconds);
      tiles.push(
        <div
          key={row.key}
          data-row-key={row.key}
          data-row-group={item.groupId ?? undefined}
          className="absolute left-0 top-0"
          style={{
            transform: `translate(${column * (GRID_TILE_WIDTH + GRID_GAP)}px, ${virtualItem.start}px)`,
          }}
        >
          <GameTile
            row={row}
            selected={selectedKeys.has(row.key)}
            focused={focusedKey === row.key}
            primaryAction={primaryAction}
            onPrimaryAction={() => {
              if (!primaryAction) return;
              // Select the tile first so the detail pane and batch bar agree
              // with the game that was launched, then launch that exact row.
              commands.libraryInput({
                select: { key: row.key, mode: 'replace' },
              });
              commands.openEntry(
                row.key,
                primaryAction === 'install' ? 'install' : 'play',
              );
            }}
            providerIcon={providerIcon(row.provider)}
            runningTime={
              row.running && elapsed != null ? formatSessionTime(elapsed) : null
            }
            onSelect={(event) => handlers.onClick(event, row.key)}
            onContextMenu={(event) => handlers.onContextMenu(event, row.key)}
            onDoubleClick={(event) => handlers.onDoubleClick(event, row.key)}
          />
        </div>,
      );
    });
  }

  return (
    <div
      ref={parentRef}
      data-testid="game-grid-container"
      className={`relative h-full w-full flex-1 overflow-y-auto bg-body px-[24px] pb-[24px] ${className ?? ''}`}
      style={style}
    >
      <div
        data-testid="game-grid-sizer"
        className="relative w-full"
        style={{ height: `${virtualizer.getTotalSize()}px` }}
      >
        {tiles}
      </div>
    </div>
  );
};
