import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MantineProvider } from '@mantine/core';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { appTheme } from '../../AppProviders';
import { commands } from '../../api/commands';
import { I18nProvider, type SupportedLocale } from '../../i18n';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { AppMenu, type AppMenuProps } from './AppMenu';

function renderMenu(props: AppMenuProps = {}, locale: SupportedLocale = 'en') {
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark" env="test">
      <I18nProvider initialLocale={locale}>
        <AppMenu {...props} />
      </I18nProvider>
    </MantineProvider>,
  );
}

const openMenu = () => {
  act(() => {
    uiStore.setState((state) => ({ ...state, appMenuOpen: true }));
  });
};

async function clickItem(name: RegExp | string) {
  openMenu();
  fireEvent.click(await screen.findByRole('menuitem', { name }));
}

describe('features/shell/AppMenu', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('opens from the account trigger with the connection header', async () => {
    renderMenu({ isSignedIn: true, userName: 'cassi' });

    fireEvent.click(screen.getByTestId('app-menu-trigger'));
    const menu = await screen.findByTestId('app-menu');

    expect(menu).toHaveAttribute('aria-label', 'Application menu');
    expect(screen.getByTestId('app-menu-header')).toHaveTextContent('cassi');
    expect(screen.getByTestId('app-menu-header')).toHaveTextContent(
      'Connected to GameNode',
    );
    expect(uiStore.state.appMenuOpen).toBe(true);
  });

  it('runs the library, add, rescan, sign-out and quit actions', async () => {
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue();
    const pick = vi
      .spyOn(commands, 'pickLocalExecutable')
      .mockResolvedValue();
    const signOut = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue();
    const quit = vi.spyOn(commands, 'quitApp').mockResolvedValue();

    renderMenu({ isSignedIn: true, userName: 'cassi' });
    act(() => {
      uiStore.setState((state) => ({
        ...state,
        activeDestination: 'ports',
      }));
    });

    await clickItem('Library');
    expect(uiStore.state.activeDestination).toBe('library');
    expect(uiStore.state.appMenuOpen).toBe(false);

    await clickItem('Add a game');
    expect(pick).toHaveBeenCalledWith(null);

    await clickItem('Rescan library');
    expect(rescan).toHaveBeenCalledTimes(1);

    await clickItem('Sign out');
    expect(signOut).toHaveBeenCalledWith('signOut');

    await clickItem(/^Quit GameNode/);
    expect(quit).toHaveBeenCalledTimes(1);
  });

  it('opens Settings and About through the settings modal without touching unsaved state', async () => {
    renderMenu({ isSignedIn: true, userName: 'cassi' });

    await clickItem(/^Settings/);
    expect(uiStore.state.settingsOpen).toBe(true);
    expect(uiStore.state.settingsTopic).toBe('account');
    expect(uiStore.state.appMenuOpen).toBe(false);

    act(() => {
      uiStore.setState((state) => ({ ...state, settingsOpen: false }));
    });

    await clickItem('About and licenses');
    expect(uiStore.state.settingsOpen).toBe(true);
    expect(uiStore.state.settingsTopic).toBe('about');
  });

  it('opens the keyboard shortcuts dialog', async () => {
    renderMenu({ isSignedIn: true, userName: 'cassi' });

    await clickItem(/^Keyboard shortcuts/);

    expect(uiStore.state.shortcutsOpen).toBe(true);
    expect(uiStore.state.appMenuOpen).toBe(false);
  });

  it('hides Sign out and shows the offline header when signed out', async () => {
    renderMenu({ isSignedIn: false });

    openMenu();
    expect(await screen.findByTestId('app-menu-header')).toHaveTextContent(
      'Offline',
    );
    expect(
      screen.queryByRole('menuitem', { name: 'Sign out' }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('menuitem', { name: /Quit GameNode/ }),
    ).toBeInTheDocument();
  });

  it('localizes the menu items', async () => {
    renderMenu({ isSignedIn: true, userName: 'cassi' }, 'pt-BR');

    openMenu();
    expect(
      await screen.findByRole('menuitem', { name: 'Biblioteca' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('menuitem', { name: /Configurações/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('menuitem', { name: /Atalhos de teclado/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('menuitem', { name: /Encerrar GameNode/ }),
    ).toBeInTheDocument();
  });
});
