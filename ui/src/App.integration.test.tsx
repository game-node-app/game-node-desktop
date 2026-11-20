import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { AppProviders } from './AppProviders';
import { App } from './App';
import { resetUiStore, uiStore } from './store/uiStore';
import {
  createDefaultFilters,
  createDefaultLibraryView,
  createDefaultSnapshot,
  type AppInfo,
  type LibraryView,
  type Row,
  type Snapshot,
  type TargetInspection,
  type ToolModalView,
} from './api';

// ---- A fake Tauri host: `invoke` and `engine:snapshot` events are the only seams mocked. ----
type Handler = (event: { payload: Snapshot }) => void;
const listeners = new Set<Handler>();
const invokeLog: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
const order: string[] = [];
let current: Snapshot;
let locale = 'en';
let pendingSnapshot: Promise<Snapshot> | null = null;

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    invokeLog.push({ cmd, args });
    order.push(`invoke:${cmd}`);
    if (cmd === 'get_snapshot') return pendingSnapshot ?? current;
    if (cmd === 'get_app_info') {
      return {
        locale,
        systemLocale: locale,
        version: '9.9.9',
        buildDate: '2026-11-16',
        osVersion: 'Test OS 1.0',
        arch: 'x86_64',
        backgroundSupported: false,
        platform: 'windows',
      } satisfies AppInfo;
    }
    if (cmd === 'get_license_notices') return 'GPL-3.0-only notices';
    if (cmd === 'sign_in_cover_wall') return [];
    return undefined;
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (_name: string, handler: Handler) => {
    order.push('listen');
    listeners.add(handler);
    return () => listeners.delete(handler);
  }),
}));

/** What the Rust engine does after any state change: publish a new snapshot. */
async function publish(next: Snapshot) {
  current = next;
  await act(async () => {
    for (const listener of listeners) listener({ payload: next });
  });
}

function rows(count: number): Row[] {
  return Array.from({ length: count }, (_, i) => ({
    key: `steam:${i}`,
    title: `Game ${i}`,
    detectedName: `game_${i}`,
    matchedName: `Game ${i}`,
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    running: false,
    status: { matched: { gameId: 100 + i, source: 'automatic' as const } },
    coverFile: `cover-${i}.jpg`,
    playtime: { seconds: 3600, source: 'store' as const, lastPlayedAt: 1_700_000_000 },
    releaseYear: 2011,
    addedAt: 1_700_000_000,
    isFavorite: false,
    hidden: false,
    gamenode: null,
  }));
}

function libraryView(count = 200, overrides: Partial<LibraryView> = {}): LibraryView {
  const games = rows(count);
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
      sidebarWidth: 216,
      detailWidth: 320,
      detailOpen: false,
      sidebarCollapsed: false,
    },
    search: '',
    access: 'verified',
    total: count,
    installedTotal: count,
    providers: [{ id: 'steam', name: 'Steam', count, installedCount: count, ownedCount: count, sharedCount: 0, sharedLabel: 'Steam Family' }],
    rows: games,
    paletteIndex: games.map((game) => ({
      key: game.key,
      title: game.title,
      providerName: game.providerName,
      installed: game.installed,
      playtimeLabel: null,
    })),
    scanning: false,
    failedProviders: [],
    selected: [],
    focused: null,
    actions: {
      selected: 0,
      play: { disabled: 'nothingSelected' },
      install: { disabled: 'nothingSelected' },
      matchGame: { disabled: 'nothingSelected' },
      addToGamenode: { disabled: 'nothingSelected' },
      changeMatch: false,
    },
    ...overrides,
  };
}

function signedIn(snapshot: Snapshot, overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    ...snapshot,
    auth: { signedIn: { user: { id: 'u1', username: 'Cassi' } } },
    library: libraryView(),
    ...overrides,
  };
}

async function mountApp() {
  render(
    <AppProviders>
      <App />
    </AppProviders>,
  );
  // App info and the first snapshot both resolve through the mocked IPC.
  await waitFor(() => expect(invokeLog.some((c) => c.cmd === 'get_app_info')).toBe(true));
  await waitFor(() => expect(invokeLog.some((c) => c.cmd === 'get_snapshot')).toBe(true));

  // The engine bridge must be attached before the first snapshot is requested.
  expect(order.indexOf('listen')).toBeGreaterThanOrEqual(0);
  expect(order.indexOf('listen')).toBeLessThan(order.indexOf('invoke:get_snapshot'));
}

const calls = (cmd: string) => invokeLog.filter((c) => c.cmd === cmd);

describe('App (integration: real component tree over a fake Tauri host)', () => {
  beforeEach(() => {
    resetUiStore();
    invokeLog.length = 0;
    order.length = 0;
    listeners.clear();
    locale = 'en';
    current = createDefaultSnapshot();
    pendingSnapshot = null;
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });

  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it('signed out: fills the window with the entry surface and hides the library shell', async () => {
    await mountApp();
    expect(screen.getByTestId('signed-out-view')).toBeInTheDocument();
    expect(screen.getByTestId('signed-out-surface')).toBeInTheDocument();
    const chrome = screen.getByTestId('signed-out-window-chrome');
    expect(chrome.querySelector('[data-tauri-drag-region="deep"]')).not.toBeNull();
    expect(screen.getByTestId('window-controls')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Minimize' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Maximize' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument();
    expect(screen.queryByTestId('top-bar')).not.toBeInTheDocument();
    expect(screen.queryByTestId('sidebar')).not.toBeInTheDocument();
    expect(screen.queryByTestId('status-bar')).not.toBeInTheDocument();
    expect(screen.getByTestId('sign-in-art')).toBeInTheDocument();
    expect(screen.getByTestId('sign-in-art')).toHaveClass(
      'hidden',
      'basis-[53%]',
      'max-w-[760px]',
      'min-[860px]:flex',
    );
  });

  it('signed in: restores the real application shell and its native window controls', async () => {
    await mountApp();
    await publish(signedIn(current));

    expect(await screen.findByTestId('top-bar')).toBeInTheDocument();
    expect(screen.getByTestId('sidebar')).toBeInTheDocument();
    expect(screen.getByTestId('status-bar')).toBeInTheDocument();
    expect(screen.getByTestId('window-controls')).toBeInTheDocument();
    expect(screen.queryByTestId('signed-out-surface')).not.toBeInTheDocument();
  });

  it('hides the status bar when the preference is off and gives the height to the library', async () => {
    current = createDefaultSnapshot();
    current.preferences = { ...current.preferences, showStatusBar: false };
    await mountApp();
    await publish(signedIn(current));

    expect(await screen.findByTestId('library-screen')).toBeInTheDocument();
    expect(screen.queryByTestId('status-bar')).not.toBeInTheDocument();
    expect(screen.getByTestId('app-shell')).toHaveClass(
      'grid-rows-[48px_minmax(0,1fr)]',
    );
  });

  it('brings the status bar back when the preference is turned on', async () => {
    current = createDefaultSnapshot();
    current.preferences = { ...current.preferences, showStatusBar: false };
    await mountApp();
    await publish(signedIn(current));

    expect(screen.queryByTestId('status-bar')).not.toBeInTheDocument();

    await publish({
      ...current,
      preferences: { ...current.preferences, showStatusBar: true },
    });

    expect(await screen.findByTestId('status-bar')).toBeInTheDocument();
    expect(screen.getByTestId('app-shell')).toHaveClass(
      'grid-rows-[48px_minmax(0,1fr)_26px]',
    );
  });

  it('keeps a cached offline library in the shell after sign-in', async () => {
    await mountApp();
    const next = signedIn(current);
    next.library = libraryView(5, { access: 'offline' });
    await publish(next);

    expect(await screen.findByTestId('library-screen')).toBeInTheDocument();
    expect(screen.getByTestId('top-bar')).toBeInTheDocument();
    expect(screen.queryByTestId('signed-out-view')).not.toBeInTheDocument();
  });

  it('keeps a cached library in the shell while verifying or when restore fails offline', async () => {
    await mountApp();
    await publish({
      ...current,
      auth: 'verifying',
      library: libraryView(4, { access: 'unverified' }),
    });

    expect(await screen.findByTestId('library-screen')).toBeInTheDocument();
    expect(screen.getByTestId('top-bar')).toBeInTheDocument();
    expect(screen.queryByTestId('signed-out-surface')).not.toBeInTheDocument();

    await publish({
      ...current,
      auth: 'restoreFailed',
      library: libraryView(4, { access: 'unverified' }),
    });

    expect(screen.getByTestId('library-screen')).toBeInTheDocument();
    expect(screen.getByTestId('top-bar')).toBeInTheDocument();
    expect(screen.queryByTestId('signed-out-surface')).not.toBeInTheDocument();
  });

  it('shows the full-window entry surface on restoreFailed when no cached library exists', async () => {
    await mountApp();
    await publish({
      ...current,
      auth: 'restoreFailed',
      library: null,
    });

    expect(await screen.findByTestId('signed-out-surface')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /retry connection/i })).toBeInTheDocument();
    expect(screen.queryByTestId('top-bar')).not.toBeInTheDocument();
    expect(screen.queryByTestId('sidebar')).not.toBeInTheDocument();
  });

  it('shows the pending state over the full-window entry surface', async () => {
    await mountApp();
    const next = createDefaultSnapshot();
    next.auth = {
      pending: {
        provider: 'google',
        awaitingBrowser: true,
        canReopen: true,
        reopenFailed: false,
      },
    };
    await publish(next);

    expect(await screen.findByTestId('sign-in-pending')).toBeInTheDocument();
    expect(screen.getByTestId('signed-out-surface')).toBeInTheDocument();
    expect(screen.queryByTestId('top-bar')).not.toBeInTheDocument();
  });

  it('signed out: Continue with Google sends dispatch_auth over IPC', async () => {
    await mountApp();
    fireEvent.click(await screen.findByText('Continue with Google'));
    await waitFor(() =>
      expect(calls('dispatch_auth').at(-1)?.args).toEqual({ input: { startSignIn: 'google' } }),
    );
  });

  it('subscribes to engine:snapshot and swaps the signed-out screen for the virtualized library', async () => {
    await mountApp();
    await waitFor(() => expect(listeners.size).toBeGreaterThan(0));

    await publish(signedIn(current));

    expect(await screen.findByTestId('library-screen')).toBeInTheDocument();
    expect(screen.queryByTestId('signed-out-view')).not.toBeInTheDocument();
    expect(screen.getByTestId('user-display-name')).toHaveTextContent('Cassi');
    // 200 games in the snapshot, only the viewport mounts.
    const tiles = screen.getAllByRole('listitem');
    expect(tiles.length).toBeGreaterThan(0);
    expect(tiles.length).toBeLessThan(60);
    const cover = screen
      .getAllByRole('img')
      .find((img) => img.getAttribute('src')?.startsWith('gamenode-asset://localhost/cover/'));
    expect(cover).toBeDefined();
    expect(cover).toHaveAttribute('loading', 'lazy');
    expect(cover).toHaveAttribute('decoding', 'async');
  });

  it('sidebar provider filter sends the All scope and a store facet', async () => {
    await mountApp();
    await publish(signedIn(current));
    const sidebar = await screen.findByTestId('sidebar');
    fireEvent.click(within(sidebar).getByText('Steam'));
    await waitFor(() =>
      expect(calls('library_input').at(-1)?.args).toEqual({
        input: {
          setFilters: expect.objectContaining({ stores: ['steam'] }),
        },
      }),
    );
    expect(calls('library_input').map((call) => call.args?.input)).toContainEqual({
      setScope: 'all',
    });
  });

  it('switches the content area to PC ports and back to the filtered library', async () => {
    await mountApp();
    await publish(signedIn(current));

    fireEvent.click(
      within(await screen.findByTestId('sidebar-ports')).getByRole('button'),
    );
    expect(await screen.findByTestId('ports-view')).toBeInTheDocument();
    expect(screen.queryByTestId('library-screen')).not.toBeInTheDocument();
    expect(calls('ports_open')).toHaveLength(1);

    fireEvent.click(
      within(screen.getByTestId('sidebar-provider-steam')).getByRole('button', {
        name: 'Steam',
      }),
    );
    expect(await screen.findByTestId('library-screen')).toBeInTheDocument();
    expect(screen.queryByTestId('ports-view')).not.toBeInTheDocument();
    await waitFor(() =>
      expect(calls('library_input').at(-1)?.args).toEqual({
        input: {
          setFilters: expect.objectContaining({ stores: ['steam'] }),
        },
      }),
    );
  });

  it('switches the content area to the catalog and back to the library', async () => {
    await mountApp();
    await publish(signedIn(current));

    fireEvent.click(await screen.findByRole('button', { name: 'Explore GameNode' }));
    expect(await screen.findByTestId('catalog-view')).toBeInTheDocument();
    expect(screen.queryByTestId('library-screen')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'All games' }));
    expect(await screen.findByTestId('library-screen')).toBeInTheDocument();
    expect(screen.queryByTestId('catalog-view')).not.toBeInTheDocument();
  });

  it('opens the selected game menu from the palette while the catalog is shown', async () => {
    await mountApp();
    await publish(signedIn(current));

    const catalogTrigger = await screen.findByRole('button', { name: 'Explore GameNode' });
    fireEvent.click(catalogTrigger);
    expect(await screen.findByTestId('catalog-view')).toBeInTheDocument();
    catalogTrigger.focus();

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    const input = await screen.findByTestId('command-palette-input');
    fireEvent.change(input, { target: { value: 'Game 1' } });
    fireEvent.keyDown(input, { key: 'F10', shiftKey: true });

    const menu = await screen.findByTestId('library-context-menu');
    expect(await screen.findByTestId('library-screen')).toBeInTheDocument();
    expect(within(menu).getByText('Game 1')).toBeInTheDocument();
    expect(calls('library_input').map((call) => call.args?.input)).toContainEqual({
      select: { key: 'steam:1', mode: 'replace' },
    });
    expect(uiStore.state.activeDestination).toBe('library');
    expect(
      uiStore.state.contextMenu.x === 0 && uiStore.state.contextMenu.y === 0,
    ).toBe(false);

    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() =>
      expect(screen.queryByTestId('library-context-menu')).not.toBeInTheDocument(),
    );
    // The palette handed focus back to its trigger; the menu did not steal or lose it.
    expect(document.activeElement).toBe(catalogTrigger);

    // The invisible-open bug swallowed Ctrl+K; the menu is visible and now gone.
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    expect(await screen.findByTestId('command-palette')).toBeInTheDocument();
  });

  it('opens the selected game menu from the palette while PC ports is shown', async () => {
    await mountApp();
    await publish(signedIn(current));

    const portsButton = within(
      await screen.findByTestId('sidebar-ports'),
    ).getByRole('button');
    fireEvent.click(portsButton);
    expect(await screen.findByTestId('ports-view')).toBeInTheDocument();
    portsButton.focus();

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    const input = await screen.findByTestId('command-palette-input');
    fireEvent.change(input, { target: { value: 'Game 1' } });
    fireEvent.keyDown(input, { key: 'F10', shiftKey: true });

    const menu = await screen.findByTestId('library-context-menu');
    expect(await screen.findByTestId('library-screen')).toBeInTheDocument();
    expect(within(menu).getByText('Game 1')).toBeInTheDocument();
    expect(uiStore.state.activeDestination).toBe('library');

    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() =>
      expect(screen.queryByTestId('library-context-menu')).not.toBeInTheDocument(),
    );
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    expect(await screen.findByTestId('command-palette')).toBeInTheDocument();
  });

  it('shows the one-time sync explanation after sign-in and dismisses it through IPC', async () => {
    await mountApp();
    const next = signedIn(current);
    next.sync = { enabled: true, explained: false, summary: null, running: false };
    await publish(next);

    expect(await screen.findByTestId('sync-explanation-modal')).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => expect(calls('dismiss_sync_explanation')).toHaveLength(1));
  });

  it('opens Settings from the app menu and with Ctrl+,', async () => {
    await mountApp();
    await publish(signedIn(current));
    fireEvent.click(await screen.findByTestId('app-menu-trigger'));
    const menu = await screen.findByTestId('app-menu');
    fireEvent.click(within(menu).getByText('Settings'));
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());

    fireEvent.keyDown(window, { key: ',', ctrlKey: true });
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
  });

  it('renders the match dialog, claim conflict and game tools modal when the engine publishes them', async () => {
    await mountApp();
    const withMatch = signedIn(current);
    withMatch.library = libraryView(5, {
      matchDialog: {
        entryKey: 'steam:1',
        detectedName: 'game_1',
        providerName: 'Steam',
        query: 'Game 1',
        searching: false,
        candidates: [],
        results: [],
        selected: null,
        noResults: false,
        error: null,
        searchBlocked: null,
        current: 'unmatched',
        canClear: false,
      },
    });
    await publish(withMatch);
    expect(await screen.findByTestId('match-dialog')).toBeInTheDocument();

    await publish(signedIn(current));
    await waitFor(() => expect(screen.queryByTestId('match-dialog')).not.toBeInTheDocument());
  });

  it('contains a crash in the game tools panel: the rest of the app stays up and Close asks the engine to close it', async () => {
    const errors = vi.spyOn(console, 'error').mockImplementation(() => {});
    await mountApp();
    const next = signedIn(current);
    const modal: ToolModalView = {
      entryKey: 'steam:1',
      title: 'Game 1',
      installDir: '/games/game_1',
      // Malformed on purpose: the backend always sends an anticheat report, but a bad payload
      // must not blank the app.
      inspection: {
        entryKey: 'steam:1',
        installDir: '/games/game_1',
        candidates: [],
        anticheat: null as unknown as TargetInspection['anticheat'],
      },
      tools: [],
      activeToolId: 'optiscaler',
      selectedChannel: 'stable',
      availableVersions: [],
      selectedVersion: null,
      selectedProxy: 'dxgi',
      anticheatConfirmed: false,
      operation: null,
    };
    next.tools = { ...current.tools, modal };
    await publish(next);

    const dialog = await screen.findByRole('alertdialog');
    expect(within(dialog).getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByTestId('top-bar')).toBeInTheDocument();
    expect(screen.getByTestId('library-screen')).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }));
    await waitFor(() => expect(calls('close_tools')).toHaveLength(1));
    errors.mockRestore();
  });

  it('adopts the host locale (pt-BR) from get_app_info', async () => {
    locale = 'pt-BR';
    await mountApp();
    expect(await screen.findByText('Continuar com Google')).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('pt-BR');
  });

  it('switches the interface when the language preference changes', async () => {
    await mountApp();
    expect(await screen.findByText('Continue with Google')).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('en');

    fireEvent.keyDown(window, { key: ',', ctrlKey: true });
    const modal = await screen.findByRole('dialog');
    expect(
      within(modal).getByRole('heading', { name: 'Settings' }),
    ).toBeInTheDocument();

    await publish({
      ...current,
      preferences: { ...current.preferences, locale: 'ptBr' },
    });
    expect(await within(modal).findByText('Continuar com Google')).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('pt-BR');
    // The open modal re-renders through the i18n context, not a reload.
    expect(
      await within(modal).findByRole('heading', { name: 'Configurações' }),
    ).toBeInTheDocument();

    // System language follows the OS again without a reload.
    await publish({
      ...current,
      preferences: { ...current.preferences, locale: 'system' },
    });
    expect(await within(modal).findByText('Continue with Google')).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('en');
  });

  it('keeps a newer engine snapshot when the initial get_snapshot reply is stale', async () => {
    let resolveInitial!: (snapshot: Snapshot) => void;
    pendingSnapshot = new Promise<Snapshot>((resolve) => {
      resolveInitial = resolve;
    });

    await mountApp();
    await waitFor(() => expect(listeners.size).toBeGreaterThan(0));

    const older = signedIn(current);
    older.auth = { signedIn: { user: { id: 'older', username: 'Older Player' } } };
    const newer = signedIn(current);
    newer.auth = { signedIn: { user: { id: 'newer', username: 'Newer Player' } } };

    await publish(newer);
    expect(await screen.findByTestId('user-display-name')).toHaveTextContent('Newer Player');

    await act(async () => {
      resolveInitial(older);
      await Promise.resolve();
    });

    expect(screen.getByTestId('user-display-name')).toHaveTextContent('Newer Player');
  });
});
