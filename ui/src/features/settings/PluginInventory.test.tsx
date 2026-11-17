import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultSnapshot,
  type PluginInfo,
  type StoreAccountView,
} from '../../api';
import { I18nProvider } from '../../i18n';
import {
  openSettingsModal,
  resetUiStore,
  uiStore,
} from '../../store/uiStore';
import { ToastContainer } from './ToastContainer';
import { PluginsPanel } from './panels/PluginsPanel';

const steam: PluginInfo = {
  providerId: 'steam',
  pluginId: 'org.gamenode.plugin.steam-discovery',
  name: 'Steam',
  version: '1.4.2',
  source: 'official',
  status: 'installed',
  capabilities: ['library.discovery', 'auth.webview', 'secrets.read'],
  session: 'active',
  nativeAccess: ['network', 'files', 'process', 'registry'],
};

const epic: PluginInfo = {
  providerId: 'epic',
  pluginId: 'org.gamenode.plugin.epic-discovery',
  name: 'Epic Games Store',
  version: '2.0.1',
  source: 'official',
  status: 'installed',
  capabilities: ['library.discovery', 'account.state.read'],
  session: 'active',
  nativeAccess: ['network', 'files'],
};

const community: PluginInfo = {
  providerId: 'community',
  pluginId: 'dev.prateleira.local',
  name: 'Prateleira Local',
  version: '0.3.1',
  source: 'community',
  status: 'installed',
  capabilities: ['provider.context.read'],
  session: 'stopped',
  nativeAccess: ['files', 'dbus'],
};

function storeAccount(
  provider: string,
  state: StoreAccountView['state'],
): StoreAccountView {
  return {
    provider,
    state,
    accountId: null,
    accountName: null,
    ownedCount: 0,
    sharedCount: 0,
    sharedLabel: null,
    installedCount: 0,
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
  };
}

function renderPlugins(
  _plugins: PluginInfo[],
  options: {
    locale?: string;
    storeAccounts?: StoreAccountView[];
  } = {},
) {
  resetUiStore();
  openSettingsModal('plugins');
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, {
    ...createDefaultSnapshot(),
    storeAccounts: options.storeAccounts ?? [],
  });
  return {
    queryClient,
    ...render(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale={options.locale ?? 'en'}>
            <ToastContainer />
            <PluginsPanel />
          </I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    ),
  };
}

describe('PluginsPanel', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    resetUiStore();
  });

  afterEach(() => {
    delete (navigator as unknown as { clipboard?: unknown }).clipboard;
  });

  it('filters by trusted origin with live counts', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([
      steam,
      epic,
      community,
    ]);
    renderPlugins([steam, epic, community]);

    const filter = await screen.findByRole('radiogroup', {
      name: 'Filter by origin',
    });
    expect(within(filter).getByRole('radio', { name: 'All · 3' })).toBeChecked();
    expect(
      within(filter).getByRole('radio', { name: 'Official · 2' }),
    ).toBeInTheDocument();
    expect(
      within(filter).getByRole('radio', { name: 'Community · 1' }),
    ).toBeInTheDocument();

    fireEvent.click(within(filter).getByRole('radio', { name: 'Community · 1' }));
    expect(screen.getByRole('article', { name: 'Prateleira Local' })).toBeInTheDocument();
    expect(screen.queryByRole('article', { name: 'Steam' })).not.toBeInTheDocument();

    fireEvent.click(within(filter).getByRole('radio', { name: 'Official · 2' }));
    expect(screen.getByRole('article', { name: 'Steam' })).toBeInTheDocument();
    expect(screen.getByRole('article', { name: 'Epic Games Store' })).toBeInTheDocument();
    expect(
      screen.queryByRole('article', { name: 'Prateleira Local' }),
    ).not.toBeInTheDocument();
  });

  it('shows versions, origin, session state, native access and the community note', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([
      steam,
      epic,
      community,
    ]);
    renderPlugins([steam, epic, community]);

    expect(await screen.findByText('1.4.2')).toBeInTheDocument();
    expect(screen.getByText('2.0.1')).toBeInTheDocument();
    expect(screen.getByText('0.3.1')).toBeInTheDocument();
    expect(screen.getAllByText('Official').length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText('Community')).toBeInTheDocument();
    expect(screen.getAllByText('Session active').length).toBe(2);
    expect(screen.getByText('Stopped')).toBeInTheDocument();
    expect(screen.getAllByText('Network')).toHaveLength(2);
    expect(screen.getAllByText('Files')).toHaveLength(3);
    expect(screen.getByText('Processes')).toBeInTheDocument();
    expect(screen.getByText('Registry')).toBeInTheDocument();
    expect(screen.getByText('D-Bus')).toBeInTheDocument();
    expect(
      screen.getByText(
        "Community plugins aren't reviewed by GameNode. Only grant permissions that make sense.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/sandbox/i)).not.toBeInTheDocument();
  });

  it('renders an unknown native-access value as its raw neutral pill', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([
      { ...steam, nativeAccess: ['files', 'customAccess'] },
    ]);
    renderPlugins([steam]);

    expect(await screen.findByText('Files')).toBeInTheDocument();
    expect(screen.getByText('customAccess')).toBeInTheDocument();
  });

  it('shows a missing executable state without offering a session control', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([
      { ...steam, status: 'missing', session: 'stopped' },
    ]);
    renderPlugins([steam]);

    expect(await screen.findByText('Missing')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Start' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Stop' })).not.toBeInTheDocument();
  });

  it('copies the plugin id and confirms with a toast', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([steam]);
    renderPlugins([steam]);

    const copyButton = await screen.findByRole('button', {
      name: /Copy identifier: org\.gamenode\.plugin\.steam-discovery/,
    });
    fireEvent.click(copyButton);

    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        'org.gamenode.plugin.steam-discovery',
      ),
    );
    expect(await screen.findByText('Identifier copied')).toBeInTheDocument();
  });

  it('expands in-card details, marks the card selected and keeps grants apart from native access', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([steam, community]);
    renderPlugins([steam, community]);

    const card = await screen.findByRole('article', { name: 'Steam' });
    const details = within(card).getByRole('button', { name: 'Details' });
    expect(details).toHaveAttribute('aria-expanded', 'false');

    fireEvent.click(details);

    expect(details).toHaveAttribute('aria-expanded', 'true');
    expect(card).toHaveAttribute('data-selected', 'true');
    expect(card.className).toContain('border-brand');
    expect(
      within(card).getByRole('heading', { name: 'API permissions' }),
    ).toBeInTheDocument();
    expect(
      within(card).getByRole('heading', { name: 'Native system access' }),
    ).toBeInTheDocument();
    expect(within(card).getByText('library.discovery')).toBeInTheDocument();
    expect(within(card).getByText('auth.webview')).toBeInTheDocument();
    expect(within(card).getByText('secrets.read')).toBeInTheDocument();

    fireEvent.click(within(card).getByRole('button', { name: 'Hide details' }));
    expect(details).toHaveAttribute('aria-expanded', 'false');
    expect(card).not.toHaveAttribute('data-selected');
    expect(
      within(card).queryByRole('heading', { name: 'API permissions' }),
    ).not.toBeInTheDocument();
  });

  it('stops and starts a plugin process and refreshes its session state', async () => {
    const inventory = vi
      .spyOn(commands, 'pluginInventory')
      .mockResolvedValueOnce([epic])
      .mockResolvedValueOnce([{ ...epic, session: 'stopped' }])
      .mockResolvedValueOnce([epic]);
    const stop = vi.spyOn(commands, 'pluginStop').mockResolvedValue(undefined);
    const start = vi.spyOn(commands, 'pluginStart').mockResolvedValue(undefined);
    renderPlugins([epic]);

    fireEvent.click(await screen.findByRole('button', { name: 'Stop' }));
    await waitFor(() => expect(stop).toHaveBeenCalledWith('epic'));
    expect(await screen.findByRole('button', { name: 'Start' })).toBeInTheDocument();
    expect(screen.getByText('Stopped')).toBeInTheDocument();
    await waitFor(() => expect(inventory).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole('button', { name: 'Start' }));
    await waitFor(() => expect(start).toHaveBeenCalledWith('epic'));
    expect(await screen.findByRole('button', { name: 'Stop' })).toBeInTheDocument();
    expect(screen.getByText('Session active')).toBeInTheDocument();
  });

  it('keeps the active session and shows an error when stopping fails', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([epic]);
    vi.spyOn(commands, 'pluginStop').mockRejectedValue(
      new Error('plugin shutdown failed'),
    );
    renderPlugins([epic]);

    fireEvent.click(await screen.findByRole('button', { name: 'Stop' }));

    expect(
      await screen.findByTestId('settings-plugins-error'),
    ).toHaveTextContent('Could not stop or start the plugin. Try again.');
    expect(screen.getByText('Session active')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Stop' })).toBeEnabled();
  });

  it('links an expired store session to the Stores topic', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([epic]);
    renderPlugins([epic], {
      storeAccounts: [storeAccount('epic', 'expired')],
    });

    const card = await screen.findByRole('article', { name: 'Epic Games Store' });
    expect(card.className).toContain('border-warning');
    const strip = within(card).getByRole('button', {
      name: "This store's session expired. Sign in again in Stores.",
    });
    fireEvent.click(strip);

    expect(uiStore.state.settingsTopic).toBe('stores');
  });

  it('reports a failed inventory load without losing the community note', async () => {
    vi.spyOn(commands, 'pluginInventory').mockRejectedValue('offline');
    renderPlugins([]);

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Plugin information could not be loaded.',
    );
    expect(
      screen.getByText(
        "Community plugins aren't reviewed by GameNode. Only grant permissions that make sense.",
      ),
    ).toBeInTheDocument();
  });

  it('localizes the filter, stopped session and permissions in pt-BR', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([
      { ...community, providerId: 'packagekit', name: 'Gerenciador de pacotes Linux' },
    ]);
    renderPlugins([community], { locale: 'pt-BR' });

    expect(
      await screen.findByRole('radiogroup', { name: 'Filtrar por origem' }),
    ).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Todos · 1' })).toBeInTheDocument();
    expect(screen.getByText('Parada')).toBeInTheDocument();
    expect(screen.getByText('Permissões')).toBeInTheDocument();
    expect(screen.getByText('Arquivos')).toBeInTheDocument();
    expect(screen.getByText('D-Bus')).toBeInTheDocument();
  });
});
