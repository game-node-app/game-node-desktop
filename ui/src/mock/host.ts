/** An isolated, in-memory host for interactive UI development. */
import {
  createDefaultFilters,
  createDefaultSnapshot,
  getMatchedGameId,
  type AuthInput,
  type LibraryFilters,
  type LibraryInput,
  type LibraryView,
  type ProviderCount,
  type Row,
  type Snapshot,
  type StorageMethod,
  type StoreAccountState,
  type StoreAccountView,
} from '../api/types';

type Listener = (snapshot: Snapshot) => void;

export type MockSteamScenario =
  | 'notConnected'
  | 'checking'
  | 'signingIn'
  | 'connected'
  | 'expired'
  | 'unreachable'
  | 'credentialError'
  | 'persistFailed';

const titles = ['Aster Vale', 'Beyond the Pines', 'Clockwork Harbor', 'Distant Skies', 'Emberfall', 'Forgotten Atlas', 'Glass Garden', 'Hollow Mountain', 'Infinite Coast', 'Juniper Station', 'Kingdom of Rain', 'Lunar Circuit'];

/**
 * The mock library mixes installed, owned-only and family-shared rows so every
 * indicator and count can be exercised from `VITE_UI_MOCK=1`.
 */
const NOW = Math.floor(Date.now() / 1000);

const allRows: Row[] = titles.map((title, index) => {
  const steam = index % 2 === 0;
  const installed = steam ? index === 0 || index === 2 || index === 8 : true;
  const familyShared = index === 4;
  const owned = steam && !familyShared;
  return {
    key: `${steam ? 'steam' : 'gog'}:${index + 1}`,
    title, detectedName: title, matchedName: index === 2 ? null : title,
    provider: steam ? 'steam' : 'gog', providerName: steam ? 'Steam' : 'GOG',
    installed, owned, familyShared,
    running: false, status: index === 2 ? 'unmatched' : { matched: { gameId: index + 1, source: 'automatic' } },
    coverFile: `mock-${index}.svg`,
    playtime: {
      seconds: index * 3720,
      source: 'store',
      lastPlayedAt: index === 0 ? null : NOW - index * 86_400,
    },
    releaseYear: index === 2 ? null : 1992 + index * 2,
    addedAt: NOW - index * 3 * 86_400,
    isFavorite: index === 1 || index === 3,
  };
});

const DAY = 86_400;
const YEAR = 365 * DAY;

function within(at: number | null | undefined, now: number, window: number): boolean {
  return at != null && at >= now - window;
}

/** The same facet rules as `crates/core/src/library/query.rs`, in memory. */
function matchesFilters(row: Row, filters: LibraryFilters, now: number): boolean {
  const availability = filters.availability;
  const anyAvailability = availability.installed || availability.owned || availability.family;
  if (
    anyAvailability &&
    !(
      (availability.installed && row.installed) ||
      (availability.owned && row.owned) ||
      (availability.family && row.familyShared)
    )
  ) return false;
  if (filters.runningOnly && !row.running) return false;
  switch (filters.matchState) {
    case 'matched':
      if (typeof row.status !== 'object' || !('matched' in row.status)) return false;
      break;
    case 'ambiguous':
      if (row.status !== 'ambiguous') return false;
      break;
    case 'unmatched':
      if (row.status !== 'unmatched') return false;
      break;
    case 'noGame':
      if (row.status !== 'noGame') return false;
      break;
    default:
      break;
  }
  const seconds = row.playtime.seconds ?? null;
  switch (filters.playtime) {
    case 'unplayed':
      if (seconds !== 0) return false;
      break;
    case 'played':
      if (seconds == null || seconds <= 0) return false;
      break;
    case 'atLeast10h':
      if (seconds == null || seconds < 10 * 3600) return false;
      break;
    case 'atLeast100h':
      if (seconds == null || seconds < 100 * 3600) return false;
      break;
    default:
      break;
  }
  const lastPlayed = row.playtime.lastPlayedAt ?? null;
  switch (filters.lastPlayed) {
    case 'last7Days':
      if (!within(lastPlayed, now, 7 * DAY)) return false;
      break;
    case 'last30Days':
      if (!within(lastPlayed, now, 30 * DAY)) return false;
      break;
    case 'lastYear':
      if (!within(lastPlayed, now, YEAR)) return false;
      break;
    default:
      break;
  }
  const release = filters.release;
  const year = row.releaseYear ?? null;
  if (typeof release === 'object' && 'decade' in release) {
    if (year == null || year < release.decade || year >= release.decade + 10) return false;
  } else if (release === 'earlier') {
    if (year == null || year >= 1980) return false;
  } else if (release === 'unknown' && year != null) {
    return false;
  }
  switch (filters.added) {
    case 'last7Days':
      if (!within(row.addedAt, now, 7 * DAY)) return false;
      break;
    case 'last30Days':
      if (!within(row.addedAt, now, 30 * DAY)) return false;
      break;
    case 'last90Days':
      if (!within(row.addedAt, now, 90 * DAY)) return false;
      break;
    case 'lastYear':
      if (!within(row.addedAt, now, YEAR)) return false;
      break;
    default:
      break;
  }
  if (filters.favoritesOnly && !row.isFavorite) return false;
  return true;
}

function compareOptional(
  a: number | null | undefined,
  b: number | null | undefined,
  descending: boolean,
): number {
  const left = a ?? null;
  const right = b ?? null;
  if (left == null && right == null) return 0;
  if (left == null) return 1;
  if (right == null) return -1;
  return descending ? right - left : left - right;
}

function compareRows(a: Row, b: Row, sort: { key: LibraryView['layout']['sort']['key']; direction: 'ascending' | 'descending' }): number {
  const descending = sort.direction === 'descending';
  const flip = (order: number) => (descending ? -order : order);
  switch (sort.key) {
    case 'provider':
      return flip(a.providerName.localeCompare(b.providerName));
    case 'matchStatus': {
      const rank = (row: Row) =>
        typeof row.status === 'object' && 'matched' in row.status
          ? 0
          : row.status === 'ambiguous'
            ? 1
            : row.status === 'unmatched'
              ? 2
              : 3;
      return flip(rank(a) - rank(b));
    }
    case 'releaseYear':
      return compareOptional(a.releaseYear, b.releaseYear, descending);
    case 'playtime':
      return compareOptional(a.playtime.seconds, b.playtime.seconds, descending);
    case 'lastPlayed':
      return compareOptional(a.playtime.lastPlayedAt, b.playtime.lastPlayedAt, descending);
    case 'dateAdded':
      return compareOptional(a.addedAt, b.addedAt, descending);
    default:
      return flip(a.title.localeCompare(b.title));
  }
}

function summarizeProvider(filtered: Row[], raw: Row[], id: string, name: string): ProviderCount {
  const providerRows = filtered.filter((row) => row.provider === id);
  const rawRows = raw.filter((row) => row.provider === id);
  return {
    id,
    name,
    count: providerRows.length,
    installedCount: rawRows.filter((row) => row.installed).length,
    ownedCount: rawRows.filter((row) => row.owned).length,
    familyCount: rawRows.filter((row) => row.familyShared && !row.owned).length,
  };
}

function providerCounts(filtered: Row[], raw: Row[]): ProviderCount[] {
  return [
    summarizeProvider(filtered, raw, 'steam', 'Steam'),
    summarizeProvider(filtered, raw, 'gog', 'GOG'),
  ];
}

function createLibrary(source: Row[]): LibraryView {
  return {
    generation: 1, layout: { view: 'grid', sort: { key: 'title', direction: 'ascending' }, filter: 'all', filters: createDefaultFilters(), sidebarWidth: 216, detailWidth: 320, detailOpen: false },
    search: '', access: 'verified', total: source.length, installedTotal: source.filter((row) => row.installed).length,
    providers: providerCounts(source, source),
    rows: structuredClone(source), scanning: false, failedProviders: [], selected: [], focused: null,
    actions: { selected: 0, play: { disabled: 'nothingSelected' }, install: { disabled: 'nothingSelected' }, matchGame: { disabled: 'nothingSelected' }, addToGamenode: { disabled: 'nothingSelected' }, changeMatch: false },
  };
}

/**
 * Optional large-library fixture (`?library=large`): 320 owned Steam games, exactly 18 installed,
 * and a few family-shared games, so virtualization, counts and the Installed filter can be checked
 * against a realistic catalog. It is additive; the default scenario stays at 12 games.
 */
function createLargeLibraryRows(): Row[] {
  const rows: Row[] = [];
  for (let index = 0; index < 320; index += 1) {
    const installed = index < 18;
    rows.push({
      key: `steam:${1000 + index}`,
      title: `Steam Saga ${index + 1}`,
      detectedName: `Steam Saga ${index + 1}`,
      matchedName: index % 5 === 0 ? null : `Steam Saga ${index + 1}`,
      provider: 'steam',
      providerName: 'Steam',
      installed,
      owned: true,
      familyShared: false,
      running: false,
      status: index % 5 === 0 ? 'unmatched' : { matched: { gameId: index + 1, source: 'automatic' } },
      coverFile: `mock-${index % 12}.svg`,
      playtime: { seconds: installed ? index * 600 : null, source: 'store', lastPlayedAt: installed ? NOW - index * 3600 : null },
      releaseYear: 2000 + (index % 25),
      addedAt: NOW - index * DAY,
      isFavorite: false,
    });
  }
  for (let index = 0; index < 8; index += 1) {
    rows.push({
      key: `steam:${5000 + index}`,
      title: `Family Shelf ${index + 1}`,
      detectedName: `Family Shelf ${index + 1}`,
      matchedName: null,
      provider: 'steam',
      providerName: 'Steam',
      installed: false,
      owned: false,
      familyShared: true,
      running: false,
      status: 'unmatched',
      coverFile: `mock-${index % 12}.svg`,
      playtime: { seconds: null, source: 'store', lastPlayedAt: null },
      releaseYear: null,
      addedAt: NOW - index * DAY,
      isFavorite: false,
    });
  }
  return rows;
}

function installedOnlyRows(source: Row[]): Row[] {
  return source
    .map((row) => row.provider === 'steam' ? { ...row, owned: false, familyShared: false } : { ...row })
    .filter((row) => row.installed || row.owned || row.familyShared);
}

function makeStoreAccount(
  scenario: MockSteamScenario,
  counts: Pick<StoreAccountView, 'ownedCount' | 'familyCount' | 'installedCount'>,
  storage: StorageMethod,
): StoreAccountView {
  const disconnected = scenario === 'notConnected';
  const signingIn = scenario === 'signingIn';
  const state: StoreAccountState = scenario === 'persistFailed' ? 'connected' : scenario;
  const identity = scenario === 'notConnected' || signingIn
    ? null
    : { accountId: '76561198000000000', accountName: 'cassi' };
  const catalogError =
    scenario === 'expired' ? 'expired' : scenario === 'unreachable' ? 'unreachable' : null;

  return {
    provider: 'steam',
    state,
    accountId: identity?.accountId ?? null,
    accountName: identity?.accountName ?? null,
    ownedCount: disconnected ? 0 : counts.ownedCount,
    familyCount: disconnected ? 0 : counts.familyCount,
    installedCount: counts.installedCount,
    lastCheckedAt: disconnected || signingIn ? null : Math.floor(Date.now() / 1000) - 120,
    lastSyncedAt: state === 'connected' ? Math.floor(Date.now() / 1000) : null,
    storage,
    persistFailed: scenario === 'persistFailed',
    catalogError,
    familyError: null,
    canSignIn: scenario === 'notConnected' || scenario === 'expired' || scenario === 'credentialError',
    canCancelSignIn: signingIn,
    canFocusSignIn: signingIn,
    canSignOut: scenario === 'connected' || scenario === 'expired' || scenario === 'unreachable' || scenario === 'persistFailed',
    canCheck: scenario === 'connected' || scenario === 'unreachable' || scenario === 'credentialError' || scenario === 'persistFailed',
    canRemoveCredential: scenario === 'credentialError',
  };
}

export function createMockHost(options: { signedOut?: boolean; locale?: string; storeAccount?: MockSteamScenario; steamStorage?: StorageMethod; largeLibrary?: boolean } = {}) {
  const listeners = new Set<Listener>();
  const snapshot = createDefaultSnapshot();
  const baseRows = options.largeLibrary ? createLargeLibraryRows() : allRows;
  let rows = structuredClone(baseRows);
  let steamScenario: MockSteamScenario = options.storeAccount ?? 'connected';
  const steamStorage: StorageMethod = options.steamStorage ?? 'secretService';
  let steamGeneration = 0;
  let previousSteamScenario: MockSteamScenario =
    steamScenario === 'signingIn' ? 'notConnected' : steamScenario;

  function steamCounts() {
    return {
      ownedCount: rows.filter((row) => row.provider === 'steam' && row.owned).length,
      familyCount: rows.filter((row) => row.provider === 'steam' && row.familyShared && !row.owned).length,
      installedCount: rows.filter((row) => row.provider === 'steam' && row.installed).length,
    };
  }

  function applySteamScenario(scenario: MockSteamScenario) {
    steamScenario = scenario;
    snapshot.storeAccounts = [makeStoreAccount(scenario, steamCounts(), steamStorage)];
  }

  function clearSteamCatalog() {
    rows = rows
      .map((row) => row.provider === 'steam' ? { ...row, owned: false, familyShared: false } : row)
      .filter((row) => row.installed || row.owned || row.familyShared);
  }

  function scheduleSteam(delay: number, action: () => void) {
    const generation = ++steamGeneration;
    setTimeout(() => {
      if (generation !== steamGeneration) return;
      action();
    }, delay);
  }

  function signIn() {
    snapshot.auth = { signedIn: { user: { id: 'mock-user', username: 'UI Developer', email: 'ui@example.invalid' } } };
    rows = steamScenario === 'notConnected' ? installedOnlyRows(baseRows) : structuredClone(baseRows);
    snapshot.library = createLibrary(rows);
  }
  if (!options.signedOut) signIn();
  applySteamScenario(steamScenario);
  const copy = () => structuredClone(snapshot);
  function updateLibrary() {
    const lib = snapshot.library;
    if (!lib) return;
    const filter = lib.layout.filter;
    const sort = lib.layout.sort;
    const filters = lib.layout.filters;
    const visible = rows.filter((row) => row.installed || row.owned || row.familyShared);
    const byFacets = visible
      .filter((row) => matchesFilters(row, filters, NOW))
      .filter((row) => row.title.toLowerCase().includes(lib.search.toLowerCase()));
    lib.total = visible.length;
    lib.installedTotal = visible.filter((row) => row.installed).length;
    lib.providers = providerCounts(byFacets, visible);
    lib.rows = byFacets
      .filter((row) => filter === 'all' || row.provider === filter.provider)
      .sort((a, b) => compareRows(a, b, sort));
    const availability = filters.availability;
    const installedOnly =
      availability.installed && !availability.owned && !availability.family;
    if (visible.length === 0) {
      lib.empty = { noGames: { stores: ['Steam', 'GOG'] } };
    } else if (installedOnly && lib.installedTotal === 0) {
      lib.empty = { noInstalled: { provider: filter === 'all' ? null : filter.provider } };
    } else if (lib.rows.length === 0) {
      lib.empty = 'nothingMatches';
    } else {
      lib.empty = null;
    }
    const selected = rows.filter((row) => lib.selected.includes(row.key));
    const one = selected.length === 1 ? selected[0] : undefined;
    const disabled = { disabled: 'nothingSelected' as const };
    lib.actions = { selected: selected.length, play: one ? 'available' : disabled, install: disabled,
      matchGame: one ? 'available' : disabled, addToGamenode: selected.some((row) => typeof row.status === 'object') ? 'available' : disabled, changeMatch: !!one };
    lib.detail = one ? { row: one, running: false, installDir: `/games/${one.title}`, releaseYear: one.releaseYear ?? 2022, hasArtwork: true, membership: 'notInLibrary' } : null;
  }
  function publish() { updateLibrary(); const next = copy(); listeners.forEach((listener) => listener(next)); }
  function libraryInput(input: LibraryInput) {
    const lib = snapshot.library;
    if (!lib) return;
    if (typeof input === 'object') {
      if ('setSearch' in input) lib.search = input.setSearch;
      else if ('setView' in input) lib.layout.view = input.setView;
      else if ('setFilter' in input) lib.layout.filter = input.setFilter;
      else if ('setFilters' in input) lib.layout.filters = input.setFilters;
      else if ('setFavorite' in input) {
        const row = rows.find((item) => item.key === input.setFavorite.key);
        const gameId = row ? getMatchedGameId(row.status) : null;
        if (gameId != null) rows = rows.map((item) => getMatchedGameId(item.status) === gameId ? { ...item, isFavorite: input.setFavorite.favorite } : item);
      }
      else if ('setSort' in input) lib.layout.sort = input.setSort;
      else if ('setDetailOpen' in input) lib.layout.detailOpen = input.setDetailOpen;
      else if ('setDetailWidth' in input) lib.layout.detailWidth = input.setDetailWidth;
      else if ('setSidebarWidth' in input) lib.layout.sidebarWidth = input.setSidebarWidth;
      else if ('select' in input) {
        const { key, mode } = input.select;
        lib.selected = mode === 'toggle' ? (lib.selected.includes(key) ? lib.selected.filter((item) => item !== key) : [...lib.selected, key]) : [key];
        lib.focused = key;
      } else if ('move' in input && lib.rows.length) {
        const index = Math.max(0, lib.rows.findIndex((row) => row.key === lib.focused));
        const direction = input.move.direction;
        const next = direction === 'home' ? 0 : direction === 'end' ? lib.rows.length - 1 : Math.max(0, Math.min(lib.rows.length - 1, index + (direction === 'left' || direction === 'up' ? -1 : 1)));
        lib.focused = lib.rows[next].key; lib.selected = [lib.focused];
      } else if ('setMatchQuery' in input && lib.matchDialog) lib.matchDialog.query = input.setMatchQuery;
      else if ('pickMatch' in input && lib.matchDialog) {
        const row = rows.find((item) => item.key === lib.matchDialog?.entryKey);
        if (row) { row.status = { matched: { gameId: input.pickMatch.gameId, source: 'chosen' } }; row.matchedName = row.title; }
        lib.matchDialog = null;
      } else if ('setAddStatus' in input && lib.addDialog) lib.addDialog.status = input.setAddStatus;
      else if ('toggleAddCollection' in input && lib.addDialog && typeof lib.addDialog.collections === 'object' && 'loaded' in lib.addDialog.collections) {
        const collection = lib.addDialog.collections.loaded.find((item) => item.id === input.toggleAddCollection);
        if (collection) collection.picked = !collection.picked;
      }
    } else if (input === 'openDetails') lib.layout.detailOpen = true;
    else if (input === 'closeMatch') lib.matchDialog = null;
    else if (input === 'openMatch') {
      const row = rows.find((item) => item.key === lib.focused);
      if (row) lib.matchDialog = { entryKey: row.key, detectedName: row.detectedName, providerName: row.providerName, query: row.title, searching: false, candidates: [{ gameId: 100, name: row.title, current: false }], results: [], noResults: false, current: row.status, canClear: typeof row.status === 'object' };
    } else if (input === 'searchMatch' && lib.matchDialog) {
      lib.matchDialog.results = [{ gameId: 100, name: lib.matchDialog.query, current: false }];
    } else if (input === 'openAdd') {
      lib.addDialog = { targets: rows.filter((row) => lib.selected.includes(row.key)).map((row) => ({ entryKey: row.key, title: row.title, matched: typeof row.status === 'object' })), editingExisting: false, status: 'playing', appliedStatus: 'playing', collections: { loaded: [{ id: 'favorites', name: 'Favorites', picked: false }] }, canConfirm: true };
    } else if (input === 'closeAdd' || input === 'confirmAdd') lib.addDialog = null;
  }
  return {
    subscribe(listener: Listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    async invoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (cmd === 'get_snapshot') return copy();
      if (cmd === 'get_app_info') {
        return { locale: options.locale ?? 'en', version: 'mock', backgroundSupported: false, logDirectory: '/tmp/gamenode-logs' };
      }
      if (cmd === 'get_license_notices') return 'Mock UI development session';
      if (cmd === 'get_recent_logs') return '2026-01-01T00:00:00Z INFO gamenode_app: mock log line';
      if (cmd === 'open_logs_folder') return undefined;
      if (cmd === 'dispatch_auth') {
        const input = args.input as AuthInput;
        if (input === 'signOut') { snapshot.auth = 'signedOut'; snapshot.library = null; }
        else if (typeof input === 'object' && 'startSignIn' in input) signIn();
      } else if (cmd === 'library_input') libraryInput(args.input as LibraryInput);
      else if (cmd === 'remove_entry') {
        const key = args.key as string;
        rows = rows.filter((row) => row.key !== key);
        const lib = snapshot.library;
        if (lib) {
          lib.selected = lib.selected.filter((item) => item !== key);
          if (lib.focused === key) lib.focused = null;
        }
      } else if (cmd === 'remove_local_game') { /* the mock has no DRM-free registrations */ }
      else if (cmd === 'set_sync_enabled') snapshot.sync.enabled = args.enabled as boolean;
      else if (cmd === 'dismiss_sync_explanation') snapshot.sync.explained = true;
      else if (cmd === 'set_playtime_source') snapshot.playtimeSource = args.source as Snapshot['playtimeSource'];
      else if (cmd === 'steam_sign_in') {
        previousSteamScenario = steamScenario === 'signingIn' ? 'notConnected' : steamScenario;
        applySteamScenario('signingIn');
        scheduleSteam(1500, () => {
          rows = structuredClone(baseRows);
          applySteamScenario('connected');
          publish();
        });
      } else if (cmd === 'steam_cancel_sign_in') {
        steamGeneration += 1;
        applySteamScenario(steamScenario === 'signingIn' ? previousSteamScenario : steamScenario);
      } else if (cmd === 'steam_focus_sign_in') { /* the mock has no separate window to focus */ }
      else if (cmd === 'steam_sign_out') {
        steamGeneration += 1;
        clearSteamCatalog();
        applySteamScenario('notConnected');
      } else if (cmd === 'steam_check') {
        applySteamScenario('checking');
        scheduleSteam(900, () => {
          rows = structuredClone(baseRows);
          applySteamScenario('connected');
          publish();
        });
      } else if (cmd === 'rescan_library' || cmd === 'update_window_geometry' || cmd === 'dismiss_launch') { /* no external effects */ }
      else { console.warn(`[UI mock] Unsupported command: ${cmd}`); return; }
      publish();
    },
  };
}

function isMockSteamScenario(value: string | null): value is MockSteamScenario {
  return value === 'notConnected' || value === 'checking' || value === 'signingIn'
    || value === 'connected' || value === 'expired' || value === 'unreachable'
    || value === 'credentialError' || value === 'persistFailed';
}

/** `?steam=expired` (or any `MockSteamScenario`) selects a store-account fixture. */
const requestedSteamScenario = new URLSearchParams(window.location.search).get('steam');
const requestedParams = new URLSearchParams(window.location.search);

export const mockHost = createMockHost({
  signedOut: requestedParams.get('scenario') === 'signed-out',
  locale: requestedParams.get('locale') === 'pt-BR' ? 'pt-BR' : 'en',
  storeAccount: isMockSteamScenario(requestedSteamScenario) ? requestedSteamScenario : undefined,
  largeLibrary: requestedParams.get('library') === 'large',
});
