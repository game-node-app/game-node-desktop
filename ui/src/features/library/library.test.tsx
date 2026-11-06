import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, act, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  commands,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Snapshot,
  type Row,
} from '../../api';
import { I18nProvider, useI18n } from '../../i18n';
import { Virtualizer } from '@tanstack/react-virtual';
import { uiStore, resetUiStore, openContextMenu } from '../../store/uiStore';
import {
  CoverImage,
  resetLoadedCovers,
  LibraryToolbar,
  GameGrid,
  GameList,
  DetailPane,
  ContextMenu,
  LibraryScreen,
  formatReason,
  formatStatus,
  formatDuration,
  formatI18n,
} from './index';

function createMockRows(count: number): Row[] {
  return Array.from({ length: count }, (_, i) => ({
    key: `game-${i}`,
    title: `Game Title ${i}`,
    detectedName: `Detected Game ${i}`,
    matchedName: `GameNode Game ${i}`,
    provider: 'steam',
    providerName: 'Steam',
    running: i === 0,
    status:
      i % 3 === 0
        ? { matched: { gameId: 1000 + i, source: 'automatic' } }
        : i % 3 === 1
          ? 'ambiguous'
          : 'unmatched',
    coverFile: i % 2 === 0 ? `cover-${i}.jpg` : null,
    playtime: {
      seconds: i * 3600 + i * 60 + 45,
      source: i % 2 === 0 ? 'store' : 'internal',
    },
  }));
}

function renderWithProviders(
  ui: React.ReactElement,
  snapshot: Snapshot,
  locale = 'en',
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);

  return render(
    <QueryClientProvider client={queryClient}>
      <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
    </QueryClientProvider>,
  );
}

describe('Library Feature Tests', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  describe('CoverImage', () => {
    beforeEach(() => {
      resetLoadedCovers();
    });

    it('retries after a failed cover when the cover file changes', () => {
      const { rerender } = render(
        <CoverImage coverFile="missing.jpg" title="Game" />,
      );
      fireEvent.error(screen.getByRole('img'));
      expect(screen.getByTestId('cover-placeholder')).toBeInTheDocument();

      rerender(<CoverImage coverFile="arrived.jpg" title="Game" />);
      expect(screen.queryByTestId('cover-placeholder')).not.toBeInTheDocument();
      expect(screen.getByRole('img')).toHaveAttribute(
        'src',
        'gamenode-asset://localhost/cover/arrived.jpg',
      );
    });

    it('fades in on first load and renders known covers immediately on remount', () => {
      const first = render(<CoverImage coverFile="seen.jpg" title="Seen" />);
      expect(screen.getByRole('img')).toHaveStyle({ opacity: '0' });
      fireEvent.load(screen.getByRole('img'));
      expect(screen.getByRole('img')).toHaveStyle({ opacity: '1' });
      first.unmount();

      render(<CoverImage coverFile="seen.jpg" title="Seen" />);
      expect(screen.getByRole('img')).toHaveStyle({ opacity: '1' });
    });

    it('renders img with gamenode-asset url and lazy loading when coverFile is present', () => {
      render(
        <CoverImage
          coverFile="witcher3.jpg"
          title="The Witcher 3"
          detectedName="witcher3_exe"
        />,
      );

      const img = screen.getByRole('img');
      expect(img).toBeInTheDocument();
      expect(img).toHaveAttribute(
        'src',
        'gamenode-asset://localhost/cover/witcher3.jpg',
      );
      expect(img).toHaveAttribute('alt', 'The Witcher 3');
      expect(img).toHaveAttribute('loading', 'lazy');
      expect(img).toHaveAttribute('decoding', 'async');
    });

    it('renders styled placeholder with title and detectedName when coverFile is null or on error', () => {
      const { rerender } = render(
        <CoverImage
          coverFile={null}
          title="Cyberpunk 2077"
          detectedName="cyberpunk_pkg"
        />,
      );

      const placeholder = screen.getByTestId('cover-placeholder');
      expect(placeholder).toHaveTextContent('Cyberpunk 2077');
      expect(placeholder).toHaveTextContent('cyberpunk_pkg');
      expect(screen.queryByRole('img')).not.toBeInTheDocument();

      // Test error fallback
      rerender(
        <CoverImage
          coverFile="bad-image.jpg"
          title="Error Game"
          detectedName="error_pkg"
        />,
      );
      const img = screen.getByRole('img');
      fireEvent.error(img);

      expect(screen.getByTestId('cover-placeholder')).toHaveTextContent('Error Game');
    });
  });

  describe('Virtualized GameGrid and GameList with 500-item fixture', () => {
    it('mounts only viewport rows (< 60 items in DOM) in GameGrid for 500 items', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: false,
        },
        search: '',
        access: 'verified',
        total: 500,
        providers: [{ id: 'steam', name: 'Steam', count: 500 }],
        rows: createMockRows(500),
        scanning: false,
        failedProviders: [],
        selected: ['game-0'],
        focused: 'game-0',
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      renderWithProviders(<GameGrid />, snapshot);

      const renderedTiles = screen.getAllByRole('listitem');
      // In a 800x600 viewport, only visible rows (~2-5 rows) mount, far fewer than 500
      expect(renderedTiles.length).toBeLessThan(60);
      expect(renderedTiles.length).toBeGreaterThan(0);

      // Verify asset urls on rendered covers
      const imgs = screen.getAllByRole('img');
      expect(imgs.length).toBeGreaterThan(0);
      expect(imgs[0].getAttribute('src')).toMatch(
        /^gamenode-asset:\/\/localhost\/cover\//,
      );
      expect(imgs[0]).toHaveAttribute('loading', 'lazy');
      expect(imgs[0]).toHaveAttribute('decoding', 'async');
    });

    it('mounts only viewport rows (< 60 items in DOM) in GameList for 500 items', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'list',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: false,
        },
        search: '',
        access: 'verified',
        total: 500,
        providers: [{ id: 'steam', name: 'Steam', count: 500 }],
        rows: createMockRows(500),
        scanning: false,
        failedProviders: [],
        selected: ['game-0'],
        focused: 'game-0',
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      renderWithProviders(<GameList />, snapshot);

      const renderedRows = screen.getAllByRole('listitem');
      expect(renderedRows.length).toBeLessThan(60);
      expect(renderedRows.length).toBeGreaterThan(0);

      expect(screen.getByTestId('game-list-header')).toBeInTheDocument();
    });

    it('updates column count and syncs setColumns (debounced) on container resize via ResizeObserver', async () => {
      let resizeCallback!: (entries: ResizeObserverEntry[]) => void;
      class MockResizeObserver {
        constructor(cb: (entries: ResizeObserverEntry[]) => void) {
          resizeCallback = cb;
        }
        observe() {}
        unobserve() {}
        disconnect() {}
      }
      const originalRO = window.ResizeObserver;
      window.ResizeObserver = MockResizeObserver as unknown as typeof ResizeObserver;

      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = createDefaultSnapshot();
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: false,
        },
        search: '',
        access: 'verified',
        total: 20,
        providers: [],
        rows: createMockRows(20),
        scanning: false,
        failedProviders: [],
        selected: ['game-0'],
        focused: 'game-0',
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      try {
        renderWithProviders(<GameGrid />, snapshot);

        // Container initial width is 800 -> columns = Math.floor((800 - 12) / 178) = 4
        // ResizeObserver triggers with width = 1200 -> columns = Math.floor((1200 - 12) / 178) = 6
        act(() => {
          resizeCallback([
            {
              contentRect: { width: 1200 } as DOMRectReadOnly,
              target: { clientWidth: 1200 } as Element,
            } as ResizeObserverEntry,
          ]);
        });

        // rAF-throttled + debounced: not sent synchronously, sent once settled.
        expect(libraryInputSpy).not.toHaveBeenCalledWith({ setColumns: 6 });
        await waitFor(() =>
          expect(libraryInputSpy).toHaveBeenCalledWith({ setColumns: 6 }),
        );
        expect(
          libraryInputSpy.mock.calls.filter(
            ([input]) => 'setColumns' in (input as object),
          ),
        ).toHaveLength(1);
      } finally {
        window.ResizeObserver = originalRO;
      }
    });

    it('scrolls virtualizer to focused index when focused game changes in GameGrid', () => {
      const scrollSpy = vi.spyOn(Virtualizer.prototype as any, '_startScroll');

      const snapshot = createDefaultSnapshot();
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: false,
        },
        search: '',
        access: 'verified',
        total: 50,
        providers: [],
        rows: createMockRows(50),
        scanning: false,
        failedProviders: [],
        selected: ['game-0'],
        focused: 'game-0',
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);

      const { rerender } = render(
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale="en">
            <GameGrid />
          </I18nProvider>
        </QueryClientProvider>,
      );

      // Initially scrolls to row 0
      expect(scrollSpy).toHaveBeenCalledWith(
        expect.objectContaining({ index: 0 }),
        expect.anything(),
        expect.anything(),
        'absolute',
      );

      scrollSpy.mockClear();

      // Focus game-20: in 4 columns, row index is Math.floor(20 / 4) = 5
      const updatedSnapshot = {
        ...snapshot,
        library: {
          ...snapshot.library,
          focused: 'game-20',
          selected: ['game-20'],
        },
      };
      queryClient.setQueryData(SNAPSHOT_QUERY_KEY, updatedSnapshot);

      rerender(
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale="en">
            <GameGrid />
          </I18nProvider>
        </QueryClientProvider>,
      );

      expect(scrollSpy).toHaveBeenCalledWith(
        expect.objectContaining({ index: 5 }),
        expect.anything(),
        expect.anything(),
        'absolute',
      );

      // A snapshot that only changes rows (same focus) must not re-scroll.
      scrollSpy.mockClear();
      queryClient.setQueryData(SNAPSHOT_QUERY_KEY, {
        ...updatedSnapshot,
        library: {
          ...updatedSnapshot.library,
          rows: [...updatedSnapshot.library.rows!],
        },
      });
      rerender(
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale="en">
            <GameGrid />
          </I18nProvider>
        </QueryClientProvider>,
      );
      expect(scrollSpy).not.toHaveBeenCalled();
    });

    it('handles mouse click selection and double click in GameList', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const activateSpy = vi.spyOn(commands, 'activateEntry');

      const snapshot = createDefaultSnapshot();
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'list',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: false,
        },
        search: '',
        access: 'verified',
        total: 10,
        providers: [],
        rows: createMockRows(10),
        scanning: false,
        failedProviders: [],
        selected: ['game-0'],
        focused: 'game-0',
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      renderWithProviders(<GameList />, snapshot);

      const row1 = screen.getByTestId('game-row-game-1');

      // Click -> replace
      fireEvent.click(row1, { button: 0 });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        select: { key: 'game-1', mode: 'replace' },
      });

      // Ctrl+Click -> toggle
      fireEvent.click(row1, { button: 0, ctrlKey: true });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        select: { key: 'game-1', mode: 'toggle' },
      });

      // Shift+Click -> range
      fireEvent.click(row1, { button: 0, shiftKey: true });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        select: { key: 'game-1', mode: 'range' },
      });

      // Double-click -> activateEntry
      fireEvent.doubleClick(row1);
      expect(activateSpy).toHaveBeenCalled();
    });
  });

  describe('LibraryToolbar', () => {
    it('dispatches search, sort, view mode, and rescan library inputs', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const rescanSpy = vi.spyOn(commands, 'rescanLibrary');

      const snapshot = createDefaultSnapshot();
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: false,
        },
        search: 'hades',
        access: 'offline',
        total: 10,
        providers: [],
        rows: createMockRows(10),
        scanning: true,
        failedProviders: ['epic', 'gog'],
        selected: [],
        actions: {
          selected: 0,
          play: { disabled: 'nothingSelected' },
          install: { disabled: 'nothingSelected' },
          matchGame: { disabled: 'nothingSelected' },
          addToGamenode: { disabled: 'nothingSelected' },
          changeMatch: false,
        },
      };

      renderWithProviders(<LibraryToolbar />, snapshot);

      // Verify status banners
      expect(screen.getByTestId('banner-scanning')).toBeInTheDocument();
      expect(screen.getByTestId('banner-failed-providers')).toHaveTextContent(
        'epic, gog',
      );
      expect(screen.getByTestId('banner-offline')).toBeInTheDocument();

      // Search input change
      const searchInput = screen.getByTestId('library-search-input');
      fireEvent.change(searchInput, { target: { value: 'portal' } });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSearch: 'portal' });

      // Clear search
      const clearBtn = screen.getByTestId('library-clear-search-btn');
      fireEvent.click(clearBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSearch: '' });

      // Sort key selector
      const sortSelect = screen.getByTestId('library-sort-select');
      fireEvent.change(sortSelect, { target: { value: 'provider' } });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        setSort: { key: 'provider', direction: 'ascending' },
      });

      // Sort direction toggle
      const sortDirBtn = screen.getByTestId('library-sort-direction-btn');
      fireEvent.click(sortDirBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith({
        setSort: { key: 'title', direction: 'descending' },
      });

      // View mode buttons
      const listBtn = screen.getByTestId('library-view-list-btn');
      fireEvent.click(listBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith({ setView: 'list' });

      const gridBtn = screen.getByTestId('library-view-grid-btn');
      fireEvent.click(gridBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith({ setView: 'grid' });

      const detailToggleBtn = screen.getByTestId('library-toggle-details-btn');
      fireEvent.click(detailToggleBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith({ setDetailOpen: true });

      // Rescan button
      const rescanBtn = screen.getByTestId('library-rescan-btn');
      fireEvent.click(rescanBtn);
      expect(rescanSpy).toHaveBeenCalled();
    });
  });

  describe('Keyboard Navigation, Multi-Select, and ContextMenu', () => {
    it('dispatches directional moves, Enter, Ctrl+Enter, Shift+F10, and F5', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const activateSpy = vi.spyOn(commands, 'activateEntry');
      const rescanSpy = vi.spyOn(commands, 'rescanLibrary');

      const snapshot = createDefaultSnapshot();
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: false,
        },
        search: '',
        access: 'verified',
        total: 5,
        providers: [],
        rows: createMockRows(5),
        scanning: false,
        failedProviders: [],
        selected: ['game-0'],
        focused: 'game-0',
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      renderWithProviders(<LibraryScreen />, snapshot);

      const screenContainer = screen.getByTestId('library-screen');

      // ArrowDown
      fireEvent.keyDown(screenContainer, { key: 'ArrowDown', shiftKey: false });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        move: { direction: 'down', extend: false },
      });

      // Shift+ArrowDown
      fireEvent.keyDown(screenContainer, { key: 'ArrowDown', shiftKey: true });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        move: { direction: 'down', extend: true },
      });

      // ArrowRight
      fireEvent.keyDown(screenContainer, { key: 'ArrowRight', shiftKey: false });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        move: { direction: 'right', extend: false },
      });

      // Home
      fireEvent.keyDown(screenContainer, { key: 'Home', shiftKey: false });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        move: { direction: 'home', extend: false },
      });

      // End
      fireEvent.keyDown(screenContainer, { key: 'End', shiftKey: false });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        move: { direction: 'end', extend: false },
      });

      // Enter
      fireEvent.keyDown(screenContainer, { key: 'Enter', ctrlKey: false });
      expect(activateSpy).toHaveBeenCalled();

      // Ctrl+Enter
      fireEvent.keyDown(screenContainer, { key: 'Enter', ctrlKey: true });
      expect(libraryInputSpy).toHaveBeenCalledWith('openDetails');

      // F5
      fireEvent.keyDown(screenContainer, { key: 'F5' });
      expect(rescanSpy).toHaveBeenCalled();

      // Shift+F10 opens ContextMenu
      fireEvent.keyDown(screenContainer, { key: 'F10', shiftKey: true });
      expect(uiStore.state.contextMenu.open).toBe(true);
      expect(uiStore.state.contextMenu.entryKey).toBe('game-0');
    });

    it('handles mouse click, Ctrl+Click, Shift+Click, double click, and right click', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const activateSpy = vi.spyOn(commands, 'activateEntry');

      const snapshot = createDefaultSnapshot();
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: false,
        },
        search: '',
        access: 'verified',
        total: 5,
        providers: [],
        rows: createMockRows(5),
        scanning: false,
        failedProviders: [],
        selected: ['game-0'],
        focused: 'game-0',
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      renderWithProviders(<GameGrid />, snapshot);

      const tile1 = screen.getByTestId('game-tile-game-1');

      // Plain click -> replace
      fireEvent.click(tile1, { button: 0 });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        select: { key: 'game-1', mode: 'replace' },
      });

      // Ctrl+Click -> toggle
      fireEvent.click(tile1, { button: 0, ctrlKey: true });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        select: { key: 'game-1', mode: 'toggle' },
      });

      // Shift+Click -> range
      fireEvent.click(tile1, { button: 0, shiftKey: true });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        select: { key: 'game-1', mode: 'range' },
      });

      // Double-click -> activateEntry
      fireEvent.doubleClick(tile1);
      expect(activateSpy).toHaveBeenCalled();

      // Right-click -> opens context menu
      fireEvent.contextMenu(tile1, { clientX: 120, clientY: 240 });
      expect(uiStore.state.contextMenu.open).toBe(true);
      expect(uiStore.state.contextMenu.entryKey).toBe('game-1');
    });

    it('renders ContextMenu actions for manual entry and closes on Escape', () => {
      const launchSpy = vi.spyOn(commands, 'launchSelected');
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const toolsSpy = vi.spyOn(commands, 'openTools');

      const snapshot = createDefaultSnapshot();
      const rows = createMockRows(3);
      rows[0].key = 'manual:local-game-1';
      rows[0].running = false;
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: false,
        },
        search: '',
        access: 'verified',
        total: 3,
        providers: [],
        rows,
        scanning: false,
        failedProviders: [],
        selected: ['manual:local-game-1'],
        focused: 'manual:local-game-1',
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      renderWithProviders(<ContextMenu />, snapshot);

      // Open context menu for manual entry
      act(() => {
        openContextMenu(200, 300, 'manual:local-game-1');
      });

      expect(screen.getByTestId('library-context-menu')).toBeInTheDocument();
      expect(screen.getByTestId('context-menu-play')).toBeInTheDocument();
      expect(screen.getByTestId('context-menu-install')).toBeInTheDocument();
      expect(screen.getByTestId('context-menu-match')).toBeInTheDocument();
      expect(screen.getByTestId('context-menu-add')).toBeInTheDocument();
      expect(screen.getByTestId('context-menu-tools')).toBeInTheDocument();
      expect(screen.getByTestId('context-menu-edit')).toBeInTheDocument();
      expect(screen.getByTestId('context-menu-save-protection')).toBeInTheDocument();
      expect(screen.getByTestId('context-menu-remove')).toBeInTheDocument();

      // Trigger play action
      fireEvent.click(screen.getByTestId('context-menu-play'));
      expect(launchSpy).toHaveBeenCalledWith('play');
      expect(uiStore.state.contextMenu.open).toBe(false);

      // Reopen and test match action
      act(() => {
        openContextMenu(200, 300, 'manual:local-game-1');
      });
      fireEvent.click(screen.getByTestId('context-menu-match'));
      expect(libraryInputSpy).toHaveBeenCalledWith('openMatch');
      expect(uiStore.state.contextMenu.open).toBe(false);

      // Reopen and test tools action
      act(() => {
        openContextMenu(200, 300, 'manual:local-game-1');
      });
      fireEvent.click(screen.getByTestId('context-menu-tools'));
      expect(toolsSpy).toHaveBeenCalledWith('manual:local-game-1');
      expect(uiStore.state.contextMenu.open).toBe(false);

      // Reopen and test add action
      act(() => {
        openContextMenu(200, 300, 'manual:local-game-1');
      });
      fireEvent.click(screen.getByTestId('context-menu-add'));
      expect(libraryInputSpy).toHaveBeenCalledWith('openAdd');
      expect(uiStore.state.contextMenu.open).toBe(false);

      // Reopen and test edit action
      act(() => {
        openContextMenu(200, 300, 'manual:local-game-1');
      });
      fireEvent.click(screen.getByTestId('context-menu-edit'));
      expect(uiStore.state.localEditorEntryKey).toBe('manual:local-game-1');
      expect(uiStore.state.contextMenu.open).toBe(false);

      // Reopen and test Escape
      act(() => {
        openContextMenu(200, 300, 'manual:local-game-1');
      });
      expect(screen.getByTestId('library-context-menu')).toBeInTheDocument();

      fireEvent.keyDown(window, { key: 'Escape' });
      expect(uiStore.state.contextMenu.open).toBe(false);
    });
  });

  describe('DetailPane and Localized Disabled Reasons', () => {
    it('handles resize drag, commits setDetailWidth, and closes pane', async () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');

      const snapshot = createDefaultSnapshot();
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: true,
        },
        search: '',
        access: 'verified',
        total: 1,
        providers: [],
        rows: createMockRows(1),
        scanning: false,
        failedProviders: [],
        selected: ['game-0'],
        focused: 'game-0',
        detail: {
          running: false,
          row: createMockRows(1)[0],
          installDir: '/games/hades',
          releaseYear: 2020,
          hasArtwork: true,
          membership: 'checking',
        },
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      renderWithProviders(<DetailPane />, snapshot);

      const resizeHandle = screen.getByTestId('detail-resize-handle');
      expect(resizeHandle).toBeInTheDocument();

      // Pointer drag resize
      fireEvent.pointerDown(resizeHandle, { clientX: 500 });
      fireEvent(
        window,
        new MouseEvent('pointermove', { clientX: 450, bubbles: true }),
      );
      // Drag updates are coalesced to one per animation frame.
      await waitFor(() => expect(uiStore.state.detailDragWidth).toBe(370));

      fireEvent(
        window,
        new MouseEvent('pointerup', { clientX: 450, bubbles: true }),
      );
      expect(libraryInputSpy).toHaveBeenCalledWith({ setDetailWidth: 370 });
      expect(uiStore.state.detailDragWidth).toBeNull();

      // Close button
      const closeBtn = screen.getByTestId('detail-close-btn');
      fireEvent.click(closeBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith({ setDetailOpen: false });
    });

    it('displays localized disabled reasons (Why) in en and pt-BR', () => {
      const makeSnapshotWithReasons = (): Snapshot => {
        const snap = createDefaultSnapshot();
        const row = createMockRows(1)[0];
        snap.library = {
          generation: 1,
          layout: {
            view: 'grid',
            sort: { key: 'title', direction: 'ascending' },
            filter: 'all',
            sidebarWidth: 200,
            detailWidth: 320,
            detailOpen: true,
          },
          search: '',
          access: 'verified',
          total: 1,
          providers: [],
          rows: [row],
          scanning: false,
          failedProviders: [],
          selected: ['game-0'],
          focused: 'game-0',
          detail: {
            running: false,
            row,
            installDir: '/games/hades',
            releaseYear: 2020,
            hasArtwork: true,
            membership: 'unavailable',
          },
          actions: {
            selected: 1,
            play: { disabled: 'needsVerifiedSession' },
            install: { disabled: 'noInstallRule' },
            matchGame: { disabled: 'notMatched' },
            addToGamenode: { disabled: { alreadyInLibrary: 'playing' } },
            changeMatch: true,
          },
        };
        return snap;
      };

      // In English
      const { unmount } = renderWithProviders(
        <DetailPane />,
        makeSnapshotWithReasons(),
        'en',
      );

      expect(screen.getByTestId('detail-play-reason')).toHaveTextContent(
        'Your GameNode session is not verified yet. Retry the connection.',
      );
      expect(screen.getByTestId('detail-match-reason')).toHaveTextContent(
        'Match this game to a GameNode game first.',
      );
      expect(screen.getByTestId('detail-add-reason')).toHaveTextContent(
        'Already in your GameNode library as Playing.',
      );

      unmount();

      // In Portuguese
      renderWithProviders(
        <DetailPane />,
        makeSnapshotWithReasons(),
        'pt-BR',
      );

      expect(screen.getByTestId('detail-play-reason')).toHaveTextContent(
        'Sua sessão do GameNode ainda não foi verificada. Tente conectar novamente.',
      );
      expect(screen.getByTestId('detail-match-reason')).toHaveTextContent(
        'Primeiro associe este jogo a um jogo do GameNode.',
      );
      expect(screen.getByTestId('detail-add-reason')).toHaveTextContent(
        'Já está na sua biblioteca do GameNode como Jogando.',
      );
    });

    it('renders launch status and allows dismiss', () => {
      const dismissSpy = vi.spyOn(commands, 'dismissLaunch');

      const snapshot = createDefaultSnapshot();
      const row = createMockRows(1)[0];
      snapshot.launch = {
        entryKey: 'game-0',
        kind: 'play',
        state: { failed: 'missingHandler' },
      };
      snapshot.library = {
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: true,
        },
        search: '',
        access: 'verified',
        total: 1,
        providers: [],
        rows: [row],
        scanning: false,
        failedProviders: [],
        selected: ['game-0'],
        detail: {
          running: false,
          row,
          installDir: '/games/hades',
          releaseYear: 2020,
          hasArtwork: true,
          membership: 'checking',
        },
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      renderWithProviders(<DetailPane />, snapshot, 'en');

      expect(screen.getByTestId('detail-launch-status')).toBeInTheDocument();
      expect(screen.getByTestId('detail-launch-status')).toHaveTextContent(
        'The store application is not installed or cannot be opened.',
      );

      fireEvent.click(screen.getByTestId('detail-dismiss-launch-btn'));
      expect(dismissSpy).toHaveBeenCalled();
    });
  });

  describe('Library Formatters and Localized Helpers', () => {
    it('formats entry status, duration, reason, and i18n interpolation in en and pt-BR', () => {
      let tEn!: (key: string) => string;
      let tPt!: (key: string) => string;

      const ConsumerEn = () => {
        const { t } = useI18n();
        tEn = t;
        return null;
      };
      const ConsumerPt = () => {
        const { t } = useI18n();
        tPt = t;
        return null;
      };

      const { unmount } = render(
        <I18nProvider initialLocale="en">
          <ConsumerEn />
        </I18nProvider>,
      );

      // formatStatus
      expect(formatStatus('playing', tEn)).toBe('Playing');
      expect(formatStatus('finished', tEn)).toBe('Finished');
      expect(formatStatus('planned', tEn)).toBe('Planned');
      expect(formatStatus('dropped', tEn)).toBe('Dropped');
      expect(formatStatus('ongoing', tEn)).toBe('Ongoing');
      expect(formatStatus(null, tEn)).toBe('');

      // formatDuration
      expect(formatDuration(null, tEn)).toBe('—');
      expect(formatDuration(-1, tEn)).toBe('—');
      expect(formatDuration(0, tEn)).toBe('0 min');
      expect(formatDuration(45, tEn)).toBe('45 s');
      expect(formatDuration(180, tEn)).toBe('3 min');
      expect(formatDuration(7260, tEn)).toBe('2 h 1 min');

      // formatI18n
      expect(formatI18n('Saved {} games', 5)).toBe('Saved 5 games');
      expect(formatI18n('Saved {} of {}', 2, 10)).toBe('Saved 2 of 10');
      expect(formatI18n('{0} from {1}', 'Hades', 'Steam')).toBe('Hades from Steam');

      // formatReason in English
      expect(formatReason('running', tEn)).toBe('Running');
      expect(formatReason('nothingSelected', tEn)).toBe('Select a game first.');
      expect(formatReason('multipleSelected', tEn)).toBe('Select a single game.');
      expect(formatReason('noLaunchRule', tEn)).toBe('This store cannot start games from GameNode.');
      expect(formatReason('noInstallRule', tEn)).toBe('This store cannot install games from GameNode.');
      expect(formatReason('missingField', tEn)).toBe('Details this action needs are missing. Rescan to refresh them.');
      expect(formatReason('needsVerifiedSession', tEn)).toBe('Your GameNode session is not verified yet. Retry the connection.');
      expect(formatReason('offline', tEn)).toBe('GameNode cannot be reached right now.');
      expect(formatReason('notMatched', tEn)).toBe('Match this game to a GameNode game first.');
      expect(formatReason('noMatchedGames', tEn)).toBe('None of the selected games is matched to a GameNode game.');
      expect(formatReason('checkingMembership', tEn)).toBe('Checking your GameNode library…');
      expect(formatReason('membershipUnavailable', tEn)).toBe('Could not check your GameNode library. Try again.');
      expect(formatReason({ alreadyInLibrary: null }, tEn)).toBe('Already in your GameNode library.');
      expect(formatReason({ alreadyInLibrary: 'playing' }, tEn)).toBe('Already in your GameNode library as Playing.');
      expect(formatReason(null, tEn)).toBe('');

      unmount();

      render(
        <I18nProvider initialLocale="pt-BR">
          <ConsumerPt />
        </I18nProvider>,
      );

      // formatStatus in pt-BR
      expect(formatStatus('playing', tPt)).toBe('Jogando');
      expect(formatStatus('finished', tPt)).toBe('Zerado');
      expect(formatStatus('planned', tPt)).toBe('Planejado');
      expect(formatStatus('dropped', tPt)).toBe('Abandonado');
      expect(formatStatus('ongoing', tPt)).toBe('Contínuo');

      // formatReason in pt-BR
      expect(formatReason('running', tPt)).toBe('Em execução');
      expect(formatReason('nothingSelected', tPt)).toBe('Selecione um jogo primeiro.');
      expect(formatReason('multipleSelected', tPt)).toBe('Selecione apenas um jogo.');
      expect(formatReason('noLaunchRule', tPt)).toBe('Esta loja não pode iniciar jogos pelo GameNode.');
      expect(formatReason('noInstallRule', tPt)).toBe('Esta loja não pode instalar jogos pelo GameNode.');
      expect(formatReason('missingField', tPt)).toBe('Faltam detalhes de que esta ação precisa. Faça uma nova varredura para atualizá-los.');
      expect(formatReason('needsVerifiedSession', tPt)).toBe('Sua sessão do GameNode ainda não foi verificada. Tente conectar novamente.');
      expect(formatReason('offline', tPt)).toBe('Não foi possível acessar o GameNode agora.');
      expect(formatReason('notMatched', tPt)).toBe('Primeiro associe este jogo a um jogo do GameNode.');
      expect(formatReason('noMatchedGames', tPt)).toBe('Nenhum dos jogos selecionados está associado a um jogo do GameNode.');
      expect(formatReason('checkingMembership', tPt)).toBe('Verificando sua biblioteca do GameNode…');
      expect(formatReason('membershipUnavailable', tPt)).toBe('Não foi possível verificar sua biblioteca do GameNode. Tente novamente.');
      expect(formatReason({ alreadyInLibrary: null }, tPt)).toBe('Já está na sua biblioteca do GameNode.');
      expect(formatReason({ alreadyInLibrary: 'playing' }, tPt)).toBe('Já está na sua biblioteca do GameNode como Jogando.');
    });
  });
});
