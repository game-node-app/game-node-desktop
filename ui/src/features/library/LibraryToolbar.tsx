import React, { useContext, useMemo, useState } from 'react';
import { Checkbox, MantineContext, MantineProvider, Popover, Switch } from '@mantine/core';
import {
  IconFilter,
  IconLayoutGrid,
  IconLayoutList,
  IconLayoutSidebarRight,
  IconRefresh,
  IconSortAscending,
  IconSortDescending,
  IconStar,
  IconStarFilled,
  IconX,
} from '@tabler/icons-react';
import {
  commands,
  useLibraryView,
  useSnapshot,
  createDefaultFilters,
  defaultSortDirection,
  filtersAreDefault,
  type AddedFilter,
  type LastPlayedFilter,
  type LibraryFilters,
  type MatchFilter,
  type PlaytimeFilter,
  type ReleaseFilter,
  type SortDirection,
  type SortKey,
} from '../../api';
import { appTheme } from '../../AppProviders';
import { openSettingsModal } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { formatI18n } from './formatters';

const bannerBase =
  'border-b border-border-subtle px-md py-compact text-xs';
const controlBase =
  'h-8 rounded-sm border border-border-subtle bg-surface text-text-default transition-colors hover:border-brand/60 hover:text-on-primary';
const toggleBase =
  'flex h-8 cursor-pointer items-center gap-1.5 rounded-sm border px-2.5 text-xs transition-colors';

const RELEASE_OPTIONS = [2020, 2010, 2000, 1990, 1980];

/** The accessible label of the direction button names the resulting order for the key. */
function directionLabel(
  key: SortKey,
  direction: SortDirection,
  t: (key: string) => string,
): string {
  const descending = direction === 'descending';
  switch (key) {
    case 'title':
    case 'provider':
      return descending ? t('Library::Z → A') : t('Library::A → Z');
    case 'matchStatus':
      return descending
        ? t('Library::Worst match first')
        : t('Library::Best match first');
    case 'releaseYear':
      return descending
        ? t('Library::Newest first')
        : t('Library::Oldest first');
    case 'playtime':
      return descending
        ? t('Library::Most played first')
        : t('Library::Least played first');
    case 'lastPlayed':
      return descending
        ? t('Library::Recently played first')
        : t('Library::Least recently played first');
    case 'dateAdded':
      return descending
        ? t('Library::Newest added first')
        : t('Library::Oldest added first');
  }
}

interface Chip {
  id: string;
  label: string;
  clear: () => LibraryFilters;
}

function activeChips(filters: LibraryFilters, t: (key: string) => string): Chip[] {
  const chips: Chip[] = [];
  const availability = (patch: Partial<LibraryFilters['availability']>) => ({
    ...filters,
    availability: { ...filters.availability, ...patch },
  });
  if (filters.availability.installed) {
    chips.push({
      id: 'installed',
      label: t('Library::Installed'),
      clear: () => availability({ installed: false }),
    });
  }
  if (filters.availability.owned) {
    chips.push({
      id: 'owned',
      label: t('Library::Owned'),
      clear: () => availability({ owned: false }),
    });
  }
  if (filters.availability.family) {
    chips.push({
      id: 'family',
      label: t('Library::Family shared'),
      clear: () => availability({ family: false }),
    });
  }
  if (filters.runningOnly) {
    chips.push({
      id: 'running',
      label: t('Library::Running'),
      clear: () => ({ ...filters, runningOnly: false }),
    });
  }
  if (filters.matchState !== 'any') {
    chips.push({
      id: 'match',
      label: matchLabel(filters.matchState, t),
      clear: () => ({ ...filters, matchState: 'any' }),
    });
  }
  if (filters.playtime !== 'any') {
    chips.push({
      id: 'playtime',
      label: playtimeLabel(filters.playtime, t),
      clear: () => ({ ...filters, playtime: 'any' }),
    });
  }
  if (filters.lastPlayed !== 'any') {
    chips.push({
      id: 'lastPlayed',
      label: formatI18n(t('Library::Last played: {}'), lastPlayedLabel(filters.lastPlayed, t)),
      clear: () => ({ ...filters, lastPlayed: 'any' }),
    });
  }
  if (filters.release !== 'any') {
    chips.push({
      id: 'release',
      label: releaseLabel(filters.release, t),
      clear: () => ({ ...filters, release: 'any' }),
    });
  }
  if (filters.added !== 'any') {
    chips.push({
      id: 'added',
      label: formatI18n(t('Library::Added: {}'), addedLabel(filters.added, t)),
      clear: () => ({ ...filters, added: 'any' }),
    });
  }
  if (filters.favoritesOnly) {
    chips.push({
      id: 'favorites',
      label: t('Library::Favorites'),
      clear: () => ({ ...filters, favoritesOnly: false }),
    });
  }
  return chips;
}

function matchLabel(value: MatchFilter, t: (key: string) => string): string {
  switch (value) {
    case 'matched':
      return t('Library::Matched');
    case 'ambiguous':
      return t('Library::Needs review');
    case 'unmatched':
      return t('Library::Not matched');
    case 'noGame':
      return t('Library::Marked as no game');
    default:
      return t('Library::Any');
  }
}

function playtimeLabel(value: PlaytimeFilter, t: (key: string) => string): string {
  switch (value) {
    case 'unplayed':
      return t('Library::Unplayed');
    case 'played':
      return t('Library::Played');
    case 'atLeast10h':
      return t('Library::10 h+');
    case 'atLeast100h':
      return t('Library::100 h+');
    default:
      return t('Library::Any');
  }
}

function lastPlayedLabel(value: LastPlayedFilter, t: (key: string) => string): string {
  switch (value) {
    case 'last7Days':
      return t('Library::Last 7 days');
    case 'last30Days':
      return t('Library::Last 30 days');
    case 'lastYear':
      return t('Library::Last year');
    default:
      return t('Library::Any');
  }
}

function releaseLabel(value: ReleaseFilter, t: (key: string) => string): string {
  if (typeof value === 'object' && 'decade' in value) {
    return `${value.decade}s`;
  }
  if (value === 'earlier') return t('Library::Earlier');
  if (value === 'unknown') return t('Library::Unknown');
  return t('Library::Any');
}

function addedLabel(value: AddedFilter, t: (key: string) => string): string {
  switch (value) {
    case 'last7Days':
      return t('Library::Last 7 days');
    case 'last30Days':
      return t('Library::Last 30 days');
    case 'last90Days':
      return t('Library::Last 90 days');
    case 'lastYear':
      return t('Library::Last year');
    default:
      return t('Library::Any');
  }
}

interface FilterSelectProps {
  label: string;
  testId: string;
  value: string;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
}

const FilterSelect: React.FC<FilterSelectProps> = ({
  label,
  testId,
  value,
  options,
  onChange,
}) => (
  <label className="flex flex-col gap-1">
    <span className="text-[11px] font-semibold uppercase tracking-[0.4px] text-text-dimmed">
      {label}
    </span>
    <select
      data-testid={testId}
      aria-label={label}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className="h-control w-full rounded-sm border border-border-subtle bg-surface px-2 text-sm text-text-default outline-none focus:border-brand"
    >
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  </label>
);

export const LibraryToolbar: React.FC = () => {
  const library = useLibraryView();
  const { data: storeAccounts } = useSnapshot((s) => s.storeAccounts);
  const { t } = useI18n();
  const [filtersOpen, setFiltersOpen] = useState(false);
  const mantineContext = useContext(MantineContext);

  const search = library?.search ?? '';
  const sort = library?.layout?.sort ?? {
    key: 'title' as SortKey,
    direction: 'ascending' as SortDirection,
  };
  const view = library?.layout?.view ?? 'grid';
  const detailOpen = library?.layout?.detailOpen ?? false;
  const filters = library?.layout?.filters ?? createDefaultFilters();
  const total = library?.total ?? 0;
  const filtered = library?.rows?.length ?? 0;
  const scanning = library?.scanning ?? false;
  const failedProviders = library?.failedProviders ?? [];
  const access = library?.access ?? 'verified';
  const steamAccount = storeAccounts?.find((account) => account.provider === 'steam');
  const steamExpired = steamAccount?.state === 'expired';
  const steamUnreachable = steamAccount?.state === 'unreachable';

  const chips = useMemo(() => activeChips(filters, t), [filters, t]);

  const setFilters = (next: LibraryFilters) => {
    commands.libraryInput({ setFilters: next });
  };

  const handleFixInSettings = () => {
    openSettingsModal('stores');
  };

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    commands.libraryInput({ setSearch: e.target.value });
  };

  const handleClearSearch = () => {
    commands.libraryInput({ setSearch: '' });
  };

  const handleSortKeyChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const key = e.target.value as SortKey;
    commands.libraryInput({
      setSort: {
        key,
        // Choosing a key starts at the direction that answers its question best.
        direction: defaultSortDirection(key),
      },
    });
  };

  const handleToggleSortDirection = () => {
    commands.libraryInput({
      setSort: {
        key: sort.key,
        direction: sort.direction === 'ascending' ? 'descending' : 'ascending',
      },
    });
  };

  const handleToggleFavorites = () => {
    setFilters({ ...filters, favoritesOnly: !filters.favoritesOnly });
  };

  const handleClearFilters = () => {
    setFilters(createDefaultFilters());
  };

  const handleViewGrid = () => {
    commands.libraryInput({ setView: 'grid' });
  };

  const handleViewList = () => {
    commands.libraryInput({ setView: 'list' });
  };

  const handleToggleDetails = () => {
    commands.libraryInput({ setDetailOpen: !detailOpen });
  };

  const handleRescan = () => {
    commands.rescanLibrary();
  };

  const toggleClass = (active: boolean) =>
    active
      ? 'border-brand bg-tertiary text-on-primary'
      : 'border-border-subtle bg-surface text-text-default hover:border-brand/60';

  const any = () => ({ value: 'any', label: t('Library::Any') });

  const releaseValue =
    typeof filters.release === 'object' && 'decade' in filters.release
      ? String(filters.release.decade)
      : filters.release;

  const content = (
    <div className="flex w-full shrink-0 flex-col">
      {/* Status Banners */}
      {scanning && (
        <div
          data-testid="banner-scanning"
          className={`${bannerBase} bg-accent-subtle text-text-default`}
        >
          {t('Library::Scanning for installed games…')}
        </div>
      )}

      {failedProviders.length > 0 && (
        <div
          data-testid="banner-failed-providers"
          className={`${bannerBase} bg-warning-subtle text-warning`}
        >
          {formatI18n(
            t('Library::Could not scan {}. Showing the last results.'),
            failedProviders.join(', '),
          )}
        </div>
      )}

      {access === 'offline' && (
        <div
          data-testid="banner-offline"
          className={`${bannerBase} bg-accent-subtle text-text-default`}
        >
          {t('Library::GameNode cannot be reached. Showing your saved library.')}
        </div>
      )}

      {access === 'unverified' && (
        <div
          data-testid="banner-unverified"
          className={`${bannerBase} bg-accent-subtle text-text-default`}
        >
          {t(
            'Library::Showing your saved library. Matching and adding to GameNode wait for your session to be verified.',
          )}
        </div>
      )}

      {steamExpired && (
        <div
          data-testid="banner-steam-expired"
          className={`${bannerBase} flex flex-wrap items-center gap-compact bg-warning-subtle text-warning`}
        >
          <span>
            {t('Library::Steam sign-in expired. Showing your saved Steam library.')}
          </span>
          <button
            type="button"
            data-testid="banner-steam-fix-btn"
            onClick={handleFixInSettings}
            className="cursor-pointer rounded-xs border border-warning bg-transparent px-2 py-0.5 text-xs font-medium text-warning transition-colors hover:bg-surface"
          >
            {t('Library::Fix in Settings')}
          </button>
        </div>
      )}

      {steamUnreachable && (
        <div
          data-testid="banner-steam-unreachable"
          className={`${bannerBase} flex flex-wrap items-center gap-compact bg-warning-subtle text-warning`}
        >
          <span>
            {t('Library::Steam cannot be reached. Showing your saved Steam library.')}
          </span>
          <button
            type="button"
            data-testid="banner-steam-fix-btn"
            onClick={handleFixInSettings}
            className="cursor-pointer rounded-xs border border-warning bg-transparent px-2 py-0.5 text-xs font-medium text-warning transition-colors hover:bg-surface"
          >
            {t('Library::Fix in Settings')}
          </button>
        </div>
      )}

      {/* Main Toolbar */}
      <div className="flex flex-wrap items-center gap-compact border-b border-border-subtle bg-chrome px-md py-xs">
        {/* Search */}
        <div className="relative min-w-[160px] flex-[1_1_200px]">
          <input
            type="text"
            data-testid="library-search-input"
            value={search}
            onChange={handleSearchChange}
            placeholder={t('Library::Search your games (Ctrl+F)')}
            className="h-8 w-full rounded-sm border border-border-subtle bg-surface pl-[10px] pr-7 text-sm text-text-default outline-none transition-colors placeholder:text-text-dimmed focus:border-brand"
          />
          {search && (
            <button
              type="button"
              data-testid="library-clear-search-btn"
              onClick={handleClearSearch}
              title={t('Library::Clear search')}
              className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded-xs p-0.5 text-text-dimmed transition-colors hover:text-text-default"
            >
              <IconX size={14} stroke={2} aria-hidden />
            </button>
          )}
        </div>

        {/* Filters */}
        <Popover
          opened={filtersOpen}
          onChange={setFiltersOpen}
          position="bottom-start"
          shadow="md"
          width={300}
          withinPortal
          trapFocus
          returnFocus
        >
          <Popover.Target>
            <button
              type="button"
              data-testid="library-filters-btn"
              aria-expanded={filtersOpen}
              aria-haspopup="dialog"
              onClick={() => setFiltersOpen((open) => !open)}
              onKeyDown={(event) => {
                if (event.key === 'Escape' && filtersOpen) {
                  event.preventDefault();
                  setFiltersOpen(false);
                }
              }}
              className={`${toggleBase} shrink-0 ${toggleClass(!filtersAreDefault(filters))}`}
            >
              <IconFilter size={14} stroke={1.75} aria-hidden />
              <span>{t('Library::Filters')}</span>
              {chips.length > 0 && (
                <span
                  data-testid="library-filters-count"
                  className="rounded-full bg-on-primary/20 px-1.5 py-px text-[11px] font-semibold"
                >
                  {chips.length}
                </span>
              )}
            </button>
          </Popover.Target>

          <Popover.Dropdown
            data-testid="library-filters-popover"
            aria-label={t('Library::Filters')}
            className="rounded-md border border-border-subtle bg-secondary p-sm shadow-alert"
          >
            <div className="flex flex-col gap-sm">
              <div className="flex flex-col gap-1">
                <span className="text-[11px] font-semibold uppercase tracking-[0.4px] text-text-dimmed">
                  {t('Library::Availability')}
                </span>
                <Checkbox
                  data-testid="filter-installed"
                  label={t('Library::Installed')}
                  checked={filters.availability.installed}
                  onChange={(event) =>
                    setFilters({
                      ...filters,
                      availability: {
                        ...filters.availability,
                        installed: event.currentTarget.checked,
                      },
                    })
                  }
                  color="brand"
                  size="xs"
                />
                <Checkbox
                  data-testid="filter-owned"
                  label={t('Library::Owned')}
                  checked={filters.availability.owned}
                  onChange={(event) =>
                    setFilters({
                      ...filters,
                      availability: {
                        ...filters.availability,
                        owned: event.currentTarget.checked,
                      },
                    })
                  }
                  color="brand"
                  size="xs"
                />
                <Checkbox
                  data-testid="filter-family"
                  label={t('Library::Family shared')}
                  checked={filters.availability.family}
                  onChange={(event) =>
                    setFilters({
                      ...filters,
                      availability: {
                        ...filters.availability,
                        family: event.currentTarget.checked,
                      },
                    })
                  }
                  color="brand"
                  size="xs"
                />
              </div>

              <div className="flex items-center justify-between gap-sm">
                <span className="text-sm text-text-default">
                  {t('Library::Running only')}
                </span>
                <Switch
                  data-testid="filter-running"
                  aria-label={t('Library::Running only')}
                  checked={filters.runningOnly}
                  onChange={(event) =>
                    setFilters({
                      ...filters,
                      runningOnly: event.currentTarget.checked,
                    })
                  }
                  color="brand"
                  size="xs"
                />
              </div>

              <FilterSelect
                label={t('Library::Match state')}
                testId="filter-match"
                value={filters.matchState}
                onChange={(value) =>
                  setFilters({ ...filters, matchState: value as MatchFilter })
                }
                options={[
                  any(),
                  { value: 'matched', label: t('Library::Matched') },
                  { value: 'ambiguous', label: t('Library::Needs review') },
                  { value: 'unmatched', label: t('Library::Not matched') },
                  { value: 'noGame', label: t('Library::Marked as no game') },
                ]}
              />

              <FilterSelect
                label={t('Library::Play history')}
                testId="filter-playtime"
                value={filters.playtime}
                onChange={(value) =>
                  setFilters({ ...filters, playtime: value as PlaytimeFilter })
                }
                options={[
                  any(),
                  { value: 'unplayed', label: t('Library::Unplayed') },
                  { value: 'played', label: t('Library::Played') },
                  { value: 'atLeast10h', label: t('Library::10 h+') },
                  { value: 'atLeast100h', label: t('Library::100 h+') },
                ]}
              />

              <FilterSelect
                label={t('Library::Last played')}
                testId="filter-last-played"
                value={filters.lastPlayed}
                onChange={(value) =>
                  setFilters({ ...filters, lastPlayed: value as LastPlayedFilter })
                }
                options={[
                  any(),
                  { value: 'last7Days', label: t('Library::Last 7 days') },
                  { value: 'last30Days', label: t('Library::Last 30 days') },
                  { value: 'lastYear', label: t('Library::Last year') },
                ]}
              />

              <FilterSelect
                label={t('Library::Release decade')}
                testId="filter-release"
                value={releaseValue}
                onChange={(value) =>
                  setFilters({
                    ...filters,
                    release:
                      value === 'any' || value === 'earlier' || value === 'unknown'
                        ? (value as ReleaseFilter)
                        : { decade: Number(value) },
                  })
                }
                options={[
                  any(),
                  ...RELEASE_OPTIONS.map((decade) => ({
                    value: String(decade),
                    label: `${decade}s`,
                  })),
                  { value: 'earlier', label: t('Library::Earlier') },
                  { value: 'unknown', label: t('Library::Unknown') },
                ]}
              />

              <FilterSelect
                label={t('Library::Date added')}
                testId="filter-added"
                value={filters.added}
                onChange={(value) =>
                  setFilters({ ...filters, added: value as AddedFilter })
                }
                options={[
                  any(),
                  { value: 'last7Days', label: t('Library::Last 7 days') },
                  { value: 'last30Days', label: t('Library::Last 30 days') },
                  { value: 'last90Days', label: t('Library::Last 90 days') },
                  { value: 'lastYear', label: t('Library::Last year') },
                ]}
              />
            </div>
          </Popover.Dropdown>
        </Popover>

        {/* Favorites filter */}
        <button
          type="button"
          data-testid="library-favorites-filter"
          aria-pressed={filters.favoritesOnly}
          aria-label={t('Library::Favorites')}
          onClick={handleToggleFavorites}
          className={`${toggleBase} shrink-0 ${toggleClass(filters.favoritesOnly)}`}
        >
          {filters.favoritesOnly ? (
            <IconStarFilled size={15} stroke={1.75} aria-hidden className="text-warning" />
          ) : (
            <IconStar size={15} stroke={1.75} aria-hidden />
          )}
          <span>{t('Library::Favorites')}</span>
        </button>

        {/* Sort Controls */}
        <div className="flex items-center gap-1.5">
          <select
            data-testid="library-sort-select"
            value={sort.key}
            onChange={handleSortKeyChange}
            aria-label={t('Library::Sort by')}
            className={`${controlBase} cursor-pointer px-2 text-xs outline-none`}
          >
            <option value="title">{t('Library::Title')}</option>
            <option value="provider">{t('Library::Store')}</option>
            <option value="matchStatus">{t('Library::Match status')}</option>
            <option value="releaseYear">{t('Library::Release year')}</option>
            <option value="playtime">{t('Library::Playtime')}</option>
            <option value="lastPlayed">{t('Library::Last played')}</option>
            <option value="dateAdded">{t('Library::Date added')}</option>
          </select>

          <button
            type="button"
            data-testid="library-sort-direction-btn"
            aria-label={directionLabel(sort.key, sort.direction, t)}
            title={directionLabel(sort.key, sort.direction, t)}
            onClick={handleToggleSortDirection}
            className={`${controlBase} cursor-pointer px-2`}
          >
            {sort.direction === 'ascending' ? (
              <IconSortAscending size={16} stroke={1.75} aria-hidden />
            ) : (
              <IconSortDescending size={16} stroke={1.75} aria-hidden />
            )}
          </button>
        </div>

        {/* View Mode Switcher */}
        <div className="flex items-center gap-1">
          <button
            type="button"
            data-testid="library-view-grid-btn"
            onClick={handleViewGrid}
            className={`${toggleBase} ${toggleClass(view === 'grid')}`}
          >
            <IconLayoutGrid size={14} stroke={1.75} aria-hidden />
            {t('Library::Grid')}
          </button>

          <button
            type="button"
            data-testid="library-view-list-btn"
            onClick={handleViewList}
            className={`${toggleBase} ${toggleClass(view === 'list')}`}
          >
            <IconLayoutList size={14} stroke={1.75} aria-hidden />
            {t('Library::List')}
          </button>

          <button
            type="button"
            data-testid="library-toggle-details-btn"
            onClick={handleToggleDetails}
            className={`${toggleBase} ${toggleClass(detailOpen)}`}
          >
            <IconLayoutSidebarRight size={14} stroke={1.75} aria-hidden />
            {t('Library::Details')}
          </button>
        </div>

        {/* Rescan Button */}
        <button
          type="button"
          data-testid="library-rescan-btn"
          onClick={handleRescan}
          className="ml-auto flex h-8 items-center gap-1.5 rounded-sm border border-brand px-3 text-xs font-medium text-brand transition-colors hover:bg-accent-subtle"
        >
          <IconRefresh size={14} stroke={1.75} aria-hidden />
          {t('Library::Rescan')}
        </button>
      </div>

      {/* Active filters */}
      {(chips.length > 0 || search.length > 0) && (
        <div
          data-testid="library-active-filters"
          className="flex flex-wrap items-center gap-1.5 border-b border-border-subtle bg-chrome px-md py-xs"
        >
          <span data-testid="library-filtered-count" className="text-xs text-text-dimmed">
            {formatI18n(t('Library::Showing {} of {}'), filtered, total)}
          </span>
          {chips.map((chip) => (
            <span
              key={chip.id}
              data-testid={`library-filter-chip-${chip.id}`}
              className="flex items-center gap-1 rounded-full bg-accent-subtle px-2 py-0.5 text-xs text-text-default"
            >
              <span>{chip.label}</span>
              <button
                type="button"
                data-testid={`library-filter-chip-remove-${chip.id}`}
                aria-label={formatI18n(t('Library::Remove filter: {}'), chip.label)}
                onClick={() => setFilters(chip.clear())}
                className="cursor-pointer rounded-full p-0.5 text-text-dimmed transition-colors hover:text-text-default"
              >
                <IconX size={12} stroke={2} aria-hidden />
              </button>
            </span>
          ))}
          {chips.length > 0 && (
            <button
              type="button"
              data-testid="library-clear-filters-btn"
              onClick={handleClearFilters}
              className="ml-1 cursor-pointer text-xs font-medium text-brand transition-colors hover:text-brand-4"
            >
              {t('Library::Clear all filters')}
            </button>
          )}
        </div>
      )}
    </div>
  );

  // Mantine v9 primitives require a provider context. The app mounts one in AppProviders;
  // keep standalone renders (component tests) working as well.
  if (mantineContext) {
    return content;
  }

  return (
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      {content}
    </MantineProvider>
  );
};
