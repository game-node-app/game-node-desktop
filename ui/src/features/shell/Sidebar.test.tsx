import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import { commands } from '../../api/commands';
import {
  createDefaultFilters,
  createDefaultLibraryLayout,
  createDefaultLibraryView,
  createDefaultPortsView,
  createDefaultSnapshot,
  type LibraryView,
  type StoreAccountView,
} from '../../api/types';
import { I18nProvider } from '../../i18n';
import {
  openSaveView,
  resetUiStore,
  setActiveDestination,
  uiStore,
} from '../../store/uiStore';
import { Sidebar, type SidebarProps } from './Sidebar';

function renderSidebar(
  props: SidebarProps = {},
  locale: 'en' | 'pt-BR' = 'en',
) {
  const queryClient = createAppQueryClient();
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>
          <Sidebar {...props} />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

function makeLibrary(overrides: Partial<LibraryView> = {}): LibraryView {
  const base = createDefaultLibraryView();
  return {
    ...base,
    generation: 1,
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
        sharedLabel: null,
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
    collections: [
      { id: 'backlog-curto', name: 'Backlog curto', count: 9 },
      { id: 'coop-no-sofa', name: 'Coop no sofá', count: 6 },
    ],
    savedViews: [
      { id: 7, name: 'Steam 10 h+', count: 42, active: false },
      { id: 8, name: 'Coop no sofá', count: 4, active: false },
    ],
    layout: createDefaultLibraryLayout(),
    ...overrides,
  };
}

function makeStoreAccount(
  overrides: Partial<StoreAccountView> = {},
): StoreAccountView {
  return {
    provider: 'steam',
    state: 'connected',
    accountId: 'steam-1',
    accountName: 'cassi',
    ownedCount: 30,
    sharedCount: 0,
    sharedLabel: null,
    installedCount: 24,
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
    canRemoveCredential: true,
    ...overrides,
  };
}

const emptyLocal = () => createDefaultSnapshot().localGames;

describe('Sidebar', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('renders every scope count from scopeCounts and sends the scope input', () => {
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    renderSidebar({ library: makeLibrary(), localGames: emptyLocal(), signedIn: true });

    const all = screen.getByRole('button', { name: 'All games' });
    expect(within(all).getByText('42')).toBeInTheDocument();
    expect(within(screen.getByRole('button', { name: 'Installed' })).getByText('30')).toBeInTheDocument();
    expect(within(screen.getByRole('button', { name: 'Favorites' })).getByText('12')).toBeInTheDocument();
    expect(within(screen.getByRole('button', { name: 'Recently played' })).getByText('8')).toBeInTheDocument();
    expect(within(screen.getByRole('button', { name: 'Running' })).getByText('1')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Installed' }));
    expect(spy).toHaveBeenLastCalledWith({ setScope: 'installed' });
    expect(uiStore.state.activeDestination).toBe('library');
    fireEvent.click(screen.getByRole('button', { name: 'Favorites' }));
    expect(spy).toHaveBeenLastCalledWith({ setScope: 'favorites' });
    fireEvent.click(screen.getByRole('button', { name: 'Recently played' }));
    expect(spy).toHaveBeenLastCalledWith({ setScope: 'recent' });
    fireEvent.click(screen.getByRole('button', { name: 'Running' }));
    expect(spy).toHaveBeenLastCalledWith({ setScope: 'running' });
  });

  it('marks the All scope only when no store facet is active', () => {
    const library = makeLibrary({ layout: createDefaultLibraryLayout() });
    const first = renderSidebar({ library, localGames: emptyLocal(), signedIn: true });

    expect(screen.getByRole('button', { name: 'All games' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    first.unmount();

    renderSidebar({
      library: makeLibrary({
        layout: {
          ...createDefaultLibraryLayout(),
          filters: { ...createDefaultFilters(), stores: ['steam'] },
        },
      }),
      localGames: emptyLocal(),
      signedIn: true,
    });
    expect(screen.getByRole('button', { name: 'All games' })).not.toHaveAttribute(
      'aria-current',
    );
  });

  it('leaves a store shortcut from All games without clearing other facets', () => {
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const filters = { ...createDefaultFilters(), stores: ['steam'], favoritesOnly: true };
    renderSidebar({
      library: makeLibrary({
        layout: { ...createDefaultLibraryLayout(), filters },
      }),
      localGames: emptyLocal(),
      signedIn: true,
    });

    fireEvent.click(screen.getByRole('button', { name: 'All games' }));
    expect(spy).toHaveBeenNthCalledWith(1, { setScope: 'all' });
    expect(spy).toHaveBeenNthCalledWith(2, {
      setFilters: { ...filters, stores: [] },
    });
  });

  it('sends the All scope plus one store without resetting other facets', () => {
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const filters = {
      ...createDefaultFilters(),
      favoritesOnly: true,
      playtime: 'atLeast10h' as const,
    };
    renderSidebar({
      library: makeLibrary({
        layout: {
          ...createDefaultLibraryLayout(),
          scope: 'installed',
          filters,
        },
      }),
      localGames: emptyLocal(),
      signedIn: true,
    });

    act(() => setActiveDestination('ports'));
    fireEvent.click(screen.getByRole('button', { name: 'Steam' }));

    expect(spy).toHaveBeenNthCalledWith(1, { setScope: 'all' });
    expect(spy).toHaveBeenNthCalledWith(2, {
      setFilters: { ...filters, stores: ['steam'] },
    });
    expect(uiStore.state.activeDestination).toBe('library');
  });

  it('highlights exactly one store, and none when two stores are selected', () => {
    const single = makeLibrary({
      layout: {
        ...createDefaultLibraryLayout(),
        filters: { ...createDefaultFilters(), stores: ['steam'] },
      },
    });
    const first = renderSidebar({ library: single, localGames: emptyLocal(), signedIn: true });
    expect(screen.getByRole('button', { name: 'Steam' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('button', { name: 'GOG' })).not.toHaveAttribute(
      'aria-current',
    );
    first.unmount();

    renderSidebar({
      library: makeLibrary({
        layout: {
          ...createDefaultLibraryLayout(),
          filters: { ...createDefaultFilters(), stores: ['steam', 'gog'] },
        },
      }),
      localGames: emptyLocal(),
      signedIn: true,
    });
    expect(screen.getByRole('button', { name: 'Steam' })).not.toHaveAttribute(
      'aria-current',
    );
    expect(screen.getByRole('button', { name: 'GOG' })).not.toHaveAttribute(
      'aria-current',
    );
  });

  it('shows store attention from failedProviders and store accounts', () => {
    renderSidebar({
      library: makeLibrary({ failedProviders: ['gog'] }),
      localGames: emptyLocal(),
      signedIn: true,
      storeAccounts: [makeStoreAccount({ state: 'expired' })],
    });

    expect(screen.getByText('GOG needs attention')).toHaveClass('sr-only');
    expect(screen.getByText('Steam needs attention')).toHaveClass('sr-only');
  });

  it('lists collections and saved views, applies them and opens the view context menu', async () => {
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const user = userEvent.setup();
    renderSidebar({
      library: makeLibrary(),
      localGames: emptyLocal(),
      signedIn: true,
    });

    const collection = screen.getByRole('button', { name: 'Backlog curto' });
    expect(within(collection).getByText('9')).toBeInTheDocument();
    fireEvent.click(collection);
    expect(spy).toHaveBeenLastCalledWith({ setScope: { collection: 'backlog-curto' } });

    const view = screen.getByRole('button', { name: 'Steam 10 h+' });
    expect(within(view).getByText('42')).toBeInTheDocument();
    fireEvent.click(view);
    expect(spy).toHaveBeenLastCalledWith({ applyView: { id: 7 } });

    fireEvent.contextMenu(view);
    const rename = await screen.findByRole('menuitem', { name: 'Rename view' });
    await user.click(rename);
    expect(uiStore.state.saveViewOpen).toBe(true);
    expect(uiStore.state.renameViewId).toBe(7);
  });

  it('opens the delete confirmation from the saved-view context menu', async () => {
    const user = userEvent.setup();
    renderSidebar({
      library: makeLibrary(),
      localGames: emptyLocal(),
      signedIn: true,
    });

    fireEvent.contextMenu(screen.getByRole('button', { name: 'Steam 10 h+' }));
    await user.click(await screen.findByRole('menuitem', { name: 'Delete view' }));
    expect(await screen.findByTestId('delete-view-modal')).toHaveTextContent(
      'Delete view “Steam 10 h+”',
    );
  });

  it('highlights the active saved view and opens Save as view from the header plus', () => {
    const library = makeLibrary({
      savedViews: [{ id: 7, name: 'Steam 10 h+', count: 42, active: true }],
    });
    renderSidebar({ library, localGames: emptyLocal(), signedIn: true });

    expect(screen.getByRole('button', { name: 'Steam 10 h+' })).toHaveAttribute(
      'aria-current',
      'page',
    );

    fireEvent.click(screen.getByTestId('sidebar-save-view'));
    expect(uiStore.state.saveViewOpen).toBe(true);
    expect(uiStore.state.renameViewId).toBeNull();
  });

  it('shows the discovered count in brand, opens the review, and hides it at zero', async () => {
    const local = {
      ...emptyLocal(),
      candidates: [
        {
          fingerprint: 'one',
          watch: 'w',
          root: '/a',
          title: 'A',
          targets: [],
          evidence: [],
          ignored: false,
          incomplete: false,
        },
        {
          fingerprint: 'two',
          watch: 'w',
          root: '/b',
          title: 'B',
          targets: [],
          evidence: [],
          ignored: false,
          incomplete: false,
        },
        {
          fingerprint: 'ignored',
          watch: 'w',
          root: '/c',
          title: 'C',
          targets: [],
          evidence: [],
          ignored: true,
          incomplete: false,
        },
      ],
    };
    const view = renderSidebar({ library: makeLibrary(), localGames: local, signedIn: true });
    const button = screen.getByTestId('sidebar-discovery-button');
    expect(within(button).getByText('2')).toHaveClass('text-brand');
    fireEvent.click(within(button).getByRole('button', { name: 'Discovered games' }));
    expect(uiStore.state.discoveryModalOpen).toBe(true);

    view.unmount();
    renderSidebar({ library: makeLibrary(), localGames: emptyLocal(), signedIn: true });
    expect(screen.queryByTestId('sidebar-discovery-button')).not.toBeInTheDocument();
  });

  it('opens the catalog destination only when signed in', () => {
    const first = renderSidebar({
      library: makeLibrary(),
      localGames: emptyLocal(),
      signedIn: true,
    });
    fireEvent.click(screen.getByRole('button', { name: 'Explore GameNode' }));
    expect(uiStore.state.activeDestination).toBe('catalog');
    expect(screen.getByRole('button', { name: 'Explore GameNode' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    first.unmount();

    renderSidebar({ library: makeLibrary(), localGames: emptyLocal(), signedIn: false });
    expect(screen.queryByRole('button', { name: 'Explore GameNode' })).not.toBeInTheDocument();
  });

  it('keeps PC ports on Windows when signed in with its unseen badge', () => {
    const portsOpen = vi.spyOn(commands, 'portsOpen').mockResolvedValue();
    const ports = { ...createDefaultPortsView(), unseenCount: 3 };
    const view = renderSidebar({
      library: makeLibrary(),
      localGames: emptyLocal(),
      ports,
      signedIn: true,
      platform: 'windows',
    });

    expect(within(screen.getByTestId('sidebar-ports')).getByTestId('sidebar-ports-badge')).toHaveTextContent(
      '3',
    );
    fireEvent.click(screen.getByRole('button', { name: 'PC ports' }));
    expect(uiStore.state.activeDestination).toBe('ports');
    expect(portsOpen).toHaveBeenCalledTimes(1);

    view.unmount();
    renderSidebar({
      library: makeLibrary(),
      localGames: emptyLocal(),
      ports,
      signedIn: true,
      platform: 'linux',
    });
    expect(screen.queryByTestId('sidebar-ports')).not.toBeInTheDocument();
  });

  it('renders the 72px rail with accessible rows, keyboard activation and no resize handle', async () => {
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const user = userEvent.setup();
    const library = makeLibrary({
      layout: { ...createDefaultLibraryLayout(), sidebarCollapsed: true },
    });
    renderSidebar({
      library,
      localGames: emptyLocal(),
      ports: { ...createDefaultPortsView(), unseenCount: 2 },
      signedIn: true,
      platform: 'windows',
      storeAccounts: [makeStoreAccount({ state: 'expired' })],
    });

    expect(screen.getByTestId('sidebar')).toHaveStyle({ width: '72px' });
    expect(screen.queryByRole('separator')).not.toBeInTheDocument();
    expect(screen.queryByText('Your space')).not.toBeInTheDocument();
    expect(screen.queryByText('Steam')).not.toBeInTheDocument();
    expect(screen.queryByText('My account')).not.toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: 'All games' })).toHaveLength(1);
    expect(within(screen.getByTestId('sidebar-ports')).getByText('2')).toBeInTheDocument();
    expect(screen.getByTestId('provider-warning-steam')).toHaveAttribute(
      'aria-label',
      'Steam needs attention',
    );

    const favorites = screen.getByRole('button', { name: 'Favorites' });
    await user.tab();
    await user.tab();
    await user.tab();
    await user.tab();
    expect(document.activeElement).toBe(favorites);
    await user.keyboard('{Enter}');
    expect(spy).toHaveBeenLastCalledWith({ setScope: 'favorites' });

    expect(screen.getByRole('button', { name: 'Add DRM-free game' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Settings' })).toBeInTheDocument();
  });

  it('keeps the default 240px width and resize handle, and sends add/settings actions', async () => {
    const pickLocal = vi.spyOn(commands, 'pickLocalExecutable').mockResolvedValue();
    const user = userEvent.setup();
    renderSidebar({ library: makeLibrary(), localGames: emptyLocal(), signedIn: true });

    expect(screen.getByTestId('sidebar')).toHaveStyle({ width: '240px' });
    expect(screen.getByTestId('sidebar-resize-handle')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('sidebar-add-game'));
    expect(pickLocal).toHaveBeenCalledWith(null);

    await user.click(screen.getByRole('button', { name: 'Settings' }));
    expect(uiStore.state.settingsOpen).toBe(true);
    expect(uiStore.state.settingsTopic).toBe('account');
  });

  it('renders the redesigned labels in pt-BR', () => {
    renderSidebar(
      {
        library: makeLibrary(),
        localGames: emptyLocal(),
        ports: { ...createDefaultPortsView(), unseenCount: 2 },
        signedIn: true,
        platform: 'windows',
      },
      'pt-BR',
    );

    expect(screen.getByText('Seu espaço')).toBeInTheDocument();
    expect(screen.getByText('Todos os jogos')).toBeInTheDocument();
    expect(screen.getByText('Coleções e visões')).toBeInTheDocument();
    expect(screen.getByText('Ports para PC')).toBeInTheDocument();
    expect(screen.getByText('Adicionar jogo DRM-free')).toBeInTheDocument();
    expect(screen.getByText('Configurações')).toBeInTheDocument();
  });

  it('clears local modal state between openings', () => {
    const library = makeLibrary();
    renderSidebar({ library, localGames: emptyLocal(), signedIn: true });

    act(() => openSaveView());
    const input = screen.getByTestId('save-view-name');
    fireEvent.change(input, { target: { value: 'Steam 10 h+' } });
    expect(screen.getByTestId('save-view-validation')).toBeInTheDocument();

    act(() => uiStore.setState((s) => ({ ...s, saveViewOpen: false, renameViewId: null })));
    expect(screen.queryByTestId('save-view-modal')).not.toBeInTheDocument();

    act(() => openSaveView());
    expect(screen.getByTestId('save-view-name')).toHaveValue('');
    expect(screen.queryByTestId('save-view-validation')).not.toBeInTheDocument();
  });
});
