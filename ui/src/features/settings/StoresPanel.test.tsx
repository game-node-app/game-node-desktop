import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  APP_INFO_QUERY_KEY,
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultSnapshot,
  type LibraryView,
  type Snapshot,
  type StoreAccountView,
  createDefaultFilters,
} from '../../api';
import { I18nProvider } from '../../i18n';
import { openSettingsModal, resetUiStore } from '../../store/uiStore';
import { SettingsModal, ToastContainer } from './index';

function account(overrides: Partial<StoreAccountView> = {}): StoreAccountView {
  return {
    provider: 'steam',
    state: 'connected',
    accountId: '76561198000000000',
    accountName: 'cassi',
    ownedCount: 5,
    familyCount: 1,
    installedCount: 3,
    lastCheckedAt: Math.floor(Date.now() / 1000),
    lastSyncedAt: null,
    storage: 'secretService',
    persistFailed: false,
    catalogError: null,
    familyError: null,
    canSignIn: false,
    canCancelSignIn: false,
    canFocusSignIn: false,
    canSignOut: true,
    canCheck: true,
    canRemoveCredential: false,
    ...overrides,
  };
}

function library(): LibraryView {
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
    total: 12,
    installedTotal: 9,
    providers: [
      { id: 'steam', name: 'Steam', count: 6, installedCount: 3, ownedCount: 5, familyCount: 1 },
      { id: 'gog', name: 'GOG', count: 6, installedCount: 6, ownedCount: 0, familyCount: 0 },
    ],
    rows: [],
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
  };
}

function baseSnapshot(overrides: Partial<Snapshot> = {}): Snapshot {
  const snapshot = createDefaultSnapshot();
  snapshot.auth = { signedIn: { user: { id: 'u1', username: 'Cassi' } } };
  snapshot.library = library();
  snapshot.storeAccounts = [account()];
  return { ...snapshot, ...overrides };
}

function renderSettings(value: Snapshot, locale = 'en') {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, value);
  queryClient.setQueryData(APP_INFO_QUERY_KEY, { locale, version: '0.2.0', backgroundSupported: false });
  return {
    queryClient,
    ...render(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale={locale}>
            <ToastContainer />
            <SettingsModal />
          </I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    ),
  };
}

function updateSnapshot(queryClient: ReturnType<typeof createAppQueryClient>, next: Snapshot) {
  act(() => {
    queryClient.setQueryData<Snapshot>(SNAPSHOT_QUERY_KEY, next);
  });
}

describe('StoresPanel Steam account integration', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('places the Steam card before the other detected stores and keeps the DRM-free section', () => {
    openSettingsModal('stores');
    renderSettings(baseSnapshot());

    const headings = screen.getAllByRole('heading', { level: 3 }).map((heading) => heading.textContent);
    expect(headings).toEqual(['Steam account', 'Installed plugins', 'Detected stores', 'DRM-free games']);
    expect(screen.getByText('Signed in as cassi')).toBeInTheDocument();
    expect(screen.getByText('GOG')).toBeInTheDocument();
    expect(screen.getByText('6 installed games')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Rescan' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Reset hidden games/i })).toBeInTheDocument();
  });

  it('renders the Steam card and sign-in without a GameNode session', () => {
    openSettingsModal('stores');
    renderSettings(baseSnapshot({
      auth: 'signedOut',
      library: null,
      storeAccounts: [account({
        state: 'notConnected',
        accountId: null,
        accountName: null,
        ownedCount: 0,
        familyCount: 0,
        installedCount: 0,
        lastCheckedAt: null,
        canSignIn: true,
        canCheck: false,
        canSignOut: false,
      })],
    }));

    expect(screen.getByRole('heading', { name: 'Steam account' })).toBeInTheDocument();
    expect(screen.getByText('Not connected')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign in with Steam' })).toBeInTheDocument();
    expect(screen.getAllByText('No installed games found').length).toBeGreaterThan(0);
    expect(screen.getByRole('heading', { name: 'DRM-free games' })).toBeInTheDocument();
  });

  it('checks a stale Steam session when Stores opens and leaves a fresh one alone', async () => {
    const check = vi.spyOn(commands, 'providerCheck').mockResolvedValue(undefined);
    openSettingsModal('stores');
    const { unmount } = renderSettings(baseSnapshot({
      storeAccounts: [account({ lastCheckedAt: Math.floor(Date.now() / 1000) - 10 * 60 })],
    }));

    await waitFor(() => expect(check).toHaveBeenCalledWith('steam'));
    unmount();

    check.mockClear();
    openSettingsModal('stores');
    renderSettings(baseSnapshot({
      storeAccounts: [account({ lastCheckedAt: Math.floor(Date.now() / 1000) - 60 })],
    }));
    await act(async () => { await Promise.resolve(); });
    expect(check).not.toHaveBeenCalled();
  });

  it('announces a connected Steam account with the imported game count without stealing focus', async () => {
    openSettingsModal('stores');
    const { queryClient } = renderSettings(baseSnapshot({
      storeAccounts: [account({
        state: 'signingIn',
        accountId: null,
        accountName: null,
        ownedCount: 0,
        familyCount: 0,
        installedCount: 3,
        lastCheckedAt: null,
        canSignIn: false,
        canCancelSignIn: true,
        canFocusSignIn: true,
        canCheck: false,
        canSignOut: false,
      })],
    }), 'en');

    // Let the modal focus trap finish its initial focus before taking over.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Account' })).toHaveFocus());
    const storesNav = screen.getByRole('button', { name: 'Stores' });
    storesNav.focus();
    expect(storesNav).toHaveFocus();

    updateSnapshot(queryClient, baseSnapshot({
      storeAccounts: [account({
        ownedCount: 312,
        familyCount: 8,
        installedCount: 18,
        lastSyncedAt: 1_700_000_120,
      })],
    }));

    expect(await screen.findByText('Steam connected. 320 games imported.')).toBeInTheDocument();
    expect(storesNav).toHaveFocus();
  });

  it('announces sign-out and leaves focus on the sign-out control', () => {
    const signOut = vi.spyOn(commands, 'providerSignOut').mockResolvedValue(undefined);
    openSettingsModal('stores');
    renderSettings(baseSnapshot());

    const signOutButton = screen.getByRole('button', { name: 'Sign out' });
    signOutButton.focus();
    fireEvent.click(signOutButton);

    expect(signOut).toHaveBeenCalledWith('steam');
    expect(screen.getByText('Steam disconnected. Installed games stay in your library.')).toBeInTheDocument();
    expect(document.activeElement).toBe(signOutButton);
  });

  it('shows the Stores attention indicator for expired, credential error and persist failure states', () => {
    openSettingsModal('account');
    const { queryClient } = renderSettings(baseSnapshot({
      storeAccounts: [account({ state: 'expired', canSignIn: true, canCheck: false })],
    }));

    expect(screen.getByRole('button', { name: 'Stores: Steam needs attention' })).toBeInTheDocument();
    expect(screen.getByText('Attention')).toBeInTheDocument();

    updateSnapshot(queryClient, baseSnapshot({
      storeAccounts: [account({ state: 'credentialError', canSignIn: true, canCheck: true, canRemoveCredential: true, canSignOut: false })],
    }));
    expect(screen.getByRole('button', { name: 'Stores: Steam needs attention' })).toBeInTheDocument();

    updateSnapshot(queryClient, baseSnapshot({
      storeAccounts: [account({ persistFailed: true })],
    }));
    expect(screen.getByRole('button', { name: 'Stores: Steam needs attention' })).toBeInTheDocument();
  });

  it('removes the attention indicator on recovery and on sign-out', async () => {
    openSettingsModal('account');
    const { queryClient } = renderSettings(baseSnapshot({
      storeAccounts: [account({ state: 'expired', canSignIn: true, canCheck: false })],
    }));

    expect(screen.getByRole('button', { name: 'Stores: Steam needs attention' })).toBeInTheDocument();

    updateSnapshot(queryClient, baseSnapshot());
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: /Steam needs attention/ })).toBeNull();
    });
    expect(screen.getByRole('button', { name: 'Stores' })).toBeInTheDocument();

    updateSnapshot(queryClient, baseSnapshot({
      storeAccounts: [account({ state: 'notConnected', canSignIn: true, canCheck: false, canSignOut: false })],
    }));
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: /Steam needs attention/ })).toBeNull();
    });
  });

  it('localizes the attention indicator in pt-BR', () => {
    openSettingsModal('account');
    renderSettings(baseSnapshot({
      storeAccounts: [account({ state: 'expired', canSignIn: true, canCheck: false })],
    }), 'pt-BR');

    expect(screen.getByRole('button', { name: 'Lojas: A Steam precisa de atenção' })).toBeInTheDocument();
    expect(screen.getByText('Atenção')).toBeInTheDocument();
  });
});
