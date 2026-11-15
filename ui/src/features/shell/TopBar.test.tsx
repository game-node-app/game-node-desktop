import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  createDefaultLibraryView,
  createDefaultSnapshot,
  type LibraryView,
  type Snapshot,
} from '../../api/types';
import { SNAPSHOT_QUERY_KEY } from '../../api/queries';
import { I18nProvider } from '../../i18n';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { TopBar } from './TopBar';

const tauri = vi.hoisted(() => {
  const api = {
    minimize: vi.fn().mockResolvedValue(undefined),
    toggleMaximize: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
    isMaximized: vi.fn().mockResolvedValue(false),
    onResized: vi.fn().mockResolvedValue(() => undefined),
  };
  return { api, getCurrentWindow: vi.fn(() => api) };
});

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: tauri.getCurrentWindow,
}));

const signedInAuth = {
  signedIn: { user: { id: 'user-1', username: 'cassi' } },
} as const;

function library(overrides: Partial<LibraryView> = {}): LibraryView {
  return {
    ...createDefaultLibraryView(),
    access: 'verified',
    ...overrides,
  };
}

function snapshot(overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    ...createDefaultSnapshot(),
    auth: signedInAuth,
    sync: { enabled: true, explained: true },
    library: library(),
    ...overrides,
  };
}

function renderTopBar(
  data: Snapshot = snapshot(),
  locale: 'en' | 'pt-BR' = 'en',
  props: Parameters<typeof TopBar>[0] = {},
) {
  const queryClient: QueryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, data);
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark" env="test">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>
          <TopBar {...props} />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('features/shell/TopBar', () => {
  beforeEach(() => {
    resetUiStore();
    vi.clearAllMocks();
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });

  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI_IPC__;
  });

  it('renders the wordmark, DESKTOP badge and account trigger', () => {
    renderTopBar(snapshot({ auth: 'signedOut' }), 'en', { auth: 'signedOut' });

    expect(screen.getByRole('img', { name: 'GameNode' })).toBeInTheDocument();
    expect(screen.getByTestId('desktop-badge')).toHaveTextContent('DESKTOP');
    expect(screen.getByTestId('app-menu-trigger')).toHaveAttribute(
      'aria-label',
      'Account menu',
    );
    expect(screen.queryByTestId('sync-status')).not.toBeInTheDocument();
  });

  it('identifies the signed-in user and opens the account menu from the avatar', async () => {
    renderTopBar();

    expect(screen.getByTestId('user-display-name')).toHaveTextContent('cassi');
    fireEvent.click(screen.getByTestId('app-menu-trigger'));

    const menu = await screen.findByTestId('app-menu');
    expect(menu).toBeInTheDocument();
    expect(uiStore.state.appMenuOpen).toBe(true);
    expect(screen.getByTestId('app-menu-header')).toHaveTextContent(
      'Connected to GameNode',
    );
  });

  it('opens the command palette from the 440 px search trigger', () => {
    renderTopBar();

    const search = screen.getByTestId('command-search');
    expect(search).toHaveClass('w-[440px]');
    expect(search).toHaveTextContent('Search games, actions and settings');
    expect(search).toHaveTextContent('Ctrl');
    expect(search).toHaveTextContent('K');

    fireEvent.click(search);
    expect(uiStore.state.paletteOpen).toBe(true);
  });

  it('shows synced, syncing, off and offline sync states', () => {
    const { unmount } = renderTopBar();
    expect(screen.getByTestId('sync-status')).toHaveTextContent('Synced');
    unmount();

    renderTopBar(snapshot({ library: library({ scanning: true }) }));
    expect(screen.getByTestId('sync-status')).toHaveTextContent('Syncing…');
  });

  it('shows Sync off when synchronization is disabled', () => {
    renderTopBar(
      snapshot({
        sync: { enabled: false, explained: true },
        library: library({ access: 'offline' }),
      }),
    );
    expect(screen.getByTestId('sync-status')).toHaveTextContent('Sync off');
  });

  it('shows Offline when the library access is offline', () => {
    renderTopBar(snapshot({ library: library({ access: 'offline' }) }));
    expect(screen.getByTestId('sync-status')).toHaveTextContent('Offline');
  });

  it('shows Syncing… when a store account is checking', () => {
    renderTopBar(
      snapshot({
        storeAccounts: [
          {
            provider: 'steam',
            state: 'checking',
            accountId: null,
            accountName: null,
            ownedCount: 0,
            sharedCount: 0,
            sharedLabel: null,
            installedCount: 0,
            lastCheckedAt: null,
            lastSyncedAt: null,
            storage: null,
            persistFailed: false,
            catalogError: null,
            sharedError: null,
            canSignIn: true,
            canCancelSignIn: false,
            canFocusSignIn: false,
            canSignOut: false,
            canCheck: false,
            canRemoveCredential: false,
          },
        ],
      }),
    );
    expect(screen.getByTestId('sync-status')).toHaveTextContent('Syncing…');
  });

  it('localizes the search trigger and sync status', () => {
    renderTopBar(snapshot(), 'pt-BR');
    expect(screen.getByTestId('command-search')).toHaveTextContent(
      'Buscar jogos, ações e configurações',
    );
    expect(screen.getByTestId('sync-status')).toHaveTextContent('Sincronizado');
  });

  it('keeps the drag region on non-interactive areas only', () => {
    renderTopBar();

    expect(screen.getByTestId('top-bar')).toHaveAttribute(
      'data-tauri-drag-region',
      'deep',
    );
    expect(screen.getByTestId('command-search')).not.toHaveAttribute(
      'data-tauri-drag-region',
    );
    expect(screen.getByTestId('app-menu-trigger')).not.toHaveAttribute(
      'data-tauri-drag-region',
    );
    expect(screen.getByTestId('window-controls')).not.toHaveAttribute(
      'data-tauri-drag-region',
    );
  });

  it('takes the second press of an empty-space double click from Tauri', () => {
    renderTopBar();
    const documentDown = vi.fn();
    document.addEventListener('mousedown', documentDown);

    fireEvent.mouseDown(screen.getByTestId('top-bar'), {
      detail: 2,
      bubbles: true,
    });
    expect(documentDown).not.toHaveBeenCalled();

    fireEvent.mouseDown(screen.getByTestId('command-search'), {
      detail: 2,
      bubbles: true,
    });
    expect(documentDown).toHaveBeenCalledTimes(1);
    document.removeEventListener('mousedown', documentDown);
  });

  it('toggles maximize on an empty-space double click but not on a control', async () => {
    renderTopBar();
    await waitFor(() => expect(tauri.getCurrentWindow).toHaveBeenCalled());

    fireEvent.doubleClick(screen.getByTestId('command-search'));
    expect(tauri.api.toggleMaximize).not.toHaveBeenCalled();

    fireEvent.doubleClick(screen.getByTestId('top-bar'));
    expect(tauri.api.toggleMaximize).toHaveBeenCalledTimes(1);
  });

  it('wires minimize, toggle maximize and close to the window controls', async () => {
    renderTopBar();
    await waitFor(() => expect(tauri.getCurrentWindow).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: 'Minimize' }));
    fireEvent.click(screen.getByRole('button', { name: 'Maximize' }));
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));

    expect(tauri.api.minimize).toHaveBeenCalledTimes(1);
    expect(tauri.api.toggleMaximize).toHaveBeenCalledTimes(1);
    expect(tauri.api.close).toHaveBeenCalledTimes(1);
  });
});
