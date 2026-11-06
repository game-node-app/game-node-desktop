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
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        width: '100%',
        height: `${size}px`,
        transform: `translateY(${start}px)`,
        display: 'flex',
        alignItems: 'center',
        padding: '0 16px',
        boxSizing: 'border-box',
        borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
        backgroundColor: isSelected ? 'var(--color-accent-subtle, #F150251F)' : 'transparent',
        outline: isFocused ? '2px solid var(--color-text-default, #D9D9D9)' : 'none',
        outlineOffset: '-2px',
        cursor: 'pointer',
        userSelect: 'none',
        transition: 'background-color 120ms ease',
      }}
    >
      {/* Thumbnail */}
      <div
        style={{
          width: '28px',
          height: '38px',
          borderRadius: 'var(--radius-xs, 2px)',
          overflow: 'hidden',
          flexShrink: 0,
          marginRight: '8px',
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
        style={{
          flex: '1 1 200px',
          display: 'flex',
          alignItems: 'center',
          gap: '6px',
          paddingRight: '12px',
          overflow: 'hidden',
        }}
      >
        <span
          style={{
            color: 'var(--color-text-default, #D9D9D9)',
            fontWeight: 500,
            fontSize: 'var(--font-size-sm, 14px)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {row.title}
        </span>
        {row.running && (
          <span
            style={{
              backgroundColor: 'var(--color-success-subtle, #3DD68C1F)',
              color: 'var(--color-success, #3DD68C)',
              fontSize: '11px',
              padding: '1px 6px',
              borderRadius: 'var(--radius-xs, 2px)',
              fontWeight: 600,
              flexShrink: 0,
            }}
          >
            {t('Library::Running')}
          </span>
        )}
      </div>

      {/* Store / Provider */}
      <div
        style={{
          width: '130px',
          paddingRight: '12px',
          color: 'var(--color-text-dimmed, #808080)',
          fontSize: 'var(--font-size-xs, 12px)',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {row.providerName}
      </div>

      {/* Match */}
      <div
        style={{
          width: '170px',
          paddingRight: '12px',
          display: 'flex',
          alignItems: 'center',
          gap: '6px',
          overflow: 'hidden',
        }}
      >
        <span
          style={{
            width: '8px',
            height: '8px',
            borderRadius: '50%',
            backgroundColor: dotColor,
            flexShrink: 0,
          }}
        />
        <span
          style={{
            color: 'var(--color-text-dimmed, #808080)',
            fontSize: 'var(--font-size-xs, 12px)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {matchText}
        </span>
      </div>

      {/* Playtime */}
      <div
        style={{
          width: '110px',
          textAlign: 'right',
          color: 'var(--color-text-dimmed, #808080)',
          fontSize: 'var(--font-size-xs, 12px)',
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
      className={className}
      style={{
        flex: 1,
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        backgroundColor: 'var(--color-background, #161616)',
        overflow: 'hidden',
        ...style,
      }}
    >
      {/* Sticky Header */}
      <div
        data-testid="game-list-header"
        style={{
          display: 'flex',
          alignItems: 'center',
          height: '32px',
          padding: '0 16px',
          backgroundColor: 'var(--color-chrome, #111111)',
          borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
          fontSize: 'var(--font-size-xs, 12px)',
          color: 'var(--color-text-dimmed, #808080)',
          fontWeight: 600,
          flexShrink: 0,
          userSelect: 'none',
        }}
      >
        <div style={{ width: '36px' }} />
        <div style={{ flex: '1 1 200px', paddingRight: '12px' }}>{t('Library::Title')}</div>
        <div style={{ width: '130px', paddingRight: '12px' }}>{t('Library::Store')}</div>
        <div style={{ width: '170px', paddingRight: '12px' }}>{t('Library::Match')}</div>
        <div style={{ width: '110px', textAlign: 'right' }}>{t('Library::Playtime')}</div>
      </div>

      {/* Virtual Scroll Area */}
      <div
        ref={parentRef}
        data-testid="game-list-scroll-area"
        style={{
          flex: 1,
          width: '100%',
          overflowY: 'auto',
          position: 'relative',
        }}
      >
        <div
          style={{
            height: `${virtualizer.getTotalSize()}px`,
            width: '100%',
            position: 'relative',
          }}
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
