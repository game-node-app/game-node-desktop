import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { cleanup } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { commands, type PluginInfo } from '../../api';
import { selectPluginConfiguration } from '../../api/pluginSettings';
import { I18nProvider } from '../../i18n';
import { PluginInventory } from './PluginInventory';

const plugin: PluginInfo = {
  providerId: 'steam',
  pluginId: 'org.gamenode.plugin.steam-discovery',
  name: 'Steam',
  source: 'official',
  status: 'installed',
  capabilities: ['library.discovery', 'auth.webview', 'secrets.read'],
  session: 'active',
  nativeAccess: ['files', 'network'],
};

function renderInventory(locale = 'en') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={client}>
        <I18nProvider initialLocale={locale}>
          <PluginInventory />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('PluginInventory', () => {
  beforeEach(() => vi.restoreAllMocks());

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

  it('checks updates, presents the signed manifest diff, and requires explicit approval', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([plugin]);
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
      publisherChanged: false, installedPublisherKeyId: 'publisher-1', candidatePublisherKeyId: 'publisher-1',
      requiresManualApproval: true,
    });
    const approve = vi.spyOn(commands, 'pluginUpdateApprove').mockResolvedValue('2.0.0');
    renderInventory();

    fireEvent.click(screen.getByRole('checkbox', { name: /Check periodically/ }));
    await waitFor(() => expect(setPeriodic).toHaveBeenCalledWith(true));
    fireEvent.click(screen.getByRole('button', { name: 'Check for updates now' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Review version 2.0.0' }));
    expect(await screen.findByRole('article', { name: 'Review plugin update' })).toHaveTextContent('selected-game → all-games');
    expect(screen.getByText(/New permissions and wider scopes stay denied/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Approve update' }));
    await waitFor(() => expect(approve).toHaveBeenCalledWith(plugin.pluginId, '2.0.0'));
  });
});
