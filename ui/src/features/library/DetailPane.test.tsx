import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { createAppQueryClient } from '../../AppProviders';
import {
  commands,
  createDefaultFilters,
  createDefaultLibraryView,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Actions,
  type BackupVersion,
  type CollectionCount,
  type DetailView,
  type LibraryView,
  type Registration,
  type Row,
  type SaveProfile,
  type Snapshot,
} from '../../api';
import { I18nProvider } from '../../i18n';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { DetailPane } from './DetailPane';

function createActions(overrides: Partial<Actions> = {}): Actions {
  return {
    selected: 1,
    play: 'available',
    install: 'available',
    matchGame: 'available',
    addToGamenode: 'available',
    changeMatch: false,
    ...overrides,
  };
}

function createRow(overrides: Partial<Row> = {}): Row {
  return {
    key: 'steam:620',
    title: 'Emberfall: Iron Trail',
    detectedName: 'Emberfall: Iron Trail',
    matchedName: 'Emberfall: Iron Trail',
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    running: false,
    status: { matched: { gameId: 42, source: 'automatic' } },
    coverFile: 'mock-1.svg',
    playtime: {
      seconds: 42 * 3600 + 18 * 60,
      source: 'store',
      lastPlayedAt: 1_760_000_000,
    },
    releaseYear: 2019,
    addedAt: 1_750_000_000,
    isFavorite: true,
    hidden: false,
    gamenode: {
      status: 'playing',
      collectionIds: ['backlog-curto'],
      entryId: 'entry-42',
    },
    ...overrides,
  };
}

function createDetail(overrides: Partial<DetailView> = {}): DetailView {
  const row = overrides.row ?? createRow();
  return {
    running: false,
    row,
    installDir: 'D:\\Steam\\steamapps\\common\\Emberfall',
    releaseYear: 2019,
    hasArtwork: false,
    membership: { inLibrary: 'playing' },
    achievementBackend: true,
    achievements: {
      state: 'ready',
      unlocked: 31,
      total: 58,
      latestUnlock: null,
      lastSuccessAt: 1_760_000_100,
      failure: null,
    },
    suggestions: [],
    ...overrides,
  };
}

function collections(): CollectionCount[] {
  return [
    { id: 'backlog-curto', name: 'Backlog curto', count: 3 },
    { id: 'coop-no-sofa', name: 'Coop no sofá', count: 1 },
  ];
}

function createLibrary(
  rows: Row[],
  detail: DetailView | null,
  options: {
    selected?: string[];
    actions?: Actions;
    collections?: CollectionCount[];
  } = {},
): LibraryView {
  return {
    ...createDefaultLibraryView(),
    generation: 1,
    layout: {
      ...createDefaultLibraryView().layout,
      filters: createDefaultFilters(),
      detailOpen: true,
      detailWidth: 360,
    },
    total: rows.length,
    installedTotal: rows.filter((row) => row.installed).length,
    providers: [
      {
        id: 'steam',
        name: 'Steam',
        count: rows.filter((row) => row.provider === 'steam').length,
        installedCount: rows.filter((row) => row.provider === 'steam' && row.installed).length,
        ownedCount: rows.filter((row) => row.provider === 'steam' && row.owned).length,
        sharedCount: rows.filter((row) => row.provider === 'steam' && row.shared).length,
        sharedLabel: 'Steam Family',
      },
    ],
    rows,
    selected: options.selected ?? (detail ? [detail.row.key] : []),
    focused: detail?.row.key ?? null,
    actions: options.actions ?? createActions(),
    detail,
    collections: options.collections ?? collections(),
  };
}

function createSnapshot(library: LibraryView): Snapshot {
  const snapshot = createDefaultSnapshot();
  snapshot.library = library;
  return snapshot;
}

function renderDetail(snapshot: Snapshot, locale = 'en'): ReturnType<typeof render> {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  return render(
    <QueryClientProvider client={queryClient}>
      <MantineProvider>
        <I18nProvider initialLocale={locale}>
          <DetailPane />
        </I18nProvider>
      </MantineProvider>
    </QueryClientProvider>,
  );
}

function drmFreeRegistration(): Registration {
  return {
    id: 'local-1',
    title: 'Distant Skies',
    executable: 'D:\\Jogos\\DistantSkies\\skies.exe',
    fileIdentity: 'skies',
    root: 'D:\\Jogos\\DistantSkies',
    workingDirectory: 'D:\\Jogos\\DistantSkies',
    arguments: ['--windowed', '--fps', '120'],
    gameplay: [],
    launchers: [],
    revision: 1,
    removed: false,
    rediscoveryAllowed: false,
    transferredFrom: null,
    available: true,
  };
}

function drmFreeProfile(): SaveProfile {
  return {
    id: 'profile-1',
    installation: 'local-1',
    revision: 1,
    title: 'Distant Skies',
    manifestHash: 'hash',
    definition: 'definition',
    files: ['%APPDATA%\\DistantSkies'],
    registry: [],
    support: 'supported',
    automatic: true,
  };
}

function backupVersion(id: string): BackupVersion {
  return {
    installation: 'local-1',
    profile: 'profile-1',
    destination: 'D:\\Backups',
    id,
    label: `Backup ${id}`,
    bytes: 1024,
  };
}

describe('DetailPane', () => {
  it('requires approval before a detected launch target can be used', async () => {
    const reviewSpy = vi.spyOn(commands, 'reviewDetectedTarget').mockResolvedValue(undefined);
    const detail = createDetail({
      detectedTarget: {
        spec: {
          executable: {
            root: 'C:\\Games\\Hades',
            path: 'Hades.exe',
            arguments: [],
            workingDirectory: '',
          },
        },
        revision: 3,
        approved: false,
        available: true,
      },
    });
    renderDetail(createSnapshot(createLibrary([detail.row], detail)));
    expect(screen.getByTestId('detected-target-review')).toHaveTextContent('Hades.exe');
    fireEvent.click(screen.getByTestId('approve-detected-target'));
    await waitFor(() => expect(reviewSpy).toHaveBeenCalledWith(detail.row.key, 3));
  });

  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('stops only supervised launches for the selected entry', async () => {
    const user = userEvent.setup();
    const list = vi.spyOn(commands, 'customLaunchIds').mockResolvedValue([41, 42]);
    const stop = vi.spyOn(commands, 'customLaunchStop').mockResolvedValue(undefined);
    renderDetail(createSnapshot(createLibrary([createRow()], createDetail())));
    await user.click(screen.getByTestId('detail-more-btn'));
    const action = await screen.findByRole('menuitem', { name: 'Stop custom launch' });
    expect(list).toHaveBeenCalledWith('steam:620');
    await user.click(action);
    await waitFor(() => expect(stop).toHaveBeenCalledTimes(2));
    expect(stop).toHaveBeenCalledWith(41);
    expect(stop).toHaveBeenCalledWith(42);
  });

  it('reports when termination cannot be confirmed', async () => {
    const user = userEvent.setup();
    vi.spyOn(commands, 'customLaunchIds').mockResolvedValue([41]);
    vi.spyOn(commands, 'customLaunchStop').mockRejectedValue(new Error('unknown state'));
    renderDetail(createSnapshot(createLibrary([createRow()], createDetail())));
    await user.click(screen.getByTestId('detail-more-btn'));
    await user.click(await screen.findByRole('menuitem', { name: 'Stop custom launch' }));
    expect(await screen.findByText('Could not confirm that the launch stopped. Try again.')).toBeInTheDocument();
  });

  it('shows the matched Steam sections and sends the single-game status change', async () => {
    const libraryInput = vi.spyOn(commands, 'libraryInput');
    const openPage = vi.spyOn(commands, 'openGameNodePage');
    const snapshot = createSnapshot(createLibrary([createRow()], createDetail()));
    snapshot.tools.installedByEntry['steam:620'] = [
      {
        toolId: 'optiscaler',
        name: 'OptiScaler',
        channel: 'stable',
        version: '0.7.9',
        proxyDll: 'dxgi',
        targetExe: 'game.exe',
        state: 'installed',
        summary: '',
      },
    ];

    renderDetail(snapshot);

    expect(screen.getByTestId('detail-artwork-container')).toBeInTheDocument();
    expect(screen.getByTestId('detail-title')).toHaveTextContent('Emberfall: Iron Trail');
    const hero = screen.getByTestId('detail-artwork-container');
    expect(within(hero).getByText('Steam · 2019')).toBeInTheDocument();

    expect(screen.getByTestId('detail-pill-installed')).toHaveTextContent(
      'Installed on this PC',
    );
    expect(screen.getByTestId('detail-pill-status')).toHaveTextContent(
      'Playing on GameNode',
    );
    expect(screen.getByTestId('detail-pill-match')).toHaveTextContent('Matched');

    const stats = screen.getByTestId('detail-stats');
    expect(within(stats).getByText('42 h 18 min')).toBeInTheDocument();
    expect(within(stats).getByText('Steam counter')).toBeInTheDocument();
    expect(within(stats).getByText('31 of 58')).toBeInTheDocument();

    const gamenode = screen.getByTestId('detail-gamenode');
    expect(within(gamenode).getByTestId('detail-status-select')).toHaveTextContent(
      'Playing',
    );
    expect(within(gamenode).getByText('Backlog curto')).toBeInTheDocument();
    expect(within(gamenode).getByTestId('detail-edit-gamenode')).toHaveTextContent(
      'Edit on GameNode',
    );

    const match = screen.getByTestId('detail-match');
    expect(within(match).getByTestId('detail-match-name')).toHaveTextContent(
      'Emberfall: Iron Trail',
    );
    expect(
      within(match).getByText('Matched automatically by title and year'),
    ).toBeInTheDocument();
    expect(within(screen.getByTestId('detail-achievements')).getByText('See all')).toBeInTheDocument();

    const onThisPc = screen.getByTestId('detail-on-this-pc');
    expect(within(onThisPc).getByTestId('detail-install-dir')).toHaveTextContent(
      'D:\\Steam\\steamapps\\common\\Emberfall',
    );
    expect(within(onThisPc).getByTestId('detail-playtime-counter')).toHaveTextContent(
      'Steam',
    );
    expect(within(onThisPc).getByTestId('detail-tools-summary')).toHaveTextContent(
      'OptiScaler 0.7.9 (dxgi.dll)',
    );

    fireEvent.click(within(gamenode).getByTestId('detail-status-select'));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Finished' }));
    expect(libraryInput).toHaveBeenCalledWith({
      setStatus: { keys: ['steam:620'], status: 'finished' },
    });

    fireEvent.click(within(gamenode).getByTestId('detail-edit-gamenode'));
    expect(openPage).toHaveBeenCalledWith({ game: { id: 42 } });
  });

  it('toggles favorite, resizes and closes the pane', () => {
    const libraryInput = vi.spyOn(commands, 'libraryInput');
    const snapshot = createSnapshot(createLibrary([createRow()], createDetail()));

    renderDetail(snapshot);

    const favorite = screen.getByTestId('detail-favorite-btn');
    expect(favorite).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(favorite);
    expect(libraryInput).toHaveBeenCalledWith({
      setFavorite: { key: 'steam:620', favorite: false, now: expect.any(Number) },
    });

    fireEvent.keyDown(screen.getByTestId('detail-resize-handle'), { key: 'ArrowLeft' });
    expect(libraryInput).toHaveBeenCalledWith({ setDetailWidth: 376 });

    fireEvent.click(screen.getByTestId('detail-close-btn'));
    expect(libraryInput).toHaveBeenCalledWith({ setDetailOpen: false });
  });

  it('offers the existing more-actions menu', async () => {
    const user = userEvent.setup();
    const openFolder = vi.spyOn(commands, 'openInstallFolder');
    const snapshot = createSnapshot(createLibrary([createRow()], createDetail()));

    renderDetail(snapshot);

    await user.click(screen.getByTestId('detail-more-btn'));
    expect(
      await screen.findByRole('menuitem', { name: 'Open install folder' }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole('menuitem', { name: 'Open install folder' }));
    expect(openFolder).toHaveBeenCalledWith(
      'D:\\Steam\\steamapps\\common\\Emberfall',
    );

    await user.click(screen.getByTestId('detail-more-btn'));
    expect(
      await screen.findByRole('menuitem', { name: 'View achievements' }),
    ).toBeInTheDocument();
  });

  it('shows an unknown running session without inventing a zero', () => {
    const row = createRow({ running: true, sessionStartedAt: null });
    const detail = createDetail({ row, running: true });
    const snapshot = createSnapshot(
      createLibrary([row], detail, {
        actions: createActions({ play: { disabled: 'running' } }),
        selected: [row.key],
      }),
    );

    renderDetail(snapshot);

    expect(screen.getByTestId('detail-running-indicator')).toHaveTextContent(
      /^Running$/,
    );
    const stats = screen.getByTestId('detail-stats');
    expect(within(stats).getByText('Session')).toBeInTheDocument();
    expect(within(stats).getByText('—')).toBeInTheDocument();
    expect(within(stats).queryByText('0:00:00')).not.toBeInTheDocument();
  });

  it('renders a family-shared game as not installed with its Steam Family note', () => {
    const row = createRow({
      key: 'steam:700',
      title: 'Shared Adventure',
      installed: false,
      owned: false,
      shared: true,
      status: { matched: { gameId: 77, source: 'automatic' } },
      gamenode: null,
    });
    const detail = createDetail({
      row,
      membership: 'notInLibrary',
      releaseYear: 2021,
      installDir: '',
      achievementBackend: false,
      achievements: null,
    });
    const actions = createActions({
      play: { disabled: 'noLaunchRule' },
      install: 'available',
      addToGamenode: 'available',
    });

    renderDetail(createSnapshot(createLibrary([row], detail, { actions })));

    expect(screen.getByTestId('detail-pill-not-installed')).toHaveTextContent(
      'Not installed',
    );
    expect(screen.getByTestId('detail-pill-family')).toHaveTextContent('Family shared');
    expect(screen.getByTestId('detail-shared-label')).toHaveTextContent(
      'Shared through Steam Family',
    );
    expect(screen.getByTestId('detail-family-note')).toHaveTextContent(
      'This game comes from Steam Family.',
    );
    expect(screen.getByTestId('detail-install-btn')).toBeEnabled();
    expect(screen.getByTestId('detail-not-installed')).toHaveTextContent(
      'Not installed on this PC',
    );
    expect(screen.queryByTestId('detail-install-dir')).not.toBeInTheDocument();
  });

  it('shows the DRM-free launch configuration and save protection without Install', () => {
    const row = createRow({
      key: 'manual:local-1',
      title: 'Distant Skies',
      detectedName: 'Distant Skies',
      matchedName: null,
      provider: 'manual',
      providerName: 'DRM-free',
      installed: true,
      owned: false,
      status: 'unmatched',
      coverFile: null,
      isFavorite: false,
      gamenode: null,
    });
    const detail = createDetail({
      row,
      hasArtwork: false,
      membership: 'notInLibrary',
      achievementBackend: false,
      achievements: null,
      installDir: 'D:\\Jogos\\DistantSkies',
      suggestions: [],
    });
    const actions = createActions({
      install: { disabled: 'noInstallRule' },
      addToGamenode: { disabled: 'notMatched' },
    });
    const snapshot = createSnapshot(
      createLibrary([row], detail, { actions, selected: [row.key] }),
    );
    snapshot.localGames.registrations = [drmFreeRegistration()];
    snapshot.localGames.profiles = [drmFreeProfile()];
    snapshot.localGames.versions = [backupVersion('a'), backupVersion('b')];

    renderDetail(snapshot);

    expect(screen.getByTestId('detail-pill-drmfree')).toHaveTextContent('DRM-free');
    expect(screen.getByTestId('detail-pill-protection')).toHaveTextContent(
      'Saves protected',
    );
    expect(screen.getByTestId('detail-play-btn')).toBeEnabled();
    expect(screen.queryByTestId('detail-install-btn')).not.toBeInTheDocument();

    expect(screen.getByTestId('detail-executable')).toHaveTextContent(
      'D:\\Jogos\\DistantSkies\\skies.exe',
    );
    expect(screen.getByTestId('detail-arguments')).toHaveTextContent(
      '--windowed --fps 120',
    );
    expect(screen.getByTestId('detail-save-folder')).toHaveTextContent(
      '%APPDATA%\\DistantSkies',
    );
    expect(screen.getByTestId('detail-save-protection-btn')).toHaveTextContent(
      'Active',
    );

    fireEvent.click(screen.getByTestId('detail-remove-btn'));
    expect(uiStore.state.removeModalEntryKey).toBe('manual:local-1');
  });

  it('shows a live session clock for a running DRM-free game', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_760_000_000_000);
    const startedAt = 1_760_000_000 - 2530;
    const row = createRow({
      key: 'manual:local-2',
      title: 'Running Local',
      detectedName: 'Running Local',
      matchedName: null,
      provider: 'manual',
      providerName: 'DRM-free',
      installed: true,
      owned: false,
      running: true,
      sessionStartedAt: startedAt,
      status: 'noGame',
      coverFile: null,
      isFavorite: false,
      gamenode: null,
    });
    const detail = createDetail({
      row,
      running: true,
      membership: 'notInLibrary',
      achievementBackend: false,
      achievements: null,
      installDir: 'D:\\Jogos\\RunningLocal',
      suggestions: [],
    });
    const actions = createActions({
      play: { disabled: 'running' },
      install: { disabled: 'noInstallRule' },
      addToGamenode: { disabled: 'notMatched' },
    });
    const snapshot = createSnapshot(
      createLibrary([row], detail, { actions, selected: [row.key] }),
    );
    snapshot.localGames.registrations = [
      { ...drmFreeRegistration(), id: 'local-2', title: 'Running Local', root: 'D:\\Jogos\\RunningLocal' },
    ];
    snapshot.localGames.profiles = [
      { ...drmFreeProfile(), installation: 'local-2', title: 'Running Local' },
    ];

    renderDetail(snapshot);

    expect(screen.getByTestId('detail-running-indicator')).toHaveTextContent(
      'Running · 0:42:10',
    );
    expect(screen.queryByTestId('detail-play-btn')).not.toBeInTheDocument();
    const stats = screen.getByTestId('detail-stats');
    expect(within(stats).getByText('Session')).toBeInTheDocument();
    expect(within(stats).getByText('0:42:10')).toBeInTheDocument();
    expect(within(stats).getByText(/^Started /)).toBeInTheDocument();
    expect(screen.getByTestId('detail-pill-protection')).toHaveTextContent(
      'Saves protected',
    );
  });

  it('keeps the session clock across rerenders and ticks it every second', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_760_000_000_000);
    const row = createRow({
      running: true,
      sessionStartedAt: 1_760_000_000 - 2530,
    });
    const detail = createDetail({ row, running: true });
    const snapshot = createSnapshot(
      createLibrary([row], detail, { actions: createActions({ play: { disabled: 'running' } }), selected: [row.key] }),
    );
    const queryClient = createAppQueryClient();
    queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
    const tree = (
      <QueryClientProvider client={queryClient}>
        <MantineProvider>
          <I18nProvider initialLocale="en">
            <DetailPane />
          </I18nProvider>
        </MantineProvider>
      </QueryClientProvider>
    );

    const { rerender } = render(tree);
    expect(screen.getByTestId('detail-running-indicator')).toHaveTextContent(
      '0:42:10',
    );

    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    expect(screen.getByTestId('detail-running-indicator')).toHaveTextContent(
      '0:42:15',
    );

    // A new snapshot for the same execution does not restart the clock.
    queryClient.setQueryData(SNAPSHOT_QUERY_KEY, {
      ...snapshot,
      library: { ...snapshot.library!, generation: 2 },
    });
    rerender(tree);
    expect(screen.getByTestId('detail-running-indicator')).toHaveTextContent(
      '0:42:15',
    );
  });

  it('renders suggestions for an unmatched game and sends Use with the real key and game', async () => {
    const libraryInput = vi.spyOn(commands, 'libraryInput');
    const row = createRow({
      key: 'manual:hollow',
      title: 'hollow_knight',
      detectedName: 'hollow_knight',
      matchedName: null,
      provider: 'manual',
      providerName: 'DRM-free',
      installed: true,
      owned: false,
      status: 'unmatched',
      coverFile: null,
      isFavorite: false,
      gamenode: null,
    });
    const detail = createDetail({
      row,
      membership: 'notInLibrary',
      achievementBackend: false,
      achievements: null,
      suggestions: [
        {
          gameId: 77,
          title: 'Neon Courier 2088',
          year: 2023,
          coverUrl: null,
          confidence: 92,
          tone: 'success',
        },
        {
          gameId: 78,
          title: 'Neon Courier',
          year: 2019,
          coverUrl: null,
          confidence: 61,
          tone: 'warning',
        },
      ],
    });
    const actions = createActions({
      install: { disabled: 'noInstallRule' },
      addToGamenode: { disabled: 'notMatched' },
      changeMatch: false,
    });

    renderDetail(
      createSnapshot(createLibrary([row], detail, { actions, selected: [row.key] })),
    );

    expect(screen.getByTestId('detail-match-warning')).toHaveTextContent(
      'No match on GameNode',
    );
    expect(screen.getByTestId('detail-match-warning')).toHaveTextContent(
      'Match to sync status, collections and playtime with your profile.',
    );
    expect(screen.getByTestId('detail-add-reason')).toHaveTextContent(
      'Match this game to a GameNode game first.',
    );
    expect(screen.getByTestId('detail-add-btn')).toBeDisabled();

    const suggestions = screen.getByTestId('detail-suggestions');
    expect(within(suggestions).getByText('Neon Courier 2088')).toBeInTheDocument();
    expect(within(suggestions).getByText('92%')).toBeInTheDocument();
    expect(within(suggestions).getByText('61%')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('detail-suggestion-use-78'));
    expect(libraryInput).toHaveBeenCalledWith({
      useSuggestion: { key: 'manual:hollow', gameId: 78, now: expect.any(Number) },
    });

    fireEvent.click(screen.getByTestId('detail-not-a-game'));
    expect(libraryInput).toHaveBeenCalledWith({
      markEntryNoGame: { key: 'manual:hollow', now: expect.any(Number) },
    });
  });

  it('uses the review copy for an ambiguous match and never suggests for a no-game entry', async () => {
    const libraryInput = vi.spyOn(commands, 'libraryInput');
    const row = createRow({
      key: 'steam:900',
      status: 'ambiguous',
      matchedName: null,
      isFavorite: false,
      gamenode: null,
    });
    const detail = createDetail({
      row,
      membership: 'notInLibrary',
      suggestions: [
        {
          gameId: 81,
          title: 'Neon Courier',
          year: 2019,
          coverUrl: null,
          confidence: 61,
          tone: 'warning',
        },
      ],
    });

    const { unmount } = renderDetail(
      createSnapshot(
        createLibrary([row], detail, {
          selected: [row.key],
          actions: createActions({ addToGamenode: { disabled: 'notMatched' } }),
        }),
      ),
    );
    expect(screen.getByTestId('detail-match-warning')).toHaveTextContent(
      'Match needs review',
    );
    fireEvent.click(screen.getByTestId('detail-match-cta'));
    expect(libraryInput).toHaveBeenCalledWith('openMatch');
    unmount();

    const noGameRow = createRow({
      key: 'steam:901',
      status: 'noGame',
      matchedName: null,
      gamenode: null,
    });
    const noGameDetail = createDetail({
      row: noGameRow,
      membership: 'notInLibrary',
      suggestions: [
        {
          gameId: 82,
          title: 'Should not render',
          year: 2019,
          coverUrl: null,
          confidence: 80,
          tone: 'success',
        },
      ],
    });
    renderDetail(
      createSnapshot(
        createLibrary([noGameRow], noGameDetail, {
          selected: [noGameRow.key],
          actions: createActions({ addToGamenode: { disabled: 'notMatched' } }),
        }),
      ),
    );
    expect(screen.getByTestId('detail-not-a-game-note')).toHaveTextContent('Not a game');
    expect(screen.queryByTestId('detail-suggestions')).not.toBeInTheDocument();
    expect(screen.queryByTestId('detail-not-a-game')).not.toBeInTheDocument();
  });

  it('summarises a multi-selection and exposes the batch actions and removal', async () => {
    const libraryInput = vi.spyOn(commands, 'libraryInput');
    const first = createRow({
      key: 'steam:1',
      title: 'Alpha',
      playtime: { seconds: 100 * 3600, source: 'store', lastPlayedAt: 1_760_000_000 },
      isFavorite: true,
      gamenode: { status: 'playing', collectionIds: [], entryId: 'entry-1' },
    });
    const second = createRow({
      key: 'steam:2',
      title: 'Bravo',
      playtime: { seconds: 100 * 3600, source: 'store', lastPlayedAt: 1_760_000_000 },
      isFavorite: true,
      gamenode: { status: 'finished', collectionIds: [], entryId: 'entry-2' },
    });
    const third = createRow({
      key: 'steam:3',
      title: 'Charlie',
      playtime: { seconds: 24 * 3600, source: 'store', lastPlayedAt: 1_760_000_000 },
      isFavorite: false,
      gamenode: null,
    });
    const rows = [first, second, third];
    const actions = createActions({ selected: 3 });

    renderDetail(
      createSnapshot(
        createLibrary(rows, null, {
          selected: ['steam:1', 'steam:2', 'steam:3'],
          actions,
        }),
      ),
    );

    expect(screen.getByText('3 games selected')).toBeInTheDocument();
    expect(screen.getByTestId('detail-multi-summary')).toHaveTextContent(
      'Steam · all installed',
    );
    expect(screen.getByText('224 h 0 min')).toBeInTheDocument();
    const stats = screen.getByTestId('detail-stats');
    expect(within(stats).getAllByText('2 of 3')).toHaveLength(2);
    expect(screen.getByText('1 not on GameNode')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('detail-multi-add'));
    expect(libraryInput).toHaveBeenCalledWith('openAdd');

    fireEvent.click(screen.getByTestId('detail-multi-status'));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Finished' }));
    expect(libraryInput).toHaveBeenCalledWith({
      setStatus: { keys: ['steam:1', 'steam:2', 'steam:3'], status: 'finished' },
    });

    fireEvent.click(screen.getByTestId('detail-multi-favorite'));
    expect(libraryInput).toHaveBeenCalledWith({
      setFavorites: {
        keys: ['steam:1', 'steam:2', 'steam:3'],
        favorite: true,
        now: expect.any(Number),
      },
    });

    fireEvent.click(screen.getByTestId('detail-multi-collection'));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Backlog curto' }));
    expect(libraryInput).toHaveBeenCalledWith({
      addToCollection: {
        keys: ['steam:1', 'steam:2', 'steam:3'],
        collectionId: 'backlog-curto',
      },
    });

    fireEvent.click(screen.getByTestId('detail-selection-remove-steam:2'));
    expect(libraryInput).toHaveBeenCalledWith({
      select: { key: 'steam:2', mode: 'toggle' },
    });

    fireEvent.click(screen.getByTestId('detail-selection-clear'));
    expect(libraryInput).toHaveBeenCalledWith('clearSelection');
  });

  it('reads Unfavorite all when every matched selected game is a favorite', () => {
    const first = createRow({
      key: 'steam:1',
      isFavorite: true,
      gamenode: { status: 'playing', collectionIds: [], entryId: 'entry-1' },
    });
    const second = createRow({
      key: 'steam:2',
      isFavorite: true,
      gamenode: { status: 'finished', collectionIds: [], entryId: 'entry-2' },
    });

    renderDetail(
      createSnapshot(
        createLibrary([first, second], null, {
          selected: ['steam:1', 'steam:2'],
          actions: createActions({ selected: 2 }),
        }),
      ),
    );

    expect(screen.getByTestId('detail-multi-favorite')).toHaveTextContent(
      'Unfavorite all',
    );
  });

  it('localizes the sections in Brazilian Portuguese', () => {
    const snapshot = createSnapshot(createLibrary([createRow()], createDetail()));

    renderDetail(snapshot, 'pt-BR');

    expect(screen.getByTestId('detail-pill-installed')).toHaveTextContent(
      'Instalado neste PC',
    );
    expect(screen.getByTestId('detail-pill-status')).toHaveTextContent(
      'Jogando no GameNode',
    );
    expect(screen.getByTestId('detail-close-btn')).toHaveAttribute(
      'aria-label',
      'Fechar detalhes',
    );
    expect(screen.getByTestId('detail-more-btn')).toHaveAttribute(
      'aria-label',
      'Mais ações',
    );
  });
});
