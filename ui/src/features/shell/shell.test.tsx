import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import type { ReactElement } from 'react';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import { commands } from '../../api/commands';
import {
  createDefaultFilters,
  createDefaultLibraryView,
  createDefaultSnapshot,
  type LibraryView,
  type StoreAccountView,
} from '../../api/types';
import { I18nProvider } from '../../i18n';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { ResizeHandle } from './ResizeHandle';
import { Sidebar } from './Sidebar';
import { SyncExplanationModal } from './SyncExplanationModal';
import { TopBar } from './TopBar';

describe('features/shell', () => {
  let queryClient: QueryClient;

  beforeAll(() => {
    if (typeof window !== 'undefined' && typeof window.PointerEvent === 'undefined') {
      (window as unknown as Record<string, unknown>).PointerEvent = window.MouseEvent;
    }
  });

  beforeEach(() => {
    queryClient = createAppQueryClient();
    resetUiStore();
    vi.restoreAllMocks();
  });

  const renderWithProviders = (ui: ReactElement, locale = 'en') =>
    render(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    );

  const sampleLibrary: LibraryView = {
    ...createDefaultLibraryView(),
    generation: 1,
    search: '',
    access: 'verified',
    total: 42,
    installedTotal: 30,
    providers: [
      {
        id: 'steam',
        name: 'Steam',
        count: 30,
        installedCount: 24,
        ownedCount: 30,
        sharedCount: 0,
        sharedLabel: 'Steam Family',
      },
      {
        id: 'gog',
        name: 'GOG',
        count: 12,
        installedCount: 6,
        ownedCount: 12,
        sharedCount: 0,
        sharedLabel: null,
      },
    ],
    scopeCounts: { all: 42, installed: 30, favorites: 12, recent: 8, running: 1 },
    rows: [],
    scanning: false,
    failedProviders: ['gog'],
    selected: [],
    actions: {
      selected: 0,
      play: { disabled: 'nothingSelected' },
      install: { disabled: 'nothingSelected' },
      matchGame: { disabled: 'nothingSelected' },
      addToGamenode: { disabled: 'nothingSelected' },
      changeMatch: false,
    },
    layout: {
      ...createDefaultLibraryView().layout,
      filters: createDefaultFilters(),
      sidebarWidth: 240,
      detailWidth: 360,
    },
  };

  const sampleStoreAccount = (provider: string): StoreAccountView => ({
    provider,
    state: 'connected',
    accountId: `${provider}-1`,
    accountName: 'cassi',
    ownedCount: 30,
    sharedCount: 0,
    sharedLabel: provider === 'steam' ? 'Steam Family' : null,
    installedCount: 18,
    lastCheckedAt: null,
    lastSyncedAt: null,
    storage: null,
    persistFailed: false,
    catalogError: null,
    sharedError: null,
    canSignIn: true,
    canCancelSignIn: false,
    canFocusSignIn: false,
    canSignOut: true,
    canCheck: true,
    canRemoveCredential: false,
  });

  describe('TopBar', () => {
    it('renders the wordmark, DESKTOP badge and account trigger', () => {
      renderWithProviders(<TopBar auth="signedOut" />);

      expect(screen.getByTestId('brand-logo')).toBeInTheDocument();
      expect(screen.getByTestId('desktop-badge')).toHaveTextContent('DESKTOP');
      expect(screen.getByRole('button', { name: 'Account menu' })).toBeInTheDocument();
      expect(screen.queryByTestId('user-display-name')).not.toBeInTheDocument();
    });

    it('shows the signed-in user and opens the app menu from the avatar', async () => {
      renderWithProviders(
        <TopBar
          auth={{
            signedIn: {
              user: { id: 'u-123', username: 'GamerG', email: 'gamer@example.com' },
            },
          }}
        />,
      );

      expect(screen.getByTestId('user-display-name')).toHaveTextContent('GamerG');
      expect(uiStore.state.appMenuOpen).toBe(false);

      fireEvent.click(screen.getByTestId('app-menu-trigger'));
      expect(await screen.findByTestId('app-menu')).toBeInTheDocument();
      expect(screen.getByTestId('app-menu-header')).toHaveTextContent('GamerG');
      expect(uiStore.state.appMenuOpen).toBe(true);
    });

    it('opens the command palette from the search trigger', () => {
      renderWithProviders(<TopBar auth="signedOut" />);

      fireEvent.click(screen.getByTestId('command-search'));

      expect(uiStore.state.paletteOpen).toBe(true);
    });
  });

  describe('Sidebar', () => {
    it('renders every scope count from scopeCounts and sends the scope input', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();

      renderWithProviders(
        <Sidebar
          library={sampleLibrary}
          localGames={createDefaultSnapshot().localGames}
          signedIn
        />,
      );

      expect(screen.getByRole('button', { name: 'All games' })).toHaveTextContent('42');
      expect(screen.getByRole('button', { name: 'Installed' })).toHaveTextContent('30');
      expect(screen.getByRole('button', { name: 'Favorites' })).toHaveTextContent('12');
      expect(screen.getByRole('button', { name: 'Recently played' })).toHaveTextContent('8');
      expect(screen.getByRole('button', { name: 'Running' })).toHaveTextContent('1');

      fireEvent.click(screen.getByRole('button', { name: 'Installed' }));
      expect(libraryInputSpy).toHaveBeenCalledWith({ setScope: 'installed' });
    });

    it('sends All plus one store and keeps the failed-provider attention icon', async () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();

      renderWithProviders(
        <Sidebar
          library={sampleLibrary}
          localGames={createDefaultSnapshot().localGames}
          storeAccounts={[sampleStoreAccount('steam'), sampleStoreAccount('gog')]}
          signedIn
        />,
      );

      expect(screen.getByText('Steam')).toBeInTheDocument();
      expect(screen.getByText('GOG needs attention')).toBeInTheDocument();

      fireEvent.click(
        within(screen.getByTestId('sidebar-provider-steam')).getByRole('button', {
          name: 'Steam',
        }),
      );

      await waitFor(() =>
        expect(libraryInputSpy).toHaveBeenCalledWith({
          setFilters: expect.objectContaining({ stores: ['steam'] }),
        }),
      );
      expect(libraryInputSpy).toHaveBeenCalledWith({ setScope: 'all' });
    });

    it('opens the discovery review and the add-game flow from the footer', () => {
      const pickLocalSpy = vi.spyOn(commands, 'pickLocalExecutable').mockResolvedValue();
      const localGames = createDefaultSnapshot().localGames;
      localGames.candidates = [
        {
          fingerprint: 'fp-1',
          watch: 'w-1',
          root: '/games/hollow-knight',
          title: 'Hollow Knight',
          targets: ['hollow_knight.exe'],
          evidence: [],
          ignored: false,
          incomplete: false,
        },
      ];

      renderWithProviders(
        <Sidebar library={sampleLibrary} localGames={localGames} signedIn />,
      );

      fireEvent.click(screen.getByTestId('sidebar-add-game'));
      expect(pickLocalSpy).toHaveBeenCalledWith(null);

      fireEvent.click(
        within(screen.getByTestId('sidebar-discovery-button')).getByRole('button'),
      );
      expect(uiStore.state.discoveryModalOpen).toBe(true);
    });
  });

  describe('ResizeHandle', () => {
    it('dragging updates the TanStack store and commits a clamped width on release', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();

      renderWithProviders(<ResizeHandle currentWidth={220} minWidth={160} maxWidth={360} />);
      const handle = screen.getByTestId('sidebar-resize-handle');

      fireEvent.pointerDown(handle, { clientX: 220 });
      fireEvent.pointerMove(window, { clientX: 280 });
      expect(uiStore.state.sidebarDragWidth).toBe(280);

      fireEvent.pointerUp(window, { clientX: 280 });
      expect(uiStore.state.sidebarDragWidth).toBeNull();
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarWidth: 280 });
    });

    it('clamps between the minimum and maximum', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
      renderWithProviders(<ResizeHandle currentWidth={200} minWidth={160} maxWidth={360} />);
      const handle = screen.getByTestId('sidebar-resize-handle');

      fireEvent.pointerDown(handle, { clientX: 200 });
      fireEvent.pointerMove(window, { clientX: 50 });
      expect(uiStore.state.sidebarDragWidth).toBe(160);
      fireEvent.pointerUp(window, { clientX: 50 });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarWidth: 160 });

      fireEvent.pointerDown(handle, { clientX: 200 });
      fireEvent.pointerMove(window, { clientX: 500 });
      expect(uiStore.state.sidebarDragWidth).toBe(360);
      fireEvent.pointerUp(window, { clientX: 500 });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarWidth: 360 });
    });

    it('moves with ArrowLeft and ArrowRight', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
      renderWithProviders(<ResizeHandle currentWidth={200} minWidth={160} maxWidth={360} />);
      const handle = screen.getByTestId('sidebar-resize-handle');

      fireEvent.keyDown(handle, { key: 'ArrowLeft' });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarWidth: 184 });
      fireEvent.keyDown(handle, { key: 'ArrowRight' });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarWidth: 216 });
    });
  });

  describe('SyncExplanationModal', () => {
    it('dismisses through the primary action', () => {
      const dismissSpy = vi.spyOn(commands, 'dismissSyncExplanation').mockResolvedValue();
      renderWithProviders(<SyncExplanationModal forceOpen />);

      expect(screen.getByRole('heading', { level: 2 })).toHaveTextContent(
        'Sync with GameNode',
      );
      fireEvent.click(screen.getByTestId('sync-primary-button'));
      expect(dismissSpy).toHaveBeenCalled();
    });

    it('opens Settings from the secondary action and dismisses', () => {
      const dismissSpy = vi.spyOn(commands, 'dismissSyncExplanation').mockResolvedValue();
      renderWithProviders(<SyncExplanationModal forceOpen />);

      fireEvent.click(screen.getByTestId('sync-secondary-button'));
      expect(dismissSpy).toHaveBeenCalled();
      expect(uiStore.state.settingsOpen).toBe(true);
      expect(uiStore.state.settingsTopic).toBe('account');
    });

    it('dismisses on Escape and renders nothing when closed', () => {
      const dismissSpy = vi.spyOn(commands, 'dismissSyncExplanation').mockResolvedValue();
      const first = renderWithProviders(<SyncExplanationModal forceOpen />);

      fireEvent.keyDown(window, { key: 'Escape' });
      expect(dismissSpy).toHaveBeenCalled();
      first.unmount();

      const { container } = renderWithProviders(<SyncExplanationModal forceOpen={false} />);
      expect(screen.queryByTestId('sync-explanation-modal')).not.toBeInTheDocument();
      expect(container.querySelector('[data-testid="sync-explanation-modal"]')).toBeNull();
    });

    it('localizes the modal in Brazilian Portuguese', () => {
      renderWithProviders(<SyncExplanationModal forceOpen />, 'pt-BR');
      expect(screen.getByText('Sincronizar com o GameNode')).toBeInTheDocument();
    });
  });
});
