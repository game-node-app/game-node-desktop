import React, { useRef, useState, useEffect } from 'react';
import { useVirtualizer, observeElementRect } from '@tanstack/react-virtual';
import { commands, useLibraryView, type Row } from '../../api';
import { openContextMenu } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { CoverImage } from './CoverImage';
import { formatDuration, formatMatchStatus } from './formatters';

export interface GameGridProps {
  className?: string;
  style?: React.CSSProperties;
}

export const GameGrid: React.FC<GameGridProps> = ({ className, style }) => {
  const library = useLibraryView();
  const { t } = useI18n();
  const parentRef = useRef<HTMLDivElement>(null);

  const rows: Row[] = library?.rows ?? [];
  const selectedKeys = new Set(library?.selected ?? []);
  const focusedKey = library?.focused;

  // ResizeObserver to detect container width, fallback to 800 for tests / initial
  const [containerWidth, setContainerWidth] = useState<number>(() => {
    if (typeof window !== 'undefined' && parentRef.current?.clientWidth) {
      return parentRef.current.clientWidth;
    }
    return 800;
  });

  useEffect(() => {
    const el = parentRef.current;
    if (!el) return;

    if (el.clientWidth > 0) {
      setContainerWidth(el.clientWidth);
    }

    if (typeof ResizeObserver === 'undefined') return;

    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const width = entry.contentRect.width || entry.target.clientWidth;
        if (width > 0) {
          setContainerWidth(width);
        }
      }
    });

    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const columns = Math.max(1, Math.floor((containerWidth - 12) / (166 + 12)));

  // Notify backend when columns change
  const prevColumnsRef = useRef(columns);
  useEffect(() => {
    if (prevColumnsRef.current !== columns) {
      prevColumnsRef.current = columns;
      commands.libraryInput({ setColumns: columns });
    }
  }, [columns]);

  const rowCount = Math.ceil(rows.length / columns);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 306, // 294 + 12 gap
    overscan: 3,
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

  // Scroll to focused item's row when focus changes
  useEffect(() => {
    if (!focusedKey) return;
    const idx = rows.findIndex((r) => r.key === focusedKey);
    if (idx >= 0) {
      const rowIndex = Math.floor(idx / columns);
      virtualizer.scrollToIndex(rowIndex, { align: 'auto' });
    }
  }, [focusedKey, columns, rows, virtualizer]);

  const handleTileClick = (e: React.MouseEvent, row: Row) => {
    if (e.button !== 0) return;
    if (e.shiftKey) {
      commands.libraryInput({ select: { key: row.key, mode: 'range' } });
    } else if (e.ctrlKey || e.metaKey) {
      commands.libraryInput({ select: { key: row.key, mode: 'toggle' } });
    } else {
      commands.libraryInput({ select: { key: row.key, mode: 'replace' } });
    }
  };

  const handleTileContextMenu = (e: React.MouseEvent, row: Row) => {
    e.preventDefault();
    if (!selectedKeys.has(row.key)) {
      commands.libraryInput({ select: { key: row.key, mode: 'replace' } });
    }
    openContextMenu(e.clientX, e.clientY, row.key);
  };

  const handleTileDoubleClick = (_e: React.MouseEvent, _row: Row) => {
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
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const startIndex = virtualRow.index * columns;
          const rowItems = rows.slice(startIndex, startIndex + columns);

          return (
            <div
              key={virtualRow.key}
              data-index={virtualRow.index}
              data-testid={`grid-virtual-row-${virtualRow.index}`}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                height: `${virtualRow.size}px`,
                transform: `translateY(${virtualRow.start}px)`,
                display: 'flex',
                gap: '12px',
                padding: '6px 12px',
                boxSizing: 'border-box',
              }}
            >
              {rowItems.map((row) => {
                const isSelected = selectedKeys.has(row.key);
                const isFocused = focusedKey === row.key;
                const dotColor = getMatchDotColor(row.status);
                const playtimeStr = formatDuration(row.playtime?.seconds, t);

                return (
                  <div
                    key={row.key}
                    data-testid={`game-tile-${row.key}`}
                    role="listitem"
                    aria-label={row.title}
                    aria-selected={isSelected}
                    onClick={(e) => handleTileClick(e, row)}
                    onContextMenu={(e) => handleTileContextMenu(e, row)}
                    onDoubleClick={(e) => handleTileDoubleClick(e, row)}
                    style={{
                      width: '166px',
                      height: '294px',
                      borderRadius: 'var(--radius-md, 8px)',
                      border: isFocused
                        ? '2px solid var(--color-text-default, #D9D9D9)'
                        : isSelected
                          ? '1px solid var(--color-primary, #F15025)'
                          : '1px solid transparent',
                      backgroundColor: isSelected
                        ? 'var(--color-accent-subtle, #F150251F)'
                        : 'transparent',
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
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
};
