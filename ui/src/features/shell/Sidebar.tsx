import {
  useAppInfo,
  useLibraryView,
  useLocalGames,
} from '../../api/useSnapshot';
import { commands } from '../../api/commands';
import type { LibraryView, LocalView } from '../../api/types';
import { openSettingsModal, uiStore, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { IconAlertTriangle, IconPlus, IconSettings } from '@tabler/icons-react';
import { formatI18n } from '../library/formatters';
import { ResizeHandle } from './ResizeHandle';

export interface SidebarProps {
  library?: LibraryView | null;
  localGames?: LocalView | null;
  version?: string;
  className?: string;
}

export function Sidebar({
  library: propLibrary,
  localGames: propLocalGames,
  version: propVersion,
  className,
}: SidebarProps = {}) {
  const { t } = useI18n();
  const hookLibrary = useLibraryView();
  const hookLocalGames = useLocalGames();
  const appInfo = useAppInfo();
  const dragWidth = useUiStore((s) => s.sidebarDragWidth);

  const library = propLibrary !== undefined ? propLibrary : hookLibrary;
  const localGames = propLocalGames !== undefined ? propLocalGames : hookLocalGames;
  const version =
    propVersion ?? appInfo?.data?.version ?? '0.1.0';

  const storedWidth = library?.layout?.sidebarWidth ?? 216;
  const rawWidth = dragWidth ?? storedWidth;
  const clampedWidth = Math.min(360, Math.max(160, Math.round(rawWidth)));

  const activeFilter = library?.layout?.filter ?? 'all';
  const installedFiltered = library?.layout?.filters?.availability.installed ?? false;
  const isAllActive = activeFilter === 'all';
  const isProviderActive = (id: string) =>
    typeof activeFilter === 'object' &&
    activeFilter !== null &&
    'provider' in activeFilter &&
    activeFilter.provider === id;
  // Provider counts already reflect every active facet except the provider choice, so their sum
  // is exactly what "All games" would show.
  const allCount = (library?.providers ?? []).reduce(
    (sum, provider) => sum + provider.count,
    0,
  );

  const failedProviders = library?.failedProviders ?? [];
  const isProviderFailed = (id: string, name: string) =>
    failedProviders.includes(id) || failedProviders.includes(name);

  const candidates = (localGames?.candidates ?? []).filter((c) => !c.ignored);
  const hasCandidates = candidates.length > 0;

  const handleSelectAll = () => {
    commands.libraryInput({ setFilter: 'all' });
  };

  const handleSelectProvider = (providerId: string) => {
    commands.libraryInput({ setFilter: { provider: providerId } });
  };

  const handleAddGame = () => {
    commands.pickLocalExecutable(null);
  };

  const handleOpenDiscovery = () => {
    uiStore.setState((s) => ({ ...s, discoveryModalOpen: true }));
  };

  const handleOpenSettings = () => {
    openSettingsModal('account');
  };

  return (
    <aside
      data-testid="sidebar"
      className={`relative flex h-full min-w-[160px] max-w-[360px] shrink-0 flex-row border-r border-border-subtle bg-chrome select-none ${className ?? ''}`}
      style={{ width: `${clampedWidth}px` }}
    >
      {/* Sidebar Content */}
      <div className="flex h-full flex-1 flex-col gap-md overflow-hidden p-md">
        {/* Navigation Section */}
        <div className="flex flex-1 flex-col gap-[4px] overflow-y-auto">
          <div className="mb-[6px] text-[11px] font-semibold tracking-[0.8px] text-text-dimmed uppercase">
            {t('YOUR SPACE')}
          </div>

          {/* All Games Item */}
          <button
            type="button"
            data-testid="sidebar-all-games"
            onClick={handleSelectAll}
            className={`flex w-full cursor-pointer items-center justify-between rounded-md border px-sm py-compact text-left text-sm transition-colors ${
              isAllActive
                ? 'border-brand bg-accent-subtle font-semibold text-brand'
                : 'border-transparent bg-transparent font-normal text-text-default'
            }`}
          >
            <span>{t('All games')}</span>
            <span
              data-testid="sidebar-all-count"
              className={`text-xs ${
                isAllActive ? 'text-brand' : 'text-text-dimmed'
              }`}
            >
              {allCount}
            </span>
          </button>

          {/* Provider Items */}
          {library?.providers && library.providers.length > 0 && (
            <div
              data-testid="sidebar-providers-list"
              className="mt-[4px] flex flex-col gap-[2px]"
            >
              {library.providers.map((provider) => {
                const active = isProviderActive(provider.id);
                const failed = isProviderFailed(provider.id, provider.name);
                const count = provider.count;
                const showInstalledSubline =
                  !installedFiltered && provider.count !== provider.installedCount;

                return (
                  <button
                    key={provider.id}
                    type="button"
                    data-testid={`sidebar-provider-${provider.id}`}
                    onClick={() => handleSelectProvider(provider.id)}
                    className={`flex w-full cursor-pointer items-center justify-between rounded-md border px-sm py-[7px] text-left text-sm ${
                      active
                        ? 'border-brand bg-accent-subtle font-semibold text-brand'
                        : 'border-transparent bg-transparent font-normal text-text-default'
                    }`}
                  >
                    <div className="flex min-w-0 flex-1 flex-col">
                      <div className="flex items-center gap-[6px] overflow-hidden">
                        <span className="truncate">{provider.name}</span>
                        {failed && (
                          <span
                            data-testid={`provider-warning-${provider.id}`}
                            aria-label="Provider sync failure"
                            title="Provider sync failure"
                            className="text-warning"
                          >
                            <IconAlertTriangle size={14} stroke={1.75} aria-hidden="true" />
                          </span>
                        )}
                      </div>
                      {showInstalledSubline && (
                        <span
                          data-testid={`sidebar-provider-installed-${provider.id}`}
                          className="text-[11px] font-normal text-text-dimmed"
                        >
                          {formatI18n(
                            t('Library::{} installed'),
                            provider.installedCount,
                          )}
                        </span>
                      )}
                    </div>
                    <span
                      data-testid={`sidebar-provider-count-${provider.id}`}
                      className={`ml-compact text-xs ${
                        active ? 'text-brand' : 'text-text-dimmed'
                      }`}
                    >
                      {count}
                    </span>
                  </button>
                );
              })}
            </div>
          )}

          {/* DRM-Free & Local Actions */}
          <div className="mt-md flex flex-col gap-compact">
            <button
              type="button"
              data-testid="sidebar-add-game"
              onClick={handleAddGame}
              className="flex w-full cursor-pointer items-center justify-center gap-compact rounded-md border border-dashed border-border-subtle bg-transparent px-sm py-compact text-[13px] text-text-default transition-colors hover:bg-surface"
            >
              <IconPlus size={14} stroke={1.75} aria-hidden="true" />
              <span>{t('Add a game')}</span>
            </button>

            {hasCandidates && (
              <button
                type="button"
                data-testid="sidebar-discovery-button"
                onClick={handleOpenDiscovery}
                className="flex w-full cursor-pointer items-center justify-between rounded-md border border-border-subtle bg-surface px-sm py-compact text-[13px] text-text-default transition-colors hover:bg-raised"
              >
                <span>{t('Discovered games')}</span>
                <span className="rounded-full bg-brand px-[6px] py-[1px] text-[11px] font-semibold text-on-primary">
                  {candidates.length}
                </span>
              </button>
            )}
          </div>
        </div>

        {/* Footer: Settings & Version */}
        <div className="flex flex-col gap-compact border-t border-border-subtle pt-sm">
          <button
            type="button"
            data-testid="sidebar-settings-button"
            onClick={handleOpenSettings}
            className="flex w-full cursor-pointer items-center gap-compact rounded-md bg-transparent px-sm py-compact text-left text-sm text-text-default transition-colors hover:bg-surface"
          >
            <IconSettings size={16} stroke={1.75} aria-hidden="true" />
            <span>{t('Settings')}</span>
          </button>

          <div
            data-testid="sidebar-version-label"
            className="pl-sm text-[11px] text-text-dimmed"
          >
            GameNode · {version}
          </div>
        </div>
      </div>

      {/* Resizable handle on the right edge */}
      <ResizeHandle currentWidth={clampedWidth} />
    </aside>
  );
}
