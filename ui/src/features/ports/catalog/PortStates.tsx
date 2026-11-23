import React from 'react';
import { IconAlertTriangle, IconRefresh } from '@tabler/icons-react';
import type { PortCatalogError } from '../../../api/ports';
import { useI18n } from '../../../i18n';
import { formatI18n } from '../../library/formatters';
import { catalogErrorMessage } from './format';

export const PortRefreshingBanner: React.FC = () => {
  const { t } = useI18n();
  return (
    <div
      data-testid="banner-ports-refreshing"
      className="border-b border-border-subtle bg-accent-subtle px-md py-compact text-xs text-text-default"
    >
      {t('Ports::Refreshing the catalog')}
    </div>
  );
};

export interface PortStaleBannerProps {
  fetchedAt: number | null;
  age: string;
  error?: PortCatalogError | null;
}

export const PortStaleBanner: React.FC<PortStaleBannerProps> = ({
  fetchedAt,
  age,
  error,
}) => {
  const { t } = useI18n();
  const detail = fetchedAt && age
    ? formatI18n(t('Ports::Showing the cached catalog from {}'), age)
    : t('Ports::Showing the cached catalog');
  return (
    <div
      data-testid="banner-ports-stale"
      className="flex flex-wrap items-center gap-compact border-b border-border-subtle bg-warning-subtle px-md py-compact text-xs text-warning"
    >
      <IconAlertTriangle size={14} stroke={1.75} aria-hidden />
      <span>{detail}</span>
      {error && (
        <span className="text-text-default">{catalogErrorMessage(error, t)}</span>
      )}
    </div>
  );
};

export interface PortUnavailableStateProps {
  error: PortCatalogError | null | undefined;
  onRetry: () => void;
  retrying: boolean;
}

export const PortUnavailableState: React.FC<PortUnavailableStateProps> = ({
  error,
  onRetry,
  retrying,
}) => {
  const { t } = useI18n();
  return (
    <div
      data-testid="ports-unavailable"
      className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center"
    >
      <IconAlertTriangle size={28} stroke={1.5} aria-hidden className="text-warning" />
      <h2 className="text-xl text-text-default">{t('Ports::Catalog unavailable')}</h2>
      <p className="max-w-[520px] text-sm text-text-dimmed">
        {catalogErrorMessage(error, t)}
      </p>
      <button
        type="button"
        data-testid="ports-unavailable-retry"
        onClick={onRetry}
        disabled={retrying}
        aria-busy={retrying}
        className="flex h-8 items-center gap-1.5 rounded-sm border border-brand px-md text-xs font-medium text-brand transition-colors hover:bg-accent-subtle disabled:cursor-default disabled:opacity-45"
      >
        <IconRefresh size={14} stroke={1.75} aria-hidden />
        {t('Library::Retry')}
      </button>
    </div>
  );
};

export const PortLoadingState: React.FC<{ label: string }> = ({ label }) => (
  <div
    data-testid="ports-loading"
    className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center text-sm text-text-dimmed"
  >
    <span>{label}</span>
  </div>
);

export interface PortEmptyStateProps {
  totalCount: number;
  hasSearch: boolean;
  hasFilters: boolean;
  onClearSearch: () => void;
  onClearFilters: () => void;
  onRefresh: () => void;
}

export const PortEmptyState: React.FC<PortEmptyStateProps> = ({
  totalCount,
  hasSearch,
  hasFilters,
  onClearSearch,
  onClearFilters,
  onRefresh,
}) => {
  const { t } = useI18n();
  if (totalCount === 0) {
    return (
      <div
        data-testid="ports-empty"
        className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center"
      >
        <h2 className="text-xl text-text-default">{t('Ports::The catalog is empty.')}</h2>
        <p className="max-w-[520px] text-sm text-text-dimmed">
          {t('Ports::Refresh to check again.')}
        </p>
        <button
          type="button"
          data-testid="ports-empty-refresh"
          onClick={onRefresh}
          className="flex h-8 items-center gap-1.5 rounded-sm border border-border-subtle bg-transparent px-md text-xs text-text-default transition-colors hover:bg-surface"
        >
          <IconRefresh size={14} stroke={1.75} aria-hidden />
          {t('Ports::Refresh')}
        </button>
      </div>
    );
  }

  return (
    <div
      data-testid="ports-empty"
      className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-center"
    >
      <h2 className="text-xl text-text-default">{t('Ports::No ports match.')}</h2>
      <p className="max-w-[520px] text-sm text-text-dimmed">
        {t('Ports::Nothing in the catalog matches your search or filters.')}
      </p>
      <div className="flex flex-wrap items-center justify-center gap-compact">
        {hasSearch && (
          <button
            type="button"
            data-testid="ports-empty-clear-search"
            onClick={onClearSearch}
            className="rounded-sm border border-border-subtle bg-transparent px-md py-compact text-text-default transition-colors hover:bg-surface"
          >
            {t('Library::Clear search')}
          </button>
        )}
        {hasFilters && (
          <button
            type="button"
            data-testid="ports-empty-clear-filters"
            onClick={onClearFilters}
            className="rounded-sm border border-brand bg-transparent px-md py-compact font-medium text-brand transition-colors hover:bg-accent-subtle"
          >
            {t('Library::Clear all filters')}
          </button>
        )}
      </div>
    </div>
  );
};
