import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { commands, type StoreAccountView } from '../../api';
import { I18nProvider } from '../../i18n';
import { SteamAccountCard } from './SteamAccountCard';

function account(overrides: Partial<StoreAccountView> = {}): StoreAccountView {
  return {
    provider: 'steam',
    state: 'notConnected',
    accountId: null,
    accountName: null,
    ownedCount: 0,
    familyCount: 0,
    installedCount: 0,
    lastCheckedAt: null,
    storage: 'secretService',
    persistFailed: false,
    catalogError: null,
    familyError: null,
    canSignIn: false,
    canCancelSignIn: false,
    canFocusSignIn: false,
    canSignOut: false,
    canCheck: false,
    canRemoveCredential: false,
    ...overrides,
  };
}

function renderCard(value: StoreAccountView, locale = 'en') {
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <I18nProvider initialLocale={locale}>
        <SteamAccountCard account={value} />
      </I18nProvider>
    </MantineProvider>,
  );
}

describe('SteamAccountCard', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('shows the connected identity, counts, last check and storage method', () => {
    renderCard(account({
      state: 'connected',
      accountId: '76561198000000000',
      accountName: 'cassi',
      ownedCount: 312,
      familyCount: 8,
      installedCount: 18,
      lastCheckedAt: 1_700_000_000,
      storage: 'secretService',
      canCheck: true,
      canSignOut: true,
    }));

    expect(screen.getByRole('region', { name: 'Steam account' })).toBeInTheDocument();
    expect(screen.getByText('Signed in as cassi')).toBeInTheDocument();
    expect(screen.getByText('Connected')).toBeInTheDocument();
    expect(screen.getByText('312')).toBeInTheDocument();
    expect(screen.getByText('8')).toBeInTheDocument();
    expect(screen.getByText('18')).toBeInTheDocument();
    expect(screen.getByText(/Last check:/)).toBeInTheDocument();
    expect(screen.getByText('Credential storage: Secret Service')).toBeInTheDocument();
    expect(screen.getByText('18 installed games')).toBeInTheDocument();
    expect(screen.getByText(/never sees your Steam password/)).toBeInTheDocument();
  });

  it('offers sign-in when there is no account', () => {
    const signIn = vi.spyOn(commands, 'providerSignIn');
    renderCard(account({ canSignIn: true }));

    expect(screen.getByText('Not connected')).toBeInTheDocument();
    expect(screen.getByText(/even when they are not installed on this PC/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Sign in with Steam' }));
    expect(signIn).toHaveBeenCalledWith('steam');
  });

  it('shows progress without actions while checking', () => {
    renderCard(account({ state: 'checking', accountName: 'cassi', ...checkedCounts() }));

    expect(screen.getByText('Checking…')).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Steam account' })).toHaveAttribute('aria-busy', 'true');
    expect(screen.queryAllByRole('button')).toHaveLength(0);
  });

  it('offers focus and cancel while signing in', () => {
    const focus = vi.spyOn(commands, 'providerFocusSignIn');
    const cancel = vi.spyOn(commands, 'providerCancelSignIn');
    renderCard(account({
      state: 'signingIn',
      canFocusSignIn: true,
      canCancelSignIn: true,
    }));

    expect(screen.getByText('Waiting for sign-in')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Bring window to front' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(focus).toHaveBeenCalledWith('steam');
    expect(cancel).toHaveBeenCalledWith('steam');
  });

  it('offers sync and sign out when connected', () => {
    const check = vi.spyOn(commands, 'providerCheck');
    const signOut = vi.spyOn(commands, 'providerSignOut');
    const onSignOut = vi.fn();
    render(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <I18nProvider initialLocale="en">
          <SteamAccountCard
            account={account({ state: 'connected', canCheck: true, canSignOut: true })}
            onSignOut={onSignOut}
          />
        </I18nProvider>
      </MantineProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Sync now' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(check).toHaveBeenCalledWith('steam');
    expect(signOut).toHaveBeenCalledWith('steam');
    expect(onSignOut).toHaveBeenCalledTimes(1);
  });

  it('offers sign in again and sign out when expired and keeps the installed-games explanation', () => {
    const signIn = vi.spyOn(commands, 'providerSignIn');
    const signOut = vi.spyOn(commands, 'providerSignOut');
    renderCard(account({
      state: 'expired',
      accountName: 'cassi',
      catalogError: 'expired',
      canSignIn: true,
      canSignOut: true,
    }));

    expect(screen.getByText('Session expired')).toBeInTheDocument();
    expect(screen.getByText(/Installed games still work/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Sign in again' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(signIn).toHaveBeenCalledWith('steam');
    expect(signOut).toHaveBeenCalledWith('steam');
  });

  it('offers retry and sign out when Steam is unreachable', () => {
    const check = vi.spyOn(commands, 'providerCheck');
    const signOut = vi.spyOn(commands, 'providerSignOut');
    renderCard(account({
      state: 'unreachable',
      accountName: 'cassi',
      catalogError: 'unreachable',
      canCheck: true,
      canSignOut: true,
    }));

    expect(screen.getByText('Steam unreachable')).toBeInTheDocument();
    expect(screen.getByText(/Showing your saved Steam library/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(check).toHaveBeenCalledWith('steam');
    expect(signOut).toHaveBeenCalledWith('steam');
  });

  it('offers retry, sign in again and removing the saved credential on a credential error', () => {
    const check = vi.spyOn(commands, 'providerCheck');
    const signIn = vi.spyOn(commands, 'providerSignIn');
    const signOut = vi.spyOn(commands, 'providerSignOut');
    renderCard(account({
      state: 'credentialError',
      canCheck: true,
      canSignIn: true,
      canRemoveCredential: true,
    }));

    expect(screen.getByText('Credential error')).toBeInTheDocument();
    expect(screen.getByText(/Unlock your keyring and retry/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    fireEvent.click(screen.getByRole('button', { name: 'Sign in again' }));
    fireEvent.click(screen.getByRole('button', { name: 'Remove saved sign-in' }));
    expect(check).toHaveBeenCalledWith('steam');
    expect(signIn).toHaveBeenCalledWith('steam');
    expect(signOut).toHaveBeenCalledWith('steam');
  });

  it('keeps the connected actions and warns when the credential could not be saved', () => {
    renderCard(account({
      state: 'connected',
      accountName: 'cassi',
      persistFailed: true,
      canCheck: true,
      canSignOut: true,
    }));

    expect(screen.getByText('Connected')).toBeInTheDocument();
    expect(screen.getByText(/will not be restored when you restart GameNode/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sync now' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign out' })).toBeInTheDocument();
  });

  it('shows a non-blocking family warning when the family sync failed', () => {
    renderCard(account({
      state: 'connected',
      accountName: 'cassi',
      familyCount: 8,
      familyError: 'familyUnavailable',
      canCheck: true,
      canSignOut: true,
    }));

    expect(screen.getByText(/Steam Family could not be synced/)).toBeInTheDocument();
    expect(screen.getByText('8')).toBeInTheDocument();
  });

  it('warns about the plain-text private file before sign-in and while connected', () => {
    const { unmount } = renderCard(account({ storage: 'privateFile', canSignIn: true }));
    expect(screen.getByText(/The file is not encrypted/)).toBeInTheDocument();
    expect(screen.getByText('Credential storage: private file (not encrypted)')).toBeInTheDocument();
    unmount();

    renderCard(account({
      state: 'connected',
      accountName: 'cassi',
      storage: 'privateFile',
      canCheck: true,
      canSignOut: true,
    }));
    expect(screen.getByText(/The file is not encrypted/)).toBeInTheDocument();
  });

  it('does not show the plain-text warning for keyring storage', () => {
    renderCard(account({
      state: 'connected',
      accountName: 'cassi',
      storage: 'windowsCredentialManager',
      canCheck: true,
      canSignOut: true,
    }));

    expect(screen.queryByText(/The file is not encrypted/)).toBeNull();
    expect(screen.getByText('Credential storage: Windows Credential Manager')).toBeInTheDocument();
  });

  it('localizes the status and actions in pt-BR', () => {
    renderCard(account({
      state: 'connected',
      accountName: 'cassi',
      storage: 'secretService',
      canCheck: true,
      canSignOut: true,
    }), 'pt-BR');

    expect(screen.getByText('Conectada')).toBeInTheDocument();
    expect(screen.getByText('Conectada como cassi')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sincronizar agora' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sair' })).toBeInTheDocument();
    expect(screen.getByText(/O GameNode nunca vê sua senha da Steam/)).toBeInTheDocument();
  });
});

function checkedCounts() {
  return {
    ownedCount: 312,
    familyCount: 8,
    installedCount: 18,
    lastCheckedAt: 1_700_000_000,
  };
}
