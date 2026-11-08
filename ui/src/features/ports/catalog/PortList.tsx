import React, { memo, useEffect, useRef } from 'react';
import { useVirtualizer, observeElementRect } from '@tanstack/react-virtual';
import type { PortEntryView } from '../../../api/ports';
import { useI18n } from '../../../i18n';
import { PortArt } from './PortArt';
import { PortEntryBadges } from './PortBadges';
import { versionLabel } from './format';

export interface PortListProps {
  entries: PortEntryView[];
  selectedId: string | null;
  onSelect: (catalogId: string) => void;
  className?: string;
  style?: React.CSSProperties;
}

const ROW_HEIGHT = 48;
const INITIAL_WIDTH = 800;
const INITIAL_HEIGHT = 600;

interface PortListRowProps {
  entry: PortEntryView;
  index: number;
  size: number;
  start: number;
  isSelected: boolean;
  onSelect: (catalogId: string) => void;
}

const PortListRow = memo(function PortListRow({
  entry,
  index,
  size,
  start,
  isSelected,
  onSelect,
}: PortListRowProps) {
  const meta = [entry.games.join(', '), entry.consoles.join(', ')]
    .filter((part) => part.length > 0)
    .join(' · ');
  const consoles = entry.consoles.join(', ');
  const version = versionLabel(entry.version);

  return (
    <div
      data-index={index}
      data-testid={`port-row-${entry.catalogId}`}
      role="listitem"
      aria-label={entry.name}
      aria-selected={isSelected}
      onClick={() => onSelect(entry.catalogId)}
      className={`absolute left-0 top-0 flex w-full cursor-pointer select-none items-center border-b border-border-subtle px-md transition-colors ${
        isSelected ? 'bg-accent-subtle' : 'bg-transparent hover:bg-surface'
      }`}
      style={{ height: `${size}px`, transform: `translateY(${start}px)` }}
    >
      <div className="mr-2 h-[40px] w-[30px] shrink-0 overflow-hidden rounded-xs">
        {entry.art.capsule ? (
          <PortArt catalogId={entry.catalogId} title={entry.name} />
        ) : (
          <div
            data-testid="port-art-placeholder"
            className="flex h-full w-full select-none items-center justify-center overflow-hidden rounded-xs bg-surface p-0.5 text-center text-[10px] text-text-default"
          >
            <span className="line-clamp-3">{entry.name}</span>
          </div>
        )}
      </div>

      <div className="flex min-w-0 flex-[1_1_200px] items-center pr-3">
        <div className="min-w-0">
          <div
            title={entry.name}
            className="truncate text-sm font-medium text-text-default"
          >
            {entry.name}
          </div>
          <div title={meta} className="truncate text-[11px] text-text-dimmed">
            {meta}
          </div>
        </div>
      </div>

      <div
        title={consoles}
        className="w-[130px] shrink-0 truncate pr-3 text-xs text-text-dimmed"
      >
        {consoles}
      </div>

      <div
        title={version}
        className="w-[100px] shrink-0 truncate pr-3 text-xs text-text-dimmed"
      >
        {version}
      </div>

      <div className="w-[150px] shrink-0 overflow-hidden">
        <PortEntryBadges entry={entry} />
      </div>
    </div>
  );
});

/**
 * Virtualized catalog list. Same information as the grid, in the library's row shape,
 * with the badges in their own column.
 */
export const PortList: React.FC<PortListProps> = ({
  entries,
  selectedId,
  onSelect,
  className,
  style,
}) => {
  const { t } = useI18n();
  const parentRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 6,
    initialRect: { width: INITIAL_WIDTH, height: INITIAL_HEIGHT },
    observeElementRect: (instance, callback) =>
      observeElementRect(instance, (rect) => {
        callback({
          width: rect.width || INITIAL_WIDTH,
          height: rect.height || INITIAL_HEIGHT,
        });
      }),
  });

  const scrolledToRef = useRef<string | null>(null);
  useEffect(() => {
    if (!selectedId) {
      scrolledToRef.current = null;
      return;
    }
    if (scrolledToRef.current === selectedId) return;
    const index = entries.findIndex((entry) => entry.catalogId === selectedId);
    if (index < 0) return;
    scrolledToRef.current = selectedId;
    virtualizer.scrollToIndex(index, { align: 'auto' });
  }, [selectedId, entries, virtualizer]);

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (entries.length === 0) return;
    const current = entries.findIndex((entry) => entry.catalogId === selectedId);
    const base = current < 0 ? 0 : current;
    let next: number;
    switch (event.key) {
      case 'ArrowUp':
        next = base - 1;
        break;
      case 'ArrowDown':
        next = base + 1;
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
    const clamped = Math.max(0, Math.min(entries.length - 1, next));
    const entry = entries[clamped];
    if (entry) onSelect(entry.catalogId);
  };

  return (
    <div
      data-testid="ports-list-container"
      className={`flex h-full w-full flex-1 flex-col overflow-hidden bg-body ${className ?? ''}`}
      style={style}
    >
      <div
        data-testid="ports-list-header"
        className="flex h-8 shrink-0 select-none items-center border-b border-border-subtle bg-chrome px-md text-xs font-semibold text-text-dimmed"
      >
        <div className="w-9" />
        <div className="flex-[1_1_200px] pr-3">{t('Ports::Port')}</div>
        <div className="w-[130px] shrink-0 pr-3">{t('Ports::Console')}</div>
        <div className="w-[100px] shrink-0 pr-3">{t('Ports::Release')}</div>
        <div className="w-[150px] shrink-0">{t('Ports::Status')}</div>
      </div>

      <div
        ref={parentRef}
        data-testid="ports-list-scroll-area"
        role="list"
        aria-label={t('Ports::Port catalog')}
        tabIndex={0}
        onKeyDown={handleKeyDown}
        className="relative w-full flex-1 overflow-y-auto outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-brand"
      >
        <div
          className="relative w-full"
          style={{ height: `${virtualizer.getTotalSize()}px` }}
        >
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const entry = entries[virtualRow.index];
            if (!entry) return null;
            return (
              <PortListRow
                key={entry.catalogId}
                entry={entry}
                index={virtualRow.index}
                size={virtualRow.size}
                start={virtualRow.start}
                isSelected={selectedId === entry.catalogId}
                onSelect={onSelect}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
};
