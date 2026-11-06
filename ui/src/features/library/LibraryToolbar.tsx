import React from 'react';
import {
  IconLayoutGrid,
  IconLayoutList,
  IconLayoutSidebarRight,
  IconRefresh,
  IconX,
} from '@tabler/icons-react';
import { commands, useLibraryView, type SortKey, type SortDirection } from '../../api';
import { useI18n } from '../../i18n';
import { formatI18n } from './formatters';

const bannerBase =
  'border-b border-border-subtle px-md py-compact text-xs';
const controlBase =
  'h-8 rounded-sm border border-border-subtle bg-surface text-text-default transition-colors hover:border-brand/60 hover:text-on-primary';
const toggleBase =
  'flex h-8 cursor-pointer items-center gap-1.5 rounded-sm border px-2.5 text-xs transition-colors';

export const LibraryToolbar: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();

  const search = library?.search ?? '';
  const sort = library?.layout?.sort ?? { key: 'title' as SortKey, direction: 'ascending' as SortDirection };
  const view = library?.layout?.view ?? 'grid';
  const detailOpen = library?.layout?.detailOpen ?? false;
  const scanning = library?.scanning ?? false;
  const failedProviders = library?.failedProviders ?? [];
  const access = library?.access ?? 'verified';

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    commands.libraryInput({ setSearch: e.target.value });
  };

  const handleClearSearch = () => {
    commands.libraryInput({ setSearch: '' });
  };

  const handleSortKeyChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    commands.libraryInput({
      setSort: {
        key: e.target.value as SortKey,
        direction: sort.direction,
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

  return (
    <div className="flex w-full flex-col">
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

        {/* Sort Controls */}
        <div className="flex items-center gap-1.5">
          <select
            data-testid="library-sort-select"
            value={sort.key}
            onChange={handleSortKeyChange}
            aria-label="Sort by"
            className={`${controlBase} cursor-pointer px-2 text-xs outline-none`}
          >
            <option value="title">{t('Library::Title')}</option>
            <option value="provider">{t('Library::Store')}</option>
            <option value="matchStatus">{t('Library::Match status')}</option>
          </select>

          <button
            type="button"
            data-testid="library-sort-direction-btn"
            onClick={handleToggleSortDirection}
            className={`${controlBase} cursor-pointer px-2.5 text-xs`}
          >
            {sort.direction === 'ascending' ? t('Library::A → Z') : t('Library::Z → A')}
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
    </div>
  );
};
