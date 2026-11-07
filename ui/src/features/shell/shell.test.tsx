import { beforeEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query';
import { createAppQueryClient } from '../../AppProviders';
import { render, screen, fireEvent } from '@testing-library/react';
import React from 'react';
import { commands } from '../../api/commands';
import type { LibraryView, LocalView } from '../../api/types';
import { I18nProvider } from '../../i18n';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { AppMenu } from './AppMenu';
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

  const renderWithProviders = (ui: React.ReactElement, locale = 'en') => {
    return render(
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>,
    );
  };

  const sampleLibrary: LibraryView = {
    generation: 1,
    search: '',
    access: 'verified',
    total: 42,
    providers: [
      { id: 'steam', name: 'Steam', count: 30 },
      { id: 'gog', name: 'GOG', count: 12 },
    ],
    rows: [],
    scanning: false,
    failedProviders: ['gog'],
    selected: [],
    actions: {
      selected: 0,
      play: 'available',
      install: 'available',
      matchGame: 'available',
      addToGamenode: 'available',
      changeMatch: false,
    },
    layout: {
      view: 'grid',
      sort: { key: 'title', direction: 'ascending' },
      filter: 'all',
      sidebarWidth: 220,
      detailWidth: 320,
      detailOpen: false,
    },
  };

  const sampleLocalGames: LocalView = {
    inferences: [],
    recentlyAdded: [],
    discoveryIncomplete: [],
    discoveryScanning: false,
    discoveryCompleted: true,
    watches: [],
    reviewSelected: [],
    versions: [],
    busy: false,
    registrations: [],
    candidates: [
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
    ],
    profiles: [],
    jobs: [],
    settings: { automatic: false, destination: '/backups' },
  };

  describe('TopBar', () => {
    it('renders brand logo and DESKTOP badge', () => {
      renderWithProviders(<TopBar auth="signedOut" />);
      expect(screen.getByText('GameNode')).toBeInTheDocument();
      expect(screen.getByTestId('desktop-badge')).toHaveTextContent('DESKTOP');
      expect(screen.queryByTestId('online-dot')).not.toBeInTheDocument();
    });

    it('renders user info and green online dot when signed in', () => {
      renderWithProviders(
        <TopBar
          auth={{
            signedIn: {
              user: {
                id: 'u-123',
                username: 'GamerG',
                email: 'gamer@example.com',
              },
            },
          }}
        />,
      );

      expect(screen.getByTestId('online-dot')).toBeInTheDocument();
      expect(screen.getByTestId('user-display-name')).toHaveTextContent('GamerG');
    });

    it('toggles app menu open state when menu button is clicked', () => {
      renderWithProviders(<TopBar auth="signedOut" />);

      const toggleBtn = screen.getByRole('button', { name: 'Application menu' });
      expect(uiStore.state.appMenuOpen).toBe(false);

      fireEvent.click(toggleBtn);
      expect(uiStore.state.appMenuOpen).toBe(true);
      expect(screen.getByTestId('app-menu')).toBeInTheDocument();

      fireEvent.click(toggleBtn);
      expect(uiStore.state.appMenuOpen).toBe(false);
      expect(screen.queryByTestId('app-menu')).not.toBeInTheDocument();
    });
  });

  describe('AppMenu', () => {
    it('executes menu items actions properly', () => {
      const rescanSpy = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue();
      const pickLocalSpy = vi.spyOn(commands, 'pickLocalExecutable').mockResolvedValue();
      const quitSpy = vi.spyOn(commands, 'quitApp').mockResolvedValue();
      const signOutSpy = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue();

      renderWithProviders(<AppMenu isSignedIn={true} />);

      // Rescan
      fireEvent.click(screen.getByRole('menuitem', { name: 'Rescan library' }));
      expect(rescanSpy).toHaveBeenCalled();

      // Add a game
      fireEvent.click(screen.getByRole('menuitem', { name: 'Add a game' }));
      expect(pickLocalSpy).toHaveBeenCalledWith(null);

      // Settings
      fireEvent.click(screen.getByRole('menuitem', { name: 'Settings' }));
      expect(uiStore.state.settingsOpen).toBe(true);
      expect(uiStore.state.settingsTopic).toBe('account');

      // About
      fireEvent.click(screen.getByRole('menuitem', { name: 'About & licenses' }));
      expect(uiStore.state.settingsTopic).toBe('about');

      // Sign out
      fireEvent.click(screen.getByRole('menuitem', { name: 'Sign out' }));
      expect(signOutSpy).toHaveBeenCalledWith('signOut');

      // Quit
      fireEvent.click(screen.getByRole('menuitem', { name: 'Quit' }));
      expect(quitSpy).toHaveBeenCalled();
    });

    it('navigates with ArrowDown, ArrowUp, and closes on Escape', () => {
      uiStore.setState((s) => ({ ...s, appMenuOpen: true }));
      const onClose = vi.fn();

      renderWithProviders(<AppMenu isSignedIn={false} onClose={onClose} />);

      const menu = screen.getByTestId('app-menu');
      const items = screen.getAllByRole('menuitem');

      // Initially first item has focus
      expect(document.activeElement).toBe(items[0]);

      // ArrowDown moves to next item
      fireEvent.keyDown(menu, { key: 'ArrowDown' });
      expect(document.activeElement).toBe(items[1]);

      // ArrowUp moves back
      fireEvent.keyDown(menu, { key: 'ArrowUp' });
      expect(document.activeElement).toBe(items[0]);

      // ArrowUp wraps to last item
      fireEvent.keyDown(menu, { key: 'ArrowUp' });
      expect(document.activeElement).toBe(items[items.length - 1]);

      // Escape closes menu
      fireEvent.keyDown(menu, { key: 'Escape' });
      expect(uiStore.state.appMenuOpen).toBe(false);
      expect(onClose).toHaveBeenCalled();
    });
  });

  describe('Sidebar & ResizeHandle', () => {
    it('renders All games count and handles filter click', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();

      renderWithProviders(<Sidebar library={sampleLibrary} localGames={sampleLocalGames} />);

      expect(screen.getByTestId('sidebar-all-count')).toHaveTextContent('42');
      fireEvent.click(screen.getByTestId('sidebar-all-games'));
      expect(libraryInputSpy).toHaveBeenCalledWith({ setFilter: 'all' });
    });

    it('renders provider list and warning indicator for failed provider', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();

      renderWithProviders(<Sidebar library={sampleLibrary} localGames={sampleLocalGames} />);

      expect(screen.getByText('Steam')).toBeInTheDocument();
      expect(screen.getByText('GOG')).toBeInTheDocument();

      // GOG was marked as failed
      expect(screen.getByTestId('provider-warning-gog')).toBeInTheDocument();

      // Click steam filter
      fireEvent.click(screen.getByTestId('sidebar-provider-steam'));
      expect(libraryInputSpy).toHaveBeenCalledWith({ setFilter: { provider: 'steam' } });
    });

    it('handles Add a game and Discovery button clicks', () => {
      const pickLocalSpy = vi.spyOn(commands, 'pickLocalExecutable').mockResolvedValue();

      renderWithProviders(<Sidebar library={sampleLibrary} localGames={sampleLocalGames} />);

      fireEvent.click(screen.getByTestId('sidebar-add-game'));
      expect(pickLocalSpy).toHaveBeenCalledWith(null);

      expect(uiStore.state.discoveryModalOpen).toBe(false);
      fireEvent.click(screen.getByTestId('sidebar-discovery-button'));
      expect(uiStore.state.discoveryModalOpen).toBe(true);
    });

    it('renders version label in footer', () => {
      renderWithProviders(
        <Sidebar
          library={sampleLibrary}
          localGames={sampleLocalGames}
          version="0.2.5"
        />,
      );

      expect(screen.getByTestId('sidebar-version-label')).toHaveTextContent('GameNode · 0.2.5');
    });

    it('drags ResizeHandle updating TanStack store and commits clamped width on release', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();

      renderWithProviders(<ResizeHandle currentWidth={220} minWidth={160} maxWidth={360} />);

      const handle = screen.getByTestId('sidebar-resize-handle');

      // Start drag at x=220
      fireEvent.pointerDown(handle, { clientX: 220 });

      // Move to x=280
      fireEvent.pointerMove(window, { clientX: 280 });
      expect(uiStore.state.sidebarDragWidth).toBe(280);

      // Release at x=280
      fireEvent.pointerUp(window, { clientX: 280 });
      expect(uiStore.state.sidebarDragWidth).toBeNull();
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarWidth: 280 });
    });

    it('clamps ResizeHandle drag between 160 and 360', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();

      renderWithProviders(<ResizeHandle currentWidth={200} minWidth={160} maxWidth={360} />);

      const handle = screen.getByTestId('sidebar-resize-handle');

      // Drag below min
      fireEvent.pointerDown(handle, { clientX: 200 });
      fireEvent.pointerMove(window, { clientX: 50 });
      expect(uiStore.state.sidebarDragWidth).toBe(160);
      fireEvent.pointerUp(window, { clientX: 50 });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarWidth: 160 });

      // Drag above max
      fireEvent.pointerDown(handle, { clientX: 200 });
      fireEvent.pointerMove(window, { clientX: 500 });
      expect(uiStore.state.sidebarDragWidth).toBe(360);
      fireEvent.pointerUp(window, { clientX: 500 });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarWidth: 360 });
    });

    it('handles keyboard navigation on ResizeHandle with ArrowLeft and ArrowRight', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();

      renderWithProviders(<ResizeHandle currentWidth={200} minWidth={160} maxWidth={360} />);

      const handle = screen.getByTestId('sidebar-resize-handle');

      // ArrowLeft shrinks by 16px (200 - 16 = 184)
      fireEvent.keyDown(handle, { key: 'ArrowLeft' });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarWidth: 184 });

      // ArrowRight grows by 16px (200 + 16 = 216)
      fireEvent.keyDown(handle, { key: 'ArrowRight' });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarWidth: 216 });
    });
  });

  describe('SyncExplanationModal', () => {
    it('renders explanation and handles primary button dismissing explanation', () => {
      const dismissSpy = vi.spyOn(commands, 'dismissSyncExplanation').mockResolvedValue();

      renderWithProviders(<SyncExplanationModal forceOpen={true} />);

      expect(screen.getByRole('heading', { level: 2 })).toHaveTextContent('Sync with GameNode');

      const primaryBtn = screen.getByTestId('sync-primary-button');
      fireEvent.click(primaryBtn);
      expect(dismissSpy).toHaveBeenCalled();
    });

    it('handles secondary button opening settings and dismissing explanation', () => {
      const dismissSpy = vi.spyOn(commands, 'dismissSyncExplanation').mockResolvedValue();

      renderWithProviders(<SyncExplanationModal forceOpen={true} />);

      const secondaryBtn = screen.getByTestId('sync-secondary-button');
      fireEvent.click(secondaryBtn);
      expect(dismissSpy).toHaveBeenCalled();
      expect(uiStore.state.settingsOpen).toBe(true);
      expect(uiStore.state.settingsTopic).toBe('account');
    });

    it('dismisses modal when Escape key is pressed', () => {
      const dismissSpy = vi.spyOn(commands, 'dismissSyncExplanation').mockResolvedValue();

      renderWithProviders(<SyncExplanationModal forceOpen={true} />);

      fireEvent.keyDown(window, { key: 'Escape' });
      expect(dismissSpy).toHaveBeenCalled();
    });

    it('returns null when forceOpen is false', () => {
      const { container } = renderWithProviders(<SyncExplanationModal forceOpen={false} />);
      expect(container.firstChild).toBeNull();
    });

    it('renders localized SyncExplanationModal in pt-BR', () => {
      renderWithProviders(<SyncExplanationModal forceOpen={true} />, 'pt-BR');
      expect(screen.getByText('Sincronizar com o GameNode')).toBeInTheDocument();
      expect(screen.getByTestId('sync-primary-button')).toBeInTheDocument();
    });
  });
});
