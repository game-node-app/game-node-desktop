import React, { memo, useEffect, useRef, useState } from 'react';
import { useVirtualizer, observeElementRect } from '@tanstack/react-virtual';
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
  x: number;
  y: number;
}

const GameTile = memo(function GameTile({
  row,
  isSelected,
  isFocused,
  x,
  y,
  onClick,
  onContextMenu,
  onDoubleClick,
}: GameTileProps) {
  const { t } = useI18n();
  const dotColor = getMatchDotColor(row.status);
  const playtimeStr = formatDuration(row.playtime?.seconds, t);

  return (
    <div
      data-testid={`game-tile-${row.key}`}
      role="listitem"
      aria-label={row.title}
      aria-selected={isSelected}
      onClick={(e) => onClick(e, row.key)}
      onContextMenu={(e) => onContextMenu(e, row.key)}
      onDoubleClick={(e) => onDoubleClick(e, row.key)}
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        transform: `translate(${x}px, ${y}px)`,
        width: `${TILE_WIDTH}px`,
        height: `${TILE_HEIGHT}px`,
        borderRadius: 'var(--radius-md, 8px)',
        border: isFocused
          ? '2px solid var(--color-text-default, #D9D9D9)'
          : isSelected
            ? '1px solid var(--color-primary, #F15025)'
            : '1px solid transparent',
        backgroundColor: isSelected ? 'var(--color-accent-subtle, #F150251F)' : 'transparent',
        cursor: 'pointer',
        padding: '8px',
        display: 'flex',
        flexDirection: 'column',
        boxSizing: 'border-box',
        userSelect: 'none',
        transition: 'background-color 120ms ease',
      }}
    >
      {/* Cover (150x200px) */}
      <div
        style={{
          width: '150px',
          height: '200px',
          borderRadius: 'var(--radius-sm, 4px)',
          overflow: 'hidden',
          flexShrink: 0,
        }}
      >
        <CoverImage
          coverFile={row.coverFile}
          title={row.title}
          detectedName={row.detectedName}
        />
      </div>

      {/* Title + Running */}
      <div
        title={row.title}
        style={{
          marginTop: '6px',
          fontSize: 'var(--font-size-sm, 14px)',
          fontWeight: 500,
          color: 'var(--color-text-default, #D9D9D9)',
          lineHeight: '17px',
          height: '34px',
          overflow: 'hidden',
          display: '-webkit-box',
          WebkitLineClamp: 2,
          WebkitBoxOrient: 'vertical',
        }}
      >
        {row.title}
        {row.running && (
          <span
            style={{
              color: 'var(--color-success, #3DD68C)',
              fontWeight: 600,
              marginLeft: '4px',
            }}
          >
            · {t('Library::Running')}
          </span>
        )}
      </div>

      {/* Provider + Match Status */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: '6px',
          marginTop: '2px',
          height: '16px',
        }}
      >
        <span
          title={formatMatchStatus(row.status, t)}
          style={{
            width: '8px',
            height: '8px',
            borderRadius: '50%',
            backgroundColor: dotColor,
            flexShrink: 0,
            display: 'inline-block',
          }}
        />
        <span
          style={{
            fontSize: 'var(--font-size-xs, 12px)',
            color: 'var(--color-text-dimmed, #808080)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {row.providerName}
        </span>
      </div>

      {/* Playtime */}
      <div
        style={{
          marginTop: '4px',
          fontSize: 'var(--font-size-xs, 12px)',
          color: 'var(--color-text-dimmed, #808080)',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
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
    overscan: 6,
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
      className={className}
      style={{
        flex: 1,
        width: '100%',
        height: '100%',
        overflowY: 'auto',
        position: 'relative',
        backgroundColor: 'var(--color-background, #161616)',
        ...style,
      }}
    >
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: '100%',
          position: 'relative',
        }}
      >
        {tiles}
      </div>
    </div>
  );
};
