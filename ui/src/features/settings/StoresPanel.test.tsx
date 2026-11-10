import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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

const NOW = Math.floor(Date.now() / 1000);

function account(overrides: Partial<StoreAccountView> = {}): StoreAccountView {
  return {
    provider: 'steam',
    state: 'connected',
    accountId: '76561198000000000',
    accountName: 'cassi',
    ownedCount: 5,
    familyCount: 1,
    installedCount: 3,
    lastCheckedAt: NOW,
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

function epicAccount(overrides: Partial<StoreAccountView> = {}): StoreAccountView {
  return account({
    provider: 'epic',
    accountId: 'epic-account-1',
    accountName: 'EpicPlayer',
    ownedCount: 4,
    familyCount: 0,
    installedCount: 2,
    storage: 'secretService',
    ...overrides,
  });
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
    total: 16,
    installedTotal: 11,
    providers: [
      { id: 'steam', name: 'Steam', count: 6, installedCount: 3, ownedCount: 5, familyCount: 1 },
      { id: 'epic', name: 'Epic Games Store', count: 4, installedCount: 2, ownedCount: 4, familyCount: 0 },
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
  snapshot.storeAccounts = [account(), epicAccount()];
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

describe('StoresPanel store account integration', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('renders one card per snapshot account before the other detected stores', () => {
    openSettingsModal('stores');
    renderSettings(baseSnapshot());

    const headings = screen.getAllByRole('heading', { level: 3 }).map((heading) => heading.textContent);
    expect(headings).toEqual([
      'Steam account',
      'Epic Games Store account',
      'Detected stores',
      'DRM-free games',
    ]);
    expect(screen.getByText('Signed in as cassi')).toBeInTheDocument();
    expect(screen.getByText('Signed in as EpicPlayer')).toBeInTheDocument();
    // Only GOG has no account card.
    expect(screen.getByText('GOG')).toBeInTheDocument();
    expect(screen.getByText('6 installed games')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Rescan' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Reset hidden games/i })).toBeInTheDocument();
  });

  it('hides the family row for a provider that never reported family data', () => {
    openSettingsModal('stores');
    renderSettings(baseSnapshot());

    const epicCard = screen.getByRole('region', { name: 'Epic Games Store account' });
    expect(within(epicCard).queryByText('Shared')).toBeNull();
    expect(within(epicCard).queryByText(/family sharing could not be synced/)).toBeNull();

    const steamCard = screen.getByRole('region', { name: 'Steam account' });
    expect(within(steamCard).getByText('Shared')).toBeInTheDocument();
  });

  it('renders the store card and sign-in without a GameNode session', () => {
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

  it('checks a stale session for each provider and leaves a fresh one alone', async () => {
    const check = vi.spyOn(commands, 'providerCheck').mockResolvedValue(undefined);
    openSettingsModal('stores');
    const { unmount } = renderSettings(baseSnapshot({
      storeAccounts: [
        account({ lastCheckedAt: NOW - 10 * 60 }),
        epicAccount({ lastCheckedAt: NOW - 60 }),
      ],
    }));

    await waitFor(() => expect(check).toHaveBeenCalledWith('steam'));
    expect(check).not.toHaveBeenCalledWith('epic');
    unmount();

    check.mockClear();
    openSettingsModal('stores');
    renderSettings(baseSnapshot({
      storeAccounts: [
        account({ lastCheckedAt: NOW - 60 }),
        epicAccount({ lastCheckedAt: NOW - 60 }),
      ],
    }));
    await act(async () => { await Promise.resolve(); });
    expect(check).not.toHaveBeenCalled();
  });

  it('announces a connected account with the imported game count without stealing focus', async () => {
    openSettingsModal('stores');
    const { queryClient } = renderSettings(baseSnapshot({
      storeAccounts: [
        account({
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
        }),
        epicAccount(),
      ],
    }), 'en');

    // Let the modal focus trap finish its initial focus before taking over.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Account' })).toHaveFocus());
    const storesNav = screen.getByRole('button', { name: 'Stores' });
    storesNav.focus();
    expect(storesNav).toHaveFocus();

    updateSnapshot(queryClient, baseSnapshot({
      storeAccounts: [
        account({
          ownedCount: 312,
          familyCount: 8,
          installedCount: 18,
          lastSyncedAt: 1_700_000_120,
        }),
        epicAccount(),
      ],
    }));

    expect(await screen.findByText('Steam connected. 320 games imported.')).toBeInTheDocument();
    expect(storesNav).toHaveFocus();
  });

  it('signs out only the card whose action was used', () => {
    const signOut = vi.spyOn(commands, 'providerSignOut').mockResolvedValue(undefined);
    openSettingsModal('stores');
    renderSettings(baseSnapshot());

    const epicCard = screen.getByRole('region', { name: 'Epic Games Store account' });
    const epicSignOut = within(epicCard).getByRole('button', { name: 'Sign out' });
    epicSignOut.focus();
    fireEvent.click(epicSignOut);

    expect(signOut).toHaveBeenCalledTimes(1);
    expect(signOut).toHaveBeenCalledWith('epic');
    expect(screen.getByText('Epic Games Store disconnected. Installed games stay in your library.')).toBeInTheDocument();
    expect(document.activeElement).toBe(epicSignOut);
  });

  it('shows the Stores attention indicator for expired, credential error and persist failure states', () => {
    openSettingsModal('account');
    const { queryClient } = renderSettings(baseSnapshot({
      storeAccounts: [account({ state: 'expired', canSignIn: true, canCheck: false }), epicAccount()],
    }));

    expect(screen.getByRole('button', { name: 'Stores: Steam needs attention' })).toBeInTheDocument();
    expect(screen.getByText('Attention')).toBeInTheDocument();

    updateSnapshot(queryClient, baseSnapshot({
      storeAccounts: [account({ state: 'credentialError', canSignIn: true, canCheck: true, canRemoveCredential: true, canSignOut: false }), epicAccount()],
    }));
    expect(screen.getByRole('button', { name: 'Stores: Steam needs attention' })).toBeInTheDocument();

    updateSnapshot(queryClient, baseSnapshot({
      storeAccounts: [account({ persistFailed: true }), epicAccount()],
    }));
    expect(screen.getByRole('button', { name: 'Stores: Steam needs attention' })).toBeInTheDocument();
  });

  it('names the provider that needs attention', () => {
    openSettingsModal('account');
    renderSettings(baseSnapshot({
      storeAccounts: [account(), epicAccount({ state: 'expired', canSignIn: true, canCheck: false })],
    }));

    expect(screen.getByRole('button', { name: 'Stores: Epic Games Store needs attention' })).toBeInTheDocument();
  });

  it('removes the attention indicator on recovery and on sign-out', async () => {
    openSettingsModal('account');
    const { queryClient } = renderSettings(baseSnapshot({
      storeAccounts: [account({ state: 'expired', canSignIn: true, canCheck: false }), epicAccount()],
    }));

    expect(screen.getByRole('button', { name: 'Stores: Steam needs attention' })).toBeInTheDocument();

    updateSnapshot(queryClient, baseSnapshot());
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: /needs attention/ })).toBeNull();
    });
    expect(screen.getByRole('button', { name: 'Stores' })).toBeInTheDocument();

    updateSnapshot(queryClient, baseSnapshot({
      storeAccounts: [account({ state: 'notConnected', canSignIn: true, canCheck: false, canSignOut: false }), epicAccount()],
    }));
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: /needs attention/ })).toBeNull();
    });
  });

  it('localizes the attention indicator in pt-BR', () => {
    openSettingsModal('account');
    renderSettings(baseSnapshot({
      storeAccounts: [account({ state: 'expired', canSignIn: true, canCheck: false }), epicAccount()],
    }), 'pt-BR');

    expect(screen.getByRole('button', { name: 'Lojas: Steam precisa de atenção' })).toBeInTheDocument();
    expect(screen.getByText('Atenção')).toBeInTheDocument();
  });
});
