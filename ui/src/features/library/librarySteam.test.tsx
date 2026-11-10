import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClientProvider } from '@tanstack/react-query';
import { createAppQueryClient } from '../../AppProviders';
import {
  commands,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Actions,
  type LibraryLayout,
  type LibraryView,
  type Row,
  type Snapshot,
  type StoreAccountState,
  type StoreAccountView,
  createDefaultFilters,
} from '../../api';
import { I18nProvider } from '../../i18n';
import { openContextMenu, resetUiStore, uiStore } from '../../store/uiStore';
import { createMockHost } from '../../mock/host';
import { DetailPane } from './DetailPane';
import { GameGrid } from './GameGrid';
import { GameList } from './GameList';
import { LibraryScreen } from './LibraryScreen';
import { ContextMenu } from './ContextMenu';
import { LibraryToolbar } from './LibraryToolbar';
import { Sidebar } from '../shell/Sidebar';

const availableActions: Actions = {
  selected: 1,
  play: 'available',
  install: 'available',
  matchGame: 'available',
  addToGamenode: 'available',
  changeMatch: false,
};

function makeRow(overrides: Partial<Row> & Pick<Row, 'key' | 'title'>): Row {
  return {
    detectedName: overrides.title,
    matchedName: null,
    provider: 'steam',
    providerName: 'Steam',
    installed: false,
    owned: true,
    familyShared: false,
    running: false,
    status: 'unmatched',
    coverFile: null,
    playtime: { seconds: null, source: 'store', lastPlayedAt: null },
    releaseYear: null,
    addedAt: 0,
    isFavorite: false,
    ...overrides,
  };
}

function makeLibrary(
  rows: Row[],
  options: {
    layout?: Partial<LibraryLayout>;
    total?: number;
    installedTotal?: number;
    providers?: LibraryView['providers'];
    empty?: LibraryView['empty'];
    search?: string;
    detail?: LibraryView['detail'];
    actions?: Actions;
  } = {},
): LibraryView {
  return {
    generation: 1,
    layout: {
      view: 'grid',
      sort: { key: 'title', direction: 'ascending' },
      filter: 'all',
      filters: createDefaultFilters(),
      sidebarWidth: 216,
      detailWidth: 320,
      detailOpen: true,
      ...options.layout,
    },
    search: options.search ?? '',
    access: 'verified',
    total: options.total ?? rows.length,
    installedTotal:
      options.installedTotal ?? rows.filter((row) => row.installed).length,
    providers: options.providers ?? [
      {
        id: 'steam',
        name: 'Steam',
        count: rows.length,
        installedCount: rows.filter((row) => row.installed).length,
        ownedCount: rows.filter((row) => row.owned).length,
        familyCount: rows.filter((row) => row.familyShared && !row.owned).length,
      },
    ],
    rows,
    scanning: false,
    failedProviders: [],
    selected: rows.length ? [rows[0].key] : [],
    focused: rows.length ? rows[0].key : null,
    actions: options.actions ?? availableActions,
    detail: options.detail ?? null,
    empty: options.empty ?? null,
  };
}

function makeStoreAccount(state: StoreAccountState, provider = 'steam'): StoreAccountView {
  return {
    provider,
    state,
    accountId: state === 'notConnected' ? null : '76561198000000000',
    accountName: state === 'notConnected' ? null : 'cassi',
    ownedCount: 320,
    familyCount: 8,
    installedCount: 18,
    lastCheckedAt: null,
    storage: 'secretService',
    persistFailed: false,
    catalogError:
      state === 'expired' ? 'expired' : state === 'unreachable' ? 'unreachable' : null,
    familyError: null,
    canSignIn: state === 'notConnected' || state === 'expired',
    canCancelSignIn: state === 'signingIn',
    canFocusSignIn: state === 'signingIn',
    canSignOut: state === 'connected' || state === 'expired' || state === 'unreachable',
    canCheck: state === 'connected' || state === 'unreachable',
    canRemoveCredential: state === 'credentialError',
  };
}

function renderSnapshot(
  ui: React.ReactElement,
  snapshot: Snapshot,
  locale = 'en',
) {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  const result = render(
    <QueryClientProvider client={queryClient}>
      <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
    </QueryClientProvider>,
  );
  return { ...result, queryClient };
}

describe('Steam library UI', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  describe('Filters', () => {
    it('opens the panel and sends each facet change', async () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([makeRow({ key: 'steam:1', title: 'Owned' })]);

      renderSnapshot(<LibraryToolbar />, snapshot);
      const user = userEvent.setup();

      const button = screen.getByTestId('library-filters-btn');
      expect(button).toHaveAttribute('aria-expanded', 'false');
      await user.click(button);
      expect(button).toHaveAttribute('aria-expanded', 'true');
      expect(await screen.findByTestId('library-filters-popover')).toBeInTheDocument();

      await user.click(await screen.findByTestId('filter-installed'));
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setFilters: {
          ...createDefaultFilters(),
          availability: { installed: true, owned: false, family: false },
        },
      });

      await user.selectOptions(screen.getByTestId('filter-playtime'), 'unplayed');
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setFilters: { ...createDefaultFilters(), playtime: 'unplayed' },
      });

      await user.selectOptions(screen.getByTestId('filter-release'), '1980');
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setFilters: { ...createDefaultFilters(), release: { decade: 1980 } },
      });
    });

    it('shows one removable chip per active facet and a clear-all action', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = createDefaultSnapshot();
      const filters = {
        ...createDefaultFilters(),
        availability: { installed: true, owned: false, family: false },
        playtime: 'played' as const,
        favoritesOnly: true,
      };
      snapshot.library = makeLibrary([makeRow({ key: 'steam:1', title: 'Owned' })], {
        layout: { filters },
        total: 18,
      });

      renderSnapshot(<LibraryToolbar />, snapshot);

      expect(screen.getByTestId('library-filters-count')).toHaveTextContent('3');
      expect(screen.getByTestId('library-filtered-count')).toHaveTextContent('Showing 1 of 18');
      expect(screen.getByTestId('library-filter-chip-installed')).toBeInTheDocument();
      expect(screen.getByTestId('library-filter-chip-playtime')).toHaveTextContent('Played');
      expect(screen.getByTestId('library-filter-chip-favorites')).toHaveTextContent('Favorites');

      fireEvent.click(screen.getByTestId('library-filter-chip-remove-installed'));
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setFilters: { ...filters, availability: { installed: false, owned: false, family: false } },
      });

      fireEvent.click(screen.getByTestId('library-clear-filters-btn'));
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setFilters: createDefaultFilters(),
      });
      // Clearing filters never clears the search text.
      expect(libraryInputSpy).not.toHaveBeenCalledWith(
        expect.objectContaining({ setSearch: expect.anything() }),
      );
    });

    it('opens from the keyboard and closes with Escape returning focus', async () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([]);
      renderSnapshot(<LibraryToolbar />, snapshot);
      const user = userEvent.setup();

      const button = screen.getByTestId('library-filters-btn');
      button.focus();
      await user.keyboard('{Enter}');
      expect(await screen.findByTestId('library-filters-popover')).toBeInTheDocument();

      await user.keyboard('{Escape}');
      await waitFor(() =>
        expect(screen.getByTestId('library-filters-popover')).not.toBeVisible(),
      );
      expect(button).toHaveFocus();
    });

    it('round-trips the filters through the mock layout', async () => {
      const host = createMockHost();
      const installed = {
        ...createDefaultFilters(),
        availability: { installed: true, owned: false, family: false },
      };
      await host.invoke('library_input', { input: { setFilters: installed } });

      let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
      expect(snapshot.library?.layout.filters.availability.installed).toBe(true);
      expect(snapshot.library?.rows.length).toBe(snapshot.library?.installedTotal);

      await host.invoke('library_input', {
        input: { setFilters: createDefaultFilters() },
      });
      snapshot = (await host.invoke('get_snapshot')) as Snapshot;
      expect(snapshot.library?.layout.filters.availability.installed).toBe(false);
      expect(snapshot.library?.rows.length).toBe(snapshot.library?.total);
    });
  });

  describe('Sidebar counts', () => {
    it('shows visible counts with a muted installed subline in the full library', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([], {
        total: 340,
        installedTotal: 18,
        providers: [
          { id: 'steam', name: 'Steam', count: 328, installedCount: 18, ownedCount: 320, familyCount: 8 },
          { id: 'gog', name: 'GOG', count: 12, installedCount: 12, ownedCount: 0, familyCount: 0 },
        ],
      });

      renderSnapshot(
        <Sidebar library={snapshot.library} localGames={snapshot.localGames} />,
        snapshot,
      );

      expect(screen.getByTestId('sidebar-all-count')).toHaveTextContent('340');
      expect(screen.getByTestId('sidebar-provider-count-steam')).toHaveTextContent('328');
      expect(screen.getByTestId('sidebar-provider-installed-steam')).toHaveTextContent(
        '18 installed',
      );
      expect(screen.queryByTestId('sidebar-provider-installed-gog')).not.toBeInTheDocument();
    });

    it('follows the installed filter instead of the visible totals', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([], {
        layout: { filters: { ...createDefaultFilters(), availability: { installed: true, owned: false, family: false } } },
        total: 340,
        installedTotal: 18,
        providers: [
          { id: 'steam', name: 'Steam', count: 18, installedCount: 18, ownedCount: 320, familyCount: 8 },
          { id: 'gog', name: 'GOG', count: 0, installedCount: 12, ownedCount: 0, familyCount: 0 },
        ],
      });

      renderSnapshot(
        <Sidebar library={snapshot.library} localGames={snapshot.localGames} />,
        snapshot,
      );

      expect(screen.getByTestId('sidebar-all-count')).toHaveTextContent('18');
      expect(screen.getByTestId('sidebar-provider-count-steam')).toHaveTextContent('18');
      expect(screen.getByTestId('sidebar-provider-count-gog')).toHaveTextContent('0');
      expect(screen.queryByTestId('sidebar-provider-installed-steam')).not.toBeInTheDocument();
    });

    it('omits the subline when nothing is owned beyond what is installed', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([], {
        total: 9,
        installedTotal: 9,
        providers: [
          { id: 'steam', name: 'Steam', count: 9, installedCount: 9, ownedCount: 0, familyCount: 0 },
        ],
      });

      renderSnapshot(
        <Sidebar library={snapshot.library} localGames={snapshot.localGames} />,
        snapshot,
      );

      expect(screen.getByTestId('sidebar-all-count')).toHaveTextContent('9');
      expect(screen.getByTestId('sidebar-provider-count-steam')).toHaveTextContent('9');
      expect(screen.queryByTestId('sidebar-provider-installed-steam')).not.toBeInTheDocument();
    });
  });

  describe('Grid and list indicators', () => {
    const rows = [
      makeRow({ key: 'steam:1', title: 'Installed Owned', installed: true, owned: true }),
      makeRow({ key: 'steam:2', title: 'Owned Only', installed: false, owned: true }),
      makeRow({ key: 'steam:3', title: 'Family Shared', owned: false, familyShared: true }),
      makeRow({ key: 'steam:4', title: 'Owned And Shared', owned: true, familyShared: false }),
    ];

    it('marks installed and family-shared games in the grid without colour alone', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary(rows);

      renderSnapshot(<GameGrid />, snapshot);

      const installed = screen.getByTestId('game-tile-installed-steam:1');
      expect(installed).toHaveAttribute('role', 'img');
      expect(installed).toHaveAttribute('aria-label', 'Installed on this PC');

      expect(screen.queryByTestId('game-tile-installed-steam:2')).not.toBeInTheDocument();
      const family = screen.getByTestId('game-tile-family-steam:3');
      expect(family).toHaveAttribute('role', 'img');
      expect(family).toHaveAttribute('aria-label', 'Shared through Steam Family');
      expect(screen.queryByTestId('game-tile-family-steam:4')).not.toBeInTheDocument();
    });

    it('marks installed and family-shared games in the list', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary(rows, { layout: { view: 'list' } });

      renderSnapshot(<GameList />, snapshot);

      expect(screen.getByTestId('game-row-installed-steam:1')).toHaveAttribute(
        'aria-label',
        'Installed on this PC',
      );
      expect(screen.queryByTestId('game-row-installed-steam:2')).not.toBeInTheDocument();
      expect(screen.getByTestId('game-row-family-steam:3')).toHaveAttribute(
        'aria-label',
        'Shared through Steam Family',
      );
      expect(screen.queryByTestId('game-row-family-steam:4')).not.toBeInTheDocument();
    });

    it('hides the installed indicator while the installed filter is active', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([rows[0]], {
        layout: { filters: { ...createDefaultFilters(), availability: { installed: true, owned: false, family: false } } },
      });

      renderSnapshot(<GameGrid />, snapshot);

      expect(screen.queryByTestId('game-tile-installed-steam:1')).not.toBeInTheDocument();
    });
  });

  describe('Detail pane ownership states', () => {
    function detailSnapshot(
      row: Row,
      installDir: string,
      actions: Actions = availableActions,
    ): Snapshot {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([row], {
        total: 1,
        installedTotal: row.installed ? 1 : 0,
        detail: {
          running: false,
          row,
          installDir,
          releaseYear: 2020,
          hasArtwork: false,
          membership: 'notChecked',
        },
        actions,
      });
      return snapshot;
    }

    it('replaces the install folder with a not-installed statement and keeps Play/Install for owned-only games', () => {
      const row = makeRow({
        key: 'steam:620',
        title: 'Portal 2',
        installed: false,
        owned: true,
      });
      const launchSpy = vi.spyOn(commands, 'launchSelected');

      renderSnapshot(<DetailPane />, detailSnapshot(row, ''));

      expect(screen.getByTestId('detail-not-installed')).toHaveTextContent(
        'Not installed on this PC',
      );
      expect(screen.queryByTestId('detail-install-dir')).not.toBeInTheDocument();
      expect(screen.queryByTestId('detail-family-label')).not.toBeInTheDocument();

      const install = screen.getByTestId('detail-install-btn');
      expect(install).toBeEnabled();
      fireEvent.click(install);
      expect(launchSpy).toHaveBeenCalledWith('install');

      expect(screen.getByTestId('detail-play-btn')).toBeEnabled();
    });

    it('identifies family-shared games in the detail pane', () => {
      const row = makeRow({
        key: 'steam:700',
        title: 'Shared Adventure',
        owned: false,
        familyShared: true,
      });

      renderSnapshot(<DetailPane />, detailSnapshot(row, ''));

      expect(screen.getByTestId('detail-family-label')).toHaveTextContent('Steam Family');
      expect(screen.getByTestId('detail-not-installed')).toBeInTheDocument();
    });

    it('keeps installed details unchanged', () => {
      const row = makeRow({
        key: 'steam:1',
        title: 'Installed Game',
        installed: true,
        owned: true,
      });

      renderSnapshot(<DetailPane />, detailSnapshot(row, '/games/installed'));

      expect(screen.getByTestId('detail-install-dir')).toHaveTextContent(
        '/games/installed',
      );
      expect(screen.queryByTestId('detail-not-installed')).not.toBeInTheDocument();
      expect(screen.queryByTestId('detail-family-label')).not.toBeInTheDocument();
    });

    it('keeps DRM-free details unchanged and omits Install', () => {
      const row = makeRow({
        key: 'manual:local-1',
        title: 'Local Game',
        provider: 'manual',
        providerName: 'DRM-free',
        installed: true,
        owned: false,
      });

      renderSnapshot(
        <DetailPane />,
        detailSnapshot(row, '/games/local', {
          ...availableActions,
          install: { disabled: 'noInstallRule' },
        }),
      );

      expect(screen.getByTestId('detail-install-dir')).toHaveTextContent('/games/local');
      expect(screen.queryByTestId('detail-not-installed')).not.toBeInTheDocument();
      expect(screen.queryByTestId('detail-install-btn')).not.toBeInTheDocument();
      expect(screen.getByTestId('detail-play-btn')).toBeEnabled();
    });
  });

  describe('Steam account banners', () => {
    it('shows the expired-session banner and opens Settings > Stores without blocking the toolbar', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([]);
      snapshot.storeAccounts = [makeStoreAccount('expired')];

      renderSnapshot(<LibraryToolbar />, snapshot);

      expect(screen.getByTestId('banner-store-expired')).toHaveTextContent(
        'Steam sign-in expired. Showing your saved Steam library.',
      );
      expect(screen.queryByTestId('banner-store-unreachable')).not.toBeInTheDocument();
      // The toolbar stays usable behind the banner.
      expect(screen.getByTestId('library-rescan-btn')).toBeInTheDocument();

      fireEvent.click(screen.getByTestId('banner-store-fix-btn'));
      expect(uiStore.state.settingsOpen).toBe(true);
      expect(uiStore.state.settingsTopic).toBe('stores');
    });

    it('shows the unreachable banner with the same action', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([]);
      snapshot.storeAccounts = [makeStoreAccount('unreachable')];

      renderSnapshot(<LibraryToolbar />, snapshot);

      expect(screen.getByTestId('banner-store-unreachable')).toHaveTextContent(
        'Steam cannot be reached. Showing your saved Steam library.',
      );
      expect(screen.queryByTestId('banner-store-expired')).not.toBeInTheDocument();

      fireEvent.click(screen.getByTestId('banner-store-fix-btn'));
      expect(uiStore.state.settingsOpen).toBe(true);
      expect(uiStore.state.settingsTopic).toBe('stores');
    });

    it('shows the expired banner for any provider without a store-specific branch', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([]);
      snapshot.storeAccounts = [makeStoreAccount('expired', 'epic')];

      renderSnapshot(<LibraryToolbar />, snapshot);

      expect(screen.getByTestId('banner-store-expired')).toHaveTextContent(
        'Epic sign-in expired. Showing your saved Epic library.',
      );
      fireEvent.click(screen.getByTestId('banner-store-fix-btn'));
      expect(uiStore.state.settingsOpen).toBe(true);
      expect(uiStore.state.settingsTopic).toBe('stores');
    });

    it('disappears when the account recovers or signs out', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([]);
      snapshot.storeAccounts = [makeStoreAccount('expired')];

      const { rerender, queryClient } = renderSnapshot(<LibraryToolbar />, snapshot);
      expect(screen.getByTestId('banner-store-expired')).toBeInTheDocument();

      act(() => {
        queryClient.setQueryData(SNAPSHOT_QUERY_KEY, {
          ...snapshot,
          storeAccounts: [makeStoreAccount('connected')],
        });
      });
      rerender(
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale="en">
            <LibraryToolbar />
          </I18nProvider>
        </QueryClientProvider>,
      );
      expect(screen.queryByTestId('banner-store-expired')).not.toBeInTheDocument();
      expect(screen.queryByTestId('banner-store-unreachable')).not.toBeInTheDocument();

      act(() => {
        queryClient.setQueryData(SNAPSHOT_QUERY_KEY, {
          ...snapshot,
          storeAccounts: [makeStoreAccount('notConnected')],
        });
      });
      rerender(
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale="en">
            <LibraryToolbar />
          </I18nProvider>
        </QueryClientProvider>,
      );
      expect(screen.queryByTestId('banner-store-expired')).not.toBeInTheDocument();
    });
  });

  describe('Empty states', () => {
    it('explains that nothing is installed and offers to show all games', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([], {
        layout: { filters: { ...createDefaultFilters(), availability: { installed: true, owned: false, family: false } } },
        total: 328,
        installedTotal: 0,
        empty: { noInstalled: { provider: 'steam' } },
      });

      renderSnapshot(<LibraryScreen />, snapshot);

      expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
        'Nothing is installed on this PC.',
      );
      expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
        'Games you own or share are hidden by the Installed filter.',
      );
      fireEvent.click(screen.getByTestId('library-empty-show-all-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith({
        setFilters: createDefaultFilters(),
      });
    });

    it('keeps the no-games state with stores, Rescan and Add game on Windows', () => {
      const rescanSpy = vi.spyOn(commands, 'rescanLibrary');
      const addGameSpy = vi.spyOn(commands, 'pickLocalExecutable');
      const originalUserAgent = window.navigator.userAgent;
      Object.defineProperty(window.navigator, 'userAgent', {
        value: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
        configurable: true,
      });

      try {
        const snapshot = createDefaultSnapshot();
        snapshot.library = makeLibrary([], {
          total: 0,
          installedTotal: 0,
          empty: { noGames: { stores: ['Steam', 'GOG'] } },
        });

        renderSnapshot(<LibraryScreen />, snapshot);

        expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
          'No games found.',
        );
        expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
          'Looked in: Steam, GOG',
        );
        expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
          'without choosing a GameNode match first',
        );
        fireEvent.click(screen.getByTestId('library-empty-rescan-btn'));
        expect(rescanSpy).toHaveBeenCalled();
        fireEvent.click(screen.getByTestId('library-empty-add-game-btn'));
        expect(addGameSpy).toHaveBeenCalledWith(null);
      } finally {
        Object.defineProperty(window.navigator, 'userAgent', {
          value: originalUserAgent,
          configurable: true,
        });
      }
    });

    it('omits Add game outside Windows and keeps a separate search-empty state', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([], {
        total: 0,
        installedTotal: 0,
        empty: { noGames: { stores: ['Steam'] } },
      });

      const first = renderSnapshot(<LibraryScreen />, snapshot);
      expect(screen.queryByTestId('library-empty-add-game-btn')).not.toBeInTheDocument();
      first.unmount();

      const searchSnapshot = createDefaultSnapshot();
      searchSnapshot.library = makeLibrary([], {
        empty: 'nothingMatches',
        search: 'zzz',
      });
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      renderSnapshot(<LibraryScreen />, searchSnapshot);

      expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
        'No games match.',
      );
      expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
        'Nothing in this view matches your search or filters.',
      );
      fireEvent.click(screen.getByTestId('library-empty-clear-search-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSearch: '' });
    });
  });

  describe('Large mock library', () => {
    it('serves 300+ owned Steam games, 18 installed, family-shared games and an expired variant', async () => {
      const host = createMockHost({ largeLibrary: true, storeAccount: 'expired' });
      const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
      const rows = snapshot.library!.rows;
      const steam = snapshot.library!.providers.find((provider) => provider.id === 'steam')!;

      expect(rows.length).toBeGreaterThanOrEqual(300);
      expect(steam.ownedCount).toBeGreaterThanOrEqual(300);
      expect(steam.installedCount).toBe(18);
      expect(steam.familyCount).toBeGreaterThan(0);
      expect(snapshot.library!.total).toBe(rows.length);
      expect(snapshot.library!.installedTotal).toBe(18);
      expect(snapshot.storeAccounts[0].state).toBe('expired');

      await host.invoke('library_input', {
        input: {
          setFilters: {
            ...createDefaultFilters(),
            availability: { installed: true, owned: false, family: false },
          },
        },
      });
      const filtered = (await host.invoke('get_snapshot')) as Snapshot;
      expect(filtered.library!.rows).toHaveLength(18);
      expect(filtered.library!.rows.every((row) => row.installed)).toBe(true);
      expect(filtered.library!.total).toBe(rows.length);
    });
  });
  describe('Sort', () => {
    it('starts every key at its natural direction', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([makeRow({ key: 'steam:1', title: 'Game' })]);
      renderSnapshot(<LibraryToolbar />, snapshot);

      const select = screen.getByTestId('library-sort-select');
      fireEvent.change(select, { target: { value: 'playtime' } });
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setSort: { key: 'playtime', direction: 'descending' },
      });

      fireEvent.change(select, { target: { value: 'releaseYear' } });
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setSort: { key: 'releaseYear', direction: 'descending' },
      });

      fireEvent.change(select, { target: { value: 'title' } });
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setSort: { key: 'title', direction: 'ascending' },
      });
    });

    it('flips the direction and names the resulting order for the key', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([makeRow({ key: 'steam:1', title: 'Game' })], {
        layout: {
          sort: { key: 'lastPlayed', direction: 'descending' },
        },
      });
      renderSnapshot(<LibraryToolbar />, snapshot);

      const button = screen.getByTestId('library-sort-direction-btn');
      expect(button).toHaveAccessibleName('Recently played first');
      fireEvent.click(button);
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setSort: { key: 'lastPlayed', direction: 'ascending' },
      });
    });

    it('uses the order each key family asks for', () => {
      type MakeLibraryOptions = NonNullable<Parameters<typeof makeLibrary>[1]>;
      const cases: Array<[MakeLibraryOptions['layout'], string]> = [
        [{ sort: { key: 'title', direction: 'ascending' } }, 'A → Z'],
        [{ sort: { key: 'title', direction: 'descending' } }, 'Z → A'],
        [{ sort: { key: 'matchStatus', direction: 'ascending' } }, 'Best match first'],
        [{ sort: { key: 'releaseYear', direction: 'descending' } }, 'Newest first'],
        [{ sort: { key: 'releaseYear', direction: 'ascending' } }, 'Oldest first'],
        [{ sort: { key: 'playtime', direction: 'ascending' } }, 'Least played first'],
        [{ sort: { key: 'dateAdded', direction: 'descending' } }, 'Newest added first'],
      ];
      for (const [layout, label] of cases) {
        const snapshot = createDefaultSnapshot();
        snapshot.library = makeLibrary([makeRow({ key: 'steam:1', title: 'Game' })], { layout });
        const { unmount } = renderSnapshot(<LibraryToolbar />, snapshot);
        expect(screen.getByTestId('library-sort-direction-btn')).toHaveAccessibleName(label);
        unmount();
      }
    });
  });

  describe('Favorites', () => {
    it('toggles the favorites filter from the toolbar', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([makeRow({ key: 'steam:1', title: 'Game' })]);
      renderSnapshot(<LibraryToolbar />, snapshot);

      const toggle = screen.getByTestId('library-favorites-filter');
      expect(toggle).toHaveAttribute('aria-pressed', 'false');
      fireEvent.click(toggle);
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setFilters: { ...createDefaultFilters(), favoritesOnly: true },
      });
    });

    it('marks favorited games in the grid and the list', () => {
      const favorite = makeRow({
        key: 'steam:1',
        title: 'Favorite',
        status: { matched: { gameId: 7, source: 'automatic' } },
        isFavorite: true,
      });
      const other = makeRow({
        key: 'steam:2',
        title: 'Other',
        status: { matched: { gameId: 8, source: 'automatic' } },
      });
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([favorite, other]);

      const grid = renderSnapshot(<GameGrid />, snapshot);
      expect(screen.getByTestId('game-tile-favorite-steam:1')).toHaveAttribute(
        'aria-label',
        'Favorite',
      );
      expect(screen.queryByTestId('game-tile-favorite-steam:2')).not.toBeInTheDocument();
      grid.unmount();

      snapshot.library = makeLibrary([favorite, other], { layout: { view: 'list' } });
      renderSnapshot(<GameList />, snapshot);
      expect(screen.getByTestId('game-row-favorite-steam:1')).toHaveAttribute(
        'aria-label',
        'Favorite',
      );
      expect(screen.queryByTestId('game-row-favorite-steam:2')).not.toBeInTheDocument();
    });

    it('toggles from the detail pane with the command payload', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const row = makeRow({
        key: 'steam:1',
        title: 'Favorite',
        status: { matched: { gameId: 7, source: 'automatic' } },
      });
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([row], {
        detail: {
          running: false,
          row,
          installDir: '/games/favorite',
          releaseYear: 2011,
          hasArtwork: false,
          membership: 'notInLibrary',
        },
      });
      renderSnapshot(<DetailPane />, snapshot);

      const button = screen.getByTestId('detail-favorite-btn');
      expect(button).toHaveAttribute('aria-pressed', 'false');
      expect(button).toHaveTextContent('Add to favorites');
      fireEvent.click(button);
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setFavorite: { key: 'steam:1', favorite: true, now: expect.any(Number) },
      });
    });

    it('disables the favorite action and explains why for an unmatched entry', () => {
      const row = makeRow({ key: 'steam:1', title: 'Unmatched', status: 'unmatched' });
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([row], {
        detail: {
          running: false,
          row,
          installDir: '/games/unmatched',
          releaseYear: null,
          hasArtwork: false,
          membership: 'notChecked',
        },
      });
      renderSnapshot(<DetailPane />, snapshot);

      expect(screen.getByTestId('detail-favorite-btn')).toBeDisabled();
      expect(screen.getByTestId('detail-favorite-reason')).toHaveTextContent(
        'Match the game first to use favorites.',
      );
    });

    it('offers the context menu action only for one matched entry', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const matched = makeRow({
        key: 'steam:1',
        title: 'Matched',
        status: { matched: { gameId: 7, source: 'automatic' } },
      });
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([matched]);
      renderSnapshot(<ContextMenu />, snapshot);

      act(() => openContextMenu(10, 10, 'steam:1'));
      const action = screen.getByTestId('context-menu-favorite');
      expect(action).toHaveTextContent('Add to favorites');
      fireEvent.click(action);
      expect(libraryInputSpy).toHaveBeenLastCalledWith({
        setFavorite: { key: 'steam:1', favorite: true, now: expect.any(Number) },
      });
    });

    it('omits the context menu action for unmatched and multi-selected rows', () => {
      const matched = makeRow({
        key: 'steam:1',
        title: 'Matched',
        status: { matched: { gameId: 7, source: 'automatic' } },
      });
      const unmatched = makeRow({ key: 'steam:2', title: 'Unmatched', status: 'unmatched' });
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([matched, unmatched]);
      renderSnapshot(<ContextMenu />, snapshot);

      act(() => openContextMenu(10, 10, 'steam:2'));
      expect(screen.queryByTestId('context-menu-favorite')).not.toBeInTheDocument();

      act(() => {
        uiStore.setState((prev) => ({
          ...prev,
          contextMenu: { ...prev.contextMenu, open: false },
        }));
        snapshot.library!.selected = ['steam:1', 'steam:2'];
      });
      act(() => openContextMenu(10, 10, 'steam:1'));
      expect(screen.queryByTestId('context-menu-favorite')).not.toBeInTheDocument();
    });

    it('keeps the same favorite through the mock host', async () => {
      const host = createMockHost();
      await host.invoke('library_input', {
        input: { setFavorite: { key: 'steam:1', favorite: true, now: 1 } },
      });
      const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
      const row = snapshot.library!.rows.find((item) => item.key === 'steam:1');
      expect(row?.isFavorite).toBe(true);
    });
  });

});
