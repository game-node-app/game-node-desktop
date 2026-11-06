/** An isolated, in-memory host for interactive UI development. */
import { createDefaultSnapshot, type AuthInput, type LibraryInput, type LibraryView, type Row, type Snapshot } from '../api/types';

type Listener = (snapshot: Snapshot) => void;
const titles = ['Aster Vale', 'Beyond the Pines', 'Clockwork Harbor', 'Distant Skies', 'Emberfall', 'Forgotten Atlas', 'Glass Garden', 'Hollow Mountain', 'Infinite Coast', 'Juniper Station', 'Kingdom of Rain', 'Lunar Circuit'];
const allRows: Row[] = titles.map((title, index) => ({
  key: `${index % 2 ? 'gog' : 'steam'}:${index + 1}`,
  title, detectedName: title, matchedName: index === 2 ? null : title,
  provider: index % 2 ? 'gog' : 'steam', providerName: index % 2 ? 'GOG' : 'Steam',
  running: false, status: index === 2 ? 'unmatched' : { matched: { gameId: index + 1, source: 'automatic' } },
  coverFile: `mock-${index}.svg`, playtime: { seconds: index * 3720, source: 'store' },
}));

function createLibrary(): LibraryView {
  return {
    generation: 1, layout: { view: 'grid', sort: { key: 'title', direction: 'ascending' }, filter: 'all', sidebarWidth: 216, detailWidth: 320, detailOpen: false },
    search: '', access: 'verified', total: allRows.length,
    providers: [{ id: 'steam', name: 'Steam', count: 6 }, { id: 'gog', name: 'GOG', count: 6 }],
    rows: structuredClone(allRows), scanning: false, failedProviders: [], selected: [], focused: null,
    actions: { selected: 0, play: { disabled: 'nothingSelected' }, install: { disabled: 'nothingSelected' }, matchGame: { disabled: 'nothingSelected' }, addToGamenode: { disabled: 'nothingSelected' }, changeMatch: false },
  };
}

export function createMockHost(options: { signedOut?: boolean; locale?: string } = {}) {
  const listeners = new Set<Listener>();
  let snapshot = createDefaultSnapshot();
  let rows = structuredClone(allRows);
  function signIn() {
    snapshot.auth = { signedIn: { user: { id: 'mock-user', username: 'UI Developer', email: 'ui@example.invalid' } } };
    snapshot.library = createLibrary();
    rows = structuredClone(allRows);
  }
  if (!options.signedOut) signIn();
  const copy = () => structuredClone(snapshot);
  function updateLibrary() {
    const lib = snapshot.library;
    if (!lib) return;
    const filter = lib.layout.filter;
    const sort = lib.layout.sort;
    lib.rows = rows.filter(row => (filter === 'all' || row.provider === filter.provider) && row.title.toLowerCase().includes(lib.search.toLowerCase()))
      .sort((a, b) => {
        const key = sort.key === 'provider' ? 'providerName' : 'title';
        const left = sort.key === 'matchStatus' ? JSON.stringify(a.status) : a[key];
        const right = sort.key === 'matchStatus' ? JSON.stringify(b.status) : b[key];
        return left.localeCompare(right) * (sort.direction === 'ascending' ? 1 : -1);
      });
    lib.empty = lib.rows.length ? null : (rows.length ? 'nothingMatches' : { noGames: { stores: ['Steam', 'GOG'] } });
    const selected = rows.filter(row => lib.selected.includes(row.key));
    const one = selected.length === 1 ? selected[0] : undefined;
    const disabled = { disabled: 'nothingSelected' as const };
    lib.actions = { selected: selected.length, play: one ? 'available' : disabled, install: disabled,
      matchGame: one ? 'available' : disabled, addToGamenode: selected.some(row => typeof row.status === 'object') ? 'available' : disabled, changeMatch: !!one };
    lib.detail = one ? { row: one, running: false, installDir: `/games/${one.title}`, releaseYear: 2022, hasArtwork: true, membership: 'notInLibrary' } : null;
  }
  function publish() { updateLibrary(); const next = copy(); listeners.forEach(listener => listener(next)); }
  function libraryInput(input: LibraryInput) {
    const lib = snapshot.library;
    if (!lib) return;
    if (typeof input === 'object') {
      if ('setSearch' in input) lib.search = input.setSearch;
      else if ('setView' in input) lib.layout.view = input.setView;
      else if ('setFilter' in input) lib.layout.filter = input.setFilter;
      else if ('setSort' in input) lib.layout.sort = input.setSort;
      else if ('setDetailOpen' in input) lib.layout.detailOpen = input.setDetailOpen;
      else if ('setDetailWidth' in input) lib.layout.detailWidth = input.setDetailWidth;
      else if ('setSidebarWidth' in input) lib.layout.sidebarWidth = input.setSidebarWidth;
      else if ('select' in input) {
        const { key, mode } = input.select;
        lib.selected = mode === 'toggle' ? (lib.selected.includes(key) ? lib.selected.filter(item => item !== key) : [...lib.selected, key]) : [key];
        lib.focused = key;
      } else if ('move' in input && lib.rows.length) {
        const index = Math.max(0, lib.rows.findIndex(row => row.key === lib.focused));
        const direction = input.move.direction;
        const next = direction === 'home' ? 0 : direction === 'end' ? lib.rows.length - 1 : Math.max(0, Math.min(lib.rows.length - 1, index + (direction === 'left' || direction === 'up' ? -1 : 1)));
        lib.focused = lib.rows[next].key; lib.selected = [lib.focused];
      } else if ('setMatchQuery' in input && lib.matchDialog) lib.matchDialog.query = input.setMatchQuery;
      else if ('pickMatch' in input && lib.matchDialog) {
        const row = rows.find(item => item.key === lib.matchDialog?.entryKey);
        if (row) { row.status = { matched: { gameId: input.pickMatch.gameId, source: 'chosen' } }; row.matchedName = row.title; }
        lib.matchDialog = null;
      } else if ('setAddStatus' in input && lib.addDialog) lib.addDialog.status = input.setAddStatus;
      else if ('toggleAddCollection' in input && lib.addDialog && typeof lib.addDialog.collections === 'object' && 'loaded' in lib.addDialog.collections) {
        const collection = lib.addDialog.collections.loaded.find(item => item.id === input.toggleAddCollection);
        if (collection) collection.picked = !collection.picked;
      }
    } else if (input === 'openDetails') lib.layout.detailOpen = true;
    else if (input === 'closeMatch') lib.matchDialog = null;
    else if (input === 'openMatch') {
      const row = rows.find(item => item.key === lib.focused);
      if (row) lib.matchDialog = { entryKey: row.key, detectedName: row.detectedName, providerName: row.providerName, query: row.title, searching: false, candidates: [{ gameId: 100, name: row.title, current: false }], results: [], noResults: false, current: row.status, canClear: typeof row.status === 'object' };
    } else if (input === 'searchMatch' && lib.matchDialog) {
      lib.matchDialog.results = [{ gameId: 100, name: lib.matchDialog.query, current: false }];
    } else if (input === 'openAdd') {
      lib.addDialog = { targets: rows.filter(row => lib.selected.includes(row.key)).map(row => ({ entryKey: row.key, title: row.title, matched: typeof row.status === 'object' })), editingExisting: false, status: 'playing', appliedStatus: 'playing', collections: { loaded: [{ id: 'favorites', name: 'Favorites', picked: false }] }, canConfirm: true };
    } else if (input === 'closeAdd' || input === 'confirmAdd') lib.addDialog = null;
  }
  return {
    subscribe(listener: Listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    async invoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (cmd === 'get_snapshot') return copy();
      if (cmd === 'get_bootstrap_state') return { snapshot: copy(), locale: options.locale ?? 'en', version: 'mock', licenseNotices: 'Mock UI development session', backgroundSupported: false };
      if (cmd === 'dispatch_auth') {
        const input = args.input as AuthInput;
        if (input === 'signOut') { snapshot.auth = 'signedOut'; snapshot.library = null; }
        else if (typeof input === 'object' && 'startSignIn' in input) signIn();
      } else if (cmd === 'library_input') libraryInput(args.input as LibraryInput);
      else if (cmd === 'set_sync_enabled') snapshot.sync.enabled = args.enabled as boolean;
      else if (cmd === 'dismiss_sync_explanation') snapshot.sync.explained = true;
      else if (cmd === 'set_playtime_source') snapshot.playtimeSource = args.source as Snapshot['playtimeSource'];
      else if (cmd === 'rescan_library' || cmd === 'update_window_geometry' || cmd === 'dismiss_launch') { /* no external effects */ }
      else { console.warn(`[UI mock] Unsupported command: ${cmd}`); return; }
      publish();
    },
  };
}

export const mockHost = createMockHost({
  signedOut: new URLSearchParams(window.location.search).get('scenario') === 'signed-out',
  locale: new URLSearchParams(window.location.search).get('locale') === 'pt-BR' ? 'pt-BR' : 'en',
});
