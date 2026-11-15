import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactElement } from 'react';
import type { StoreAccountView } from '../../api';
import { commands } from '../../api/commands';
import { I18nProvider } from '../../i18n';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { deriveLibraryBanners, LibraryBanners } from './LibraryBanners';

function account(
  overrides: Partial<StoreAccountView> = {},
): StoreAccountView {
  return {
    provider: 'steam',
    state: 'connected',
    accountId: 'steam-1',
    accountName: 'cassi',
    ownedCount: 42,
    sharedCount: 10,
    sharedLabel: 'Steam Family',
    installedCount: 18,
    lastCheckedAt: 1_700_000_000,
    lastSyncedAt: 1_700_000_000,
    storage: null,
    persistFailed: false,
    catalogError: null,
    sharedError: null,
    canSignIn: true,
    canCancelSignIn: false,
    canFocusSignIn: false,
    canSignOut: true,
    canCheck: true,
    canRemoveCredential: true,
    ...overrides,
  };
}

function wrap(ui: ReactElement) {
  return render(<I18nProvider initialLocale="en">{ui}</I18nProvider>);
}

describe('LibraryBanners', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('orders every kind offline → expired → unreachable → discovered and keeps one per kind', () => {
    const banners = deriveLibraryBanners({
      access: 'offline',
      storeAccounts: [
        account({ provider: 'steam', state: 'expired', lastSyncedAt: 1 }),
        account({ provider: 'epic', state: 'expired', lastSyncedAt: 2 }),
        account({ provider: 'gog', state: 'unreachable', lastSyncedAt: 3 }),
      ],
      recentlyAdded: ['candidate-1'],
    });

    expect(banners.map((banner) => banner.kind)).toEqual([
      'offline',
      'expired',
      'unreachable',
      'discovered',
    ]);
    expect(banners.filter((banner) => banner.kind === 'expired')).toHaveLength(1);
    expect(banners.find((banner) => banner.kind === 'expired')?.provider).toBe('steam');
  });

  it('renders only the highest-priority banner', () => {
    wrap(
      <LibraryBanners
        access="offline"
        storeAccounts={[account({ state: 'expired' })]}
        recentlyAdded={['candidate-1']}
      />,
    );

    expect(screen.getByTestId('library-banner-offline')).toBeInTheDocument();
    expect(screen.queryByTestId('library-banner-expired')).not.toBeInTheDocument();
    expect(screen.queryByTestId('library-banner-discovered')).not.toBeInTheDocument();
  });

  it('opens Settings › Stores for an expired session, naming the store and its game count', () => {
    wrap(
      <LibraryBanners
        access="verified"
        storeAccounts={[
          account({ state: 'expired', ownedCount: 42, sharedCount: 10 }),
        ]}
        recentlyAdded={[]}
      />,
    );

    expect(screen.getByTestId('library-banner-expired')).toHaveTextContent(
      'The Steam session expired',
    );
    expect(screen.getByTestId('library-banner-expired')).toHaveTextContent(
      'Sign in again to refresh the 52 games from this store.',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Sign in again' }));
    expect(uiStore.state.settingsOpen).toBe(true);
    expect(uiStore.state.settingsTopic).toBe('stores');
  });

  it('retries the library scan from the unreachable banner', () => {
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue();
    wrap(
      <LibraryBanners
        access="verified"
        storeAccounts={[
          account({ provider: 'epic', state: 'unreachable' }),
        ]}
        recentlyAdded={[]}
        providers={[
          {
            id: 'epic',
            name: 'Epic Games Store',
            count: 0,
            installedCount: 0,
            ownedCount: 0,
            sharedCount: 0,
            sharedLabel: null,
          },
        ]}
      />,
    );

    expect(screen.getByTestId('library-banner-unreachable')).toHaveTextContent(
      "Couldn't reach Epic Games Store",
    );
    fireEvent.click(screen.getByRole('button', { name: 'Try now' }));
    expect(rescan).toHaveBeenCalledTimes(1);
  });

  it('opens the discovery review from the discovered banner', () => {
    wrap(
      <LibraryBanners
        access="verified"
        storeAccounts={[]}
        recentlyAdded={['a', 'b', 'c']}
      />,
    );

    expect(screen.getByTestId('library-banner-discovered')).toHaveTextContent(
      '3 new games discovered',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Review' }));
    expect(uiStore.state.discoveryModalOpen).toBe(true);
  });

  it('dismisses the visible banner for the session and reveals the next one', () => {
    wrap(
      <LibraryBanners
        access="offline"
        storeAccounts={[account({ state: 'expired' })]}
        recentlyAdded={[]}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByTestId('library-banner-offline')).not.toBeInTheDocument();
    expect(screen.getByTestId('library-banner-expired')).toBeInTheDocument();
    expect(uiStore.state.dismissedBanners.size).toBe(1);
  });

  it('shows the same problem again after a real recovery', async () => {
    const expired = account({ state: 'expired', lastSyncedAt: 10 });
    const view = wrap(
      <LibraryBanners
        access="verified"
        storeAccounts={[expired]}
        recentlyAdded={[]}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByTestId('library-banner-expired')).not.toBeInTheDocument();

    view.rerender(
      <I18nProvider initialLocale="en">
        <LibraryBanners
          access="verified"
          storeAccounts={[account({ state: 'connected', lastSyncedAt: 20 })]}
          recentlyAdded={[]}
        />
      </I18nProvider>,
    );
    await waitFor(() => expect(uiStore.state.dismissedBanners.size).toBe(0));

    act(() => {
      view.rerender(
        <I18nProvider initialLocale="en">
          <LibraryBanners
            access="verified"
            storeAccounts={[account({ state: 'expired', lastSyncedAt: 30 })]}
            recentlyAdded={[]}
          />
        </I18nProvider>,
      );
    });
    expect(screen.getByTestId('library-banner-expired')).toBeInTheDocument();
  });

  it('does not repeat a dismissed banner when the same failure retries without recovering', () => {
    const expired = account({ state: 'expired', lastSyncedAt: 10 });
    const view = wrap(
      <LibraryBanners
        access="verified"
        storeAccounts={[expired]}
        recentlyAdded={[]}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));

    view.rerender(
      <I18nProvider initialLocale="en">
        <LibraryBanners
          access="verified"
          storeAccounts={[account({ state: 'checking', lastSyncedAt: 10 })]}
          recentlyAdded={[]}
        />
      </I18nProvider>,
    );
    view.rerender(
      <I18nProvider initialLocale="en">
        <LibraryBanners
          access="verified"
          storeAccounts={[account({ state: 'expired', lastSyncedAt: 10 })]}
          recentlyAdded={[]}
        />
      </I18nProvider>,
    );

    expect(screen.queryByTestId('library-banner-expired')).not.toBeInTheDocument();
  });

  it('derives the same problems from a connected account whose catalog sync failed', () => {
    const banners = deriveLibraryBanners({
      access: 'offline',
      storeAccounts: [
        account({
          provider: 'steam',
          state: 'connected',
          catalogError: 'expired',
          lastSyncedAt: 5,
        }),
        account({
          provider: 'epic',
          state: 'connected',
          catalogError: 'unreachable',
          lastSyncedAt: 6,
        }),
      ],
      recentlyAdded: ['candidate-1'],
    });

    expect(banners.map((banner) => banner.kind)).toEqual([
      'offline',
      'expired',
      'unreachable',
      'discovered',
    ]);
    expect(banners.find((banner) => banner.kind === 'expired')?.provider).toBe(
      'steam',
    );
    expect(
      banners.find((banner) => banner.kind === 'unreachable')?.provider,
    ).toBe('epic');
  });

  it('opens Settings › Stores when a connected account reports an expired catalog credential', () => {
    wrap(
      <LibraryBanners
        access="verified"
        storeAccounts={[
          account({ state: 'connected', catalogError: 'expired' }),
        ]}
        recentlyAdded={[]}
      />,
    );

    expect(screen.getByTestId('library-banner-expired')).toHaveTextContent(
      'The Steam session expired',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Sign in again' }));
    expect(uiStore.state.settingsOpen).toBe(true);
    expect(uiStore.state.settingsTopic).toBe('stores');
  });

  it('retries the scan when a connected account reports an unreachable catalog', () => {
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue();
    wrap(
      <LibraryBanners
        access="verified"
        storeAccounts={[
          account({ state: 'connected', catalogError: 'unreachable' }),
        ]}
        recentlyAdded={[]}
      />,
    );

    expect(screen.getByTestId('library-banner-unreachable')).toHaveTextContent(
      "Couldn't reach Steam",
    );
    fireEvent.click(screen.getByRole('button', { name: 'Try now' }));
    expect(rescan).toHaveBeenCalledTimes(1);
  });

  it('keeps a catalog-error dismissal until recovery, then shows the next occurrence', async () => {
    const catalogDown = account({
      state: 'connected',
      catalogError: 'unreachable',
      lastSyncedAt: 10,
    });
    const view = wrap(
      <LibraryBanners
        access="verified"
        storeAccounts={[catalogDown]}
        recentlyAdded={[]}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(
      screen.queryByTestId('library-banner-unreachable'),
    ).not.toBeInTheDocument();

    // Another snapshot with the same problem while the account still says connected must not
    // clear the dismissal: connected alone is not a recovery while catalogError persists.
    view.rerender(
      <I18nProvider initialLocale="en">
        <LibraryBanners
          access="verified"
          storeAccounts={[
            account({
              state: 'connected',
              catalogError: 'unreachable',
              lastSyncedAt: 10,
            }),
          ]}
          recentlyAdded={[]}
        />
      </I18nProvider>,
    );
    expect(
      screen.queryByTestId('library-banner-unreachable'),
    ).not.toBeInTheDocument();
    expect(uiStore.state.dismissedBanners.size).toBe(1);

    // A retry in progress is not a recovery either.
    view.rerender(
      <I18nProvider initialLocale="en">
        <LibraryBanners
          access="verified"
          storeAccounts={[
            account({
              state: 'checking',
              catalogError: 'unreachable',
              lastSyncedAt: 10,
            }),
          ]}
          recentlyAdded={[]}
        />
      </I18nProvider>,
    );
    expect(uiStore.state.dismissedBanners.size).toBe(1);

    // A successful sync clears the dismissal.
    view.rerender(
      <I18nProvider initialLocale="en">
        <LibraryBanners
          access="verified"
          storeAccounts={[
            account({
              state: 'connected',
              catalogError: null,
              lastSyncedAt: 20,
            }),
          ]}
          recentlyAdded={[]}
        />
      </I18nProvider>,
    );
    await waitFor(() => expect(uiStore.state.dismissedBanners.size).toBe(0));
    expect(
      screen.queryByTestId('library-banner-unreachable'),
    ).not.toBeInTheDocument();

    // The same problem returning after the recovery is a new occurrence and shows again.
    act(() => {
      view.rerender(
        <I18nProvider initialLocale="en">
          <LibraryBanners
            access="verified"
            storeAccounts={[
              account({
                state: 'connected',
                catalogError: 'unreachable',
                lastSyncedAt: 30,
              }),
            ]}
            recentlyAdded={[]}
          />
        </I18nProvider>,
      );
    });
    expect(screen.getByTestId('library-banner-unreachable')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Try now' }),
    ).toBeInTheDocument();
  });
});
