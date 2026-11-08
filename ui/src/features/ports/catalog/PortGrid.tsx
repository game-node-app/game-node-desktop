import React, { memo, useEffect, useRef, useState } from 'react';
import { useVirtualizer, observeElementRect } from '@tanstack/react-virtual';
import type { PortEntryView } from '../../../api/ports';
import { useI18n } from '../../../i18n';
import { PortArt } from './PortArt';
import { PortEntryIcons, PortReleaseBadge } from './PortBadges';
import { versionLabel } from './format';

export interface PortGridProps {
  entries: PortEntryView[];
  selectedId: string | null;
  onSelect: (catalogId: string) => void;
  className?: string;
  style?: React.CSSProperties;
}

const TILE_WIDTH = 166;
const TILE_HEIGHT = 294;
const GAP = 12;
const ROW_PADDING_Y = 6;
const ROW_HEIGHT = TILE_HEIGHT + GAP;
const INITIAL_WIDTH = 800;
const INITIAL_HEIGHT = 600;

function columnsForWidth(width: number): number {
  return Math.max(1, Math.floor((width - GAP) / (TILE_WIDTH + GAP)));
}

interface PortTileProps {
  entry: PortEntryView;
  isSelected: boolean;
  x: number;
  y: number;
  onSelect: (catalogId: string) => void;
}

const PortTile = memo(function PortTile({
  entry,
  isSelected,
  x,
  y,
  onSelect,
}: PortTileProps) {
  const meta = [entry.games.join(', '), entry.consoles.join(', ')]
    .filter((part) => part.length > 0)
    .join(' · ');
  const version = versionLabel(entry.version);

  return (
    <div
      data-testid={`port-tile-${entry.catalogId}`}
      role="listitem"
      aria-label={entry.name}
      aria-selected={isSelected}
      onClick={() => onSelect(entry.catalogId)}
      className={`absolute left-0 top-0 box-border flex h-[294px] w-[166px] cursor-pointer select-none flex-col rounded-md border p-2 transition-colors ${
        isSelected
          ? 'border-brand bg-accent-subtle'
          : 'border-transparent bg-transparent hover:border-border-subtle'
      }`}
      style={{ transform: `translate(${x}px, ${y}px)` }}
    >
      <div className="h-[200px] w-[150px] shrink-0 overflow-hidden rounded-sm">
        {entry.art.capsule ? (
          <PortArt catalogId={entry.catalogId} title={entry.name} />
        ) : (
          <div
            data-testid="port-art-placeholder"
            className="flex h-full w-full select-none items-center justify-center overflow-hidden break-words rounded-sm bg-surface p-2 text-center text-xs text-text-default"
          >
            <span className="line-clamp-3 font-medium">{entry.name}</span>
          </div>
        )}
      </div>

      <div
        title={entry.name}
        className="mt-1 line-clamp-2 h-[34px] overflow-hidden text-sm font-medium leading-[17px] text-text-default"
      >
        {entry.name}
      </div>

      <div title={meta} className="mt-0.5 truncate text-xs text-text-dimmed">
        {meta}
      </div>

      <div className="mt-1 flex h-[18px] items-center gap-1 overflow-hidden">
        <PortReleaseBadge entry={entry} />
        {version && (
          <span title={version} className="truncate text-[11px] text-text-dimmed">
            {version}
          </span>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-1">
          <PortEntryIcons entry={entry} />
        </span>
      </div>
    </div>
  );
});

/**
 * Virtualized catalog grid. The tile geometry and selection treatment are the library's
 * (`GameGrid`), with `port-art` covers and the entry badges instead of store metadata.
 */
export const PortGrid: React.FC<PortGridProps> = ({
  entries,
  selectedId,
  onSelect,
  className,
  style,
}) => {
  const { t } = useI18n();
  const parentRef = useRef<HTMLDivElement>(null);
  const [columns, setColumns] = useState<number>(() => columnsForWidth(INITIAL_WIDTH));

  useEffect(() => {
    const el = parentRef.current;
    if (!el) return undefined;

    if (el.clientWidth > 0) {
      setColumns(columnsForWidth(el.clientWidth));
    }

    if (typeof ResizeObserver === 'undefined') return undefined;

    let frame = 0;
    let pendingWidth = 0;
    const ro = new ResizeObserver((observed) => {
      for (const entry of observed) {
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

  const rowCount = Math.ceil(entries.length / columns);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 2,
    initialRect: { width: INITIAL_WIDTH, height: INITIAL_HEIGHT },
    observeElementRect: (instance, callback) =>
      observeElementRect(instance, (rect) => {
        callback({
          width: rect.width || INITIAL_WIDTH,
          height: rect.height || INITIAL_HEIGHT,
        });
      }),
  });

  // Follow the keyboard selection only when it changes (or the column count reflows),
  // never on every snapshot, so scrolling is never stolen from the user.
  const entriesRef = useRef(entries);
  entriesRef.current = entries;
  const scrolledToRef = useRef<string | null>(null);
  useEffect(() => {
    if (!selectedId) {
      scrolledToRef.current = null;
      return;
    }
    const token = `${selectedId}|${columns}`;
    if (scrolledToRef.current === token) return;
    const index = entriesRef.current.findIndex((entry) => entry.catalogId === selectedId);
    if (index < 0) return;
    scrolledToRef.current = token;
    virtualizer.scrollToIndex(Math.floor(index / columns), { align: 'auto' });
  }, [selectedId, columns, virtualizer, entries]);

  const selectAt = (index: number) => {
    const clamped = Math.max(0, Math.min(entries.length - 1, index));
    const entry = entries[clamped];
    if (entry) onSelect(entry.catalogId);
  };

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (entries.length === 0) return;
    const current = entries.findIndex((entry) => entry.catalogId === selectedId);
    const base = current < 0 ? 0 : current;
    let next: number | null = null;
    switch (event.key) {
      case 'ArrowLeft':
        next = base - 1;
        break;
      case 'ArrowRight':
        next = base + 1;
        break;
      case 'ArrowUp':
        next = current < 0 ? 0 : base - columns;
        break;
      case 'ArrowDown':
        next = current < 0 ? 0 : base + columns;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = entries.length - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    selectAt(next);
  };

  const tiles: React.ReactNode[] = [];
  for (const virtualRow of virtualizer.getVirtualItems()) {
    const startIndex = virtualRow.index * columns;
    const rowItems = entries.slice(startIndex, startIndex + columns);
    rowItems.forEach((entry, column) => {
      tiles.push(
        <PortTile
          key={entry.catalogId}
          entry={entry}
          isSelected={selectedId === entry.catalogId}
          x={GAP + column * (TILE_WIDTH + GAP)}
          y={virtualRow.start + ROW_PADDING_Y}
          onSelect={onSelect}
        />,
      );
    });
  }

  return (
    <div
      ref={parentRef}
      data-testid="ports-grid-container"
      role="list"
      aria-label={t('Ports::Port catalog')}
      tabIndex={0}
      onKeyDown={handleKeyDown}
      className={`relative h-full w-full flex-1 overflow-y-auto bg-body outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-brand ${className ?? ''}`}
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
