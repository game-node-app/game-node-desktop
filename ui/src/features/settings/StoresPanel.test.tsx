import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  APP_INFO_QUERY_KEY,
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultLibraryView,
  createDefaultSnapshot,
  type LibraryView,
  type Snapshot,
  type StoreAccountView,
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
    sharedCount: 1,
    sharedLabel: 'Steam Family',
    installedCount: 3,
    lastCheckedAt: NOW,
    lastSyncedAt: null,
    storage: 'secretService',
    persistFailed: false,
    catalogError: null,
    sharedError: null,
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
    sharedCount: 0,
    sharedLabel: null,
    installedCount: 2,
    storage: 'secretService',
    ...overrides,
  });
}

function library(): LibraryView {
  return {
    ...createDefaultLibraryView(),
    generation: 1,
    layout: {
      ...createDefaultLibraryView().layout,
      view: 'grid',
    },
    search: '',
    access: 'verified',
    total: 16,
    installedTotal: 11,
    providers: [
      {
        id: 'steam',
        name: 'Steam',
        count: 6,
        installedCount: 3,
        ownedCount: 5,
        sharedCount: 1,
        sharedLabel: 'Steam Family',
        installRoot: 'C:\\Program Files (x86)\\Steam',
        libraryFolders: 4,
        detected: true,
      },
      {
        id: 'epic',
        name: 'Epic Games Store',
        count: 4,
        installedCount: 2,
        ownedCount: 4,
        sharedCount: 0,
        sharedLabel: null,
        detected: true,
      },
      {
        id: 'gog',
        name: 'GOG',
        count: 6,
        installedCount: 6,
        ownedCount: 0,
        sharedCount: 0,
        sharedLabel: null,
        detected: true,
      },
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
  snapshot.auth = {
    signedIn: { user: { id: 'u1', username: 'Cassi' } },
  };
  snapshot.library = library();
  snapshot.storeAccounts = [account(), epicAccount()];
  return { ...snapshot, ...overrides };
}

function renderSettings(value: Snapshot, locale = 'en') {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, value);
  queryClient.setQueryData(APP_INFO_QUERY_KEY, {
    locale,
    systemLocale: locale,
    version: '0.2.0',
    buildDate: '2026-01-01',
    osVersion: 'Windows 11',
    arch: 'x86_64',
    backgroundSupported: false,
    platform: 'windows',
  });
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

function updateSnapshot(
  queryClient: ReturnType<typeof createAppQueryClient>,
  next: Snapshot,
) {
  act(() => {
    queryClient.setQueryData<Snapshot>(SNAPSHOT_QUERY_KEY, next);
  });
}

describe('StoresPanel', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('renders one card per account, then credential storage and detected stores', () => {
    openSettingsModal('stores');
    renderSettings(baseSnapshot());

    const headings = screen
      .getAllByRole('heading', { level: 3 })
      .map((heading) => heading.textContent);
    expect(headings).toEqual([
      'Steam',
      'Epic Games Store',
      'Credential storage',
      'Detected on this PC',
    ]);
    expect(screen.getByText(/Account cassi · synced/)).toBeInTheDocument();
    expect(screen.getByText(/Account EpicPlayer · synced/)).toBeInTheDocument();

    // Every scanned provider is listed in "Detected on this PC", including the
    // ones that also have an account card.
    expect(
      screen.getByText('C:\\Program Files (x86)\\Steam · 4 libraries'),
    ).toBeInTheDocument();
    expect(screen.getByText('2 installed')).toBeInTheDocument();
    expect(screen.getAllByText('Detected')).toHaveLength(3);
    expect(screen.getByRole('button', { name: 'Rescan' })).toBeInTheDocument();
    expect(
      screen.getAllByRole('button', { name: 'Show hidden games' }).length,
    ).toBeGreaterThan(0);

    // DRM-free discovery no longer lives in Lojas.
    expect(
      screen.queryByText('Watched folders and local discoveries.'),
    ).toBeNull();
    expect(screen.queryByText('Watched folders')).toBeNull();
    expect(screen.queryByText('Playtime source')).toBeNull();
  });

  it('shows shared data only for a provider that declares shared access', () => {
    openSettingsModal('stores');
    renderSettings(baseSnapshot());

    const epicCard = screen.getByRole('region', { name: 'Epic Games Store' });
    expect(within(epicCard).queryByText('Steam Family')).toBeNull();
    expect(
      within(epicCard).queryByRole('switch', {
        name: 'Include family-shared games',
      }),
    ).toBeNull();

    const steamCard = screen.getByRole('region', { name: 'Steam' });
    expect(within(steamCard).getByText('shared (Steam Family)')).toBeInTheDocument();
    expect(
      within(steamCard).getByRole('switch', {
        name: 'Include family-shared games',
      }),
    ).toBeChecked();
  });

  it('writes the shared-games preference and rolls it back on failure', async () => {
    const preference = vi
      .spyOn(commands, 'setAppPreference')
      .mockResolvedValue(undefined);
    openSettingsModal('stores');
    const { unmount } = renderSettings(
      baseSnapshot({ preferences: { ...createDefaultSnapshot().preferences } }),
    );

    let toggle = screen.getByRole('switch', {
      name: 'Include family-shared games',
    });
    fireEvent.click(toggle);
    expect(preference).toHaveBeenCalledWith({ sharedExcluded: ['steam'] });

    unmount();
    preference.mockRejectedValue('offline');
    openSettingsModal('stores');
    renderSettings(
      baseSnapshot({ preferences: { ...createDefaultSnapshot().preferences } }),
    );

    toggle = screen.getByRole('switch', {
      name: 'Include family-shared games',
    });
    fireEvent.click(toggle);
    expect(
      await screen.findByText(
        'Could not save the shared-games preference. The switch was restored.',
      ),
    ).toBeInTheDocument();
    await waitFor(() => expect(toggle).toBeChecked());
  });

  it('renders the store card and sign-in without a GameNode session', () => {
    openSettingsModal('stores');
    renderSettings(
      baseSnapshot({
        auth: 'signedOut',
        library: null,
        storeAccounts: [
          account({
            state: 'notConnected',
            accountId: null,
            accountName: null,
            ownedCount: 0,
            sharedCount: 0,
            installedCount: 0,
            lastCheckedAt: null,
            canSignIn: true,
            canCheck: false,
            canSignOut: false,
          }),
        ],
      }),
    );

    expect(screen.getByRole('heading', { name: 'Steam' })).toBeInTheDocument();
    expect(screen.getByText('Not connected')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeInTheDocument();
    expect(screen.getByText('No installed games found')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Detected on this PC' })).toBeInTheDocument();
  });

  it('checks a stale session for each provider and leaves a fresh one alone', async () => {
    const check = vi.spyOn(commands, 'providerCheck').mockResolvedValue(undefined);
    openSettingsModal('stores');
    const { unmount } = renderSettings(
      baseSnapshot({
        storeAccounts: [
          account({ lastCheckedAt: NOW - 10 * 60 }),
          epicAccount({ lastCheckedAt: NOW - 60 }),
        ],
      }),
    );

    await waitFor(() => expect(check).toHaveBeenCalledWith('steam'));
    expect(check).not.toHaveBeenCalledWith('epic');
    unmount();

    check.mockClear();
    openSettingsModal('stores');
    renderSettings(
      baseSnapshot({
        storeAccounts: [
          account({ lastCheckedAt: NOW - 60 }),
          epicAccount({ lastCheckedAt: NOW - 60 }),
        ],
      }),
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(check).not.toHaveBeenCalled();
  });

  it('announces a connected account with the imported game count without stealing focus', async () => {
    openSettingsModal('stores');
    const { queryClient } = renderSettings(
      baseSnapshot({
        storeAccounts: [
          account({
            state: 'signingIn',
            accountId: null,
            accountName: null,
            ownedCount: 0,
            sharedCount: 0,
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
      }),
      'en',
    );

    await waitFor(() =>
      expect(document.activeElement).not.toBe(document.body),
    );
    const storesNav = screen.getByRole('button', { name: 'Stores' });
    storesNav.focus();
    expect(storesNav).toHaveFocus();

    updateSnapshot(
      queryClient,
      baseSnapshot({
        storeAccounts: [
          account({
            ownedCount: 312,
            sharedCount: 8,
            installedCount: 18,
            lastSyncedAt: 1_700_000_120,
          }),
          epicAccount(),
        ],
      }),
    );

    expect(
      await screen.findByText('Steam connected. 320 games imported.'),
    ).toBeInTheDocument();
    expect(storesNav).toHaveFocus();
  });

  it('signs out only the card whose overflow action was used', async () => {
    const signOut = vi.spyOn(commands, 'providerSignOut').mockResolvedValue(undefined);
    openSettingsModal('stores');
    renderSettings(baseSnapshot());

    const epicCard = screen.getByRole('region', { name: 'Epic Games Store' });
    fireEvent.click(
      within(epicCard).getByRole('button', { name: 'More actions' }),
    );
    fireEvent.click(
      await screen.findByRole('menuitem', { name: 'Sign out of store' }),
    );

    expect(signOut).toHaveBeenCalledTimes(1);
    expect(signOut).toHaveBeenCalledWith('epic');
    expect(
      screen.getByText(
        'Epic Games Store disconnected. Installed games stay in your library.',
      ),
    ).toBeInTheDocument();
  });

  it('shows the credential storage card and the Linux private-file warning', () => {
    openSettingsModal('stores');
    const { unmount } = renderSettings(baseSnapshot());

    const keyring = screen.getByRole('heading', { name: 'Credential storage' })
      .parentElement?.parentElement;
    expect(within(keyring as HTMLElement).getByText('System keyring (Secret Service)')).toBeInTheDocument();
    expect(within(keyring as HTMLElement).getByText('Available')).toBeInTheDocument();
    unmount();

    openSettingsModal('stores');
    renderSettings(baseSnapshot({ storage: 'privateFile' }));
    expect(screen.getByText('Private file (not encrypted)')).toBeInTheDocument();
    expect(screen.getByText('Unavailable')).toBeInTheDocument();
    expect(
      screen.getByText(/The file is not encrypted; install a system keyring/),
    ).toBeInTheDocument();
  });

  it('shows scan failure and the not-found state for detected stores', () => {
    openSettingsModal('stores');
    const next = library();
    next.providers = [
      next.providers[0],
      {
        id: 'gog',
        name: 'GOG',
        count: 0,
        installedCount: 0,
        ownedCount: 0,
        sharedCount: 0,
        sharedLabel: null,
        detected: false,
      },
      {
        id: 'epic',
        name: 'Epic Games Store',
        count: 0,
        installedCount: 0,
        ownedCount: 0,
        sharedCount: 0,
        sharedLabel: null,
        detected: false,
      },
    ];
    next.failedProviders = ['gog'];
    renderSettings(baseSnapshot({ library: next }));

    expect(screen.getByText('Scan failed')).toBeInTheDocument();
    expect(screen.getByText('Not found')).toBeInTheDocument();
    expect(screen.getByText('C:\\Program Files (x86)\\Steam · 4 libraries')).toBeInTheDocument();
  });

  it('keeps catalog-only stores not found and local scans detected behind hidden rows', () => {
    openSettingsModal('stores');
    const next = library();
    next.providers = [
      {
        id: 'steam',
        name: 'Steam',
        // A saved catalog remains online even though the local install is gone.
        count: 312,
        installedCount: 0,
        ownedCount: 312,
        sharedCount: 8,
        sharedLabel: 'Steam Family',
        detected: false,
      },
      {
        id: 'epic',
        name: 'Epic Games Store',
        count: 0,
        installedCount: 0,
        ownedCount: 0,
        sharedCount: 0,
        sharedLabel: null,
        // The current search/filters/hidden state hide every row, but the local
        // scan still found the store.
        detected: true,
      },
    ];
    renderSettings(baseSnapshot({ library: next }));

    expect(screen.getByText('Not found')).toBeInTheDocument();
    expect(screen.getByText('Detected')).toBeInTheDocument();
  });

  it('rescans the library and resets hidden games', () => {
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue(undefined);
    const input = vi.spyOn(commands, 'libraryInput').mockResolvedValue(undefined);
    openSettingsModal('stores');
    renderSettings(baseSnapshot());

    fireEvent.click(screen.getByRole('button', { name: 'Rescan' }));
    expect(rescan).toHaveBeenCalledTimes(1);

    fireEvent.click(
      screen.getByRole('button', { name: 'Show hidden games' }),
    );
    expect(input).toHaveBeenCalledWith('resetExcluded');
    const notifications = screen.getByRole('region', { name: 'Notifications' });
    expect(
      within(notifications).getByText('Show hidden games'),
    ).toBeInTheDocument();
  });

  it('shows the Stores attention indicator for expired, credential error and persist failure states', () => {
    openSettingsModal('account');
    const { queryClient } = renderSettings(
      baseSnapshot({
        storeAccounts: [
          account({ state: 'expired', canSignIn: true, canCheck: false }),
          epicAccount(),
        ],
      }),
    );

    expect(
      screen.getByRole('button', { name: 'Stores: Steam needs attention' }),
    ).toBeInTheDocument();

    updateSnapshot(
      queryClient,
      baseSnapshot({
        storeAccounts: [
          account({
            state: 'credentialError',
            canSignIn: true,
            canCheck: true,
            canRemoveCredential: true,
            canSignOut: false,
          }),
          epicAccount(),
        ],
      }),
    );
    expect(
      screen.getByRole('button', { name: 'Stores: Steam needs attention' }),
    ).toBeInTheDocument();

    updateSnapshot(
      queryClient,
      baseSnapshot({
        storeAccounts: [account({ persistFailed: true }), epicAccount()],
      }),
    );
    expect(
      screen.getByRole('button', { name: 'Stores: Steam needs attention' }),
    ).toBeInTheDocument();
  });

  it('names the provider that needs attention', () => {
    openSettingsModal('account');
    renderSettings(
      baseSnapshot({
        storeAccounts: [
          account(),
          epicAccount({ state: 'expired', canSignIn: true, canCheck: false }),
        ],
      }),
    );

    expect(
      screen.getByRole('button', {
        name: 'Stores: Epic Games Store needs attention',
      }),
    ).toBeInTheDocument();
  });

  it('removes the attention indicator on recovery and on sign-out', async () => {
    openSettingsModal('account');
    const { queryClient } = renderSettings(
      baseSnapshot({
        storeAccounts: [
          account({ state: 'expired', canSignIn: true, canCheck: false }),
          epicAccount(),
        ],
      }),
    );

    expect(
      screen.getByRole('button', { name: 'Stores: Steam needs attention' }),
    ).toBeInTheDocument();

    updateSnapshot(queryClient, baseSnapshot());
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: /needs attention/ })).toBeNull();
    });
    expect(screen.getByRole('button', { name: 'Stores' })).toBeInTheDocument();

    updateSnapshot(
      queryClient,
      baseSnapshot({
        storeAccounts: [
          account({
            state: 'notConnected',
            canSignIn: true,
            canCheck: false,
            canSignOut: false,
          }),
          epicAccount(),
        ],
      }),
    );
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: /needs attention/ })).toBeNull();
    });
  });

  it('localizes the attention indicator in pt-BR', () => {
    openSettingsModal('account');
    renderSettings(
      baseSnapshot({
        storeAccounts: [
          account({ state: 'expired', canSignIn: true, canCheck: false }),
          epicAccount(),
        ],
      }),
      'pt-BR',
    );

    expect(
      screen.getByRole('button', { name: 'Lojas: Steam precisa de atenção' }),
    ).toBeInTheDocument();
  });

  it('counts shared games in the imported toast only for a labelled provider', async () => {
    openSettingsModal('stores');
    const { queryClient } = renderSettings(
      baseSnapshot({
        storeAccounts: [
          account(),
          epicAccount({
            state: 'signingIn',
            accountId: null,
            accountName: null,
            ownedCount: 0,
            sharedCount: 0,
            installedCount: 0,
            lastCheckedAt: null,
            canSignIn: false,
            canCancelSignIn: true,
            canFocusSignIn: true,
            canCheck: false,
            canSignOut: false,
          }),
        ],
      }),
    );

    updateSnapshot(
      queryClient,
      baseSnapshot({
        storeAccounts: [
          account(),
          epicAccount({
            ownedCount: 4,
            sharedCount: 5,
            sharedLabel: null,
            installedCount: 2,
            lastSyncedAt: 1_700_000_120,
          }),
        ],
      }),
    );

    expect(
      await screen.findByText('Epic Games Store connected. 4 games imported.'),
    ).toBeInTheDocument();
  });
});
