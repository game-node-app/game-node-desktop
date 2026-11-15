import React, { useMemo, useState } from 'react';
import { Popover } from '@mantine/core';
import {
  IconAlertTriangle,
  IconArrowBackUp,
  IconBooks,
  IconDeviceDesktop,
  IconEye,
  IconEyeOff,
  IconPlayerPlay,
  IconSearch,
  IconStar,
  IconUsersGroup,
  type TablerIcon,
} from '@tabler/icons-react';
import {
  commands,
  createDefaultFilterCounts,
  createDefaultFilters,
  useLibraryView,
  type FilterCounts,
  type GameNodeStatusFilter,
  type LibraryFilters,
  type MatchFilterValue,
  type ReleaseFilter,
} from '../../api';
import {
  Button,
  Checkbox,
  ChoiceChip,
  SectionLabel,
  SegmentedControl,
  Switch,
  popoverClassNames,
} from '../../components/ui';
import { setFiltersOpen, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { formatI18n } from './formatters';
import {
  addedLabel,
  gameNodeStatusLabel,
  lastPlayedLabel,
  matchStateLabel,
  playedLabel,
  playtimeLabel,
  releaseLabel,
} from './libraryControlLabels';
import { providerIcon } from './libraryControlIcons';

type FilterControl = 'checkbox' | 'switch' | 'segmented' | 'chip';
type Column = 1 | 2 | 3;

interface FilterOption {
  id: string;
  label: string;
  count: number;
  selected: boolean;
  disabled?: boolean;
  icon?: TablerIcon;
  dotClassName?: string;
  onSelect: () => void;
}

interface FilterSection {
  id: string;
  label: string;
  column: Column;
  control: FilterControl;
  options: FilterOption[];
  disabledReason?: string;
}

function releaseKey(value: ReleaseFilter): string {
  if (typeof value === 'object' && 'decade' in value) return `decade:${value.decade}`;
  return value;
}

function isReleaseSelected(active: ReleaseFilter, option: ReleaseFilter): boolean {
  if (typeof active === 'object' && 'decade' in active) {
    return typeof option === 'object' && 'decade' in option && active.decade === option.decade;
  }
  return active === option;
}

function countLabel(count: number): React.ReactNode {
  return (
    <span className="shrink-0 text-xs font-normal tabular-nums text-text-dimmed">{count}</span>
  );
}

function matchDotClass(value: MatchFilterValue): string {
  switch (value) {
    case 'matched':
      return 'bg-success';
    case 'ambiguous':
      return 'bg-warning';
    case 'unmatched':
      return 'bg-text-dimmed';
    case 'noGame':
      return 'bg-text-muted';
    default:
      return 'bg-text-dimmed';
  }
}

interface FilterPopoverProps {
  /** The toolbar Filters button. The popover owns the open state in `uiStore`. */
  children: React.ReactElement;
}

/**
 * Floating UI's `size` middleware measures the space actually available on the
 * chosen side of the Filters button and Mantine applies it as the dropdown's
 * max-height. The 8 px padding keeps the surface clear of the window edge.
 * With enough room the fixed 565 px height still wins, so the 760×565
 * reference is unchanged; at the 960×600 minimum the body scrolls and the
 * header and footer stay reachable. `flip` and `shift` keep the whole surface
 * on screen when the toolbar button sits low in the window.
 */
export const FILTER_POPOVER_MIDDLEWARES = {
  flip: true,
  shift: true,
  // Vertical only: the 760 px width already fits above the 960 px minimum,
  // while the height must follow the space the trigger leaves.
  size: { padding: { top: 8, bottom: 8 } },
} as const;

/**
 * 760×565 three-column filter popover. Every option reads its count from the
 * engine's faceted `filterCounts`; nothing is inferred from the visible rows.
 * Changes apply immediately through `libraryInput`, and the shared
 * `uiStore.filtersOpen` keeps Ctrl+Shift+F and the toolbar button in sync.
 */
export const FilterPopover: React.FC<FilterPopoverProps> = ({ children }) => {
  const library = useLibraryView();
  const { t } = useI18n();
  const opened = useUiStore((state) => state.filtersOpen);
  const [search, setSearch] = useState('');

  const filters = library?.layout.filters ?? createDefaultFilters();
  const counts: FilterCounts = library?.filterCounts ?? createDefaultFilterCounts();

  const apply = (next: LibraryFilters) => {
    commands.libraryInput({ setFilters: next });
  };

  const patchAvailability = (patch: Partial<LibraryFilters['availability']>) => {
    apply({ ...filters, availability: { ...filters.availability, ...patch } });
  };

  const sections = useMemo<FilterSection[]>(() => {
    const statusAvailable = counts.gamenodeStatus.available;
    const statusReason = statusAvailable
      ? undefined
      : t('Library::Could not check your GameNode library. Try again.');

    const availabilitySection: FilterSection = {
      id: 'availability',
      label: t('Library::Availability'),
      column: 1,
      control: 'checkbox',
      options: [
        {
          id: 'installed',
          label: t('Library::Installed on this PC'),
          count: counts.availability.installed,
          selected: filters.availability.installed,
          icon: IconDeviceDesktop,
          onSelect: () => patchAvailability({ installed: !filters.availability.installed }),
        },
        {
          id: 'owned',
          label: t('Library::In library'),
          count: counts.availability.owned,
          selected: filters.availability.owned,
          icon: IconBooks,
          onSelect: () => patchAvailability({ owned: !filters.availability.owned }),
        },
        {
          id: 'shared',
          label: t('Library::Family shared'),
          count: counts.availability.shared,
          selected: filters.availability.shared,
          icon: IconUsersGroup,
          onSelect: () => patchAvailability({ shared: !filters.availability.shared }),
        },
      ],
    };

    const storeSection: FilterSection = {
      id: 'stores',
      label: t('Library::Store'),
      column: 1,
      control: 'checkbox',
      options: counts.stores.map((store) => ({
        id: store.id,
        label: store.name,
        count: store.count,
        selected: filters.stores.includes(store.id),
        icon: providerIcon(store.id),
        onSelect: () =>
          apply({
            ...filters,
            stores: filters.stores.includes(store.id)
              ? filters.stores.filter((id) => id !== store.id)
              : [...filters.stores, store.id],
          }),
      })),
    };

    const shortcutsSection: FilterSection = {
      id: 'shortcuts',
      label: t('Library::Shortcuts'),
      column: 1,
      control: 'switch',
      options: [
        {
          id: 'favorites',
          label: t('Library::Favorites only'),
          count: counts.favoritesOnly,
          selected: filters.favoritesOnly,
          icon: IconStar,
          onSelect: () => apply({ ...filters, favoritesOnly: !filters.favoritesOnly }),
        },
        {
          id: 'running',
          label: t('Library::Running only'),
          count: counts.runningOnly,
          selected: filters.runningOnly,
          icon: IconPlayerPlay,
          onSelect: () => apply({ ...filters, runningOnly: !filters.runningOnly }),
        },
        {
          id: 'showHidden',
          label: t('Library::Show hidden'),
          count: counts.showHidden,
          selected: filters.showHidden,
          icon: filters.showHidden ? IconEye : IconEyeOff,
          onSelect: () => apply({ ...filters, showHidden: !filters.showHidden }),
        },
      ],
    };

    const matchSection: FilterSection = {
      id: 'match',
      label: t('Library::Match'),
      column: 2,
      control: 'checkbox',
      options: (['matched', 'ambiguous', 'unmatched', 'noGame'] as MatchFilterValue[]).map(
        (value) => ({
          id: value,
          label: matchStateLabel(value, t),
          count: counts.matchStates[value],
          selected: filters.matchStates.includes(value),
          dotClassName: matchDotClass(value),
          disabled: false,
          onSelect: () =>
            apply({
              ...filters,
              matchStates: filters.matchStates.includes(value)
                ? filters.matchStates.filter((entry) => entry !== value)
                : [...filters.matchStates, value],
            }),
        }),
      ),
    };

    const statusValues: GameNodeStatusFilter[] = ['playing', 'finished', 'planned'];
    if (counts.gamenodeStatus.ongoing > 0 || filters.gamenodeStatus.includes('ongoing')) {
      statusValues.push('ongoing');
    }
    statusValues.push('dropped', 'inLibrary', 'notInLibrary');

    const statusSection: FilterSection = {
      id: 'gamenode-status',
      label: t('Library::GameNode status'),
      column: 2,
      control: 'checkbox',
      disabledReason: statusReason,
      options: statusValues.map((value) => ({
        id: value,
        label: gameNodeStatusLabel(value, t),
        count: counts.gamenodeStatus[value],
        selected: filters.gamenodeStatus.includes(value),
        disabled: !statusAvailable,
        onSelect: () =>
          apply({
            ...filters,
            gamenodeStatus: filters.gamenodeStatus.includes(value)
              ? filters.gamenodeStatus.filter((entry) => entry !== value)
              : [...filters.gamenodeStatus, value],
          }),
      })),
    };

    const playtimeSection: FilterSection = {
      id: 'playtime',
      label: t('Library::Playtime'),
      column: 3,
      control: 'segmented',
      options: (['any', 'atLeast10h', 'atLeast100h'] as const).map((value) => ({
        id: value,
        label: playtimeLabel(value, t),
        count: counts.playtime[value],
        selected: filters.playtime === value,
        onSelect: () => apply({ ...filters, playtime: value }),
      })),
    };

    const playedSection: FilterSection = {
      id: 'played',
      label: t('Library::History'),
      column: 3,
      control: 'segmented',
      options: (['any', 'played', 'unplayed'] as const).map((value) => ({
        id: value,
        label: playedLabel(value, t),
        count: counts.played[value],
        selected: filters.played === value,
        onSelect: () => apply({ ...filters, played: value }),
      })),
    };

    const lastPlayedSection: FilterSection = {
      id: 'last-played',
      label: t('Library::Last session'),
      column: 3,
      control: 'chip',
      options: (['last7Days', 'last30Days', 'last90Days', 'lastYear', 'olderThanYear'] as const).map(
        (value) => ({
          id: value,
          label: lastPlayedLabel(value, t),
          count: counts.lastPlayed[value],
          selected: filters.lastPlayed === value,
          onSelect: () =>
            apply({
              ...filters,
              lastPlayed: filters.lastPlayed === value ? 'any' : value,
            }),
        }),
      ),
    };

    const releaseSection: FilterSection = {
      id: 'release',
      label: t('Library::Release'),
      column: 3,
      control: 'chip',
      options: counts.release.map((entry) => ({
        id: releaseKey(entry.value),
        label: releaseLabel(entry.value, t),
        count: entry.count,
        selected: isReleaseSelected(filters.release, entry.value),
        onSelect: () =>
          apply({
            ...filters,
            release: isReleaseSelected(filters.release, entry.value) ? 'any' : entry.value,
          }),
      })),
    };

    const addedSection: FilterSection = {
      id: 'added',
      label: t('Library::Added'),
      column: 3,
      control: 'chip',
      options: (['last7Days', 'last30Days', 'lastYear', 'olderThanYear'] as const).map((value) => ({
        id: value,
        label: addedLabel(value, t),
        count: counts.added[value],
        selected: filters.added === value,
        onSelect: () =>
          apply({ ...filters, added: filters.added === value ? 'any' : value }),
      })),
    };

    return [
      availabilitySection,
      storeSection,
      shortcutsSection,
      matchSection,
      statusSection,
      playtimeSection,
      playedSection,
      lastPlayedSection,
      releaseSection,
      addedSection,
    ];
  }, [counts, filters, t]);

  const query = search.trim().toLocaleLowerCase();
  const matches = (label: string) => label.toLocaleLowerCase().includes(query);

  // Mantine does not attach a click handler to a controlled Popover's target,
  // so the shared `filtersOpen` state needs the toggle here.
  const trigger = React.cloneElement(
    children as React.ReactElement<{
      onClick?: React.MouseEventHandler<HTMLElement>;
    }>,
    {
      onClick: (event: React.MouseEvent<HTMLElement>) => {
        (children.props as { onClick?: React.MouseEventHandler<HTMLElement> }).onClick?.(event);
        setFiltersOpen(!opened);
      },
    },
  );

  const visibleSections = sections
    .map((section) => {
      if (section.options.length === 0) return null;
      const labelMatches = query.length === 0 || matches(section.label);
      const options = labelMatches
        ? section.options
        : section.options.filter((option) => matches(option.label));
      if (options.length === 0) return null;
      return { ...section, options };
    })
    .filter((section): section is FilterSection => section !== null);

  const renderOption = (section: FilterSection, option: FilterOption) => {
    const testId = `filter-option-${section.id}-${option.id}`;
    if (section.control === 'switch') {
      return (
        <div
          key={option.id}
          data-testid={testId}
          className="flex min-h-[32px] items-center gap-[8px] px-[6px] py-[2px]"
        >
          {option.icon ? (
            <option.icon size={14} stroke={2} aria-hidden="true" className="shrink-0 text-text-secondary" />
          ) : null}
          <span className="min-w-0 flex-1 truncate text-body font-normal text-text-default">
            {option.label}
          </span>
          {countLabel(option.count)}
          <Switch
            data-testid={`${testId}-control`}
            aria-label={option.label}
            checked={option.selected}
            disabled={option.disabled}
            onChange={option.onSelect}
          />
        </div>
      );
    }

    if (section.control === 'chip') {
      return (
        <ChoiceChip
          key={option.id}
          data-testid={testId}
          selected={option.selected}
          disabled={option.disabled}
          onClick={option.onSelect}
        >
          <span>{option.label}</span>
          {countLabel(option.count)}
        </ChoiceChip>
      );
    }

    return (
      <div
        key={option.id}
        data-testid={testId}
        title={option.disabled ? section.disabledReason : undefined}
        className={`flex min-h-[30px] items-center gap-[8px] rounded-control px-[6px] text-body ${
          option.disabled
            ? 'cursor-not-allowed text-text-dimmed'
            : 'cursor-pointer text-text-default hover:bg-raised'
        }`}
        onClick={(event) => {
          if (option.disabled) return;
          if ((event.target as HTMLElement).closest('input')) return;
          option.onSelect();
        }}
      >
        <Checkbox
          aria-label={option.label}
          checked={option.selected}
          disabled={option.disabled}
          onChange={option.onSelect}
        />
        {option.dotClassName ? (
          <span aria-hidden="true" className={`h-[8px] w-[8px] shrink-0 rounded-full ${option.dotClassName}`} />
        ) : null}
        {option.icon ? (
          <option.icon size={14} stroke={2} aria-hidden="true" className="shrink-0 text-text-secondary" />
        ) : null}
        <span className="min-w-0 flex-1 truncate">{option.label}</span>
        {countLabel(option.count)}
      </div>
    );
  };

  const renderSection = (section: FilterSection): React.ReactNode => (
    <section
      key={section.id}
      data-testid={`filter-section-${section.id}`}
      className="flex flex-col gap-[4px]"
    >
      <SectionLabel>{section.label}</SectionLabel>
      {section.disabledReason ? (
        <p
          data-testid={`filter-section-${section.id}-disabled`}
          className="flex items-start gap-[6px] px-[2px] pb-[4px] text-xs font-normal text-text-dimmed"
        >
          <IconAlertTriangle
            size={14}
            stroke={2}
            aria-hidden="true"
            className="mt-px shrink-0 text-warning"
          />
          <span>{section.disabledReason}</span>
        </p>
      ) : null}
      {section.control === 'segmented' ? (
        <SegmentedControl
          aria-label={section.label}
          fullWidth
          value={section.options.find((option) => option.selected)?.id ?? section.options[0]?.id}
          onChange={(id) => section.options.find((option) => option.id === id)?.onSelect()}
          data={section.options.map((option) => ({
            value: option.id,
            disabled: option.disabled,
            label: (
              <span className="inline-flex items-center gap-1">
                <span>{option.label}</span>
                <span className="text-caption font-normal text-text-dimmed">{option.count}</span>
              </span>
            ),
          }))}
        />
      ) : section.control === 'chip' ? (
        <div className="flex flex-wrap gap-[6px]">
          {section.options.map((option) => renderOption(section, option))}
        </div>
      ) : (
        <div className="flex flex-col gap-[2px]">
          {section.options.map((option) => renderOption(section, option))}
        </div>
      )}
    </section>
  );

  return (
    <Popover
      opened={opened}
      onChange={(next) => {
        setFiltersOpen(next);
        if (!next) setSearch('');
      }}
      position="bottom-start"
      width="calc(760px)"
      middlewares={FILTER_POPOVER_MIDDLEWARES}
      withinPortal
      hideDetached={false}
      trapFocus
      returnFocus
      closeOnEscape
      shadow="md"
      transitionProps={{ duration: 0 }}
      classNames={popoverClassNames}
    >
      <Popover.Target>{trigger}</Popover.Target>
      <Popover.Dropdown
        data-testid="library-filters-popover"
        aria-label={t('Library::Filters')}
        className="flex h-[565px] w-[760px] flex-col"
      >
        <div
          data-testid="filter-popover-header"
          className="flex shrink-0 items-center gap-[10px] border-b border-border-subtle px-[16px] py-[12px]"
        >
          <span className="shrink-0 text-title font-semibold text-text-primary">
            {t('Library::Filters')}
          </span>
          <div className="flex h-[30px] min-w-0 flex-1 items-center gap-[6px] rounded-control border border-border-subtle bg-surface px-[10px]">
            <IconSearch size={14} stroke={2} aria-hidden="true" className="shrink-0 text-text-dimmed" />
            <input
              type="text"
              data-testid="library-filter-search"
              aria-label={t('Library::Search a filter')}
              placeholder={t('Library::Search a filter')}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              className="min-w-0 flex-1 bg-transparent text-xs text-text-default outline-none placeholder:text-text-dimmed"
            />
          </div>
        </div>

        <div
          data-testid="filter-popover-body"
          className="grid min-h-0 flex-1 grid-cols-3 gap-[18px] overflow-y-auto p-[16px]"
        >
          {visibleSections.length === 0 ? (
            <p
              data-testid="filter-popover-empty"
              className="col-span-3 px-[4px] py-[24px] text-sm text-text-dimmed"
            >
              {formatI18n(t('Library::No filter matches “{}”'), search.trim())}
            </p>
          ) : (
            ([1, 2, 3] as Column[]).map((column) => (
              <div key={column} className="flex min-w-0 flex-col gap-[18px]">
                {visibleSections
                  .filter((section) => section.column === column)
                  .map((section) => renderSection(section))}
              </div>
            ))
          )}
        </div>

        <div
          data-testid="filter-popover-footer"
          className="flex shrink-0 items-center gap-[10px] border-t border-border-subtle bg-surface px-[16px] py-[12px]"
        >
          <span className="text-xs font-normal text-text-dimmed">
            {t('Library::Filters apply immediately')}
          </span>
          <div className="flex-1" />
          <Button
            variant="ghost"
            size="sm"
            leftIcon={IconArrowBackUp}
            data-testid="filter-clear-all-btn"
            onClick={() => apply(createDefaultFilters())}
          >
            {t('Library::Clear all')}
          </Button>
          <Button
            variant="primary"
            size="sm"
            data-testid="filter-done-btn"
            onClick={() => setFiltersOpen(false)}
          >
            {t('Library::Done')}
          </Button>
        </div>
      </Popover.Dropdown>
    </Popover>
  );
};
