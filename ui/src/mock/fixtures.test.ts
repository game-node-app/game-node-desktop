import { describe, expect, it, vi } from 'vitest';
import { createDefaultFilters, type Row, type Snapshot } from '../api/types';
import { createMockHost, MOCK_HIDDEN_KEY } from './host';

async function snap(host: ReturnType<typeof createMockHost>): Promise<Snapshot> {
  return (await host.invoke('get_snapshot')) as Snapshot;
}

async function waitForViewWrite(
  host: ReturnType<typeof createMockHost>,
  state: 'pending' | 'succeeded' | 'failed',
) {
  await vi.waitFor(async () => {
    const current = await snap(host);
    expect(current.library?.viewWrite?.state).toBe(state);
  }, { timeout: 1_000 });
}

function matchKind(row: Row): string {
  const status = row.status;
  if (typeof status === 'object' && 'matched' in status) return 'matched';
  if (typeof status === 'object' && 'ambiguous' in status) return 'ambiguous';
  if (status === 'ambiguous') return 'ambiguous';
  if (status === 'unmatched') return 'unmatched';
  return 'noGame';
}

describe('mock comprehensive and state fixtures', () => {
  it('adds every match shape, DRM-free registrations and discovered candidates on demand', async () => {
    const host = createMockHost({ variantLibrary: true, discoveries: true });
    const snapshot = await snap(host);
    const library = snapshot.library!;
    const rows = library.rows;

    const kinds = new Set(rows.map((row) => matchKind(row)));
    expect([...kinds].sort()).toEqual(
      ['ambiguous', 'matched', 'noGame', 'unmatched'],
    );

    const ambiguous = rows.find((row) => row.key === 'steam:1301')!;
    expect(ambiguous.status).toEqual({
      ambiguous: { candidates: [301, 302] },
    });
    expect(rows.find((row) => row.key === 'steam:1302')?.status).toBe('noGame');

    const runningDrm = rows.find((row) => row.key === 'manual:brasa')!;
    expect(runningDrm).toMatchObject({
      provider: 'manual',
      providerName: 'DRM-free',
      installed: true,
      running: true,
      status: { matched: { gameId: 305, source: 'chosen' } },
      gamenode: { status: 'ongoing', collectionIds: ['backlog-curto'] },
    });
    expect(runningDrm.sessionStartedAt).toBeGreaterThan(0);
    expect(snapshot.tracking.title).toBe('Brasa');
    expect(snapshot.tracking.running).toEqual(['manual:brasa']);

    // The family-shared row stays owned=false, shared=true and not installed.
    expect(rows.find((row) => row.key === 'steam:7')).toMatchObject({
      installed: false,
      owned: false,
      shared: true,
    });

    // The manual provider is a store shortcut only in this fixture, and the sidebar facts are
    // derived from the same rows.
    expect(library.providers.map((provider) => provider.id)).toEqual([
      'steam',
      'gog',
      'epic',
      'manual',
    ]);
    expect(library.providers.find((provider) => provider.id === 'manual')).toMatchObject({
      name: 'DRM-free',
      count: 2,
    });

    // The fixtures behind the L6 DRM-free detail: launch configuration, saves and backups.
    expect(snapshot.localGames.registrations.find((entry) => entry.id === 'brasa')).toMatchObject({
      executable: 'D:\\Jogos\\Brasa\\brasa.exe',
      arguments: ['--windowed', '--fps', '120'],
      root: 'D:\\Jogos\\Brasa',
    });
    expect(snapshot.localGames.profiles[0]).toMatchObject({
      installation: 'brasa',
      files: ['%APPDATA%\\Brasa'],
      support: 'supported',
    });
    expect(
      snapshot.localGames.versions.filter((version) => version.installation === 'brasa'),
    ).toHaveLength(12);
    expect(snapshot.localGames.jobs[0].installation).toBe('brasa');

    // The existing organization fixtures stay present.
    expect(library.savedViews).toHaveLength(1);
    expect(library.collections).toHaveLength(2);
    expect(library.filterCounts!.showHidden).toBe(1);
    expect(rows.some((row) => row.key === MOCK_HIDDEN_KEY)).toBe(false);

    // Discovered candidates feed both the sidebar count and the banner.
    expect(snapshot.localGames.candidates).toHaveLength(3);
    expect(snapshot.localGames.recentlyAdded).toEqual([
      'Signal Ridge',
      'Paper Harbor',
      'Tiny Comet',
    ]);

    await host.invoke('library_input', {
      input: { select: { key: 'manual:drift', mode: 'replace' } },
    });
    const drift = await snap(host);
    expect(drift.library!.detail!.suggestions).toHaveLength(2);
    expect(drift.library!.detail!.suggestions[0]).toMatchObject({
      title: 'Drifting Ember',
      confidence: 84,
      tone: 'success',
    });

    // The detail pane's direct Not a game action needs no match dialog and clears suggestions.
    await host.invoke('library_input', {
      input: {
        markEntryNoGame: { key: 'manual:drift', now: Math.floor(Date.now() / 1000) },
      },
    });
    const noGame = await snap(host);
    expect(noGame.library!.matchDialog).toBeNull();
    expect(noGame.library!.detail!.row.status).toBe('noGame');
    expect(noGame.library!.detail!.suggestions).toEqual([]);

    await host.invoke('library_input', {
      input: { select: { key: 'manual:brasa', mode: 'replace' } },
    });
    const brasa = await snap(host);
    expect(brasa.library!.detail!.row.running).toBe(true);
    expect(brasa.library!.actions.play).toEqual({ disabled: 'running' });
    expect(brasa.library!.detail!.membership).toEqual({ inLibrary: 'ongoing' });
  });

  it('classifies the Installed scope and saved-view payload as NoInstalled and shows all on All', async () => {
    const host = createMockHost({ noInstalledLibrary: true });
    let snapshot = await snap(host);
    expect(snapshot.library!.titleSource).toEqual({
      kind: 'scope',
      id: 'installed',
    });
    expect(snapshot.library!.empty).toEqual({
      noInstalled: { provider: null },
    });
    expect(snapshot.library!.rows).toEqual([]);
    expect(snapshot.library!.total).toBeGreaterThan(0);

    // Show all: the rows the other filters keep are still there and none is installed.
    await host.invoke('library_input', { input: { setScope: 'all' } });
    snapshot = await snap(host);
    expect(snapshot.library!.empty).toBeNull();
    expect(snapshot.library!.rows.length).toBeGreaterThan(0);
    expect(snapshot.library!.rows.every((row) => !row.installed)).toBe(true);

    // The installed-only availability fact gets the same actionable state.
    await host.invoke('library_input', {
      input: {
        setFilters: {
          ...createDefaultFilters(),
          availability: { installed: true, owned: false, shared: false },
        },
      },
    });
    snapshot = await snap(host);
    expect(snapshot.library!.empty).toEqual({
      noInstalled: { provider: null },
    });

    // A saved view whose payload scope is Installed keeps the payload classification even while
    // the layout shows the SavedView marker.
    await host.invoke('library_input', {
      input: { setFilters: createDefaultFilters() },
    });
    await host.invoke('library_input', { input: { setScope: 'installed' } });
    await host.invoke('library_input', {
      input: { saveView: { name: 'Installed picks' } },
    });
    await waitForViewWrite(host, 'succeeded');
    await host.invoke('library_input', { input: { setScope: 'all' } });
    await host.invoke('library_input', { input: { applyView: { id: 2 } } });
    snapshot = await snap(host);
    expect(snapshot.library!.layout.scope).toEqual({ savedView: 2 });
    expect(snapshot.library!.titleSource).toEqual({
      kind: 'savedView',
      id: '2',
    });
    expect(snapshot.library!.empty).toEqual({
      noInstalled: { provider: null },
    });

    // Show all after a saved view reveals the owned rows again.
    await host.invoke('library_input', { input: { setScope: 'all' } });
    snapshot = await snap(host);
    expect(snapshot.library!.rows.length).toBeGreaterThan(0);
    expect(snapshot.library!.empty).toBeNull();
  });

  it('exposes connected accounts whose catalog failed, for the banner fixtures', async () => {
    const expired = await snap(createMockHost({ storeAccount: 'catalogExpired' }));
    expect(expired.storeAccounts[0]).toMatchObject({
      provider: 'steam',
      state: 'connected',
      catalogError: 'expired',
      canSignIn: true,
    });
    expect(expired.storeAccounts[0].lastSyncedAt).toBeGreaterThan(0);

    const unreachable = await snap(
      createMockHost({ storeAccount: 'catalogUnreachable' }),
    );
    expect(unreachable.storeAccounts[0]).toMatchObject({
      provider: 'steam',
      state: 'connected',
      catalogError: 'unreachable',
    });
  });

  it('serves the empty first-run and scanning states on demand', async () => {
    const discovered = createMockHost({ discoveries: true });
    let snapshot = await snap(discovered);
    expect(snapshot.library!.rows).toHaveLength(12);
    expect(snapshot.localGames.candidates).toHaveLength(3);
    expect(snapshot.localGames.recentlyAdded).toHaveLength(3);

    const empty = createMockHost({ emptyLibrary: true });
    snapshot = await snap(empty);
    expect(snapshot.library!.rows).toEqual([]);
    expect(snapshot.library!.savedViews).toEqual([]);
    expect(snapshot.library!.collections).toEqual([]);
    expect(snapshot.library!.empty).toEqual({
      noGames: { stores: ['Steam', 'GOG', 'Epic Games Store'] },
    });

    const firstRun = createMockHost({ firstRun: true });
    snapshot = await snap(firstRun);
    expect(snapshot.auth).toEqual({
      signedIn: expect.objectContaining({
        user: expect.objectContaining({ id: 'mock-user' }),
      }),
    });
    expect(snapshot.library!.rows).toEqual([]);
    expect(
      snapshot.storeAccounts.every((account) => account.state === 'notConnected'),
    ).toBe(true);
    expect(snapshot.localGames.watches).toEqual([]);
    expect(snapshot.localGames.candidates).toEqual([]);

    const scanning = createMockHost({ emptyLibrary: true, startScanning: true });
    snapshot = await snap(scanning);
    expect(snapshot.library!.scanning).toBe(true);
    expect(snapshot.localGames.discoveryScanning).toBe(true);
    expect(snapshot.library!.scanProgress!.every((entry) => entry.state === 'waiting')).toBe(true);
    await vi.waitFor(async () => {
      const current = await snap(scanning);
      expect(current.library!.scanProgress!.every((entry) => entry.state === 'done')).toBe(true);
    }, { timeout: 1_500 });
    snapshot = await snap(scanning);
    expect(snapshot.library!.scanning).toBe(false);
    expect(snapshot.localGames.discoveryScanning).toBe(false);
    expect(snapshot.library!.empty).toEqual({
      noGames: { stores: ['Steam', 'GOG', 'Epic Games Store'] },
    });
  });
});

describe('mock saved-view writes', () => {
  it('reports pending, then the real saved id, and never invents a view', async () => {
    const host = createMockHost();
    let snapshot = await snap(host);
    expect(snapshot.library!.viewWrite).toBeNull();
    expect(snapshot.library!.savedViews).toHaveLength(1);
    await host.invoke('library_input', {
      input: { setFilter: { provider: 'epic' } },
    });

    await host.invoke('library_input', {
      input: { saveView: { name: '  Epic only  ' } },
    });
    snapshot = await snap(host);
    expect(snapshot.library!.viewWrite).toMatchObject({
      operation: 'save',
      viewId: 0,
      name: 'Epic only',
      state: 'pending',
      error: null,
      savedViewId: null,
    });
    expect(snapshot.library!.savedViews).toHaveLength(1);

    await waitForViewWrite(host, 'succeeded');
    snapshot = await snap(host);
    expect(snapshot.library!.viewWrite).toMatchObject({
      state: 'succeeded',
      error: null,
      savedViewId: 2,
    });
    expect(snapshot.library!.savedViews).toHaveLength(2);
    expect(snapshot.library!.savedViews![1].name).toBe('Epic only');
    expect(snapshot.library!.lastSavedView).toMatchObject({
      id: 2,
      name: 'Epic only',
    });
    expect(snapshot.library!.viewError).toBeNull();
  });

  it('fails one storage write, keeps the previous state, then accepts a retry', async () => {
    const host = createMockHost({ viewWriteFailure: 'once' });
    await host.invoke('library_input', {
      input: { setFilter: { provider: 'epic' } },
    });

    await host.invoke('library_input', {
      input: { saveView: { name: 'Retry me' } },
    });
    let snapshot = await snap(host);
    expect(snapshot.library!.viewWrite!.state).toBe('pending');
    await waitForViewWrite(host, 'failed');
    snapshot = await snap(host);
    expect(snapshot.library!.viewWrite).toMatchObject({
      operation: 'save',
      state: 'failed',
      error: 'failed',
      savedViewId: null,
    });
    expect(snapshot.library!.savedViews).toHaveLength(1);
    expect(snapshot.library!.lastSavedView).toBeNull();
    expect(snapshot.library!.viewError).toBeNull();
    const failedRun = snapshot.library!.viewWrite!.run;

    // The retry is a newer attempt against the same open view and succeeds.
    await host.invoke('library_input', {
      input: { saveView: { name: 'Retry me' } },
    });
    await waitForViewWrite(host, 'succeeded');
    snapshot = await snap(host);
    expect(snapshot.library!.viewWrite!.run).toBeGreaterThan(failedRun);
    expect(snapshot.library!.savedViews).toHaveLength(2);
    expect(snapshot.library!.savedViews![1].name).toBe('Retry me');
  });

  it('gives every validation refusal a new failed run and clears the old error on retry', async () => {
    const host = createMockHost();
    await host.invoke('library_input', {
      input: { setFilter: { provider: 'epic' } },
    });

    await host.invoke('library_input', { input: { saveView: { name: '   ' } } });
    let snapshot = await snap(host);
    const first = snapshot.library!.viewWrite!;
    expect(first).toMatchObject({ state: 'failed', error: 'empty' });

    await host.invoke('library_input', { input: { saveView: { name: '   ' } } });
    snapshot = await snap(host);
    const second = snapshot.library!.viewWrite!;
    expect(second).toMatchObject({ state: 'failed', error: 'empty' });
    expect(second.run).toBeGreaterThan(first.run);

    await host.invoke('library_input', {
      input: { saveView: { name: 'Steam 10 h+' } },
    });
    snapshot = await snap(host);
    expect(snapshot.library!.viewWrite).toMatchObject({
      state: 'failed',
      error: 'duplicate',
    });
    expect(snapshot.library!.viewError).toBe('duplicate');
    expect(snapshot.library!.savedViews).toHaveLength(1);

    await host.invoke('library_input', {
      input: { saveView: { name: 'Distinct' } },
    });
    snapshot = await snap(host);
    expect(snapshot.library!.viewError).toBeNull();
    expect(snapshot.library!.viewWrite!.state).toBe('pending');
    await waitForViewWrite(host, 'succeeded');
    snapshot = await snap(host);
    expect(snapshot.library!.savedViews).toHaveLength(2);
  });

  it('keeps a failed rename payload and id, then renames it on retry', async () => {
    const host = createMockHost({ viewWriteFailure: 'once' });
    let snapshot = await snap(host);
    const before = structuredClone(snapshot.library!.savedViews![0]);

    await host.invoke('library_input', {
      input: { renameView: { id: 1, name: 'Steam, long sessions' } },
    });
    snapshot = await snap(host);
    expect(snapshot.library!.viewWrite).toMatchObject({
      operation: 'rename',
      viewId: 1,
      state: 'pending',
    });
    await waitForViewWrite(host, 'failed');
    snapshot = await snap(host);
    expect(snapshot.library!.savedViews![0]).toEqual(before);
    expect(snapshot.library!.viewWrite).toMatchObject({
      state: 'failed',
      error: 'failed',
      savedViewId: null,
    });

    await host.invoke('library_input', {
      input: { renameView: { id: 1, name: 'Steam, long sessions' } },
    });
    await waitForViewWrite(host, 'succeeded');
    snapshot = await snap(host);
    expect(snapshot.library!.savedViews![0]).toMatchObject({
      id: before.id,
      name: 'Steam, long sessions',
      count: before.count,
    });
    expect(snapshot.library!.viewWrite!.savedViewId).toBe(before.id);

    // The rename kept the payload: applying the view restores its original filters and sort.
    await host.invoke('library_input', { input: { applyView: { id: before.id } } });
    snapshot = await snap(host);
    expect(snapshot.library!.layout.filters.stores).toEqual(['steam']);
    expect(snapshot.library!.layout.filters.playtime).toBe('atLeast10h');
    expect(snapshot.library!.layout.sort).toEqual({
      key: 'playtime',
      direction: 'descending',
    });
    expect(snapshot.library!.layout.group).toBe('store');
  });

  it('keeps the newest attempt result when an older write answers later', async () => {
    const host = createMockHost();
    await host.invoke('library_input', {
      input: { setFilter: { provider: 'epic' } },
    });

    await host.invoke('library_input', {
      input: { saveView: { name: 'Slow but valid' } },
    });
    let snapshot = await snap(host);
    const older = snapshot.library!.viewWrite!;
    expect(older.state).toBe('pending');

    // A newer attempt is refused immediately as a duplicate of the fixture view.
    await host.invoke('library_input', {
      input: { saveView: { name: 'Steam 10 h+' } },
    });
    snapshot = await snap(host);
    const newer = snapshot.library!.viewWrite!;
    expect(newer.state).toBe('failed');
    expect(newer.error).toBe('duplicate');
    expect(newer.run).toBeGreaterThan(older.run);

    // The older success still lands in the stored views, but it never replaces the newer run's
    // failed result with its own success.
    await new Promise((resolve) => setTimeout(resolve, 220));
    snapshot = await snap(host);
    expect(snapshot.library!.viewWrite).toMatchObject({
      run: newer.run,
      state: 'failed',
      error: 'duplicate',
    });
    expect(
      snapshot.library!.savedViews!.some((view) => view.name === 'Slow but valid'),
    ).toBe(true);
  });

  it('drops an in-flight write when the account changes and isolates a signed-out host', async () => {
    const signedOut = createMockHost({ signedOut: true });
    await signedOut.invoke('library_input', {
      input: { saveView: { name: 'No account' } },
    });
    expect((await snap(signedOut)).library).toBeNull();

    const host = createMockHost();
    await host.invoke('library_input', {
      input: { setFilter: { provider: 'epic' } },
    });
    await host.invoke('library_input', {
      input: { saveView: { name: 'Never lands' } },
    });
    await host.invoke('dispatch_auth', { input: 'signOut' });
    await new Promise((resolve) => setTimeout(resolve, 220));
    await host.invoke('dispatch_auth', { input: { startSignIn: 'google' } });
    const snapshot = await snap(host);
    expect(snapshot.library!.viewWrite).toBeNull();
    expect(snapshot.library!.savedViews).toHaveLength(1);
  });
});

describe('mock selection projection', () => {
  it('masks the selection, focus, actions and detail under a search and restores them', async () => {
    const host = createMockHost();
    await host.invoke('library_input', {
      input: { select: { key: 'steam:1', mode: 'replace' } },
    });
    await host.invoke('library_input', { input: { setDetailOpen: true } });
    let snapshot = await snap(host);
    expect(snapshot.library!.selected).toEqual(['steam:1']);
    expect(snapshot.library!.focused).toBe('steam:1');
    expect(snapshot.library!.detail!.row.key).toBe('steam:1');

    await host.invoke('library_input', { input: { setSearch: 'zzz no match' } });
    snapshot = await snap(host);
    expect(snapshot.library!.rows).toEqual([]);
    expect(snapshot.library!.selected).toEqual([]);
    expect(snapshot.library!.focused).toBeNull();
    expect(snapshot.library!.detail).toBeNull();
    expect(snapshot.library!.actions.selected).toBe(0);
    expect(snapshot.library!.actions.play).toEqual({
      disabled: 'nothingSelected',
    });

    await host.invoke('library_input', { input: { setSearch: '' } });
    snapshot = await snap(host);
    expect(snapshot.library!.selected).toEqual(['steam:1']);
    expect(snapshot.library!.focused).toBe('steam:1');
    expect(snapshot.library!.detail!.row.key).toBe('steam:1');
  });

  it('projects the selection in display order and masks it under a facet or scope', async () => {
    const host = createMockHost();
    await host.invoke('library_input', {
      input: { select: { key: 'gog:2', mode: 'replace' } },
    });
    await host.invoke('library_input', {
      input: { select: { key: 'steam:1', mode: 'toggle' } },
    });
    let snapshot = await snap(host);
    // Display order (title A–Z), not click order.
    expect(snapshot.library!.selected).toEqual(['steam:1', 'gog:2']);

    await host.invoke('library_input', {
      input: {
        setFilters: { ...createDefaultFilters(), favoritesOnly: true },
      },
    });
    snapshot = await snap(host);
    expect(snapshot.library!.selected).toEqual(['gog:2']);
    expect(snapshot.library!.detail!.row.key).toBe('gog:2');
    expect(snapshot.library!.rows.every((row) => row.isFavorite)).toBe(true);

    await host.invoke('library_input', {
      input: { setFilters: createDefaultFilters() },
    });
    snapshot = await snap(host);
    expect(snapshot.library!.selected).toEqual(['steam:1', 'gog:2']);

    // Neither selected row is in the Installed scope? Steam:1 is installed, GOG:2 is installed;
    // use a not-installed row to prove the scope mask and its restoration.
    await host.invoke('library_input', {
      input: { select: { key: 'steam:5', mode: 'replace' } },
    });
    await host.invoke('library_input', { input: { setScope: 'installed' } });
    snapshot = await snap(host);
    expect(snapshot.library!.selected).toEqual([]);
    expect(snapshot.library!.focused).toBeNull();
    await host.invoke('library_input', { input: { setScope: 'all' } });
    snapshot = await snap(host);
    expect(snapshot.library!.selected).toEqual(['steam:5']);
    expect(snapshot.library!.focused).toBe('steam:5');
  });

  it('prunes the selection when a group collapses, and never returns it on expand', async () => {
    const host = createMockHost();
    await host.invoke('library_input', { input: { setGroup: 'store' } });
    await host.invoke('library_input', {
      input: { select: { key: 'steam:1', mode: 'replace' } },
    });
    await host.invoke('library_input', {
      input: { toggleGroup: { id: 'steam' } },
    });
    let snapshot = await snap(host);
    expect(snapshot.library!.selected).toEqual([]);
    expect(snapshot.library!.focused).toBeNull();

    await host.invoke('library_input', {
      input: { toggleGroup: { id: 'steam' } },
    });
    snapshot = await snap(host);
    expect(snapshot.library!.selected).toEqual([]);
    expect(snapshot.library!.focused).toBeNull();
  });
});
