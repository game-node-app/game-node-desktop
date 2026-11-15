import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import {
  commands,
  createDefaultFilters,
  type Row,
  type RowGroup,
  type StoreAccountView,
} from '../../api';
import { resetUiStore, openFilters, closeFilters, uiStore } from '../../store/uiStore';
import { makeLibrary, renderControls, spyLibraryInput, setInputs } from './controlsTestUtils';
import { LibraryScreen } from './LibraryScreen';

function row(overrides: Partial<Row> & Pick<Row, 'key' | 'title'>): Row {
  return {
    detectedName: overrides.title,
    matchedName: overrides.title,
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    running: false,
    status: { matched: { gameId: 1, source: 'automatic' } },
    coverFile: null,
    playtime: { seconds: 3600, source: 'store', lastPlayedAt: 1_700_000_000 },
    releaseYear: 2020,
    addedAt: 1_700_000_000,
    isFavorite: false,
    hidden: false,
    gamenode: null,
    ...overrides,
  };
}

const rows: Row[] = [
  row({ key: 'game-0', title: 'Game Zero' }),
  row({ key: 'game-1', title: 'Game One', isFavorite: true }),
  row({ key: 'game-2', title: 'Game Two' }),
];

const groups: RowGroup[] = [
  { id: 'steam', labelKey: 'Steam', count: 3, collapsed: false, firstIndex: 0 },
];

function library(overrides: Parameters<typeof makeLibrary>[0] = {}) {
  return makeLibrary({
    total: rows.length,
    installedTotal: rows.length,
    rows,
    selected: ['game-0'],
    focused: 'game-0',
    ...overrides,
  });
}

function renderScreen(overrides: Parameters<typeof makeLibrary>[0] = {}) {
  return renderControls(<LibraryScreen />, library(overrides));
}

describe('LibraryScreen keyboard', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('moves through the engine with arrows, Home and End, extending with Shift', () => {
    const input = spyLibraryInput();
    renderScreen();
    const root = screen.getByTestId('library-screen');

    fireEvent.keyDown(root, { key: 'ArrowDown' });
    fireEvent.keyDown(root, { key: 'ArrowDown', shiftKey: true });
    fireEvent.keyDown(root, { key: 'ArrowRight' });
    fireEvent.keyDown(root, { key: 'ArrowLeft', shiftKey: true });
    fireEvent.keyDown(root, { key: 'Home' });
    fireEvent.keyDown(root, { key: 'End', shiftKey: true });

    expect(setInputs(input)).toEqual([
      { move: { direction: 'down', extend: false } },
      { move: { direction: 'down', extend: true } },
      { move: { direction: 'right', extend: false } },
      { move: { direction: 'left', extend: true } },
      { move: { direction: 'home', extend: false } },
      { move: { direction: 'end', extend: true } },
    ]);
  });

  it.each([
    ['ascending', 'descending'],
    ['descending', 'ascending'],
  ] as const)(
    'Shift+S flips %s to %s while preserving the sort key and never opens the sort menu',
    (direction, next) => {
      const input = spyLibraryInput();
      renderScreen({
        layout: { sort: { key: 'playtime', direction } },
      });

      fireEvent.keyDown(screen.getByTestId('library-screen'), {
        key: 'S',
        shiftKey: true,
      });

      expect(setInputs(input)).toEqual([
        { setSort: { key: 'playtime', direction: next } },
      ]);
      expect(screen.queryByTestId('library-sort-menu')).not.toBeInTheDocument();
    },
  );

  it('handles favorites, activation, details, select all and clearing the selection', () => {
    const input = spyLibraryInput();
    const activate = vi.spyOn(commands, 'activateEntry').mockResolvedValue();
    renderScreen();

    const root = screen.getByTestId('library-screen');
    fireEvent.keyDown(root, { key: 'f' });
    expect(setInputs(input)).toContainEqual({
      setFavorite: { key: 'game-0', favorite: true, now: expect.any(Number) },
    });

    fireEvent.keyDown(root, { key: 'Enter' });
    expect(activate).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(root, { key: 'Enter', ctrlKey: true });
    expect(setInputs(input)).toContainEqual('openDetails');

    fireEvent.keyDown(root, { key: 'a', ctrlKey: true });
    expect(setInputs(input)).toContainEqual('selectAll');

    fireEvent.keyDown(root, { key: 'Escape' });
    expect(setInputs(input)).toContainEqual('clearSelection');
  });

  it('ignores Shift+S while an overlay owns the keyboard and when another handler consumed it', () => {
    const input = spyLibraryInput();
    openFilters();
    renderScreen();
    const root = screen.getByTestId('library-screen');

    fireEvent.keyDown(root, { key: 'S', shiftKey: true });
    expect(setInputs(input)).toEqual([]);

    closeFilters();
    const consumed = new KeyboardEvent('keydown', {
      key: 'S',
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    consumed.preventDefault();
    root.dispatchEvent(consumed);
    expect(setInputs(input)).toEqual([]);
  });

  it('does not favorite or select all while the search field has focus', () => {
    const input = spyLibraryInput();
    renderScreen();
    const search = screen.getByTestId('library-search-input');

    fireEvent.keyDown(search, { key: 'f' });
    fireEvent.keyDown(search, { key: 'S', shiftKey: true });
    fireEvent.keyDown(search, { key: 'a', ctrlKey: true });

    expect(setInputs(input)).not.toContainEqual(
      expect.objectContaining({ setFavorite: expect.anything() }),
    );
    expect(setInputs(input)).not.toContainEqual('selectAll');
    expect(setInputs(input)).not.toContainEqual(
      expect.objectContaining({ setSort: expect.anything() }),
    );
  });

  it('does not activate a game from a focused group header', () => {
    const activate = vi.spyOn(commands, 'activateEntry').mockResolvedValue();
    const input = spyLibraryInput();
    renderScreen({
      layout: { group: 'store' },
      groups,
    });

    const header = screen.getByTestId('group-header-steam').querySelector('button');
    expect(header).not.toBeNull();
    fireEvent.keyDown(header!, { key: 'Enter' });

    expect(activate).not.toHaveBeenCalled();
    expect(setInputs(input)).not.toContainEqual('openDetails');
    expect(setInputs(input)).not.toContainEqual('selectAll');
  });

  it('opens the context menu at the real focused row, not the viewport origin', () => {
    renderScreen();
    const root = screen.getByTestId('library-screen');
    const rowElement = document.querySelector<HTMLElement>('[data-row-key="game-0"]');
    expect(rowElement).not.toBeNull();
    rowElement!.getBoundingClientRect = () =>
      ({
        left: 100,
        top: 200,
        width: 172,
        height: 270,
        right: 272,
        bottom: 470,
        x: 100,
        y: 200,
        toJSON: () => ({}),
      }) as DOMRect;

    fireEvent.keyDown(root, { key: 'F10', shiftKey: true });

    expect(uiStore.state.contextMenu.open).toBe(true);
    expect(uiStore.state.contextMenu.entryKey).toBe('game-0');
    expect(uiStore.state.contextMenu.x).toBe(186);
    expect(uiStore.state.contextMenu.y).toBe(335);
  });

  it('leaves the global library shortcuts to useGlobalShortcuts', () => {
    const input = spyLibraryInput();
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue();
    renderScreen();
    const root = screen.getByTestId('library-screen');

    fireEvent.keyDown(root, { key: 'l', ctrlKey: true });
    fireEvent.keyDown(root, { key: 'i', ctrlKey: true });
    fireEvent.keyDown(root, { key: 'k', ctrlKey: true });
    fireEvent.keyDown(root, { key: 'F5' });

    expect(setInputs(input)).toEqual([]);
    expect(rescan).not.toHaveBeenCalled();
  });
});

describe('LibraryScreen composition and engine states', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  const connectedStore = (): StoreAccountView => ({
    provider: 'steam',
    state: 'connected',
    accountId: 'steam-1',
    accountName: 'cassi',
    ownedCount: 12,
    sharedCount: 0,
    sharedLabel: 'Steam Family',
    installedCount: 4,
    lastCheckedAt: 1_700_000_000,
    lastSyncedAt: 1_700_000_000,
    storage: null,
    persistFailed: false,
    catalogError: null,
    sharedError: null,
    canSignIn: true,
    canCancelSignIn: false,
    canFocusSignIn: false,
    canSignOut: true,
    canCheck: true,
    canRemoveCredential: false,
  });

  it('mounts the header, toolbar, chips row, grid, context menu and batch bar once', () => {
    renderScreen({
      selected: ['game-0', 'game-1'],
      focused: 'game-0',
      layout: { filters: { ...createDefaultFilters(), favoritesOnly: true } },
    });

    expect(screen.getAllByTestId('library-header')).toHaveLength(1);
    expect(screen.getAllByTestId('library-toolbar')).toHaveLength(1);
    expect(screen.getAllByTestId('library-screen')).toHaveLength(1);
    expect(screen.getAllByTestId('batch-bar')).toHaveLength(1);
    expect(screen.getByTestId('library-active-filters')).toBeInTheDocument();
  });

  it('renders the first-run checklist when no store or folder is configured', () => {
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue();
    const folder = vi.spyOn(commands, 'pickLocalWatchFolder').mockResolvedValue();
    renderControls(
      <LibraryScreen />,
      library({
        total: 0,
        rows: [],
        selected: [],
        focused: null,
        empty: { noGames: { stores: [] } },
      }),
      'en',
      { auth: { signedIn: { user: { id: 'u-1', username: 'cassi' } } } },
    );

    expect(screen.getByTestId('library-first-run')).toBeInTheDocument();
    expect(screen.getByTestId('first-run-step-1')).toHaveAttribute('data-done', 'true');

    fireEvent.click(screen.getByTestId('first-run-action-2'));
    expect(uiStore.state.settingsOpen).toBe(true);
    expect(uiStore.state.settingsTopic).toBe('stores');

    fireEvent.click(screen.getByTestId('first-run-action-3'));
    expect(folder).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByTestId('first-run-rescan'));
    expect(rescan).toHaveBeenCalledTimes(1);
  });

  it('shows the no-games state with the stores it looked at when a store is connected', () => {
    renderControls(
      <LibraryScreen />,
      library({
        total: 0,
        rows: [],
        selected: [],
        focused: null,
        empty: { noGames: { stores: ['Steam'] } },
      }),
      'en',
      { storeAccounts: [connectedStore()] },
    );

    expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
      'Looked in: Steam',
    );
    expect(screen.getByTestId('library-empty-rescan-btn')).toBeInTheDocument();
  });

  it('shows the installed-filter state and clears only the installed fact', () => {
    const input = spyLibraryInput();
    const filters = {
      ...createDefaultFilters(),
      availability: { installed: true, owned: false, shared: false },
      favoritesOnly: true,
    };
    renderScreen({
      total: 8,
      installedTotal: 0,
      rows: [],
      selected: [],
      focused: null,
      empty: { noInstalled: { provider: 'steam' } },
      layout: { filters },
    });

    expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
      'Nothing is installed on this PC.',
    );
    fireEvent.click(screen.getByTestId('library-empty-show-all-btn'));
    expect(setInputs(input)).toContainEqual({
      setFilters: {
        ...filters,
        availability: { installed: false, owned: false, shared: false },
      },
    });
  });

  it('leaves the Installed scope as well as its fact when Show all games is chosen', () => {
    const input = spyLibraryInput();
    const filters = {
      ...createDefaultFilters(),
      availability: { installed: true, owned: false, shared: false },
      favoritesOnly: true,
    };
    renderScreen({
      total: 8,
      installedTotal: 0,
      rows: [],
      selected: [],
      focused: null,
      empty: { noInstalled: { provider: 'steam' } },
      layout: { scope: 'installed', filters },
    });

    fireEvent.click(screen.getByTestId('library-empty-show-all-btn'));

    // Scope first: leaving Installed is what reveals owned rows, and the
    // availability fact that was hiding them goes with it. The unrelated
    // favorites facet stays active.
    expect(setInputs(input)).toEqual([
      { setScope: 'all' },
      {
        setFilters: {
          ...filters,
          availability: { installed: false, owned: false, shared: false },
        },
      },
    ]);
  });

  it('mounts the detail pane as a full-height sibling of the content column', () => {
    renderScreen({
      selected: ['game-0', 'game-1'],
      layout: { detailOpen: true, detailWidth: 360 },
    });

    const root = screen.getByTestId('library-screen');
    const column = screen.getByTestId('library-content-column');
    const pane = screen.getByTestId('detail-pane');

    expect(column.parentElement).toBe(root);
    expect(pane.parentElement).toBe(root);
    expect(within(column).getByTestId('library-header')).toBeInTheDocument();
    expect(within(column).getByTestId('library-toolbar')).toBeInTheDocument();
    expect(column).not.toContainElement(pane);

    // The declared width covers the pane and its overlaid resize handle; the
    // handle never adds a second column of pixels.
    expect(pane).toHaveStyle({ width: '360px' });
    const handle = within(pane).getByTestId('detail-resize-handle');
    expect(handle).toHaveClass('absolute');
    expect(handle).not.toHaveClass('relative');

    // One batch bar, anchored inside the content column it acts on.
    expect(within(column).getByTestId('batch-bar')).toHaveClass('absolute');
  });

  it('distinguishes a search no-match, clears filters without the search and opens the prefilled catalog', () => {
    const input = spyLibraryInput();
    renderScreen({
      total: 8,
      rows: [],
      selected: [],
      focused: null,
      empty: 'nothingMatches',
      search: 'lumen drift',
      layout: { filters: { ...createDefaultFilters(), favoritesOnly: true } },
    });

    expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
      'No game matches “lumen drift”',
    );
    fireEvent.click(screen.getByTestId('library-empty-clear-filters-btn'));
    expect(setInputs(input)).toContainEqual({ setFilters: createDefaultFilters() });
    expect(setInputs(input)).not.toContainEqual(
      expect.objectContaining({ setSearch: expect.anything() }),
    );

    fireEvent.click(screen.getByTestId('library-empty-search-gamenode-btn'));
    expect(uiStore.state.activeDestination).toBe('catalog');
    expect(setInputs(input)).toContainEqual({ catalogSearch: 'lumen drift' });
  });

  it('shows the per-provider scanning panel with twelve skeletons before the first rows', () => {
    renderScreen({
      total: 0,
      rows: [],
      selected: [],
      focused: null,
      scanning: true,
      scanProgress: [
        { provider: 'steam', name: 'Steam', state: 'done', found: 164 },
        { provider: 'drm-free', name: 'DRM-free folders', state: 'scanning', found: null },
      ],
    });

    expect(screen.getByTestId('library-scanning-panel')).toBeInTheDocument();
    expect(screen.getByTestId('scan-progress-steam')).toHaveAttribute('data-state', 'done');
    expect(screen.getByTestId('library-scanning-skeletons').children).toHaveLength(12);
  });

  it('keeps known rows usable below the scanning panel', () => {
    renderScreen({
      scanning: true,
      scanProgress: [{ provider: 'steam', name: 'Steam', state: 'scanning', found: null }],
    });

    expect(screen.getByTestId('library-scanning-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('library-scanning-skeletons')).not.toBeInTheDocument();
    expect(screen.getAllByRole('listitem').length).toBeGreaterThan(0);
  });

  it('renders one banner for the active problem above the toolbar', () => {
    renderControls(
      <LibraryScreen />,
      library(),
      'en',
      {
        storeAccounts: [{ ...connectedStore(), state: 'expired' }],
      },
    );

    expect(screen.getByTestId('library-banner-expired')).toBeInTheDocument();
    expect(screen.getAllByTestId('library-banner-expired')).toHaveLength(1);
    expect(within(screen.getByTestId('library-toolbar')).queryByTestId('library-banner-expired')).toBeNull();
  });
});
