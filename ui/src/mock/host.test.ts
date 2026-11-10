import { describe, expect, it, vi } from 'vitest';
import { createMockHost } from './host';
import { createDefaultFilters, type AppInfo, type LibraryFilters, type PluginInfo, type Snapshot } from '../api/types';
import type { PortProgress, PortSuppliedPick, PortToolsCacheView } from '../api/ports';

describe('UI mock host', () => {
  it('starts with a verified mock session and deterministic games', async () => {
    const host = createMockHost();
    const snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.auth).toEqual({ signedIn: { user: { id: 'mock-user', username: 'UI Developer', email: 'ui@example.invalid' } } });
    expect(snapshot.library?.access).toBe('verified');
    expect(snapshot.library?.layout.sidebarWidth).toBe(248);
    expect(snapshot.library?.layout.sidebarCollapsed).toBe(false);
    expect(snapshot.library?.rows).toHaveLength(12);
    snapshot.library!.rows.length = 0;
    expect((await host.invoke('get_snapshot') as Snapshot).library?.rows).toHaveLength(12);
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
    expect(selected.library?.detail?.achievements?.total).toBe(5);

    const all = await host.invoke('achievement_list', {
      key: row.key,
      filter: 'all',
    }) as { achievements: Array<{ unlocked: boolean; hidden: boolean }> };
    expect(all.achievements).toHaveLength(5);
    const unlocked = await host.invoke('achievement_list', {
      key: row.key,
      filter: 'unlocked',
    }) as { achievements: Array<{ unlocked: boolean }> };
    expect(unlocked.achievements).toHaveLength(2);
    expect(unlocked.achievements.every((entry) => entry.unlocked)).toBe(true);
    const locked = await host.invoke('achievement_list', {
      key: row.key,
      filter: 'locked',
    }) as { achievements: Array<{ unlocked: boolean; hidden: boolean }> };
    expect(locked.achievements).toHaveLength(3);
    expect(locked.achievements.some((entry) => entry.hidden)).toBe(true);
    expect(await host.invoke('achievement_list', { key: 'gog:2', filter: 'all' })).toBeNull();
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

  it('keeps Epic connected and without family data while a Steam card action runs', async () => {
    const host = createMockHost();
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    const epic = snapshot.storeAccounts.find((account) => account.provider === 'epic')!;
    expect(epic.state).toBe('connected');
    expect(epic.familyCount).toBe(0);
    expect(epic.familyError).toBeNull();

    await host.invoke('provider_sign_out', { providerId: 'epic' });
    snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.storeAccounts.find((account) => account.provider === 'epic')!.state).toBe('notConnected');
    expect(snapshot.storeAccounts.find((account) => account.provider === 'steam')!.state).toBe('connected');
    expect(snapshot.library!.providers.find((provider) => provider.id === 'steam')!.ownedCount).toBe(5);
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

  it.each(['en', 'pt-BR'] as const)('keeps Steam account and PC ports state together in the %s snapshot', async (locale) => {
    const host = createMockHost({ locale });
    await host.invoke('ports_pick_parent');
    const snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.storeAccounts[0].provider).toBe('steam');
    expect(snapshot.ports).toBeDefined();
    expect(snapshot.ports.preferences.lastParentFolder).toBe('D:\\Ports');
  });

  it('marks rows with installed, owned and family facts and keeps counts consistent', async () => {
    const snapshot = await createMockHost().invoke('get_snapshot') as Snapshot;
    const rows = snapshot.library!.rows;

    expect(rows).toHaveLength(12);
    expect(rows.every((row) => typeof row.installed === 'boolean')).toBe(true);
    expect(rows.some((row) => row.owned)).toBe(true);
    expect(rows.some((row) => row.familyShared && !row.owned)).toBe(true);
    expect(snapshot.library!.installedTotal).toBe(rows.filter((row) => row.installed).length);

    const steam = snapshot.library!.providers.find((provider) => provider.id === 'steam')!;
    expect(steam.count).toBe(rows.filter((row) => row.provider === 'steam').length);
    expect(steam.installedCount).toBe(rows.filter((row) => row.provider === 'steam' && row.installed).length);
    expect(steam.ownedCount).toBe(rows.filter((row) => row.provider === 'steam' && row.owned).length);
    expect(steam.familyCount).toBe(rows.filter((row) => row.provider === 'steam' && row.familyShared && !row.owned).length);
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
      availability: { installed: true, owned: false, family: false },
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
      playtime: 'unplayed',
      installed: false,
    } as LibraryFilters);
    // The first fixture row is installed, owned and has a known zero; the unknown rows are not
    // presented as unplayed.
    expect(unplayed.library!.rows.map((row) => row.key)).toEqual(['steam:1']);

    const favoritesInstalled = await withFilters({
      ...createDefaultFilters(),
      favoritesOnly: true,
      availability: { installed: true, owned: false, family: false },
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
      availability: { installed: true, owned: false, family: false },
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
