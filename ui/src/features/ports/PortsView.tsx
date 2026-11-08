import React, { useDeferredValue, useEffect, useMemo, useState } from 'react';
import { commands } from '../../api/commands';
import {
  usePortsView,
  type PortFilters,
  type PortViewMode,
} from '../../api/ports';
import { useI18n } from '../../i18n';
import { uiStore, useUiStore } from '../../store/uiStore';
import { formatI18n } from '../library/formatters';
import {
  PortCredit,
  PortEmptyState,
  PortGrid,
  PortList,
  PortLoadingState,
  PortRefreshingBanner,
  PortStaleBanner,
  PortUnavailableState,
  PortsToolbar,
  filterPortEntries,
  formatCatalogAge,
  portFiltersAreDefault,
} from './catalog';
import { PortDetailPane } from './detail';
import { PortSetupHost } from './setup';

const DEFAULT_FILTERS: PortFilters = {
  consoles: [],
  guided: false,
  verified: false,
  installed: false,
};

/**
 * Content area of the PC ports destination.
 *
 * The sidebar owns `activeDestination` and the single `ports_open` call that starts the
 * first fetch; this view reads `Snapshot.ports`, applies the local search plus the
 * remembered filters, and keeps selection local (the contract persists the view and the
 * filters only). The guided setup modal is mounted once here so closing it never stops a
 * running job (desktop-port-catalog D1).
 */
export const PortsView: React.FC = () => {
  const ports = usePortsView();
  const { t, locale } = useI18n();

  const [search, setSearch] = useState('');
  const deferredSearch = useDeferredValue(search);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const focusCatalogId = useUiStore((s) => s.portsFocusCatalogId);

  // A library entry's provenance link opens PC ports on that catalog entry once.
  useEffect(() => {
    if (!focusCatalogId) return;
    setSelectedId(focusCatalogId);
    uiStore.setState((prev) => ({ ...prev, portsFocusCatalogId: null }));
  }, [focusCatalogId]);

  const preferences = ports.preferences;
  const filters = preferences?.filters ?? DEFAULT_FILTERS;
  const view: PortViewMode = preferences?.view ?? 'grid';
  const catalog = ports.catalog;
  const entries = ports.entries;

  const consoles = useMemo(() => {
    const names = new Set<string>();
    for (const entry of entries) {
      for (const consoleName of entry.consoles) names.add(consoleName);
    }
    return Array.from(names).sort((a, b) => a.localeCompare(b));
  }, [entries]);

  const filtered = useMemo(
    () => filterPortEntries(entries, deferredSearch, filters),
    [entries, deferredSearch, filters],
  );

  const selectedEntry = useMemo(
    () => entries.find((entry) => entry.catalogId === selectedId) ?? null,
    [entries, selectedId],
  );

  // A filter, search or refresh can remove the selected entry; close the pane with it.
  useEffect(() => {
    if (!selectedId) return;
    if (!filtered.some((entry) => entry.catalogId === selectedId)) {
      setSelectedId(null);
    }
  }, [filtered, selectedId]);

  const setFilters = (next: PortFilters) => {
    commands.portsSetPreferences({ filters: next });
  };

  const setView = (next: PortViewMode) => {
    commands.portsSetPreferences({ view: next });
  };

  const refresh = () => {
    commands.portsRefresh();
  };

  const clearFilters = () => {
    setFilters({ ...DEFAULT_FILTERS });
  };

  const refreshing = catalog.state === 'refreshing';
  const unavailable = catalog.state === 'unavailable' && entries.length === 0;
  const loading =
    entries.length === 0 &&
    (catalog.state === 'idle' ||
      (catalog.state === 'refreshing' && catalog.fetchedAt == null));
  const showGridOrList = !unavailable && !loading;
  const hasSearch = search.length > 0;
  const hasFilters = !portFiltersAreDefault(filters);
  const age = formatCatalogAge(catalog.fetchedAt, locale);

  const statusText = (() => {
    if (refreshing) return t('Ports::Refreshing the catalog');
    if (catalog.state === 'unavailable') return t('Ports::Catalog unavailable');
    if (catalog.fetchedAt != null && age) {
      return formatI18n(t('Ports::Last updated {}'), age);
    }
    return t('Ports::Catalog ready');
  })();

  return (
    <div
      data-testid="ports-view"
      className="flex h-full w-full flex-col overflow-hidden bg-body"
    >
      <PortsToolbar
        search={search}
        onSearchChange={setSearch}
        filters={filters}
        onFiltersChange={setFilters}
        view={view}
        onViewChange={setView}
        consoles={consoles}
        visibleCount={filtered.length}
        totalCount={entries.length}
        refreshing={refreshing}
        onRefresh={refresh}
      />

      <div className="flex min-h-0 w-full flex-1 overflow-hidden">
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          {refreshing && entries.length > 0 && <PortRefreshingBanner />}
          {catalog.state === 'stale' && (
            <PortStaleBanner
              fetchedAt={catalog.fetchedAt}
              age={age}
              error={catalog.error}
            />
          )}

          {unavailable ? (
            <PortUnavailableState
              error={catalog.error}
              onRetry={refresh}
              retrying={refreshing}
            />
          ) : loading ? (
            <PortLoadingState label={t('Ports::The port catalog is being prepared.')} />
          ) : showGridOrList && filtered.length === 0 ? (
            <PortEmptyState
              totalCount={entries.length}
              hasSearch={hasSearch}
              hasFilters={hasFilters}
              onClearSearch={() => setSearch('')}
              onClearFilters={clearFilters}
              onRefresh={refresh}
            />
          ) : view === 'grid' ? (
            <PortGrid
              entries={filtered}
              selectedId={selectedId}
              onSelect={setSelectedId}
            />
          ) : (
            <PortList
              entries={filtered}
              selectedId={selectedId}
              onSelect={setSelectedId}
            />
          )}

          <footer
            data-testid="ports-footer"
            className="flex shrink-0 flex-wrap items-center justify-between gap-compact border-t border-border-subtle bg-chrome px-md py-compact"
          >
            <PortCredit />
            <span
              data-testid="ports-catalog-status"
              data-state={catalog.state}
              role="status"
              className="text-xs text-text-dimmed"
            >
              {statusText}
            </span>
          </footer>
        </div>

        {selectedEntry && (
          <PortDetailPane
            entry={selectedEntry}
            onClose={() => setSelectedId(null)}
          />
        )}
      </div>

      <PortSetupHost />
    </div>
  );
};
