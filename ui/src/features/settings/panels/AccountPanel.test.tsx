import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../../AppProviders';
import {
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultLibraryView,
  createDefaultSnapshot,
  type LibraryView,
  type Snapshot,
} from '../../../api';
import { I18nProvider } from '../../../i18n';
import { resetUiStore } from '../../../store/uiStore';
import { ToastContainer } from '../ToastContainer';
import { AccountPanel } from './AccountPanel';

function library(access: LibraryView['access'] = 'verified'): LibraryView {
  return {
    ...createDefaultLibraryView(),
    access,
    layout: { ...createDefaultLibraryView().layout, view: 'grid' },
  };
}

function signedInSnapshot(overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    ...createDefaultSnapshot(),
    auth: { signedIn: { user: { id: 'u1', username: 'cassi' } } },
    storage: 'secretService',
    library: library(),
    ...overrides,
  };
}

function renderPanel(snapshot: Snapshot, locale = 'en') {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  return {
    queryClient,
    ...render(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale={locale}>
            <ToastContainer />
            <AccountPanel />
          </I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    ),
  };
}

function stubOpenProfile() {
  return vi.spyOn(commands, 'openGameNodeLink').mockResolvedValue(undefined);
}

describe('AccountPanel', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('shows the connected identity, session storage and account actions', () => {
    renderPanel(
      signedInSnapshot({
        auth: {
          signedIn: {
            user: { id: 'user-77', username: 'AlexRider', email: 'alex@example.com' },
          },
        },
      }),
    );

    expect(screen.getByRole('heading', { name: 'AlexRider' })).toBeInTheDocument();
    expect(screen.getByText('Session saved in the system keyring')).toBeInTheDocument();
    expect(screen.getByText('Signed in')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open profile' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign out' })).toBeInTheDocument();
  });

  it('opens the profile through the host link command and stays mounted', () => {
    const openLink = stubOpenProfile();
    renderPanel(signedInSnapshot());

    fireEvent.click(screen.getByRole('button', { name: 'Open profile' }));

    expect(openLink).toHaveBeenCalledWith('profile');
    expect(screen.getByRole('heading', { name: 'cassi' })).toBeInTheDocument();
  });

  it('shows an inline error when the profile link cannot open', async () => {
    const openLink = stubOpenProfile();
    openLink.mockRejectedValue(new Error('no browser'));
    renderPanel(signedInSnapshot());

    fireEvent.click(screen.getByRole('button', { name: 'Open profile' }));

    expect(
      await screen.findByText('Could not open your profile.'),
    ).toBeInTheDocument();
  });

  it('signs out through the app auth action', () => {
    const signOut = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue(undefined);
    renderPanel(signedInSnapshot());

    fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));

    expect(signOut).toHaveBeenCalledWith('signOut');
  });

  it('offers browser sign-in without identity, sign out or Sync now when signed out', () => {
    const startSignIn = vi
      .spyOn(commands, 'dispatchAuth')
      .mockResolvedValue(undefined);
    renderPanel({
      ...createDefaultSnapshot(),
      auth: 'signedOut',
      library: null,
    });

    expect(screen.queryByText('cassi')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Sign out' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Sync now' })).toBeNull();
    expect(screen.getByText('No session')).toBeInTheDocument();
    expect(
      screen.getByText('Sign in with your GameNode account in your browser.'),
    ).toBeInTheDocument();
    expect(screen.getByText('Playtime source')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Continue with Discord' }));
    expect(startSignIn).toHaveBeenCalledWith({ startSignIn: 'discord' });
  });

  it('disables Sync now while sync is off', () => {
    renderPanel(
      signedInSnapshot({
        sync: { enabled: false, explained: true, summary: null, running: false },
      }),
    );

    expect(screen.getByText('Sync is off')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sync now' })).toBeDisabled();
  });

  it('disables Sync now while a pass is running', () => {
    renderPanel(
      signedInSnapshot({
        sync: { enabled: true, explained: true, summary: null, running: true },
      }),
    );

    expect(screen.getByRole('button', { name: 'Syncing…' })).toBeDisabled();
  });

  it('disables Sync now without a verified session', () => {
    renderPanel(signedInSnapshot({ library: library('unverified') }));

    expect(screen.getByRole('button', { name: 'Sync now' })).toBeDisabled();
  });

  it('shows the last sync summary and starts Sync now', async () => {
    const syncNow = vi.spyOn(commands, 'syncNow').mockResolvedValue(undefined);
    const completedAt = Math.floor(Date.now() / 1000) - 120;
    renderPanel(
      signedInSnapshot({
        sync: {
          enabled: true,
          explained: true,
          summary: {
            completedAt,
            acceptedGames: 248,
            stores: 3,
            held: 0,
          },
          running: false,
        },
      }),
    );

    expect(
      screen.getByText('248 games · 3 stores · no errors'),
    ).toBeInTheDocument();
    expect(screen.getByText(/2 min ago/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Sync now' }));
    expect(syncNow).toHaveBeenCalledTimes(1);
  });

  it('shows held totals and Never synced states', () => {
    const { unmount } = renderPanel(
      signedInSnapshot({
        sync: {
          enabled: true,
          explained: true,
          summary: {
            completedAt: Math.floor(Date.now() / 1000) - 60,
            acceptedGames: 10,
            stores: 2,
            held: 2,
          },
          running: false,
        },
      }),
    );
    expect(
      screen.getByText('10 games · 2 stores · 2 pending'),
    ).toBeInTheDocument();
    unmount();

    renderPanel(signedInSnapshot());
    expect(screen.getByText('Never synced')).toBeInTheDocument();
  });

  it('reports a failed Sync now', async () => {
    vi.spyOn(commands, 'syncNow').mockRejectedValue('syncUnavailable');
    renderPanel(signedInSnapshot());

    fireEvent.click(screen.getByRole('button', { name: 'Sync now' }));

    expect(
      await screen.findByText('Could not sync now. Try again.'),
    ).toBeInTheDocument();
  });

  it('rolls the sync switch back and shows an error when saving fails', async () => {
    vi.spyOn(commands, 'setSyncEnabled').mockRejectedValue('offline');
    renderPanel(signedInSnapshot());

    const syncSwitch = screen.getByRole('switch', { name: 'Sync with GameNode' });
    expect(syncSwitch).toBeChecked();

    fireEvent.click(syncSwitch);

    expect(
      await screen.findByText(
        'Could not save this preference. The control was restored to the saved value.',
      ),
    ).toBeInTheDocument();
    await waitFor(() => expect(syncSwitch).toBeChecked());
  });

  it('rolls the playtime source back and shows an error when saving fails', async () => {
    vi.spyOn(commands, 'setPlaytimeSource').mockRejectedValue('offline');
    renderPanel(signedInSnapshot({ playtimeSource: 'store' }));

    fireEvent.click(screen.getByRole('radio', { name: 'Internal' }));

    expect(
      await screen.findByText(
        'Could not save this preference. The control was restored to the saved value.',
      ),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole('radio', { name: 'Store' })).toBeChecked(),
    );
  });

  it('applies the playtime source immediately and shows its toast', async () => {
    const setSource = vi
      .spyOn(commands, 'setPlaytimeSource')
      .mockResolvedValue(undefined);
    renderPanel(signedInSnapshot({ playtimeSource: 'store' }));

    fireEvent.click(screen.getByRole('radio', { name: 'Internal' }));

    expect(setSource).toHaveBeenCalledWith('internal');
    const notifications = await screen.findByRole('region', {
      name: 'Notifications',
    });
    expect(within(notifications).getByText('Playtime source')).toBeInTheDocument();
  });

  it('lists what is and is not synced', () => {
    renderPanel(signedInSnapshot());

    for (const item of [
      'Playtime and last session',
      'Status and chosen collections',
      'Save files',
      'Local paths and executables',
    ]) {
      expect(screen.getByText(item)).toBeInTheDocument();
    }
  });

  it('localizes the account panel in pt-BR', () => {
    renderPanel(signedInSnapshot(), 'pt-BR');

    expect(screen.getByRole('heading', { name: 'cassi' })).toBeInTheDocument();
    expect(screen.getByText('Sessão salva no chaveiro do sistema')).toBeInTheDocument();
    expect(screen.getByText('Conectado')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Abrir perfil' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sair' })).toBeInTheDocument();
  });
});
