import React, { useRef, useEffect } from 'react';
import { useVirtualizer, observeElementRect } from '@tanstack/react-virtual';
import { commands, useLibraryView, type Row } from '../../api';
import { openContextMenu } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { CoverImage } from './CoverImage';
import { formatDuration, formatMatchStatus } from './formatters';

export interface GameListProps {
  className?: string;
  style?: React.CSSProperties;
}

export const GameList: React.FC<GameListProps> = ({ className, style }) => {
  const library = useLibraryView();
  const { t } = useI18n();
  const parentRef = useRef<HTMLDivElement>(null);

  const rows: Row[] = library?.rows ?? [];
  const selectedKeys = new Set(library?.selected ?? []);
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

  // Scroll to focused item when focus changes
  useEffect(() => {
    if (!focusedKey) return;
    const idx = rows.findIndex((r) => r.key === focusedKey);
    if (idx >= 0) {
      virtualizer.scrollToIndex(idx, { align: 'auto' });
    }
  }, [focusedKey, rows, virtualizer]);

  const handleRowClick = (e: React.MouseEvent, row: Row) => {
    if (e.button !== 0) return;
    if (e.shiftKey) {
      commands.libraryInput({ select: { key: row.key, mode: 'range' } });
    } else if (e.ctrlKey || e.metaKey) {
      commands.libraryInput({ select: { key: row.key, mode: 'toggle' } });
    } else {
      commands.libraryInput({ select: { key: row.key, mode: 'replace' } });
    }
  };

  const handleRowContextMenu = (e: React.MouseEvent, row: Row) => {
    e.preventDefault();
    if (!selectedKeys.has(row.key)) {
      commands.libraryInput({ select: { key: row.key, mode: 'replace' } });
    }
    openContextMenu(e.clientX, e.clientY, row.key);
  };

  const handleRowDoubleClick = (_e: React.MouseEvent, _row: Row) => {
    commands.activateEntry();
  };

  const getMatchDotColor = (status: Row['status']) => {
    if (typeof status === 'object' && 'matched' in status) {
      return 'var(--color-success, #3DD68C)';
    }
    if (status === 'ambiguous') {
      return 'var(--color-warning, #F5A524)';
    }
    return 'var(--color-text-dimmed, #808080)';
  };

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

            const isSelected = selectedKeys.has(row.key);
            const isFocused = focusedKey === row.key;
            const dotColor = getMatchDotColor(row.status);
            const matchText = formatMatchStatus(row.status, t);
            const playtimeStr = formatDuration(row.playtime?.seconds, t);

            return (
              <div
                key={row.key}
                data-index={virtualRow.index}
                data-testid={`game-row-${row.key}`}
                role="listitem"
                aria-label={row.title}
                aria-selected={isSelected}
                onClick={(e) => handleRowClick(e, row)}
                onContextMenu={(e) => handleRowContextMenu(e, row)}
                onDoubleClick={(e) => handleRowDoubleClick(e, row)}
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${virtualRow.start}px)`,
                  display: 'flex',
                  alignItems: 'center',
                  padding: '0 16px',
                  boxSizing: 'border-box',
                  borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
                  backgroundColor: isSelected
                    ? 'var(--color-accent-subtle, #F150251F)'
                    : 'transparent',
                  outline: isFocused
                    ? '2px solid var(--color-text-default, #D9D9D9)'
                    : 'none',
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
          })}
        </div>
      </div>
    </div>
  );
};
