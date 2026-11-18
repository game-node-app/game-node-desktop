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
    ['en', 'This extension is installed but cannot be started yet. Extension runtime support is not available in this release.'],
    ['pt-BR', 'Esta extensão está instalada, mas ainda não pode ser iniciada. O suporte a runtime de extensões não está disponível nesta versão.'],
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
      installedPlaytime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' },
      candidatePlaytime: { identifiesGameplay: true, identifiesPauses: false, withoutPauseDetection: 'application_open' },
      playtimeChanged: true,
      publisherChanged: false, installedPublisherKeyId: 'publisher-1', candidatePublisherKeyId: 'publisher-1',
      publisherFingerprint: '', changedDataSchemas: [], preservesPriorData: true, isDowngrade: false,
      requiresManualApproval: true,
    });
    const approve = vi.spyOn(commands, 'pluginUpdateApprove').mockResolvedValue('2.0.0');
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

  it('reviews a staged package, requires exact publisher trust, and installs without starting it', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    const staged = {
      token: 'review-token', pluginId: 'org.example.community', name: 'Community', version: '1.0.0',
      publisherKeyId: 'key-1', publisherFingerprint: 'SHA256:abc', trust: 'untrusted' as const,
      license: 'MIT', licenseFiles: ['LICENSE'], licenseNotices: [], permissionIds: ['library.read'],
      requestedPermissions: [{ id: 'library.read', required: false, scope: 'selected-game', reason: 'Read selected game data' }],
      nativeAccess: [], compatibility: 'compatible' as const,
    };
    const importReview = vi.spyOn(commands, 'pluginImportReview').mockResolvedValue(staged);
    const confirm = vi.spyOn(commands, 'pluginImportConfirmPublisher').mockResolvedValue({ ...staged, trust: 'trusted' });
    const install = vi.spyOn(commands, 'pluginImportInstall').mockResolvedValue(staged.pluginId);
    renderInventory('en', 'account-import');

    fireEvent.change(await screen.findByRole('textbox', { name: 'Path to signed ZIP package' }), { target: { value: '/packages/community.zip' } });
    await userEvent.setup().type(screen.getByRole('textbox', { name: 'Path to signed ZIP package' }), '{Enter}');
    expect(await screen.findByRole('article', { name: 'Review plugin package' })).toHaveTextContent('SHA256:abc');
    expect(screen.getByRole('article', { name: 'Review plugin package' })).toHaveTextContent('library.read · selected-game · optional · Read selected game data');
    fireEvent.click(screen.getByRole('button', { name: 'Trust this publisher' }));
    await waitFor(() => expect(confirm).toHaveBeenCalledWith('review-token', 'SHA256:abc'));
    fireEvent.click(screen.getByRole('button', { name: 'Install without starting' }));
    await waitFor(() => expect(install).toHaveBeenCalledWith('review-token'));
    expect(importReview).toHaveBeenCalledWith('/packages/community.zip');
  });

  it('shows the old fingerprint and requires explicit re-trust when a publisher key changes', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    const changed = {
      token: 'changed-token', pluginId: 'org.example.changed', name: 'Changed', version: '2.0.0',
      publisherKeyId: 'key-2', publisherFingerprint: 'SHA256:new', pinnedFingerprint: 'SHA256:old',
      trust: 'changed' as const, license: 'MIT', licenseFiles: [], licenseNotices: [],
      requestedPermissions: [],
      permissionIds: [], nativeAccess: [], compatibility: 'compatible' as const,
    };
    vi.spyOn(commands, 'pluginImportReview').mockResolvedValue(changed);
    const retrust = vi.spyOn(commands, 'pluginImportConfirmPublisher').mockResolvedValue({ ...changed, trust: 'trusted', pinnedFingerprint: undefined });
    renderInventory('en', 'account-import');
    fireEvent.change(await screen.findByRole('textbox', { name: 'Path to signed ZIP package' }), { target: { value: '/packages/changed.zip' } });
    await userEvent.setup().type(screen.getByRole('textbox', { name: 'Path to signed ZIP package' }), '{Enter}');

    expect(await screen.findByText('Previously trusted fingerprint: SHA256:old')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Re-trust this changed publisher key' }));
    await waitFor(() => expect(retrust).toHaveBeenCalledWith('changed-token', 'SHA256:new'));
  });

  it('saves only manifest-declared scopes with the cached account and grant revision', async () => {
    const managed: PluginInfo = {
      ...plugin, pluginId: 'org.example.community', source: 'community',
      publisherFingerprint: `SHA256:${'a'.repeat(64)}`, runtimeAvailable: true, enabled: false,
    };
    const permissionSnapshot: PluginPermissionSnapshot = {
      pluginId: managed.pluginId, publisherFingerprint: managed.publisherFingerprint!,
      profileId: 'account-one', grantRevision: 12,
      requestedPermissions: [{
        permission_id: 'library.root.read', required: false,
        allowed_scopes: [{ scope: 'own-creator', category: 'library-root' }],
      }],
      effectiveGrants: [],
    };
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([managed]);
    vi.spyOn(commands, 'pluginUpdatePreferences').mockResolvedValue({ periodicChecks: false });
    vi.spyOn(commands, 'pluginUpdateAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginPermissionSnapshot').mockResolvedValue(permissionSnapshot);
    const replace = vi.spyOn(commands, 'pluginPermissionsReplace').mockResolvedValue({ ...permissionSnapshot, grantRevision: 13 });
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('checkbox', { name: /own-creator.*library-root/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Save permission choices' }));
    await waitFor(() => expect(replace).toHaveBeenCalledWith(
      managed.pluginId,
      'account-one',
      12,
      [{ permission_id: 'library.root.read', scope: { scope: 'own-creator', category: 'library-root' }, granted: true }],
    ));
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
    fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(remove).toHaveBeenCalledWith(managed.pluginId));
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
      expect(commands.pluginPermissionSnapshot).toHaveBeenCalledWith(managed.pluginId);
      expect(commands.pluginConfigurationState).toHaveBeenCalledWith(managed.pluginId);
    });
    expect(client.getQueryData(pluginQueryKeys.permissions(managed.pluginId, 'account-one')))
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
        pluginQueryKeys.permissions(managed.pluginId, 'account-two'),
        savedPermissions,
      );
      queryClient.setQueryData(pluginQueryKeys.configuration(managed.pluginId), savedConfiguration);
    });
    await waitFor(() => {
      expect(commands.pluginPermissionSnapshot).toHaveBeenCalledWith(managed.pluginId);
      expect(commands.pluginConfigurationState).toHaveBeenCalledWith(managed.pluginId);
    });
    expect(client.getQueryData(pluginQueryKeys.permissions(managed.pluginId, 'account-two')))
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
});
