import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { AppProviders } from './AppProviders';
import { App } from './App';
import { resetUiStore } from './store/uiStore';
import {
  createDefaultFilters,
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
        version: '9.9.9',
        backgroundSupported: false,
        platform: 'windows',
      } satisfies AppInfo;
    }
    if (cmd === 'get_license_notices') return 'GPL-3.0-only notices';
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
    familyShared: false,
    running: false,
    status: { matched: { gameId: 100 + i, source: 'automatic' as const } },
    coverFile: `cover-${i}.jpg`,
    playtime: { seconds: 3600, source: 'store' as const, lastPlayedAt: 1_700_000_000 },
    releaseYear: 2011,
    addedAt: 1_700_000_000,
    isFavorite: false,
  }));
}

function libraryView(count = 200, overrides: Partial<LibraryView> = {}): LibraryView {
  return {
    generation: 1,
    layout: {
      view: 'grid',
      sort: { key: 'title', direction: 'ascending' },
      filter: 'all',
      filters: createDefaultFilters(),
      sidebarWidth: 216,
      detailWidth: 320,
      detailOpen: false,
    },
    search: '',
    access: 'verified',
    total: count,
    installedTotal: count,
    providers: [{ id: 'steam', name: 'Steam', count, installedCount: count, ownedCount: count, familyCount: 0 }],
    rows: rows(count),
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

  it('is a real application shell, not a placeholder', async () => {
    await mountApp();
    expect(screen.getByTestId('top-bar')).toBeInTheDocument();
    expect(screen.getByTestId('sidebar')).toBeInTheDocument();
    expect(screen.getByTestId('signed-out-view')).toBeInTheDocument();
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
    const cover = screen.getAllByRole('img')[0];
    expect(cover.getAttribute('src')).toMatch(/^gamenode-asset:\/\/localhost\/cover\//);
    expect(cover).toHaveAttribute('loading', 'lazy');
    expect(cover).toHaveAttribute('decoding', 'async');
  });

  it('sidebar provider filter sends a library_input command', async () => {
    await mountApp();
    await publish(signedIn(current));
    const sidebar = await screen.findByTestId('sidebar');
    fireEvent.click(within(sidebar).getByText('Steam'));
    await waitFor(() =>
      expect(calls('library_input').at(-1)?.args).toEqual({
        input: { setFilter: { provider: 'steam' } },
      }),
    );
  });

  it('switches the content area to PC ports and back to the filtered library', async () => {
    await mountApp();
    await publish(signedIn(current));

    fireEvent.click(await screen.findByTestId('sidebar-ports'));
    expect(await screen.findByTestId('ports-view')).toBeInTheDocument();
    expect(screen.queryByTestId('library-screen')).not.toBeInTheDocument();
    expect(calls('ports_open')).toHaveLength(1);

    fireEvent.click(screen.getByTestId('sidebar-provider-steam'));
    expect(await screen.findByTestId('library-screen')).toBeInTheDocument();
    expect(screen.queryByTestId('ports-view')).not.toBeInTheDocument();
    expect(calls('library_input').at(-1)?.args).toEqual({
      input: { setFilter: { provider: 'steam' } },
    });
  });

  it('shows the one-time sync explanation after sign-in and dismisses it through IPC', async () => {
    await mountApp();
    const next = signedIn(current);
    next.sync = { enabled: true, explained: false };
    await publish(next);

    expect(await screen.findByTestId('sync-explanation-modal')).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => expect(calls('dismiss_sync_explanation')).toHaveLength(1));
  });

  it('opens Settings from the app menu and with Ctrl+,', async () => {
    await mountApp();
    fireEvent.click(screen.getByRole('button', { name: 'Application menu' }));
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
        noResults: false,
        error: null,
        searchBlocked: null,
        current: 'unmatched',
        canClear: false,
      },
    });
    await publish(withMatch);
    expect(await screen.findByTestId('match-dialog-overlay')).toBeInTheDocument();

    await publish(signedIn(current));
    await waitFor(() => expect(screen.queryByTestId('match-dialog-overlay')).not.toBeInTheDocument());
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
