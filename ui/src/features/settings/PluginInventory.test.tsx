import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import {
  commands,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type AvailablePluginUpdate,
  type PluginConfigurationState,
  type PluginInfo,
  type PluginPermissionSnapshot,
  type PluginSharedDataSnapshot,
} from '../../api';
import { cleanup } from '@testing-library/react';
import { selectPluginConfiguration } from '../../api/pluginSettings';
import { I18nProvider } from '../../i18n';
import { PluginInventory } from './PluginInventory';
import { pluginQueryKeys } from '../../api/pluginQueryKeys';

const { listenMock } = vi.hoisted(() => ({ listenMock: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

const plugin: PluginInfo = {
  providerId: 'steam',
  pluginId: 'org.gamenode.plugin.steam-discovery',
  pluginKind: 'provider',
  name: 'Steam',
  source: 'official',
  status: 'installed',
  capabilities: ['library.discovery', 'auth.webview', 'secrets.read'],
  session: 'active',
  nativeAccess: ['files', 'network'],
};

function renderInventory(locale = 'en', profileId?: string, seed?: (client: QueryClient) => void) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (profileId) {
    client.setQueryData(SNAPSHOT_QUERY_KEY, {
      ...createDefaultSnapshot(),
      auth: { signedIn: { user: { id: profileId } } },
    });
  }
  seed?.(client);
  const view = render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={client}>
        <I18nProvider initialLocale={locale}>
          <PluginInventory />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
  return { ...view, client };
}

describe('PluginInventory', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    listenMock.mockReset();
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it('discloses the signed playtime warning in Portuguese and persists the per-plugin setting', async () => {
    const declared = {
      pluginId: 'org.example.emulator',
      name: 'Emulador',
      version: '1.0.0',
      playtime: { identifiesGameplay: true, identifiesPauses: false, withoutPauseDetection: 'application_open' as const },
      accountingEnabled: true,
    };
    const inventory = vi.spyOn(commands, 'pluginPlaytimeInventory')
      .mockResolvedValueOnce([declared])
      .mockResolvedValueOnce([{ ...declared, accountingEnabled: false }]);
    const save = vi.spyOn(commands, 'pluginPlaytimeSetAccounting').mockResolvedValue(undefined);

    renderInventory('pt-BR');

    const card = await screen.findByRole('article', { name: 'Emulador' });
    expect(within(card).getByText('Identifica o conteúdo do jogo: Sim')).toBeInTheDocument();
    expect(within(card).getByRole('note')).toHaveTextContent(
      'Este plugin não consegue distinguir pausas. O tempo com o aplicativo aberto pode ser contabilizado como tempo de jogo.',
    );
    expect(screen.getByText(/Desativar a contabilização preserva o histórico/)).toBeInTheDocument();
    expect(screen.getByText(/não altera permissões de descoberta ou lançamento/)).toBeInTheDocument();

    const accounting = within(card).getByRole('checkbox', { name: 'Permitir que este plugin contabilize tempo de jogo' });
    expect(accounting).toBeChecked();
    fireEvent.click(accounting);
    await waitFor(() => expect(save).toHaveBeenCalledWith('org.example.emulator', false));
    await waitFor(() => expect(accounting).not.toBeChecked());
    expect(inventory).toHaveBeenCalledTimes(2);
  });

  it('loads a fresh data review when the publisher changes and binds the save to that publisher', async () => {
    const oldFingerprint = `SHA256:${'a'.repeat(64)}`;
    const newFingerprint = `SHA256:${'b'.repeat(64)}`;
    const managed: PluginInfo = {
      ...plugin, source: 'community', publisherFingerprint: oldFingerprint,
    };
    const review = {
      pluginId: managed.pluginId, profileId: 'a', policyRevision: 0,
      publisherFingerprint: oldFingerprint,
      commonCategories: ['gamePaths' as const], selectedRecordIds: [], catalog: [], sourcePolicies: [],
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([managed]);
    vi.spyOn(commands, 'pluginSharedDataSnapshot')
      .mockResolvedValueOnce(review)
      .mockResolvedValue({ ...review, publisherFingerprint: newFingerprint, commonCategories: [] });
    const save = vi.spyOn(commands, 'pluginSharedDataReplace')
      .mockResolvedValue({ ...review, publisherFingerprint: newFingerprint });

    const { client } = renderInventory('en', 'a');
    fireEvent.click(await screen.findByRole('button', { name: 'Review data access' }));
    expect(await screen.findByRole('switch', { name: 'Game paths' })).toBeChecked();
    act(() => {
      client.setQueryData(pluginQueryKeys.inventory, [{ ...managed, publisherFingerprint: newFingerprint }]);
    });
    await waitFor(() => expect(screen.getByRole('switch', { name: 'Game paths' })).not.toBeChecked());
    fireEvent.click(screen.getByRole('switch', { name: 'Game paths' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save data permissions' }));
    await waitFor(() => expect(save).toHaveBeenCalledWith(managed.pluginId, {
      profileId: 'a', expectedPublisherFingerprint: newFingerprint, expectedRevision: 0,
      commonCategories: ['gamePaths'], selectedRecordIds: [], sourcePolicies: [],
    }));
  });

  it('keeps a completed save in the reviewed publisher cache after inventory changes', async () => {
    const oldFingerprint = `SHA256:${'a'.repeat(64)}`;
    const newFingerprint = `SHA256:${'b'.repeat(64)}`;
    const managed: PluginInfo = {
      ...plugin, source: 'community', publisherFingerprint: oldFingerprint,
    };
    const review: PluginSharedDataSnapshot = {
      pluginId: managed.pluginId, profileId: 'a', policyRevision: 0,
      publisherFingerprint: oldFingerprint,
      commonCategories: [], selectedRecordIds: [], catalog: [], sourcePolicies: [],
    };
    const newReview = { ...review, publisherFingerprint: newFingerprint };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([managed]);
    vi.spyOn(commands, 'pluginSharedDataSnapshot')
      .mockResolvedValueOnce(review)
      .mockResolvedValue(newReview);
    let finishSave!: (snapshot: PluginSharedDataSnapshot) => void;
    const pendingSave = new Promise<PluginSharedDataSnapshot>((resolve) => { finishSave = resolve; });
    const save = vi.spyOn(commands, 'pluginSharedDataReplace').mockReturnValue(pendingSave);
    const { client } = renderInventory('en', 'a');

    fireEvent.click(await screen.findByRole('button', { name: 'Review data access' }));
    fireEvent.click(await screen.findByRole('switch', { name: 'Game paths' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save data permissions' }));
    expect(save).toHaveBeenCalledOnce();

    const oldKey = pluginQueryKeys.sharedData(managed.pluginId, 'a', oldFingerprint);
    const newKey = pluginQueryKeys.sharedData(managed.pluginId, 'a', newFingerprint);
    act(() => {
      client.setQueryData(pluginQueryKeys.inventory, [{ ...managed, publisherFingerprint: newFingerprint }]);
    });
    await waitFor(() => expect(client.getQueryData(newKey)).toEqual(newReview));

    const saved: PluginSharedDataSnapshot = { ...review, policyRevision: 1, commonCategories: ['gamePaths'] };
    await act(async () => { finishSave(saved); });
    expect(client.getQueryData(oldKey)).toEqual(saved);
    expect(client.getQueryData(newKey)).toEqual(newReview);
    expect(screen.getByRole('switch', { name: 'Game paths' })).not.toBeChecked();
  });

  it.each([
    ['external-only', { externalDeclared: true, externalAuthorized: true, declarativeDeclared: false, declarativeAuthorized: false }, 'external'],
    ['form-only', { externalDeclared: false, externalAuthorized: false, declarativeDeclared: true, declarativeAuthorized: true }, 'form'],
    ['both', { externalDeclared: true, externalAuthorized: true, declarativeDeclared: true, declarativeAuthorized: true }, 'external'],
    ['denied', { externalDeclared: true, externalAuthorized: false, declarativeDeclared: true, declarativeAuthorized: false }, 'denied'],
    ['absent', { externalDeclared: false, externalAuthorized: false, declarativeDeclared: false, declarativeAuthorized: false }, 'absent'],
  ] as const)('selects authorized configuration mode for %s', (_name, modes, expected) => {
    expect(selectPluginConfiguration(modes)).toBe(expected);
  });

  it('prefers the authorized external window when a form is also available', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin]);
    vi.spyOn(commands, 'pluginConfigurationModes').mockResolvedValue({
      externalDeclared: true, externalAuthorized: true,
      declarativeDeclared: true, declarativeAuthorized: true,
    });
    const describe = vi.spyOn(commands, 'pluginSettingsDescribe');
    renderInventory();
    expect(await screen.findByRole('button', { name: 'Configure' })).toBeInTheDocument();
    expect(describe).not.toHaveBeenCalled();
  });

  it('routes Configure to open, then focus, while plugin settings navigation stays usable', async () => {
    const secondPlugin: PluginInfo = { ...plugin, providerId: 'second-provider', pluginId: 'org.example.second', name: 'Second plugin' };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin, secondPlugin]);
    vi.spyOn(commands, 'pluginConfigurationModes').mockResolvedValue({
      externalDeclared: true, externalAuthorized: true,
      declarativeDeclared: true, declarativeAuthorized: true,
    });
    const configure = vi.spyOn(commands, 'pluginConfigurationOpen')
      .mockResolvedValueOnce({ status: 'open', windowId: 'window-1', message: null })
      .mockResolvedValueOnce({ status: 'focused', windowId: 'window-1', message: null });
    renderInventory();

    fireEvent.click(await screen.findByRole('button', { name: 'Configure' }));
    await waitFor(() => expect(configure).toHaveBeenCalledWith(plugin.providerId, false));
    expect(await screen.findByText('Configuration window opened.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Configure' }));
    await waitFor(() => expect(configure).toHaveBeenLastCalledWith(plugin.providerId, true));
    expect(await screen.findByText('Configuration window focused.')).toBeInTheDocument();

    fireEvent.click(within(await screen.findByRole('group', { name: 'Installed plugins' })).getByRole('button', { name: /Second plugin/ }));
    expect(screen.getByRole('article', { name: 'Plugin details' })).toHaveTextContent('org.example.second');
  });

  it('shows timeout feedback from the external configuration lifecycle', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin]);
    vi.spyOn(commands, 'pluginConfigurationModes').mockResolvedValue({
      externalDeclared: true, externalAuthorized: true,
      declarativeDeclared: false, declarativeAuthorized: false,
    });
    vi.spyOn(commands, 'pluginConfigurationOpen').mockResolvedValue({ status: 'timedOut', windowId: null, message: 'timeout' });
    renderInventory();
    fireEvent.click(await screen.findByRole('button', { name: 'Configure' }));
    expect(await screen.findByText('The configuration request timed out. Try again.')).toBeInTheDocument();
  });

  it('keeps plugin navigation usable while a durable usage read is slow', async () => {
    const fingerprint = `SHA256:${'c'.repeat(64)}`;
    const first: PluginInfo = { ...plugin, publisherFingerprint: fingerprint };
    const second: PluginInfo = {
      ...plugin,
      providerId: 'second-provider',
      pluginId: 'org.example.second',
      name: 'Second plugin',
      publisherFingerprint: fingerprint,
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([first, second]);
    vi.spyOn(commands, 'pluginPermissionSnapshot').mockImplementation(async (pluginId: string) => ({
      pluginId,
      publisherFingerprint: fingerprint,
      profileId: 'a',
      grantRevision: 0,
      requestedPermissions: [],
      effectiveGrants: [],
    }));
    vi.spyOn(commands, 'pluginSharedDataSnapshot').mockImplementation(async (pluginId: string) => ({
      pluginId,
      publisherFingerprint: fingerprint,
      profileId: 'a',
      policyRevision: 0,
      commonCategories: [],
      selectedRecordIds: [],
      catalog: [],
      sourcePolicies: [],
    }));
    vi.spyOn(commands, 'pluginConfigurationState').mockImplementation(async (pluginId: string) => ({
      pluginId, available: false, revision: null,
    }));
    vi.spyOn(commands, 'pluginConfigurationModes').mockResolvedValue({
      externalDeclared: false, externalAuthorized: false,
      declarativeDeclared: false, declarativeAuthorized: false,
    });
    vi.spyOn(commands, 'pluginDurableUsage').mockReturnValue(new Promise(() => undefined));

    renderInventory('en', 'a');
    expect(await screen.findByText('Reading retained shared data…')).toBeInTheDocument();

    fireEvent.click(within(await screen.findByRole('group', { name: 'Installed plugins' })).getByRole('button', { name: /Second plugin/ }));
    expect(screen.getByRole('article', { name: 'Plugin details' })).toHaveTextContent('org.example.second');
    expect(screen.getByText('Reading retained shared data…')).toBeInTheDocument();
  });

  it('reports disabled and unavailable modes without issuing Configure requests', async () => {
    const disabledConfigure = vi.spyOn(commands, 'pluginConfigurationOpen');
    const disabledModes = vi.spyOn(commands, 'pluginConfigurationModes');
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([{ ...plugin, status: 'missing', session: 'stopped' }]);
    renderInventory();
    expect(await screen.findByText(/unavailable while this plugin is disabled or stopped/i)).toBeInTheDocument();
    expect(disabledModes).not.toHaveBeenCalled();
    expect(disabledConfigure).not.toHaveBeenCalled();

    cleanup();
    vi.restoreAllMocks();
    const unavailableConfigure = vi.spyOn(commands, 'pluginConfigurationOpen');
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin]);
    vi.spyOn(commands, 'pluginConfigurationModes').mockRejectedValue(new Error('provider unavailable'));
    renderInventory();
    expect(await screen.findByText(/configuration is unavailable for this plugin right now/i)).toBeInTheDocument();
    expect(unavailableConfigure).not.toHaveBeenCalled();
  });

  it('loads and renders the authorized declarative schema in form-only mode', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin]);
    vi.spyOn(commands, 'pluginConfigurationModes').mockResolvedValue({
      externalDeclared: false, externalAuthorized: false,
      declarativeDeclared: true, declarativeAuthorized: true,
    });
    vi.spyOn(commands, 'pluginSettingsDescribe').mockResolvedValue({
      context: { plugin_id: plugin.pluginId, provider_id: plugin.providerId, session_id: 's1', profile_epoch: 1, form_handle: 'f1' },
      outcome: { type: 'form', form: {
        schema_version: 1, form_id: 'prefs', schema_revision: 'schema-1', configuration_revision: 'config-1',
        title: { fallback: 'Plugin preferences', translations: {} },
        fields: [{ id: 'label', label: { fallback: 'Catalog label', translations: {} }, kind: { type: 'text', min_length: 1, max_length: 32 }, default: { type: 'text', value: 'Games' } }],
      }, values: { label: { type: 'text', value: 'Games' } } },
    });
    renderInventory();
    expect(await screen.findByRole('textbox', { name: 'Catalog label' })).toHaveValue('Games');
    expect(commands.pluginSettingsDescribe).toHaveBeenCalledWith(plugin.providerId);
  });

  it.each([
    [{ externalDeclared: true, externalAuthorized: false, declarativeDeclared: true, declarativeAuthorized: false }, /not authorized/i],
    [{ externalDeclared: false, externalAuthorized: false, declarativeDeclared: false, declarativeAuthorized: false }, /does not provide an authorized configuration/i],
  ])('does not request settings for denied or absent configuration', async (modes, message) => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin]);
    vi.spyOn(commands, 'pluginConfigurationModes').mockResolvedValue(modes);
    const describe = vi.spyOn(commands, 'pluginSettingsDescribe');
    renderInventory();
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(describe).not.toHaveBeenCalled();
  });

  it.each([
    ['en', 'Linux Package Manager'],
    ['pt-BR', 'Gerenciador de pacotes Linux'],
  ])('localizes the Portuguese manifest in %s and retains unknown names', async (locale, label) => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([
      { ...plugin, providerId: 'packagekit', name: 'Gerenciador de pacotes Linux' },
      { ...plugin, providerId: 'community', pluginId: 'community.plugin', name: 'Community Games' },
    ]);
    renderInventory(locale);
    expect((await screen.findAllByText(label)).length).toBeGreaterThan(0);
    expect(screen.getByText('Community Games')).toBeInTheDocument();
  });

  it('shows trusted origin, runtime status, actual grants and native access separately', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin]);
    renderInventory();

    expect((await screen.findAllByText('Official')).length).toBeGreaterThan(0);
    expect((await screen.findAllByText('Installed')).length).toBeGreaterThan(0);
    expect(screen.getByText('library.discovery')).toBeInTheDocument();
    expect(screen.getByText('auth.webview')).toBeInTheDocument();
    expect(screen.getByText('secrets.read')).toBeInTheDocument();
    expect(screen.getByText('Files')).toBeInTheDocument();
    expect(screen.getByText('Network')).toBeInTheDocument();
    expect(screen.getByText(/not sandboxed/i)).toBeInTheDocument();
    expect(screen.getByText(/Process session: Active/)).toBeInTheDocument();
    expect(JSON.stringify(plugin)).not.toMatch(/token|cookie|password|credentialValue/i);
    expect(screen.queryByRole('button', { name: 'Enable' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Remove' })).not.toBeInTheDocument();
  });

  it('keeps community inventory read-only while signed out', async () => {
    const managed: PluginInfo = {
      ...plugin, source: 'community', pluginId: 'org.example.community',
      publisherFingerprint: `SHA256:${'c'.repeat(64)}`, runtimeAvailable: true,
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([managed]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    renderInventory();
    expect(await screen.findByText('Sign in to manage community plugins.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Enable' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Remove' })).not.toBeInTheDocument();
  });

  it.each([
    ['en', 'This extension is installed but its runtime could not be registered. Review the package compatibility or reinstall it.'],
    ['pt-BR', 'Esta extensão está instalada, mas o runtime dela não pôde ser registrado. Verifique a compatibilidade do pacote ou reinstale-o.'],
  ])('explains unavailable extension runtime in %s and keeps removal available', async (locale, explanation) => {
    const extension: PluginInfo = {
      ...plugin,
      providerId: 'org.example.extension',
      pluginId: 'org.example.extension',
      pluginKind: 'extension',
      source: 'community',
      session: 'stopped',
      enabled: false,
      runtimeAvailable: false,
      publisherFingerprint: `SHA256:${'e'.repeat(64)}`,
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([extension]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    renderInventory(locale, 'user-1');

    expect(await screen.findByText(explanation)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Enable' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: locale === 'pt-BR' ? 'Remover' : 'Remove' })).toBeInTheDocument();
  });

  it('lets the user select a plugin and inspect its identity and permissions', async () => {
    const secondPlugin: PluginInfo = {
      ...plugin,
      providerId: 'packagekit',
      pluginId: 'org.gamenode.plugin.packagekit-discovery',
      name: 'Gerenciador de pacotes Linux',
      source: 'official',
      capabilities: ['provider.context.read', 'library.discovery'],
      nativeAccess: ['files', 'process', 'dbus'],
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin, secondPlugin]);
    renderInventory();

    const list = await screen.findByRole('group', { name: 'Installed plugins' });
    const packagePluginOption = within(list).getByRole('button', { name: /Linux Package Manager/ });
    fireEvent.click(packagePluginOption);

    expect(packagePluginOption).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('article', { name: 'Plugin details' })).toHaveTextContent('org.gamenode.plugin.packagekit-discovery');
    expect(screen.getByRole('article', { name: 'Plugin details' })).toHaveTextContent('library.discovery');
    expect(screen.getByRole('article', { name: 'Plugin details' })).toHaveTextContent('provider.context.read');
    expect(screen.getByRole('article', { name: 'Plugin details' })).toHaveTextContent('Local process execution');
    expect(screen.getByRole('article', { name: 'Plugin details' })).toHaveTextContent('System D-Bus');
    expect(screen.getByRole('article', { name: 'Plugin details' })).toHaveTextContent('Official');
    expect(screen.getByRole('article', { name: 'Plugin details' })).toHaveTextContent('Process session: Active');
    expect(packagePluginOption).toHaveTextContent('Installed');
    expect(screen.getByRole('article', { name: 'Plugin details' })).not.toHaveTextContent('auth.webview');
  });

  it('shows missing executable state and Portuguese labels', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([{ ...plugin, status: 'missing', session: 'stopped' }]);
    renderInventory('pt-BR');

    expect((await screen.findAllByText('Oficial')).length).toBeGreaterThan(0);
    expect((await screen.findAllByText('Executável ausente')).length).toBeGreaterThan(0);
    expect(screen.getByText('Permissões de plugin concedidas')).toBeInTheDocument();
    expect(screen.getByText(/Sessão do processo: Parada/)).toBeInTheDocument();
  });

  it('renders native package manager access labels in Portuguese', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([{
      ...plugin,
      providerId: 'packagekit',
      pluginId: 'org.gamenode.plugin.packagekit-discovery',
      name: 'Gerenciador de pacotes Linux',
      capabilities: ['provider.context.read', 'library.discovery'],
      nativeAccess: ['files', 'process', 'dbus'],
    }]);
    renderInventory('pt-BR');

    expect(await screen.findByText('Execução de processos locais')).toBeInTheDocument();
    expect(screen.getByText('D-Bus do sistema')).toBeInTheDocument();
  });

  it.each([false, true])('reviews explicit updates and reports the active version (activation fails: %s)', async (fails) => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([{ ...plugin, version: '1.0.0' }]);
    const setPeriodic = vi.spyOn(commands, 'pluginUpdateSetPeriodicChecks').mockResolvedValue({ periodicChecks: true });
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateCheck').mockResolvedValue([{
      pluginId: plugin.pluginId, name: plugin.name, installedVersion: '1.0.0', versions: ['2.0.0'],
    }]);
    vi.spyOn(commands, 'pluginUpdateReview').mockResolvedValue({
      pluginId: plugin.pluginId, installedVersion: '1.0.0', candidateVersion: '2.0.0',
      addedPermissions: ['network.fetch'], removedPermissions: [], changedPermissions: ['files.read'],
      permissionChanges: [{ id: 'files.read', installedScope: 'selected-game', candidateScope: 'all-games', installedRequired: false, candidateRequired: true }],
      addedNativeAccess: ['process'], removedNativeAccess: [], apiChanged: true,
      installedApi: '1.0.0 – 1.4.0', candidateApi: '1.0.0 – 2.0.0',
      licenseChanged: true, installedLicense: 'MIT', candidateLicense: 'GPL-3.0-only',
      installedLicenseFiles: ['LICENSE'], candidateLicenseFiles: ['LICENSE', 'NOTICE'],
      installedLicenseNotices: [], candidateLicenseNotices: ['Third-party notice'],
      installedPlaytime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' },
      candidatePlaytime: { identifiesGameplay: true, identifiesPauses: false, withoutPauseDetection: 'application_open' },
      playtimeChanged: true,
      publisherChanged: false, installedPublisherKeyId: 'publisher-1', candidatePublisherKeyId: 'publisher-1',
      publisherFingerprint: '', changedDataSchemas: [], preservesPriorData: true, isDowngrade: false,
      requiresManualApproval: true,
    });
    let finish!: () => void;
    const approve = vi.spyOn(commands, 'pluginUpdateApprove').mockReturnValue(new Promise((resolve, reject) => {
      finish = () => fails ? reject(new Error('candidate did not become ready')) : resolve('2.0.0');
    }));
    renderInventory();

    const periodicCheckbox = await screen.findByRole('checkbox', { name: /Check periodically/ });
    await waitFor(() => expect(periodicCheckbox).toBeEnabled());
    fireEvent.click(periodicCheckbox);
    await waitFor(() => expect(setPeriodic).toHaveBeenCalledWith(true));
    fireEvent.click(screen.getByRole('button', { name: 'Check for updates now' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Review version 2.0.0' }));
    expect(await screen.findByRole('article', { name: 'Review plugin update' })).toHaveTextContent('selected-game → all-games');
    expect(screen.getByText(/Signed playtime policy change/)).toBeInTheDocument();
    expect(screen.getByText(/New permissions and wider scopes stay denied/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Approve update' }));
    await waitFor(() => expect(approve).toHaveBeenCalledWith(plugin.pluginId, '2.0.0'));
    expect(screen.getByRole('button', { name: 'Approve update' })).toBeDisabled();
    expect(periodicCheckbox).toBeEnabled();
    finish();
    if (fails) {
      expect(await screen.findByText('Active version: 1.0.0')).toBeInTheDocument();
      expect(screen.getByText('candidate did not become ready')).toBeInTheDocument();
    } else {
      expect(await screen.findByText('Update complete. Active version: 2.0.0')).toBeInTheDocument();
    }
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Approve update' })).not.toBeInTheDocument());
  });

  it.each([
    { rollback: false, version: '2.0.0', reviewButton: 'Review version 2.0.0', approval: 'Approve update', locale: 'en', chooseLabel: 'Choose a game for the readiness check', targetLabel: 'Target game' },
    { rollback: true, version: '1.0.0', reviewButton: 'Revisar rollback para 1.0.0', approval: 'Confirmar rollback', locale: 'pt-BR', chooseLabel: 'Escolher um jogo para a verificação', targetLabel: 'Jogo alvo' },
  ])('selects an explicit target and can cancel extension approval (rollback=$rollback)', async ({ rollback, version, reviewButton, approval, locale, chooseLabel, targetLabel }) => {
    const pluginId = 'org.example.extension';
    const extension = { ...plugin, pluginId, providerId: '', pluginKind: 'extension' as const, session: 'stopped' as const, enabled: true };
    const target = { installationId: 'installation-42', revision: '7', profileEpoch: '3' };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([extension]);
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue(rollback ? [] : [{
      pluginId, name: 'Test extension', installedVersion: '1.0.0', versions: [version],
    }]);
    vi.spyOn(commands, 'pluginRollbackAvailable').mockResolvedValue(rollback ? [{
      pluginId, name: 'Test extension', activeVersion: '2.0.0', versions: [version],
    }] : []);
    const readReview = rollback ? commands.pluginRollbackReview : commands.pluginUpdateReview;
    const review = await readReview(pluginId, version);
    vi.spyOn(commands, rollback ? 'pluginRollbackReview' : 'pluginUpdateReview').mockResolvedValue({
      ...review, extensionTargets: [{ name: 'Example Game', providerId: 'steam', selection: target }],
    });
    const approveUpdate = vi.spyOn(commands, 'pluginUpdateApprove').mockResolvedValue(version);
    const approveRollback = vi.spyOn(commands, 'pluginRollbackApprove').mockResolvedValue(version);
    const decline = vi.spyOn(commands, 'pluginUpdateDecline').mockResolvedValue(undefined);
    renderInventory(locale, 'signed-in');

    async function openTargetPicker() {
      fireEvent.click(await screen.findByRole('button', { name: reviewButton }));
      const choose = await screen.findByRole('button', { name: chooseLabel });
      if (rollback) {
        expect(choose).toBeDisabled();
        fireEvent.click(screen.getByRole('checkbox', { name: /Entendo que isso ativará/ }));
      }
      fireEvent.click(choose);
      return within(await screen.findByRole('dialog', { name: chooseLabel }));
    }

    const firstPicker = await openTargetPicker();
    expect(firstPicker.getByRole('button', { name: approval })).toBeDisabled();
    fireEvent.change(firstPicker.getByRole('combobox', { name: targetLabel }), { target: { value: target.installationId } });
    await userEvent.setup().keyboard('{Escape}');
    await waitFor(() => expect(decline).toHaveBeenCalledWith(pluginId));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(approveUpdate).not.toHaveBeenCalled();
    expect(approveRollback).not.toHaveBeenCalled();

    const secondPicker = await openTargetPicker();
    expect(secondPicker.getByRole('button', { name: approval })).toBeDisabled();
    fireEvent.change(secondPicker.getByRole('combobox', { name: targetLabel }), { target: { value: target.installationId } });
    fireEvent.click(secondPicker.getByRole('button', { name: approval }));
    if (rollback) {
      await waitFor(() => expect(approveRollback).toHaveBeenCalledWith(pluginId, version, true, target));
    } else {
      await waitFor(() => expect(approveUpdate).toHaveBeenCalledWith(pluginId, version, target));
    }
  });

  it('keeps the saved opt-in authoritative when persistence fails', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin]);
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: true });
    vi.spyOn(commands, 'pluginUpdateSetPeriodicChecks').mockRejectedValue(new Error('disk full'));
    renderInventory();

    const periodicCheckbox = await screen.findByRole('checkbox', { name: /Check periodically/ });
    await waitFor(() => expect(periodicCheckbox).toBeChecked());
    fireEvent.click(periodicCheckbox);

    await waitFor(() => {
      expect(periodicCheckbox).toBeChecked();
      expect(screen.getByText(/Could not check or review this update/)).toBeInTheDocument();
    });
  });

  it('reviews a staged package, requires exact publisher trust, and installs it enabled', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    const staged = {
      token: 'review-token', pluginId: 'org.example.community', name: 'Community', version: '1.0.0',
      publisherKeyId: 'key-1', publisherFingerprint: 'SHA256:abc', trust: 'untrusted' as const,
      license: 'MIT', licenseFiles: ['LICENSE'], licenseNotices: [], permissionIds: ['library.read'],
      requestedPermissions: [{
        id: 'library.read', required: false, scope: 'selected-game',
        reason: 'Read selected game data',
        impactIfDisabled: 'Selected game details are not shown.',
      }],
      supportedPlatforms: ['gamecube', 'wii'],
      nativeAccess: [],
      playtime: { identifiesGameplay: true, identifiesPauses: false, withoutPauseDetection: 'application_open' as const },
      compatibility: 'compatible' as const,
    };
    const importReview = vi.spyOn(commands, 'pluginImportReview').mockResolvedValue(staged);
    const confirm = vi.spyOn(commands, 'pluginImportConfirmPublisher').mockResolvedValue({ ...staged, trust: 'trusted' });
    const viewLicense = vi.spyOn(commands, 'pluginImportLicenseText').mockResolvedValue('MIT license text available offline');
    const install = vi.spyOn(commands, 'pluginImportInstall').mockResolvedValue({ pluginId: staged.pluginId, activated: true });
    renderInventory('en', 'account-import');

    fireEvent.change(await screen.findByRole('textbox', { name: 'Path to signed bundle' }), { target: { value: '/packages/community.zip' } });
    await userEvent.setup().type(screen.getByRole('textbox', { name: 'Path to signed bundle' }), '{Enter}');
    expect(await screen.findByRole('article', { name: 'Review plugin package' })).toHaveTextContent('SHA256:abc');
    const packageReview = screen.getByRole('article', { name: 'Review plugin package' });
    expect(packageReview).toHaveTextContent('Platforms: GameCube, Wii');
    expect(packageReview).toHaveTextContent('Read library');
    expect(packageReview).toHaveTextContent('Selected game');
    expect(packageReview).toHaveTextContent('Optional');
    expect(packageReview).toHaveTextContent('Read selected game data');

    // Optional permissions default on; turning one off explains what stops working.
    const toggle = within(packageReview).getByRole('switch', { name: 'Allow Read library' });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(toggle).not.toBeChecked();
    expect(within(packageReview).getByText('Selected game details are not shown.')).toBeInTheDocument();

    fireEvent.click(within(packageReview).getByText('Technical details'));
    expect(packageReview).toHaveTextContent('Playtime behavior declared by this signed package');
    expect(packageReview).toHaveTextContent('Application-open time');
    expect(within(packageReview).getByText(/This plugin cannot distinguish pauses/)).toBeInTheDocument();
    fireEvent.click(screen.getByText('LICENSE'));
    fireEvent.click(screen.getByRole('button', { name: 'View this license or notice offline' }));
    expect(await screen.findByText('MIT license text available offline')).toBeInTheDocument();
    expect(viewLicense).toHaveBeenCalledWith('review-token', 'LICENSE');

    const accept = within(packageReview).getByRole('button', { name: 'Accept, install and enable' });
    expect(accept).toBeDisabled();
    expect(accept).toHaveClass('w-full');
    fireEvent.click(within(packageReview).getByRole('button', { name: 'Trust this publisher' }));
    await waitFor(() => expect(confirm).toHaveBeenCalledWith('review-token', 'SHA256:abc'));
    await waitFor(() => expect(accept).toBeEnabled());
    fireEvent.click(accept);
    await waitFor(() => expect(install).toHaveBeenCalledWith('review-token', false, ['library.read']));
    expect(importReview).toHaveBeenCalledWith('/packages/community.zip');
  });

  it('locks required permissions and omits an empty platform line', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    const staged = {
      token: 'required-review', pluginId: 'org.example.required', name: 'Required', version: '1.0.0',
      publisherKeyId: 'key-required', publisherFingerprint: 'SHA256:required', trust: 'trusted' as const,
      license: 'MIT', licenseFiles: [], licenseNotices: [], permissionIds: ['provider.context.read'],
      requestedPermissions: [{
        id: 'provider.context.read', required: true, scope: 'own-provider',
        reason: 'Read the host-issued provider context.',
      }],
      nativeAccess: [],
      playtime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' as const },
      compatibility: 'compatible' as const,
    };
    vi.spyOn(commands, 'pluginImportReview').mockResolvedValue(staged);
    const install = vi.spyOn(commands, 'pluginImportInstall').mockResolvedValue({ pluginId: staged.pluginId, activated: true });
    renderInventory('en', 'account-required');

    fireEvent.change(await screen.findByRole('textbox', { name: 'Path to signed bundle' }), { target: { value: '/packages/required.gnplugin' } });
    await userEvent.setup().type(screen.getByRole('textbox', { name: 'Path to signed bundle' }), '{Enter}');
    const review = await screen.findByRole('article', { name: 'Review plugin package' });
    expect(review).toHaveTextContent('Provider context');
    expect(review).toHaveTextContent('Required by the package and cannot be turned off.');
    expect(within(review).queryByRole('switch')).not.toBeInTheDocument();
    expect(review).not.toHaveTextContent('Platforms');

    const accept = within(review).getByRole('button', { name: 'Accept, install and enable' });
    expect(accept).toBeEnabled();
    fireEvent.click(accept);
    await waitFor(() => expect(install).toHaveBeenCalledWith('required-review', false, []));
  });

  it('blocks installing a package whose only target mismatches the libc family', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    const target = {
      os: 'linux', arch: 'x86_64', abi: 'gnu', entrypoint: 'bin/plugin',
      requires: { linux: { libc: { family: 'glibc' as const, min_version: '2.31' } } }, tested_on: [],
    };
    vi.spyOn(commands, 'pluginImportReview').mockResolvedValue({
      token: 'musl-review', pluginId: 'org.example.musl', name: 'Musl', version: '1.0.0',
      publisherKeyId: 'key-musl', publisherFingerprint: 'SHA256:musl', trust: 'trusted',
      license: 'MIT', licenseFiles: [], licenseNotices: [], permissionIds: [], requestedPermissions: [],
      nativeAccess: [], playtime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' },
      compatibility: 'incompatible', compatibilityReason: 'no compatible target',
      compatibilityDetails: {
        state: 'incompatible', ambiguous: false, os: 'alpine', release: '3.19', architecture: 'x86_64', abi: 'musl',
        libc: { family: 'musl', version: '1.2.4' }, emulation: 'native',
        targets: [{ target, verdict: { state: 'incompatible', reasons: ['linuxLibcMismatch'], unknownFacts: [] } }],
      },
    });
    renderInventory('en', 'account-musl');
    fireEvent.change(await screen.findByRole('textbox', { name: 'Path to signed bundle' }), { target: { value: '/packages/musl.zip' } });
    await userEvent.setup().type(screen.getByRole('textbox', { name: 'Path to signed bundle' }), '{Enter}');
    const review = await screen.findByRole('article', { name: 'Review plugin package' });
    expect(within(review).getByRole('region', { name: 'Compatibility with this system' })).toHaveTextContent('The C library family does not match');
    expect(screen.getByRole('button', { name: 'Accept, install and enable' })).toBeDisabled();
  });

  it('requires separate explicit consent for declared native access', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    const staged = {
      token: 'native-review', pluginId: 'org.example.native', name: 'Native', version: '1.0.0',
      publisherKeyId: 'key-native', publisherFingerprint: 'SHA256:native', trust: 'trusted' as const,
      license: 'MIT', licenseFiles: [], licenseNotices: [], permissionIds: [], requestedPermissions: [],
      nativeAccess: ['process'], playtime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' as const },
      compatibility: 'compatible' as const,
    };
    const install = vi.spyOn(commands, 'pluginImportInstall').mockResolvedValue({ pluginId: staged.pluginId, activated: true });
    vi.spyOn(commands, 'pluginImportReview').mockResolvedValue(staged);
    renderInventory('en', 'account-import');

    fireEvent.change(await screen.findByRole('textbox', { name: 'Path to signed bundle' }), { target: { value: '/packages/native.zip' } });
    fireEvent.click(screen.getByRole('button', { name: 'Review package' }));
    const installButton = await screen.findByRole('button', { name: 'Accept, install and enable' });
    expect(installButton).toBeDisabled();
    fireEvent.click(screen.getByRole('checkbox', { name: /reviewed the declared native access/i }));
    expect(installButton).toBeEnabled();
    fireEvent.click(installButton);
    await waitFor(() => expect(install).toHaveBeenCalledWith('native-review', true, []));
  });

  it('shows the old fingerprint and requires explicit re-trust when a publisher key changes', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    const changed = {
      token: 'changed-token', pluginId: 'org.example.changed', name: 'Changed', version: '2.0.0',
      publisherKeyId: 'key-2', publisherFingerprint: 'SHA256:new', pinnedFingerprint: 'SHA256:old',
      trust: 'changed' as const, license: 'MIT', licenseFiles: [], licenseNotices: [],
      requestedPermissions: [], playtime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' as const },
      permissionIds: [], nativeAccess: [], compatibility: 'compatible' as const,
    };
    vi.spyOn(commands, 'pluginImportReview').mockResolvedValue(changed);
    const retrust = vi.spyOn(commands, 'pluginImportConfirmPublisher').mockResolvedValue({ ...changed, trust: 'trusted', pinnedFingerprint: undefined });
    renderInventory('en', 'account-import');
    fireEvent.change(await screen.findByRole('textbox', { name: 'Path to signed bundle' }), { target: { value: '/packages/changed.zip' } });
    await userEvent.setup().type(screen.getByRole('textbox', { name: 'Path to signed bundle' }), '{Enter}');

    expect(await screen.findByText('Previously trusted fingerprint: SHA256:old')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Re-trust this changed publisher key' }));
    await waitFor(() => expect(retrust).toHaveBeenCalledWith('changed-token', 'SHA256:new'));
  });

  it('saves only selected network origins for the reviewed account, publisher and revision', async () => {
    const managed: PluginInfo = {
      ...plugin, pluginId: 'org.example.community', source: 'community',
      publisherFingerprint: `SHA256:${'a'.repeat(64)}`, runtimeAvailable: true, enabled: false,
    };
    const permissionSnapshot: PluginPermissionSnapshot = {
      pluginId: managed.pluginId, publisherFingerprint: managed.publisherFingerprint!,
      profileId: 'host-profile-one', grantRevision: 12,
      requestedPermissions: [{
        permission_id: 'network.fetch', required: false,
        allowed_scopes: [
          { scope: 'declared', value: 'https://api.example.com' },
          { scope: 'declared', value: 'https://cdn.example.com:8443' },
        ],
      }],
      effectiveGrants: [],
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([managed]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginPermissionSnapshot').mockResolvedValue(permissionSnapshot);
    const replace = vi.spyOn(commands, 'pluginPermissionsReplace').mockResolvedValue({ ...permissionSnapshot, grantRevision: 13 });
    renderInventory('en', 'account-one');

    expect(await screen.findByText(/Requested scopes come from the verified package manifest/)).toBeInTheDocument();
    const apiOrigin = await screen.findByRole('checkbox', { name: /api\.example\.com.*Denied/ });
    const cdnOrigin = screen.getByRole('checkbox', { name: /cdn\.example\.com:8443.*Denied/ });
    expect(apiOrigin).not.toBeChecked();
    fireEvent.click(apiOrigin);
    expect(cdnOrigin).not.toBeChecked();
    fireEvent.click(screen.getByRole('button', { name: 'Save permission choices' }));
    await waitFor(() => expect(replace).toHaveBeenCalledWith(
      managed.pluginId,
      'host-profile-one',
      managed.publisherFingerprint,
      12,
      [
        { permission_id: 'network.fetch', scope: { scope: 'declared', value: 'https://api.example.com' }, granted: true },
        { permission_id: 'network.fetch', scope: { scope: 'declared', value: 'https://cdn.example.com:8443' }, granted: false },
      ],
    ));
  });

  it('keeps a completed network permission save in the reviewed publisher cache', async () => {
    const oldFingerprint = `SHA256:${'a'.repeat(64)}`;
    const newFingerprint = `SHA256:${'b'.repeat(64)}`;
    const managed: PluginInfo = {
      ...plugin, source: 'community', publisherFingerprint: oldFingerprint, runtimeAvailable: true,
    };
    const scope = { scope: 'declared', value: 'https://api.example.com' } as const;
    const review: PluginPermissionSnapshot = {
      pluginId: managed.pluginId, profileId: 'a', publisherFingerprint: oldFingerprint, grantRevision: 0,
      requestedPermissions: [{ permission_id: 'network.fetch', required: false, allowed_scopes: [scope] }],
      effectiveGrants: [],
    };
    const newReview = { ...review, publisherFingerprint: newFingerprint };
    const newInventory = [{ ...managed, publisherFingerprint: newFingerprint }];
    vi.spyOn(commands, 'pluginInventory').mockResolvedValueOnce([managed]).mockResolvedValue(newInventory);
    vi.spyOn(commands, 'pluginPermissionSnapshot').mockResolvedValueOnce(review).mockResolvedValue(newReview);
    let finishSave!: (snapshot: PluginPermissionSnapshot) => void;
    const pendingSave = new Promise<PluginPermissionSnapshot>((resolve) => { finishSave = resolve; });
    const replace = vi.spyOn(commands, 'pluginPermissionsReplace').mockReturnValue(pendingSave);
    const { client } = renderInventory('en', 'a');

    fireEvent.click(await screen.findByRole('checkbox', { name: /api\.example\.com.*Denied/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Save permission choices' }));
    expect(replace).toHaveBeenCalledOnce();
    const oldKey = pluginQueryKeys.permissions(managed.pluginId, 'a', oldFingerprint);
    const newKey = pluginQueryKeys.permissions(managed.pluginId, 'a', newFingerprint);
    act(() => { client.setQueryData(pluginQueryKeys.inventory, newInventory); });
    await waitFor(() => expect(client.getQueryData(newKey)).toEqual(newReview));

    const saved: PluginPermissionSnapshot = {
      ...review, grantRevision: 1,
      effectiveGrants: [{ permission_id: 'network.fetch', required: false, scope, state: 'granted' }],
    };
    await act(async () => { finishSave(saved); });
    expect(client.getQueryData(oldKey)).toEqual(saved);
    expect(client.getQueryData(newKey)).toEqual(newReview);
    expect(screen.getByRole('checkbox', { name: /api\.example\.com.*Denied/ })).not.toBeChecked();
  });

  it('uses the shared removal modal with Escape and invokes stop-before-remove lifecycle command', async () => {
    const managed: PluginInfo = {
      ...plugin, source: 'community', pluginId: 'org.example.community',
      publisherFingerprint: `SHA256:${'d'.repeat(64)}`, runtimeAvailable: true, enabled: false,
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([managed]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginPermissionSnapshot').mockResolvedValue({
      pluginId: managed.pluginId, publisherFingerprint: managed.publisherFingerprint!,
      profileId: 'account-one', grantRevision: 1, requestedPermissions: [], effectiveGrants: [],
    });
    const remove = vi.spyOn(commands, 'pluginRemove').mockResolvedValue(undefined);
    renderInventory('en', 'account-one');

    const trigger = await screen.findByRole('button', { name: 'Remove' });
    fireEvent.click(trigger);
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    await userEvent.setup().keyboard('{Escape}');
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    fireEvent.click(trigger);
    expect(within(await screen.findByRole('dialog'))
      .getByRole('radio', { name: 'Keep private data for the signed-in account (recommended)' })
    ).toBeChecked();
    fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(remove).toHaveBeenCalledWith(managed.pluginId, false));
  });

  it('applies the single-plugin Keep/Delete choice to the private data', async () => {
    const managed: PluginInfo = {
      ...plugin, source: 'community', pluginId: 'org.example.community',
      publisherFingerprint: `SHA256:${'d'.repeat(64)}`, runtimeAvailable: true, enabled: false,
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([managed]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginPermissionSnapshot').mockResolvedValue({
      pluginId: managed.pluginId, publisherFingerprint: managed.publisherFingerprint!,
      profileId: 'account-one', grantRevision: 1, requestedPermissions: [], effectiveGrants: [],
    });
    const remove = vi.spyOn(commands, 'pluginRemove').mockResolvedValue(undefined);
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('radio', {
      name: 'Delete private data for the signed-in account',
    }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Remove' }));

    await waitFor(() =>
      expect(remove).toHaveBeenCalledWith('org.example.community', true));
  });

  it('caches host permission revisions and configuration metadata without plugin preferences', async () => {
    const managed: PluginInfo = {
      ...plugin,
      pluginId: 'org.example.community',
      source: 'community',
      publisherFingerprint: `SHA256:${'a'.repeat(64)}`,
    };
    const permissions: PluginPermissionSnapshot = {
      pluginId: managed.pluginId,
      publisherFingerprint: managed.publisherFingerprint!,
      profileId: 'account-one',
      grantRevision: 7,
      requestedPermissions: [],
      effectiveGrants: [],
    };
    const configuration: PluginConfigurationState = {
      pluginId: managed.pluginId,
      available: true,
      revision: '1.2.0:external=true:declarative=false',
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([managed]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginPermissionSnapshot').mockResolvedValue(permissions);
    vi.spyOn(commands, 'pluginConfigurationState').mockResolvedValue(configuration);

    const { client } = renderInventory('en', 'account-one');
    await waitFor(() => {
      expect(commands.pluginPermissionSnapshot).toHaveBeenCalledWith(managed.pluginId, 'account-one');
      expect(commands.pluginConfigurationState).toHaveBeenCalledWith(managed.pluginId);
    });
    expect(client.getQueryData(pluginQueryKeys.permissions(managed.pluginId, 'account-one', managed.publisherFingerprint)))
      .toEqual(permissions);
    expect(client.getQueryData(pluginQueryKeys.configuration(managed.pluginId)))
      .toEqual(configuration);
  });

  it('retains the last host permission and configuration snapshots after read errors', async () => {
    const managed: PluginInfo = {
      ...plugin,
      pluginId: 'org.example.community',
      source: 'community',
      publisherFingerprint: `SHA256:${'b'.repeat(64)}`,
    };
    const savedPermissions: PluginPermissionSnapshot = {
      pluginId: managed.pluginId,
      publisherFingerprint: managed.publisherFingerprint!,
      profileId: 'account-two',
      grantRevision: 11,
      requestedPermissions: [],
      effectiveGrants: [],
    };
    const savedConfiguration: PluginConfigurationState = {
      pluginId: managed.pluginId,
      available: true,
      revision: '2.0.0:external=false:declarative=true',
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([managed]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginPermissionSnapshot').mockRejectedValue(new Error('offline'));
    vi.spyOn(commands, 'pluginConfigurationState').mockRejectedValue(new Error('offline'));

    const { client } = renderInventory('en', 'account-two', (queryClient) => {
      queryClient.setQueryData(
        pluginQueryKeys.permissions(managed.pluginId, 'account-two', managed.publisherFingerprint),
        savedPermissions,
      );
      queryClient.setQueryData(pluginQueryKeys.configuration(managed.pluginId), savedConfiguration);
    });
    await waitFor(() => {
      expect(commands.pluginPermissionSnapshot).toHaveBeenCalledWith(managed.pluginId, 'account-two');
      expect(commands.pluginConfigurationState).toHaveBeenCalledWith(managed.pluginId);
    });
    expect(client.getQueryData(pluginQueryKeys.permissions(managed.pluginId, 'account-two', managed.publisherFingerprint)))
      .toEqual(savedPermissions);
    expect(client.getQueryData(pluginQueryKeys.configuration(managed.pluginId)))
      .toEqual(savedConfiguration);
  });

  it('does not let a stale check response overwrite a newer host update event', async () => {
    let publishUpdate: ((event: { payload: AvailablePluginUpdate[] }) => void) | undefined;
    listenMock.mockImplementation(async (event: string, callback: (event: { payload: AvailablePluginUpdate[] }) => void) => {
      if (event === 'plugin:update-available') publishUpdate = callback;
      return () => { publishUpdate = undefined; };
    });
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true });

    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    let finishCheck!: (updates: AvailablePluginUpdate[]) => void;
    vi.spyOn(commands, 'pluginUpdateCheck').mockImplementation(() => new Promise((resolve) => {
      finishCheck = resolve;
    }));
    renderInventory();

    const checkButton = await screen.findByRole('button', { name: 'Check for updates now' });
    await waitFor(() => expect(publishUpdate).toBeDefined());
    fireEvent.click(checkButton);
    await waitFor(() => expect(commands.pluginUpdateCheck).toHaveBeenCalled());

    const newest: AvailablePluginUpdate[] = [{
      pluginId: plugin.pluginId, name: plugin.name, installedVersion: '1.0.0', versions: ['3.0.0'],
    }];
    act(() => publishUpdate?.({ payload: newest }));
    expect(await screen.findByRole('button', { name: 'Review version 3.0.0' })).toBeInTheDocument();

    await act(async () => {
      finishCheck([{ ...newest[0], versions: ['2.0.0'] }]);
    });
    expect(screen.getByRole('button', { name: 'Review version 3.0.0' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Review version 2.0.0' })).not.toBeInTheDocument();
  });

  it('requires separate rollback and exact publisher fingerprint confirmations', async () => {
    const fingerprint = `SHA256:${'a'.repeat(64)}`;
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin]);
    vi.spyOn(commands, 'pluginRollbackAvailable').mockResolvedValue([{
      pluginId: 'org.example.update', name: 'Example update', activeVersion: '2.0.0', versions: ['1.0.0'],
    }]);
    vi.spyOn(commands, 'pluginRollbackReview').mockResolvedValue({
      pluginId: 'org.example.update', installedVersion: '2.0.0', candidateVersion: '1.0.0',
      addedPermissions: [], removedPermissions: ['files.read'], changedPermissions: [], permissionChanges: [],
      addedNativeAccess: [], removedNativeAccess: [], apiChanged: false, installedApi: '1.0.0 – 1.0.0',
      candidateApi: '1.0.0 – 1.0.0', licenseChanged: false, installedLicense: 'MIT', candidateLicense: 'MIT',
      installedLicenseFiles: [], candidateLicenseFiles: [], installedLicenseNotices: [], candidateLicenseNotices: [],
      installedPlaytime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' },
      candidatePlaytime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' },
      playtimeChanged: false,
      publisherChanged: true, installedPublisherKeyId: 'new-key', candidatePublisherKeyId: 'old-key',
      publisherFingerprint: fingerprint, changedDataSchemas: [], preservesPriorData: true, isDowngrade: true,
      requiresManualApproval: true,
    });
    const retrust = vi.spyOn(commands, 'pluginUpdateRetrustPublisher').mockResolvedValue(undefined);
    const approveRollback = vi.spyOn(commands, 'pluginRollbackApprove').mockResolvedValue('1.0.0');
    renderInventory();

    fireEvent.click(await screen.findByRole('button', { name: 'Review rollback to 1.0.0' }));
    await screen.findByRole('article', { name: 'Review plugin rollback' });
    const approve = screen.getByRole('button', { name: 'Confirm rollback' });
    expect(approve).toBeDisabled();
    fireEvent.click(screen.getByRole('checkbox', { name: /I understand this will activate/ }));
    expect(approve).toBeDisabled();
    fireEvent.change(screen.getByRole('textbox', { name: /Type the new publisher fingerprint/ }), {
      target: { value: fingerprint },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Confirm publisher re-trust' }));
    await waitFor(() => expect(retrust).toHaveBeenCalledWith('org.example.update', '1.0.0', fingerprint));
    await waitFor(() => expect(approve).toBeEnabled());
    fireEvent.click(approve);
    await waitFor(() => expect(approveRollback).toHaveBeenCalledWith('org.example.update', '1.0.0', true));
  });
  it('installs, enables immediately, and reports an activation failure without hiding the package', async () => {
    const inventory = vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginPlaytimeInventory').mockResolvedValue([]);
    const staged = {
      token: 'activation-review', pluginId: 'org.example.launch', name: 'Launch', version: '1.0.0',
      publisherKeyId: 'key-launch', publisherFingerprint: 'SHA256:launch', trust: 'trusted' as const,
      license: 'MIT', licenseFiles: [], licenseNotices: [], permissionIds: [],
      requestedPermissions: [], nativeAccess: [],
      playtime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' as const },
      compatibility: 'compatible' as const,
    };
    vi.spyOn(commands, 'pluginImportReview').mockResolvedValue(staged);
    const install = vi.spyOn(commands, 'pluginImportInstall').mockResolvedValue({
      pluginId: staged.pluginId,
      activated: false,
      activationError: 'activation failed: packaged provider executable could not start',
    });
    renderInventory('en', 'account-import');

    fireEvent.change(await screen.findByRole('textbox', { name: 'Path to signed bundle' }), { target: { value: '/packages/launch.zip' } });
    await userEvent.setup().type(screen.getByRole('textbox', { name: 'Path to signed bundle' }), '{Enter}');
    fireEvent.click(await screen.findByRole('button', { name: 'Accept, install and enable' }));

    await waitFor(() => expect(install).toHaveBeenCalledWith('activation-review', false, []));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Installed, but not activated');
    expect(alert).toHaveTextContent('org.example.launch');
    expect(alert).toHaveTextContent('packaged provider executable could not start');
    await waitFor(() => expect(inventory.mock.calls.length).toBeGreaterThan(1));
  });

});
