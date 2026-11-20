import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../../AppProviders';
import {
  APP_INFO_QUERY_KEY,
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultLibraryLayout,
  createDefaultLibraryView,
  createDefaultSnapshot,
  type AppInfo,
  type AppPreferences,
  type LibraryLayout,
  type LibraryView,
  type Snapshot,
} from '../../../api';
import { I18nProvider } from '../../../i18n';
import { openSettingsModal, resetUiStore } from '../../../store/uiStore';
import { LibraryToolbar } from '../../library/LibraryToolbar';
import { ShortcutsModal } from '../../shell/ShortcutsModal';
import { SettingsModal } from '../SettingsModal';
import { ToastContainer } from '../ToastContainer';

function makeLibrary(layout: Partial<LibraryLayout> = {}): LibraryView {
  return {
    ...createDefaultLibraryView(),
    layout: { ...createDefaultLibraryLayout(), ...layout },
  };
}

interface RenderGeneralOptions {
  platform?: 'windows' | 'linux';
  preferences?: Partial<AppPreferences>;
  layout?: Partial<LibraryLayout>;
  appInfo?: Partial<AppInfo>;
  withToolbar?: boolean;
}

function renderGeneral(options: RenderGeneralOptions = {}) {
  resetUiStore();
  openSettingsModal('general');

  const platform = options.platform ?? 'windows';
  const snapshot: Snapshot = {
    ...createDefaultSnapshot(),
    preferences: {
      ...createDefaultSnapshot().preferences,
      ...options.preferences,
    },
    library: makeLibrary(options.layout),
  };
  const appInfo: AppInfo = {
    locale: 'en',
    systemLocale: 'en',
    version: '0.2.0',
    buildDate: '2026-11-16',
    osVersion: 'Test OS 1.0',
    arch: 'x86_64',
    backgroundSupported: platform === 'windows',
    platform,
    ...options.appInfo,
  };
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  queryClient.setQueryData(APP_INFO_QUERY_KEY, appInfo);

  const result = render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale="en">
          <ToastContainer />
          {options.withToolbar ? <LibraryToolbar /> : null}
          <SettingsModal />
          <ShortcutsModal />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );

  return { ...result, queryClient, snapshot };
}

function publish(
  queryClient: ReturnType<typeof createAppQueryClient>,
  snapshot: Snapshot,
) {
  act(() => {
    queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  });
}

function generalPanel() {
  return screen.getByRole('region', { name: 'General' });
}

describe('GeneralPanel', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('shows the Startup, Library and Language cards with current values on Windows', () => {
    renderGeneral({
      preferences: { startMinimized: false, closeToTray: true },
    });

    expect(
      screen.getByRole('heading', { level: 3, name: 'Startup' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('switch', { name: 'Start with Windows' }),
    ).toBeChecked();
    expect(
      screen.getByRole('switch', { name: 'Start minimized to the tray' }),
    ).not.toBeChecked();
    expect(
      screen.getByRole('switch', {
        name: 'Closing the window minimizes to the tray',
      }),
    ).toBeChecked();
    expect(
      screen.getByText(/Keeps the playtime counter running from sign-in/),
    ).toHaveTextContent('Applies only to installed GameNode builds.');

    expect(
      screen.getByRole('heading', { level: 3, name: 'Library' }),
    ).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Grid' })).toBeChecked();
    expect(screen.getByRole('radio', { name: 'Medium' })).toBeChecked();
    expect(screen.getByRole('combobox', { name: 'Open in' })).toHaveTextContent(
      'Last view',
    );
    expect(
      screen.getByRole('switch', { name: 'Show status bar' }),
    ).toBeChecked();

    expect(
      screen.getByRole('heading', { level: 3, name: 'Language & shortcuts' }),
    ).toBeInTheDocument();
    expect(screen.getByText('?')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'View shortcuts' }),
    ).toBeInTheDocument();
  });

  it('hides the Startup card on Linux', () => {
    renderGeneral({ platform: 'linux' });

    expect(
      screen.queryByRole('heading', { level: 3, name: 'Startup' }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText('Start with Windows')).not.toBeInTheDocument();
    expect(
      screen.getByRole('heading', { level: 3, name: 'Library' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('heading', { level: 3, name: 'Language & shortcuts' }),
    ).toBeInTheDocument();
  });

  it('hides the Startup card when the build does not support background startup', () => {
    renderGeneral({
      platform: 'windows',
      appInfo: { backgroundSupported: false },
    });

    expect(
      screen.queryByRole('heading', { level: 3, name: 'Startup' }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('heading', { level: 3, name: 'Library' }),
    ).toBeInTheDocument();
  });

  it('applies a startup switch immediately and persists it', () => {
    const setPreference = vi
      .spyOn(commands, 'setAppPreference')
      .mockResolvedValue(undefined);

    renderGeneral();
    const toggle = screen.getByRole('switch', { name: 'Start with Windows' });

    fireEvent.click(toggle);

    expect(toggle).not.toBeChecked();
    expect(setPreference).toHaveBeenCalledWith({ startWithWindows: false });
  });

  it('restores the saved value and shows the panel error when persisting fails', async () => {
    vi.spyOn(commands, 'setAppPreference').mockRejectedValue('invalidPreference');

    renderGeneral({ preferences: { startMinimized: true } });
    const toggle = screen.getByRole('switch', {
      name: 'Start minimized to the tray',
    });

    fireEvent.click(toggle);

    await waitFor(() =>
      expect(screen.getByTestId('settings-general-error')).toBeInTheDocument(),
    );
    expect(toggle).toBeChecked();
    expect(screen.getByRole('alert')).toHaveTextContent(
      'The control was restored to the saved value',
    );
  });

  it('swaps the tray help for the quit warning when close to tray is off', () => {
    renderGeneral({ preferences: { closeToTray: false } });

    expect(
      screen.getByText(
        'When off, closing the window quits GameNode and stops counting playtime.',
      ),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole('switch', {
        name: 'Closing the window minimizes to the tray',
      }),
    );

    expect(
      screen.getByText('GameNode keeps counting playtime in the background.'),
    ).toBeInTheDocument();
  });

  it('changes the default view, cover size, open-in scope and status bar through their saved paths', async () => {
    const libraryInput = vi
      .spyOn(commands, 'libraryInput')
      .mockResolvedValue(undefined);
    const setPreference = vi
      .spyOn(commands, 'setAppPreference')
      .mockResolvedValue(undefined);

    renderGeneral({ layout: { view: 'grid', coverSize: 'medium' } });

    fireEvent.click(screen.getByRole('radio', { name: 'List' }));
    expect(libraryInput).toHaveBeenCalledWith({ setView: 'list' });
    expect(screen.getByRole('radio', { name: 'List' })).toBeChecked();

    fireEvent.click(screen.getByRole('radio', { name: 'Large' }));
    expect(libraryInput).toHaveBeenCalledWith({ setCoverSize: 'large' });
    expect(screen.getByRole('radio', { name: 'Large' })).toBeChecked();

    // jsdom gives every box a zero rect, so Floating UI marks the portal hidden;
    // the options are still mounted and clickable, as the other Settings selects.
    fireEvent.click(screen.getByRole('combobox', { name: 'Open in' }));
    fireEvent.click(
      await screen.findByRole('option', { name: 'Installed', hidden: true }),
    );
    expect(setPreference).toHaveBeenCalledWith({ startScope: 'installed' });

    fireEvent.click(screen.getByRole('switch', { name: 'Show status bar' }));
    expect(setPreference).toHaveBeenCalledWith({ showStatusBar: false });
    expect(
      screen.getByRole('switch', { name: 'Show status bar' }),
    ).not.toBeChecked();
  });

  it('keeps the default view in sync with the library toolbar switch', async () => {
    const libraryInput = vi
      .spyOn(commands, 'libraryInput')
      .mockResolvedValue(undefined);

    const { queryClient, snapshot } = renderGeneral({
      layout: { view: 'grid' },
      withToolbar: true,
    });
    const toolbar = screen.getByTestId('library-toolbar');
    expect(
      within(toolbar).getByRole('radio', { name: 'Grid' }),
    ).toBeChecked();

    // The toolbar writes the same layout.view the Settings control reads.
    fireEvent.click(within(toolbar).getByRole('radio', { name: 'List' }));
    expect(libraryInput).toHaveBeenCalledWith({ setView: 'list' });

    publish(queryClient, {
      ...snapshot,
      library: makeLibrary({ view: 'list' }),
    });
    await waitFor(() =>
      expect(
        within(generalPanel()).getByRole('radio', { name: 'List' }),
      ).toBeChecked(),
    );

    // …and changing it in Settings updates the toolbar through the snapshot.
    fireEvent.click(
      within(generalPanel()).getByRole('radio', { name: 'Grid' }),
    );
    expect(libraryInput).toHaveBeenLastCalledWith({ setView: 'grid' });
    publish(queryClient, {
      ...snapshot,
      library: makeLibrary({ view: 'grid' }),
    });
    await waitFor(() => {
      expect(
        within(generalPanel()).getByRole('radio', { name: 'Grid' }),
      ).toBeChecked();
      expect(
        within(toolbar).getByRole('radio', { name: 'Grid' }),
      ).toBeChecked();
    });
  });

  it('switches the interface language at once and persists the preference', async () => {
    const setPreference = vi
      .spyOn(commands, 'setAppPreference')
      .mockResolvedValue(undefined);

    renderGeneral();

    fireEvent.click(screen.getByRole('combobox', { name: 'Language' }));
    fireEvent.click(
      await screen.findByRole('option', {
        name: 'Português (Brasil)',
        hidden: true,
      }),
    );

    expect(setPreference).toHaveBeenCalledWith({ locale: 'ptBr' });
    expect(
      screen.getByRole('heading', { level: 2, name: 'Geral' }),
    ).toBeInTheDocument();
    expect(screen.getByText('Idioma e atalhos')).toBeInTheDocument();
  });

  it('opens the keyboard shortcuts above Settings and returns focus to View shortcuts', async () => {
    const user = userEvent.setup();
    renderGeneral();

    const trigger = screen.getByRole('button', { name: 'View shortcuts' });
    await user.click(trigger);

    expect(await screen.findByTestId('shortcuts-modal')).toBeInTheDocument();
    expect(
      screen.getByRole('dialog', { name: 'Settings' }),
    ).toBeInTheDocument();

    await user.keyboard('{Escape}');

    await waitFor(() =>
      expect(screen.queryByTestId('shortcuts-modal')).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    expect(
      screen.getByRole('dialog', { name: 'Settings' }),
    ).toBeInTheDocument();
  });
});
