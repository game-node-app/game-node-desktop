import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest';
import { render, screen, fireEvent, act, within, waitFor } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { I18nProvider } from '../../i18n';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  SNAPSHOT_QUERY_KEY,
  APP_INFO_QUERY_KEY,
  LICENSE_NOTICES_QUERY_KEY,
  createDefaultLibraryView,
  createDefaultSnapshot,
  commands,
  type Snapshot,
  type AppInfo,
  createDefaultFilters,
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
} from './index';
import { useGlobalShortcuts } from '../shell/useGlobalShortcuts';

function ShortcutTestHarness() {
  useGlobalShortcuts();
  return (
    <div>
      <button type="button">Library settings trigger</button>
      <input data-library-search aria-label="Library search" />
      <SettingsModal />
    </div>
  );
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
    buildDate: '2026-11-16',
    osVersion: 'Test OS 1.0',
    arch: 'x86_64',
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
    it('renders the eight topics in specification order with their groups (en)', () => {
      openSettingsModal();
      renderSettings('en');

      const nav = screen.getByRole('navigation', { name: 'Settings' });
      const topicButtons = within(nav).getAllByRole('button');
      expect(topicButtons.map((btn) => btn.textContent?.trim())).toEqual([
        'Account & sync',
        'Stores',
        'DRM-free games',
        'Backups',
        'General',
        'Tools',
        'Plugins',
        'About',
      ]);
      expect(within(nav).getByText('Account')).toBeInTheDocument();
      expect(within(nav).getByText('Library')).toBeInTheDocument();
      expect(within(nav).getByText('App')).toBeInTheDocument();

      // Verify removed Advanced topic is NOT present anywhere
      expect(screen.queryByText(/Advanced/i)).toBeNull();
      expect(screen.queryByText(/Avançado/i)).toBeNull();
    });

    it('renders the eight topics in specification order with their groups (pt-BR)', () => {
      openSettingsModal();
      renderSettings('pt-BR');

      const nav = screen.getByRole('navigation', { name: 'Configurações' });
      const topicButtons = within(nav).getAllByRole('button');
      expect(topicButtons.map((btn) => btn.textContent?.trim())).toEqual([
        'Conta e sincronização',
        'Lojas',
        'Jogos DRM-free',
        'Backups',
        'Geral',
        'Ferramentas',
        'Plugins',
        'Sobre',
      ]);
      expect(within(nav).getByText('Conta')).toBeInTheDocument();
      expect(within(nav).getByText('Biblioteca')).toBeInTheDocument();
      expect(within(nav).getByText('Aplicativo')).toBeInTheDocument();

      expect(screen.queryByText(/Advanced/i)).toBeNull();
      expect(screen.queryByText(/Avançado/i)).toBeNull();
    });

    it('applies the shell size and token classes from the design reference', () => {
      openSettingsModal();
      renderSettings('en');

      const dialog = screen.getByRole('dialog', { name: 'Settings' });
      expect(dialog).toHaveClass(
        'rounded-dialog',
        'border-border-overlay',
        '!bg-canvas',
        '!shadow-alert',
      );

      const titleBar = document.querySelector('[data-floating-modal-handle]');
      expect(titleBar).toHaveClass('!h-[48px]', 'bg-chrome');

      const nav = screen.getByRole('navigation', { name: 'Settings' });
      expect(nav).toHaveClass('w-settings-nav', 'gap-md', 'p-sm', 'bg-chrome');

      const scroller = document.querySelector('.settings-scroll-content');
      expect(scroller).toHaveClass(
        'px-xl',
        'pt-[28px]',
        'pb-xl',
        'gap-lg',
        'overflow-y-auto',
      );
    });

    it('shows the title bar, footer version and decorative Esc hint', () => {
      openSettingsModal();
      renderSettings('en');

      expect(screen.getByRole('dialog', { name: 'Settings' })).toBeInTheDocument();
      expect(screen.getByText('GameNode Desktop 0.2.0')).toBeInTheDocument();
      expect(screen.getByText('Esc')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Esc' })).toBeNull();
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
      expect(screen.getByRole('heading', { level: 2, name: 'Account & sync' })).toBeInTheDocument();

      // Switch to Stores
      const storesBtn = screen.getByRole('button', { name: 'Stores' });
      fireEvent.click(storesBtn);
      expect(screen.getByRole('heading', { level: 2, name: 'Stores' })).toBeInTheDocument();
      expect(scrollContainer?.scrollTop).toBe(0);

      fireEvent.click(screen.getByRole('button', { name: 'DRM-free games' }));
      expect(screen.getByRole('heading', { level: 2, name: 'DRM-free games' })).toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Plugins' }));
      expect(screen.getByRole('heading', { level: 2, name: 'Plugins' })).toBeInTheDocument();

      // Switch to Backups
      const backupsBtn = screen.getByRole('button', { name: 'Backups' });
      fireEvent.click(backupsBtn);
      expect(screen.getByRole('heading', { level: 2, name: 'Backups' })).toBeInTheDocument();

      // Switch to General
      fireEvent.click(screen.getByRole('button', { name: 'General' }));
      expect(screen.getByRole('heading', { level: 2, name: 'General' })).toBeInTheDocument();

      // Switch to Tools
      const toolsBtn = screen.getByRole('button', { name: 'Tools' });
      fireEvent.click(toolsBtn);
      expect(screen.getByRole('heading', { level: 2, name: 'Tools' })).toBeInTheDocument();

      // Switch to About
      const aboutBtn = screen.getByRole('button', { name: 'About' });
      fireEvent.click(aboutBtn);
      expect(screen.getByRole('heading', { level: 2, name: 'About' })).toBeInTheDocument();
    });

    it('keeps settings topic navigation responsive while Configure awaits the external window', async () => {
      vi.spyOn(commands, 'pluginInventory').mockResolvedValue([{
        providerId: 'fixture', pluginId: 'org.example.fixture', pluginKind: 'extension', name: 'Fixture', source: 'community',
        status: 'installed', capabilities: ['ui.configure'], session: 'active', nativeAccess: [],
      }]);
      vi.spyOn(commands, 'pluginConfigurationModes').mockResolvedValue({
        externalDeclared: true, externalAuthorized: true,
        declarativeDeclared: false, declarativeAuthorized: false,
      });
      let finishConfigure!: (result: { status: 'open'; windowId: string; message: null }) => void;
      vi.spyOn(commands, 'pluginConfigurationOpen').mockReturnValue(new Promise((resolve) => { finishConfigure = resolve; }));

      openSettingsModal('plugins');
      renderSettings('en');
      fireEvent.click(await screen.findByRole('button', { name: 'Details' }));
      fireEvent.click(await screen.findByRole('button', { name: 'Configure' }));
      fireEvent.click(screen.getByRole('button', { name: 'Stores' }));

      expect(screen.getByRole('heading', { level: 2, name: 'Stores' })).toBeInTheDocument();
      await act(async () => { finishConfigure({ status: 'open', windowId: 'window-1', message: null }); });
      expect(uiStore.state.settingsTopic).toBe('stores');
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
      expect(screen.getByRole('button', { name: 'Tools' })).toHaveAttribute(
        'aria-current',
        'page',
      );
    });

    it('closes from the title-bar button, Escape and the overlay', () => {
      openSettingsModal();
      renderSettings('en');

      fireEvent.click(screen.getByRole('button', { name: 'Close' }));
      expect(screen.queryByRole('dialog', { name: /Settings/i })).toBeNull();

      act(() => {
        openSettingsModal();
      });
      fireEvent.keyDown(window, { key: 'Escape' });
      expect(screen.queryByRole('dialog', { name: /Settings/i })).toBeNull();

      act(() => {
        openSettingsModal();
      });
      const overlay = document.querySelector('[data-floating-modal-overlay]');
      expect(overlay).not.toBeNull();
      fireEvent.click(overlay as Element);
      expect(screen.queryByRole('dialog', { name: /Settings/i })).toBeNull();
    });

    it('returns focus to the control that opened the modal', () => {
      renderSettings('en');
      const trigger = screen.getByRole('button', { name: 'Library settings trigger' });
      trigger.focus();

      act(() => {
        openSettingsModal();
      });
      expect(screen.getByRole('dialog', { name: 'Settings' })).toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Close' }));
      expect(trigger).toHaveFocus();
    });

    it('moves the selection with the arrow keys, skipping group labels', () => {
      openSettingsModal('account');
      renderSettings('en');

      const account = screen.getByRole('button', { name: 'Account & sync' });
      account.focus();

      fireEvent.keyDown(account, { key: 'ArrowDown' });
      const stores = screen.getByRole('button', { name: 'Stores' });
      expect(stores).toHaveFocus();

      fireEvent.keyDown(stores, { key: 'ArrowDown' });
      const drmFree = screen.getByRole('button', { name: 'DRM-free games' });
      expect(uiStore.state.settingsTopic).toBe('drmFree');
      expect(drmFree).toHaveFocus();
      expect(
        screen.getByRole('heading', { level: 2, name: 'DRM-free games' }),
      ).toBeInTheDocument();
    });
  });

  describe('Settings search', () => {
    const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;

    afterEach(() => {
      if (originalScrollIntoView) {
        HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
      } else {
        Reflect.deleteProperty(HTMLElement.prototype, 'scrollIntoView');
      }
    });

    it('finds Geral by a tray row label, selects it and highlights matching rows', () => {
      const scrollIntoView = vi.fn();
      HTMLElement.prototype.scrollIntoView = scrollIntoView;

      openSettingsModal('account');
      renderSettings('pt-BR', undefined, {
        platform: 'windows',
        backgroundSupported: true,
      });

      const input = screen.getByRole('searchbox', {
        name: 'Buscar configurações',
      });
      fireEvent.change(input, { target: { value: 'bandeja' } });

      const nav = screen.getByRole('navigation', { name: 'Configurações' });
      expect(
        within(nav).getAllByRole('button').map((button) => button.textContent?.trim()),
      ).toEqual(['Geral']);
      expect(uiStore.state.settingsTopic).toBe('general');
      expect(screen.getByRole('heading', { level: 2, name: 'Geral' })).toBeInTheDocument();

      const closeRow = screen.getByText(
        'Fechar a janela minimiza para a bandeja',
      );
      expect(closeRow).toHaveClass('decoration-brand');
      expect(closeRow.closest('[data-search-match="true"]')).not.toBeNull();

      const startRow = screen.getByText('Iniciar minimizado na bandeja');
      expect(startRow.closest('[data-search-match="true"]')).not.toBeNull();
      expect(scrollIntoView).toHaveBeenCalled();
    });

    it('shows the empty state and keeps the current panel when nothing matches', () => {
      openSettingsModal('general');
      renderSettings('pt-BR');

      fireEvent.change(
        screen.getByRole('searchbox', { name: 'Buscar configurações' }),
        { target: { value: 'zzzz-nao-existe' } },
      );

      expect(
        screen.getByText('Nenhuma configuração encontrada'),
      ).toBeInTheDocument();
      expect(uiStore.state.settingsTopic).toBe('general');
      expect(screen.getByRole('heading', { level: 2, name: 'Geral' })).toBeInTheDocument();
    });

    it('restores every topic when the search is cleared and keeps the selection', () => {
      openSettingsModal('account');
      renderSettings('en', undefined, { platform: 'windows' });

      const input = screen.getByRole('searchbox', { name: 'Search settings' });
      fireEvent.change(input, { target: { value: 'tray' } });
      expect(uiStore.state.settingsTopic).toBe('general');

      fireEvent.change(input, { target: { value: '' } });

      const nav = screen.getByRole('navigation', { name: 'Settings' });
      expect(within(nav).getAllByRole('button')).toHaveLength(8);
      expect(uiStore.state.settingsTopic).toBe('general');
      expect(screen.getByRole('button', { name: 'General' })).toHaveAttribute(
        'aria-current',
        'page',
      );
    });

    it('clears the search after an inner dialog and before closing the modal', () => {
      openSettingsModal('about');
      renderSettings(
        'pt-BR',
        undefined,
        undefined,
        'GameNode Desktop - avisos de licença completos',
      );

      const input = screen.getByRole('searchbox', {
        name: 'Buscar configurações',
      });
      fireEvent.change(input, { target: { value: 'licença' } });
      // About itself matches, so the current panel stays.
      expect(uiStore.state.settingsTopic).toBe('about');

      fireEvent.click(
        screen.getByRole('button', { name: 'Mostrar avisos de licença' }),
      );
      expect(
        screen.getByRole('dialog', { name: 'Avisos de licença' }),
      ).toBeInTheDocument();

      // Escape closes the innermost dialog first.
      fireEvent.keyDown(window, { key: 'Escape' });
      expect(screen.queryByRole('dialog', { name: 'Avisos de licença' })).toBeNull();
      expect(screen.getByRole('dialog', { name: 'Configurações' })).toBeInTheDocument();
      expect(input).toHaveValue('licença');

      // Then the focused search clears.
      input.focus();
      fireEvent.keyDown(input, { key: 'Escape' });
      expect(input).toHaveValue('');
      expect(screen.getByRole('dialog', { name: 'Configurações' })).toBeInTheDocument();

      // Only the next Escape closes the modal.
      fireEvent.keyDown(input, { key: 'Escape' });
      expect(screen.queryByRole('dialog', { name: 'Configurações' })).toBeNull();
    });

    it('focuses the settings search with Ctrl+F without triggering the library search', () => {
      openSettingsModal('account');
      renderSettings('en');

      fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

      expect(
        screen.getByRole('searchbox', { name: 'Search settings' }),
      ).toHaveFocus();
      expect(screen.getByLabelText('Library search')).not.toHaveFocus();
    });
  });

  describe('Dirty-State Discard Confirmation & Esc Handling', () => {
    it('lists the pending fields, focuses Keep editing, and Discard runs the pending topic change', async () => {
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/original/backups',
          },
        },
      });

      const destInput = screen.getByLabelText(/Backup destination/i);
      expect(destInput).toHaveValue('/original/backups');

      // Type dirty changes
      fireEvent.change(destInput, { target: { value: '/modified/backups' } });
      expect(destInput).toHaveValue('/modified/backups');
      expect(uiStore.state.settingsDraft).toEqual({
        topic: 'backups',
        values: { destination: '/modified/backups' },
      });
      expect(screen.getByTestId('settings-unsaved-count')).toHaveTextContent(
        '1 setting changed',
      );
      expect(screen.getByText('Before: /original/backups')).toBeInTheDocument();

      // Try switching topic to 'account'
      fireEvent.click(screen.getByRole('button', { name: 'Account & sync' }));

      // Discard confirmation dialog must be shown
      const discardDialog = screen.getByRole('dialog', { name: 'Discard changes?' });
      expect(discardDialog).toBeInTheDocument();
      expect(within(discardDialog).getByText('Backup destination')).toBeInTheDocument();
      await waitFor(() => expect(screen.getByTestId('discard-keep-editing')).toHaveFocus());
      expect(uiStore.state.settingsTopic).toBe('backups');

      // Keep editing: dialog closes, still on backups and input still dirty
      fireEvent.click(within(discardDialog).getByTestId('discard-keep-editing'));

      expect(screen.queryByTestId('discard-changes-dialog')).toBeNull();
      expect(screen.getByLabelText(/Backup destination/i)).toHaveValue('/modified/backups');
      expect(uiStore.state.settingsTopic).toBe('backups');

      // Try switching again to 'account' and discard
      fireEvent.click(screen.getByRole('button', { name: 'Account & sync' }));
      const discardDialog2 = screen.getByRole('dialog', { name: 'Discard changes?' });
      fireEvent.click(within(discardDialog2).getByTestId('discard-confirm'));

      // Confirmation dialog closes, topic switches to Account, draft reverted
      expect(screen.queryByTestId('discard-changes-dialog')).toBeNull();
      expect(uiStore.state.settingsTopic).toBe('account');
      expect(uiStore.state.settingsDraft).toBeNull();
    });

    it('prompts discard confirmation when pressing Escape with dirty backup settings', () => {
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/saved/path',
          },
        },
      });

      const destInput = screen.getByLabelText(/Backup destination/i);
      fireEvent.change(destInput, { target: { value: '/unsaved/path' } });

      // Press Escape
      fireEvent.keyDown(window, { key: 'Escape' });

      // Discard dialog should open, SettingsModal should NOT close yet
      expect(screen.getByTestId('discard-changes-dialog')).toBeInTheDocument();
      expect(screen.getByRole('dialog', { name: /Settings/i })).toBeInTheDocument();

      // Press Escape again inside discard dialog -> cancels discard dialog
      fireEvent.keyDown(window, { key: 'Escape' });
      expect(screen.queryByTestId('discard-changes-dialog')).toBeNull();
      expect(screen.getByRole('dialog', { name: /Settings/i })).toBeInTheDocument();

      // Trigger discard confirmation again and confirm discard
      fireEvent.keyDown(window, { key: 'Escape' });
      const discardDialog = screen.getByTestId('discard-changes-dialog');
      expect(discardDialog).toBeInTheDocument();

      fireEvent.click(within(discardDialog).getByTestId('discard-confirm'));

      // Now SettingsModal closes
      expect(screen.queryByRole('dialog', { name: /Settings/i })).toBeNull();
      expect(uiStore.state.settingsOpen).toBe(false);
    });

    it('prompts discard confirmation in pt-BR with localized strings', async () => {
      openSettingsModal('backups');
      renderSettings('pt-BR', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/meus/backups',
          },
        },
      });

      const destInput = screen.getByLabelText(/Destino dos backups|Backup destination/i);
      fireEvent.change(destInput, { target: { value: '/novo/destino' } });

      fireEvent.click(screen.getByRole('button', { name: 'Conta e sincronização' }));

      const discardDialog = screen.getByRole('dialog', { name: 'Descartar alterações?' });
      expect(discardDialog).toBeInTheDocument();
      expect(within(discardDialog).getByText('Destino dos backups')).toBeInTheDocument();
      expect(within(discardDialog).getByTestId('discard-confirm')).toHaveTextContent('Descartar');
      expect(within(discardDialog).getByTestId('discard-keep-editing')).toHaveTextContent(
        'Continuar editando',
      );
      await waitFor(() => expect(screen.getByTestId('discard-keep-editing')).toHaveFocus());
    });

    it('discards pending settings before closing the modal', () => {
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/saved/backups',
          },
        },
      });

      fireEvent.change(screen.getByLabelText(/Backup destination/i), {
        target: { value: '/unsaved/backups' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Close' }));

      expect(screen.getByTestId('discard-changes-dialog')).toBeInTheDocument();
      expect(uiStore.state.settingsOpen).toBe(true);

      fireEvent.click(screen.getByTestId('discard-confirm'));
      expect(uiStore.state.settingsOpen).toBe(false);
      expect(uiStore.state.settingsDraft).toBeNull();
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
    it('dispatches sync toggle and shows toast notification', async () => {
      const setSyncSpy = vi.spyOn(commands, 'setSyncEnabled');
      openSettingsModal('account');
      renderSettings('en', {
        sync: { enabled: true, explained: true, summary: null, running: false },
      });

      const syncToggle = screen.getByLabelText(/Sync with GameNode/i);
      expect(syncToggle).toBeChecked();

      fireEvent.click(syncToggle);
      expect(setSyncSpy).toHaveBeenCalledWith(false);

      // Toast appears in ToastContainer
      const notifications = await screen.findByRole('region', {
        name: 'Notifications',
      });
      expect(notifications).toBeInTheDocument();
      expect(within(notifications).getByText('Sync with GameNode')).toBeInTheDocument();
    });

    it('dispatches playtime source change and shows toast notification', async () => {
      const setPlaytimeSpy = vi.spyOn(commands, 'setPlaytimeSource');
      openSettingsModal('account');
      renderSettings('en', {
        playtimeSource: 'store',
      });

      const internalBtn = screen.getByRole('radio', { name: 'Internal' });
      fireEvent.click(internalBtn);

      expect(setPlaytimeSpy).toHaveBeenCalledWith('internal');
      const notifications = await screen.findByRole('region', {
        name: 'Notifications',
      });
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
      expect(screen.getByText(/system keyring/i)).toBeInTheDocument();

      const signOutBtn = screen.getByRole('button', { name: 'Sign out' });
      fireEvent.click(signOutBtn);
      expect(signOutSpy).toHaveBeenCalledWith('signOut');
    });

    it('clears tool cache and shows toast notification', async () => {
      const clearSpy = vi.spyOn(commands, 'clearToolCache');
      openSettingsModal('tools');
      renderSettings('en', {
        tools: {
          installedByEntry: {},
          totalCacheBytes: 10485760,
          totalCacheArchives: 1,
          achievementIconCacheBytes: 0,
          cacheEntries: [
            {
              toolId: 'optiscaler',
              version: '0.7.4',
              channel: 'stable',
              bytes: 10485760,
              sha256Verified: true,
              inUse: true,
            },
          ],
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

      expect(screen.getByText('10 MB · 1 cached version')).toBeInTheDocument();
      expect(screen.getByText('OptiScaler 0.7.4')).toBeInTheDocument();
      expect(screen.getByText('OptiScaler')).toBeInTheDocument();

      const clearBtn = screen.getByRole('button', { name: 'Clear cache' });
      fireEvent.click(clearBtn);

      expect(clearSpy).toHaveBeenCalled();
      expect(await screen.findByText('Tool cache cleared')).toBeInTheDocument();
    });

    it('keeps the PC ports cache off non-Windows builds', () => {
      const infoSpy = vi.spyOn(commands, 'portsToolsCache');
      openSettingsModal('tools');
      renderSettings('en', undefined, { platform: 'linux' });

      expect(screen.queryByText('Ports cache')).toBeNull();
      expect(screen.queryByTestId('ports-helper-tools')).toBeNull();
      expect(screen.queryByTestId('ports-clear-cache')).toBeNull();
      expect(infoSpy).not.toHaveBeenCalled();
    });

    it('shows the pinned extraction tool and the ports download cache on Windows', async () => {      const infoSpy = vi.spyOn(commands, 'portsToolsCache').mockResolvedValue({
        downloadsBytes: 734_003_200,
        helperTools: [
          {
            id: 'ps4-pkg',
            name: 'Fixture PKG extractor',
            license: 'GPL-3.0',
            sourceUrl: 'https://github.com/example/fixture-pkg',
            version: 'v2.4.1',
            cacheBytes: 18_874_368,
          },
        ],
        installActive: false,
      });
      openSettingsModal('tools');
      renderSettings('en', undefined, { platform: 'windows' });

      expect(await screen.findByText('Fixture PKG extractor')).toBeInTheDocument();
      expect(screen.getByText('Pinned version v2.4.1')).toBeInTheDocument();
      expect(screen.getByText('GPL-3.0')).toBeInTheDocument();
      expect(screen.getByText('18 MB')).toBeInTheDocument();
      expect(screen.getByTestId('ports-downloads-size')).toHaveTextContent('700 MB');
      expect(infoSpy).toHaveBeenCalled();
    });

    it('shows the helper-tools group in pt-BR', async () => {
      vi.spyOn(commands, 'portsToolsCache').mockResolvedValue({
        downloadsBytes: 734_003_200,
        helperTools: [
          {
            id: 'ps4-pkg',
            name: 'Fixture PKG extractor',
            license: 'GPL-3.0',
            sourceUrl: 'https://github.com/example/fixture-pkg',
            version: 'v2.4.1',
            cacheBytes: 18_874_368,
          },
        ],
        installActive: false,
      });
      openSettingsModal('tools');
      renderSettings('pt-BR', undefined, { platform: 'windows' });

      expect(await screen.findByText('Versão fixada v2.4.1')).toBeInTheDocument();
      expect(screen.getByText('Ferramentas auxiliares')).toBeInTheDocument();
      expect(screen.getByText('Cache de ports')).toBeInTheDocument();
      expect(screen.getByTestId('ports-downloads-size')).toHaveTextContent('700 MB');
    });

    it('handles an overlay with no extraction tool configured', async () => {
      vi.spyOn(commands, 'portsToolsCache').mockResolvedValue({
        downloadsBytes: 0,
        helperTools: [],
        installActive: false,
      });
      openSettingsModal('tools');
      renderSettings('en', undefined, { platform: 'windows' });

      expect(await screen.findByText('No extraction tool configured')).toBeInTheDocument();
      expect(screen.getByTestId('ports-clear-cache')).toBeDisabled();
    });

    it('clears the ports download cache and shows a toast', async () => {
      vi.spyOn(commands, 'portsToolsCache').mockResolvedValue({
        downloadsBytes: 734_003_200,
        helperTools: [],
        installActive: false,
      });
      const clearSpy = vi.spyOn(commands, 'portsClearCache').mockResolvedValue({
        downloadsBytes: 0,
        helperTools: [],
        installActive: false,
      });
      openSettingsModal('tools');
      renderSettings('en', undefined, { platform: 'windows' });

      expect(await screen.findByText('700 MB')).toBeInTheDocument();
      const clearButton = screen.getByTestId('ports-clear-cache');
      expect(clearButton).toBeEnabled();
      fireEvent.click(clearButton);

      await waitFor(() => expect(clearSpy).toHaveBeenCalledTimes(1));
      expect(await screen.findByText('Port cache cleared')).toBeInTheDocument();
      expect(screen.getByTestId('ports-downloads-size')).toHaveTextContent('0 B');
    });

    it('shows the refusal when a port install owns the cache', async () => {
      vi.spyOn(commands, 'portsToolsCache').mockResolvedValue({
        downloadsBytes: 734_003_200,
        helperTools: [],
        installActive: true,
      });
      const clearSpy = vi
        .spyOn(commands, 'portsClearCache')
        .mockRejectedValue({ code: 'installInProgress' });
      openSettingsModal('tools');
      renderSettings('en', undefined, { platform: 'windows' });

      expect(await screen.findByText('Install running')).toBeInTheDocument();
      fireEvent.click(screen.getByTestId('ports-clear-cache'));

      await waitFor(() => expect(clearSpy).toHaveBeenCalledTimes(1));
      expect(
        await screen.findByText(
          'A port install is running. Clear the cache when it finishes.',
        ),
      ).toBeInTheDocument();
      expect(screen.getByTestId('ports-downloads-size')).toHaveTextContent('700 MB');
    });

    it('counts the pending settings and reverts them from the unsaved bar', () => {
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/default/destination',
          },
        },
      });

      expect(screen.queryByTestId('settings-unsaved-bar')).toBeNull();

      const destInput = screen.getByLabelText(/Backup destination/i);
      fireEvent.change(destInput, { target: { value: '/new/destination' } });
      fireEvent.click(screen.getByRole('switch', { name: 'Protect saves automatically' }));

      expect(screen.getByTestId('settings-unsaved-count')).toHaveTextContent(
        '2 settings changed',
      );
      expect(screen.getByText('Before: /default/destination')).toBeInTheDocument();
      expect(document.querySelector('.settings-scroll-content')).toHaveClass(
        'pb-[88px]',
      );

      fireEvent.click(screen.getByRole('button', { name: 'Revert' }));

      expect(uiStore.state.settingsDraft).toBeNull();
      expect(screen.queryByTestId('settings-unsaved-bar')).toBeNull();
      expect(document.querySelector('.settings-scroll-content')).toHaveClass(
        'pb-xl',
      );
      expect(screen.getByLabelText(/Backup destination/i)).toHaveValue(
        '/default/destination',
      );
      expect(
        screen.getByRole('switch', { name: 'Protect saves automatically' }),
      ).toBeChecked();
    });

    it('saves every pending backup setting together from the unsaved bar', async () => {
      const backupSpy = vi
        .spyOn(commands, 'setLocalBackupSettings')
        .mockResolvedValue(undefined);
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/default/destination',
          },
        },
      });

      fireEvent.change(screen.getByLabelText(/Backup destination/i), {
        target: { value: '/new/destination' },
      });
      fireEvent.click(screen.getByRole('switch', { name: 'Protect saves automatically' }));

      const saveBtn = screen.getByRole('button', { name: 'Save changes' });
      expect(saveBtn).not.toBeDisabled();
      fireEvent.click(saveBtn);

      await waitFor(() =>
        expect(backupSpy).toHaveBeenCalledWith(false, '/new/destination', 10),
      );
      await waitFor(() => expect(uiStore.state.settingsDraft).toBeNull());
      expect(await screen.findByText('Settings saved')).toBeInTheDocument();
    });

    it('saves a changed default retention together with the other backup fields', async () => {
      const backupSpy = vi
        .spyOn(commands, 'setLocalBackupSettings')
        .mockResolvedValue(undefined);
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/default/destination',
          },
        },
      });
      act(() => {
        uiStore.setState((previous) => ({
          ...previous,
          settingsDraft: { topic: 'backups', values: { retention: 20 } },
        }));
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
      await waitFor(() =>
        expect(backupSpy).toHaveBeenCalledWith(true, '/default/destination', 20),
      );
      await waitFor(() => expect(uiStore.state.settingsDraft).toBeNull());
    });

    it('saves the pending settings with Ctrl+S while the bar is visible', async () => {
      const backupSpy = vi
        .spyOn(commands, 'setLocalBackupSettings')
        .mockResolvedValue(undefined);
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/default/destination',
          },
        },
      });

      fireEvent.change(screen.getByLabelText(/Backup destination/i), {
        target: { value: '/new/destination' },
      });
      fireEvent.keyDown(window, { key: 's', ctrlKey: true });

      await waitFor(() =>
        expect(backupSpy).toHaveBeenCalledWith(true, '/new/destination', 10),
      );
      await waitFor(() => expect(uiStore.state.settingsDraft).toBeNull());
    });

    it('keeps the draft and shows the destination error when saving fails', async () => {
      vi.spyOn(commands, 'setLocalBackupSettings').mockRejectedValue(
        'invalid destination',
      );
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/default/destination',
          },
        },
      });

      fireEvent.change(screen.getByLabelText(/Backup destination/i), {
        target: { value: '/new/destination' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() =>
        expect(uiStore.state.settingsDraftError).toBe('invalid destination'),
      );
      expect(uiStore.state.settingsDraft).toEqual({
        topic: 'backups',
        values: { destination: '/new/destination' },
      });
      expect(screen.getByRole('alert')).toHaveTextContent('invalid destination');
      expect(screen.getByTestId('settings-unsaved-bar')).toBeInTheDocument();
    });

    it('keeps the draft and blocks a duplicate save until the awaited save ends', async () => {
      let resolveSave: () => void = () => undefined;
      const pending = new Promise<void>((resolve) => {
        resolveSave = resolve;
      });
      const backupSpy = vi
        .spyOn(commands, 'setLocalBackupSettings')
        .mockReturnValue(pending);
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/default/destination',
          },
        },
      });

      fireEvent.change(screen.getByLabelText(/Backup destination/i), {
        target: { value: '/new/destination' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() => expect(backupSpy).toHaveBeenCalledTimes(1));
      expect(uiStore.state.settingsDraft).toEqual({
        topic: 'backups',
        values: { destination: '/new/destination' },
      });
      expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();

      // Ctrl+S and a second click cannot start a parallel save.
      fireEvent.keyDown(window, { key: 's', ctrlKey: true });
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
      await act(async () => {
        await Promise.resolve();
      });
      expect(backupSpy).toHaveBeenCalledTimes(1);

      resolveSave();
      await waitFor(() => expect(uiStore.state.settingsDraft).toBeNull());
    });

    it('keeps edits made while a save is in flight instead of erasing them', async () => {
      let resolveSave: () => void = () => undefined;
      const pending = new Promise<void>((resolve) => {
        resolveSave = resolve;
      });
      vi.spyOn(commands, 'setLocalBackupSettings').mockReturnValue(pending);
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/default/destination',
          },
        },
      });

      fireEvent.change(screen.getByLabelText(/Backup destination/i), {
        target: { value: '/submitted' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
      await waitFor(() =>
        expect(commands.setLocalBackupSettings).toHaveBeenCalledWith(
          true,
          '/submitted',
          10,
        ),
      );

      fireEvent.change(screen.getByLabelText(/Backup destination/i), {
        target: { value: '/newer' },
      });
      resolveSave();

      await waitFor(() =>
        expect(uiStore.state.settingsDraft).toEqual({
          topic: 'backups',
          values: { destination: '/newer' },
        }),
      );
      expect(screen.getByTestId('settings-unsaved-count')).toHaveTextContent(
        '1 setting changed',
      );
      expect(screen.getByLabelText(/Backup destination/i)).toHaveValue('/newer');
    });

    it('keeps all three pending fields and the error when the destination is rejected', async () => {
      vi.spyOn(commands, 'setLocalBackupSettings').mockRejectedValue(
        'invalid destination',
      );
      openSettingsModal('backups');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          settings: {
            automatic: true,
            retention: 10,
            destination: '/default/destination',
          },
        },
      });

      fireEvent.change(screen.getByLabelText(/Backup destination/i), {
        target: { value: '/protected/folder' },
      });
      fireEvent.click(screen.getByRole('switch', { name: 'Protect saves automatically' }));
      act(() => {
        uiStore.setState((previous) => ({
          ...previous,
          settingsDraft: {
            topic: 'backups',
            values: {
              ...previous.settingsDraft?.values,
              automatic: false,
              destination: '/protected/folder',
              retention: 20,
            },
          },
        }));
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

      await waitFor(() =>
        expect(uiStore.state.settingsDraftError).toBe('invalid destination'),
      );
      expect(uiStore.state.settingsDraft).toEqual({
        topic: 'backups',
        values: {
          automatic: false,
          destination: '/protected/folder',
          retention: 20,
        },
      });
      expect(screen.getByTestId('settings-unsaved-count')).toHaveTextContent(
        '3 settings changed',
      );
      expect(screen.getByRole('alert')).toHaveTextContent('invalid destination');
      expect(screen.getByTestId('settings-unsaved-bar')).toBeInTheDocument();
    });

    it('resets hidden games in stores panel and shows toast', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      openSettingsModal('stores');
      renderSettings('en', {
        library: {
          ...createDefaultLibraryView(),
          generation: 1,
          layout: {
            view: 'grid',
            sort: { key: 'title', direction: 'ascending' },
            scope: 'all',
            group: 'none',
            collapsedGroups: {},
            filters: createDefaultFilters(),
            sidebarWidth: 200,
            detailWidth: 300,
            detailOpen: false,
            sidebarCollapsed: false,
          },
          search: '',
          access: 'verified',
          total: 10,
          installedTotal: 5,
          providers: [{ id: 'steam', name: 'Steam', count: 5, installedCount: 5, ownedCount: 0, sharedCount: 0, sharedLabel: 'Steam Family' }],
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
        storeAccounts: [{
          provider: 'steam',
          state: 'connected',
          accountId: '76561198000000000',
          accountName: 'cassi',
          ownedCount: 0,
          sharedCount: 0,
          sharedLabel: null,
          installedCount: 5,
          lastCheckedAt: null,
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
        }],
      });

      expect(screen.getByRole('heading', { name: 'Steam' })).toBeInTheDocument();
      expect(screen.getByText('installed on this PC')).toBeInTheDocument();

      const resetBtn = screen.getByRole('button', { name: /Show hidden games/i });
      fireEvent.click(resetBtn);

      expect(libraryInputSpy).toHaveBeenCalledWith('resetExcluded');
      const notifications = screen.getByRole('region', { name: 'Notifications' });
      expect(within(notifications).getByRole('status')).toHaveTextContent(/Show hidden games/i);
    });
  });

  describe('DRM-free watched folders', () => {
    const watchedSnapshot: Partial<Snapshot> = {
      localGames: {
        ...createDefaultSnapshot().localGames,
        watches: [
          {
            id: 'w-1',
            path: 'C:\\Games\\Indie',
            enabled: true,
            entryCount: 1,
            lastCheckedAt: null,
            state: 'idle',
            unavailableSince: null,
          },
        ],
      },
    };

    it('shows the watched folders on the DRM-free topic', () => {
      openSettingsModal('drmFree');
      renderSettings('en', watchedSnapshot);

      expect(screen.getByText('C:\\Games\\Indie')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Check all now' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Add folder…' })).toBeInTheDocument();
    });

    it('keeps the watched-folder controls off the Stores page', () => {
      openSettingsModal('stores');
      renderSettings('en', watchedSnapshot);

      expect(screen.queryByText('C:\\Games\\Indie')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Add folder…' })).not.toBeInTheDocument();
    });

    it('adds a folder through the native picker', () => {
      const pickSpy = vi
        .spyOn(commands, 'pickLocalWatchFolder')
        .mockResolvedValue(undefined);
      openSettingsModal('drmFree');
      renderSettings('en');

      fireEvent.click(screen.getByRole('button', { name: 'Add folder…' }));

      expect(pickSpy).toHaveBeenCalledTimes(1);
    });

    it('stops monitoring a folder without removing its registered games', () => {
      const removeSpy = vi
        .spyOn(commands, 'removeLocalWatch')
        .mockResolvedValue(undefined);
      openSettingsModal('drmFree');
      renderSettings('en', watchedSnapshot);

      fireEvent.click(screen.getByRole('button', { name: 'Stop watching' }));

      expect(removeSpy).toHaveBeenCalledWith('w-1');
    });

    it('does not list disabled watches', () => {
      openSettingsModal('drmFree');
      renderSettings('en', {
        localGames: {
          ...createDefaultSnapshot().localGames,
          watches: [
            {
              id: 'w-1',
              path: 'C:\\Games\\Indie',
              enabled: false,
              entryCount: 1,
              lastCheckedAt: null,
              state: 'idle',
              unavailableSince: null,
            },
          ],
        },
      });

      expect(screen.queryByText('C:\\Games\\Indie')).not.toBeInTheDocument();
      expect(screen.getByText('No watched folders')).toBeInTheDocument();
    });

    it('uses the Portuguese labels on the DRM-free topic', () => {
      openSettingsModal('drmFree');
      renderSettings('pt-BR', watchedSnapshot);

      expect(
        screen.getByRole('button', { name: 'Adicionar pasta…' }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole('button', { name: 'Verificar tudo agora' }),
      ).toBeInTheDocument();
    });
  });
});
