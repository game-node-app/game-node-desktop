import React, { memo, useEffect, useRef, useState } from 'react';
import { useVirtualizer, observeElementRect } from '@tanstack/react-virtual';
import { IconDeviceDesktop, IconStarFilled, IconUsersGroup } from '@tabler/icons-react';
import { commands, useLibraryView, type Row } from '../../api';
import { useI18n } from '../../i18n';
import { CoverImage } from './CoverImage';
import { formatDuration, formatMatchStatus, getMatchDotColor } from './formatters';
import { useLibrarySelection, type RowHandlers } from './useRowHandlers';

export interface GameGridProps {
  className?: string;
  style?: React.CSSProperties;
}

const TILE_WIDTH = 166;
const TILE_HEIGHT = 294;
const GAP = 12;
const ROW_PADDING_Y = 6;
const ROW_HEIGHT = TILE_HEIGHT + GAP; // 294 + 12 gap
const INITIAL_WIDTH = 800;
const SET_COLUMNS_DEBOUNCE_MS = 150;
const EMPTY_ROWS: Row[] = [];

function columnsForWidth(width: number): number {
  return Math.max(1, Math.floor((width - GAP) / (TILE_WIDTH + GAP)));
}

interface GameTileProps extends RowHandlers {
  row: Row;
  isSelected: boolean;
  isFocused: boolean;
  showInstalled: boolean;
  showFamily: boolean;
  x: number;
  y: number;
}

const GameTile = memo(function GameTile({
  row,
  isSelected,
  isFocused,
  showInstalled,
  showFamily,
  x,
  y,
  onClick,
  onContextMenu,
  onDoubleClick,
}: GameTileProps) {
  const { t } = useI18n();
  const dotColor = getMatchDotColor(row.status);
  const playtimeStr = formatDuration(row.playtime?.seconds, t);

  const selectionClass = isFocused
    ? 'border-2 border-text-default'
    : isSelected
      ? 'border border-brand'
      : 'border border-transparent';

  return (
    <div
      data-testid={`game-tile-${row.key}`}
      role="listitem"
      aria-label={row.title}
      aria-selected={isSelected}
      onClick={(e) => onClick(e, row.key)}
      onContextMenu={(e) => onContextMenu(e, row.key)}
      onDoubleClick={(e) => onDoubleClick(e, row.key)}
      className={`absolute left-0 top-0 box-border flex h-[294px] w-[166px] cursor-pointer select-none flex-col rounded-md p-2 transition-colors ${selectionClass} ${
        isSelected ? 'bg-accent-subtle' : 'bg-transparent'
      }`}
      style={{ transform: `translate(${x}px, ${y}px)` }}
    >
      {/* Cover (150x200px) */}
      <div className="h-[200px] w-[150px] shrink-0 overflow-hidden rounded-sm">
        <CoverImage
          coverFile={row.coverFile}
          title={row.title}
          detectedName={row.detectedName}
        />
      </div>

      {/* Title + Running */}
      <div
        title={row.title}
        className="mt-1.5 line-clamp-2 h-[34px] overflow-hidden text-sm font-medium leading-[17px] text-text-default"
      >
        {row.title}
        {row.running && (
          <span className="ml-1 font-semibold text-success">
            · {t('Library::Running')}
          </span>
        )}
      </div>

      {/* Provider + Access + Match Status */}
      <div className="mt-0.5 flex h-4 items-center gap-1.5">
        <span
          title={formatMatchStatus(row.status, t)}
          className="inline-block h-2 w-2 shrink-0 rounded-full"
          style={{ backgroundColor: dotColor }}
        />
        <span className="truncate text-xs text-text-dimmed">
          {row.providerName}
        </span>
        <span className="ml-auto flex shrink-0 items-center gap-1 text-text-dimmed">
          {row.isFavorite && (
            <span
              data-testid={`game-tile-favorite-${row.key}`}
              role="img"
              aria-label={t('Library::Favorite')}
              title={t('Library::Favorite')}
              className="text-warning"
            >
              <IconStarFilled size={12} aria-hidden />
            </span>
          )}
          {showInstalled && row.installed && (
            <span
              data-testid={`game-tile-installed-${row.key}`}
              role="img"
              aria-label={t('Library::Installed on this PC')}
              title={t('Library::Installed on this PC')}
            >
              <IconDeviceDesktop size={12} stroke={1.75} aria-hidden />
            </span>
          )}
          {showFamily && row.familyShared && !row.owned && (
            <span
              data-testid={`game-tile-family-${row.key}`}
              role="img"
              aria-label={t('Library::Shared through Steam Family')}
              title={t('Library::Shared through Steam Family')}
            >
              <IconUsersGroup size={12} stroke={1.75} aria-hidden />
            </span>
          )}
        </span>
      </div>

      {/* Playtime */}
      <div className="mt-1 truncate text-xs text-text-dimmed">
        {playtimeStr}
      </div>
    </div>
  );
});

export const GameGrid: React.FC<GameGridProps> = ({ className, style }) => {
  const library = useLibraryView();
  const parentRef = useRef<HTMLDivElement>(null);

  const rows: Row[] = library?.rows ?? EMPTY_ROWS;
  const { selectedKeys, handlers } = useLibrarySelection();
  const focusedKey = library?.focused;
  const filters = library?.layout?.filters;
  const showInstalled = !(filters?.availability.installed ?? false);
  const showFamily = !(filters?.availability.family ?? false);

  // Track the column count (not the raw width): the ResizeObserver fires every
  // frame while resizing, but React bails out when the count is unchanged.
  const [columns, setColumns] = useState<number>(() => columnsForWidth(INITIAL_WIDTH));

  useEffect(() => {
    const el = parentRef.current;
    if (!el) return;

    if (el.clientWidth > 0) {
      setColumns(columnsForWidth(el.clientWidth));
    }

    if (typeof ResizeObserver === 'undefined') return;

    let frame = 0;
    let pendingWidth = 0;
    const ro = new ResizeObserver((entries) => {
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
    return () => {
      ro.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }, []);

  // Notify backend when the column count changes (debounced, only on real change).
  const sentColumnsRef = useRef(columns);
  useEffect(() => {
    if (sentColumnsRef.current === columns) return;
    const timer = setTimeout(() => {
      sentColumnsRef.current = columns;
      commands.libraryInput({ setColumns: columns });
    }, SET_COLUMNS_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [columns]);

  const rowCount = Math.ceil(rows.length / columns);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    // A 4-column viewport with overscan 6 kept 63 covers decoded at rest and 112
    // mid-scroll. Two rows above and below are enough for smooth scrolling and trim
    // roughly 10-25 MB of decoded cover bitmaps (150x200 RGBA is ~120 KB each).
    overscan: 2,
    initialRect: { width: INITIAL_WIDTH, height: 600 },
    observeElementRect: (instance, cb) => {
      return observeElementRect(instance, (rect) => {
        cb({
          width: rect.width || INITIAL_WIDTH,
          height: rect.height || 600,
        });
      });
    },
  });

  // Scroll to the focused item's row only when the focused key (or column count)
  // changes, not on every snapshot, so we never fight the user's own scrolling.
  const rowsRef = useRef(rows);
  rowsRef.current = rows;
  const scrolledToRef = useRef<string | null>(null);
  useEffect(() => {
    if (!focusedKey) {
      scrolledToRef.current = null;
      return;
    }
    const token = `${focusedKey}|${columns}`;
    if (scrolledToRef.current === token) return;
    const idx = rowsRef.current.findIndex((r) => r.key === focusedKey);
    if (idx < 0) return; // rows may arrive later; retried via `rows` below
    scrolledToRef.current = token;
    virtualizer.scrollToIndex(Math.floor(idx / columns), { align: 'auto' });
  }, [focusedKey, columns, virtualizer, rows]);

  // Flatten tiles of all virtual rows under one parent, keyed by game: a tile
  // keeps its DOM node (and decoded <img>) when columns/filter/sort reflow it.
  const tiles: React.ReactNode[] = [];
  for (const virtualRow of virtualizer.getVirtualItems()) {
    const startIndex = virtualRow.index * columns;
    const rowItems = rows.slice(startIndex, startIndex + columns);
    rowItems.forEach((row, col) => {
      tiles.push(
        <GameTile
          key={row.key}
          row={row}
          isSelected={selectedKeys.has(row.key)}
          isFocused={focusedKey === row.key}
          showInstalled={showInstalled}
          showFamily={showFamily}
          x={GAP + col * (TILE_WIDTH + GAP)}
          y={virtualRow.start + ROW_PADDING_Y}
          {...handlers}
        />,
      );
    });
  }

  return (
    <div
      ref={parentRef}
      data-testid="game-grid-container"
      className={`relative h-full w-full flex-1 overflow-y-auto bg-body ${className ?? ''}`}
      style={style}
    >
      <div
        className="relative w-full"
        style={{ height: `${virtualizer.getTotalSize()}px` }}
      >
        {tiles}
      </div>
    </div>
  );
};
