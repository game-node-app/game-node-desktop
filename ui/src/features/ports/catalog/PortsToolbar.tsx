import React from 'react';
import {
  IconLayoutGrid,
  IconLayoutList,
  IconRefresh,
  IconX,
} from '@tabler/icons-react';
import type { PortFilters, PortViewMode } from '../../../api/ports';
import { useI18n } from '../../../i18n';
import { formatI18n } from '../../library/formatters';
import { portFiltersAreDefault } from './format';

const controlBase =
  'h-8 rounded-sm border border-border-subtle bg-surface text-text-default transition-colors hover:border-brand/60 hover:text-on-primary';
const toggleBase =
  'flex h-8 cursor-pointer items-center gap-1.5 rounded-sm border px-2.5 text-xs transition-colors';

export interface PortsToolbarProps {
  search: string;
  onSearchChange: (value: string) => void;
  filters: PortFilters;
  onFiltersChange: (next: PortFilters) => void;
  view: PortViewMode;
  onViewChange: (view: PortViewMode) => void;
  consoles: string[];
  visibleCount: number;
  totalCount: number;
  refreshing: boolean;
  onRefresh: () => void;
}

interface Chip {
  id: string;
  label: string;
  remove: () => void;
}

/**
 * Catalog toolbar: search, console filter, the three persisted toggles, the view switch
 * and Refresh. The layout follows the library toolbar so both destinations feel like one
 * application and both wrap cleanly with longer pt-BR labels.
 */
export const PortsToolbar: React.FC<PortsToolbarProps> = ({
  search,
  onSearchChange,
  filters,
  onFiltersChange,
  view,
  onViewChange,
  consoles,
  visibleCount,
  totalCount,
  refreshing,
  onRefresh,
}) => {
  const { t } = useI18n();

  const chips: Chip[] = [];
  for (const consoleName of filters.consoles) {
    chips.push({
      id: `console-${consoleName}`,
      label: consoleName,
      remove: () =>
        onFiltersChange({
          ...filters,
          consoles: filters.consoles.filter((value) => value !== consoleName),
        }),
    });
  }
  if (filters.guided) {
    chips.push({
      id: 'guided',
      label: t('Ports::Guided setup'),
      remove: () => onFiltersChange({ ...filters, guided: false }),
    });
  }
  if (filters.verified) {
    chips.push({
      id: 'verified',
      label: t('Ports::Verified'),
      remove: () => onFiltersChange({ ...filters, verified: false }),
    });
  }
  if (filters.installed) {
    chips.push({
      id: 'installed',
      label: t('Ports::Installed'),
      remove: () => onFiltersChange({ ...filters, installed: false }),
    });
  }

  const toggleClass = (active: boolean) =>
    active
      ? 'border-brand bg-tertiary text-on-primary'
      : 'border-border-subtle bg-surface text-text-default hover:border-brand/60';

  const toggle = (
    id: string,
    label: string,
    active: boolean,
    next: (value: boolean) => void,
  ) => (
    <button
      type="button"
      data-testid={`ports-${id}-filter`}
      aria-pressed={active}
      onClick={() => next(!active)}
      className={`${toggleBase} shrink-0 ${toggleClass(active)}`}
    >
      {label}
    </button>
  );

  const consoleValue = filters.consoles[0] ?? '';

  return (
    <div className="flex w-full shrink-0 flex-col">
      <div className="flex flex-wrap items-center gap-compact border-b border-border-subtle bg-chrome px-md py-xs">
        <div className="relative min-w-[160px] flex-[1_1_200px]">
          <input
            type="text"
            data-testid="ports-search-input"
            value={search}
            onChange={(event) => onSearchChange(event.target.value)}
            placeholder={t('Ports::Search the port catalog')}
            aria-label={t('Ports::Search the port catalog')}
            className="h-8 w-full rounded-sm border border-border-subtle bg-surface pl-[10px] pr-7 text-sm text-text-default outline-none transition-colors placeholder:text-text-dimmed focus:border-brand"
          />
          {search && (
            <button
              type="button"
              data-testid="ports-clear-search-btn"
              onClick={() => onSearchChange('')}
              title={t('Library::Clear search')}
              aria-label={t('Library::Clear search')}
              className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded-xs p-0.5 text-text-dimmed transition-colors hover:text-text-default"
            >
              <IconX size={14} stroke={2} aria-hidden />
            </button>
          )}
        </div>

        <label className="flex shrink-0 items-center gap-1.5">
          <span className="sr-only">{t('Ports::Console')}</span>
          <select
            data-testid="ports-console-filter"
            aria-label={t('Ports::Console')}
            value={consoleValue}
            onChange={(event) => {
              const value = event.target.value;
              onFiltersChange({
                ...filters,
                consoles: value ? [value] : [],
              });
            }}
            className={`${controlBase} max-w-[180px] cursor-pointer px-2 text-xs outline-none focus:border-brand`}
          >
            <option value="">{t('Ports::All consoles')}</option>
            {consoles.map((consoleName) => (
              <option key={consoleName} value={consoleName}>
                {consoleName}
              </option>
            ))}
          </select>
        </label>

        {toggle('guided', t('Ports::Guided setup'), filters.guided, (value) =>
          onFiltersChange({ ...filters, guided: value }),
        )}
        {toggle('verified', t('Ports::Verified'), filters.verified, (value) =>
          onFiltersChange({ ...filters, verified: value }),
        )}
        {toggle('installed', t('Ports::Installed'), filters.installed, (value) =>
          onFiltersChange({ ...filters, installed: value }),
        )}

        <div className="flex items-center gap-1">
          <button
            type="button"
            data-testid="ports-view-grid-btn"
            aria-pressed={view === 'grid'}
            onClick={() => onViewChange('grid')}
            className={`${toggleBase} ${toggleClass(view === 'grid')}`}
          >
            <IconLayoutGrid size={14} stroke={1.75} aria-hidden />
            {t('Library::Grid')}
          </button>
          <button
            type="button"
            data-testid="ports-view-list-btn"
            aria-pressed={view === 'list'}
            onClick={() => onViewChange('list')}
            className={`${toggleBase} ${toggleClass(view === 'list')}`}
          >
            <IconLayoutList size={14} stroke={1.75} aria-hidden />
            {t('Library::List')}
          </button>
        </div>

        <button
          type="button"
          data-testid="ports-refresh-btn"
          disabled={refreshing}
          onClick={onRefresh}
          aria-busy={refreshing}
          className="ml-auto flex h-8 shrink-0 items-center gap-1.5 rounded-sm border border-brand px-3 text-xs font-medium text-brand transition-colors hover:bg-accent-subtle disabled:cursor-default disabled:opacity-45"
        >
          <IconRefresh size={14} stroke={1.75} aria-hidden />
          {t('Ports::Refresh')}
        </button>
      </div>

      {(chips.length > 0 || search.length > 0) && (
        <div
          data-testid="ports-active-filters"
          className="flex flex-wrap items-center gap-1.5 border-b border-border-subtle bg-chrome px-md py-xs"
        >
          <span data-testid="ports-filtered-count" className="text-xs text-text-dimmed">
            {formatI18n(
              t('Ports::Showing {} of {} ports'),
              visibleCount,
              totalCount,
            )}
          </span>
          {search.length > 0 && (
            <span
              data-testid="ports-filter-chip-search"
              className="flex items-center gap-1 rounded-full bg-accent-subtle px-2 py-0.5 text-xs text-text-default"
            >
              <span className="max-w-[220px] truncate" title={search}>
                {search}
              </span>
              <button
                type="button"
                data-testid="ports-filter-chip-remove-search"
                aria-label={t('Library::Clear search')}
                onClick={() => onSearchChange('')}
                className="cursor-pointer rounded-full p-0.5 text-text-dimmed transition-colors hover:text-text-default"
              >
                <IconX size={12} stroke={2} aria-hidden />
              </button>
            </span>
          )}
          {chips.map((chip) => (
            <span
              key={chip.id}
              data-testid={`ports-filter-chip-${chip.id}`}
              className="flex items-center gap-1 rounded-full bg-accent-subtle px-2 py-0.5 text-xs text-text-default"
            >
              <span className="max-w-[220px] truncate" title={chip.label}>
                {chip.label}
              </span>
              <button
                type="button"
                data-testid={`ports-filter-chip-remove-${chip.id}`}
                aria-label={formatI18n(t('Library::Remove filter: {}'), chip.label)}
                onClick={chip.remove}
                className="cursor-pointer rounded-full p-0.5 text-text-dimmed transition-colors hover:text-text-default"
              >
                <IconX size={12} stroke={2} aria-hidden />
              </button>
            </span>
          ))}
          {!portFiltersAreDefault(filters) && (
            <button
              type="button"
              data-testid="ports-clear-filters-btn"
              onClick={() =>
                onFiltersChange({
                  consoles: [],
                  guided: false,
                  verified: false,
                  installed: false,
                })
              }
              className="ml-1 cursor-pointer text-xs font-medium text-brand transition-colors hover:text-brand-4"
            >
              {t('Library::Clear all filters')}
            </button>
          )}
        </div>
      )}
    </div>
  );
};
