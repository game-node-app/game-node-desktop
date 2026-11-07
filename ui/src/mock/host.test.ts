import { describe, expect, it, vi } from 'vitest';
import { createMockHost } from './host';
import { createDefaultFilters, type AppInfo, type LibraryFilters, type Snapshot } from '../api/types';

describe('UI mock host', () => {
  it('starts with a verified mock session and deterministic games', async () => {
    const host = createMockHost();
    const snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.auth).toEqual({ signedIn: { user: { id: 'mock-user', username: 'UI Developer', email: 'ui@example.invalid' } } });
    expect(snapshot.library?.access).toBe('verified');
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
    snapshot = listener.mock.lastCall![0] as Snapshot;
    expect(snapshot.library?.detail?.row.title).toBe('Aster Vale');
    expect(snapshot.library?.layout.detailOpen).toBe(true);
    await host.invoke('dispatch_auth', { input: 'signOut' });
    await host.invoke('dispatch_auth', { input: { startSignIn: 'discord' } });
    expect((await host.invoke('get_snapshot') as Snapshot).library?.rows).toHaveLength(12);
    unsubscribe();
    listener.mockClear();
    await host.invoke('set_sync_enabled', { enabled: false });
    expect(listener).not.toHaveBeenCalled();
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
      expect(snapshot.storeAccounts).toHaveLength(1);
      expect(snapshot.storeAccounts[0].provider).toBe('steam');
      expect(snapshot.storeAccounts[0].state).toBe(scenario === 'persistFailed' ? 'connected' : scenario);
      expect(snapshot.storeAccounts[0].persistFailed).toBe(scenario === 'persistFailed');
    }
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

      await host.invoke('steam_check');
      let snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('checking');
      vi.advanceTimersByTime(900);
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('connected');

      await host.invoke('steam_sign_out');
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('notConnected');
      expect(snapshot.library!.rows.every((row) => row.installed)).toBe(true);
      expect(snapshot.library!.providers.find((provider) => provider.id === 'steam')!.ownedCount).toBe(0);

      await host.invoke('steam_sign_in');
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('signingIn');
      vi.advanceTimersByTime(1500);
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('connected');

      await host.invoke('steam_sign_in');
      await host.invoke('steam_cancel_sign_in');
      snapshot = await host.invoke('get_snapshot') as Snapshot;
      expect(snapshot.storeAccounts[0].state).toBe('connected');
      await host.invoke('steam_focus_sign_in');

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
