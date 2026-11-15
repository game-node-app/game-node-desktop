import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  createDefaultLibraryView,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Actions,
  type LibraryView,
  type Row,
  type Snapshot,
  createDefaultFilters,
} from '../../api';
import { I18nProvider } from '../../i18n';
import { resetUiStore } from '../../store/uiStore';
import { createMockHost } from '../../mock/host';
import { DetailPane } from './DetailPane';
import { GameGrid } from './GameGrid';
import { GameList } from './GameList';

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
    shared: false,
    running: false,
    sessionStartedAt: null,
    status: 'unmatched',
    coverFile: null,
    playtime: { seconds: null, source: 'store', lastPlayedAt: null },
    releaseYear: null,
    addedAt: 0,
    isFavorite: false,
    hidden: false,
    gamenode: null,
    ...overrides,
  };
}

function makeLibrary(
  rows: Row[],
  overrides: Partial<LibraryView> = {},
): LibraryView {
  return {
    ...createDefaultLibraryView(),
    generation: 1,
    layout: {
      view: 'grid',
      sort: { key: 'title', direction: 'ascending' },
      scope: 'all',
      group: 'none',
      collapsedGroups: {},
      filters: createDefaultFilters(),
      sidebarWidth: 240,
      detailWidth: 360,
      detailOpen: false,
      sidebarCollapsed: false,
    },
    search: '',
    access: 'verified',
    total: rows.length,
    installedTotal: rows.filter((row) => row.installed).length,
    providers: [
      {
        id: 'steam',
        name: 'Steam',
        count: rows.length,
        installedCount: rows.filter((row) => row.installed).length,
        ownedCount: rows.filter((row) => row.owned).length,
        sharedCount: rows.filter((row) => row.shared && !row.owned).length,
        sharedLabel: 'Steam Family',
      },
    ],
    rows,
    scanning: false,
    failedProviders: [],
    selected: rows.length ? [rows[0].key] : [],
    focused: rows.length ? rows[0].key : null,
    actions: availableActions,
    ...overrides,
  };
}

function renderSnapshot(ui: React.ReactElement, snapshot: Snapshot, locale = 'en') {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('Steam library UI', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  describe('Grid and list indicators', () => {
    const rows = [
      makeRow({ key: 'steam:1', title: 'Installed Owned', installed: true, owned: true }),
      makeRow({ key: 'steam:2', title: 'Owned Only', installed: false, owned: true }),
      makeRow({ key: 'steam:3', title: 'Shared Game', owned: false, shared: true }),
    ];

    it('marks installed and family-shared games in the grid without colour alone', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary(rows);

      renderSnapshot(<GameGrid />, snapshot);

      const installed = screen.getByTestId('game-tile-steam:1');
      expect(installed).toHaveTextContent('Installed');
      expect(installed).toHaveAttribute('aria-label', expect.stringContaining('Installed'));

      expect(screen.getByTestId('game-tile-steam:2')).not.toHaveTextContent('Installed');
      expect(screen.getByTestId('game-tile-steam:3')).toHaveTextContent('Family');
      expect(screen.getByTestId('game-tile-steam:1')).not.toHaveTextContent('Family');
    });

    it('marks installed and family-shared games in the list', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary(rows, {
        layout: { ...createDefaultLibraryView().layout, view: 'list' },
      });

      renderSnapshot(<GameList />, snapshot);

      expect(screen.getByTestId('game-row-steam:1')).toHaveTextContent('Installed');
      expect(screen.getByTestId('game-row-steam:2')).not.toHaveTextContent('Installed');
      expect(screen.getByTestId('game-row-steam:3')).toHaveTextContent('Family shared');
    });

    it('keeps the declared Steam Family label untranslated in Brazilian Portuguese', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary([rows[2]]);

      renderSnapshot(<GameGrid />, snapshot, 'pt-BR');

      expect(screen.getByTestId('game-tile-steam:3')).toHaveTextContent('Família');
    });
  });

  describe('Detail pane ownership states', () => {
    function detailSnapshot(row: Row, installDir: string): Snapshot {
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
          membership: 'notInLibrary',
          achievementBackend: false,
          achievements: null,
          suggestions: [],
        },
        layout: {
          ...createDefaultLibraryView().layout,
          detailOpen: true,
        },
      });
      return snapshot;
    }

    it('replaces the install folder with a not-installed statement and keeps Install/Play', () => {
      const row = makeRow({
        key: 'steam:620',
        title: 'Portal 2',
        installed: false,
        owned: true,
      });

      renderSnapshot(<DetailPane />, detailSnapshot(row, ''));

      expect(screen.getByTestId('detail-not-installed')).toHaveTextContent(
        'Not installed on this PC',
      );
      expect(screen.queryByTestId('detail-install-dir')).not.toBeInTheDocument();
      expect(screen.getByTestId('detail-install-btn')).toBeEnabled();
      expect(screen.getByTestId('detail-play-btn')).toBeEnabled();
    });

    it('identifies shared games in the detail pane through the declared label', () => {
      const row = makeRow({
        key: 'steam:700',
        title: 'Shared Adventure',
        owned: false,
        shared: true,
      });

      renderSnapshot(<DetailPane />, detailSnapshot(row, ''));

      expect(screen.getByTestId('detail-family-note')).toHaveTextContent(
        'This game comes from Steam Family.',
      );
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
      expect(screen.queryByTestId('detail-family-note')).not.toBeInTheDocument();
    });
  });

  describe('Mock host parity', () => {
    it('serves 300+ owned Steam games, 18 installed, shared games and an expired variant', async () => {
      const host = createMockHost({ largeLibrary: true, storeAccount: 'expired' });
      const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
      const rows = snapshot.library!.rows;
      const steam = snapshot.library!.providers.find((provider) => provider.id === 'steam')!;

      expect(rows.length).toBeGreaterThanOrEqual(300);
      expect(steam.ownedCount).toBeGreaterThanOrEqual(300);
      expect(steam.installedCount).toBe(18);
      expect(steam.sharedCount).toBeGreaterThan(0);
      expect(steam.sharedLabel).toBe('Steam Family');
      expect(snapshot.library!.total).toBe(rows.length);
      expect(snapshot.library!.installedTotal).toBe(18);
      expect(snapshot.storeAccounts[0].state).toBe('expired');

      await host.invoke('library_input', {
        input: {
          setFilters: {
            ...createDefaultFilters(),
            availability: { installed: true, owned: false, shared: false },
          },
        },
      });
      const filtered = (await host.invoke('get_snapshot')) as Snapshot;
      expect(filtered.library!.rows).toHaveLength(18);
      expect(filtered.library!.rows.every((row) => row.installed)).toBe(true);
      expect(filtered.library!.total).toBe(rows.length);
    });

    it('round-trips filters through the mock layout', async () => {
      const host = createMockHost();
      const installed = {
        ...createDefaultFilters(),
        availability: { installed: true, owned: false, shared: false },
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
});
