import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { commands, type StoreAccountView } from '../../api';
import { I18nProvider } from '../../i18n';
import { StoreAccountCard } from './StoreAccountCard';

function account(overrides: Partial<StoreAccountView> = {}): StoreAccountView {
  return {
    provider: 'steam',
    state: 'notConnected',
    accountId: null,
    accountName: null,
    ownedCount: 0,
    sharedCount: 0,
    sharedLabel: null,
    installedCount: 0,
    lastCheckedAt: null,
    lastSyncedAt: null,
    storage: 'secretService',
    persistFailed: false,
    catalogError: null,
    sharedError: null,
    canSignIn: false,
    canCancelSignIn: false,
    canFocusSignIn: false,
    canSignOut: false,
    canCheck: false,
    canRemoveCredential: false,
    ...overrides,
  };
}

function renderCard(
  value: StoreAccountView,
  options: {
    locale?: string;
    providerName?: string;
    includeShared?: boolean;
    onIncludeSharedChange?: (checked: boolean) => void;
    onSignOut?: () => void;
  } = {},
) {
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <I18nProvider initialLocale={options.locale ?? 'en'}>
        <StoreAccountCard
          account={value}
          providerName={options.providerName ?? 'Steam'}
          includeShared={options.includeShared}
          onIncludeSharedChange={options.onIncludeSharedChange}
          onSignOut={options.onSignOut}
        />
      </I18nProvider>
    </MantineProvider>,
  );
}

describe('StoreAccountCard', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('shows the connected identity, sync time, counts and shared switch', () => {
    const includeShared = vi.fn();
    renderCard(
      account({
        state: 'connected',
        accountId: '76561198000000000',
        accountName: 'cassi',
        ownedCount: 312,
        sharedCount: 8,
        sharedLabel: 'Steam Family',
        installedCount: 18,
        lastCheckedAt: 1_700_000_000,
        lastSyncedAt: Math.floor(Date.now() / 1000) - 120,
        canCheck: true,
        canSignOut: true,
      }),
      { onIncludeSharedChange: includeShared },
    );

    expect(screen.getByRole('region', { name: 'Steam' })).toBeInTheDocument();
    expect(screen.getByText(/Account cassi · synced/)).toBeInTheDocument();
    expect(screen.getByText('Connected')).toBeInTheDocument();
    expect(screen.getByText('312')).toBeInTheDocument();
    expect(screen.getByText('in library')).toBeInTheDocument();
    expect(screen.getByText('8')).toBeInTheDocument();
    expect(screen.getByText('shared (Steam Family)')).toBeInTheDocument();
    expect(screen.getByText('18')).toBeInTheDocument();
    expect(screen.getByText('installed on this PC')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sync now' })).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole('switch', { name: 'Include family-shared games' }),
    );
    expect(includeShared).toHaveBeenCalledWith(false);
  });

  it('starts a store check from the primary action', () => {
    const check = vi.spyOn(commands, 'providerCheck').mockResolvedValue(undefined);
    renderCard(
      account({
        state: 'connected',
        accountName: 'cassi',
        canCheck: true,
        canSignOut: true,
      }),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Sync now' }));
    expect(check).toHaveBeenCalledWith('steam');
  });

  it('opens the overflow menu and signs out only this provider', async () => {
    const signOut = vi.spyOn(commands, 'providerSignOut').mockResolvedValue(undefined);
    const onSignOut = vi.fn();
    renderCard(
      account({
        state: 'connected',
        accountName: 'cassi',
        canCheck: true,
        canSignOut: true,
      }),
      { onSignOut },
    );

    fireEvent.click(screen.getByRole('button', { name: 'More actions' }));
    fireEvent.click(
      await screen.findByRole('menuitem', { name: 'Sign out of store' }),
    );

    expect(signOut).toHaveBeenCalledWith('steam');
    expect(onSignOut).toHaveBeenCalledTimes(1);
  });

  it('offers sign-in when there is no account', () => {
    const signIn = vi.spyOn(commands, 'providerSignIn').mockResolvedValue(undefined);
    renderCard(account({ canSignIn: true }));

    expect(screen.getByText('Not connected')).toBeInTheDocument();
    expect(
      screen.getByText(/even when they are not installed on this PC/),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(signIn).toHaveBeenCalledWith('steam');
  });

  it('shows progress without actions while checking', () => {
    renderCard(
      account({
        state: 'checking',
        accountName: 'cassi',
        ownedCount: 312,
        installedCount: 18,
      }),
    );

    expect(screen.getByText('Checking…')).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Steam' })).toHaveAttribute(
      'aria-busy',
      'true',
    );
    expect(screen.queryAllByRole('button')).toHaveLength(0);
  });

  it('offers the sign-in window and cancels it while signing in', async () => {
    const focus = vi
      .spyOn(commands, 'providerFocusSignIn')
      .mockResolvedValue(undefined);
    const cancel = vi
      .spyOn(commands, 'providerCancelSignIn')
      .mockResolvedValue(undefined);
    renderCard(
      account({
        state: 'signingIn',
        canFocusSignIn: true,
        canCancelSignIn: true,
      }),
    );

    expect(screen.getByText('Signing in…')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Show sign-in window' }));
    fireEvent.click(screen.getByRole('button', { name: 'More actions' }));
    fireEvent.click(
      await screen.findByRole('menuitem', { name: 'Cancel sign-in' }),
    );

    expect(focus).toHaveBeenCalledWith('steam');
    expect(cancel).toHaveBeenCalledWith('steam');
  });

  it('keeps the installed-games explanation and the remove row when expired', async () => {
    const signIn = vi.spyOn(commands, 'providerSignIn').mockResolvedValue(undefined);
    const signOut = vi.spyOn(commands, 'providerSignOut').mockResolvedValue(undefined);
    renderCard(
      account({
        state: 'expired',
        accountName: 'cassi',
        ownedCount: 52,
        canSignIn: true,
        canSignOut: true,
      }),
    );

    const card = screen.getByRole('region', { name: 'Steam' });
    expect(card.className).toContain('border-warning');
    expect(screen.getByText('Session expired')).toBeInTheDocument();
    expect(screen.getByText(/Showing your saved library \(52 games\)/)).toBeInTheDocument();
    expect(
      screen.getByText(/Installed games still work\. Sign in again/),
    ).toBeInTheDocument();
    expect(screen.getByText('Remove saved sign-in')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Sign in again' }));
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    expect(signIn).toHaveBeenCalledWith('steam');
    expect(signOut).toHaveBeenCalledWith('steam');

    fireEvent.click(screen.getByRole('button', { name: 'More actions' }));
    expect(
      await screen.findByRole('menuitem', { name: 'Remove saved sign-in' }),
    ).toBeInTheDocument();
  });

  it('offers Try again and Sign out of store when unreachable', async () => {
    const check = vi.spyOn(commands, 'providerCheck').mockResolvedValue(undefined);
    const signOut = vi.spyOn(commands, 'providerSignOut').mockResolvedValue(undefined);
    renderCard(
      account({
        state: 'unreachable',
        accountName: 'cassi',
        canCheck: true,
        canSignOut: true,
      }),
    );

    expect(screen.getByText('Steam unreachable')).toBeInTheDocument();
    expect(screen.getByText(/Showing your saved library/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    fireEvent.click(screen.getByRole('button', { name: 'More actions' }));
    fireEvent.click(
      await screen.findByRole('menuitem', { name: 'Sign out of store' }),
    );

    expect(check).toHaveBeenCalledWith('steam');
    expect(signOut).toHaveBeenCalledWith('steam');
  });

  it('uses the danger border and offers the credential actions', async () => {
    const check = vi.spyOn(commands, 'providerCheck').mockResolvedValue(undefined);
    const signIn = vi.spyOn(commands, 'providerSignIn').mockResolvedValue(undefined);
    renderCard(
      account({
        state: 'credentialError',
        canCheck: true,
        canSignIn: true,
        canRemoveCredential: true,
        canSignOut: false,
      }),
    );

    const card = screen.getByRole('region', { name: 'Steam' });
    expect(card.className).toContain('border-danger');
    expect(screen.getByText('Credential error')).toBeInTheDocument();
    expect(screen.getByText(/Unlock your keyring and retry/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Remove' })).toBeEnabled();

    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    fireEvent.click(screen.getByRole('button', { name: 'More actions' }));
    fireEvent.click(
      await screen.findByRole('menuitem', { name: 'Sign in again' }),
    );
    expect(check).toHaveBeenCalledWith('steam');
    expect(signIn).toHaveBeenCalledWith('steam');
  });

  it('warns when the credential could not be saved', () => {
    renderCard(
      account({
        state: 'connected',
        accountName: 'cassi',
        persistFailed: true,
        canCheck: true,
        canSignOut: true,
      }),
    );

    expect(screen.getByText('Connected')).toBeInTheDocument();
    expect(
      screen.getByText(/will not be restored when you restart GameNode/),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sync now' })).toBeInTheDocument();
  });

  it('shows the shared warning with the declared label', () => {
    renderCard(
      account({
        state: 'connected',
        accountName: 'cassi',
        sharedCount: 8,
        sharedLabel: 'Steam Family',
        sharedError: 'sharedUnavailable',
        canCheck: true,
      }),
    );

    expect(
      screen.getByText(/Steam Family could not be synced/),
    ).toBeInTheDocument();
  });

  it('never shows shared data for a provider without shared access', () => {
    renderCard(
      account({
        provider: 'epic',
        state: 'connected',
        accountName: 'EpicPlayer',
        ownedCount: 4,
        sharedCount: 5,
        sharedLabel: null,
        installedCount: 2,
        sharedError: 'sharedUnavailable',
        canCheck: true,
      }),
      { providerName: 'Epic Games Store' },
    );

    expect(
      screen.getByRole('region', { name: 'Epic Games Store' }),
    ).toBeInTheDocument();
    expect(screen.queryByText('shared (Steam Family)')).toBeNull();
    expect(screen.queryByText(/could not be synced/)).toBeNull();
    expect(
      screen.queryByRole('switch', { name: 'Include family-shared games' }),
    ).toBeNull();
  });

  it('keeps private-file credential copy in the credential card, not the account card', () => {
    renderCard(
      account({
        state: 'connected',
        accountName: 'cassi',
        storage: 'privateFile',
        canCheck: true,
      }),
    );

    expect(screen.queryByText('Private file (not encrypted)')).toBeNull();
    expect(screen.queryByText(/The file is not encrypted/)).toBeNull();
  });

  it('localizes the state, stats and actions in pt-BR', async () => {
    renderCard(
      account({
        state: 'connected',
        accountName: 'cassi',
        ownedCount: 3,
        sharedLabel: 'Steam Family',
        sharedCount: 1,
        installedCount: 2,
        canCheck: true,
        canSignOut: true,
      }),
      { locale: 'pt-BR' },
    );

    expect(screen.getByText('Conectado')).toBeInTheDocument();
    expect(screen.getByText('na biblioteca')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sincronizar agora' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Mais ações' }));
    expect(
      await screen.findByRole('menuitem', { name: 'Desconectar' }),
    ).toBeInTheDocument();
  });
});
