import { beforeEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query';
import { createAppQueryClient } from '../../AppProviders';
import { render, screen, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { commands } from '../../api/commands';
import { createDefaultFilters, type LibraryView, type LocalView } from '../../api/types';
import type { PortsView } from '../../api/ports';
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
    installedTotal: 42,
    providers: [
      { id: 'steam', name: 'Steam', count: 30, installedCount: 30, ownedCount: 0, familyCount: 0 },
      { id: 'gog', name: 'GOG', count: 12, installedCount: 12, ownedCount: 0, familyCount: 0 },
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
      filters: createDefaultFilters(),
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

  const samplePorts: PortsView = {
    catalog: {
      state: 'ready',
      fetchedAt: 1_700_000_000,
      entryCount: 0,
      error: null,
    },
    entries: [],
    provenance: {},
    jobs: [],
    unseenCount: 2,
    preferences: {
      view: 'grid',
      filters: { consoles: [], guided: false, verified: false, installed: false },
      lastParentFolder: null,
    },
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

    it.each([
      ['en', 'Linux Package Manager'],
      ['pt-BR', 'Gerenciador de pacotes Linux'],
    ])('localizes the PackageKit manifest in the %s sidebar', (locale, label) => {
      const library = {
        ...sampleLibrary,
        providers: [
          ...sampleLibrary.providers,
          { ...sampleLibrary.providers[0], id: 'packagekit', name: 'Gerenciador de pacotes Linux' },
          { ...sampleLibrary.providers[0], id: 'external', name: 'External Games' },
        ],
      };
      renderWithProviders(<Sidebar library={library} localGames={sampleLocalGames} />, locale);
      expect(screen.getByTestId('sidebar-provider-packagekit')).toHaveTextContent(label);
      expect(screen.getByTestId('sidebar-provider-external')).toHaveTextContent('External Games');
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

    it('no longer renders the removed version label', () => {
      renderWithProviders(
        <Sidebar library={sampleLibrary} localGames={sampleLocalGames} />,
      );

      expect(screen.queryByTestId('sidebar-version-label')).not.toBeInTheDocument();
      expect(screen.queryByText(/GameNode · /)).not.toBeInTheDocument();
    });

    it('sends setSidebarCollapsed in both directions from the header toggle', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();

      const view = renderWithProviders(
        <Sidebar library={sampleLibrary} localGames={sampleLocalGames} />,
      );

      const expandedToggle = screen.getByTestId('sidebar-collapse-toggle');
      expect(expandedToggle).toHaveAttribute('aria-label', 'Collapse sidebar');
      expect(expandedToggle).toHaveAttribute('aria-expanded', 'true');

      fireEvent.click(expandedToggle);
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarCollapsed: true });

      const collapsedLibrary: LibraryView = {
        ...sampleLibrary,
        layout: { ...sampleLibrary.layout, sidebarCollapsed: true },
      };

      view.rerender(
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale="en">
            <Sidebar library={collapsedLibrary} localGames={sampleLocalGames} />
          </I18nProvider>
        </QueryClientProvider>,
      );

      const collapsedToggle = screen.getByTestId('sidebar-collapse-toggle');
      expect(collapsedToggle).toHaveAttribute('aria-label', 'Expand sidebar');
      expect(collapsedToggle).toHaveAttribute('aria-expanded', 'false');

      fireEvent.click(collapsedToggle);
      expect(libraryInputSpy).toHaveBeenCalledWith({ setSidebarCollapsed: false });
    });

    it('renders the collapsed dock at 72px with accessible rows, dots and no resize handle', () => {
      const collapsedLibrary: LibraryView = {
        ...sampleLibrary,
        layout: { ...sampleLibrary.layout, sidebarCollapsed: true },
      };

      renderWithProviders(
        <Sidebar
          library={collapsedLibrary}
          localGames={sampleLocalGames}
          ports={samplePorts}
          signedIn
          user={{ id: 'u-1', username: 'GamerG' }}
          platform="windows"
        />,
      );

      expect(screen.getByTestId('sidebar')).toHaveStyle({ width: '72px' });
      expect(screen.queryByRole('separator')).not.toBeInTheDocument();
      expect(screen.queryByText('Navigate')).not.toBeInTheDocument();
      expect(screen.queryByText('Explore')).not.toBeInTheDocument();
      expect(screen.queryByText('Your space')).not.toBeInTheDocument();
      expect(screen.queryByText('Steam')).not.toBeInTheDocument();
      expect(screen.queryByTestId('sidebar-all-count')).not.toBeInTheDocument();
      expect(screen.queryByTestId('sidebar-provider-count-steam')).not.toBeInTheDocument();
      expect(
        screen.queryByTestId('sidebar-provider-installed-steam'),
      ).not.toBeInTheDocument();

      expect(screen.getByRole('button', { name: 'PC ports, 2 installs running or unseen' })).toHaveAttribute(
        'title',
        'PC ports',
      );
      expect(screen.getByTestId('sidebar-ports-badge')).toHaveAttribute(
        'aria-label',
        '2 installs running or unseen',
      );
      expect(screen.getByTestId('provider-warning-gog')).toBeInTheDocument();
      expect(screen.getByTestId('sidebar-discovery-badge')).toBeInTheDocument();

      expect(screen.getByRole('button', { name: 'All games' })).toHaveAttribute(
        'title',
        'All games',
      );
      expect(screen.getByRole('button', { name: 'Steam' })).toHaveAttribute(
        'title',
        'Steam',
      );
      expect(screen.getByRole('button', { name: 'GOG, Provider sync failure' })).toHaveAttribute(
        'title',
        'GOG',
      );
      expect(
        screen.getByRole('button', { name: 'Discovered games, 1 games' }),
      ).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Add a game' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Settings' })).toBeInTheDocument();

      const account = screen.getByTestId('sidebar-account-button');
      expect(account).toHaveAttribute('aria-label', 'GamerG');
      expect(account).toHaveAttribute('title', 'GamerG');
      expect(screen.queryByText('My account')).not.toBeInTheDocument();
    });

    it('shows the signed-in account and opens account settings', () => {
      renderWithProviders(
        <Sidebar
          library={sampleLibrary}
          localGames={sampleLocalGames}
          signedIn
          user={{ id: 'u-1', username: 'GamerG', email: 'gamer@example.com' }}
        />,
      );

      const account = screen.getByTestId('sidebar-account-button');
      expect(account).toHaveTextContent('GamerG');
      expect(account).toHaveTextContent('My account');

      fireEvent.click(account);
      expect(uiStore.state.settingsOpen).toBe(true);
      expect(uiStore.state.settingsTopic).toBe('account');
    });

    it('omits the account row when signed out or without a user', () => {
      const view = renderWithProviders(
        <Sidebar
          library={sampleLibrary}
          localGames={sampleLocalGames}
          signedIn={false}
        />,
      );
      expect(screen.queryByTestId('sidebar-account-button')).not.toBeInTheDocument();
      view.unmount();

      const noUser = renderWithProviders(
        <Sidebar
          library={sampleLibrary}
          localGames={sampleLocalGames}
          signedIn
          user={null}
        />,
      );
      expect(screen.queryByTestId('sidebar-account-button')).not.toBeInTheDocument();
      noUser.unmount();

      // A signed-in flag injected without a user must not crash and shows no account row.
      renderWithProviders(
        <Sidebar library={sampleLibrary} localGames={sampleLocalGames} signedIn />,
      );
      expect(screen.queryByTestId('sidebar-account-button')).not.toBeInTheDocument();
    });

    it('does not render destinations the app does not offer', () => {
      renderWithProviders(
        <Sidebar
          library={sampleLibrary}
          localGames={sampleLocalGames}
          ports={samplePorts}
          signedIn
          platform="windows"
        />,
      );

      for (const label of [
        'Home',
        'Daily Node',
        'Collections',
        'Achievements',
        'Premium',
      ]) {
        expect(
          screen.queryByRole('button', { name: label }),
        ).not.toBeInTheDocument();
        expect(screen.queryByText(label)).not.toBeInTheDocument();
      }
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

  describe('Sidebar PC ports destination', () => {
    const renderSignedInSidebar = (
      overrides: Partial<React.ComponentProps<typeof Sidebar>> = {},
      locale = 'en',
    ) =>
      renderWithProviders(
        <Sidebar
          library={sampleLibrary}
          localGames={sampleLocalGames}
          ports={samplePorts}
          signedIn
          platform="windows"
          {...overrides}
        />,
        locale,
      );

    it('switches the content area and marks only PC ports active', () => {
      const portsOpenSpy = vi.spyOn(commands, 'portsOpen').mockResolvedValue();

      renderSignedInSidebar();

      expect(screen.getByText('Navigate')).toBeInTheDocument();
      expect(screen.getByText('Explore')).toBeInTheDocument();
      expect(screen.getByText('Your space')).toBeInTheDocument();
      expect(screen.getByTestId('sidebar-explore-group')).toBeInTheDocument();
      expect(uiStore.state.activeDestination).toBe('library');
      expect(screen.getByTestId('sidebar-all-games')).toHaveAttribute('aria-current', 'page');

      fireEvent.click(screen.getByTestId('sidebar-ports'));

      expect(portsOpenSpy).toHaveBeenCalledTimes(1);
      expect(uiStore.state.activeDestination).toBe('ports');
      expect(screen.getByTestId('sidebar-ports')).toHaveAttribute('aria-current', 'page');
      expect(screen.getByTestId('sidebar-all-games')).not.toHaveAttribute('aria-current');
      expect(screen.getByTestId('sidebar-provider-steam')).not.toHaveAttribute('aria-current');
    });

    it('returns to the library with the chosen filter and preserves the rest of the layout', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
      const steamLibrary: LibraryView = {
        ...sampleLibrary,
        layout: { ...sampleLibrary.layout, filter: { provider: 'steam' } },
      };

      const view = renderSignedInSidebar({ library: steamLibrary });

      expect(screen.getByTestId('sidebar-provider-steam')).toHaveAttribute('aria-current', 'page');

      fireEvent.click(screen.getByTestId('sidebar-ports'));
      // Opening the catalog must not clear or rewrite the library filter.
      expect(libraryInputSpy).not.toHaveBeenCalled();
      expect(screen.getByTestId('sidebar-provider-steam')).not.toHaveAttribute('aria-current');

      fireEvent.click(screen.getByTestId('sidebar-provider-gog'));

      expect(uiStore.state.activeDestination).toBe('library');
      expect(libraryInputSpy).toHaveBeenCalledTimes(1);
      expect(libraryInputSpy).toHaveBeenCalledWith({ setFilter: { provider: 'gog' } });

      // The engine answers with the updated layout; the sidebar marks exactly that item.
      const gogLibrary: LibraryView = {
        ...sampleLibrary,
        layout: { ...sampleLibrary.layout, filter: { provider: 'gog' } },
      };
      view.rerender(
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale="en">
            <Sidebar
              library={gogLibrary}
              localGames={sampleLocalGames}
              ports={samplePorts}
              signedIn
              platform="windows"
            />
          </I18nProvider>
        </QueryClientProvider>,
      );
      expect(screen.getByTestId('sidebar-provider-gog')).toHaveAttribute('aria-current', 'page');
      expect(screen.getByTestId('sidebar-ports')).not.toHaveAttribute('aria-current');
    });

    it('shows the unseen install badge with an accessible count and hides it at zero', () => {
      const view = renderSignedInSidebar();

      const badge = screen.getByTestId('sidebar-ports-badge');
      expect(badge).toHaveTextContent('2');
      expect(badge).toHaveAttribute('aria-label', '2 installs running or unseen');
      expect(screen.getByTestId('sidebar-ports')).toHaveTextContent('PC ports');

      view.unmount();
      renderSignedInSidebar({ ports: { ...samplePorts, unseenCount: 0 } });
      expect(screen.queryByTestId('sidebar-ports-badge')).not.toBeInTheDocument();
    });

    it('reaches PC ports by keyboard before the library rows', async () => {
      const user = userEvent.setup();
      renderSignedInSidebar();

      await user.tab();
      expect(document.activeElement).toBe(screen.getByTestId('sidebar-collapse-toggle'));
      await user.tab();
      expect(document.activeElement).toBe(screen.getByTestId('sidebar-ports'));
      await user.tab();
      expect(document.activeElement).toBe(screen.getByTestId('sidebar-all-games'));
      await user.tab();
      expect(document.activeElement).toBe(screen.getByTestId('sidebar-provider-steam'));
      await user.tab();
      expect(document.activeElement).toBe(screen.getByTestId('sidebar-provider-gog'));

      screen.getByTestId('sidebar-ports').focus();
      await user.keyboard('{Enter}');
      expect(uiStore.state.activeDestination).toBe('ports');

      fireEvent.click(screen.getByTestId('sidebar-all-games'));
      expect(uiStore.state.activeDestination).toBe('library');

      screen.getByTestId('sidebar-ports').focus();
      await user.keyboard(' ');
      expect(uiStore.state.activeDestination).toBe('ports');
    });

    it('hides the destination on Linux and when signed out', () => {
      const linux = renderSignedInSidebar({ platform: 'linux' });
      expect(screen.queryByTestId('sidebar-explore-group')).not.toBeInTheDocument();
      expect(screen.queryByTestId('sidebar-ports')).not.toBeInTheDocument();
      linux.unmount();

      renderSignedInSidebar({ signedIn: false });
      expect(screen.queryByTestId('sidebar-explore-group')).not.toBeInTheDocument();
      expect(screen.queryByTestId('sidebar-ports')).not.toBeInTheDocument();
    });

    it('renders the pt-BR labels and badge text', () => {
      renderSignedInSidebar({}, 'pt-BR');

      expect(screen.getByText('Navegar')).toBeInTheDocument();
      expect(screen.getByText('Explorar')).toBeInTheDocument();
      expect(screen.getByText('Seu espaço')).toBeInTheDocument();
      expect(screen.getByText('Ports para PC')).toBeInTheDocument();
      expect(screen.getByText('Todos os jogos')).toBeInTheDocument();
      expect(screen.getByText('Adicionar jogo')).toBeInTheDocument();
      expect(screen.getByText('Jogos descobertos')).toBeInTheDocument();
      expect(screen.getByTestId('sidebar-ports-badge')).toHaveAttribute(
        'aria-label',
        '2 instalações em andamento ou ainda não vistas',
      );
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
