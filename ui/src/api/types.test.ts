import { describe, expect, it } from 'vitest';
import {
  createDefaultCatalogView,
  createDefaultFilterCounts,
  createDefaultFilters,
  createDefaultLibraryLayout,
  createDefaultLibraryView,
  createDefaultScopeCounts,
  createDefaultSnapshot,
  createDefaultTitleSource,
  filtersAreDefault,
  getMatchedGameId,
  type LibraryFilters,
  type MatchStatus,
} from './types';

function filters(overrides: Partial<LibraryFilters> = {}): LibraryFilters {
  return { ...createDefaultFilters(), ...overrides };
}

describe('match status contract', () => {
  it('accepts the engine ambiguous object without treating it as matched', () => {
    const status: MatchStatus = { ambiguous: { candidates: [10, 11] } };

    expect(getMatchedGameId(status)).toBeNull();
    expect(getMatchedGameId({ matched: { gameId: 42, source: 'chosen' } })).toBe(42);
  });
});

describe('library filter defaults', () => {
  it('returns every facet at its default and reports the filters as default', () => {
    const filters = createDefaultFilters();

    expect(filters).toEqual({
      availability: { installed: false, owned: false, shared: false },
      stores: [],
      matchStates: [],
      gamenodeStatus: [],
      playtime: 'any',
      played: 'any',
      lastPlayed: 'any',
      release: 'any',
      added: 'any',
      favoritesOnly: false,
      runningOnly: false,
      showHidden: false,
    });
    expect(filtersAreDefault(filters)).toBe(true);
  });

  it.each<[string, Partial<LibraryFilters>]>([
    ['the installed availability fact', { availability: { installed: true, owned: false, shared: false } }],
    ['the owned availability fact', { availability: { installed: false, owned: true, shared: false } }],
    ['the shared availability fact', { availability: { installed: false, owned: false, shared: true } }],
    ['a store', { stores: ['steam'] }],
    ['a match state', { matchStates: ['ambiguous'] }],
    ['a GameNode status', { gamenodeStatus: ['playing'] }],
    ['a playtime threshold', { playtime: 'atLeast10h' }],
    ['a played choice', { played: 'unplayed' }],
    ['a last-session window', { lastPlayed: 'last30Days' }],
    ['a release decade', { release: { decade: 2010 } }],
    ['a release bucket', { release: 'earlier' }],
    ['an added window', { added: 'last7Days' }],
    ['favorites only', { favoritesOnly: true }],
    ['running only', { runningOnly: true }],
    ['show hidden', { showHidden: true }],
  ])('is not default with %s', (_label, overrides) => {
    expect(filtersAreDefault(filters(overrides))).toBe(false);
  });

  it('never shares mutable state between calls', () => {
    const first = createDefaultFilters();
    const second = createDefaultFilters();

    expect(first.availability).not.toBe(second.availability);
    expect(first.stores).not.toBe(second.stores);
    expect(first.matchStates).not.toBe(second.matchStates);
    expect(first.gamenodeStatus).not.toBe(second.gamenodeStatus);

    first.stores.push('steam');
    first.matchStates.push('matched');
    first.gamenodeStatus.push('finished');
    first.availability.installed = true;

    expect(second.stores).toEqual([]);
    expect(second.matchStates).toEqual([]);
    expect(second.gamenodeStatus).toEqual([]);
    expect(second.availability.installed).toBe(false);

    const layout = createDefaultLibraryLayout();
    expect(layout.filters).not.toBe(first);
    expect(layout.filters.stores).not.toBe(first.stores);
    layout.filters.stores.push('gog');
    expect(first.stores).toEqual(['steam']);
  });
});

describe('library layout defaults', () => {
  it('matches the engine layout defaults', () => {
    const layout = createDefaultLibraryLayout();

    expect(layout.view).toBe('grid');
    expect(layout.scope).toBe('all');
    expect(layout.sort).toEqual({ key: 'title', direction: 'ascending' });
    expect(layout.group).toBe('none');
    expect(layout.collapsedGroups).toEqual({});
    expect(layout.sidebarWidth).toBe(240);
    expect(layout.detailWidth).toBe(360);
    expect(layout.detailOpen).toBe(true);
    expect(layout.sidebarCollapsed).toBe(false);
    expect(filtersAreDefault(layout.filters)).toBe(true);
  });

  it('returns fresh layouts and nested collections', () => {
    const first = createDefaultLibraryLayout();
    const second = createDefaultLibraryLayout();

    expect(first).not.toBe(second);
    expect(first.filters).not.toBe(second.filters);
    expect(first.sort).not.toBe(second.sort);
    expect(first.collapsedGroups).not.toBe(second.collapsedGroups);
  });
});

describe('library view defaults', () => {
  it('sends every Rust field instead of optional placeholders', () => {
    const view = createDefaultLibraryView();

    expect(view.generation).toBe(0);
    expect(view.rows).toEqual([]);
    expect(view.filterCounts).toEqual(createDefaultFilterCounts());
    expect(view.scopeCounts).toEqual(createDefaultScopeCounts());
    expect(view.groups).toEqual([]);
    expect(view.savedViews).toEqual([]);
    expect(view.collections).toEqual([]);
    expect(view.viewError).toBeNull();
    expect(view.viewWrite).toBeNull();
    expect(view.lastSavedView).toBeNull();
    expect(view.lastHidden).toBeNull();
    expect(view.canSaveView).toBe(false);
    expect(view.titleSource).toEqual(createDefaultTitleSource());
    expect(view.catalog).toEqual(createDefaultCatalogView());
    expect(view.scanProgress).toEqual([]);
    expect(view.paletteIndex).toEqual([]);
    expect(view.focused).toBeNull();
    expect(view.detail).toBeNull();
    expect(view.empty).toBeNull();
    expect(view.matchDialog).toBeNull();
    expect(view.addDialog).toBeNull();
    expect(view.addSummary).toBeNull();
    expect(view.undo).toBeNull();
    expect(view.statusRun).toBeNull();
    expect(view.collectionRun).toBeNull();
    expect(view.actions.selected).toBe(0);
    expect(view.actions.play).toEqual({ disabled: 'nothingSelected' });
  });

  it('does not share arrays, objects or nested defaults between views', () => {
    const first = createDefaultLibraryView();
    const second = createDefaultLibraryView();

    expect(first.layout).not.toBe(second.layout);
    expect(first.filterCounts).not.toBe(second.filterCounts);
    expect(first.filterCounts.stores).not.toBe(second.filterCounts.stores);
    expect(first.scopeCounts).not.toBe(second.scopeCounts);
    expect(first.rows).not.toBe(second.rows);
    expect(first.savedViews).not.toBe(second.savedViews);
    expect(first.collections).not.toBe(second.collections);
    expect(first.scanProgress).not.toBe(second.scanProgress);
    expect(first.paletteIndex).not.toBe(second.paletteIndex);
    expect(first.catalog).not.toBe(second.catalog);
    expect(first.catalog.results).not.toBe(second.catalog.results);
    expect(first.actions).not.toBe(second.actions);

    first.rows.push({} as never);
    first.savedViews.push({ id: 1, name: 'One', count: 0, active: false });
    first.catalog.results.push({} as never);
    first.actions.selected = 3;

    expect(second.rows).toEqual([]);
    expect(second.savedViews).toEqual([]);
    expect(second.catalog.results).toEqual([]);
    expect(second.actions.selected).toBe(0);
  });
});

describe('default factories', () => {
  it('returns fresh filter and scope counts', () => {
    const firstCounts = createDefaultFilterCounts();
    const secondCounts = createDefaultFilterCounts();
    const firstScopes = createDefaultScopeCounts();
    const secondScopes = createDefaultScopeCounts();

    expect(firstCounts).not.toBe(secondCounts);
    expect(firstCounts.stores).not.toBe(secondCounts.stores);
    expect(firstCounts.availability).not.toBe(secondCounts.availability);
    expect(firstScopes).not.toBe(secondScopes);
  });

  it('returns a fresh catalog view', () => {
    const first = createDefaultCatalogView();
    const second = createDefaultCatalogView();

    expect(first).toEqual({ query: '', status: 'idle', results: [], incomplete: false });
    expect(first).not.toBe(second);
    expect(first.results).not.toBe(second.results);
  });

  it('returns a fresh snapshot with the new discovery timestamp default', () => {
    const first = createDefaultSnapshot();
    const second = createDefaultSnapshot();

    expect(first.localGames.discoveryLastCheckedAt).toBeNull();
    expect(first.localGames.watches).not.toBe(second.localGames.watches);
    expect(first.localGames.registrations).not.toBe(second.localGames.registrations);
    expect(first.tools.installedByEntry).not.toBe(second.tools.installedByEntry);
    expect(first.storeAccounts).not.toBe(second.storeAccounts);
    expect(first.tracking.sessionStartedAt).not.toBe(
      second.tracking.sessionStartedAt,
    );
    expect(first.ports.entries).not.toBe(second.ports.entries);
    expect(first.library).toBeNull();

    first.localGames.watches.push({
      id: 'w',
      path: '/games',
      enabled: true,
      entryCount: 0,
      lastCheckedAt: null,
      state: 'idle',
      unavailableSince: null,
    });
    first.tools.installedByEntry['steam:1'] = [];
    expect(second.localGames.watches).toEqual([]);
    expect(second.tools.installedByEntry).toEqual({});
  });

  it('returns a fresh title source', () => {
    const first = createDefaultTitleSource();
    const second = createDefaultTitleSource();

    expect(first).toEqual({ kind: 'scope', id: 'all' });
    expect(first).not.toBe(second);
  });
});
