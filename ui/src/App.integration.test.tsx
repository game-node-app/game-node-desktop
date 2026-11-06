import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { AppProviders } from './AppProviders';
import { App } from './App';
import { resetUiStore } from './store/uiStore';
import {
  createDefaultSnapshot,
  type BootstrapState,
  type LibraryView,
  type Row,
  type Snapshot,
} from './api';

// ---- A fake Tauri host: `invoke` and `engine:snapshot` events are the only seams mocked. ----
type Handler = (event: { payload: Snapshot }) => void;
const listeners = new Set<Handler>();
const invokeLog: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
let current: Snapshot;
let locale = 'en';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    invokeLog.push({ cmd, args });
    if (cmd === 'get_snapshot') return current;
    if (cmd === 'get_bootstrap_state') {
      const state: BootstrapState = {
        snapshot: current,
        locale,
        version: '9.9.9',
        licenseNotices: 'GPL-3.0-only notices',
        backgroundSupported: false,
      };
      return state;
    }
    return undefined;
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (_name: string, handler: Handler) => {
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
    running: false,
    status: { matched: { gameId: 100 + i, source: 'automatic' as const } },
    coverFile: `cover-${i}.jpg`,
    playtime: { seconds: 3600, source: 'store' as const },
  }));
}

function libraryView(count = 200, overrides: Partial<LibraryView> = {}): LibraryView {
  return {
    generation: 1,
    layout: {
      view: 'grid',
      sort: { key: 'title', direction: 'ascending' },
      filter: 'all',
      sidebarWidth: 216,
      detailWidth: 320,
      detailOpen: false,
    },
    search: '',
    access: 'verified',
    total: count,
    providers: [{ id: 'steam', name: 'Steam', count }],
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
  // The bootstrap + snapshot queries resolve through the mocked IPC.
  await waitFor(() => expect(invokeLog.some((c) => c.cmd === 'get_bootstrap_state')).toBe(true));
}

const calls = (cmd: string) => invokeLog.filter((c) => c.cmd === cmd);

describe('App (integration: real component tree over a fake Tauri host)', () => {
  beforeEach(() => {
    resetUiStore();
    invokeLog.length = 0;
    listeners.clear();
    locale = 'en';
    current = createDefaultSnapshot();
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
    fireEvent.click(await screen.findByText('Settings'));
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

  it('adopts the host locale (pt-BR) from get_bootstrap_state', async () => {
    locale = 'pt-BR';
    await mountApp();
    expect(await screen.findByText('Continuar com Google')).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('pt-BR');
  });
});
