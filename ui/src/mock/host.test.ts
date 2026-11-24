import { describe, expect, it, vi } from 'vitest';
import {
  createMockHost,
  MOCK_CATALOG_GAMES,
  MOCK_COLLECTIONS,
  MOCK_COVER_WALL,
  MOCK_HIDDEN_KEY,
  MOCK_LIFECYCLE_PLUGIN_ID,
  MOCK_LIFECYCLE_PLUGIN_NAME,
  MOCK_LIFECYCLE_PROVIDER,
  MOCK_LIMIT_COLLECTION_NAME,
  mockHostOptionsFromSearch,
} from './host';
import { groupRows, runLibraryQuery, sortRows, type ProviderRef } from './query';
import {
  createDefaultFilters,
  createDefaultLibraryLayout,
  type AppInfo,
  type LibraryFilters,
  type LibraryLayout,
  type PluginDurableDeletion,
  type PluginDurableUsage,
  type PluginInfo,
  type PluginPlaytimeInfo,
  type Row,
  type Snapshot,
} from '../api/types';
import type { PortProgress, PortSuppliedPick, PortToolsCacheView } from '../api/ports';

/** Waits for the mock's asynchronous saved-view write to finish. */
async function waitForViewWrite(
  host: ReturnType<typeof createMockHost>,
  state: 'succeeded' | 'failed',
) {
  await vi.waitFor(async () => {
    const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library?.viewWrite?.state).toBe(state);
  }, { timeout: 1_000 });
}

describe('UI mock host', () => {
  it('maps the documented Settings query fixtures', () => {
    const options = mockHostOptionsFromSearch(
      '?steam=notConnected&epic=expired&watches=unavailable&platform=linux',
    );
    expect(options).toMatchObject({
      storeAccount: 'notConnected',
      epicAccount: 'expired',
      watches: 'unavailable',
      platform: 'linux',
    });
    expect(mockHostOptionsFromSearch('?epic=bogus').epicAccount).toBeUndefined();
    expect(mockHostOptionsFromSearch('?watches=other').watches).toBeUndefined();
  });

  it('starts with a verified mock session and deterministic games', async () => {
    const host = createMockHost();
    const snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.auth).toEqual({ signedIn: { user: { id: 'mock-user', username: 'UI Developer', email: 'ui@example.invalid' } } });
    expect(snapshot.library?.access).toBe('verified');
    expect(snapshot.library?.layout.sidebarWidth).toBe(240);
    expect(snapshot.library?.layout.detailWidth).toBe(360);
    expect(snapshot.library?.layout.sidebarCollapsed).toBe(false);
    expect(snapshot.library?.rows).toHaveLength(12);
    snapshot.library!.rows.length = 0;
    expect((await host.invoke('get_snapshot') as Snapshot).library?.rows).toHaveLength(12);
  });

  it('exposes the running fixture with its real process start', async () => {
    const host = createMockHost();
    const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const runningKey = snapshot.tracking.running[0];
    expect(runningKey).toBeTruthy();
    const row = snapshot.library!.rows.find((item) => item.key === runningKey);
    expect(row?.running).toBe(true);
    expect(row?.sessionStartedAt).toBe(
      snapshot.tracking.sessionStartedAt[runningKey],
    );
    expect(snapshot.tracking.sessionStartedAt[runningKey]).toBeLessThanOrEqual(
      Math.floor(Date.now() / 1000) - 2500,
    );
  });

  it('serves the tracking conflict fixture and lets the user keep or move tracking', async () => {
    const host = createMockHost({ claim: true });
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const claim = snapshot.localGames.pendingClaim;
    expect(claim?.storeKey).toBe('steam:9');
    expect(claim?.storeEntry).toMatchObject({
      title: 'Infinite Coast',
      provider: 'steam',
      coverFile: 'mock-8.svg',
      playtimeSeconds: 8 * 3600 + 16 * 60,
    });
    expect(claim?.registration).toMatchObject({
      title: 'Infinite Coast',
      root: 'D:\\Jogos\\Infinite Coast',
      coverFile: 'mock-2.svg',
    });

    await host.invoke('resolve_local_claim', { transfer: false });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.pendingClaim).toBeNull();
    expect(snapshot.library?.selected).toContain('steam:9');

    const canceled = createMockHost({ claim: true });
    await canceled.invoke('cancel_local_claim');
    snapshot = (await canceled.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.pendingClaim).toBeNull();
  });

  it('mirrors the cached account library statuses, collections and last check', async () => {
    const host = createMockHost();
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.localGames.discoveryLastCheckedAt).toBeGreaterThan(0);
    const label = (row: Row): string =>
      row.gamenode ? row.gamenode.status ?? 'inLibrary' : 'notInLibrary';
    const statuses = new Set(snapshot.library!.rows.map(label));
    for (const status of [
      'playing',
      'finished',
      'planned',
      'ongoing',
      'dropped',
      'inLibrary',
      'notInLibrary',
    ]) {
      expect(statuses.has(status)).toBe(true);
    }
    for (const collection of MOCK_COLLECTIONS) {
      expect(
        snapshot.library!.rows.some((row) =>
          row.gamenode?.collectionIds.includes(collection.id),
        ),
      ).toBe(true);
    }
    expect(snapshot.library!.filterCounts!.gamenodeStatus.available).toBe(true);
    expect(snapshot.library!.filterCounts!.gamenodeStatus.finished).toBeGreaterThan(0);

    await host.invoke('library_input', {
      input: { setScope: { collection: 'backlog-curto' } },
    });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.rows.length).toBeGreaterThan(0);
    expect(
      snapshot.library!.rows.every((row) =>
        row.gamenode?.collectionIds.includes('backlog-curto'),
      ),
    ).toBe(true);
    expect(snapshot.library!.titleSource).toEqual({
      kind: 'collection',
      id: 'backlog-curto',
    });
    const counts = snapshot.library!.filterCounts!.gamenodeStatus;
    expect(
      counts.playing +
        counts.finished +
        counts.planned +
        counts.ongoing +
        counts.dropped +
        counts.inLibrary +
        counts.notInLibrary,
    ).toBe(snapshot.library!.rows.length);
  });

  it('persists application preferences and reports the effective language', async () => {
    const host = createMockHost({ locale: 'pt-BR' });
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.preferences).toMatchObject({
      startWithWindows: true,
      startMinimized: true,
      closeToTray: true,
      locale: 'system',
      showStatusBar: true,
      startScope: 'last',
      toolDefaultChannel: 'stable',
      sharedExcluded: [],
    });
    expect((await host.invoke('get_app_info') as AppInfo).locale).toBe('pt-BR');

    const listener = vi.fn();
    const unsubscribe = host.subscribe(listener);

    await host.invoke('set_app_preference', { patch: { locale: 'en' } });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.preferences.locale).toBe('en');
    let appInfo = (await host.invoke('get_app_info')) as AppInfo;
    expect(appInfo.locale).toBe('en');
    expect(appInfo.systemLocale).toBe('pt-BR');
    expect(listener).toHaveBeenCalledTimes(1);

    await host.invoke('set_app_preference', { patch: { closeToTray: false } });
    await host.invoke('set_app_preference', {
      patch: { sharedExcluded: ['steam', 'epic'] },
    });
    await host.invoke('set_app_preference', { patch: { startScope: 'installed' } });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.preferences.closeToTray).toBe(false);
    expect(snapshot.preferences.sharedExcluded).toEqual(['steam', 'epic']);
    expect(snapshot.preferences.startScope).toBe('installed');
    expect(snapshot.preferences.locale).toBe('en');

    await expect(
      host.invoke('set_app_preference', { patch: { locale: 'klingon' } }),
    ).rejects.toBe('invalidPreference');
    await expect(
      host.invoke('set_app_preference', { patch: { sharedExcluded: ['not a store'] } }),
    ).rejects.toBe('invalidPreference');
    expect((await host.invoke('get_snapshot') as Snapshot).preferences.locale).toBe('en');

    unsubscribe();
  });

  it('persists the grid cover size in the layout through library_input', async () => {
    const host = createMockHost();
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library?.layout.coverSize).toBe('medium');

    await host.invoke('library_input', { input: { setCoverSize: 'large' } });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library?.layout.coverSize).toBe('large');
    expect(snapshot.library?.rows).toHaveLength(12);
  });

  it('publishes interactions and resets on sign out and sign in', async () => {
    const host = createMockHost({ signedOut: true, locale: 'pt-BR' });
    expect((await host.invoke('get_app_info') as AppInfo).locale).toBe('pt-BR');
    expect((await host.invoke('get_snapshot') as Snapshot).auth).toBe('signedOut');
    const listener = vi.fn();
    const unsubscribe = host.subscribe(listener);
    await host.invoke('dispatch_auth', { input: { startSignIn: 'google' } });
    await host.invoke('library_input', { input: { setSearch: 'aster' } });
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library?.rows.map(row => row.title)).toEqual(['Aster Vale']);
    await host.invoke('library_input', { input: { select: { key: 'steam:1', mode: 'replace' } } });
    await host.invoke('library_input', { input: { setDetailOpen: true } });
    await host.invoke('library_input', { input: { setSidebarCollapsed: true } });
    snapshot = listener.mock.lastCall![0] as Snapshot;
    expect(snapshot.library?.detail?.row.title).toBe('Aster Vale');
    expect(snapshot.library?.layout.detailOpen).toBe(true);
    expect(snapshot.library?.layout.sidebarCollapsed).toBe(true);
    await host.invoke('library_input', { input: { setSidebarCollapsed: false } });
    snapshot = listener.mock.lastCall![0] as Snapshot;
    expect(snapshot.library?.layout.sidebarCollapsed).toBe(false);
    await host.invoke('dispatch_auth', { input: 'signOut' });
    await host.invoke('dispatch_auth', { input: { startSignIn: 'discord' } });
    expect((await host.invoke('get_snapshot') as Snapshot).library?.rows).toHaveLength(12);
    unsubscribe();
    listener.mockClear();
    await host.invoke('set_sync_enabled', { enabled: false });
    expect(listener).not.toHaveBeenCalled();
  });

  it('serves the sign-in cover wall and the pending reopen flow', async () => {
    const host = createMockHost({
      signedOut: true,
      signInPending: true,
      signInReopenFails: true,
    });
    expect(await host.invoke('sign_in_cover_wall')).toEqual(MOCK_COVER_WALL);
    expect(MOCK_COVER_WALL).toHaveLength(12);

    await host.invoke('dispatch_auth', { input: { startSignIn: 'discord' } });
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.auth).toEqual({
      pending: {
        provider: 'discord',
        awaitingBrowser: true,
        canReopen: true,
        reopenFailed: false,
      },
    });

    await host.invoke('dispatch_auth', { input: 'reopenBrowser' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.auth).toEqual({
      pending: {
        provider: 'discord',
        awaitingBrowser: true,
        canReopen: true,
        reopenFailed: true,
      },
    });

    await host.invoke('dispatch_auth', { input: 'cancelSignIn' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.auth).toBe('signedOut');
    expect(snapshot.notice).toBe('signInCancelled');
  });

  it('serves achievement summaries and filtered lists', async () => {
    const host = createMockHost();
    const snapshot = await host.invoke('get_snapshot') as Snapshot;
    const row = snapshot.library!.rows.find((entry) => entry.provider === 'steam')!;
    await host.invoke('library_input', {
      input: { select: { key: row.key, mode: 'replace' } },
    });
    const selected = await host.invoke('get_snapshot') as Snapshot;
    expect(selected.library?.detail?.achievementBackend).toBe(true);
    expect(selected.library?.detail?.achievements?.state).toBe('ready');
    expect(selected.library?.detail?.achievements?.total).toBe(58);
    expect(selected.library?.detail?.achievements?.unlocked).toBe(31);

    type MockList = {
      achievements: Array<{
        name: string;
        unlocked: boolean;
        hidden: boolean;
        globalPercent: number | null;
      }>;
      counts: { all: number; unlocked: number; locked: number };
      hasRarity: boolean;
      providerName: string;
      hasProviderPage: boolean;
      refreshedAt: number | null;
    };
    const all = await host.invoke('achievement_list', {
      key: row.key,
      filter: 'all',
      sort: 'mostRecent',
    }) as MockList;
    expect(all.achievements).toHaveLength(58);
    expect(all.counts).toEqual({ all: 58, unlocked: 31, locked: 27 });
    expect(all.hasRarity).toBe(true);
    expect(all.providerName).toBe('Steam');
    expect(all.hasProviderPage).toBe(true);
    expect(all.refreshedAt).toBeGreaterThan(0);
    expect(all.achievements[0].unlocked).toBe(true);
    expect(all.achievements.at(-1)?.unlocked).toBe(false);

    const unlocked = await host.invoke('achievement_list', {
      key: row.key,
      filter: 'unlocked',
      sort: 'mostRecent',
    }) as MockList;
    expect(unlocked.achievements).toHaveLength(31);
    expect(unlocked.achievements.every((entry) => entry.unlocked)).toBe(true);
    const locked = await host.invoke('achievement_list', {
      key: row.key,
      filter: 'locked',
      sort: 'mostRecent',
    }) as MockList;
    expect(locked.achievements).toHaveLength(27);
    expect(locked.achievements.some((entry) => entry.hidden)).toBe(true);

    const rarest = await host.invoke('achievement_list', {
      key: row.key,
      filter: 'all',
      sort: 'rarest',
    }) as MockList;
    const percents = rarest.achievements
      .map((entry) => entry.globalPercent)
      .filter((percent): percent is number => percent != null);
    expect(percents).toEqual([...percents].sort((left, right) => left - right));
    expect(
      rarest.achievements.slice(percents.length).every(
        (entry) => entry.globalPercent == null,
      ),
    ).toBe(true);

    const byName = await host.invoke('achievement_list', {
      key: row.key,
      filter: 'all',
      sort: 'name',
    }) as MockList;
    const names = byName.achievements.map((entry) =>
      entry.hidden && !entry.unlocked ? 'Hidden achievement' : entry.name,
    );
    expect(names).toEqual([...names].sort((left, right) => left.localeCompare(right)));

    expect(await host.invoke('achievement_list', { key: 'gog:2', filter: 'all' })).toBeNull();
    await expect(
      host.invoke('open_achievements_page', { entryKey: row.key }),
    ).resolves.toBeUndefined();
    await expect(
      host.invoke('open_achievements_page', { entryKey: 'gog:2' }),
    ).rejects.toEqual({ code: 'unsupported' });
  });

  it('does not route unsupported commands to any real service', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const host = createMockHost();
    await host.invoke('open_entry', { entryKey: 'steam:1', kind: 'play' });
    expect(warn).toHaveBeenCalledWith('[UI mock] Unsupported command: open_entry');
    warn.mockRestore();
  });

  it('exposes Steam account fixtures for every connection state', async () => {
    const scenarios = [
      'notConnected', 'checking', 'signingIn', 'connected',
      'expired', 'unreachable', 'credentialError', 'persistFailed',
    ] as const;

    for (const scenario of scenarios) {
      const snapshot = await createMockHost({ storeAccount: scenario }).invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts).toHaveLength(2);
      expect(snapshot.storeAccounts.map((account) => account.provider)).toEqual(['steam', 'epic']);
      const steam = snapshot.storeAccounts[0];
      expect(steam.state).toBe(scenario === 'persistFailed' ? 'connected' : scenario);
      expect(steam.persistFailed).toBe(scenario === 'persistFailed');
    }
  });

  it('keeps Epic connected and without shared data while a Steam card action runs', async () => {
    const host = createMockHost();
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    const epic = snapshot.storeAccounts.find((account) => account.provider === 'epic')!;
    const steam = snapshot.storeAccounts.find((account) => account.provider === 'steam')!;
    expect(epic.state).toBe('connected');
    expect(epic.sharedCount).toBe(0);
    expect(epic.sharedError).toBeNull();
    expect(epic.sharedLabel).toBeNull();
    expect(steam.sharedLabel).toBe('Steam Family');

    await host.invoke('provider_sign_out', { providerId: 'epic' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.storeAccounts.find((account) => account.provider === 'epic')!.state).toBe('notConnected');
    expect(snapshot.storeAccounts.find((account) => account.provider === 'steam')!.state).toBe('connected');
    expect(snapshot.library!.providers.find((provider) => provider.id === 'steam')!.ownedCount).toBe(5);
  });

  it('exposes the expired Epic fixture while Steam stays connected', async () => {
    const snapshot = await createMockHost({ epicAccount: 'expired' })
      .invoke('get_snapshot') as Snapshot;
    const epic = snapshot.storeAccounts.find((account) => account.provider === 'epic')!;
    const steam = snapshot.storeAccounts.find((account) => account.provider === 'steam')!;
    expect(epic.state).toBe('expired');
    expect(epic.canSignIn).toBe(true);
    expect(epic.canSignOut).toBe(true);
    expect(epic.accountName).toBe('EpicPlayer');
    expect(epic.sharedLabel).toBeNull();
    expect(steam.state).toBe('connected');
  });

  it('returns trusted plugin inventory without credential data and respects the mock platform', async () => {
    const windowsPlugins = await createMockHost({ platform: 'windows' }).invoke('plugin_inventory') as PluginInfo[];
    const windowsPlugin = windowsPlugins[0];
    expect(windowsPlugin.source).toBe('official');
    expect(windowsPlugin.status).toBe('installed');
    expect(windowsPlugin.capabilities).toContain('library.discovery');
    expect(windowsPlugin.nativeAccess).toContain('registry');
    expect(JSON.stringify(windowsPlugin)).not.toMatch(/token|cookieValue|password|credentialValue/i);

    const epicPlugin = windowsPlugins.find((plugin) => plugin.providerId === 'epic');
    expect(epicPlugin?.name).toBe('Epic Games Store');
    expect(epicPlugin?.capabilities).toContain('playtime.counter');
    expect(epicPlugin?.capabilities).not.toContain('auth.webview.cookies.read');

    const linuxPlugins = await createMockHost({ platform: 'linux' }).invoke('plugin_inventory') as PluginInfo[];
    expect(linuxPlugins[0].nativeAccess).not.toContain('registry');
    expect(linuxPlugins.some((plugin) => plugin.providerId === 'epic')).toBe(false);
    const packagePlugin = linuxPlugins.find((plugin) => plugin.providerId === 'packagekit');
    expect(packagePlugin?.name).toBe('Gerenciador de pacotes Linux');
    expect(packagePlugin?.capabilities).toEqual(['provider.context.read', 'library.discovery']);
    expect(packagePlugin?.nativeAccess).toEqual(['files', 'process', 'dbus']);
    expect((await createMockHost({ platform: 'windows' }).invoke('plugin_inventory') as PluginInfo[])
      .some((plugin) => plugin.providerId === 'packagekit')).toBe(false);
  });

  it('serves a stopped community plugin alongside the official inventory', async () => {
    const windowsPlugins = await createMockHost({ platform: 'windows' })
      .invoke('plugin_inventory') as PluginInfo[];
    const windowsCommunity = windowsPlugins.find(
      (plugin) => plugin.source === 'community',
    );
    expect(windowsCommunity).toMatchObject({
      providerId: 'community',
      pluginId: 'dev.prateleira.local',
      name: 'Prateleira Local',
      version: '0.3.1',
      status: 'installed',
      session: 'stopped',
      capabilities: ['provider.context.read'],
      nativeAccess: ['files', 'network'],
    });
    expect(windowsPlugins.filter((plugin) => plugin.source === 'official')).toHaveLength(2);

    const linuxPlugins = await createMockHost({ platform: 'linux' })
      .invoke('plugin_inventory') as PluginInfo[];
    expect(linuxPlugins.filter((plugin) => plugin.source === 'official')).toHaveLength(2);
    expect(linuxPlugins.find((plugin) => plugin.source === 'community')).toMatchObject({
      name: 'Prateleira Local',
      session: 'stopped',
      nativeAccess: ['files', 'dbus'],
    });
  });

  it('lists trusted plugin versions and lets Stop/Start change the session', async () => {
    const host = createMockHost({ platform: 'windows' });
    let plugins = await host.invoke('plugin_inventory') as PluginInfo[];
    expect(plugins.map((plugin) => plugin.version)).toEqual(['0.1.0', '0.1.0', '0.3.1', '1.0.0', '0.2.0']);
    expect(plugins.find((plugin) => plugin.providerId === 'community')?.session)
      .toBe('stopped');

    await host.invoke('plugin_stop', { providerId: 'epic' });
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    plugins = await host.invoke('plugin_inventory') as PluginInfo[];
    expect(plugins.find((plugin) => plugin.providerId === 'epic')?.session).toBe('stopped');
    expect(plugins.find((plugin) => plugin.providerId === 'steam')?.session).toBe('active');
    expect(snapshot.storeAccounts.find((account) => account.provider === 'epic')?.canCheck)
      .toBe(false);

    await host.invoke('plugin_start', { providerId: 'epic' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    plugins = await host.invoke('plugin_inventory') as PluginInfo[];
    expect(plugins.find((plugin) => plugin.providerId === 'epic')?.session).toBe('active');
    expect(snapshot.storeAccounts.find((account) => account.provider === 'epic')?.canCheck)
      .toBe(true);
  });

  it('maps the plugin lifecycle fixture from the documented query', () => {
    expect(mockHostOptionsFromSearch('?plugins=lifecycle')).toMatchObject({
      pluginLifecycle: true,
    });
    expect(mockHostOptionsFromSearch('?plugins=other').pluginLifecycle).toBe(false);
  });

  it('removes an uninstalled provider and its live rows, then restores the rescan count', async () => {
    const host = createMockHost({ pluginLifecycle: true });
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.providers.find(
      (provider) => provider.id === MOCK_LIFECYCLE_PROVIDER.id,
    )).toMatchObject({
      name: MOCK_LIFECYCLE_PROVIDER.name,
      count: 2,
      installedCount: 2,
    });
    expect(snapshot.library!.rows.filter(
      (row) => row.provider === MOCK_LIFECYCLE_PROVIDER.id,
    )).toHaveLength(2);
    expect((await host.invoke('plugin_inventory') as PluginInfo[]).find(
      (plugin) => plugin.pluginId === MOCK_LIFECYCLE_PLUGIN_ID,
    )).toMatchObject({ status: 'installed', session: 'active', enabled: true });

    await host.invoke('plugin_remove', {
      pluginId: MOCK_LIFECYCLE_PLUGIN_ID,
      deleteData: false,
    });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.providers.some(
      (provider) => provider.id === MOCK_LIFECYCLE_PROVIDER.id,
    )).toBe(false);
    expect(snapshot.library!.rows.some(
      (row) => row.provider === MOCK_LIFECYCLE_PROVIDER.id,
    )).toBe(false);
    expect((await host.invoke('plugin_inventory') as PluginInfo[]).some(
      (plugin) => plugin.pluginId === MOCK_LIFECYCLE_PLUGIN_ID,
    )).toBe(false);

    expect(await host.invoke('plugin_import_install', {
      token: 'mock-package-review',
      nativeCodeConsent: false,
      deniedPermissionIds: [],
    })).toEqual({ pluginId: MOCK_LIFECYCLE_PLUGIN_ID, activated: true });

    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.providers.find(
      (provider) => provider.id === MOCK_LIFECYCLE_PROVIDER.id,
    )).toMatchObject({ count: 3, installedCount: 3 });
    expect(snapshot.library!.rows.filter(
      (row) => row.provider === MOCK_LIFECYCLE_PROVIDER.id,
    )).toHaveLength(3);
    expect(snapshot.library!.rows.some((row) => row.key === 'dolphin:fzero')).toBe(true);
    expect((await host.invoke('plugin_inventory') as PluginInfo[]).find(
      (plugin) => plugin.pluginId === MOCK_LIFECYCLE_PLUGIN_ID,
    )).toMatchObject({ status: 'installed', session: 'active', enabled: true });
  });

  it('drops playtime accounting rows with an uninstalled plugin and restores them on reinstall', async () => {
    const host = createMockHost({ pluginLifecycle: true });
    expect(await host.invoke('plugin_playtime_inventory')).toEqual([{
      pluginId: MOCK_LIFECYCLE_PLUGIN_ID,
      name: MOCK_LIFECYCLE_PLUGIN_NAME,
      version: '0.1.0',
      playtime: {
        identifiesGameplay: true,
        identifiesPauses: false,
        withoutPauseDetection: 'application_open',
      },
      accountingEnabled: true,
    }] satisfies PluginPlaytimeInfo[]);

    await host.invoke('plugin_remove', {
      pluginId: MOCK_LIFECYCLE_PLUGIN_ID,
      deleteData: false,
    });
    expect(await host.invoke('plugin_playtime_inventory')).toEqual([]);

    await host.invoke('plugin_import_install', {
      token: 'mock-package-review',
      nativeCodeConsent: false,
      deniedPermissionIds: [],
    });
    expect(await host.invoke('plugin_playtime_inventory')).toHaveLength(1);
  });

  it('keeps cached provider games when its plugin is disabled and restores the active state on enable', async () => {
    const host = createMockHost({ pluginLifecycle: true });
    await host.invoke('plugin_disable', { pluginId: MOCK_LIFECYCLE_PLUGIN_ID });

    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.providers.find(
      (provider) => provider.id === MOCK_LIFECYCLE_PROVIDER.id,
    )).toMatchObject({ count: 2, installedCount: 2 });
    expect(snapshot.library!.rows.filter(
      (row) => row.provider === MOCK_LIFECYCLE_PROVIDER.id,
    )).toHaveLength(2);
    expect((await host.invoke('plugin_inventory') as PluginInfo[]).find(
      (plugin) => plugin.pluginId === MOCK_LIFECYCLE_PLUGIN_ID,
    )).toMatchObject({
      status: 'unavailable',
      session: 'stopped',
      enabled: false,
      runtimeAvailable: true,
    });

    await host.invoke('plugin_enable', { pluginId: MOCK_LIFECYCLE_PLUGIN_ID });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.rows.filter(
      (row) => row.provider === MOCK_LIFECYCLE_PROVIDER.id,
    )).toHaveLength(2);
    expect((await host.invoke('plugin_inventory') as PluginInfo[]).find(
      (plugin) => plugin.pluginId === MOCK_LIFECYCLE_PLUGIN_ID,
    )).toMatchObject({ status: 'installed', session: 'active', enabled: true });
  });

  it.each(['en', 'pt-BR'] as const)('keeps Steam account and PC ports state together in the %s snapshot', async (locale) => {
    const host = createMockHost({ locale });
    await host.invoke('ports_pick_parent');
    const snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.storeAccounts[0].provider).toBe('steam');
    expect(snapshot.ports).toBeDefined();
    expect(snapshot.ports.preferences.lastParentFolder).toBe('D:\\Ports');
  });

  it('marks rows with installed, owned and shared facts and keeps counts consistent', async () => {
    const snapshot = await createMockHost().invoke('get_snapshot') as Snapshot;
    const rows = snapshot.library!.rows;

    expect(rows).toHaveLength(12);
    expect(rows.every((row) => typeof row.installed === 'boolean')).toBe(true);
    expect(rows.some((row) => row.owned)).toBe(true);
    expect(rows.some((row) => row.shared && !row.owned)).toBe(true);
    expect(rows.every((row) => !row.shared || row.provider === 'steam')).toBe(true);
    expect(snapshot.library!.installedTotal).toBe(rows.filter((row) => row.installed).length);

    const steam = snapshot.library!.providers.find((provider) => provider.id === 'steam')!;
    expect(steam.count).toBe(rows.filter((row) => row.provider === 'steam').length);
    expect(steam.installedCount).toBe(rows.filter((row) => row.provider === 'steam' && row.installed).length);
    expect(steam.ownedCount).toBe(rows.filter((row) => row.provider === 'steam' && row.owned).length);
    expect(steam.sharedCount).toBe(rows.filter((row) => row.provider === 'steam' && row.shared && !row.owned).length);
    expect(steam.sharedLabel).toBe('Steam Family');
    const epic = snapshot.library!.providers.find((provider) => provider.id === 'epic')!;
    expect(epic.sharedLabel).toBeNull();
  });

  it('reacts to Steam checking, sign-out and sign-in commands with published snapshots', async () => {
    vi.useFakeTimers();
    try {
      const host = createMockHost();
      const listener = vi.fn();
      host.subscribe(listener);

      await host.invoke('provider_check', { providerId: 'steam' });
      let snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('checking');
      vi.advanceTimersByTime(900);
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('connected');

      await host.invoke('provider_sign_out', { providerId: 'steam' });
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('notConnected');
      expect(snapshot.library!.rows.every((row) => row.installed)).toBe(true);
      expect(snapshot.library!.providers.find((provider) => provider.id === 'steam')!.ownedCount).toBe(0);

      await host.invoke('provider_sign_in', { providerId: 'steam' });
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('signingIn');
      vi.advanceTimersByTime(1500);
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('connected');

      await host.invoke('provider_sign_in', { providerId: 'steam' });
      await host.invoke('provider_cancel_sign_in', { providerId: 'steam' });
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('connected');
      await host.invoke('provider_focus_sign_in', { providerId: 'steam' });

      expect(listener).toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it('applies the installed availability filter and updates the provider counts', async () => {
    const host = createMockHost();
    const installed = {
      ...createDefaultFilters(),
      availability: { installed: true, owned: false, shared: false },
    };
    await host.invoke('library_input', { input: { setFilters: installed } });
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.layout.filters.availability.installed).toBe(true);
    expect(snapshot.library!.rows.length).toBe(snapshot.library!.installedTotal);
    expect(snapshot.library!.rows.every((row) => row.installed)).toBe(true);
    expect(snapshot.library!.total).toBe(12);
    expect(snapshot.library!.providers.find((provider) => provider.id === 'steam')!.count).toBe(3);

    await host.invoke('library_input', { input: { setFilters: createDefaultFilters() } });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.layout.filters.availability.installed).toBe(false);
    expect(snapshot.library!.rows).toHaveLength(12);
  });

  it('shows only installed games when the Steam account fixture is disconnected', async () => {
    const snapshot = await createMockHost({ storeAccount: 'notConnected' }).invoke('get_snapshot') as Snapshot;

    expect(snapshot.storeAccounts[0].state).toBe('notConnected');
    expect(snapshot.storeAccounts[0].ownedCount).toBe(0);
    expect(snapshot.library!.rows).toHaveLength(9);
    expect(snapshot.library!.rows.every((row) => row.installed)).toBe(true);
    expect(snapshot.library!.providers.find((provider) => provider.id === 'steam')!.ownedCount).toBe(0);
  });
});

async function withFilters(filters: LibraryFilters): Promise<Snapshot> {
  const host = createMockHost();
  await host.invoke('library_input', { input: { setFilters: filters } });
  return (await host.invoke('get_snapshot')) as Snapshot;
}

describe('mock library filters and sorting', () => {
  it('keeps unknown playtime out of the unplayed facet and combines facets with AND', async () => {
    const unplayed = await withFilters({
      ...createDefaultFilters(),
      played: 'unplayed',
    });
    // The first fixture row is installed, owned and has a known zero; the unknown rows are not
    // presented as unplayed.
    expect(unplayed.library!.rows.map((row) => row.key)).toEqual(['steam:1']);

    const favoritesInstalled = await withFilters({
      ...createDefaultFilters(),
      favoritesOnly: true,
      availability: { installed: true, owned: false, shared: false },
    });
    expect(favoritesInstalled.library!.rows.map((row) => row.key)).toEqual([
      'gog:2',
      'gog:4',
    ]);
  });

  it('sorts by playtime with unknown values last in both directions', async () => {
    const host = createMockHost({ largeLibrary: true });
    await host.invoke('library_input', {
      input: { setSort: { key: 'playtime', direction: 'descending' } },
    });
    const descending = (await host.invoke('get_snapshot')) as Snapshot;
    expect(descending.library!.rows.at(-1)!.playtime.seconds).toBeNull();
    expect(descending.library!.rows[0].playtime.seconds).toBeGreaterThan(0);

    await host.invoke('library_input', {
      input: { setSort: { key: 'playtime', direction: 'ascending' } },
    });
    const ascending = (await host.invoke('get_snapshot')) as Snapshot;
    expect(ascending.library!.rows.at(-1)!.playtime.seconds).toBeNull();
  });

  it('counts providers over the filtered rows but keeps the raw facts', async () => {
    const snapshot = await withFilters({
      ...createDefaultFilters(),
      availability: { installed: true, owned: false, shared: false },
    });
    const steam = snapshot.library!.providers.find((provider) => provider.id === 'steam')!;
    expect(steam.count).toBe(3);
    expect(steam.ownedCount).toBe(5);
    expect(snapshot.library!.rows.every((row) => row.installed)).toBe(true);
  });

  it('shows one removable chip per active facet through the same filters', async () => {
    const filters = {
      ...createDefaultFilters(),
      favoritesOnly: true,
      added: 'last30Days' as const,
    };
    const snapshot = await withFilters(filters);
    expect(snapshot.library!.layout.filters).toEqual(filters);
    expect(snapshot.library!.rows.every((row) => row.isFavorite)).toBe(true);
  });
});

describe('mock PC ports catalog', () => {
  const getSnapshot = async (host: ReturnType<typeof createMockHost>) =>
    (await host.invoke('get_snapshot')) as Snapshot;

  it('starts as a signed-in Windows build with every catalog state represented', async () => {
    const host = createMockHost();
    const appInfo = (await host.invoke('get_app_info')) as AppInfo;
    expect(appInfo.platform).toBe('windows');

    const snapshot = await getSnapshot(host);
    const ports = snapshot.ports;
    expect(ports.catalog.state).toBe('stale');
    expect(ports.catalog.entryCount).toBe(ports.entries.length);

    const bySlug = new Map(ports.entries.map((entry) => [entry.slug, entry]));
    const guided = bySlug.get('aster-vale-revival')!;
    expect(guided.guided).toBe(true);
    expect(guided.requirements[0].required).toBe(true);
    expect(guided.requirements[0].packageOption?.extractorId).toBe('ps4-pkg');
    expect(bySlug.get('beyond-the-pines')?.guided).toBe(false);
    expect(bySlug.get('clockwork-harbor')?.prerelease).toBe(true);
    expect(bySlug.get('distant-skies')?.withdrawn).toBe(true);
    expect(bySlug.get('glass-garden-enhanced')?.noAssetReason).toBe('installerOnly');

    const installed = bySlug.get('emberfall-iron-trail')!;
    expect(installed.installedRegistrationKey).toBe('drmfree:mock-emberfall');
    expect(ports.provenance['drmfree:mock-emberfall'].installedVersion).toBe('1.0.4');

    expect(ports.jobs.some((job) => job.phase === 'failed')).toBe(true);
    expect(ports.unseenCount).toBe(1);
  });

  it('refreshes the cached snapshot when PC ports opens', async () => {
    vi.useFakeTimers();
    try {
      const host = createMockHost();
      let snapshot = await getSnapshot(host);
      expect(snapshot.ports.catalog.state).toBe('stale');

      await host.invoke('ports_open');
      snapshot = await getSnapshot(host);
      expect(snapshot.ports.catalog.state).toBe('refreshing');

      vi.advanceTimersByTime(600);
      snapshot = await getSnapshot(host);
      expect(snapshot.ports.catalog.state).toBe('ready');
      expect(snapshot.ports.catalog.fetchedAt).not.toBeNull();
      expect(snapshot.ports.catalog.error).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('simulates an install with progress events, provenance and the unseen badge', async () => {
    vi.useFakeTimers();
    try {
      const host = createMockHost();
      const progress: PortProgress[] = [];
      const unsubscribe = host.subscribePortProgress((event) => progress.push(event));

      const jobId = (await host.invoke('ports_install', {
        request: {
          catalogId: 'qcat_mock_aster',
          parentFolder: 'D:\\Ports',
          supplied: [
            { slotId: 'game-folder', path: 'D:\\Dumps\\CUSA10101', package: true },
            {
              slotId: 'game-folder',
              path: 'D:\\Dumps\\CUSA10101-update.pkg',
              package: true,
              update: true,
            },
          ],
        },
      })) as string;
      expect(jobId).toMatch(/^job-mock-/);

      let snapshot = await getSnapshot(host);
      expect(snapshot.ports.jobs[0].jobId).toBe(jobId);
      expect(snapshot.ports.unseenCount).toBe(2);
      expect(snapshot.ports.preferences.lastParentFolder).toBe('D:\\Ports');

      vi.advanceTimersByTime(1200);
      expect(
        progress.some(
          (event) =>
            event.jobId === jobId && event.phase === 'downloading' && event.bytesDone > 0,
        ),
      ).toBe(true);

      vi.advanceTimersByTime(1800);
      snapshot = await getSnapshot(host);
      const job = snapshot.ports.jobs.find((item) => item.jobId === jobId)!;
      expect(job.phase).toBe('completed');
      expect(job.outcome).toBe('completed');
      expect(job.seen).toBe(false);
      expect(progress.some((event) => event.jobId === jobId && event.phase === 'hashing')).toBe(
        true,
      );
      expect(progress.some((event) => event.jobId === jobId && event.phase === 'copying')).toBe(
        true,
      );
      expect(
        snapshot.ports.entries.find((entry) => entry.catalogId === 'qcat_mock_aster')
          ?.installedRegistrationKey,
      ).toBe('drmfree:aster-vale-revival');
      expect(
        snapshot.ports.provenance['drmfree:aster-vale-revival'].installedVersion,
      ).toBe('1.4.2');

      await host.invoke('ports_mark_seen', { catalogId: 'qcat_mock_aster' });
      snapshot = await getSnapshot(host);
      expect(snapshot.ports.unseenCount).toBe(1);
      unsubscribe();
    } finally {
      vi.useRealTimers();
    }
  });

  it('cancels a running install without creating a library entry', async () => {
    vi.useFakeTimers();
    try {
      const host = createMockHost();
      const jobId = (await host.invoke('ports_install', {
        request: { catalogId: 'qcat_mock_beyond', parentFolder: 'D:\\Ports', supplied: [] },
      })) as string;

      vi.advanceTimersByTime(500);
      await host.invoke('ports_cancel', { jobId });
      let snapshot = await getSnapshot(host);
      expect(snapshot.ports.jobs.find((job) => job.jobId === jobId)?.phase).toBe('cancelling');

      vi.advanceTimersByTime(300);
      snapshot = await getSnapshot(host);
      const job = snapshot.ports.jobs.find((item) => item.jobId === jobId)!;
      expect(job.phase).toBe('cancelled');
      expect(job.outcome).toBe('cancelled');
      expect(job.retryable).toBe(true);
      expect(snapshot.ports.provenance['drmfree:beyond-the-pines']).toBeUndefined();
    } finally {
      vi.useRealTimers();
    }
  });

  it('fails the Hollow Mountain install with extraction-tool output', async () => {
    vi.useFakeTimers();
    try {
      const host = createMockHost();
      const jobId = (await host.invoke('ports_install', {
        request: {
          catalogId: 'qcat_mock_hollow',
          parentFolder: 'D:\\Ports',
          supplied: [
            { slotId: 'game-package', path: 'D:\\Dumps\\base-CUSA20202.pkg', package: true },
            {
              slotId: 'game-package',
              path: 'D:\\Dumps\\update-CUSA20202.pkg',
              package: true,
              update: true,
            },
          ],
        },
      })) as string;

      vi.advanceTimersByTime(2000);
      const snapshot = await getSnapshot(host);
      const job = snapshot.ports.jobs.find((item) => item.jobId === jobId)!;
      expect(job.phase).toBe('failed');
      expect(job.outcome).toBe('failed');
      expect(job.failure?.message).toContain('extraction tool');
      expect(job.failure?.toolOutput).toContain('encrypted package');
      expect(job.retryable).toBe(true);
      expect(snapshot.ports.provenance['drmfree:hollow-mountain-deep-delve']).toBeUndefined();
    } finally {
      vi.useRealTimers();
    }
  });

  it('refuses a base package that omits the required update', async () => {
    const host = createMockHost();
    await expect(
      host.invoke('ports_install', {
        request: {
          catalogId: 'qcat_mock_hollow',
          parentFolder: 'D:\\Ports',
          supplied: [
            { slotId: 'game-package', path: 'D:\\Dumps\\base.pkg', package: true },
          ],
        },
      }),
    ).rejects.toEqual({
      code: 'installBlocked',
      message: {
        code: 'missingRequiredUpdate',
        slot: 'game-package',
        version: '01.01',
      },
    });
  });

  it('names the package option when a package folder is picked in folder mode', async () => {
    const host = createMockHost();
    const pick = (await host.invoke('ports_pick_supplied', {
      request: {
        setupId: 'setup-1',
        catalogId: 'qcat_mock_hollow',
        slotId: 'game-package',
        kind: 'folder',
      },
    })) as PortSuppliedPick;

    expect(pick.status).toMatchObject({
      rejected: { packagesFound: true },
    });

    const extracted = (await host.invoke('ports_pick_supplied', {
      request: {
        setupId: 'setup-2',
        catalogId: 'qcat_mock_aster',
        slotId: 'game-folder',
        kind: 'folder',
      },
    })) as PortSuppliedPick;
    expect(extracted.status).toBe('accepted');
  });

  it('exposes a fictional helper tool and refuses the cache clear during an install', async () => {
    vi.useFakeTimers();
    try {
      const host = createMockHost();
      let cache = (await host.invoke('ports_tools_cache')) as PortToolsCacheView;
      expect(cache.downloadsBytes).toBeGreaterThan(0);
      expect(cache.helperTools).toHaveLength(1);
      expect(cache.helperTools[0].name).toBe('Fixture PKG extractor');
      expect(cache.helperTools[0].license).toBe('GPL-3.0');
      expect(cache.installActive).toBe(false);

      const jobId = (await host.invoke('ports_install', {
        request: { catalogId: 'qcat_mock_beyond', parentFolder: 'D:\\Ports', supplied: [] },
      })) as string;
      expect(jobId).toMatch(/^job-mock-/);
      cache = (await host.invoke('ports_tools_cache')) as PortToolsCacheView;
      expect(cache.installActive).toBe(true);
      await expect(host.invoke('ports_clear_cache')).rejects.toEqual({
        code: 'installInProgress',
      });

      vi.advanceTimersByTime(3000);
      cache = (await host.invoke('ports_clear_cache')) as PortToolsCacheView;
      expect(cache.installActive).toBe(false);
      expect(cache.downloadsBytes).toBe(0);
      expect(cache.helperTools[0].cacheBytes).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it('accepts picks, remembers preferences and exposes the unavailable scenario', async () => {    const host = createMockHost();
    await host.invoke('ports_set_preferences', {
      patch: { view: 'list', filters: { guided: true }, lastParentFolder: 'D:\\Games' },
    });
    let snapshot = await getSnapshot(host);
    expect(snapshot.ports.preferences.view).toBe('list');
    expect(snapshot.ports.preferences.filters.guided).toBe(true);

    expect(await host.invoke('ports_pick_parent')).toBe('D:\\Games');
    const pick = (await host.invoke('ports_pick_supplied', {
      request: {
        setupId: 'setup-1',
        slotId: 'game-folder',
        kind: 'folder',
      },
    })) as PortSuppliedPick;
    expect(pick.status).toBe('accepted');
    expect(pick.path).toContain('game-folder');
    expect(await host.invoke('ports_pick_executable', { jobId: 'job-mock-failed' })).toContain(
      '.exe',
    );

    const unavailable = createMockHost({ portsScenario: 'unavailable', platform: 'linux' });
    snapshot = await getSnapshot(unavailable);
    expect((await unavailable.invoke('get_app_info') as AppInfo).platform).toBe('linux');
    expect(snapshot.ports.catalog.state).toBe('unavailable');
    expect(snapshot.ports.catalog.error).toBe('unreachable');
    expect(snapshot.ports.entries).toHaveLength(0);

    vi.useFakeTimers();
    try {
      await unavailable.invoke('ports_refresh');
      vi.advanceTimersByTime(600);
      snapshot = await getSnapshot(unavailable);
      expect(snapshot.ports.catalog.state).toBe('unavailable');
    } finally {
      vi.useRealTimers();
    }
  });
});

const QUERY_NOW = 10_000_000;
const QUERY_DAY = 86_400;
const queryProviders: ProviderRef[] = [
  { id: 'steam', name: 'Steam' },
  { id: 'gog', name: 'GOG' },
  { id: 'epic', name: 'Epic Games Store' },
];

function queryRow(overrides: Partial<Row> & Pick<Row, 'key' | 'title'>): Row {
  return {
    detectedName: overrides.title,
    matchedName: overrides.title,
    provider: 'steam',
    providerName: 'Steam',
    installed: false,
    owned: true,
    shared: false,
    running: false,
    status: { matched: { gameId: 1, source: 'automatic' } },
    coverFile: null,
    playtime: { seconds: null, source: 'store', lastPlayedAt: null },
    releaseYear: null,
    addedAt: QUERY_NOW,
    isFavorite: false,
    hidden: false,
    gamenode: null,
    ...overrides,
  };
}

function queryRows(): Row[] {
  return [
    queryRow({
      key: 'steam:1',
      title: 'Alpha',
      installed: true,
      running: true,
      isFavorite: true,
      playtime: { seconds: 12 * 3600, source: 'store', lastPlayedAt: QUERY_NOW - QUERY_DAY },
      releaseYear: 2021,
      addedAt: QUERY_NOW - QUERY_DAY,
      gamenode: {
        status: 'playing',
        collectionIds: ['backlog'],
        entryId: 'entry-1',
      },
    }),
    queryRow({
      key: 'steam:2',
      title: 'Bravo',
      installed: false,
      owned: true,
      status: 'unmatched',
      playtime: { seconds: 0, source: 'store', lastPlayedAt: null },
      releaseYear: 1995,
      addedAt: QUERY_NOW - 100 * QUERY_DAY,
      gamenode: { status: null, collectionIds: [], entryId: 'entry-2' },
    }),
    queryRow({
      key: 'gog:3',
      title: 'Charlie',
      provider: 'gog',
      providerName: 'GOG',
      installed: false,
      owned: false,
      shared: true,
      isFavorite: true,
      status: 'ambiguous',
      releaseYear: null,
      addedAt: QUERY_NOW - 40 * QUERY_DAY,
      gamenode: { status: 'finished', collectionIds: [], entryId: 'entry-3' },
    }),
    queryRow({
      key: 'epic:4',
      title: 'Delta',
      provider: 'epic',
      providerName: 'Epic Games Store',
      status: 'noGame',
      playtime: { seconds: 101 * 3600, source: 'store', lastPlayedAt: QUERY_NOW - 500 * QUERY_DAY },
      releaseYear: 1985,
      addedAt: QUERY_NOW - 400 * QUERY_DAY,
      gamenode: { status: 'planned', collectionIds: [], entryId: 'entry-4' },
    }),
  ];
}

function query(
  rows: Row[],
  filters: LibraryFilters,
  overrides: Partial<LibraryLayout> = {},
  gamenodeAvailable = true,
) {
  return runLibraryQuery({
    rows,
    layout: { ...createDefaultLibraryLayout(), filters, ...overrides },
    search: '',
    now: QUERY_NOW,
    providers: queryProviders,
    gamenodeAvailable,
  });
}

describe('mock D3 query pipeline', () => {
  it('matches every facet option count against filtering with that option alone', () => {
    const rows = queryRows();
    const base: LibraryFilters = {
      ...createDefaultFilters(),
      stores: ['steam', 'gog'],
      favoritesOnly: true,
    };
    const counts = query(rows, base).filterCounts!;
    const visibleWith = (change: (filters: LibraryFilters) => void) => {
      const filters = structuredClone(base);
      change(filters);
      return query(rows, filters).visibleCount;
    };

    expect(counts.availability.installed).toBe(
      visibleWith((filters) => {
        filters.availability = { installed: true, owned: false, shared: false };
      }),
    );
    expect(counts.availability.shared).toBe(
      visibleWith((filters) => {
        filters.availability = { installed: false, owned: false, shared: true };
      }),
    );
    for (const [count, state] of [
      [counts.matchStates.matched, 'matched'],
      [counts.matchStates.ambiguous, 'ambiguous'],
      [counts.matchStates.unmatched, 'unmatched'],
      [counts.matchStates.noGame, 'noGame'],
    ] as const) {
      expect(count).toBe(visibleWith((filters) => { filters.matchStates = [state]; }));
    }
    for (const [count, status] of [
      [counts.gamenodeStatus.playing, 'playing'],
      [counts.gamenodeStatus.finished, 'finished'],
      [counts.gamenodeStatus.planned, 'planned'],
      [counts.gamenodeStatus.inLibrary, 'inLibrary'],
      [counts.gamenodeStatus.notInLibrary, 'notInLibrary'],
    ] as const) {
      expect(count).toBe(visibleWith((filters) => { filters.gamenodeStatus = [status]; }));
    }
    expect(counts.playtime.atLeast10h).toBe(
      visibleWith((filters) => { filters.playtime = 'atLeast10h'; }),
    );
    expect(counts.playtime.atLeast100h).toBe(
      visibleWith((filters) => { filters.playtime = 'atLeast100h'; }),
    );
    expect(counts.played.played).toBe(
      visibleWith((filters) => { filters.played = 'played'; }),
    );
    expect(counts.played.unplayed).toBe(
      visibleWith((filters) => { filters.played = 'unplayed'; }),
    );
    expect(counts.lastPlayed.last7Days).toBe(
      visibleWith((filters) => { filters.lastPlayed = 'last7Days'; }),
    );
    expect(counts.lastPlayed.olderThanYear).toBe(
      visibleWith((filters) => { filters.lastPlayed = 'olderThanYear'; }),
    );
    expect(counts.added.last30Days).toBe(
      visibleWith((filters) => { filters.added = 'last30Days'; }),
    );
    expect(counts.release).toEqual(
      expect.arrayContaining([
        { value: { decade: 2020 }, count: visibleWith((filters) => { filters.release = { decade: 2020 }; }) },
        { value: 'unknown', count: visibleWith((filters) => { filters.release = 'unknown'; }) },
      ]),
    );
    expect(counts.favoritesOnly).toBe(
      visibleWith((filters) => { filters.favoritesOnly = true; }),
    );
    expect(counts.runningOnly).toBe(
      visibleWith((filters) => { filters.runningOnly = true; }),
    );
  });

  it('counts stores without the store facet or scope and keeps scope counts', () => {
    const rows = queryRows();
    const filters = { ...createDefaultFilters(), stores: ['steam'], favoritesOnly: true };
    const result = query(rows, filters, { scope: { collection: 'backlog' } });
    const withoutStores = { ...filters, stores: [] };
    const all = query(rows, withoutStores, { scope: 'all' });
    expect(result.filterCounts!.stores.reduce((sum, store) => sum + store.count, 0)).toBe(
      result.scopeCounts!.all,
    );
    expect(result.scopeCounts!.all).toBe(all.visibleCount);
    expect(result.filterCounts!.stores.find((store) => store.id === 'gog')!.count).toBe(1);

    for (const scope of ['installed', 'favorites', 'recent', 'running'] as const) {
      expect(result.scopeCounts![scope]).toBe(
        query(rows, withoutStores, { scope }).visibleCount,
      );
    }
  });

  it('excludes hidden rows until Show hidden and reports the hidden count', () => {
    const rows = queryRows();
    rows[0].hidden = true;
    const hidden = rows[0].key;
    const filters = createDefaultFilters();
    const result = query(rows, filters);
    expect(result.rows.map((row) => row.key)).not.toContain(hidden);
    expect(result.filterCounts!.showHidden).toBe(1);
    expect(result.filterCounts!.availability.installed).toBe(0);
    expect(result.total).toBe(3);

    const shown = query(rows, { ...filters, showHidden: true });
    expect(shown.rows.map((row) => row.key)).toContain(hidden);
    expect(shown.filterCounts!.availability.installed).toBe(1);
    expect(shown.filterCounts!.showHidden).toBe(1);
    expect(shown.total).toBe(4);
  });

  it('uses the 14-day Recent boundary', () => {
    const rows = queryRows();
    rows[0].playtime = { seconds: 1, source: 'store', lastPlayedAt: QUERY_NOW - 13 * QUERY_DAY };
    rows[1].playtime = { seconds: 1, source: 'store', lastPlayedAt: QUERY_NOW - 15 * QUERY_DAY };
    const result = query(rows, createDefaultFilters(), { scope: 'recent' });
    expect(result.rows.map((row) => row.key)).toEqual(['steam:1']);
  });

  it('keeps every grouping in its fixed order and omits empty groups', () => {
    const rows = queryRows();
    const sorted = sortRows(rows, { key: 'title', direction: 'ascending' });
    const expected: Array<[Parameters<typeof groupRows>[1], string[]]> = [
      ['store', ['steam', 'gog', 'epic']],
      ['gameNodeStatus', ['playing', 'finished', 'planned', 'inLibrary']],
      ['matchState', ['matched', 'ambiguous', 'unmatched', 'noGame']],
      ['installation', ['installed', 'notInstalled']],
      ['releaseDecade', ['2020', '1990', 'earlier', 'unknown']],
    ];
    for (const [group, ids] of expected) {
      const result = groupRows(sorted, group, [], queryProviders);
      expect(result.groups.map((item) => item.id)).toEqual(ids);
    }
    const onlyPlaying = groupRows(
      sorted.map((row) => ({ ...row, gamenode: { status: 'playing' as const, collectionIds: [], entryId: 'e' } })),
      'gameNodeStatus',
      [],
      queryProviders,
    );
    expect(onlyPlaying.groups.map((item) => item.id)).toEqual(['playing']);
  });

  it('drops collapsed group rows but keeps their headers and counts', () => {
    const sorted = sortRows(queryRows(), { key: 'title', direction: 'ascending' });
    const collapsed = groupRows(sorted, 'store', ['steam'], queryProviders);
    expect(collapsed.rows.map((row) => row.key)).toEqual(['gog:3', 'epic:4']);
    expect(collapsed.groups[0]).toMatchObject({
      id: 'steam',
      count: 2,
      collapsed: true,
      firstIndex: 0,
    });
  });

  it('sorts GameNode status with the sort rank, not the group order', () => {
    const rows = queryRows();
    rows[0].gamenode = { status: 'planned', collectionIds: [], entryId: '1' };
    rows[1].gamenode = { status: 'ongoing', collectionIds: [], entryId: '2' };
    rows[2].gamenode = { status: 'playing', collectionIds: [], entryId: '3' };
    rows[3].gamenode = { status: 'dropped', collectionIds: [], entryId: '4' };
    expect(
      sortRows(rows, { key: 'gameNodeStatus', direction: 'ascending' }).map((row) => row.gamenode?.status),
    ).toEqual(['playing', 'ongoing', 'planned', 'dropped']);
  });

  it('mirrors the new inputs through the host: grouping, collapse, select all and clear', async () => {
    const host = createMockHost();
    await host.invoke('library_input', { input: { setGroup: 'store' } });
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.groups!.map((group) => group.id)).toEqual(['steam', 'gog', 'epic']);
    expect(snapshot.library!.titleSource).toEqual({ kind: 'scope', id: 'all' });

    // Each group starts a new line in the grid; Down crosses into GOG's line from Steam's last.
    await host.invoke('library_input', {
      input: { select: { key: 'steam:11', mode: 'replace' } },
    });
    await host.invoke('library_input', { input: { setColumns: 2 } });
    await host.invoke('library_input', {
      input: { move: { direction: 'down', extend: false } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.selected).toEqual(['gog:4']);

    // Down crosses into GOG's line, and a collapsed GOG group is skipped for the Epic group.
    await host.invoke('library_input', { input: { toggleGroup: { id: 'gog' } } });
    await host.invoke('library_input', {
      input: { select: { key: 'steam:11', mode: 'replace' } },
    });
    await host.invoke('library_input', {
      input: { move: { direction: 'down', extend: false } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.selected).toEqual(['epic:8']);
    await host.invoke('library_input', { input: { toggleGroup: { id: 'gog' } } });

    await host.invoke('library_input', { input: { toggleGroup: { id: 'steam' } } });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.groups!.find((group) => group.id === 'steam')!.collapsed).toBe(true);
    expect(snapshot.library!.rows.every((row) => row.provider !== 'steam')).toBe(true);

    await host.invoke('library_input', { input: 'selectAll' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.selected).toHaveLength(snapshot.library!.rows.length);
    expect(snapshot.library!.selected.some((key) => key.startsWith('steam:'))).toBe(false);

    await host.invoke('library_input', { input: 'clearSelection' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.selected).toEqual([]);
    expect(snapshot.library!.focused).toBeNull();

    await host.invoke('library_input', { input: { setAllGroupsCollapsed: false } });
    await host.invoke('library_input', { input: { setScope: 'favorites' } });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.titleSource).toEqual({ kind: 'scope', id: 'favorites' });
    expect(snapshot.library!.rows.every((row) => row.isFavorite)).toBe(true);

    await host.invoke('library_input', { input: { setScope: 'all' } });
    await host.invoke('library_input', { input: { setFilters: { ...createDefaultFilters(), stores: ['gog'] } } });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.titleSource).toEqual({ kind: 'store', id: 'gog' });
    expect(snapshot.library!.rows.every((row) => row.provider === 'gog')).toBe(true);
  });

  it('starts from one saved view and one hidden entry, with counts that agree', async () => {
    const host = createMockHost();
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const lib = snapshot.library!;
    expect(lib.canSaveView).toBe(false);
    expect(lib.savedViews).toHaveLength(1);
    expect(lib.savedViews![0]).toMatchObject({
      id: 1,
      name: 'Steam 10 h+',
      active: false,
    });
    expect(lib.savedViews![0].count).toBe(1);
    expect(lib.collections).toEqual(
      MOCK_COLLECTIONS.map((collection) => ({
        ...collection,
        count: expect.any(Number),
      })),
    );
    expect(lib.filterCounts!.showHidden).toBe(1);
    expect(lib.rows.some((row) => row.key === MOCK_HIDDEN_KEY)).toBe(false);

    // The hidden fixture is in the row data but stays out of the normal view.
    await host.invoke('library_input', {
      input: {
        setFilters: { ...lib.layout.filters, showHidden: true },
      },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.rows).toHaveLength(13);
    const hidden = snapshot.library!.rows.find((row) => row.key === MOCK_HIDDEN_KEY)!;
    expect(hidden.hidden).toBe(true);
    expect(snapshot.library!.filterCounts!.showHidden).toBe(1);
  });

  it('saves, applies, renames and deletes a view with live name validation', async () => {
    const host = createMockHost();
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const filters = snapshot.library!.layout.filters;

    await host.invoke('library_input', {
      input: { setFilters: { ...filters, stores: ['epic'] } },
    });
    await host.invoke('library_input', {
      input: { setSort: { key: 'lastPlayed', direction: 'descending' } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.canSaveView).toBe(true);

    await host.invoke('library_input', {
      input: { saveView: { name: '  Epic plays  ' } },
    });
    await waitForViewWrite(host, 'succeeded');
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.viewError).toBeNull();
    expect(snapshot.library!.savedViews).toHaveLength(2);
    expect(snapshot.library!.savedViews![1].name).toBe('Epic plays');
    expect(snapshot.library!.lastSavedView).toMatchObject({
      id: snapshot.library!.savedViews![1].id,
      name: 'Epic plays',
      run: expect.any(Number),
    });

    await host.invoke('library_input', {
      input: { saveView: { name: 'epic PLAYS' } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.viewError).toBe('duplicate');
    expect(snapshot.library!.savedViews).toHaveLength(2);

    await host.invoke('library_input', { input: { saveView: { name: '   ' } } });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.viewError).toBe('empty');

    // Applying the fixture view highlights it and uses its own store and playtime predicates.
    await host.invoke('library_input', { input: { applyView: { id: 1 } } });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.layout.scope).toEqual({ savedView: 1 });
    expect(snapshot.library!.titleSource).toEqual({ kind: 'savedView', id: '1' });
    expect(snapshot.library!.savedViews![0].active).toBe(true);
    expect(snapshot.library!.rows).toHaveLength(snapshot.library!.savedViews![0].count);
    expect(snapshot.library!.rows.every((row) => row.provider === 'steam')).toBe(true);

    // Changing the sort leaves the view but keeps the payload's own (All) scope.
    await host.invoke('library_input', {
      input: { setSort: { key: 'title', direction: 'ascending' } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.layout.scope).toBe('all');
    expect(snapshot.library!.titleSource).toEqual({ kind: 'store', id: 'steam' });
    expect(snapshot.library!.savedViews![0].active).toBe(false);
    expect(snapshot.library!.layout.filters.stores).toEqual(['steam']);

    // A rename updates the sidebar without touching the payload.
    await host.invoke('library_input', {
      input: { renameView: { id: 2, name: 'Epic backlog' } },
    });
    await waitForViewWrite(host, 'succeeded');
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.savedViews!.map((view) => view.name)).toEqual([
      'Steam 10 h+',
      'Epic backlog',
    ]);
    await host.invoke('library_input', {
      input: { renameView: { id: 2, name: 'STEAM 10 H+' } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.viewError).toBe('duplicate');

    // Deleting the active view keeps its filters but drops the broken marker.
    await host.invoke('library_input', { input: { applyView: { id: 2 } } });
    await host.invoke('library_input', { input: { deleteView: { id: 2 } } });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.savedViews!.map((view) => view.id)).toEqual([1]);
    expect(snapshot.library!.layout.scope).toBe('all');
    expect(snapshot.library!.layout.filters.stores).toEqual(['epic']);
    expect(snapshot.library!.titleSource).toEqual({ kind: 'store', id: 'epic' });
  });

  it('hides and reveals rows, prunes the selection and keeps the undo run', async () => {
    const host = createMockHost();
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const key = snapshot.library!.rows[0].key;
    const before = snapshot.library!.scopeCounts!.all;

    await host.invoke('library_input', {
      input: { select: { key, mode: 'replace' } },
    });
    await host.invoke('library_input', {
      input: { setHidden: { keys: [key], hidden: true } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.rows.some((row) => row.key === key)).toBe(false);
    expect(snapshot.library!.scopeCounts!.all).toBe(before - 1);
    expect(snapshot.library!.filterCounts!.showHidden).toBe(2);
    expect(snapshot.library!.selected).toEqual([]);
    expect(snapshot.library!.lastHidden).toMatchObject({
      keys: [key],
      hidden: true,
      run: expect.any(Number),
    });

    // Show hidden reveals it dimmed; Show in library keeps it after Show hidden is off.
    await host.invoke('library_input', {
      input: {
        setFilters: { ...snapshot.library!.layout.filters, showHidden: true },
      },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.rows.find((row) => row.key === key)?.hidden).toBe(true);
    await host.invoke('library_input', {
      input: { setHidden: { keys: [key], hidden: false } },
    });
    await host.invoke('library_input', {
      input: {
        setFilters: { ...snapshot.library!.layout.filters, showHidden: false },
      },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.rows.some((row) => row.key === key)).toBe(true);
    expect(snapshot.library!.filterCounts!.showHidden).toBe(1);
  });

  it('keeps saved views and hidden entries for the same mock account across sign-out', async () => {
    const first = createMockHost();
    let snapshot = (await first.invoke('get_snapshot')) as Snapshot;
    await first.invoke('library_input', {
      input: { setSort: { key: 'title', direction: 'descending' } },
    });
    await first.invoke('library_input', {
      input: { saveView: { name: 'Mine' } },
    });
    await waitForViewWrite(first, 'succeeded');
    snapshot = (await first.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.savedViews).toHaveLength(2);

    // A new host is an isolated account fixture: the previous host's changes never leak in.
    const second = createMockHost();
    snapshot = (await second.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.savedViews).toHaveLength(1);

    // Signing out and back in is the same account: views and hidden keys stay.
    await first.invoke('dispatch_auth', { input: 'signOut' });
    snapshot = (await first.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library).toBeNull();
    await first.invoke('dispatch_auth', { input: { startSignIn: 'google' } });
    snapshot = (await first.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.savedViews).toHaveLength(2);
    expect(snapshot.library!.filterCounts!.showHidden).toBe(1);
  });
});

describe('mock GameNode batch writes', () => {
  it('adds matched games, keeps the summary after close and undoes only created entries', async () => {
    const host = createMockHost();
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.rows.find((row) => row.key === 'epic:8')!.gamenode).toBeNull();

    await host.invoke('library_input', { input: { select: { key: 'epic:8', mode: 'toggle' } } });
    await host.invoke('library_input', { input: { select: { key: 'steam:3', mode: 'toggle' } } });
    await host.invoke('library_input', { input: 'openAdd' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.addDialog!.targets).toEqual([
      {
        target: { row: 'steam:3' },
        entryKey: 'steam:3',
        title: 'Clockwork Harbor',
        coverFile: 'mock-2.svg',
        matched: false,
      },
      {
        target: { row: 'epic:8' },
        entryKey: 'epic:8',
        title: 'Hollow Mountain',
        coverFile: 'mock-7.svg',
        matched: true,
      },
    ]);

    await host.invoke('library_input', { input: 'confirmAdd' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    const summary = snapshot.library!.addSummary!;
    expect(summary).toMatchObject({ added: 1, skipped: 1, undoable: true });
    expect(
      snapshot.library!.rows.find((row) => row.key === 'epic:8')!.gamenode,
    ).toMatchObject({ status: 'playing', collectionIds: [] });

    await host.invoke('library_input', { input: 'closeAdd' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.addDialog).toBeNull();
    expect(snapshot.library!.addSummary!.runId).toBe(summary.runId);

    await host.invoke('library_input', { input: { undoAdd: { run: summary.runId } } });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.undo).toEqual({
      run: summary.runId,
      state: 'done',
      deleted: 1,
      failed: 0,
    });
    expect(snapshot.library!.addSummary!.undoable).toBe(false);
    expect(
      snapshot.library!.rows.find((row) => row.key === 'epic:8')!.gamenode,
    ).toBeNull();
  });

  it('applies batch status, collection and favorite changes with skip reasons', async () => {
    const host = createMockHost();
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.rows.find((row) => row.key === 'epic:6')!.gamenode!.status).toBeNull();

    await host.invoke('library_input', {
      input: {
        setStatus: {
          keys: ['steam:1', 'epic:6', 'epic:8', 'steam:3'],
          status: 'finished',
        },
      },
    });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.statusRun).toEqual({
      run: 1,
      status: 'finished',
      changed: 1,
      skipped: 3,
      failed: 0,
      skippedUnmatched: 1,
      skippedNotInLibrary: 1,
      skippedUnknownStatus: 1,
    });
    expect(
      snapshot.library!.rows.find((row) => row.key === 'steam:1')!.gamenode!.status,
    ).toBe('finished');

    await host.invoke('library_input', {
      input: { addToCollection: { keys: ['gog:2'], collectionId: 'backlog-curto' } },
    });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.collectionRun).toMatchObject({
      changed: 1,
      collectionName: 'Backlog curto',
      skipped: 0,
      failed: 0,
    });
    expect(
      snapshot.library!.rows.find((row) => row.key === 'gog:2')!.gamenode,
    ).toMatchObject({
      status: 'finished',
      collectionIds: ['coop-no-sofa', 'backlog-curto'],
    });

    await host.invoke('library_input', {
      input: { setFavorites: { keys: ['steam:1', 'steam:3'], favorite: true, now: 1 } },
    });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.rows.find((row) => row.key === 'steam:1')!.isFavorite).toBe(true);
    expect(snapshot.library!.rows.find((row) => row.key === 'steam:3')!.isFavorite).toBe(false);
    await host.invoke('library_input', {
      input: { setFavorites: { keys: ['steam:1'], favorite: false, now: 2 } },
    });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.rows.find((row) => row.key === 'steam:1')!.isFavorite).toBe(false);
  });

  it('creates a collection inline, selects it and reports the account limit', async () => {
    const host = createMockHost();
    await host.invoke('library_input', {
      input: { select: { key: 'epic:8', mode: 'replace' } },
    });
    await host.invoke('library_input', { input: 'openAdd' });
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.addDialog!.collections).toMatchObject({
      loaded: MOCK_COLLECTIONS.map((collection) => ({
        id: collection.id,
        name: collection.name,
        picked: false,
      })),
    });

    await host.invoke('library_input', { input: 'startNewCollection' });
    await host.invoke('library_input', {
      input: { setNewCollectionName: 'Coop de domingo' },
    });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.addDialog!.newCollection).toMatchObject({
      name: 'Coop de domingo',
      creating: false,
      canCreate: true,
      error: null,
    });

    await host.invoke('library_input', { input: 'createCollection' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    const dialog = snapshot.library!.addDialog!;
    expect(dialog.newCollection).toBeNull();
    const created = (dialog.collections as { loaded: Array<{ id: string; name: string; picked: boolean; entryCount: number | null }> }).loaded.find(
      (collection) => collection.name === 'Coop de domingo',
    )!;
    expect(created.picked).toBe(true);
    expect(created.entryCount).toBe(0);
    // Creating a collection writes no entry and adds no game.
    expect(snapshot.library!.addSummary).toBeNull();
    expect(
      snapshot.library!.rows.find((row) => row.key === 'epic:8')!.gamenode,
    ).toBeNull();

    await host.invoke('library_input', { input: 'startNewCollection' });
    await host.invoke('library_input', {
      input: { setNewCollectionName: MOCK_LIMIT_COLLECTION_NAME },
    });
    await host.invoke('library_input', { input: 'createCollection' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.addDialog!.newCollection).toMatchObject({
      name: MOCK_LIMIT_COLLECTION_NAME,
      error: 'limitReached',
      canCreate: true,
    });
    expect(
      snapshot.library!.addDialog!.collections,
    ).toMatchObject({ loaded: expect.any(Array) });
  });

  it('keeps a hidden three-game batch adding with one unmatched game', async () => {
    vi.useFakeTimers();
    try {
      const host = createMockHost({ addBatchDelayMs: 10 });
      for (const key of ['steam:3', 'steam:1', 'epic:8']) {
        await host.invoke('library_input', {
          input: { select: { key, mode: key === 'steam:3' ? 'replace' : 'toggle' } },
        });
      }
      await host.invoke('library_input', { input: 'openAdd' });
      await host.invoke('library_input', { input: 'confirmAdd' });
      let snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.addDialog!.progress).toMatchObject({ total: 3 });
      expect(snapshot.library!.addDialog!.items).toHaveLength(3);

      await host.invoke('library_input', { input: 'hideAdd' });
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.addDialog).toBeNull();
      expect(snapshot.library!.addSummary).toBeNull();

      await vi.advanceTimersByTimeAsync(100);
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      const summary = snapshot.library!.addSummary!;
      expect(summary).toMatchObject({
        added: 1,
        alreadyPresent: 1,
        skipped: 1,
        undoable: true,
      });
      const runId = summary.runId;
      expect(
        snapshot.library!.rows.find((row) => row.key === 'epic:8')!.gamenode,
      ).not.toBeNull();

      // The notification is raised once: a late hide or a second confirm cannot re-run it.
      await host.invoke('library_input', { input: 'hideAdd' });
      await host.invoke('library_input', { input: 'confirmAdd' });
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.addSummary!.runId).toBe(runId);
      expect(snapshot.library!.addSummary!.added).toBe(1);

      // A new Add after the run finished starts a fresh dialog instead of the old progress.
      await host.invoke('library_input', { input: 'openAdd' });
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.addDialog!.summary).toBeNull();
      expect(snapshot.library!.addDialog!.progress).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('keeps a hidden batch outcome after sign-out in the notice channel', async () => {
    vi.useFakeTimers();
    try {
      const host = createMockHost({ addBatchDelayMs: 10 });
      for (const key of ['steam:3', 'steam:1', 'epic:8']) {
        await host.invoke('library_input', {
          input: { select: { key, mode: key === 'steam:3' ? 'replace' : 'toggle' } },
        });
      }
      await host.invoke('library_input', { input: 'openAdd' });
      await host.invoke('library_input', { input: 'confirmAdd' });
      await host.invoke('library_input', { input: 'hideAdd' });
      await host.invoke('dispatch_auth', { input: 'signOut' });

      let snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library).toBeNull();
      const notice = snapshot.addNotice!;
      expect(notice).toMatchObject({
        accountId: 'mock-user',
        batch: true,
        summary: { added: 0, failed: 3 },
      });
      const runId = notice.summary.runId;

      // The stopped timer cannot add anything after sign-out or change the reported counts.
      await vi.advanceTimersByTimeAsync(100);
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.addNotice!.summary.runId).toBe(runId);
      expect(snapshot.addNotice!.summary).toMatchObject({ added: 0, failed: 3 });
      expect(snapshot.library).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('opens game tools through the usual action and mirrors picker, anti-cheat and install', async () => {
    const host = createMockHost();

    await host.invoke('library_input', {
      input: { select: { key: 'steam:3', mode: 'replace' } },
    });
    await host.invoke('open_tools_for_selected');
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    let modal = snapshot.tools.modal!;
    expect(modal).toMatchObject({
      entryKey: 'steam:3',
      title: 'Clockwork Harbor',
      activeToolId: 'optiscaler',
      selectedChannel: 'stable',
    });
    expect(modal.availableVersions.map((release) => release.version)).toEqual([
      '0.7.9',
      '0.7.2',
    ]);
    expect(modal.inspection!.candidates.length).toBeGreaterThan(0);
    expect(modal.inspection!.anticheat.detected).toEqual([]);
    expect(snapshot.tools.catalog[0].name).toBe('OptiScaler');

    await host.invoke('select_tool_channel', { channel: 'nightly' });
    await host.invoke('select_tool_version', { version: '0.8.0-pre1' });
    await host.invoke('select_tool_proxy', { proxy: 'version' });
    await host.invoke('select_tool_target_exe', {
      relativeExe: 'bin/x64/Launcher.exe',
    });
    await host.invoke('set_tool_reset_config', { reset: true });
    await host.invoke('pick_tool_target_exe');
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    modal = snapshot.tools.modal!;
    expect(modal.selectedChannel).toBe('nightly');
    expect(modal.selectedVersion).toBe('0.8.0-pre1');
    expect(modal.selectedProxy).toBe('version');
    expect(modal.inspection!.selectedExe).toBe('bin/x64/picked.exe');

    // The clean game installs, clears cached archives, and uninstalls without an anti-cheat confirmation.
    await host.invoke('install_or_update_tool', { isUpdate: false });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.installedByEntry['steam:3']).toHaveLength(1);
    expect(snapshot.tools.installedByEntry['steam:3'][0]).toMatchObject({
      version: '0.8.0-pre1',
      proxyDll: 'version.dll',
      targetExe: 'bin/x64/picked.exe',
    });
    await host.invoke('clear_tool_cache');
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.totalCacheBytes).toBe(0);
    expect(snapshot.tools.achievementIconCacheBytes).toBe(0);
    expect(snapshot.tools.totalCacheArchives).toBe(0);
    await host.invoke('uninstall_tool');
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.installedByEntry['steam:3']).toBeUndefined();

    // The anti-cheat fixture reaches the danger callout and blocks install until confirmed.
    await host.invoke('open_tools', { entryKey: 'steam:9' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    modal = snapshot.tools.modal!;
    expect(modal.inspection!.anticheat.detected).toEqual(['easyAntiCheat']);
    expect(modal.anticheatConfirmed).toBe(false);
    await host.invoke('install_or_update_tool', { isUpdate: false });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.installedByEntry['steam:9']).toBeUndefined();
    expect(snapshot.tools.modal!.operation).toMatchObject({
      failed: { message: expect.stringContaining('Anti-cheat') },
    });

    await host.invoke('set_tool_anticheat_confirmed', { confirmed: true });
    await host.invoke('install_or_update_tool', { isUpdate: false });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.modal!.anticheatConfirmed).toBe(true);
    expect(snapshot.tools.modal!.operation).toBeNull();
    expect(snapshot.tools.installedByEntry['steam:9']).toHaveLength(1);
  });

  it('lists cached tool versions and preselects the configured default channel', async () => {
    const host = createMockHost();
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.cacheEntries).toHaveLength(3);
    expect(snapshot.tools.installedByEntry['epic:6']).toHaveLength(1);
    expect(snapshot.tools.installedByEntry['epic:8']).toHaveLength(1);
    expect(snapshot.tools.cacheEntries.find((entry) => entry.version === '0.7.9'))
      .toMatchObject({ channel: 'stable', sha256Verified: true, inUse: true });
    expect(snapshot.tools.cacheEntries.find((entry) => entry.version === '0.7.2'))
      .toMatchObject({ channel: 'stable', sha256Verified: false, inUse: false });
    expect(snapshot.tools.cacheEntries.find((entry) => entry.version === '0.8.0-pre1'))
      .toMatchObject({ channel: 'nightly', sha256Verified: true });
    expect(snapshot.tools.totalCacheBytes).toBe(
      snapshot.tools.cacheEntries.reduce((total, entry) => total + entry.bytes, 0),
    );

    await host.invoke('open_tools', { entryKey: 'steam:3' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.modal!.selectedChannel).toBe('stable');

    await host.invoke('set_app_preference', {
      patch: { toolDefaultChannel: 'nightly' },
    });
    await host.invoke('open_tools', { entryKey: 'steam:3' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.modal!.selectedChannel).toBe('nightly');
    expect(snapshot.tools.modal!.selectedVersion).toBe('0.8.0-pre1');

    // Installing marks the cached version in use; clearing empties the listing.
    await host.invoke('install_or_update_tool', { isUpdate: false });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.cacheEntries.find((entry) => entry.version === '0.8.0-pre1')?.inUse)
      .toBe(true);
    await host.invoke('clear_tool_cache');
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.cacheEntries).toEqual([]);
  });

  it('keeps the target on picker cancel and reports picker or outside-folder failures', async () => {
    const canceled = createMockHost({ toolPick: 'cancel' });
    await canceled.invoke('open_tools', { entryKey: 'steam:3' });
    let snapshot = await canceled.invoke('get_snapshot') as Snapshot;
    const initial = snapshot.tools.modal!.inspection!.selectedExe;
    expect(initial).toBeTruthy();
    await canceled.invoke('pick_tool_target_exe');
    snapshot = await canceled.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.modal!.inspection!.selectedExe).toBe(initial);
    expect(snapshot.tools.modal!.operation).toBeNull();

    const failing = createMockHost({ toolPick: 'fail' });
    await failing.invoke('open_tools', { entryKey: 'steam:3' });
    await failing.invoke('pick_tool_target_exe');
    snapshot = await failing.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.modal!.inspection!.selectedExe).toBe(initial);
    expect(snapshot.tools.modal!.operation).toMatchObject({
      failed: {
        code: 'pickerFailed',
        message: expect.stringContaining('could not open'),
      },
    });

    const outside = createMockHost({ toolPick: 'outside' });
    await outside.invoke('open_tools', { entryKey: 'steam:3' });
    await outside.invoke('pick_tool_target_exe');
    snapshot = await outside.invoke('get_snapshot') as Snapshot;
    expect(snapshot.tools.modal!.inspection!.selectedExe).toBe(initial);
    expect(snapshot.tools.modal!.operation).toMatchObject({
      failed: {
        code: 'targetOutsideInstallDir',
      },
    });
  });

  it('serves the first-sync explanation only when opted in and applies keep-on and turn-off in order', async () => {
    const regular = createMockHost();
    expect((await regular.invoke('get_snapshot') as Snapshot).sync).toEqual({
      enabled: true,
      explained: true,
      summary: null,
      running: false,
    });

    const keep = createMockHost({ syncFirstRun: true });
    let snapshot = await keep.invoke('get_snapshot') as Snapshot;
    expect(snapshot.sync).toEqual({ enabled: true, explained: false, summary: null, running: false });
    await keep.invoke('dismiss_sync_explanation');
    snapshot = await keep.invoke('get_snapshot') as Snapshot;
    expect(snapshot.sync).toEqual({ enabled: true, explained: true, summary: null, running: false });

    const off = createMockHost({ syncFirstRun: true });
    await off.invoke('set_sync_enabled', { enabled: false });
    await off.invoke('dismiss_sync_explanation');
    snapshot = await off.invoke('get_snapshot') as Snapshot;
    expect(snapshot.sync).toEqual({ enabled: false, explained: true, summary: null, running: false });

    // The explanation stays explained for this mock session across sign-out and sign-in.
    await off.invoke('dispatch_auth', { input: 'signOut' });
    await off.invoke('dispatch_auth', { input: { startSignIn: 'google' } });
    snapshot = await off.invoke('get_snapshot') as Snapshot;
    expect(snapshot.sync).toEqual({ enabled: false, explained: true, summary: null, running: false });
  });

  it('simulates failed GameNode writes without losing local favorites', async () => {
    const host = createMockHost({ failMutations: true });
    await host.invoke('library_input', { input: { select: { key: 'epic:8', mode: 'replace' } } });
    await host.invoke('library_input', { input: 'openAdd' });
    await host.invoke('library_input', { input: 'confirmAdd' });
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.addSummary).toMatchObject({
      added: 0,
      failed: 1,
      undoable: false,
    });
    expect(
      snapshot.library!.rows.find((row) => row.key === 'epic:8')!.gamenode,
    ).toBeNull();

    await host.invoke('library_input', {
      input: { setStatus: { keys: ['steam:1'], status: 'finished' } },
    });
    await host.invoke('library_input', {
      input: { addToCollection: { keys: ['gog:2'], collectionId: 'backlog-curto' } },
    });
    await host.invoke('library_input', {
      input: { setFavorites: { keys: ['steam:1'], favorite: true, now: 3 } },
    });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.statusRun).toMatchObject({ changed: 0, failed: 1 });
    expect(snapshot.library!.collectionRun).toMatchObject({ changed: 0, failed: 1 });
    expect(snapshot.library!.rows.find((row) => row.key === 'steam:1')!.isFavorite).toBe(true);
  });
});

describe('mock catalog, suggestions, scan progress and palette', () => {
  it('serves 40 fictional catalog games and ignores stale or short queries', async () => {
    vi.useFakeTimers();
    try {
      expect(MOCK_CATALOG_GAMES).toHaveLength(40);
      const host = createMockHost();
      await host.invoke('library_input', { input: { catalogSearch: 'a' } });
      let snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.catalog!.status).toBe('idle');
      expect(snapshot.library!.catalog!.results).toHaveLength(0);

      await host.invoke('library_input', { input: { catalogSearch: 'aster' } });
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.catalog!.status).toBe('loading');
      vi.advanceTimersByTime(250);
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.catalog!.status).toBe('ready');
      expect(snapshot.library!.catalog!.results.map((game) => game.title)).toEqual(['Aster Vale']);
      expect(snapshot.library!.catalog!.results[0].inLibrary).toBe(true);
      expect(snapshot.library!.catalog!.results[0].localKey).toBe('steam:1');

      // A newer query wins even though the older timer was still pending.
      await host.invoke('library_input', { input: { catalogSearch: 'aster' } });
      await host.invoke('library_input', { input: { catalogSearch: 'amber' } });
      vi.advanceTimersByTime(250);
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.catalog!.results.map((game) => game.title)).toEqual([
        'Amber Circuit',
      ]);
      expect(snapshot.library!.catalog!.results[0].inLibrary).toBe(true);
      expect(snapshot.library!.catalog!.results[0].localKey).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('mirrors the catalog error, offline and incomplete states', async () => {
    vi.useFakeTimers();
    try {
      const errorHost = createMockHost({ catalogScenario: 'error' });
      await errorHost.invoke('library_input', { input: { catalogSearch: 'aster' } });
      vi.advanceTimersByTime(250);
      let snapshot = await errorHost.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.catalog!.status).toBe('error');
      expect(snapshot.library!.catalog!.results).toHaveLength(0);

      const offlineHost = createMockHost({ catalogScenario: 'offline' });
      await offlineHost.invoke('library_input', { input: { catalogSearch: 'aster' } });
      snapshot = await offlineHost.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.catalog!.status).toBe('offline');

      const incompleteHost = createMockHost({ catalogScenario: 'incomplete' });
      await incompleteHost.invoke('library_input', { input: { catalogSearch: 'aster' } });
      vi.advanceTimersByTime(250);
      snapshot = await incompleteHost.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.catalog!.status).toBe('ready');
      expect(snapshot.library!.catalog!.incomplete).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it('adds and undoes a catalog game and refreshes the result membership', async () => {
    const host = createMockHost();
    await host.invoke('library_input', { input: { catalogSearch: 'brass' } });
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    // The mock catalog answers asynchronously; let the 250 ms timer run.
    await new Promise((resolve) => setTimeout(resolve, 280));
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.catalog!.status).toBe('ready');
    expect(snapshot.library!.catalog!.results[0].inLibrary).toBe(false);

    await host.invoke('library_input', {
      input: {
        openAddForGame: {
          gameId: 14,
          title: 'Brass Meridian',
          coverUrl: '/mock-covers/mock-1.svg',
        },
      },
    });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.addDialog?.mode).toBe('catalog');
    expect(snapshot.library!.addDialog?.targets[0].target).toEqual({ game: 14 });

    await host.invoke('library_input', { input: 'confirmAdd' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.addSummary?.added).toBe(1);
    expect(snapshot.library!.catalog!.results[0].inLibrary).toBe(true);
    expect(snapshot.library!.rows.some((row) => row.title === 'Brass Meridian')).toBe(false);

    const run = snapshot.library!.addSummary!.runId;
    await host.invoke('library_input', { input: { undoAdd: { run } } });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.undo?.state).toBe('done');
    expect(snapshot.library!.catalog!.results[0].inLibrary).toBe(false);
  });

  it('shows both suggestions for the unmatched fixture and uses one', async () => {
    const host = createMockHost();
    await host.invoke('library_input', {
      input: { select: { key: 'steam:3', mode: 'replace' } },
    });
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    const detail = snapshot.library!.detail!;
    expect(detail.suggestions).toHaveLength(2);
    expect(detail.suggestions![0].title).toBe('Clockwork Harbor');
    expect(detail.suggestions![0].confidence).toBe(95);

    await host.invoke('library_input', {
      input: { useSuggestion: { key: 'steam:3', gameId: 201, now: Math.floor(Date.now() / 1000) } },
    });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.detail!.row.status).toEqual({
      matched: { gameId: 201, source: 'chosen' },
    });
    expect(snapshot.library!.detail!.suggestions).toHaveLength(0);
  });

  it('keeps the palette index independent of filters and excludes hidden rows', async () => {
    const host = createMockHost();
    await host.invoke('library_input', { input: { setSearch: 'aster' } });
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library!.rows).toHaveLength(1);
    expect(snapshot.library!.paletteIndex).toHaveLength(12);
    expect(
      snapshot.library!.paletteIndex!.some((entry) => entry.key === MOCK_HIDDEN_KEY),
    ).toBe(false);
    expect(
      snapshot.library!.paletteIndex!.some((entry) => entry.key === 'epic:8'),
    ).toBe(true);
    expect(snapshot.library!.paletteIndex![0]).toMatchObject({
      title: expect.any(String),
      providerName: expect.any(String),
      installed: expect.any(Boolean),
    });
  });

  it('walks scan progress through waiting, scanning and done including DRM-free', async () => {
    vi.useFakeTimers();
    try {
      const host = createMockHost();
      let snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.scanProgress).toHaveLength(4);
      expect(
        snapshot.library!.scanProgress!.find((entry) => entry.provider === 'drm-free'),
      ).toMatchObject({ state: 'done', found: 2 });

      await host.invoke('rescan_library');
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.scanning).toBe(true);
      expect(snapshot.library!.scanProgress!.every((entry) => entry.state === 'waiting')).toBe(true);
      expect(snapshot.localGames.discoveryScanning).toBe(true);

      vi.advanceTimersByTime(120);
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.scanProgress!.every((entry) => entry.state === 'scanning')).toBe(true);

      vi.advanceTimersByTime(700);
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.library!.scanning).toBe(false);
      expect(snapshot.library!.scanProgress!.every((entry) => entry.state === 'done')).toBe(true);
      expect(snapshot.localGames.discoveryScanning).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it('logs openGameNodePage without side effects', async () => {
    const info = vi.spyOn(console, 'info').mockImplementation(() => {});
    const host = createMockHost();
    await host.invoke('open_gamenode_page', { page: 'library' });
    await host.invoke('open_gamenode_page', { page: { game: { id: 42 } } });
    expect(info).toHaveBeenCalledWith('[UI mock] openGameNodePage', 'library');
    expect(info).toHaveBeenCalledWith('[UI mock] openGameNodePage', { game: { id: 42 } });
    info.mockRestore();
  });

  it('reports build facts, redacted system information and fixed website links', async () => {
    const info = vi.spyOn(console, 'info').mockImplementation(() => {});
    const host = createMockHost();
    const appInfo = (await host.invoke('get_app_info')) as AppInfo;
    expect(appInfo).toMatchObject({
      buildDate: '2026-01-01',
      osVersion: 'Windows 11 Pro (build 26100)',
      arch: 'x86_64',
    });

    const text = (await host.invoke('system_info_text')) as string;
    expect(text).toContain('Build 2026-01-01');
    expect(text).toContain('Windows 11 Pro (build 26100) x86_64');
    expect(text).toContain('Steam 0.1.0 (official, installed)');
    expect(text).not.toMatch(/token|credential|cookie|Users/i);

    await host.invoke('open_gamenode_link', { kind: 'profile' });
    expect(info).toHaveBeenCalledWith('[UI mock] openGameNodeLink', 'profile');
    info.mockRestore();
  });
});

describe('mock match dialog selection', () => {
  it('preselects the best of three candidates, selects without matching and confirms the choice', async () => {
    const host = createMockHost();
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const row = snapshot.library!.rows.find((item) => item.status === 'unmatched');
    expect(row).toBeTruthy();

    await host.invoke('library_input', {
      input: { select: { key: row!.key, mode: 'replace' } },
    });
    await host.invoke('library_input', { input: 'openMatch' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const dialog = snapshot.library!.matchDialog!;
    expect(dialog.candidates).toHaveLength(3);
    expect(dialog.candidates.map((candidate) => candidate.confidence)).toEqual([
      96, 74, 38,
    ]);
    expect(dialog.candidates[0].tone).toBe('success');
    expect(dialog.selected?.gameId).toBe(dialog.candidates[0].gameId);

    // Selecting only moves the preview.
    const second = dialog.candidates[1];
    await host.invoke('library_input', {
      input: { selectMatch: { gameId: second.gameId } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.matchDialog!.selected?.gameId).toBe(second.gameId);
    expect(
      snapshot.library!.rows.find((item) => item.key === row!.key)?.status,
    ).toBe('unmatched');

    // Confirming matches the entry to the selected game and closes the dialog.
    await host.invoke('library_input', {
      input: { confirmMatch: { now: 1 } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.matchDialog).toBeNull();
    expect(
      snapshot.library!.rows.find((item) => item.key === row!.key)?.status,
    ).toEqual({ matched: { gameId: second.gameId, source: 'chosen' } });
  });

  it('keeps the search results in the server order and lets a result be selected', async () => {
    const host = createMockHost();
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const row = snapshot.library!.rows.find((item) => item.status === 'unmatched')!;
    await host.invoke('library_input', {
      input: { select: { key: row.key, mode: 'replace' } },
    });
    await host.invoke('library_input', { input: 'openMatch' });
    await host.invoke('library_input', { input: 'searchMatch' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const dialog = snapshot.library!.matchDialog!;
    expect(dialog.results.map((result) => result.confidence)).toEqual([60, 30]);

    const result = dialog.results[1];
    await host.invoke('library_input', {
      input: { selectMatch: { gameId: result.gameId } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.matchDialog!.selected?.gameId).toBe(result.gameId);
    expect(snapshot.library!.matchDialog!.selected?.name).toBe(result.name);
  });

  it('narrows the Dolphin match dialog by the declared platform and restores it on Show all', async () => {
    const host = createMockHost({ variantLibrary: true });

    // GameCube: only the NGC candidate and result are visible by default.
    await host.invoke('library_input', {
      input: { select: { key: 'dolphin:pikmin', mode: 'replace' } },
    });
    await host.invoke('library_input', { input: 'openMatch' });
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    let dialog = snapshot.library!.matchDialog!;
    expect(dialog.platformFilter).toEqual({
      platform: 'gamecube',
      label: 'GameCube',
      aliases: ['NGC', 'GC', 'GCN', 'GameCube'],
      enabled: true,
    });
    expect(dialog.candidates.map((candidate) => candidate.gameId)).toEqual([400]);
    expect(dialog.candidates[0].platforms).toEqual(['NGC']);
    expect(dialog.filteredOut).toBe(1);
    expect(dialog.selected?.gameId).toBe(400);

    await host.invoke('library_input', { input: 'searchMatch' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    dialog = snapshot.library!.matchDialog!;
    expect(dialog.results.map((result) => result.gameId)).toEqual([402]);
    expect(dialog.filteredOut).toBe(2);

    // Show all restores the complete answer.
    await host.invoke('library_input', {
      input: { setMatchPlatformFilter: { enabled: false } },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    dialog = snapshot.library!.matchDialog!;
    expect(dialog.platformFilter?.enabled).toBe(false);
    expect(dialog.candidates.map((candidate) => candidate.gameId)).toEqual([400, 401]);
    expect(dialog.results.map((result) => result.gameId)).toEqual([402, 403]);
    expect(dialog.filteredOut).toBe(0);

    // Wii: the Wii release is visible; the platform-less remaster only with Show all.
    await host.invoke('library_input', { input: 'closeMatch' });
    await host.invoke('library_input', {
      input: { select: { key: 'dolphin:metroid', mode: 'replace' } },
    });
    await host.invoke('library_input', { input: 'openMatch' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    dialog = snapshot.library!.matchDialog!;
    expect(dialog.platformFilter?.label).toBe('Wii');
    expect(dialog.candidates.map((candidate) => candidate.gameId)).toEqual([410]);
    expect(dialog.filteredOut).toBe(1);

    // A row without a declared platform keeps the full list and shows no filter.
    await host.invoke('library_input', { input: 'closeMatch' });
    await host.invoke('library_input', {
      input: { select: { key: 'steam:1', mode: 'replace' } },
    });
    await host.invoke('library_input', { input: 'openMatch' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    dialog = snapshot.library!.matchDialog!;
    expect(dialog.platformFilter).toBeNull();
    expect(dialog.filteredOut).toBe(0);
    expect(dialog.candidates).toHaveLength(3);
  });

  it('serves backup origins, retention and the history totals for the variant fixture', async () => {
    const host = createMockHost({ variantLibrary: true });
    const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const { versions, settings, historyBytes, historyCount } = snapshot.localGames;
    expect(settings.retention).toBe(10);
    expect(versions).toHaveLength(4);
    expect(versions.map((version) => version.reason)).toEqual([
      'afterSession',
      'afterSession',
      'manual',
      'recovery',
    ]);
    expect(versions[0].playSeconds).toBe(42 * 60);
    expect(versions[3].profile).toContain('_recovery_');
    expect(historyCount).toBe(4);
    expect(historyBytes).toBe(
      versions.reduce((total, version) => total + version.bytes, 0),
    );
  });

  it('updates retention per game, appends manual copies and keeps history in sync', async () => {
    const host = createMockHost({ variantLibrary: true });
    await host.invoke('set_local_backup_retention', {
      entryKey: 'manual:brasa',
      retention: 50,
    });
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(
      snapshot.localGames.profiles.find((profile) => profile.id === 'brasa-profile')
        ?.retention,
    ).toBe(50);
    expect(snapshot.localGames.versions).toHaveLength(4);

    await host.invoke('load_local_history', { id: 'brasa' });
    await host.invoke('backup_local_now', { id: 'brasa' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.versions).toHaveLength(5);
    expect(snapshot.localGames.versions[0].reason).toBe('manual');
    expect(snapshot.localGames.historyCount).toBe(5);
    expect(snapshot.localGames.historyBytes).toBe(
      snapshot.localGames.versions.reduce(
        (total, version) => total + version.bytes,
        0,
      ),
    );

    await expect(
      host.invoke('set_local_backup_retention', {
        entryKey: 'manual:brasa',
        retention: 7,
      }),
    ).rejects.toBeDefined();
  });

  it('picks a bundled sample cover and mirrors save, cancel and clear on the DRM-free row', async () => {
    const host = createMockHost({ variantLibrary: true });
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const registration = snapshot.localGames.registrations.find(
      (item) => item.id === 'drift',
    )!;
    const rowBefore = snapshot.library!.rows.find(
      (item) => item.key === 'manual:drift',
    )!;
    const gameNodeCover = rowBefore.coverFile;
    expect(registration.customCover).toBeNull();

    const picked = await host.invoke('pick_local_cover', {
      entryKey: 'manual:drift',
    });
    expect(picked).toMatch(/^mock-\d+\.svg$/);

    // Cancelling leaves the committed cover untouched.
    await host.invoke('discard_local_cover', { entryKey: 'manual:drift' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(
      snapshot.localGames.registrations.find((item) => item.id === 'drift')
        ?.customCover,
    ).toBeNull();
    expect(
      snapshot.library!.rows.find((item) => item.key === 'manual:drift')
        ?.coverFile,
    ).toBe(gameNodeCover);

    await host.invoke('edit_local_game', {
      value: { ...registration, customCover: picked },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const savedRow = snapshot.library!.rows.find(
      (item) => item.key === 'manual:drift',
    )!;
    expect(
      snapshot.localGames.registrations.find((item) => item.id === 'drift')
        ?.customCover,
    ).toBe(picked);
    expect(savedRow.coverFile).toBe(picked);
    expect(savedRow.gameCoverFile).toBe(gameNodeCover);

    await host.invoke('edit_local_game', {
      value: {
        ...snapshot.localGames.registrations.find((item) => item.id === 'drift')!,
        customCover: null,
      },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(
      snapshot.library!.rows.find((item) => item.key === 'manual:drift')
        ?.coverFile,
    ).toBe(gameNodeCover);
  });
});

describe('mock discovery review', () => {
  it('serves three Figma-shaped rows with the non-game unchecked by default', async () => {
    const host = createMockHost({ discoveries: true });
    const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const candidates = snapshot.localGames.candidates;
    expect(candidates).toHaveLength(3);
    expect(candidates.find((item) => item.title === 'Moss & Machine')?.suggestion).toMatchObject({
      title: 'Moss & Machine',
      year: 2023,
      tone: 'success',
    });
    expect(
      candidates.find((item) => item.title === 'Frostbyte Arcade')?.suggestion,
    ).toBeNull();
    expect(candidates.find((item) => item.title === 'setup_redist')).toMatchObject({
      likelyNonGame: true,
      selectedTarget: 'D:\\Jogos\\Instaladores\\vc_redist.x64.exe',
    });
    expect(snapshot.localGames.reviewSelected).toEqual([
      'candidate-moss-machine',
      'candidate-frostbyte-arcade',
    ]);
  });

  it('toggles a row selection', async () => {
    const host = createMockHost({ discoveries: true });
    await host.invoke('select_local_candidate', {
      fingerprint: 'candidate-frostbyte-arcade',
    });
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.reviewSelected).toEqual(['candidate-moss-machine']);

    await host.invoke('select_local_candidate', {
      fingerprint: 'candidate-frostbyte-arcade',
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.reviewSelected).toEqual([
      'candidate-moss-machine',
      'candidate-frostbyte-arcade',
    ]);
  });

  it('adds the selected rows and clears their selection', async () => {
    const host = createMockHost({ discoveries: true });
    await host.invoke('review_selected_local_candidates', {
      add: true,
      allowRediscovery: true,
    });
    const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.candidates.map((item) => item.title)).toEqual([
      'setup_redist',
    ]);
    expect(snapshot.localGames.reviewSelected).toEqual([]);
  });

  it('ignores with rediscovery by forgetting the rows, and without by marking them', async () => {
    const rediscoverable = createMockHost({ discoveries: true });
    await rediscoverable.invoke('review_selected_local_candidates', {
      add: false,
      allowRediscovery: true,
    });
    let snapshot = (await rediscoverable.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.candidates.map((item) => item.title)).toEqual([
      'setup_redist',
    ]);

    const permanent = createMockHost({ discoveries: true });
    await permanent.invoke('review_selected_local_candidates', {
      add: false,
      allowRediscovery: false,
    });
    snapshot = (await permanent.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.candidates).toHaveLength(3);
    expect(
      snapshot.localGames.candidates
        .filter((item) => item.ignored)
        .map((item) => item.title),
    ).toEqual(['Moss & Machine', 'Frostbyte Arcade']);
    expect(snapshot.localGames.reviewSelected).toEqual([]);
  });

  it('resets ignored rows and restores the non-game-aware defaults', async () => {
    const host = createMockHost({ discoveries: true });
    await host.invoke('review_selected_local_candidates', {
      add: false,
      allowRediscovery: false,
    });
    await host.invoke('reset_local_ignores');
    const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.candidates.every((item) => !item.ignored)).toBe(true);
    expect(snapshot.localGames.reviewSelected).toEqual([
      'candidate-moss-machine',
      'candidate-frostbyte-arcade',
    ]);
  });
});

describe('UI mock host settings data', () => {
  it('runs Sync now, shows progress and records the last pass', async () => {
    const host = createMockHost();
    await host.invoke('set_sync_enabled', { enabled: false });
    await expect(host.invoke('sync_now')).rejects.toBe('syncUnavailable');
    await host.invoke('set_sync_enabled', { enabled: true });
    await host.invoke('sync_now');
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.sync.running).toBe(true);
    await vi.waitFor(
      async () => {
        snapshot = (await host.invoke('get_snapshot')) as Snapshot;
        expect(snapshot.sync.running).toBe(false);
      },
      { timeout: 1_000 },
    );
    expect(snapshot.sync.summary).toMatchObject({ held: 0 });
    expect(snapshot.sync.summary!.acceptedGames).toBeGreaterThan(0);
    expect(snapshot.sync.summary!.stores).toBeGreaterThan(0);
  });

  it('seeds the unavailable watched-folder fixture without the variant library', async () => {
    const host = createMockHost({ watches: 'unavailable' });
    const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.watches).toHaveLength(2);
    expect(snapshot.localGames.watches.find((watch) => watch.id === 'watched-games'))
      .toMatchObject({ path: 'D:\\Jogos', state: 'idle', entryCount: 2 });
    expect(snapshot.localGames.watches.find((watch) => watch.id === 'watched-itch'))
      .toMatchObject({
        path: 'E:\\Itch',
        state: 'unavailable',
        entryCount: 6,
      });
    expect(snapshot.library!.rows).toHaveLength(12);
  });

  it('checks one watched folder and keeps an unavailable folder alone', async () => {
    const host = createMockHost({ variantLibrary: true });
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const unavailable = snapshot.localGames.watches.find(
      (watch) => watch.state === 'unavailable',
    )!;
    await host.invoke('check_local_watch', { id: unavailable.id });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(
      snapshot.localGames.watches.find((watch) => watch.id === unavailable.id)!
        .state,
    ).toBe('unavailable');

    const idle = snapshot.localGames.watches.find((watch) => watch.state === 'idle')!;
    await host.invoke('check_local_watch', { id: idle.id });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(
      snapshot.localGames.watches.find((watch) => watch.id === idle.id)!.state,
    ).toBe('checking');
    await vi.waitFor(
      async () => {
        snapshot = (await host.invoke('get_snapshot')) as Snapshot;
        expect(
          snapshot.localGames.watches.find((watch) => watch.id === idle.id)!.state,
        ).toBe('idle');
      },
      { timeout: 1_000 },
    );
    expect(
      snapshot.localGames.watches.find((watch) => watch.id === idle.id)!
        .lastCheckedAt,
    ).toBeGreaterThan(0);
  });

  it('leaves shared-only entries out without losing the store shared count', async () => {
    const host = createMockHost();
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const before = snapshot.library!;
    const shared = before.rows.find(
      (row) => row.provider === 'steam' && row.shared && !row.owned && !row.installed,
    )!;
    expect(shared).toBeDefined();
    const steamBefore = before.providers.find((provider) => provider.id === 'steam')!;
    expect(before.filterCounts.availability.shared).toBe(1);

    await host.invoke('set_app_preference', {
      patch: { sharedExcluded: ['steam'] },
    });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const after = snapshot.library!;
    expect(after.rows.some((row) => row.key === shared.key)).toBe(false);
    expect(after.rows).toHaveLength(before.rows.length - 1);
    const steamAfter = after.providers.find((provider) => provider.id === 'steam')!;
    expect(steamAfter.count).toBe(steamBefore.count - 1);
    expect(steamAfter.sharedCount).toBe(steamBefore.sharedCount);
    expect(steamAfter.ownedCount).toBe(steamBefore.ownedCount);
    expect(after.filterCounts.availability.shared).toBe(0);
    expect(
      after.rows.some((row) => row.provider === 'steam' && row.owned),
    ).toBe(true);

    await host.invoke('set_app_preference', { patch: { sharedExcluded: [] } });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library!.rows.some((row) => row.key === shared.key)).toBe(true);
    expect(snapshot.library!.rows).toHaveLength(before.rows.length);
  });

  it('reports detected store locations and keeps unreported ones without a path', async () => {
    const host = createMockHost();
    const snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const steam = snapshot.library!.providers.find(
      (provider) => provider.id === 'steam',
    )!;
    expect(steam.installRoot).toBe('C:\\Program Files (x86)\\Steam');
    expect(steam.libraryFolders).toBe(4);
    const epic = snapshot.library!.providers.find(
      (provider) => provider.id === 'epic',
    )!;
    expect(epic.installedCount).toBeGreaterThan(0);
    expect(epic.installRoot).toBeNull();
    expect(epic.libraryFolders).toBeNull();
  });

  it('saves the default retention with the other backup fields and reports usage', async () => {
    const host = createMockHost({ variantLibrary: true });
    await host.invoke('set_local_backup_settings', {
      automatic: false,
      destination: 'D:\\Backups',
      retention: 20,
    });
    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.settings).toEqual({
      automatic: false,
      destination: 'D:\\Backups',
      retention: 20,
    });
    await expect(
      host.invoke('set_local_backup_settings', {
        automatic: true,
        destination: 'D:\\Backups',
        retention: 7,
      }),
    ).rejects.toMatchObject({ code: 'invalidRetention' });
    await expect(
      host.invoke('set_local_backup_settings', {
        automatic: true,
        destination: '   ',
        retention: 10,
      }),
    ).rejects.toMatchObject({ code: 'invalidDestination' });
    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.localGames.settings.retention).toBe(20);
    expect(snapshot.localGames.settings.destination).toBe('D:\\Backups');

    const usage = (await host.invoke('local_backup_usage')) as {
      totalBytes: number;
      totalVersions: number;
      games: { title: string; bytes: number; versions: number }[];
    };
    expect(usage.totalVersions).toBe(4);
    expect(usage.totalBytes).toBe(7_100_000 + 7_000_000 + 6_800_000 + 6_800_000);
    expect(usage.games[0].title).toBe('Brasa');
    expect(usage.games[0].versions).toBe(4);
  });

  it('provides a fictional durable producer and deletes only its retained records', async () => {
    const host = createMockHost();
    const profileId = 'mock-profile';
    const pluginId = 'dev.prateleira.local';
    const usage = await host.invoke('plugin_durable_usage', {
      pluginId,
      expectedProfileId: profileId,
    }) as PluginDurableUsage;
    expect(usage).toMatchObject({
      pluginId,
      profileId,
      recordCount: 12,
      logicalBytes: 153_600,
      maxBytes: 16 * 1024 * 1024,
    });

    const deleted = await host.invoke('plugin_durable_delete', {
      pluginId,
      expectedProfileId: profileId,
      confirmed: true,
    }) as PluginDurableDeletion;
    expect(deleted.removedRecords).toBe(12);
    expect(deleted.reclaimedBytes).toBe(153_600);
    expect(deleted.usage.recordCount).toBe(0);

    const other = await host.invoke('plugin_durable_usage', {
      pluginId: 'community.example.save-sync',
      expectedProfileId: profileId,
    }) as PluginDurableUsage;
    expect(other.recordCount).toBe(0);
    expect(other.logicalBytes).toBe(0);
  });
});
