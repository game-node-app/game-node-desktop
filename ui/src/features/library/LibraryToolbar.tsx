import React from 'react';
import { commands, useLibraryView, type SortKey, type SortDirection } from '../../api';
import { useI18n } from '../../i18n';
import { formatI18n } from './formatters';

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

  return (
    <div style={{ display: 'flex', flexDirection: 'column', width: '100%' }}>
      {/* Status Banners */}
      {scanning && (
        <div
          data-testid="banner-scanning"
          style={{
            backgroundColor: 'var(--color-accent-subtle, #F150251F)',
            color: 'var(--color-text-default, #D9D9D9)',
            padding: '8px 16px',
            fontSize: 'var(--font-size-xs, 12px)',
            borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
          }}
        >
          {t('Library::Scanning for installed games…')}
        </div>
      )}

      {failedProviders.length > 0 && (
        <div
          data-testid="banner-failed-providers"
          style={{
            backgroundColor: 'var(--color-warning-subtle, #F5A5241F)',
            color: 'var(--color-warning, #F5A524)',
            padding: '8px 16px',
            fontSize: 'var(--font-size-xs, 12px)',
            borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
          }}
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
          style={{
            backgroundColor: 'var(--color-accent-subtle, #F150251F)',
            color: 'var(--color-text-default, #D9D9D9)',
            padding: '8px 16px',
            fontSize: 'var(--font-size-xs, 12px)',
            borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
          }}
        >
          {t('Library::GameNode cannot be reached. Showing your saved library.')}
        </div>
      )}

      {access === 'unverified' && (
        <div
          data-testid="banner-unverified"
          style={{
            backgroundColor: 'var(--color-accent-subtle, #F150251F)',
            color: 'var(--color-text-default, #D9D9D9)',
            padding: '8px 16px',
            fontSize: 'var(--font-size-xs, 12px)',
            borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
          }}
        >
          {t(
            'Library::Showing your saved library. Matching and adding to GameNode wait for your session to be verified.',
          )}
        </div>
      )}

      {/* Main Toolbar */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: '8px',
          padding: '10px 16px',
          flexWrap: 'wrap',
          backgroundColor: 'var(--color-chrome, #111111)',
          borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
        }}
      >
        {/* Search */}
        <div style={{ position: 'relative', flex: '1 1 200px', minWidth: '160px' }}>
          <input
            type="text"
            data-testid="library-search-input"
            value={search}
            onChange={handleSearchChange}
            placeholder={t('Library::Search your games (Ctrl+F)')}
            style={{
              width: '100%',
              height: '32px',
              padding: '0 28px 0 10px',
              backgroundColor: 'var(--color-surface, #262525)',
              color: 'var(--color-text-default, #D9D9D9)',
              border: '1px solid var(--color-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              fontSize: 'var(--font-size-sm, 14px)',
              outline: 'none',
            }}
          />
          {search && (
            <button
              type="button"
              data-testid="library-clear-search-btn"
              onClick={handleClearSearch}
              title={t('Library::Clear search')}
              style={{
                position: 'absolute',
                right: '6px',
                top: '50%',
                transform: 'translateY(-50%)',
                background: 'transparent',
                border: 'none',
                color: 'var(--color-text-dimmed, #808080)',
                cursor: 'pointer',
                fontSize: '16px',
                lineHeight: 1,
                padding: '2px',
              }}
            >
              ×
            </button>
          )}
        </div>

        {/* Sort Controls */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
          <select
            data-testid="library-sort-select"
            value={sort.key}
            onChange={handleSortKeyChange}
            aria-label="Sort by"
            style={{
              height: '32px',
              backgroundColor: 'var(--color-surface, #262525)',
              color: 'var(--color-text-default, #D9D9D9)',
              border: '1px solid var(--color-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              padding: '0 8px',
              fontSize: 'var(--font-size-xs, 12px)',
              cursor: 'pointer',
              outline: 'none',
            }}
          >
            <option value="title">{t('Library::Title')}</option>
            <option value="provider">{t('Library::Store')}</option>
            <option value="matchStatus">{t('Library::Match status')}</option>
          </select>

          <button
            type="button"
            data-testid="library-sort-direction-btn"
            onClick={handleToggleSortDirection}
            style={{
              height: '32px',
              backgroundColor: 'var(--color-surface, #262525)',
              color: 'var(--color-text-default, #D9D9D9)',
              border: '1px solid var(--color-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              padding: '0 10px',
              fontSize: 'var(--font-size-xs, 12px)',
              cursor: 'pointer',
            }}
          >
            {sort.direction === 'ascending' ? t('Library::A → Z') : t('Library::Z → A')}
          </button>
        </div>

        {/* View Mode Switcher */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
          <button
            type="button"
            data-testid="library-view-grid-btn"
            onClick={handleViewGrid}
            style={{
              height: '32px',
              backgroundColor: view === 'grid' ? 'var(--color-tertiary, #872C13)' : 'var(--color-surface, #262525)',
              color: view === 'grid' ? 'var(--color-on-primary, #EDEDED)' : 'var(--color-text-default, #D9D9D9)',
              border: `1px solid ${view === 'grid' ? 'var(--color-primary, #F15025)' : 'var(--color-border-subtle, #2A2A2A)'}`,
              borderRadius: 'var(--radius-sm, 4px)',
              padding: '0 10px',
              fontSize: 'var(--font-size-xs, 12px)',
              cursor: 'pointer',
            }}
          >
            {t('Library::Grid')}
          </button>

          <button
            type="button"
            data-testid="library-view-list-btn"
            onClick={handleViewList}
            style={{
              height: '32px',
              backgroundColor: view === 'list' ? 'var(--color-tertiary, #872C13)' : 'var(--color-surface, #262525)',
              color: view === 'list' ? 'var(--color-on-primary, #EDEDED)' : 'var(--color-text-default, #D9D9D9)',
              border: `1px solid ${view === 'list' ? 'var(--color-primary, #F15025)' : 'var(--color-border-subtle, #2A2A2A)'}`,
              borderRadius: 'var(--radius-sm, 4px)',
              padding: '0 10px',
              fontSize: 'var(--font-size-xs, 12px)',
              cursor: 'pointer',
            }}
          >
            {t('Library::List')}
          </button>

          <button
            type="button"
            data-testid="library-toggle-details-btn"
            onClick={handleToggleDetails}
            style={{
              height: '32px',
              backgroundColor: detailOpen ? 'var(--color-tertiary, #872C13)' : 'var(--color-surface, #262525)',
              color: detailOpen ? 'var(--color-on-primary, #EDEDED)' : 'var(--color-text-default, #D9D9D9)',
              border: `1px solid ${detailOpen ? 'var(--color-primary, #F15025)' : 'var(--color-border-subtle, #2A2A2A)'}`,
              borderRadius: 'var(--radius-sm, 4px)',
              padding: '0 10px',
              fontSize: 'var(--font-size-xs, 12px)',
              cursor: 'pointer',
            }}
          >
            {t('Library::Details')}
          </button>
        </div>

        {/* Rescan Button */}
        <button
          type="button"
          data-testid="library-rescan-btn"
          onClick={handleRescan}
          style={{
            height: '32px',
            backgroundColor: 'transparent',
            color: 'var(--color-primary, #F15025)',
            border: '1px solid var(--color-primary, #F15025)',
            borderRadius: 'var(--radius-sm, 4px)',
            padding: '0 12px',
            fontSize: 'var(--font-size-xs, 12px)',
            fontWeight: 500,
            cursor: 'pointer',
            marginLeft: 'auto',
          }}
        >
          {t('Library::Rescan')}
        </button>
      </div>
    </div>
  );
};
