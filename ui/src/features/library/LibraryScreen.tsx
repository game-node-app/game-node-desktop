import React, { useMemo, useRef } from 'react';
import {
  commands,
  useAuth,
  useLibraryView,
  useLocalGames,
  useSnapshot,
  createDefaultFilters,
} from '../../api';
import { openSettingsModal, setActiveDestination } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { LibraryHeader } from './LibraryHeader';
import { LibraryToolbar } from './LibraryToolbar';
import { FilterChipsRow } from './FilterChipsRow';
import { LibraryBanners } from './LibraryBanners';
import { GameGrid } from './GameGrid';
import { GameList } from './GameList';
import { DetailPane } from './DetailPane';
import { ContextMenu } from './ContextMenu';
import { BatchBar } from './BatchBar';
import { buildFilterChips } from './libraryControlLabels';
import { isShortcutBlockedByOverlay, isTextEntryTarget } from './libraryShortcuts';
import { openContextMenuForElement } from './contextMenuAnchor';
import {
  FirstRunGuide,
  NoGamesState,
  NoInstalledState,
  NoResultsState,
  ScanningPanel,
} from './states';

/**
 * Controls that own Enter, Space or printable characters. Library navigation
 * must not steal them; the group header, row buttons and detail controls keep
 * their native behavior.
 */
function isInteractiveTarget(target: HTMLElement): boolean {
  return Boolean(
    target.closest(
      'button, a, input, textarea, select, [contenteditable="true"], [role="button"], [role="menuitem"], [role="menu"], [role="dialog"], [role="alertdialog"], [role="slider"], [role="checkbox"]',
    ),
  );
}

export const LibraryScreen: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();
  const { auth } = useAuth();
  const localGames = useLocalGames();
  const storeAccounts = useSnapshot((snapshot) => snapshot.storeAccounts).data;
  const rootRef = useRef<HTMLDivElement>(null);

  const viewMode = library?.layout?.view ?? 'grid';
  const detailOpen = library?.layout?.detailOpen ?? false;
  const filters = library?.layout?.filters ?? createDefaultFilters();
  const search = library?.search ?? '';
  const rows = library?.rows ?? [];
  const empty = library?.empty;
  const scanning = library?.scanning ?? false;
  const activeFilterCount = useMemo(
    () => buildFilterChips(library, t).length,
    [library, t],
  );

  const signedIn =
    typeof auth === 'object' && auth !== null && 'signedIn' in auth;
  const username =
    signedIn && 'signedIn' in auth
      ? auth.signedIn.user.username ?? auth.signedIn.user.email ?? auth.signedIn.user.id
      : null;
  const storeConnected = storeAccounts.some(
    (account) => account.state !== 'notConnected' || account.accountId != null,
  );
  const folderAdded = localGames.watches.length > 0;

  const handleClearFilters = () => {
    commands.libraryInput({ setFilters: createDefaultFilters() });
  };

  const handleClearInstalled = () => {
    // "Show all games" leaves the Installed scope and its availability fact, so
    // owned or family-shared rows become visible again. Every other facet, the
    // search text, the sort and the grouping stay exactly as they are.
    if (library?.layout.scope !== 'all') {
      commands.libraryInput({ setScope: 'all' });
    }
    if (filters.availability.installed) {
      commands.libraryInput({
        setFilters: {
          ...filters,
          availability: { ...filters.availability, installed: false },
        },
      });
    }
  };

  const handleSearchGameNode = () => {
    setActiveDestination('catalog');
    void commands.libraryInput({ catalogSearch: search });
  };

  const focusedRow = library?.focused
    ? rows.find((row) => row.key === library.focused)
    : undefined;

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.defaultPrevented || event.nativeEvent.isComposing) return;

    const key = event.key;
    const command = event.ctrlKey || event.metaKey;
    const target = event.target as HTMLElement;

    if (key === 'Escape' && !command && !event.altKey) {
      if (isShortcutBlockedByOverlay(library)) return;
      if (isTextEntryTarget(target)) target.blur();
      commands.libraryInput('clearSelection');
      event.preventDefault();
      return;
    }

    if (isShortcutBlockedByOverlay(library)) return;
    if (isTextEntryTarget(target)) return;
    if (event.altKey) return;

    // Native buttons (group headers, row actions, toolbar, detail and batch
    // controls) keep their own Enter/Space behavior. Group headers still take
    // the arrow keys so a collapsed group can be skipped without a pointer.
    const groupHeader = Boolean(target.closest('[data-group-header]'));
    if (isInteractiveTarget(target) && !groupHeader) return;

    const move = (direction: 'up' | 'down' | 'left' | 'right' | 'home' | 'end') => {
      commands.libraryInput({ move: { direction, extend: event.shiftKey } });
      event.preventDefault();
    };

    if (groupHeader) {
      if (!command) {
        if (key === 'ArrowUp') return move('up');
        if (key === 'ArrowDown') return move('down');
        if (key === 'ArrowLeft') return move('left');
        if (key === 'ArrowRight') return move('right');
        if (key === 'Home') return move('home');
        if (key === 'End') return move('end');
      }
      return;
    }

    if (!command) {
      if (key === 'ArrowUp') return move('up');
      if (key === 'ArrowDown') return move('down');
      if (key === 'ArrowLeft') return move('left');
      if (key === 'ArrowRight') return move('right');
      if (key === 'Home') return move('home');
      if (key === 'End') return move('end');

      if (key === 'Enter' && library?.focused) {
        commands.activateEntry();
        event.preventDefault();
        return;
      }

      if ((key === 'f' || key === 'F') && !event.shiftKey && focusedRow) {
        if (event.repeat) return;
        commands.libraryInput({
          setFavorite: {
            key: focusedRow.key,
            favorite: !focusedRow.isFavorite,
            now: Math.floor(Date.now() / 1000),
          },
        });
        event.preventDefault();
        return;
      }

      if ((key === 's' || key === 'S') && event.shiftKey && library) {
        if (event.repeat) return;
        const sort = library.layout.sort;
        commands.libraryInput({
          setSort: {
            key: sort.key,
            direction:
              sort.direction === 'descending' ? 'ascending' : 'descending',
          },
        });
        event.preventDefault();
        return;
      }

      if (key === 'ContextMenu' || (key === 'F10' && event.shiftKey)) {
        if (!library?.focused) return;
        const elements = rootRef.current?.querySelectorAll<HTMLElement>(
          '[data-row-key]',
        );
        const rowElement = elements
          ? Array.from(elements).find(
              (element) => element.dataset.rowKey === library.focused,
            )
          : undefined;
        openContextMenuForElement(
          rowElement ?? rootRef.current ?? null,
          library.focused,
        );
        event.preventDefault();
        return;
      }

      return;
    }

    // Ctrl/Cmd combinations. Ctrl+F, Ctrl+Shift+F, Ctrl+L, Ctrl+I, Ctrl+K and
    // Ctrl+, belong to `useGlobalShortcuts`; only Select all is library-only.
    if (key === 'a' || key === 'A') {
      if (event.shiftKey) return;
      commands.libraryInput('selectAll');
      event.preventDefault();
      return;
    }

    if (key === 'Enter' && library?.focused) {
      commands.libraryInput('openDetails');
      event.preventDefault();
    }
  };

  let content: React.ReactNode;
  if (empty) {
    if (typeof empty === 'object' && 'noGames' in empty) {
      content =
        !storeConnected && !folderAdded ? (
          <FirstRunGuide
            signedIn={signedIn}
            username={username}
            storeConnected={storeConnected}
            folderAdded={folderAdded}
            onConnect={() => {
              // Settings > Stores owns the store connection flow.
              openSettingsModal('stores');
            }}
            onChooseFolder={() => void commands.pickLocalWatchFolder()}
            onRescan={() => void commands.rescanLibrary()}
          />
        ) : (
          <NoGamesState
            stores={empty.noGames.stores}
            onRescan={() => void commands.rescanLibrary()}
          />
        );
    } else if (typeof empty === 'object' && 'noInstalled' in empty) {
      content = <NoInstalledState onShowAll={handleClearInstalled} />;
    } else {
      content = (
        <NoResultsState
          search={search}
          activeFilterCount={activeFilterCount}
          onClearFilters={handleClearFilters}
          onSearchGameNode={handleSearchGameNode}
        />
      );
    }
  } else {
    content = (
      <div className="flex h-full min-h-0 min-w-0 flex-1 overflow-hidden">
        {viewMode === 'grid' ? <GameGrid /> : <GameList />}
      </div>
    );
  }

  const showScanSkeletons = scanning && rows.length === 0;
  // The pane is a full-height sibling of the whole left column, so it starts
  // below the top bar instead of below the header, banners, toolbar and chips.
  const showDetailPane = detailOpen && !empty;

  return (
    <div
      ref={rootRef}
      data-testid="library-screen"
      tabIndex={0}
      onKeyDown={handleKeyDown}
      className="relative flex h-full min-h-0 w-full overflow-hidden bg-body outline-none"
    >
      <div
        data-testid="library-content-column"
        className="relative flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
      >
        <LibraryHeader />
        <LibraryBanners
          access={library?.access ?? 'unverified'}
          storeAccounts={storeAccounts}
          recentlyAdded={localGames.recentlyAdded}
          providers={library?.providers}
        />
        <LibraryToolbar />
        <FilterChipsRow />

        {scanning && (
          <ScanningPanel
            progress={library?.scanProgress ?? []}
            showSkeletons={showScanSkeletons}
          />
        )}

        {!showScanSkeletons && content}

        {/* One instance, anchored to the content column it acts on. */}
        <BatchBar />
      </div>

      {showDetailPane && <DetailPane />}

      <ContextMenu />
    </div>
  );
};
