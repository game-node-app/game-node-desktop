/**
 * The D3 query pipeline of `crates/core/src/library/query.rs`, in TypeScript.
 *
 * The mock host uses this for every snapshot so the browser preview counts, sorts and groups like
 * the engine. `host.test.ts` exercises it directly with custom rows.
 */
import type {
  FilterCounts,
  GameNodeStatusFilter,
  GroupBy,
  LibraryFilters,
  LibraryLayout,
  LibraryScope,
  MatchFilterValue,
  PlayedFilter,
  PlaytimeThreshold,
  ProviderCount,
  ReleaseFilter,
  Row,
  RowGroup,
  ScopeCounts,
  TitleSource,
} from '../api/types';

const DAY = 86_400;
const YEAR = 365 * DAY;
const RECENT_DAYS = 14;
const FIRST_RELEASE_DECADE = 1990;

const FACET_AVAILABILITY = 1 << 0;
const FACET_STORES = 1 << 1;
const FACET_MATCH_STATES = 1 << 2;
const FACET_GAMENODE_STATUS = 1 << 3;
const FACET_PLAYTIME = 1 << 4;
const FACET_PLAYED = 1 << 5;
const FACET_LAST_PLAYED = 1 << 6;
const FACET_RELEASE = 1 << 7;
const FACET_ADDED = 1 << 8;
const FACET_FAVORITES = 1 << 9;
const FACET_RUNNING = 1 << 10;
const ALL_FACETS = (1 << 11) - 1;

const MATCH_MATCHED = 1 << 0;
const MATCH_AMBIGUOUS = 1 << 1;
const MATCH_UNMATCHED = 1 << 2;
const MATCH_NO_GAME = 1 << 3;

const STATUS_PLAYING = 1 << 0;
const STATUS_FINISHED = 1 << 1;
const STATUS_PLANNED = 1 << 2;
const STATUS_ONGOING = 1 << 3;
const STATUS_DROPPED = 1 << 4;
const STATUS_IN_LIBRARY = 1 << 5;
const STATUS_NOT_IN_LIBRARY = 1 << 6;

const PLAYTIME_10H = 1 << 0;
const PLAYTIME_100H = 1 << 1;
const PLAYED = 1 << 0;
const UNPLAYED = 1 << 1;

const WINDOW_7 = 1 << 0;
const WINDOW_30 = 1 << 1;
const WINDOW_90 = 1 << 2;
const WINDOW_YEAR = 1 << 3;
const WINDOW_OLDER = 1 << 4;

export interface ProviderRef {
  id: string;
  name: string;
}

export interface RowFacts {
  facetPass: number;
  search: boolean;
  hidden: boolean;
  availability: number;
  matchStates: number;
  gamenodeStatus: number;
  playtime: number;
  played: number;
  lastPlayed: number;
  added: number;
  isFavorite: boolean;
  running: boolean;
}

export interface QueryInput {
  rows: Row[];
  layout: LibraryLayout;
  /** The effective scope while a saved view is highlighted; defaults to `layout.scope`. */
  effectiveScope?: LibraryScope;
  search: string;
  now: number;
  providers: ProviderRef[];
  gamenodeAvailable: boolean;
}

export interface QueryResult {
  total: number;
  installedTotal: number;
  installedInScope: number;
  visibleCount: number;
  rows: Row[];
  groups: RowGroup[];
  filterCounts: FilterCounts;
  scopeCounts: ScopeCounts;
  titleSource: TitleSource;
}

function within(at: number | null | undefined, now: number, window: number): boolean {
  return at != null && at >= now - window;
}

function windowMask(at: number | null | undefined, now: number): number {
  if (at == null) return 0;
  if (at < now - YEAR) return WINDOW_OLDER;
  if (at >= now - 7 * DAY) return WINDOW_7 | WINDOW_30 | WINDOW_90 | WINDOW_YEAR;
  if (at >= now - 30 * DAY) return WINDOW_30 | WINDOW_90 | WINDOW_YEAR;
  if (at >= now - 90 * DAY) return WINDOW_90 | WINDOW_YEAR;
  return WINDOW_YEAR;
}

function isAmbiguousStatus(status: Row['status']): boolean {
  return status === 'ambiguous' || (typeof status === 'object' && 'ambiguous' in status);
}

function matchMask(row: Row): number {
  if (typeof row.status === 'object' && 'matched' in row.status) return MATCH_MATCHED;
  if (isAmbiguousStatus(row.status)) return MATCH_AMBIGUOUS;
  if (row.status === 'unmatched') return MATCH_UNMATCHED;
  return MATCH_NO_GAME;
}

function gamenodeStatusMask(row: Row): number {
  const status = row.gamenode?.status;
  switch (status) {
    case 'playing':
      return STATUS_PLAYING;
    case 'finished':
      return STATUS_FINISHED;
    case 'planned':
      return STATUS_PLANNED;
    case 'ongoing':
      return STATUS_ONGOING;
    case 'dropped':
      return STATUS_DROPPED;
    case null:
    case undefined:
      return row.gamenode ? STATUS_IN_LIBRARY : STATUS_NOT_IN_LIBRARY;
  }
}

function playtimeMask(seconds: number | null | undefined): number {
  if (seconds == null) return 0;
  let mask = 0;
  if (seconds >= 10 * 3600) mask |= PLAYTIME_10H;
  if (seconds >= 100 * 3600) mask |= PLAYTIME_100H;
  return mask;
}

function playedMask(seconds: number | null | undefined): number {
  if (seconds == null) return 0;
  if (seconds > 0) return PLAYED;
  if (seconds === 0) return UNPLAYED;
  return 0;
}

function matchesAvailability(filters: LibraryFilters, row: Row): boolean {
  const availability = filters.availability;
  const any = availability.installed || availability.owned || availability.shared;
  return (
    !any ||
    (availability.installed && row.installed) ||
    (availability.owned && row.owned) ||
    (availability.shared && row.shared)
  );
}

function matchesStores(filters: LibraryFilters, row: Row): boolean {
  return filters.stores.length === 0 || filters.stores.includes(row.provider);
}

function matchesMatchStates(filters: MatchFilterValue[], mask: number): boolean {
  if (filters.length === 0) return true;
  return filters.some((state) => {
    const bit =
      state === 'matched'
        ? MATCH_MATCHED
        : state === 'ambiguous'
          ? MATCH_AMBIGUOUS
          : state === 'unmatched'
            ? MATCH_UNMATCHED
            : MATCH_NO_GAME;
    return (mask & bit) !== 0;
  });
}

function matchesGamenodeStatus(filters: GameNodeStatusFilter[], mask: number): boolean {
  if (filters.length === 0) return true;
  return filters.some((status) => {
    const bit =
      status === 'playing'
        ? STATUS_PLAYING
        : status === 'finished'
          ? STATUS_FINISHED
          : status === 'planned'
            ? STATUS_PLANNED
            : status === 'ongoing'
              ? STATUS_ONGOING
              : status === 'dropped'
                ? STATUS_DROPPED
                : status === 'inLibrary'
                  ? STATUS_IN_LIBRARY
                  : STATUS_NOT_IN_LIBRARY;
    return (mask & bit) !== 0;
  });
}

function matchesPlaytime(filter: PlaytimeThreshold, mask: number): boolean {
  if (filter === 'atLeast10h') return (mask & PLAYTIME_10H) !== 0;
  if (filter === 'atLeast100h') return (mask & PLAYTIME_100H) !== 0;
  return true;
}

function matchesPlayed(filter: PlayedFilter, mask: number): boolean {
  if (filter === 'played') return (mask & PLAYED) !== 0;
  if (filter === 'unplayed') return (mask & UNPLAYED) !== 0;
  return true;
}

function matchesWindow(filter: string, mask: number): boolean {
  switch (filter) {
    case 'last7Days':
      return (mask & WINDOW_7) !== 0;
    case 'last30Days':
      return (mask & WINDOW_30) !== 0;
    case 'last90Days':
      return (mask & WINDOW_90) !== 0;
    case 'lastYear':
      return (mask & WINDOW_YEAR) !== 0;
    case 'olderThanYear':
      return (mask & WINDOW_OLDER) !== 0;
    default:
      return true;
  }
}

function matchesRelease(filter: ReleaseFilter, year: number | null | undefined): boolean {
  if (typeof filter === 'object' && 'decade' in filter) {
    return year != null && year >= filter.decade && year < filter.decade + 10;
  }
  if (filter === 'earlier') return year != null && year < FIRST_RELEASE_DECADE;
  if (filter === 'unknown') return year == null;
  return true;
}

function contains(row: Row, needle: string): boolean {
  return (
    row.detectedName.toLowerCase().includes(needle) ||
    (row.matchedName ?? '').toLowerCase().includes(needle)
  );
}

export function buildRowFacts(row: Row, filters: LibraryFilters, needle: string, now: number): RowFacts {
  const availability =
    (row.installed ? 1 : 0) | (row.owned ? 2 : 0) | (row.shared ? 4 : 0);
  const matchStates = matchMask(row);
  const gamenodeStatus = gamenodeStatusMask(row);
  const playtime = playtimeMask(row.playtime.seconds);
  const played = playedMask(row.playtime.seconds);
  const lastPlayed = windowMask(row.playtime.lastPlayedAt, now);
  const added = windowMask(row.addedAt, now);

  let facetPass = 0;
  if (matchesAvailability(filters, row)) facetPass |= FACET_AVAILABILITY;
  if (matchesStores(filters, row)) facetPass |= FACET_STORES;
  if (matchesMatchStates(filters.matchStates, matchStates)) facetPass |= FACET_MATCH_STATES;
  if (matchesGamenodeStatus(filters.gamenodeStatus, gamenodeStatus)) facetPass |= FACET_GAMENODE_STATUS;
  if (matchesPlaytime(filters.playtime, playtime)) facetPass |= FACET_PLAYTIME;
  if (matchesPlayed(filters.played, played)) facetPass |= FACET_PLAYED;
  if (matchesWindow(filters.lastPlayed, lastPlayed)) facetPass |= FACET_LAST_PLAYED;
  if (matchesRelease(filters.release, row.releaseYear)) facetPass |= FACET_RELEASE;
  if (matchesWindow(filters.added, added)) facetPass |= FACET_ADDED;
  if (!filters.favoritesOnly || row.isFavorite) facetPass |= FACET_FAVORITES;
  if (!filters.runningOnly || row.running) facetPass |= FACET_RUNNING;

  return {
    facetPass,
    search: needle.length === 0 || contains(row, needle),
    hidden: row.hidden,
    availability,
    matchStates,
    gamenodeStatus,
    playtime,
    played,
    lastPlayed,
    added,
    isFavorite: row.isFavorite,
    running: row.running,
  };
}

export function scopeMatches(scope: LibraryScope, row: Row, now: number): boolean {
  if (typeof scope === 'object') {
    if ('collection' in scope) {
      return row.gamenode?.collectionIds.includes(scope.collection) ?? false;
    }
    // SavedView is only a highlight marker; applying it copies its payload into the layout.
    return true;
  }
  switch (scope) {
    case 'installed':
      return row.installed;
    case 'favorites':
      return row.isFavorite;
    case 'recent':
      return within(row.playtime.lastPlayedAt, now, RECENT_DAYS * DAY);
    case 'running':
      return row.running;
    default:
      return true;
  }
}

function passesOthers(facts: RowFacts, facet: number): boolean {
  return (facts.facetPass | facet) === ALL_FACETS;
}

function gamenodeStatusRank(row: Row): number {
  const mask = gamenodeStatusMask(row);
  if (mask === STATUS_PLAYING) return 0;
  if (mask === STATUS_ONGOING) return 1;
  if (mask === STATUS_PLANNED) return 2;
  if (mask === STATUS_FINISHED) return 3;
  if (mask === STATUS_DROPPED) return 4;
  if (mask === STATUS_IN_LIBRARY) return 5;
  return 6;
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

function compareTitles(a: Row, b: Row): number {
  return a.title.toLowerCase().localeCompare(b.title.toLowerCase()) || (a.title < b.title ? -1 : a.title > b.title ? 1 : 0);
}

function tieBreak(a: Row, b: Row, key: LibraryLayout['sort']['key']): number {
  if (key === 'title') {
    return a.provider.localeCompare(b.provider) || a.key.localeCompare(b.key);
  }
  if (key === 'gameNodeStatus') {
    return compareTitles(a, b) || a.key.localeCompare(b.key);
  }
  return compareTitles(a, b) || a.provider.localeCompare(b.provider) || a.key.localeCompare(b.key);
}

export function compareRows(a: Row, b: Row, sort: LibraryLayout['sort']): number {
  const descending = sort.direction === 'descending';
  const flip = (order: number) => (descending ? -order : order);
  switch (sort.key) {
    case 'provider':
      return flip(a.providerName.toLowerCase().localeCompare(b.providerName.toLowerCase()));
    case 'matchStatus': {
      const rank = (row: Row) =>
        typeof row.status === 'object' && 'matched' in row.status
          ? 0
          : isAmbiguousStatus(row.status)
            ? 1
            : row.status === 'unmatched'
              ? 2
              : 3;
      return flip(rank(a) - rank(b));
    }
    case 'gameNodeStatus':
      return flip(gamenodeStatusRank(a) - gamenodeStatusRank(b));
    case 'releaseYear':
      return compareOptional(a.releaseYear, b.releaseYear, descending);
    case 'playtime':
      return compareOptional(a.playtime.seconds, b.playtime.seconds, descending);
    case 'lastPlayed':
      return compareOptional(a.playtime.lastPlayedAt, b.playtime.lastPlayedAt, descending);
    case 'dateAdded':
      return compareOptional(a.addedAt, b.addedAt, descending);
    default: {
      const title = compareTitles(a, b);
      return flip(title);
    }
  }
}

export function sortRows(rows: Row[], sort: LibraryLayout['sort']): Row[] {
  return [...rows].sort((a, b) => compareRows(a, b, sort) || tieBreak(a, b, sort.key));
}

function groupCandidates(rows: Row[], by: GroupBy, providers: ProviderRef[]): Array<{ id: string; labelKey: string }> {
  switch (by) {
    case 'store': {
      const candidates = providers.map((provider) => ({ id: provider.id, labelKey: provider.name }));
      const known = new Set(candidates.map((candidate) => candidate.id));
      const extras = Array.from(
        new Set(rows.map((row) => row.provider).filter((id) => !known.has(id))),
      ).sort();
      candidates.push(...extras.map((id) => ({ id, labelKey: id })));
      return candidates;
    }
    case 'gameNodeStatus':
      return [
        { id: 'playing', labelKey: 'Library::Playing' },
        { id: 'finished', labelKey: 'Library::Finished' },
        { id: 'planned', labelKey: 'Library::Planned' },
        { id: 'ongoing', labelKey: 'Library::Ongoing' },
        { id: 'dropped', labelKey: 'Library::Dropped' },
        { id: 'inLibrary', labelKey: 'Library::In library' },
        { id: 'notInLibrary', labelKey: 'Library::Not on GameNode' },
      ];
    case 'matchState':
      return [
        { id: 'matched', labelKey: 'Library::Matched' },
        { id: 'ambiguous', labelKey: 'Library::Needs review' },
        { id: 'unmatched', labelKey: 'Library::No match' },
        { id: 'noGame', labelKey: 'Library::Marked as not a game' },
      ];
    case 'installation':
      return [
        { id: 'installed', labelKey: 'Library::Installed on this PC' },
        { id: 'notInstalled', labelKey: 'Library::Not installed' },
      ];
    case 'releaseDecade': {
      const decades = new Set<number>();
      let earlier = false;
      let unknown = false;
      for (const row of rows) {
        if (row.releaseYear == null) unknown = true;
        else if (row.releaseYear < FIRST_RELEASE_DECADE) earlier = true;
        else decades.add(Math.floor(row.releaseYear / 10) * 10);
      }
      const candidates = Array.from(decades)
        .sort((a, b) => b - a)
        .map((decade) => ({ id: String(decade), labelKey: 'Library::{}s' }));
      if (earlier) candidates.push({ id: 'earlier', labelKey: 'Library::Before 1990' });
      if (unknown) candidates.push({ id: 'unknown', labelKey: 'Library::Unknown release' });
      return candidates;
    }
    default:
      return [];
  }
}

export function groupId(row: Row, by: GroupBy): string {
  switch (by) {
    case 'store':
      return row.provider;
    case 'gameNodeStatus': {
      const mask = gamenodeStatusMask(row);
      if (mask === STATUS_PLAYING) return 'playing';
      if (mask === STATUS_FINISHED) return 'finished';
      if (mask === STATUS_PLANNED) return 'planned';
      if (mask === STATUS_ONGOING) return 'ongoing';
      if (mask === STATUS_DROPPED) return 'dropped';
      return mask === STATUS_IN_LIBRARY ? 'inLibrary' : 'notInLibrary';
    }
    case 'matchState': {
      if (typeof row.status === 'object' && 'matched' in row.status) return 'matched';
      if (isAmbiguousStatus(row.status)) return 'ambiguous';
      return row.status === 'noGame' ? 'noGame' : 'unmatched';
    }
    case 'installation':
      return row.installed ? 'installed' : 'notInstalled';
    case 'releaseDecade':
      if (row.releaseYear == null) return 'unknown';
      if (row.releaseYear < FIRST_RELEASE_DECADE) return 'earlier';
      return String(Math.floor(row.releaseYear / 10) * 10);
    default:
      return '';
  }
}

export function groupRows(
  rows: Row[],
  by: GroupBy,
  collapsedGroups: string[],
  providers: ProviderRef[],
): { rows: Row[]; groups: RowGroup[] } {
  if (by === 'none') return { rows, groups: [] };
  const collapsed = new Set(collapsedGroups);
  const candidates = groupCandidates(rows, by, providers);
  const buckets = new Map<string, Row[]>(candidates.map((candidate) => [candidate.id, []]));
  const extras: Row[] = [];
  for (const row of rows) {
    const bucket = buckets.get(groupId(row, by));
    if (bucket) bucket.push(row);
    else extras.push(row);
  }
  const grouped: Row[] = [];
  const groups: RowGroup[] = [];
  for (const candidate of candidates) {
    const bucket = buckets.get(candidate.id) ?? [];
    if (bucket.length === 0) continue;
    const firstIndex = grouped.length;
    const isCollapsed = collapsed.has(candidate.id);
    if (!isCollapsed) grouped.push(...bucket);
    groups.push({
      id: candidate.id,
      labelKey: candidate.labelKey,
      count: bucket.length,
      collapsed: isCollapsed,
      firstIndex,
    });
  }
  if (extras.length > 0) {
    groups.push({
      id: 'other',
      labelKey: 'Library::Unknown',
      count: extras.length,
      collapsed: false,
      firstIndex: grouped.length,
    });
    grouped.push(...extras);
  }
  return { rows: grouped, groups };
}

function titleSourceFor(layout: LibraryLayout, providers: ProviderRef[]): TitleSource {
  const scope = layout.scope;
  if (typeof scope === 'object') {
    if ('collection' in scope) return { kind: 'collection', id: scope.collection };
    return { kind: 'savedView', id: String(scope.savedView) };
  }
  if (scope === 'all') {
    if (
      layout.filters.stores.length === 1 &&
      providers.some((provider) => provider.id === layout.filters.stores[0])
    ) {
      return { kind: 'store', id: layout.filters.stores[0] };
    }
    return { kind: 'scope', id: 'all' };
  }
  return { kind: 'scope', id: scope };
}

function emptyCounts(): FilterCounts {
  return {
    stores: [],
    availability: { installed: 0, owned: 0, shared: 0 },
    matchStates: { matched: 0, ambiguous: 0, unmatched: 0, noGame: 0 },
    gamenodeStatus: {
      playing: 0,
      finished: 0,
      planned: 0,
      ongoing: 0,
      dropped: 0,
      inLibrary: 0,
      notInLibrary: 0,
      available: false,
    },
    playtime: { any: 0, atLeast10h: 0, atLeast100h: 0 },
    played: { any: 0, played: 0, unplayed: 0 },
    lastPlayed: {
      any: 0,
      last7Days: 0,
      last30Days: 0,
      last90Days: 0,
      lastYear: 0,
      olderThanYear: 0,
    },
    release: [],
    added: {
      any: 0,
      last7Days: 0,
      last30Days: 0,
      last90Days: 0,
      lastYear: 0,
      olderThanYear: 0,
    },
    favoritesOnly: 0,
    runningOnly: 0,
    showHidden: 0,
  };
}

export function runLibraryQuery(input: QueryInput): QueryResult {
  const { rows, layout, search, now, providers, gamenodeAvailable } = input;
  const scope = input.effectiveScope ?? layout.scope;
  const filters = layout.filters;
  const needle = search.trim().toLowerCase();
  const prepared = rows.map((row) => ({ row, facts: buildRowFacts(row, filters, needle, now) }));
  const hiddenAllowed = (facts: RowFacts) => filters.showHidden || !facts.hidden;
  const scoped = (row: Row, facts: RowFacts) =>
    hiddenAllowed(facts) && facts.search && scopeMatches(scope, row, now);
  const visible = prepared.filter(({ row, facts }) => scoped(row, facts) && facts.facetPass === ALL_FACETS);

  const filterCounts = emptyCounts();
  filterCounts.gamenodeStatus.available = gamenodeAvailable;

  // Store counts ignore both the store facet and the active scope.
  const storeCounts = new Map<string, number>();
  for (const { row, facts } of prepared) {
    if (!hiddenAllowed(facts) || !facts.search || !passesOthers(facts, FACET_STORES)) continue;
    storeCounts.set(row.provider, (storeCounts.get(row.provider) ?? 0) + 1);
  }
  const storeRefs = [...providers];
  const knownStores = new Set(storeRefs.map((provider) => provider.id));
  for (const id of Array.from(storeCounts.keys()).sort()) {
    if (!knownStores.has(id)) storeRefs.push({ id, name: id });
  }
  filterCounts.stores = storeRefs.map((provider) => ({
    id: provider.id,
    name: provider.name,
    count: storeCounts.get(provider.id) ?? 0,
  }));

  let showHidden = 0;
  for (const { row, facts } of prepared) {
    if (!scoped(row, facts)) continue;

    if (passesOthers(facts, FACET_AVAILABILITY)) {
      if (facts.availability & 1) filterCounts.availability.installed += 1;
      if (facts.availability & 2) filterCounts.availability.owned += 1;
      if (facts.availability & 4) filterCounts.availability.shared += 1;
    }
    if (passesOthers(facts, FACET_MATCH_STATES)) {
      if (facts.matchStates & MATCH_MATCHED) filterCounts.matchStates.matched += 1;
      if (facts.matchStates & MATCH_AMBIGUOUS) filterCounts.matchStates.ambiguous += 1;
      if (facts.matchStates & MATCH_UNMATCHED) filterCounts.matchStates.unmatched += 1;
      if (facts.matchStates & MATCH_NO_GAME) filterCounts.matchStates.noGame += 1;
    }
    if (passesOthers(facts, FACET_GAMENODE_STATUS)) {
      if (facts.gamenodeStatus & STATUS_PLAYING) filterCounts.gamenodeStatus.playing += 1;
      if (facts.gamenodeStatus & STATUS_FINISHED) filterCounts.gamenodeStatus.finished += 1;
      if (facts.gamenodeStatus & STATUS_PLANNED) filterCounts.gamenodeStatus.planned += 1;
      if (facts.gamenodeStatus & STATUS_ONGOING) filterCounts.gamenodeStatus.ongoing += 1;
      if (facts.gamenodeStatus & STATUS_DROPPED) filterCounts.gamenodeStatus.dropped += 1;
      if (facts.gamenodeStatus & STATUS_IN_LIBRARY) filterCounts.gamenodeStatus.inLibrary += 1;
      if (facts.gamenodeStatus & STATUS_NOT_IN_LIBRARY) filterCounts.gamenodeStatus.notInLibrary += 1;
    }
    if (passesOthers(facts, FACET_PLAYTIME)) {
      filterCounts.playtime.any += 1;
      if (facts.playtime & PLAYTIME_10H) filterCounts.playtime.atLeast10h += 1;
      if (facts.playtime & PLAYTIME_100H) filterCounts.playtime.atLeast100h += 1;
    }
    if (passesOthers(facts, FACET_PLAYED)) {
      filterCounts.played.any += 1;
      if (facts.played & PLAYED) filterCounts.played.played += 1;
      if (facts.played & UNPLAYED) filterCounts.played.unplayed += 1;
    }
    if (passesOthers(facts, FACET_LAST_PLAYED)) {
      filterCounts.lastPlayed.any += 1;
      if (facts.lastPlayed & WINDOW_7) filterCounts.lastPlayed.last7Days += 1;
      if (facts.lastPlayed & WINDOW_30) filterCounts.lastPlayed.last30Days += 1;
      if (facts.lastPlayed & WINDOW_90) filterCounts.lastPlayed.last90Days += 1;
      if (facts.lastPlayed & WINDOW_YEAR) filterCounts.lastPlayed.lastYear += 1;
      if (facts.lastPlayed & WINDOW_OLDER) filterCounts.lastPlayed.olderThanYear += 1;
    }
    if (passesOthers(facts, FACET_ADDED)) {
      filterCounts.added.any += 1;
      if (facts.added & WINDOW_7) filterCounts.added.last7Days += 1;
      if (facts.added & WINDOW_30) filterCounts.added.last30Days += 1;
      if (facts.added & WINDOW_90) filterCounts.added.last90Days += 1;
      if (facts.added & WINDOW_YEAR) filterCounts.added.lastYear += 1;
      if (facts.added & WINDOW_OLDER) filterCounts.added.olderThanYear += 1;
    }
    if (facts.isFavorite && passesOthers(facts, FACET_FAVORITES)) {
      filterCounts.favoritesOnly += 1;
    }
    if (facts.running && passesOthers(facts, FACET_RUNNING)) {
      filterCounts.runningOnly += 1;
    }
  }

  const decades = new Map<number, number>();
  let earlier = 0;
  let unknown = 0;
  for (const { row, facts } of prepared) {
    if (!scoped(row, facts) || !passesOthers(facts, FACET_RELEASE)) continue;
    if (row.releaseYear == null) unknown += 1;
    else if (row.releaseYear < FIRST_RELEASE_DECADE) earlier += 1;
    else {
      const decade = Math.floor(row.releaseYear / 10) * 10;
      decades.set(decade, (decades.get(decade) ?? 0) + 1);
    }
  }
  filterCounts.release = Array.from(decades.entries())
    .sort(([a], [b]) => b - a)
    .map(([decade, count]) => ({ value: { decade }, count }));
  if (earlier > 0) filterCounts.release.push({ value: 'earlier', count: earlier });
  if (unknown > 0) filterCounts.release.push({ value: 'unknown', count: unknown });

  const scopeCounts: ScopeCounts = { all: 0, installed: 0, favorites: 0, recent: 0, running: 0 };
  for (const { row, facts } of prepared) {
    if (!hiddenAllowed(facts) || !facts.search || !passesOthers(facts, FACET_STORES)) continue;
    scopeCounts.all += 1;
    if (row.installed) scopeCounts.installed += 1;
    if (row.isFavorite) scopeCounts.favorites += 1;
    if (within(row.playtime.lastPlayedAt, now, RECENT_DAYS * DAY)) scopeCounts.recent += 1;
    if (row.running) scopeCounts.running += 1;
  }

  for (const { row, facts } of prepared) {
    if (facts.hidden && facts.search && scopeMatches(scope, row, now) && facts.facetPass === ALL_FACETS) {
      showHidden += 1;
    }
  }
  filterCounts.showHidden = showHidden;

  const total = prepared.filter(({ facts }) => hiddenAllowed(facts)).length;
  const installedTotal = prepared.filter(
    ({ row, facts }) => row.installed && hiddenAllowed(facts),
  ).length;
  const installedInScope = prepared.filter(
    ({ row, facts }) =>
      row.installed &&
      hiddenAllowed(facts) &&
      scopeMatches(scope, row, now) &&
      matchesStores(filters, row),
  ).length;

  const sorted = sortRows(visible.map(({ row }) => row), layout.sort);
  const collapsed = layout.collapsedGroups[layout.group] ?? [];
  const grouped = groupRows(sorted, layout.group, collapsed, providers);
  return {
    total,
    installedTotal,
    installedInScope,
    visibleCount: sorted.length,
    rows: grouped.rows,
    groups: grouped.groups,
    filterCounts,
    scopeCounts,
    titleSource: titleSourceFor(layout, providers),
  };
}

/** Mirrors `ProviderCount` from a query's store counts plus the raw per-provider facts. */
export function providerCounts(
  rows: Row[],
  stores: FilterCounts['stores'],
  showHidden: boolean,
): ProviderCount[] {
  return stores.map((store) => {
    const own = rows.filter(
      (row) => row.provider === store.id && (showHidden || !row.hidden),
    );
    return {
      id: store.id,
      name: store.name,
      count: store.count,
      installedCount: own.filter((row) => row.installed).length,
      ownedCount: own.filter((row) => row.owned).length,
      sharedCount: own.filter((row) => row.shared && !row.owned).length,
      sharedLabel: store.id === 'steam' ? 'Steam Family' : null,
    };
  });
}
