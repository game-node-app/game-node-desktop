import { describe, expect, it, beforeEach, vi } from 'vitest';
import { render, screen, fireEvent, act, within } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { I18nProvider } from '../../i18n';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  SNAPSHOT_QUERY_KEY,
  APP_INFO_QUERY_KEY,
  LICENSE_NOTICES_QUERY_KEY,
  createDefaultSnapshot,
  commands,
  type Snapshot,
  type AppInfo,
} from '../../api';
import {
  uiStore,
  resetUiStore,
  openSettingsModal,
  closeSettingsModal,
} from '../../store/uiStore';
import {
  SettingsModal,
  ToastContainer,
  useSettingsKeyboardShortcut,
} from './index';

function ShortcutTestHarness() {
  useSettingsKeyboardShortcut();
  return <SettingsModal />;
}

function createTestQueryClient(
  snapshot?: Partial<Snapshot>,
  appInfo?: Partial<AppInfo>,
  licenseNotices = 'GameNode Desktop - Full offline license notices text',
) {
  const qc = createAppQueryClient();

  const fullSnapshot: Snapshot = {
    ...createDefaultSnapshot(),
    ...snapshot,
    localGames: {
      ...createDefaultSnapshot().localGames,
      ...(snapshot?.localGames ?? {}),
      settings: {
        ...createDefaultSnapshot().localGames.settings,
        ...(snapshot?.localGames?.settings ?? {}),
      },
    },
    tools: {
      ...createDefaultSnapshot().tools,
      ...(snapshot?.tools ?? {}),
      catalog: snapshot?.tools?.catalog ?? [
        {
          id: 'optiscaler',
          name: 'OptiScaler',
          summary: 'Upscaling backend',
          license: 'MIT',
          sourceUrl: 'https://github.com/optiscaler/optiscaler',
        },
      ],
    },
  };

  const fullAppInfo: AppInfo = {
    locale: 'en',
    version: '0.2.0',
    backgroundSupported: false,
    ...appInfo,
  };

  qc.setQueryData(SNAPSHOT_QUERY_KEY, fullSnapshot);
  qc.setQueryData(APP_INFO_QUERY_KEY, fullAppInfo);
  qc.setQueryData(LICENSE_NOTICES_QUERY_KEY, licenseNotices);

  return qc;
}

function renderSettings(
  locale = 'en',
  snapshot?: Partial<Snapshot>,
  appInfo?: Partial<AppInfo>,
  licenseNotices?: string,
) {
  const queryClient = createTestQueryClient(snapshot, appInfo, licenseNotices);

  const utils = render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>
          <ToastContainer />
          <ShortcutTestHarness />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );

  return { ...utils, queryClient };
}

describe('SettingsModal Feature Suite', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  describe('Topic Navigation and Content', () => {
    it('renders the 6 topics in exact order and confirms Advanced/Avançado is absent (en)', () => {
      openSettingsModal();
      renderSettings('en');

      const navButtons = screen.getAllByRole('button', {
        name: /Account|Stores|Backups|Tools|Plugins|About/i,
      });

      const topicTexts = navButtons.map((btn) => btn.textContent?.trim());
      expect(topicTexts).toEqual(['Account', 'Stores', 'Backups', 'Tools', 'Plugins', 'About']);

      // Verify removed Advanced topic is NOT present anywhere
      expect(screen.queryByText(/Advanced/i)).toBeNull();
      expect(screen.queryByText(/Avançado/i)).toBeNull();
    });

    it('renders the 6 topics in exact order and confirms Advanced/Avançado is absent (pt-BR)', () => {
      openSettingsModal();
      renderSettings('pt-BR');

      const navButtons = screen.getAllByRole('button', {
        name: /Conta|Lojas|Backups|Ferramentas|Sobre|Plugins/i,
      });

      const topicTexts = navButtons.map((btn) => btn.textContent?.trim());
      expect(topicTexts).toEqual(['Conta', 'Lojas', 'Backups', 'Ferramentas', 'Plugins', 'Sobre']);

      expect(screen.queryByText(/Advanced/i)).toBeNull();
      expect(screen.queryByText(/Avançado/i)).toBeNull();
    });

    it('switches topics and resets scroll top position', () => {
      openSettingsModal();
      renderSettings('en');

      const scrollContainer = document.querySelector('.settings-scroll-content');
      expect(scrollContainer).not.toBeNull();

      if (scrollContainer) {
        scrollContainer.scrollTop = 150;
      }

      // Default topic is account
      expect(screen.getByRole('heading', { level: 2, name: 'Account' })).toBeInTheDocument();

      // Switch to Stores
      const storesBtn = screen.getByRole('button', { name: 'Stores' });
      fireEvent.click(storesBtn);

      expect(screen.getByRole('heading', { level: 2, name: 'Stores' })).toBeInTheDocument();
      expect(scrollContainer?.scrollTop).toBe(0);

      // Switch to Backups
      const backupsBtn = screen.getByRole('button', { name: 'Backups' });
      fireEvent.click(backupsBtn);
      expect(screen.getByRole('heading', { level: 2, name: 'Backups' })).toBeInTheDocument();

      // Switch to Tools
      const toolsBtn = screen.getByRole('button', { name: 'Tools' });
      fireEvent.click(toolsBtn);
      expect(screen.getByRole('heading', { level: 2, name: 'Tools' })).toBeInTheDocument();

      // Switch to About
      const aboutBtn = screen.getByRole('button', { name: 'About' });
      fireEvent.click(aboutBtn);
      expect(screen.getByRole('heading', { level: 2, name: 'About' })).toBeInTheDocument();
    });

    it('persists selected topic across closing and reopening in the same session', () => {
      openSettingsModal();
      renderSettings('en');

      // Select 'tools'
      fireEvent.click(screen.getByRole('button', { name: 'Tools' }));
      expect(uiStore.state.settingsTopic).toBe('tools');

      // Close modal
      act(() => {
        closeSettingsModal();
      });
      expect(screen.queryByRole('dialog', { name: /Settings/i })).toBeNull();
      expect(uiStore.state.settingsTopic).toBe('tools');

      // Reopen modal without passing topic
      act(() => {
        openSettingsModal();
      });

      // Still on tools topic!
      expect(screen.getByRole('heading', { level: 2, name: 'Tools' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Tools' })).toHaveClass('active');
    });
  });

  describe('Dirty-State Discard Confirmation & Esc Handling', () => {
    it('prompts discard confirmation when switching topics with dirty backup destination', () => {
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            destination: '/original/backups',
          },
        },
      });

      const destInput = screen.getByLabelText(/Backup destination/i);
      expect(destInput).toHaveValue('/original/backups');

      // Type dirty changes
      fireEvent.change(destInput, { target: { value: '/modified/backups' } });
      expect(destInput).toHaveValue('/modified/backups');
      expect(uiStore.state.backupDestinationDraft).toBe('/modified/backups');

      // Try switching topic to 'account'
      fireEvent.click(screen.getByRole('button', { name: 'Account' }));

      // Discard confirmation dialog must be shown
      const alertdialog = screen.getByRole('alertdialog');
      expect(alertdialog).toBeInTheDocument();
      expect(within(alertdialog).getByText('Discard unsaved changes?')).toBeInTheDocument();
      expect(uiStore.state.settingsTopic).toBe('backups');

      // Click Keep editing / Cancel
      fireEvent.click(within(alertdialog).getByRole('button', { name: /Keep editing|Cancel/i }));

      // Dialog closed, still on backups and input still dirty
      expect(screen.queryByRole('alertdialog')).toBeNull();
      expect(screen.getByLabelText(/Backup destination/i)).toHaveValue('/modified/backups');
      expect(uiStore.state.settingsTopic).toBe('backups');

      // Try switching again to 'account'
      fireEvent.click(screen.getByRole('button', { name: 'Account' }));
      const alertdialog2 = screen.getByRole('alertdialog');
      expect(alertdialog2).toBeInTheDocument();

      // Click Discard
      fireEvent.click(within(alertdialog2).getByRole('button', { name: 'Discard' }));

      // Confirmation dialog closes, topic switches to Account, draft reverted
      expect(screen.queryByRole('alertdialog')).toBeNull();
      expect(uiStore.state.settingsTopic).toBe('account');
      expect(uiStore.state.backupDestinationDraft).toBeNull();
    });

    it('prompts discard confirmation when pressing Escape with dirty backup destination', () => {
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            destination: '/saved/path',
          },
        },
      });

      const destInput = screen.getByLabelText(/Backup destination/i);
      fireEvent.change(destInput, { target: { value: '/unsaved/path' } });

      // Press Escape
      fireEvent.keyDown(window, { key: 'Escape' });

      // Discard dialog should open, SettingsModal should NOT close yet
      const alertdialog = screen.getByRole('alertdialog');
      expect(alertdialog).toBeInTheDocument();
      expect(screen.getByRole('dialog', { name: /Settings/i })).toBeInTheDocument();

      // Press Escape again inside discard dialog -> cancels discard dialog
      fireEvent.keyDown(window, { key: 'Escape' });
      expect(screen.queryByRole('alertdialog')).toBeNull();
      expect(screen.getByRole('dialog', { name: /Settings/i })).toBeInTheDocument();

      // Trigger discard confirmation again and confirm discard
      fireEvent.keyDown(window, { key: 'Escape' });
      const alertdialog2 = screen.getByRole('alertdialog');
      expect(alertdialog2).toBeInTheDocument();

      fireEvent.click(within(alertdialog2).getByRole('button', { name: 'Discard' }));

      // Now SettingsModal closes
      expect(screen.queryByRole('dialog', { name: /Settings/i })).toBeNull();
      expect(uiStore.state.settingsOpen).toBe(false);
    });

    it('prompts discard confirmation in pt-BR with localized strings', () => {
      openSettingsModal('backups');
      renderSettings('pt-BR', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            destination: '/meus/backups',
          },
        },
      });

      const destInput = screen.getByLabelText(/Destino dos backups|Backup destination/i);
      fireEvent.change(destInput, { target: { value: '/novo/destino' } });

      fireEvent.click(screen.getByRole('button', { name: 'Conta' }));

      const alertdialog = screen.getByRole('alertdialog');
      expect(alertdialog).toBeInTheDocument();
      expect(within(alertdialog).getByText('Descartar alterações não salvas?')).toBeInTheDocument();
      expect(within(alertdialog).getByRole('button', { name: 'Descartar' })).toBeInTheDocument();
      expect(within(alertdialog).getByRole('button', { name: /Continuar editando|Cancelar/i })).toBeInTheDocument();
    });
  });

  describe('Keyboard Shortcuts and License Notices Modal', () => {
    it('opens SettingsModal on Ctrl+, or Meta+,', () => {
      renderSettings('en');
      expect(screen.queryByRole('dialog', { name: /Settings/i })).toBeNull();

      // Trigger Ctrl+,
      fireEvent.keyDown(window, { ctrlKey: true, key: ',' });
      expect(screen.getByRole('dialog', { name: /Settings/i })).toBeInTheDocument();
      expect(uiStore.state.settingsOpen).toBe(true);

      // Close modal
      act(() => {
        closeSettingsModal();
      });
      expect(screen.queryByRole('dialog', { name: /Settings/i })).toBeNull();

      // Trigger Meta+,
      fireEvent.keyDown(window, { metaKey: true, key: ',' });
      expect(screen.getByRole('dialog', { name: /Settings/i })).toBeInTheDocument();
    });

    it('opens License Notices modal, and Escape closes it first while restoring focus to Show license notices', () => {
      openSettingsModal('about');
      renderSettings(
        'en',
        undefined,
        undefined,
        'Copyright 2026 GameNode Desktop Full License Text',
      );

      const showNoticesBtn = screen.getByRole('button', { name: /Show license notices/i });
      showNoticesBtn.focus();
      expect(document.activeElement).toBe(showNoticesBtn);

      // Click Show license notices
      fireEvent.click(showNoticesBtn);

      // License notices dialog is open
      const licenseDialog = screen.getByRole('dialog', { name: /License notices/i });
      expect(licenseDialog).toBeInTheDocument();
      expect(screen.getByText(/Copyright 2026 GameNode Desktop Full License Text/i)).toBeInTheDocument();

      // Press Escape: License notices modal closes, focus is restored to the button, SettingsModal remains open!
      fireEvent.keyDown(window, { key: 'Escape' });

      expect(screen.queryByRole('dialog', { name: /License notices/i })).toBeNull();
      expect(screen.getByRole('dialog', { name: /Settings/i })).toBeInTheDocument();
      expect(document.activeElement).toBe(showNoticesBtn);

      // Press Escape again: now SettingsModal closes
      fireEvent.keyDown(window, { key: 'Escape' });
      expect(screen.queryByRole('dialog', { name: /Settings/i })).toBeNull();
    });
  });

  describe('Panel Operations and Toasts', () => {
    it('dispatches sync toggle and shows toast notification', () => {
      const setSyncSpy = vi.spyOn(commands, 'setSyncEnabled');
      openSettingsModal('account');
      renderSettings('en', {
        sync: { enabled: true, explained: true },
      });

      const syncToggle = screen.getByLabelText(/Sync with GameNode/i);
      expect(syncToggle).toBeChecked();

      fireEvent.click(syncToggle);
      expect(setSyncSpy).toHaveBeenCalledWith(false);

      // Toast appears in ToastContainer
      const notifications = screen.getByRole('region', { name: 'Notifications' });
      expect(notifications).toBeInTheDocument();
      expect(within(notifications).getByText('Sync with GameNode')).toBeInTheDocument();
    });

    it('dispatches playtime source change and shows toast notification', () => {
      const setPlaytimeSpy = vi.spyOn(commands, 'setPlaytimeSource');
      openSettingsModal('account');
      renderSettings('en', {
        playtimeSource: 'store',
      });

      const internalBtn = screen.getByRole('radio', { name: 'Internal' });
      fireEvent.click(internalBtn);

      expect(setPlaytimeSpy).toHaveBeenCalledWith('internal');
      const notifications = screen.getByRole('region', { name: 'Notifications' });
      expect(within(notifications).getByText('Playtime source')).toBeInTheDocument();
    });

    it('shows signed-in identity and sign out action', () => {
      const signOutSpy = vi.spyOn(commands, 'dispatchAuth');
      openSettingsModal('account');
      renderSettings('en', {
        auth: {
          signedIn: {
            user: {
              id: 'user-77',
              username: 'AlexRider',
              email: 'alex@example.com',
            },
          },
        },
        storage: 'secretService',
      });

      expect(screen.getAllByText('AlexRider').length).toBeGreaterThanOrEqual(1);
      expect(screen.getByText(/Secret Service/i)).toBeInTheDocument();

      const signOutBtn = screen.getByRole('button', { name: 'Sign out' });
      fireEvent.click(signOutBtn);
      expect(signOutSpy).toHaveBeenCalledWith('signOut');
    });

    it('clears tool cache and shows toast notification', () => {
      const clearSpy = vi.spyOn(commands, 'clearToolCache');
      openSettingsModal('tools');
      renderSettings('en', {
        tools: {
          installedByEntry: {},
          totalCacheBytes: 10485760,
          totalCacheArchives: 3,
          catalog: [
            {
              id: 'optiscaler',
              name: 'OptiScaler',
              summary: 'Upscaling backend',
              license: 'MIT',
              sourceUrl: 'https://github.com/optiscaler/optiscaler',
            },
          ],
        },
      });

      expect(screen.getByText(/3 archives cached/i)).toBeInTheDocument();
      expect(screen.getByText('OptiScaler')).toBeInTheDocument();

      const clearBtn = screen.getByRole('button', { name: 'Clear cache' });
      fireEvent.click(clearBtn);

      expect(clearSpy).toHaveBeenCalled();
      expect(screen.getByText('Tool cache cleared')).toBeInTheDocument();
    });

    it('saves backup destination and shows toast notification', () => {
      const backupSpy = vi.spyOn(commands, 'setLocalBackupSettings');
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            destination: '/default/destination',
          },
        },
      });

      const destInput = screen.getByLabelText(/Backup destination/i);
      fireEvent.change(destInput, { target: { value: '/new/destination' } });

      const saveBtn = screen.getByRole('button', { name: 'Save changes' });
      expect(saveBtn).not.toBeDisabled();

      fireEvent.click(saveBtn);
      expect(backupSpy).toHaveBeenCalledWith(true, '/new/destination');
      expect(uiStore.state.backupDestinationDraft).toBeNull();
      expect(screen.getByText('Backup destination saved')).toBeInTheDocument();
    });

    it('resets hidden games in stores panel and shows toast', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      openSettingsModal('stores');
      renderSettings('en', {
        library: {
          generation: 1,
          layout: {
            view: 'grid',
            sort: { key: 'title', direction: 'ascending' },
            filter: 'all',
            sidebarWidth: 200,
            detailWidth: 300,
            detailOpen: false,
          },
          search: '',
          access: 'verified',
          total: 10,
          providers: [{ id: 'steam', name: 'Steam', count: 5 }],
          rows: [],
          scanning: false,
          failedProviders: [],
          selected: [],
          actions: {
            selected: 0,
            play: 'available',
            install: 'available',
            matchGame: 'available',
            addToGamenode: 'available',
            changeMatch: false,
          },
        },
      });

      expect(screen.getByText('Steam')).toBeInTheDocument();
      expect(screen.getByText('5 installed games')).toBeInTheDocument();

      const resetBtn = screen.getByRole('button', { name: /Reset hidden games/i });
      fireEvent.click(resetBtn);

      expect(libraryInputSpy).toHaveBeenCalledWith('resetExcluded');
      expect(screen.getByRole('status')).toHaveTextContent(/Reset hidden games/i);
    });
  });

  describe('DRM-free watched folders', () => {
    const watchedSnapshot: Partial<Snapshot> = {
      localGames: {
        ...createDefaultSnapshot().localGames,
        watches: [{ id: 'w-1', path: 'C:\\Games\\Indie', enabled: true }],
      },
    };

    it('shows the watched-folder controls on the Stores page', () => {
      openSettingsModal('stores');
      renderSettings('en', watchedSnapshot);

      expect(screen.getByPlaceholderText('Folder to watch')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Watch folder' })).toBeInTheDocument();
      expect(screen.getByText('C:\\Games\\Indie')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Rescan watched folders' })).toBeInTheDocument();
    });

    it('keeps the watched-folder controls off the Backups page', () => {
      openSettingsModal('backups');
      renderSettings('en', watchedSnapshot);

      expect(screen.queryByPlaceholderText('Folder to watch')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Watch folder' })).not.toBeInTheDocument();
    });

    it('adds a typed folder from the Stores page', () => {
      const watchSpy = vi.spyOn(commands, 'watchLocalFolder');
      openSettingsModal('stores');
      renderSettings('en');

      fireEvent.change(screen.getByPlaceholderText('Folder to watch'), {
        target: { value: '  D:\\Roms  ' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Watch folder' }));

      expect(watchSpy).toHaveBeenCalledWith('D:\\Roms');
    });

    it('stops monitoring a folder without removing its registered games', () => {
      const removeSpy = vi.spyOn(commands, 'removeLocalWatch');
      openSettingsModal('stores');
      renderSettings('en', watchedSnapshot);

      fireEvent.click(screen.getByRole('button', { name: 'Remove watch' }));

      expect(removeSpy).toHaveBeenCalledWith('w-1');
    });

    it('does not list disabled watches', () => {
      openSettingsModal('stores');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          watches: [{ id: 'w-1', path: 'C:\\Games\\Indie', enabled: false }],
        },
      });

      expect(screen.queryByText('C:\\Games\\Indie')).not.toBeInTheDocument();
      expect(screen.getByText('No watched folders configured.')).toBeInTheDocument();
    });

    it('uses the Portuguese label on the Stores page', () => {
      openSettingsModal('stores');
      renderSettings('pt-BR');

      expect(screen.getByRole('button', { name: 'Monitorar pasta' })).toBeInTheDocument();
    });
  });
});
