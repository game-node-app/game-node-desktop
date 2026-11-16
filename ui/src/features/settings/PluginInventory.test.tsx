import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { commands, type PluginInfo } from '../../api';
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
      publisherFingerprint: '', changedDataSchemas: [], preservesPriorData: true, isDowngrade: false,
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
});
