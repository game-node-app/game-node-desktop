import React, { memo, useEffect, useRef } from 'react';
import { useVirtualizer, observeElementRect } from '@tanstack/react-virtual';
import { useLibraryView, type Row } from '../../api';
import { useI18n } from '../../i18n';
import { CoverImage } from './CoverImage';
import { formatDuration, formatMatchStatus, getMatchDotColor } from './formatters';
import { useLibrarySelection, type RowHandlers } from './useRowHandlers';

export interface GameListProps {
  className?: string;
  style?: React.CSSProperties;
}

const EMPTY_ROWS: Row[] = [];

interface GameListRowProps extends RowHandlers {
  row: Row;
  index: number;
  size: number;
  start: number;
  isSelected: boolean;
  isFocused: boolean;
}

const GameListRow = memo(function GameListRow({
  row,
  index,
  size,
  start,
  isSelected,
  isFocused,
  onClick,
  onContextMenu,
  onDoubleClick,
}: GameListRowProps) {
  const { t } = useI18n();
  const dotColor = getMatchDotColor(row.status);
  const matchText = formatMatchStatus(row.status, t);
  const playtimeStr = formatDuration(row.playtime?.seconds, t);

  return (
    <div
      data-index={index}
      data-testid={`game-row-${row.key}`}
      role="listitem"
      aria-label={row.title}
      aria-selected={isSelected}
      onClick={(e) => onClick(e, row.key)}
      onContextMenu={(e) => onContextMenu(e, row.key)}
      onDoubleClick={(e) => onDoubleClick(e, row.key)}
      className={`absolute left-0 top-0 flex w-full cursor-pointer select-none items-center border-b border-border-subtle px-md transition-colors ${
        isSelected ? 'bg-accent-subtle' : 'bg-transparent'
      } ${isFocused ? '-outline-offset-2 outline outline-2 outline-text-default' : 'outline-none'}`}
      style={{ height: `${size}px`, transform: `translateY(${start}px)` }}
    >
      {/* Thumbnail */}
      <div className="mr-2 h-[38px] w-[28px] shrink-0 overflow-hidden rounded-xs">
        <CoverImage
          coverFile={row.coverFile}
          title={row.title}
          detectedName={row.detectedName}
        />
      </div>

      {/* Title + Running */}
      <div className="flex flex-[1_1_200px] items-center gap-1.5 overflow-hidden pr-3">
        <span className="truncate text-sm font-medium text-text-default">
          {row.title}
        </span>
        {row.running && (
          <span className="shrink-0 rounded-xs bg-success-subtle px-1.5 py-px text-[11px] font-semibold text-success">
            {t('Library::Running')}
          </span>
        )}
      </div>

      {/* Store / Provider */}
      <div className="w-[130px] truncate pr-3 text-xs text-text-dimmed">
        {row.providerName}
      </div>

      {/* Match */}
      <div className="flex w-[170px] items-center gap-1.5 overflow-hidden pr-3">
        <span
          className="h-2 w-2 shrink-0 rounded-full"
          style={{ backgroundColor: dotColor }}
        />
        <span className="truncate text-xs text-text-dimmed">
          {matchText}
        </span>
      </div>

      {/* Playtime */}
      <div className="w-[110px] truncate text-right text-xs text-text-dimmed">
        {playtimeStr}
      </div>
    </div>
  );
});

export const GameList: React.FC<GameListProps> = ({ className, style }) => {
  const library = useLibraryView();
  const { t } = useI18n();
  const parentRef = useRef<HTMLDivElement>(null);

  const rows: Row[] = library?.rows ?? EMPTY_ROWS;
  const { selectedKeys, handlers } = useLibrarySelection();
  const focusedKey = library?.focused;

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 48,
    overscan: 6,
    initialRect: { width: 800, height: 600 },
    observeElementRect: (instance, cb) => {
      return observeElementRect(instance, (rect) => {
        cb({
          width: rect.width || 800,
          height: rect.height || 600,
        });
      });
    },
  });

  // Scroll to the focused item only when the focused key changes, not on every
  // snapshot, so we never fight the user's own scrolling.
  const scrolledToRef = useRef<string | null>(null);
  useEffect(() => {
    if (!focusedKey) {
      scrolledToRef.current = null;
      return;
    }
    if (scrolledToRef.current === focusedKey) return;
    const idx = rows.findIndex((r) => r.key === focusedKey);
    if (idx < 0) return; // rows may arrive later; retried when `rows` changes
    scrolledToRef.current = focusedKey;
    virtualizer.scrollToIndex(idx, { align: 'auto' });
  }, [focusedKey, rows, virtualizer]);

  return (
    <div
      data-testid="game-list-container"
      className={`flex h-full w-full flex-1 flex-col overflow-hidden bg-body ${className ?? ''}`}
      style={style}
    >
      {/* Sticky Header */}
      <div
        data-testid="game-list-header"
        className="flex h-8 shrink-0 select-none items-center border-b border-border-subtle bg-chrome px-md text-xs font-semibold text-text-dimmed"
      >
        <div className="w-9" />
        <div className="flex-[1_1_200px] pr-3">{t('Library::Title')}</div>
        <div className="w-[130px] pr-3">{t('Library::Store')}</div>
        <div className="w-[170px] pr-3">{t('Library::Match')}</div>
        <div className="w-[110px] text-right">{t('Library::Playtime')}</div>
      </div>

      {/* Virtual Scroll Area */}
      <div
        ref={parentRef}
        data-testid="game-list-scroll-area"
        className="relative w-full flex-1 overflow-y-auto"
      >
        <div
          className="relative w-full"
          style={{ height: `${virtualizer.getTotalSize()}px` }}
        >
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const row = rows[virtualRow.index];
            if (!row) return null;
            return (
              <GameListRow
                key={row.key}
                row={row}
                index={virtualRow.index}
                size={virtualRow.size}
                start={virtualRow.start}
                isSelected={selectedKeys.has(row.key)}
                isFocused={focusedKey === row.key}
                {...handlers}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
};
