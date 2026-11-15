import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { observeElementRect, useVirtualizer } from '@tanstack/react-virtual';
import {
  IconAlertTriangle,
  IconCloudOff,
  IconMoodEmpty,
  IconSearch,
  IconWorldSearch,
  IconX,
} from '@tabler/icons-react';
import {
  commands,
  createDefaultFilters,
  filtersAreDefault,
  useLibraryView,
  type CatalogResult,
} from '../../api';
import { Button, EmptyState, Skeleton } from '../../components/ui';
import { useI18n } from '../../i18n';
import { pushToast, setActiveDestination, useUiStore } from '../../store/uiStore';
import { formatI18n } from '../library/formatters';
import {
  isShortcutBlockedByOverlay,
  isTextEntryTarget,
} from '../library/libraryShortcuts';
import { CatalogContextMenu } from './CatalogContextMenu';
import { CatalogDetail } from './CatalogDetail';
import { CatalogTile } from './CatalogTile';
import {
  CATALOG_LINE_HEIGHT,
  catalogColumnsForWidth,
  catalogDisplayStatus,
  catalogFocusTarget,
  catalogLines,
  catalogSelectedResult,
  isCatalogInteractiveTarget,
} from './catalogPresentation';
import { useCatalogSearch } from './useCatalogSearch';

const INITIAL_WIDTH = 800;
const INITIAL_HEIGHT = 600;
const DETAIL_MIN_WIDTH = 260;
const DETAIL_MAX_WIDTH = 520;
const EMPTY_RESULTS: CatalogResult[] = [];

interface MenuState {
  open: boolean;
  x: number;
  y: number;
  gameId: number | null;
}

const CLOSED_MENU: MenuState = { open: false, x: 0, y: 0, gameId: null };

/**
 * The in-app GameNode catalog ("Explore GameNode", design D8).
 *
 * This component owns only the catalog content area: header, search, results
 * and its own detail slot. The shell (top bar, sidebar, status bar and the Add
 * dialog) stays in `App`. Results come from the engine's `library.catalog`;
 * membership and the local row are recomputed there, so an add flips `inLibrary`
 * without another search.
 */
export const CatalogView: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();
  const destination = useUiStore((state) => state.activeDestination);

  const libraryRef = useRef(library);
  libraryRef.current = library;
  const rootRef = useRef<HTMLDivElement>(null);
  const resultsRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const handleSearchError = useCallback(() => {
    pushToast('danger', t('Library::Failed'));
  }, [t]);
  const search = useCatalogSearch(library?.catalog, handleSearchError);

  const [selectedGameId, setSelectedGameId] = useState<number | null>(null);
  const [focusedGameId, setFocusedGameId] = useState<number | null>(null);
  const [menu, setMenu] = useState<MenuState>(CLOSED_MENU);
  const [columns, setColumns] = useState(() =>
    catalogColumnsForWidth(INITIAL_WIDTH),
  );

  const displayStatus = catalogDisplayStatus(library?.catalog, search.draft);
  const results =
    displayStatus === 'ready'
      ? (library?.catalog.results ?? EMPTY_RESULTS)
      : EMPTY_RESULTS;
  const incomplete =
    displayStatus === 'ready' && library?.catalog.incomplete === true;
  const selectedResult = catalogSelectedResult(results, selectedGameId);
  const focusedIndex = results.findIndex(
    (result) => result.gameId === focusedGameId,
  );
  const focusedResult = focusedIndex >= 0 ? results[focusedIndex] : null;
  const menuResult =
    menu.open && menu.gameId != null
      ? (results.find((result) => result.gameId === menu.gameId) ?? null)
      : null;
  const detailWidth = Math.max(
    DETAIL_MIN_WIDTH,
    Math.min(DETAIL_MAX_WIDTH, library?.layout.detailWidth ?? 360),
  );

  // A new query hides the old detail immediately, even before the engine answer
  // replaces the result list.
  useEffect(() => {
    setSelectedGameId(null);
    setMenu((current) =>
      current.open ? { ...current, open: false, gameId: null } : current,
    );
  }, [search.draft]);

  // Keep one tile focused while results exist; the first result takes focus
  // when nothing valid is focused, so Enter works as soon as an answer lands.
  useEffect(() => {
    setFocusedGameId((current) => {
      if (results.length === 0) return null;
      return results.some((result) => result.gameId === current)
        ? current
        : results[0].gameId;
    });
  }, [results]);

  // Measure the actual results width; only the column count is stored, so a
  // resize frame that keeps the same count does not re-render the grid.
  useEffect(() => {
    const element = resultsRef.current;
    if (!element) return;

    const measure = () => {
      if (element.clientWidth > 0) {
        setColumns(catalogColumnsForWidth(element.clientWidth));
      }
    };
    measure();

    window.addEventListener('resize', measure);
    let observer: ResizeObserver | undefined;
    if (typeof ResizeObserver !== 'undefined') {
      observer = new ResizeObserver(measure);
      observer.observe(element);
    }

    return () => {
      window.removeEventListener('resize', measure);
      observer?.disconnect();
    };
  }, []);

  const lines = useMemo(() => catalogLines(results, columns), [results, columns]);
  const virtualizer = useVirtualizer({
    count: lines.length,
    getScrollElement: () => resultsRef.current,
    estimateSize: () => CATALOG_LINE_HEIGHT,
    getItemKey: (index) =>
      lines[index]?.map((result) => result.gameId).join('|') ?? index,
    overscan: 2,
    initialRect: { width: INITIAL_WIDTH, height: INITIAL_HEIGHT },
    observeElementRect: (instance, callback) =>
      observeElementRect(instance, (rect) =>
        callback({
          width: rect.width || INITIAL_WIDTH,
          height: rect.height || INITIAL_HEIGHT,
        }),
      ),
  });

  // Reveal the focused tile, including when arrow keys move past the viewport.
  const focusedLine = focusedIndex >= 0 ? Math.floor(focusedIndex / columns) : -1;
  const scrolledToRef = useRef<string | null>(null);
  useEffect(() => {
    if (focusedLine < 0) {
      scrolledToRef.current = null;
      return;
    }
    const token = `${focusedGameId}|${focusedLine}|${columns}`;
    if (scrolledToRef.current === token) return;
    scrolledToRef.current = token;
    try {
      virtualizer.scrollToIndex(focusedLine, { align: 'auto' });
    } catch {
      // JSDOM has no layout; the real webview scrolls normally.
    }
  }, [focusedGameId, focusedLine, columns, virtualizer]);

  const runAction = useCallback(
    (action: () => Promise<unknown>, failure: string = t('Library::Failed')) => {
      void Promise.resolve()
        .then(action)
        .catch(() => pushToast('danger', failure));
    },
    [t],
  );

  const closeMenu = useCallback(() => setMenu(CLOSED_MENU), []);

  const openAdd = useCallback(
    (result: CatalogResult) => {
      runAction(() =>
        commands.libraryInput({
          openAddForGame: {
            gameId: result.gameId,
            title: result.title,
            coverUrl: result.coverUrl,
          },
        }),
      );
    },
    [runAction],
  );

  const openView = useCallback(
    (result: CatalogResult) => {
      runAction(
        () => commands.openGameNodePage({ game: { id: result.gameId } }),
        t('Library::The link could not be opened.'),
      );
    },
    [runAction, t],
  );

  /**
   * Switch to the library with the matched local row visible and selected.
   * Only the things that can hide the row are cleared, in order, and every
   * engine input is awaited before the selection and `openDetails`. A hidden
   * row is revealed with Show hidden rather than permanently unhidden.
   */
  const showInLibrary = useCallback(
    (result: CatalogResult) => {
      const key = result.localKey;
      if (key == null) return;

      runAction(async () => {
        const current = libraryRef.current;
        if (!current) return;

        const visible = current.rows.some((row) => row.key === key);
        if (!visible) {
          if (current.search !== '') {
            await commands.libraryInput({ setSearch: '' });
          }
          if (current.layout.scope !== 'all') {
            await commands.libraryInput({ setScope: 'all' });
          }
          const collapsed = Object.values(
            current.layout.collapsedGroups,
          ).some((ids) => ids != null && ids.length > 0);
          if (collapsed) {
            await commands.libraryInput({ setAllGroupsCollapsed: false });
          }

          // `paletteIndex` is every non-hidden row, independent of filters, so
          // a key missing there is hidden and must be revealed with Show hidden.
          const hidden = !current.paletteIndex.some((entry) => entry.key === key);
          const filters = current.layout.filters;
          if (!filtersAreDefault(filters) || (hidden && !filters.showHidden)) {
            await commands.libraryInput({
              setFilters: { ...createDefaultFilters(), showHidden: hidden },
            });
          }
        }

        await commands.libraryInput({ select: { key, mode: 'replace' } });
        await commands.libraryInput('openDetails');
        setActiveDestination('library');
      });
    },
    [runAction],
  );

  const focusSearch = useCallback(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const clearSearch = useCallback(() => {
    search.clear();
    focusSearch();
  }, [focusSearch, search]);

  const openMenuFor = useCallback(
    (result: CatalogResult, x?: number, y?: number) => {
      setFocusedGameId(result.gameId);
      const element = rootRef.current?.querySelector<HTMLElement>(
        `[data-catalog-game-id="${result.gameId}"]`,
      );
      const rect = element?.getBoundingClientRect();
      const viewportWidth =
        typeof window !== 'undefined' ? window.innerWidth : 800;
      const viewportHeight =
        typeof window !== 'undefined' ? window.innerHeight : 600;
      setMenu({
        open: true,
        x: x ?? (rect ? rect.left + rect.width / 2 : viewportWidth / 2),
        y: y ?? (rect ? rect.top + rect.height / 2 : viewportHeight / 2),
        gameId: result.gameId,
      });
    },
    [],
  );

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (destination !== 'catalog') return;
    if (event.defaultPrevented || event.nativeEvent.isComposing) return;

    const key = event.key;
    const command = event.ctrlKey || event.metaKey;
    const target = event.target as HTMLElement;

    if (menu.open || isShortcutBlockedByOverlay(library)) return;

    if (command && key.toLowerCase() === 'f' && !event.shiftKey && !event.altKey) {
      event.preventDefault();
      focusSearch();
      return;
    }

    if (key === 'Escape' && !command && !event.altKey) {
      if (selectedGameId != null) {
        event.preventDefault();
        setSelectedGameId(null);
        return;
      }
      if (search.draft !== '') {
        event.preventDefault();
        search.clear();
        return;
      }
      return;
    }

    if (isTextEntryTarget(target)) return;
    if (event.altKey) return;

    if (key === '/' && !command) {
      event.preventDefault();
      focusSearch();
      return;
    }

    // Detail buttons, the clear button and the context menu keep their native
    // Enter/Space behavior; only the tile grid handles navigation.
    if (isCatalogInteractiveTarget(target)) return;
    if (command) return;

    if (
      key === 'ArrowUp' ||
      key === 'ArrowDown' ||
      key === 'ArrowLeft' ||
      key === 'ArrowRight'
    ) {
      if (results.length === 0) return;
      event.preventDefault();
      const direction =
        key === 'ArrowUp'
          ? 'up'
          : key === 'ArrowDown'
            ? 'down'
            : key === 'ArrowLeft'
              ? 'left'
              : 'right';
      const next = catalogFocusTarget(
        focusedIndex,
        direction,
        columns,
        results.length,
      );
      if (next >= 0 && results[next]) {
        setFocusedGameId(results[next].gameId);
      }
      return;
    }

    if (key === 'Enter' && focusedResult) {
      event.preventDefault();
      setSelectedGameId(focusedResult.gameId);
      return;
    }

    if (
      focusedResult &&
      (key === 'ContextMenu' || (key === 'F10' && event.shiftKey))
    ) {
      event.preventDefault();
      openMenuFor(focusedResult);
    }
  };

  const noResults = displayStatus === 'ready' && results.length === 0;
  const searching = displayStatus === 'loading';

  return (
    <div
      ref={rootRef}
      data-testid="catalog-view"
      tabIndex={0}
      onKeyDown={handleKeyDown}
      className="relative flex h-full min-h-0 w-full overflow-hidden bg-body outline-none"
    >
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="shrink-0 px-[24px] pb-[16px] pt-[24px]">
          <h1 className="m-0 text-heading font-semibold leading-7 text-text-primary">
            {t('Catalog::Explore GameNode')}
          </h1>
          <p className="m-0 mt-[4px] text-body font-normal text-text-secondary">
            {t(
              "Catalog::Find games that aren't on this PC and add them to your GameNode library.",
            )}
          </p>
        </header>

        <div className="flex h-[34px] shrink-0 items-center gap-[8px] px-[24px]">
          <div className="flex h-[34px] w-[480px] max-w-full items-center gap-[8px] rounded-control border border-border-subtle bg-surface px-[10px] focus-within:border-brand">
            <IconSearch
              size={16}
              stroke={2}
              aria-hidden="true"
              className="shrink-0 text-text-dimmed"
            />
            <input
              ref={inputRef}
              data-library-search
              data-testid="catalog-search-input"
              type="text"
              autoFocus
              autoComplete="off"
              spellCheck={false}
              aria-label={t('Catalog::Search the GameNode catalog')}
              placeholder={t('Catalog::Search the GameNode catalog')}
              value={search.draft}
              onChange={(event) => search.setDraft(event.target.value)}
              className="min-w-0 flex-1 border-0 bg-transparent p-0 text-body text-text-default outline-none placeholder:text-text-dimmed"
            />
            {search.draft !== '' ? (
              <button
                type="button"
                data-testid="catalog-clear-search"
                aria-label={t('Library::Clear search')}
                title={t('Library::Clear search')}
                onClick={clearSearch}
                className="flex h-5 w-5 shrink-0 items-center justify-center rounded-xs text-text-dimmed transition-colors hover:bg-raised hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
              >
                <IconX size={14} stroke={2} aria-hidden="true" />
              </button>
            ) : null}
          </div>
        </div>

        <div
          ref={resultsRef}
          data-testid="catalog-results"
          aria-busy={searching || undefined}
          className="relative min-h-0 flex-1 overflow-y-auto px-[24px] pb-[24px] pt-[16px]"
        >
          {displayStatus === 'idle' ? (
            <div className="flex min-h-[420px] items-center justify-center">
              <EmptyState
                icon={IconWorldSearch}
                title={t('Catalog::Search the GameNode catalog')}
                body={t('Catalog::Type at least 2 characters')}
              />
            </div>
          ) : null}

          {searching ? (
            <>
              <span role="status" className="sr-only">
                {t('Catalog::Searching…')}
              </span>
              <div
                data-testid="catalog-loading"
                className="flex flex-wrap items-start gap-[16px]"
              >
                {Array.from({ length: 12 }, (_, index) => (
                  <div
                    key={index}
                    data-testid="catalog-skeleton"
                    aria-hidden="true"
                    className="flex h-[270px] w-[172px] flex-col gap-[8px] rounded-md p-[6px]"
                  >
                    <Skeleton className="h-[214px] w-[160px]" />
                    <Skeleton className="h-4 w-28" />
                    <Skeleton className="h-3 w-20" />
                  </div>
                ))}
              </div>
            </>
          ) : null}

          {noResults ? (
            <div className="flex min-h-[420px] items-center justify-center">
              <EmptyState
                icon={IconMoodEmpty}
                title={formatI18n(
                  t('Catalog::No results for “{}”'),
                  search.draft.trim(),
                )}
                actions={
                  <Button
                    variant="ghost"
                    size="sm"
                    data-testid="catalog-empty-clear"
                    onClick={clearSearch}
                  >
                    {t('Library::Clear search')}
                  </Button>
                }
              />
            </div>
          ) : null}

          {displayStatus === 'error' ? (
            <div className="flex min-h-[420px] items-center justify-center [&_svg]:text-warning">
              <EmptyState
                icon={IconAlertTriangle}
                title={t("Catalog::Couldn't search GameNode")}
                actions={
                  <Button
                    variant="secondary"
                    size="sm"
                    data-testid="catalog-retry"
                    onClick={search.retry}
                  >
                    {t('Catalog::Try again')}
                  </Button>
                }
              />
            </div>
          ) : null}

          {displayStatus === 'offline' ? (
            <div className="flex min-h-[420px] items-center justify-center">
              <EmptyState
                icon={IconCloudOff}
                title={t(
                  "Catalog::GameNode can't be reached. Search needs a connection.",
                )}
                actions={
                  <Button
                    variant="secondary"
                    size="sm"
                    data-testid="catalog-retry"
                    onClick={search.retry}
                  >
                    {t('Catalog::Try again')}
                  </Button>
                }
              />
            </div>
          ) : null}

          {displayStatus === 'ready' && results.length > 0 ? (
            <>
              <div
                role="list"
                aria-label={t('Catalog::Explore GameNode')}
                data-testid="catalog-grid"
                className="relative w-full"
                style={{ height: `${virtualizer.getTotalSize()}px` }}
              >
                {virtualizer.getVirtualItems().map((virtualLine) => {
                  const line = lines[virtualLine.index];
                  if (!line) return null;
                  return (
                    <div
                      key={virtualLine.key}
                      className="absolute left-0 top-0 flex items-start gap-[16px]"
                      style={{
                        transform: `translateY(${virtualLine.start}px)`,
                      }}
                    >
                      {line.map((result) => (
                        <CatalogTile
                          key={result.gameId}
                          result={result}
                          selected={selectedGameId === result.gameId}
                          focused={focusedGameId === result.gameId}
                          onSelect={(event) => {
                            if (event.button !== 0) return;
                            setFocusedGameId(result.gameId);
                            setSelectedGameId(result.gameId);
                          }}
                          onContextMenu={(event) => {
                            event.preventDefault();
                            openMenuFor(
                              result,
                              event.clientX,
                              event.clientY,
                            );
                          }}
                          onDoubleClick={() => {
                            setFocusedGameId(result.gameId);
                            setSelectedGameId(result.gameId);
                          }}
                        />
                      ))}
                    </div>
                  );
                })}
              </div>

              {incomplete ? (
                <p
                  data-testid="catalog-incomplete"
                  className="mt-4 text-center text-caption font-normal text-text-dimmed"
                >
                  {t('Catalog::Results may be incomplete. Refine the search.')}
                </p>
              ) : null}
            </>
          ) : null}
        </div>
      </div>

      {selectedResult ? (
        <CatalogDetail
          result={selectedResult}
          width={detailWidth}
          onClose={() => setSelectedGameId(null)}
          onAdd={openAdd}
          onView={openView}
          onShow={showInLibrary}
        />
      ) : null}

      <CatalogContextMenu
        result={menuResult}
        x={menu.x}
        y={menu.y}
        onClose={closeMenu}
        onAdd={(result) => openAdd(result)}
        onView={(result) => openView(result)}
        onShow={(result) => showInLibrary(result)}
      />
    </div>
  );
};
