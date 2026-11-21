import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import {
  commands,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type PluginBundleImportReview,
  type PluginGrantScope,
  type PluginInfo,
  type PluginPermissionSnapshot,
} from '../../api';
import { I18nProvider } from '../../i18n';
import { PluginInventory } from './PluginInventory';

const publisher = `SHA256:${'ef'.repeat(32)}`;

const official: PluginInfo = {
  providerId: 'steam',
  pluginId: 'org.gamenode.plugin.steam-discovery',
  pluginKind: 'provider',
  name: 'Steam',
  version: '0.1.0',
  source: 'official',
  status: 'installed',
  capabilities: ['library.discovery'],
  session: 'active',
  nativeAccess: ['files', 'network'],
};

const bundle: PluginInfo = {
  providerId: '',
  pluginId: 'org.example.dolphin.bundle',
  pluginKind: 'bundle',
  name: 'Dolphin GameCube and Wii',
  version: '0.2.0',
  source: 'community',
  status: 'installed',
  capabilities: [],
  session: 'stopped',
  nativeAccess: ['files', 'network', 'process'],
  bundleId: 'org.example.dolphin.bundle',
  bundleName: 'Dolphin GameCube and Wii',
  bundleVersion: '0.2.0',
  bundlePublisherFingerprint: publisher,
  bundleComponents: [
    {
      pluginId: 'org.example.dolphin',
      pluginKind: 'provider',
      name: 'Dolphin discovery',
      version: '0.1.0',
      status: 'installed',
      session: 'stopped',
      enabled: false,
      runtimeAvailable: true,
      publisherFingerprint: publisher,
      nativeAccess: ['files', 'network'],
    },
    {
      pluginId: 'org.example.dolphin.launch',
      pluginKind: 'extension',
      name: 'Dolphin launch',
      version: '0.2.0',
      status: 'unavailable',
      session: 'stopped',
      enabled: false,
      runtimeAvailable: false,
      publisherFingerprint: publisher,
      nativeAccess: ['files', 'process'],
    },
  ],
};

function renderInventory(locale = 'en', profileId?: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (profileId) {
    client.setQueryData(SNAPSHOT_QUERY_KEY, {
      ...createDefaultSnapshot(),
      auth: { signedIn: { user: { id: profileId } } },
    });
  }
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

function permissionSnapshot(
  pluginId: string,
  permissionId: string,
  scope: PluginPermissionSnapshot['requestedPermissions'][number]['allowed_scopes'][number],
  revision = 0,
): PluginPermissionSnapshot {
  return {
    pluginId,
    publisherFingerprint: publisher,
    profileId: 'account-one',
    grantRevision: revision,
    requestedPermissions: [{ permission_id: permissionId, required: false, allowed_scopes: [scope] }],
    effectiveGrants: [],
  };
}

const providerScope: PluginGrantScope = { scope: 'declared', value: 'https://api.example.com' };
const extensionScope: PluginGrantScope = { scope: 'selected-records', category: 'runtime', record_ids: [] };

function bundleReview(trust: PluginBundleImportReview['trust'] = 'untrusted'): PluginBundleImportReview {
  const component = (
    kind: 'provider' | 'extension',
    pluginId: string,
    name: string,
    version: string,
  ): PluginBundleImportReview['components'][number] => ({
    kind,
    availability: kind === 'provider'
      ? { status: 'available' as const }
      : { status: 'unavailable' as const, reason: 'noCompatibleTarget' as const },
    review: {
      pluginId,
      name,
      version,
      publisherKeyId: 'dolphin-dev',
      publisherFingerprint: publisher,
      trust,
      license: 'MIT',
      licenseFiles: ['licenses/LICENSE'],
      licenseNotices: [],
      permissionIds: kind === 'provider' ? ['storage.persist'] : ['game.launch'],
      requestedPermissions: kind === 'provider'
        ? [{ id: 'storage.persist', required: false, scope: 'own-provider', reason: 'Keep the local catalog.' }]
        : [{ id: 'game.launch', required: false, scope: 'approved-game-session', reason: 'Start the selected game.' }],
      nativeAccess: ['files'],
      playtime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' as const },
      compatibility: kind === 'provider' ? 'compatible' as const : 'incompatible' as const,
      compatibilityDetails: {
        state: kind === 'provider' ? 'compatible' as const : 'incompatible' as const,
        ambiguous: false, os: 'linux', release: '24.04', architecture: 'x86_64', abi: 'gnu',
        libc: { family: 'gnu' as const, version: '2.39' }, emulation: 'native' as const,
        targets: [],
      },
    },
  });
  return {
    token: 'bundle-token',
    bundle: {
      id: 'org.example.dolphin.bundle',
      name: 'Dolphin GameCube and Wii',
      version: '0.2.0',
      publisherKeyId: 'dolphin-dev',
      publisherFingerprint: publisher,
    },
    trust,
    license: {
      expressions: ['MIT'],
      components: [
        { pluginId: 'org.example.dolphin', expression: 'MIT', files: ['LICENSE'], notices: [] },
        { pluginId: 'org.example.dolphin.launch', expression: 'MIT', files: ['LICENSE'], notices: [] },
      ],
    },
    components: [
      component('provider', 'org.example.dolphin', 'Dolphin discovery', '0.1.0'),
      component('extension', 'org.example.dolphin.launch', 'Dolphin launch', '0.2.0'),
    ],
  };
}

describe('bundle inventory', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it('renders one row per bundle with every component status and never merges official plugins', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([official, bundle]);
    renderInventory('en', 'account-one');

    const list = await screen.findByRole('group', { name: 'Installed plugins' });
    const rows = within(list).getAllByRole('button');
    expect(rows).toHaveLength(2);
    const bundleRow = within(list).getByRole('button', { name: /Dolphin GameCube and Wii/ });
    expect(bundleRow).toHaveTextContent('Bundle version: 0.2.0');
    expect(bundleRow).toHaveTextContent('Dolphin discovery: Installed');
    expect(bundleRow).toHaveTextContent('Dolphin launch: Unavailable');
    const officialRow = within(list).getByRole('button', { name: /Steam/ });
    expect(officialRow).not.toHaveTextContent('Bundle version');

    // The bundle is one row even though it has two components; the details keep them separate.
    expect(within(list).getAllByRole('button', { name: /Dolphin GameCube and Wii/ })).toHaveLength(1);
    fireEvent.click(bundleRow);
    const details = await screen.findByRole('article', { name: 'Bundle details' });
    expect(details).toHaveTextContent(publisher);
    expect(details).toHaveTextContent('Dolphin discovery');
    expect(details).toHaveTextContent('Dolphin launch');
    expect(details).toHaveTextContent('Runtime unavailable');
    const componentItems = within(details).getByRole('list', { name: 'Bundle components' }).querySelectorAll('li');
    expect(componentItems).toHaveLength(2);
    expect(within(componentItems[0] as HTMLElement).getByRole('button', { name: 'Enable' })).toBeEnabled();
    // The unavailable extension cannot be enabled, and it says why.
    const extensionItem = within(componentItems[1] as HTMLElement);
    expect(extensionItem.getByRole('button', { name: 'Enable' })).toBeDisabled();
  });

  it('keeps activation per component in the bundle details', async () => {
    let enabled = false;
    vi.spyOn(commands, 'pluginInventory').mockImplementation(async () => [{
      ...bundle,
      bundleComponents: bundle.bundleComponents?.map((component) =>
        component.pluginKind === 'provider' ? { ...component, enabled } : component),
    }]);
    const enable = vi.spyOn(commands, 'pluginEnable').mockImplementation(async () => { enabled = true; });
    const disable = vi.spyOn(commands, 'pluginDisable').mockImplementation(async () => { enabled = false; });
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    const details = await screen.findByRole('article', { name: 'Bundle details' });
    const componentItems = within(details).getByRole('list', { name: 'Bundle components' }).querySelectorAll('li');
    fireEvent.click(within(componentItems[0] as HTMLElement).getByRole('button', { name: 'Enable' }));
    await waitFor(() => expect(enable).toHaveBeenCalledWith('org.example.dolphin'));
    const enabledItems = within(details).getByRole('list', { name: 'Bundle components' }).querySelectorAll('li');
    await waitFor(() => expect(within(enabledItems[0] as HTMLElement).getByRole('button', { name: 'Disable' })).toBeInTheDocument());
    fireEvent.click(within(enabledItems[0] as HTMLElement).getByRole('button', { name: 'Disable' }));
    await waitFor(() => expect(disable).toHaveBeenCalledWith('org.example.dolphin'));
  });

  it.each([
    ['en', 'Bundle details', 'Uninstall bundle', 'Sign in to manage community plugins.'],
    ['pt-BR', 'Detalhes do pacote', 'Desinstalar pacote', 'Entre na sua conta para gerenciar plugins da comunidade.'],
  ])('keeps the bundle read-only while signed out in %s', async (locale, detailsLabel, uninstallLabel, signIn) => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    renderInventory(locale);

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    const details = await screen.findByRole('article', { name: detailsLabel });
    expect(details).toHaveTextContent(signIn);
    expect(within(details).queryByRole('button', { name: uninstallLabel })).not.toBeInTheDocument();
    expect(within(details).queryByRole('button', { name: /Ativar|Enable/ })).not.toBeInTheDocument();
  });

  it('uninstalls the bundle once with one retention choice and refreshes the inventory', async () => {
    const inventory = vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    const uninstall = vi.spyOn(commands, 'pluginBundleUninstall').mockResolvedValue({
      bundleId: 'org.example.dolphin.bundle',
      removed: [
        { pluginId: 'org.example.dolphin', version: '0.1.0', stopped: true },
        { pluginId: 'org.example.dolphin.launch', version: '0.2.0', stopped: false },
      ],
      failures: [],
      dataRetention: 'deleted',
    });
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    fireEvent.click(within(await screen.findByRole('article', { name: 'Bundle details' }))
      .getByRole('button', { name: 'Uninstall bundle' }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('radio', {
      name: 'Delete private data for the signed-in account',
    }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Uninstall bundle' }));

    await waitFor(() => expect(uninstall).toHaveBeenCalledWith('org.example.dolphin.bundle', true));
    expect(await screen.findByText(/Bundle uninstalled\. Private data was deleted\./)).toBeInTheDocument();
    await waitFor(() => expect(inventory.mock.calls.length).toBeGreaterThan(1));
  });

  it('names every remaining component when the uninstall is only partial', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    vi.spyOn(commands, 'pluginBundleUninstall').mockResolvedValue({
      bundleId: 'org.example.dolphin.bundle',
      removed: [{ pluginId: 'org.example.dolphin', version: '0.1.0', stopped: true }],
      failures: [{
        pluginId: 'org.example.dolphin.launch',
        version: '0.2.0',
        error: 'remove org.example.dolphin.launch 0.2.0: permission denied',
      }],
      dataRetention: 'kept',
    });
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    const details = await screen.findByRole('article', { name: 'Bundle details' });
    fireEvent.click(within(details).getByRole('button', { name: 'Uninstall bundle' }));
    fireEvent.click(within(await screen.findByRole('dialog'))
      .getByRole('button', { name: 'Uninstall bundle' }));

    expect(await screen.findByText(/only partly uninstalled/i)).toBeInTheDocument();
    expect(screen.getByText(/org\.example\.dolphin\.launch 0\.2\.0: remove .*permission denied/))
      .toBeInTheDocument();
    expect(screen.queryByText('Could not uninstall this bundle. The last saved host state is still in effect.'))
      .not.toBeInTheDocument();
  });

  it('submits grouped permission decisions once and scopes a component error', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    vi.spyOn(commands, 'pluginPermissionSnapshot')
      .mockImplementation(async (pluginId) => pluginId === 'org.example.dolphin'
        ? permissionSnapshot(pluginId, 'storage.persist', providerScope)
        : permissionSnapshot(pluginId, 'game.launch', extensionScope));
    const replace = vi.spyOn(commands, 'pluginBundlePermissionsReplace').mockResolvedValue({
      components: [
        { pluginId: 'org.example.dolphin' },
        { pluginId: 'org.example.dolphin.launch', error: 'grants changed while saving' },
      ],
    });
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    const details = await screen.findByRole('article', { name: 'Bundle details' });
    fireEvent.click(await within(details).findByRole('checkbox', { name: /api\.example\.com/ }));
    fireEvent.click(within(details).getByRole('checkbox', { name: /selected-records/ }));
    fireEvent.click(within(details).getByRole('button', { name: 'Save bundle permissions' }));

    await waitFor(() => expect(replace).toHaveBeenCalledTimes(1));
    const submissions = replace.mock.calls[0][0];
    expect(submissions).toHaveLength(2);
    expect(submissions.map((submission) => submission.pluginId)).toEqual([
      'org.example.dolphin',
      'org.example.dolphin.launch',
    ]);
    expect(submissions.every((submission) => submission.expectedRevision === 0)).toBe(true);
    expect(submissions[0].decisions).toEqual([
      { permission_id: 'storage.persist', scope: providerScope, granted: true },
    ]);
    // The denied/failed component is named beside its own section, not as a bundle-wide failure.
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Dolphin launch: grants changed while saving',
    );
  });

  it('denies one component without touching its sibling decision', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    vi.spyOn(commands, 'pluginPermissionSnapshot')
      .mockImplementation(async (pluginId) => pluginId === 'org.example.dolphin'
        ? permissionSnapshot(pluginId, 'storage.persist', providerScope)
        : permissionSnapshot(pluginId, 'game.launch', extensionScope));
    const replace = vi.spyOn(commands, 'pluginBundlePermissionsReplace').mockResolvedValue({
      components: [{ pluginId: 'org.example.dolphin' }, { pluginId: 'org.example.dolphin.launch' }],
    });
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    const details = await screen.findByRole('article', { name: 'Bundle details' });
    // Only the provider is granted; the extension stays denied on its own scope.
    fireEvent.click(await within(details).findByRole('checkbox', { name: /api\.example\.com/ }));
    fireEvent.click(within(details).getByRole('button', { name: 'Save bundle permissions' }));

    await waitFor(() => expect(replace).toHaveBeenCalledTimes(1));
    const submissions = replace.mock.calls[0][0];
    expect(submissions[0].decisions).toEqual([
      { permission_id: 'storage.persist', scope: providerScope, granted: true },
    ]);
    expect(submissions[1].decisions).toEqual([
      { permission_id: 'game.launch', scope: extensionScope, granted: false },
    ]);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('reviews a bundle with localized labels in pt-BR', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([official]);
    vi.spyOn(commands, 'pluginBundleImportReview').mockResolvedValue(bundleReview());
    renderInventory('pt-BR', 'account-one');

    const path = await screen.findByRole('textbox', { name: 'Caminho do pacote ZIP assinado' });
    fireEvent.change(path, { target: { value: '/pacotes/dolphin.gnbundle' } });
    await userEvent.setup().type(path, '{Enter}');

    const article = await screen.findByRole('article', { name: 'Revisar pacote de plugins' });
    expect(article).toHaveTextContent('Identificador do pacote: org.example.dolphin.bundle');
    expect(article).toHaveTextContent('Permissões solicitadas');
    expect(article).toHaveTextContent('Confiar neste publicador');
  });

  it('reviews a bundle with per-component permissions and compatibility behind one fingerprint', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([official]);
    const review = vi.spyOn(commands, 'pluginBundleImportReview').mockResolvedValue(bundleReview());
    const confirm = vi.spyOn(commands, 'pluginBundleImportConfirmPublisher')
      .mockResolvedValue(bundleReview('trusted'));
    renderInventory('en', 'account-one');

    const path = await screen.findByRole('textbox', { name: 'Path to signed ZIP package' });
    fireEvent.change(path, { target: { value: '/packages/dolphin.gnbundle' } });
    await userEvent.setup().type(path, '{Enter}');

    const article = await screen.findByRole('article', { name: 'Review plugin bundle' });
    expect(review).toHaveBeenCalledWith('/packages/dolphin.gnbundle');
    expect(article).toHaveTextContent('Dolphin GameCube and Wii 0.2.0');
    expect(article).toHaveTextContent(publisher);
    expect(article).toHaveTextContent('storage.persist · own-provider · optional · Keep the local catalog.');
    expect(article).toHaveTextContent('game.launch · approved-game-session · optional · Start the selected game.');
    expect(article).toHaveTextContent('Unavailable on this system');
    expect(article).toHaveTextContent('No package target matches this system');
    expect(article).toHaveTextContent('Incompatible');

    fireEvent.click(within(article).getByRole('button', { name: 'Trust this publisher' }));
    await waitFor(() => expect(confirm).toHaveBeenCalledWith('bundle-token', publisher));
    expect(await screen.findByText(/Bundle publisher confirmed/)).toBeInTheDocument();
  });

  it('keeps the bundle review signed out and disables the publisher decision', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([official]);
    const review = vi.spyOn(commands, 'pluginBundleImportReview').mockResolvedValue(bundleReview());
    renderInventory('en');

    const button = await screen.findByRole('button', { name: 'Review package' });
    expect(button).toBeDisabled();
    expect(screen.getByText('Sign in to manage community plugins.')).toBeInTheDocument();
    expect(review).not.toHaveBeenCalled();
  });
});
