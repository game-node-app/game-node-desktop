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

const openFile = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...args: unknown[]) => openFile(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));

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
        ? [{
            id: 'storage.persist',
            required: false,
            scope: 'own-provider',
            reason: 'Keep the local catalog.',
            impactIfDisabled: 'The local catalog is rebuilt on every start.',
          }]
        : [{
            id: 'game.launch',
            required: false,
            scope: 'approved-game-session',
            reason: 'Start the selected game.',
            impactIfDisabled: 'Games cannot be launched from GameNode.',
          }],
      supportedPlatforms: ['gamecube', 'wii'],
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

function bundleUpdateReviewFixture(rollback = false) {
  const component = (
    pluginId: string,
    kind: 'provider' | 'extension',
    installedVersion: string,
    candidateVersion: string,
  ) => ({
    pluginId,
    kind,
    installedVersion,
    candidateVersion,
    addedPermissions: rollback ? [] : ['library.write'],
    removedPermissions: [],
    changedPermissions: ['game.launch'],
    permissionChanges: [{
      id: 'game.launch',
      installedScope: 'approved-game-session',
      candidateScope: 'selected-game-session',
      installedRequired: false,
      candidateRequired: false,
    }],
    addedNativeAccess: [],
    removedNativeAccess: [],
    apiChanged: false,
    installedApi: '1.6.0',
    candidateApi: '1.6.0',
    availability: { status: 'available' as const },
    compatibility: 'compatible' as const,
    compatibilityDetails: {
      state: 'compatible' as const,
      ambiguous: false, os: 'linux', release: '24.04', architecture: 'x86_64', abi: 'gnu',
      libc: { family: 'gnu' as const, version: '2.39' }, emulation: 'native' as const, targets: [],
    },
    publisherChanged: false,
    isDowngrade: rollback,
  });
  return {
    bundleId: 'org.example.dolphin.bundle',
    name: 'Dolphin GameCube and Wii',
    installedVersion: rollback ? '0.3.0' : '0.2.0',
    candidateVersion: rollback ? '0.2.0' : '0.3.0',
    bundleSha256: 'ab'.repeat(32),
    publisherFingerprint: publisher,
    components: [
      component(
        'org.example.dolphin',
        'provider',
        rollback ? '0.2.0' : '0.1.0',
        rollback ? '0.1.0' : '0.2.0',
      ),
      component(
        'org.example.dolphin.launch',
        'extension',
        rollback ? '0.3.0' : '0.2.0',
        rollback ? '0.2.0' : '0.3.0',
      ),
    ],
    addedPermissions: rollback ? [] : ['library.write'],
    removedPermissions: [],
    changedPermissions: ['game.launch'],
    requiresManualApproval: true,
    isDowngrade: rollback,
    isRollback: rollback,
  };
}

describe('bundle inventory', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    openFile.mockReset();
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

  it('selects a bundle row from the keyboard', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([official, bundle]);
    renderInventory('en', 'account-one');

    const row = await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ });
    row.focus();
    expect(row).toHaveFocus();
    await userEvent.setup().keyboard('{Enter}');
    expect(await screen.findByRole('article', { name: 'Bundle details' })).toBeInTheDocument();
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

    const path = await screen.findByRole('textbox', { name: 'Caminho do bundle assinado' });
    fireEvent.change(path, { target: { value: '/pacotes/dolphin.gnbundle' } });
    await userEvent.setup().type(path, '{Enter}');

    const article = await screen.findByRole('article', { name: 'Revisar pacote de plugins' });
    expect(article).toHaveTextContent('Identificador do pacote: org.example.dolphin.bundle');
    expect(article).toHaveTextContent('Permissões solicitadas');
    expect(article).toHaveTextContent('Plataformas: GameCube, Wii');
    expect(article).toHaveTextContent('Persistir dados do plugin');
    expect(article).toHaveTextContent('Aceitar, instalar e ativar');
    expect(article).toHaveTextContent('Confiar neste publicador');
  });

  it('opens the native file picker and fills the signed package path', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([official]);
    openFile.mockResolvedValue('/tmp/dolphin-emu-0.1.0-linux-x64.gnbundle');
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};

    renderInventory('en', 'account-one');
    fireEvent.click(await screen.findByRole('button', { name: 'Choose file…' }));

    await waitFor(() => expect(openFile).toHaveBeenCalledWith({
      multiple: false,
      directory: false,
      title: 'Choose a signed plugin bundle',
      filters: [
        { name: 'Signed plugin bundles', extensions: ['gnbundle'] },
        { name: 'Individual plugin packages', extensions: ['gnplugin'] },
        { name: 'Signed plugin bundles and individual packages', extensions: ['gnbundle', 'gnplugin'] },
      ],
    }));
    const input = screen.getByRole('textbox', { name: 'Path to signed bundle' });
    await waitFor(() => expect(input).toHaveValue('/tmp/dolphin-emu-0.1.0-linux-x64.gnbundle'));
  });

  it('reviews a bundle with per-component permissions and compatibility behind one fingerprint', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([official]);
    const review = vi.spyOn(commands, 'pluginBundleImportReview').mockResolvedValue(bundleReview());
    const confirm = vi.spyOn(commands, 'pluginBundleImportConfirmPublisher')
      .mockResolvedValue(bundleReview('trusted'));
    renderInventory('en', 'account-one');

    const path = await screen.findByRole('textbox', { name: 'Path to signed bundle' });
    fireEvent.change(path, { target: { value: '/packages/dolphin.gnbundle' } });
    await userEvent.setup().type(path, '{Enter}');

    const article = await screen.findByRole('article', { name: 'Review plugin bundle' });
    expect(review).toHaveBeenCalledWith('/packages/dolphin.gnbundle');
    expect(article).toHaveTextContent('Dolphin GameCube and Wii 0.2.0');
    expect(article).toHaveTextContent(publisher);
    expect(article).toHaveTextContent('Persist plugin data');
    expect(article).toHaveTextContent('Own provider');
    expect(article).toHaveTextContent('Optional');
    expect(article).toHaveTextContent('Keep the local catalog.');
    expect(article).toHaveTextContent('Launch games');
    expect(article).toHaveTextContent('Approved game session');
    expect(article).toHaveTextContent('Platforms: GameCube, Wii');
    expect(article).toHaveTextContent('Unavailable on this system');
    expect(article).toHaveTextContent('No package target matches this system');
    expect(article).toHaveTextContent('Incompatible');
    expect(within(article).getByRole('button', { name: 'Accept, install and enable' })).toBeDisabled();

    fireEvent.click(within(article).getByRole('button', { name: 'Trust this publisher' }));
    await waitFor(() => expect(confirm).toHaveBeenCalledWith('bundle-token', publisher));
    expect(await screen.findByText(/Bundle publisher confirmed/)).toBeInTheDocument();
  });

  it('installs an approved bundle from the review and refreshes the inventory', async () => {
    const inventory = vi.spyOn(commands, 'pluginInventory').mockResolvedValue([official]);
    vi.spyOn(commands, 'pluginBundleImportReview').mockResolvedValue(bundleReview());
    vi.spyOn(commands, 'pluginBundleImportConfirmPublisher').mockResolvedValue(bundleReview('trusted'));
    const install = vi.spyOn(commands, 'pluginBundleImportInstall').mockResolvedValue({
      bundleId: 'org.example.dolphin.bundle',
      components: [
        {
          pluginId: 'org.example.dolphin', kind: 'provider', version: '0.1.0', active: true,
          activated: true,
        },
        {
          pluginId: 'org.example.dolphin.launch', kind: 'extension', version: '0.2.0', active: false,
          activated: false,
        },
      ],
    });
    renderInventory('en', 'account-one');

    const path = await screen.findByRole('textbox', { name: 'Path to signed bundle' });
    fireEvent.change(path, { target: { value: '/packages/dolphin.gnbundle' } });
    await userEvent.setup().type(path, '{Enter}');
    const article = await screen.findByRole('article', { name: 'Review plugin bundle' });
    fireEvent.click(within(article).getByRole('button', { name: 'Trust this publisher' }));

    const consent = await within(article).findByRole('checkbox');
    fireEvent.click(consent);
    fireEvent.click(within(article).getByRole('button', { name: 'Accept, install and enable' }));

    await waitFor(() => expect(install).toHaveBeenCalledWith('bundle-token', true, [
      { pluginId: 'org.example.dolphin', deniedPermissionIds: [] },
      { pluginId: 'org.example.dolphin.launch', deniedPermissionIds: [] },
    ]));
    expect(screen.queryByRole('article', { name: 'Review plugin bundle' })).not.toBeInTheDocument();
    await waitFor(() => expect(inventory.mock.calls.length).toBeGreaterThan(1));
  });

  it('collects denied optional permissions per bundle component', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([official]);
    vi.spyOn(commands, 'pluginBundleImportReview').mockResolvedValue(bundleReview('trusted'));
    const install = vi.spyOn(commands, 'pluginBundleImportInstall').mockResolvedValue({
      bundleId: 'org.example.dolphin.bundle',
      components: [],
    });
    renderInventory('en', 'account-one');

    const path = await screen.findByRole('textbox', { name: 'Path to signed bundle' });
    fireEvent.change(path, { target: { value: '/packages/dolphin.gnbundle' } });
    await userEvent.setup().type(path, '{Enter}');
    const article = await screen.findByRole('article', { name: 'Review plugin bundle' });

    const persist = within(article).getByRole('switch', { name: 'Allow Persist plugin data' });
    expect(persist).toBeChecked();
    fireEvent.click(persist);
    expect(await within(article).findByText('The local catalog is rebuilt on every start.')).toBeInTheDocument();

    fireEvent.click(within(article).getByRole('checkbox', { name: /reviewed the declared native access/i }));
    fireEvent.click(within(article).getByRole('button', { name: 'Accept, install and enable' }));

    await waitFor(() => expect(install).toHaveBeenCalledWith('bundle-token', true, [
      { pluginId: 'org.example.dolphin', deniedPermissionIds: ['storage.persist'] },
      { pluginId: 'org.example.dolphin.launch', deniedPermissionIds: [] },
    ]));
  });

  it('reviews and applies one bundle update with per-component old and new versions', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    vi.spyOn(commands, 'pluginBundleUpdateAvailable').mockResolvedValue([{
      bundleId: 'org.example.dolphin.bundle',
      name: 'Dolphin GameCube and Wii',
      installedVersion: '0.2.0',
      versions: ['0.3.0'],
    }]);
    vi.spyOn(commands, 'pluginBundleRollbackAvailable').mockResolvedValue([]);
    const check = vi.spyOn(commands, 'pluginBundleUpdateCheck').mockResolvedValue([{
      bundleId: 'org.example.dolphin.bundle',
      name: 'Dolphin GameCube and Wii',
      installedVersion: '0.2.0',
      versions: ['0.3.0'],
    }]);
    const review = vi.spyOn(commands, 'pluginBundleUpdateReview')
      .mockResolvedValue(bundleUpdateReviewFixture());
    const approve = vi.spyOn(commands, 'pluginBundleUpdateApprove').mockResolvedValue({
      bundleId: 'org.example.dolphin.bundle',
      previousVersion: '0.2.0',
      version: '0.3.0',
      components: [
        { pluginId: 'org.example.dolphin', kind: 'provider', previousVersion: '0.1.0', version: '0.2.0', active: true },
        { pluginId: 'org.example.dolphin.launch', kind: 'extension', previousVersion: '0.2.0', version: '0.3.0', active: true },
      ],
    });
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    const details = await screen.findByRole('article', { name: 'Bundle details' });
    const updates = within(details).getByRole('region', { name: 'Bundle updates and rollback' });
    fireEvent.click(within(updates).getByRole('button', { name: 'Check for bundle updates' }));
    await waitFor(() => expect(check).toHaveBeenCalledTimes(1));

    fireEvent.click(await within(updates).findByRole('button', { name: 'Review bundle 0.3.0' }));
    await waitFor(() => expect(review).toHaveBeenCalledWith('org.example.dolphin.bundle', '0.3.0'));
    const reviewPanel = await within(updates).findByRole('article', { name: 'Bundle update review' });
    expect(reviewPanel).toHaveTextContent('Dolphin GameCube and Wii: 0.2.0 → 0.3.0');
    expect(reviewPanel).toHaveTextContent('0.1.0 → 0.2.0');
    expect(reviewPanel).toHaveTextContent('0.2.0 → 0.3.0');
    expect(reviewPanel).toHaveTextContent('game.launch: approved-game-session → selected-game-session');
    expect(reviewPanel).toHaveTextContent('Signed bundle hash');

    fireEvent.click(within(reviewPanel).getByRole('button', { name: 'Update every component' }));
    await waitFor(() => expect(approve).toHaveBeenCalledWith('org.example.dolphin.bundle', '0.3.0'));
    expect(await within(updates).findByText('Bundle updated to 0.3.0.')).toBeInTheDocument();
  });

  it('keeps unrelated bundle controls usable during a slow update and restores the saved state on failure', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    vi.spyOn(commands, 'pluginBundleUpdateAvailable').mockResolvedValue([{
      bundleId: 'org.example.dolphin.bundle',
      name: 'Dolphin GameCube and Wii',
      installedVersion: '0.2.0',
      versions: ['0.3.0'],
    }]);
    vi.spyOn(commands, 'pluginBundleRollbackAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginBundleUpdateReview').mockResolvedValue(bundleUpdateReviewFixture());
    let rejectApprove!: (error: Error) => void;
    const pendingApprove = new Promise<never>((_, reject) => {
      rejectApprove = reject;
    });
    const approve = vi.spyOn(commands, 'pluginBundleUpdateApprove')
      .mockReturnValue(pendingApprove);
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    const details = await screen.findByRole('article', { name: 'Bundle details' });
    const updates = within(details).getByRole('region', { name: 'Bundle updates and rollback' });
    fireEvent.click(await within(updates).findByRole('button', { name: 'Review bundle 0.3.0' }));
    const reviewPanel = await within(updates).findByRole('article', { name: 'Bundle update review' });
    fireEvent.click(within(reviewPanel).getByRole('button', { name: 'Update every component' }));
    await waitFor(() => expect(approve).toHaveBeenCalledTimes(1));

    // The pending update only disables its own controls: per-component activation stays usable.
    const componentItems = within(details)
      .getAllByRole('list', { name: 'Bundle components' })[0]
      .querySelectorAll('li');
    expect(within(componentItems[0] as HTMLElement).getByRole('button', { name: 'Enable' })).toBeEnabled();
    expect(within(reviewPanel).getByRole('button', { name: 'Update every component' })).toBeDisabled();

    rejectApprove(new Error('persistence failed'));
    expect(await within(updates).findByText(
      'The bundle change could not be saved. The last saved host state is still in effect.',
    )).toBeInTheDocument();
    expect(details).toHaveTextContent('Bundle version: 0.2.0');
    expect(within(reviewPanel).getByRole('button', { name: 'Update every component' })).toBeEnabled();
  });

  it('localizes a failed bundle update review in pt-BR', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    vi.spyOn(commands, 'pluginBundleUpdateAvailable').mockResolvedValue([{
      bundleId: 'org.example.dolphin.bundle',
      name: 'Dolphin GameCube and Wii',
      installedVersion: '0.2.0',
      versions: ['0.3.0'],
    }]);
    vi.spyOn(commands, 'pluginBundleRollbackAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginBundleUpdateReview').mockRejectedValue(new Error('hash mismatch'));
    renderInventory('pt-BR', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    const details = await screen.findByRole('article', { name: 'Detalhes do pacote' });
    const updates = within(details).getByRole('region', { name: 'Atualizações e reversão do pacote' });
    fireEvent.click(await within(updates).findByRole('button', { name: 'Revisar pacote 0.3.0' }));

    expect(await within(updates).findByText(
      'Não foi possível revisar esta versão do pacote. Nada foi instalado.',
    )).toBeInTheDocument();
  });

  it('reviews and confirms one bundle rollback', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    vi.spyOn(commands, 'pluginBundleUpdateAvailable').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginBundleRollbackAvailable').mockResolvedValue([{
      bundleId: 'org.example.dolphin.bundle',
      name: 'Dolphin GameCube and Wii',
      activeVersion: '0.3.0',
      versions: ['0.2.0'],
    }]);
    vi.spyOn(commands, 'pluginBundleRollbackReview')
      .mockResolvedValue(bundleUpdateReviewFixture(true));
    const approve = vi.spyOn(commands, 'pluginBundleRollbackApprove').mockResolvedValue({
      bundleId: 'org.example.dolphin.bundle',
      previousVersion: '0.3.0',
      version: '0.2.0',
      components: [],
    });
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    const details = await screen.findByRole('article', { name: 'Bundle details' });
    const updates = within(details).getByRole('region', { name: 'Bundle updates and rollback' });
    fireEvent.click(await within(updates).findByRole('button', { name: 'Review rollback to 0.2.0' }));
    const reviewPanel = await within(updates).findByRole('article', { name: 'Bundle rollback review' });
    const confirm = within(reviewPanel).getByRole('checkbox');
    const action = within(reviewPanel).getByRole('button', { name: 'Roll back every component' });
    expect(action).toBeDisabled();
    fireEvent.click(confirm);
    fireEvent.click(action);

    await waitFor(() => expect(approve).toHaveBeenCalledWith(
      'org.example.dolphin.bundle',
      '0.2.0',
      true,
    ));
    expect(await within(updates).findByText('Bundle rolled back to 0.2.0.')).toBeInTheDocument();
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
  it('reports a compatible component that stayed installed but was not activated', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([official]);
    vi.spyOn(commands, 'pluginBundleImportReview').mockResolvedValue(bundleReview('trusted'));
    vi.spyOn(commands, 'pluginBundleImportInstall').mockResolvedValue({
      bundleId: 'org.example.dolphin.bundle',
      components: [
        {
          pluginId: 'org.example.dolphin', kind: 'provider', version: '0.1.0', active: true,
          activated: false, activationError: 'activation failed: plugin did not establish its command session',
        },
      ],
    });
    renderInventory('en', 'account-one');

    const path = await screen.findByRole('textbox', { name: 'Path to signed bundle' });
    fireEvent.change(path, { target: { value: '/packages/dolphin.gnbundle' } });
    await userEvent.setup().type(path, '{Enter}');
    const article = await screen.findByRole('article', { name: 'Review plugin bundle' });
    fireEvent.click(within(article).getByRole('checkbox', { name: /reviewed the declared native access/i }));
    fireEvent.click(within(article).getByRole('button', { name: 'Accept, install and enable' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Installed, but not activated');
    expect(alert).toHaveTextContent('org.example.dolphin');
    expect(alert).toHaveTextContent('plugin did not establish its command session');
  });

  it('removes playtime rows immediately after uninstalling the bundle', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    vi.spyOn(commands, 'pluginBundleUninstall').mockResolvedValue({
      bundleId: 'org.example.dolphin.bundle',
      removed: [
        { pluginId: 'org.example.dolphin', version: '0.1.0', stopped: true },
        { pluginId: 'org.example.dolphin.launch', version: '0.2.0', stopped: false },
      ],
      failures: [],
      dataRetention: 'kept',
    });
    vi.spyOn(commands, 'pluginPlaytimeInventory')
      .mockResolvedValueOnce([{
        pluginId: 'org.example.dolphin',
        name: 'Dolphin discovery',
        version: '0.1.0',
        playtime: {
          identifiesGameplay: true,
          identifiesPauses: true,
          withoutPauseDetection: 'disabled',
        },
        accountingEnabled: true,
      }])
      .mockResolvedValue([]);
    renderInventory('en', 'account-one');

    expect(await screen.findByRole('article', { name: 'Dolphin discovery' })).toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    fireEvent.click(within(await screen.findByRole('article', { name: 'Bundle details' }))
      .getByRole('button', { name: 'Uninstall bundle' }));
    fireEvent.click(within(await screen.findByRole('dialog'))
      .getByRole('button', { name: 'Uninstall bundle' }));

    await waitFor(() => expect(screen.queryByRole('article', { name: 'Dolphin discovery' })).not.toBeInTheDocument());
    expect(screen.getByText('No active trusted third-party plugins declare playtime support.')).toBeInTheDocument();
  });

  it('renders folder and program approvals for each bundle component with the component ids', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundle]);
    const folders = vi.spyOn(commands, 'pluginFolderApprovals').mockResolvedValue([]);
    const runtimes = vi.spyOn(commands, 'pluginRuntimeApprovals').mockResolvedValue([]);
    renderInventory('en', 'account-one');

    fireEvent.click(await screen.findByRole('button', { name: /Dolphin GameCube and Wii/ }));
    const discovery = await screen.findByRole('region', { name: 'Approvals for Dolphin discovery' });
    const launch = await screen.findByRole('region', { name: 'Approvals for Dolphin launch' });
    expect(within(discovery).getByRole('region', { name: 'Folder access and library roots' })).toBeInTheDocument();
    expect(within(discovery).getByRole('region', { name: 'Approved runtime programs' })).toBeInTheDocument();
    expect(within(launch).getByRole('region', { name: 'Folder access and library roots' })).toBeInTheDocument();
    expect(within(launch).getByRole('region', { name: 'Approved runtime programs' })).toBeInTheDocument();

    await waitFor(() => expect(folders).toHaveBeenCalledWith('org.example.dolphin', publisher));
    expect(folders).toHaveBeenCalledWith('org.example.dolphin.launch', publisher);
    expect(runtimes).toHaveBeenCalledWith('org.example.dolphin', publisher);
    expect(runtimes).toHaveBeenCalledWith('org.example.dolphin.launch', publisher);
    // A bundle has no namespace of its own, so no bundle-level approvals are requested.
    expect(folders).not.toHaveBeenCalledWith('org.example.dolphin.bundle', publisher);
    expect(runtimes).not.toHaveBeenCalledWith('org.example.dolphin.bundle', publisher);
  });

});
