import React, { useCallback, useEffect, useMemo, useRef } from 'react';
import { observeElementRect, useVirtualizer } from '@tanstack/react-virtual';
import {
  IconSortAscending,
  IconSortDescending,
} from '@tabler/icons-react';
import {
  commands,
  defaultSortDirection,
  useLibraryView,
  type Row,
  type RowGroup,
  type Sort,
  type SortKey,
} from '../../api';
import { Checkbox } from '../../components/ui/Checkbox';
import { GAME_ROW_LAYOUT, GameRow } from '../../components/ui/GameRow';
import { GroupHeader } from '../../components/ui/GroupHeader';
import {
  formatSessionTime,
  sessionElapsedSeconds,
  useNowSeconds,
} from '../../hooks/useSessionClock';
import { useI18n } from '../../i18n';
import { cx } from '../../components/ui/cx';
import { providerIcon } from './libraryControlIcons';
import {
  buildListItems,
  groupCountLabel,
  groupIcon,
  groupLabel,
  listItemHeight,
} from './libraryGroups';
import { useLibrarySelection } from './useRowHandlers';

export interface GameListProps {
  className?: string;
  style?: React.CSSProperties;
  /**
   * Explicit row to reveal even when the engine focus did not change. Rows
   * expose `data-row-key` and group headers expose `data-group-header` for the
   * key handlers in `LibraryScreen`; the focused row is revealed automatically.
   */
  scrollToKey?: string | null;
}

const INITIAL_WIDTH = 800;
const INITIAL_HEIGHT = 600;
const EMPTY_ROWS: Row[] = [];
const EMPTY_GROUPS: RowGroup[] = [];

/** Sticky table header height from `raw/04-L2-list-grouped-multiselect.txt`. */
export const GAME_LIST_HEADER_HEIGHT = 34;

/**
 * `GAME_ROW_MIN_WIDTH` from `GameRow.tsx` as a Tailwind literal. The class has
 * to stay static so Tailwind emits it; `GameList.test.tsx` keeps the two in
 * sync. The header and the rows share this surface and scroll together.
 */
const LIST_SURFACE_MIN_WIDTH_CLASS = 'min-w-[974px]';

/** Column widths from `GAME_ROW_LAYOUT`, kept as static Tailwind classes. */
const COLUMN_CLASS: Record<keyof typeof GAME_ROW_LAYOUT.columns, string> = {
  checkbox: 'w-[16px]',
  cover: 'w-[30px]',
  store: 'w-[130px]',
  gameNodeStatus: 'w-[120px]',
  match: 'w-[130px]',
  playtime: 'w-[90px]',
  lastSession: 'w-[110px]',
  favorite: 'w-[28px]',
} as const;

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

interface SortHeaderProps {
  label: string;
  sortKey: SortKey;
  sort: Sort;
  className: string;
  onSort: (key: SortKey) => void;
}

/**
 * One sortable column header. The wrapper owns `aria-sort`; the button carries
 * the click target and the brand direction icon from the reference dump.
 */
function SortHeader({ label, sortKey, sort, className, onSort }: SortHeaderProps) {
  const active = sort.key === sortKey;
  const DirectionIcon =
    sort.direction === 'ascending' ? IconSortAscending : IconSortDescending;

  return (
    <div
      role="columnheader"
      aria-sort={
        active
          ? sort.direction === 'ascending'
            ? 'ascending'
            : 'descending'
          : 'none'
      }
      data-sort={active ? sort.direction : undefined}
      className={cx('flex min-w-0 shrink-0 items-center', className)}
    >
      <button
        type="button"
        data-testid={`game-list-sort-${sortKey}`}
        onClick={() => onSort(sortKey)}
        className={cx(
          'flex min-w-0 items-center gap-1 rounded-sm text-[11px] font-semibold uppercase tracking-[0.6px] transition-colors',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-2 focus-visible:ring-offset-body',
          active ? 'text-text-primary' : 'text-text-muted hover:text-text-primary',
        )}
      >
        <span className="truncate">{label}</span>
        {active ? (
          <DirectionIcon
            size={12}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-brand"
          />
        ) : null}
      </button>
    </div>
  );
}

/**
 * Virtualized library list on the 52 px `GameRow`, with the sticky 34 px header
 * from `raw/04-L2-list-grouped-multiselect.txt`. Group headers are 36 px
 * virtual items; collapsed groups mount no rows. Sorting, selection and counts
 * stay engine-owned; header clicks only send `setSort`, `selectAll` or
 * `clearSelection`.
 *
 * One scrollport owns both axes: the sticky header and the virtualized rows are
 * children of the same min-width surface, so the header columns cannot drift
 * from the row columns when the vertical scrollbar appears, and a narrow
 * window scrolls every trailing column instead of clipping it.
 */
export const GameList: React.FC<GameListProps> = ({
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
  const sort: Sort = library?.layout?.sort ?? {
    key: 'title',
    direction: 'ascending',
  };
  const gameNodeStatusAvailable =
    library?.filterCounts.gamenodeStatus.available ?? false;
  const { selectedKeys, handlers } = useLibrarySelection();
  const focusedKey = library?.focused;

  // One clock for the whole list, shared by the running flags and the relative
  // last-session labels. Unknown session starts stay null.
  const nowSeconds = useNowSeconds(
    1000,
    rows.some((row) => row.running),
  );

  const selectedVisible = rows.reduce(
    (count, row) => count + (selectedKeys.has(row.key) ? 1 : 0),
    0,
  );
  const allVisible = rows.length > 0 && selectedVisible === rows.length;
  const someVisible = selectedVisible > 0 && !allVisible;

  const items = useMemo(() => buildListItems(rows, groups), [rows, groups]);
  const estimateSize = useCallback(
    (index: number) => listItemHeight(items[index]),
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
    overscan: 6,
    initialRect: { width: INITIAL_WIDTH, height: INITIAL_HEIGHT },
    // The sticky header is the first child of the same scroll surface, so the
    // list starts one header below the scroll content origin. `scrollMargin`
    // keeps measurements and reveal offsets in content coordinates, and
    // `scrollPaddingStart` keeps a revealed row below the sticky header.
    scrollMargin: GAME_LIST_HEADER_HEIGHT,
    scrollPaddingStart: GAME_LIST_HEADER_HEIGHT,
    observeElementRect: (instance, cb) =>
      observeElementRect(instance, (rect) =>
        cb({
          width: rect.width || INITIAL_WIDTH,
          height: rect.height || INITIAL_HEIGHT,
        }),
      ),
  });

  // Reveal the requested or engine-focused row only when its position changes.
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
      (item) => item.kind === 'row' && item.row.key === revealKey,
    );
    if (index < 0) return; // Rows may arrive later; retried when `items` changes.
    const token = `${revealKey}|${index}|${group}`;
    if (scrolledToRef.current === token) return;
    scrolledToRef.current = token;
    revealIndex(parentRef.current, virtualizer.scrollToIndex, index);
  }, [revealKey, group, virtualizer, items]);

  const applySort = (key: SortKey) => {
    const direction =
      sort.key === key
        ? sort.direction === 'ascending'
          ? 'descending'
          : 'ascending'
        : defaultSortDirection(key);
    commands.libraryInput({ setSort: { key, direction } });
  };

  const toggleAll = () => {
    if (allVisible) {
      commands.libraryInput('clearSelection');
    } else {
      commands.libraryInput('selectAll');
    }
  };

  const rendered: React.ReactNode[] = [];
  for (const virtualItem of virtualizer.getVirtualItems()) {
    const item = items[virtualItem.index];
    if (!item) continue;
    if (item.kind === 'group') {
      const Icon = groupIcon(group, item.group.id);
      rendered.push(
        <div
          key={item.key}
          data-testid={`group-header-${item.group.id}`}
          data-group-header={item.group.id}
          className="absolute left-0 top-0 w-full"
          style={{ transform: `translateY(${virtualItem.start - GAME_LIST_HEADER_HEIGHT}px)` }}
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

    const { row } = item;
    const elapsed = sessionElapsedSeconds(row.sessionStartedAt, nowSeconds);
    rendered.push(
      <div
        key={item.key}
        data-row-key={row.key}
        className="absolute left-0 top-0 w-full"
        style={{ transform: `translateY(${virtualItem.start - GAME_LIST_HEADER_HEIGHT}px)` }}
      >
        <GameRow
          row={row}
          selected={selectedKeys.has(row.key)}
          focused={focusedKey === row.key}
          gameNodeStatusAvailable={gameNodeStatusAvailable}
          providerIcon={providerIcon(row.provider)}
          runningTime={
            row.running && elapsed != null ? formatSessionTime(elapsed) : null
          }
          nowSeconds={nowSeconds}
          onSelect={(event) => handlers.onClick(event, row.key)}
          onContextMenu={(event) => handlers.onContextMenu(event, row.key)}
          onDoubleClick={(event) => handlers.onDoubleClick(event, row.key)}
          onToggleSelected={() =>
            commands.libraryInput({
              select: { key: row.key, mode: 'toggle' },
            })
          }
          onToggleFavorite={() =>
            commands.libraryInput({
              setFavorite: {
                key: row.key,
                favorite: !row.isFavorite,
                now: Math.floor(Date.now() / 1000),
              },
            })
          }
        />
      </div>,
    );
  }

  return (
    <div
      data-testid="game-list-container"
      className={`flex h-full w-full min-w-0 flex-1 flex-col overflow-hidden bg-body ${className ?? ''}`}
      style={style}
    >
      <div
        ref={parentRef}
        data-testid="game-list-scroll-area"
        className="relative min-h-0 flex-1 overflow-auto"
      >
        <div
          data-testid="game-list-surface"
          className={cx('flex w-full flex-col', LIST_SURFACE_MIN_WIDTH_CLASS)}
        >
          <div
            data-testid="game-list-header"
            className="sticky top-0 z-10 flex h-[34px] w-full shrink-0 select-none items-center gap-[12px] border-b border-border-subtle bg-chrome px-[12px]"
          >
            <div className={cx('flex shrink-0 items-center justify-center', COLUMN_CLASS.checkbox)}>
              <Checkbox
                checked={allVisible}
                indeterminate={someVisible}
                aria-label={t('Library::Select all')}
                onChange={toggleAll}
              />
            </div>
            <div aria-hidden="true" className={cx('shrink-0', COLUMN_CLASS.cover)} />
            {/* Title floor mirrors GAME_ROW_TITLE_MIN_WIDTH (200 px). */}
            <SortHeader
              label={t('Library::Title')}
              sortKey="title"
              sort={sort}
              className="min-w-0 flex-1 basis-[200px]"
              onSort={applySort}
            />
            <SortHeader
              label={t('Library::Store')}
              sortKey="provider"
              sort={sort}
              className={COLUMN_CLASS.store}
              onSort={applySort}
            />
            <SortHeader
              label={t('Library::On GameNode')}
              sortKey="gameNodeStatus"
              sort={sort}
              className={COLUMN_CLASS.gameNodeStatus}
              onSort={applySort}
            />
            <SortHeader
              label={t('Library::Match')}
              sortKey="matchStatus"
              sort={sort}
              className={COLUMN_CLASS.match}
              onSort={applySort}
            />
            <SortHeader
              label={t('Library::Time')}
              sortKey="playtime"
              sort={sort}
              className={COLUMN_CLASS.playtime}
              onSort={applySort}
            />
            <SortHeader
              label={t('Library::Last session')}
              sortKey="lastPlayed"
              sort={sort}
              className={COLUMN_CLASS.lastSession}
              onSort={applySort}
            />
            <div aria-hidden="true" className={cx('shrink-0', COLUMN_CLASS.favorite)} />
          </div>

          <div
            data-testid="game-list-sizer"
            className="relative w-full"
            style={{
              // `getTotalSize()` is already relative to the list start; the
              // sticky header adds its own 34 px above the sizer.
              height: `${virtualizer.getTotalSize()}px`,
            }}
          >
            {rendered}
          </div>
        </div>
      </div>
    </div>
  );
};
