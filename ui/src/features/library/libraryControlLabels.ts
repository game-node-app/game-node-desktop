import type {
  AddedFilter,
  GameNodeStatusFilter,
  GroupBy,
  LastPlayedFilter,
  LibraryFilters,
  LibraryView,
  MatchFilterValue,
  PlayedFilter,
  PlaytimeThreshold,
  ReleaseFilter,
  SortDirection,
  SortKey,
  TitleSource,
} from '../../api';
import { createDefaultFilters } from '../../api';
import { providerLabel } from '../../i18n/providerLabel';
import { formatI18n } from './formatters';

export type Translate = (key: string) => string;

/** Prefix a facet name for an active-filter chip, for example "Store:". */
export function facetLabel(t: Translate, key: string): string {
  return `${t(key)}:`;
}

export function scopeTitle(id: string, t: Translate): string {
  switch (id) {
    case 'all':
      return t('Sidebar::All games');
    case 'installed':
      return t('Sidebar::Installed');
    case 'favorites':
      return t('Sidebar::Favorites');
    case 'recent':
      return t('Sidebar::Recently played');
    case 'running':
      return t('Sidebar::Running');
    default:
      return id;
  }
}

export function providerName(library: LibraryView | null | undefined, id: string, t: Translate): string {
  const provider = library?.providers.find((entry) => entry.id === id);
  return providerLabel(id, provider?.name, t);
}

export function storeCountName(
  library: LibraryView | null | undefined,
  id: string,
  t: Translate,
): string {
  // A known provider keeps its localized built-in name even when the engine's facet count
  // carries the manifest text (for example PackageKit's Portuguese manifest name).
  if (library?.providers.some((entry) => entry.id === id)) {
    return providerName(library, id, t);
  }
  const counted = library?.filterCounts.stores.find((store) => store.id === id);
  if (counted) return counted.name;
  return providerLabel(id, undefined, t);
}

/** The header title named by the engine's title source. */
export function titleSourceLabel(
  source: TitleSource | null | undefined,
  library: LibraryView | null | undefined,
  t: Translate,
): string {
  if (!source) return t('Sidebar::All games');
  switch (source.kind) {
    case 'scope':
      return scopeTitle(source.id, t);
    case 'store':
      return providerName(library, source.id, t);
    case 'collection': {
      const collection = library?.collections.find((entry) => entry.id === source.id);
      return collection?.name ?? source.id;
    }
    case 'savedView': {
      const id = Number(source.id);
      const view = library?.savedViews.find((entry) => entry.id === id);
      return view?.name ?? source.id;
    }
    default:
      return source.id;
  }
}

export function sortLabel(key: SortKey, direction: SortDirection, t: Translate): string {
  switch (key) {
    case 'title':
      return direction === 'descending' ? t('Library::Title (Z–A)') : t('Library::Title (A–Z)');
    case 'provider':
      return t('Library::Store');
    case 'matchStatus':
      return t('Library::Match status');
    case 'gameNodeStatus':
      return t('Library::GameNode status');
    case 'releaseYear':
      return t('Library::Release year');
    case 'playtime':
      return t('Library::Playtime');
    case 'lastPlayed':
      return t('Library::Recently played');
    case 'dateAdded':
      return t('Library::Recently added');
    default:
      return key;
  }
}

export function groupByLabel(group: GroupBy, t: Translate): string {
  switch (group) {
    case 'store':
      return t('Library::Store');
    case 'gameNodeStatus':
      return t('Library::GameNode status');
    case 'matchState':
      return t('Library::Match');
    case 'installation':
      return t('Library::Installation');
    case 'releaseDecade':
      return t('Library::Release decade');
    default:
      return t('Library::No grouping');
  }
}

export function releaseLabel(value: ReleaseFilter, t: Translate): string {
  if (typeof value === 'object' && 'decade' in value) {
    return formatI18n(t('Library::{}s'), value.decade);
  }
  if (value === 'earlier') return t('Library::Before 1990');
  if (value === 'unknown') return t('Library::Unknown');
  return t('Library::Any');
}

export function lastPlayedLabel(value: LastPlayedFilter, t: Translate): string {
  switch (value) {
    case 'last7Days':
      return t('Library::7 days');
    case 'last30Days':
      return t('Library::30 days');
    case 'last90Days':
      return t('Library::90 days');
    case 'lastYear':
      return t('Library::Last year');
    case 'olderThanYear':
      return t('Library::Older');
    default:
      return t('Library::Any');
  }
}

export function addedLabel(value: AddedFilter, t: Translate): string {
  switch (value) {
    case 'last7Days':
      return t('Library::This week');
    case 'last30Days':
      return t('Library::This month');
    case 'last90Days':
      return t('Library::90 days');
    case 'lastYear':
      return t('Library::This year');
    case 'olderThanYear':
      return t('Library::Earlier');
    default:
      return t('Library::Any');
  }
}

export function playtimeLabel(value: PlaytimeThreshold, t: Translate): string {
  switch (value) {
    case 'atLeast10h':
      return t('Library::10 h+');
    case 'atLeast100h':
      return t('Library::100 h+');
    default:
      return t('Library::Any');
  }
}

export function playedLabel(value: PlayedFilter, t: Translate): string {
  switch (value) {
    case 'played':
      return t('Library::Played');
    case 'unplayed':
      return t('Library::Never played');
    default:
      return t('Library::All');
  }
}

export function matchStateLabel(value: MatchFilterValue, t: Translate): string {
  switch (value) {
    case 'matched':
      return t('Library::Matched');
    case 'ambiguous':
      return t('Library::Needs review');
    case 'unmatched':
      return t('Library::No match');
    case 'noGame':
      return t('Library::Marked as not a game');
    default:
      return value;
  }
}

export function gameNodeStatusLabel(value: GameNodeStatusFilter, t: Translate): string {
  switch (value) {
    case 'playing':
      return t('Library::Playing');
    case 'finished':
      return t('Library::Finished');
    case 'planned':
      return t('Library::Planned');
    case 'ongoing':
      return t('Library::Ongoing');
    case 'dropped':
      return t('Library::Dropped');
    case 'inLibrary':
      return t('Library::In library');
    case 'notInLibrary':
      return t('Library::Not on GameNode');
    default:
      return value;
  }
}

/** One active facet rendered by `FilterChipsRow`; `remove` returns the next filters. */
export interface FilterChipModel {
  id: string;
  label: string;
  value: string;
  remove: () => LibraryFilters;
}

/**
 * Builds one chip per active facet. Multi-valued facets join their labels with
 * ", " exactly as `copy.md` asks. The models are pure: callers send `remove()`
 * through `libraryInput` and the search text is never part of a filter.
 */
export function buildFilterChips(
  library: LibraryView | null | undefined,
  t: Translate,
): FilterChipModel[] {
  const filters = library?.layout.filters ?? createDefaultFilters();
  const chips: FilterChipModel[] = [];

  const availabilityLabels: string[] = [];
  if (filters.availability.installed) availabilityLabels.push(t('Library::Installed on this PC'));
  if (filters.availability.owned) availabilityLabels.push(t('Library::In library'));
  if (filters.availability.shared) availabilityLabels.push(t('Library::Family shared'));
  if (availabilityLabels.length > 0) {
    chips.push({
      id: 'availability',
      label: facetLabel(t, 'Library::Availability'),
      value: availabilityLabels.join(', '),
      remove: () => ({
        ...filters,
        availability: { installed: false, owned: false, shared: false },
      }),
    });
  }

  if (filters.stores.length > 0) {
    chips.push({
      id: 'stores',
      label: facetLabel(t, 'Library::Store'),
      value: filters.stores
        .map((id) => storeCountName(library, id, t))
        .join(', '),
      remove: () => ({ ...filters, stores: [] }),
    });
  }

  if (filters.matchStates.length > 0) {
    chips.push({
      id: 'match',
      label: facetLabel(t, 'Library::Match'),
      value: filters.matchStates.map((value) => matchStateLabel(value, t)).join(', '),
      remove: () => ({ ...filters, matchStates: [] }),
    });
  }

  if (filters.gamenodeStatus.length > 0) {
    chips.push({
      id: 'gamenodeStatus',
      label: facetLabel(t, 'Library::GameNode status'),
      value: filters.gamenodeStatus
        .map((value) => gameNodeStatusLabel(value, t))
        .join(', '),
      remove: () => ({ ...filters, gamenodeStatus: [] }),
    });
  }

  if (filters.playtime !== 'any') {
    chips.push({
      id: 'playtime',
      label: facetLabel(t, 'Library::Playtime'),
      value: playtimeLabel(filters.playtime, t),
      remove: () => ({ ...filters, playtime: 'any' }),
    });
  }

  if (filters.played !== 'any') {
    chips.push({
      id: 'played',
      label: facetLabel(t, 'Library::History'),
      value: playedLabel(filters.played, t),
      remove: () => ({ ...filters, played: 'any' }),
    });
  }

  if (filters.lastPlayed !== 'any') {
    chips.push({
      id: 'lastPlayed',
      label: facetLabel(t, 'Library::Last session'),
      value: lastPlayedLabel(filters.lastPlayed, t),
      remove: () => ({ ...filters, lastPlayed: 'any' }),
    });
  }

  if (filters.release !== 'any') {
    chips.push({
      id: 'release',
      label: facetLabel(t, 'Library::Release'),
      value: releaseLabel(filters.release, t),
      remove: () => ({ ...filters, release: 'any' }),
    });
  }

  if (filters.added !== 'any') {
    chips.push({
      id: 'added',
      label: facetLabel(t, 'Library::Added'),
      value: addedLabel(filters.added, t),
      remove: () => ({ ...filters, added: 'any' }),
    });
  }

  if (filters.favoritesOnly) {
    chips.push({
      id: 'favorites',
      label: t('Library::Favorites only'),
      value: '',
      remove: () => ({ ...filters, favoritesOnly: false }),
    });
  }

  if (filters.runningOnly) {
    chips.push({
      id: 'running',
      label: t('Library::Running only'),
      value: '',
      remove: () => ({ ...filters, runningOnly: false }),
    });
  }

  if (filters.showHidden) {
    chips.push({
      id: 'showHidden',
      label: t('Library::Show hidden'),
      value: '',
      remove: () => ({ ...filters, showHidden: false }),
    });
  }

  return chips;
}
