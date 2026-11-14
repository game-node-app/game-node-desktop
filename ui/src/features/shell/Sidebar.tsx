import { providerLabel } from '../../i18n/providerLabel';
import type { ReactNode } from 'react';
import {
  useAppInfo,
  useAuth,
  useLibraryView,
  useLocalGames,
} from '../../api/useSnapshot';
import {
  detectPortPlatform,
  usePortsView,
  type PortPlatform,
  type PortsView,
} from '../../api/ports';
import { commands } from '../../api/commands';
import type { LibraryView, LocalView, User } from '../../api/types';
import {
  openSettingsModal,
  setActiveDestination,
  uiStore,
  useUiStore,
} from '../../store/uiStore';
import { useI18n } from '../../i18n';
import {
  IconAdjustmentsHorizontal,
  IconAlertTriangle,
  IconBooks,
  IconChevronLeft,
  IconChevronRight,
  IconCompass,
  IconDeviceGamepad2,
  IconPlus,
  IconRadar,
} from '@tabler/icons-react';
import { formatI18n } from '../library/formatters';
import { ResizeHandle } from './ResizeHandle';

export interface SidebarProps {
  library?: LibraryView | null;
  localGames?: LocalView | null;
  /** Injected in tests; the hook reads `Snapshot.ports` otherwise. */
  ports?: PortsView | null;
  /** Injected in tests; the snapshot auth decides otherwise. */
  signedIn?: boolean;
  /** Injected in tests; the snapshot auth user decides otherwise. */
  user?: User | null;
  /** Injected in tests; `AppInfo.platform` or the user agent decides otherwise. */
  platform?: PortPlatform;
  className?: string;
}

interface SidebarItemStatus {
  testId?: string;
  label: string;
  count: number;
}

interface SidebarItemProps {
  testId?: string;
  label: string;
  icon: ReactNode;
  collapsed: boolean;
  active?: boolean;
  onClick: () => void;
  count?: number;
  countTestId?: string;
  subline?: string;
  sublineTestId?: string;
  warning?: { testId: string; label: string };
  status?: SidebarItemStatus;
}

/**
 * One dock destination. Expanded rows show the reference design's label, optional
 * count and provider subline; collapsed rows keep the accessible name through
 * `aria-label`/`title` and reduce status to a dot on the icon.
 */
function SidebarItem({
  testId,
  label,
  icon,
  collapsed,
  active,
  onClick,
  count,
  countTestId,
  subline,
  sublineTestId,
  warning,
  status,
}: SidebarItemProps) {
  const stateClasses = active
    ? 'bg-accent-subtle font-semibold text-brand'
    : 'font-normal text-text-secondary hover:bg-raised';

  // The button's own label replaces its contents for assistive tech, so status joins it here.
  const collapsedName = [label, status?.label, warning?.label].filter(Boolean).join(', ');

  return (
    <button
      type="button"
      data-testid={testId}
      aria-current={active ? 'page' : undefined}
      aria-label={collapsed ? collapsedName : undefined}
      title={collapsed ? label : undefined}
      onClick={onClick}
      className={`flex h-[40px] shrink-0 cursor-pointer items-center rounded-md text-left text-sm transition-colors focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-inset focus-visible:outline-none ${
        collapsed ? 'w-[48px] justify-center' : 'w-full gap-sm px-sm'
      } ${stateClasses}`}
    >
      <span className="relative flex shrink-0 items-center justify-center">
        {icon}
        {collapsed && status && (
          <span
            data-testid={status.testId}
            aria-label={status.label}
            title={status.label}
            className="absolute -top-[2px] -right-[2px] h-[8px] w-[8px] rounded-full bg-brand"
          />
        )}
        {collapsed && warning && (
          <span
            data-testid={warning.testId}
            aria-label={warning.label}
            title={warning.label}
            className="absolute -top-[2px] -right-[2px] h-[8px] w-[8px] rounded-full bg-warning"
          />
        )}
      </span>

      {!collapsed && (
        <>
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="flex items-center gap-[6px] overflow-hidden">
              <span className="truncate">{label}</span>
              {warning && (
                <span
                  data-testid={warning.testId}
                  aria-label={warning.label}
                  title={warning.label}
                  className="shrink-0 text-warning"
                >
                  <IconAlertTriangle size={14} stroke={1.75} aria-hidden="true" />
                </span>
              )}
            </span>
            {subline && (
              <span
                data-testid={sublineTestId}
                className="truncate text-[11px] font-normal text-text-dimmed"
              >
                {subline}
              </span>
            )}
          </span>

          {count !== undefined && (
            <span
              data-testid={countTestId}
              className={`ml-auto shrink-0 text-xs ${
                active ? 'text-brand' : 'text-text-secondary'
              }`}
            >
              {count}
            </span>
          )}

          {status && (
            <span
              data-testid={status.testId}
              aria-label={status.label}
              className="ml-auto shrink-0 rounded-full bg-brand px-[6px] py-[1px] text-[11px] font-semibold text-on-primary"
            >
              {status.count}
            </span>
          )}
        </>
      )}
    </button>
  );
}

function providerIcon(_id: string): ReactNode {
  // Every provider shares a generic glyph so host crates and the UI never branch on a store id.
  return <IconDeviceGamepad2 size={20} stroke={1.75} aria-hidden="true" />;
}

export function Sidebar({
  library: propLibrary,
  localGames: propLocalGames,
  ports: propPorts,
  signedIn: propSignedIn,
  user: propUser,
  platform: propPlatform,
  className,
}: SidebarProps = {}) {
  const { t } = useI18n();
  const hookLibrary = useLibraryView();
  const hookLocalGames = useLocalGames();
  const hookPorts = usePortsView();
  const { auth } = useAuth();
  const appInfo = useAppInfo();
  const dragWidth = useUiStore((s) => s.sidebarDragWidth);
  const destination = useUiStore((s) => s.activeDestination);

  const library = propLibrary !== undefined ? propLibrary : hookLibrary;
  const localGames = propLocalGames !== undefined ? propLocalGames : hookLocalGames;
  const ports = propPorts !== undefined ? propPorts : hookPorts;

  const authUser =
    typeof auth === 'object' && auth !== null && 'signedIn' in auth
      ? auth.signedIn.user
      : null;
  const user = propUser !== undefined ? propUser : authUser;

  const signedIn =
    propSignedIn ??
    (typeof auth === 'object' && auth !== null && 'signedIn' in auth);
  const platform =
    propPlatform ??
    detectPortPlatform(
      typeof navigator !== 'undefined' ? navigator.userAgent : null,
      appInfo?.data?.platform,
    );
  const showPortsDestination = signedIn && platform === 'windows';

  const collapsed = library?.layout?.sidebarCollapsed ?? false;
  const storedWidth = library?.layout?.sidebarWidth ?? 248;
  const rawWidth = dragWidth ?? storedWidth;
  const clampedWidth = Math.min(360, Math.max(160, Math.round(rawWidth)));

  // Exactly one destination is active. The library's filter only marks one of its items
  // while the library destination itself is showing.
  const onLibraryDestination = destination === 'library';
  const onPortsDestination = destination === 'ports';
  const activeFilter = library?.layout?.filter ?? 'all';
  const installedFiltered = library?.layout?.filters?.availability.installed ?? false;
  const isAllActive = onLibraryDestination && activeFilter === 'all';
  const isProviderActive = (id: string) =>
    onLibraryDestination &&
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
  const unseenInstalls = ports?.unseenCount ?? 0;

  const displayName = user ? user.username || user.email || user.id : '';
  const avatarInitial = displayName.slice(0, 1).toUpperCase();

  const handleToggleCollapsed = () => {
    commands.libraryInput({ setSidebarCollapsed: !collapsed });
  };

  const handleSelectAll = () => {
    setActiveDestination('library');
    commands.libraryInput({ setFilter: 'all' });
  };

  const handleSelectProvider = (providerId: string) => {
    setActiveDestination('library');
    commands.libraryInput({ setFilter: { provider: providerId } });
  };

  const handleSelectPorts = () => {
    setActiveDestination('ports');
    // The host starts the first catalog fetch only when it has none; the command is
    // idempotent, so repeated selections do not refetch.
    commands.portsOpen();
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
      className={`relative flex h-full shrink-0 flex-row border-r border-border-subtle bg-chrome select-none ${
        collapsed ? '' : 'min-w-[160px] max-w-[360px]'
      } ${className ?? ''}`}
      style={{ width: `${collapsed ? 72 : clampedWidth}px` }}
    >
      <div
        className={`flex h-full min-w-0 flex-1 flex-col gap-md overflow-hidden ${
          collapsed ? 'px-sm py-md' : 'p-md'
        }`}
      >
        {/* Header: navigation label plus the collapse/expand dock control. */}
        <div
          className={`flex h-[40px] shrink-0 items-center ${
            collapsed ? 'justify-center' : 'justify-between gap-sm'
          }`}
        >
          {!collapsed && (
            <span className="truncate text-sm font-semibold text-white">
              {t('Navigate')}
            </span>
          )}
          <button
            type="button"
            data-testid="sidebar-collapse-toggle"
            aria-label={collapsed ? t('Expand sidebar') : t('Collapse sidebar')}
            aria-expanded={!collapsed}
            onClick={handleToggleCollapsed}
            className="flex h-[40px] w-[40px] shrink-0 cursor-pointer items-center justify-center rounded-md text-text-default transition-colors hover:bg-raised focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-inset focus-visible:outline-none"
          >
            {collapsed ? (
              <IconChevronRight size={20} stroke={1.75} aria-hidden="true" />
            ) : (
              <IconChevronLeft size={20} stroke={1.75} aria-hidden="true" />
            )}
          </button>
        </div>

        {/* Scrolling destinations; the utilities and account rows stay pinned below. */}
        <div className="flex min-h-0 flex-1 flex-col gap-md overflow-y-auto overflow-x-hidden">
          {showPortsDestination && (
            <section
              data-testid="sidebar-explore-group"
              className="flex flex-col gap-sm"
            >
              {!collapsed && (
                <div className="text-xs font-normal text-text-secondary">
                  {t('Explore')}
                </div>
              )}
              <div className="flex flex-col gap-[4px]">
                <SidebarItem
                  testId="sidebar-ports"
                  label={t('PC ports')}
                  icon={<IconCompass size={20} stroke={1.75} aria-hidden="true" />}
                  collapsed={collapsed}
                  active={onPortsDestination}
                  onClick={handleSelectPorts}
                  status={
                    unseenInstalls > 0
                      ? {
                          testId: 'sidebar-ports-badge',
                          label: formatI18n(
                            t('Ports::{} installs running or unseen'),
                            unseenInstalls,
                          ),
                          count: unseenInstalls,
                        }
                      : undefined
                  }
                />
              </div>
            </section>
          )}

          <section className="flex flex-col gap-sm">
            {!collapsed && (
              <div className="text-xs font-normal text-text-secondary">
                {t('Your space')}
              </div>
            )}
            <div className="flex flex-col gap-[4px]">
              <SidebarItem
                testId="sidebar-all-games"
                label={t('All games')}
                icon={<IconBooks size={20} stroke={1.75} aria-hidden="true" />}
                collapsed={collapsed}
                active={isAllActive}
                onClick={handleSelectAll}
                count={allCount}
                countTestId="sidebar-all-count"
              />

              {library?.providers && library.providers.length > 0 && (
                <div
                  data-testid="sidebar-providers-list"
                  className="flex flex-col gap-[4px]"
                >
                  {library.providers.map((provider) => {
                    const active = isProviderActive(provider.id);
                    const failed = isProviderFailed(provider.id, provider.name);
                    const showInstalledSubline =
                      !installedFiltered &&
                      provider.count !== provider.installedCount;

                    return (
                      <SidebarItem
                        key={provider.id}
                        testId={`sidebar-provider-${provider.id}`}
                        label={providerLabel(provider.id, provider.name, t)}
                        icon={providerIcon(provider.id)}
                        collapsed={collapsed}
                        active={active}
                        onClick={() => handleSelectProvider(provider.id)}
                        count={provider.count}
                        countTestId={`sidebar-provider-count-${provider.id}`}
                        subline={
                          showInstalledSubline
                            ? formatI18n(
                                t('Library::{} installed'),
                                provider.installedCount,
                              )
                            : undefined
                        }
                        sublineTestId={`sidebar-provider-installed-${provider.id}`}
                        warning={
                          failed
                            ? {
                                testId: `provider-warning-${provider.id}`,
                                label: 'Provider sync failure',
                              }
                            : undefined
                        }
                      />
                    );
                  })}
                </div>
              )}
            </div>
          </section>

          <div className="flex-1" aria-hidden="true" />
        </div>

        {/* Utilities, pinned to the bottom of the navigation column. */}
        <div className="flex shrink-0 flex-col gap-[4px]">
          {hasCandidates && (
            <SidebarItem
              testId="sidebar-discovery-button"
              label={t('Discovered games')}
              icon={<IconRadar size={20} stroke={1.75} aria-hidden="true" />}
              collapsed={collapsed}
              onClick={handleOpenDiscovery}
              status={{
                testId: 'sidebar-discovery-badge',
                label: formatI18n(t('Library::{} games'), candidates.length),
                count: candidates.length,
              }}
            />
          )}
          <SidebarItem
            testId="sidebar-add-game"
            label={t('Add a game')}
            icon={<IconPlus size={20} stroke={1.75} aria-hidden="true" />}
            collapsed={collapsed}
            onClick={handleAddGame}
          />
          <SidebarItem
            testId="sidebar-settings-button"
            label={t('Settings')}
            icon={
              <IconAdjustmentsHorizontal
                size={20}
                stroke={1.75}
                aria-hidden="true"
              />
            }
            collapsed={collapsed}
            onClick={handleOpenSettings}
          />
        </div>

        {/* Account row: the snapshot has no avatar, so the initial stands in for it. */}
        {signedIn && user && (
          <button
            type="button"
            data-testid="sidebar-account-button"
            aria-label={collapsed ? displayName : undefined}
            title={collapsed ? displayName : undefined}
            onClick={handleOpenSettings}
            className={`flex h-[52px] shrink-0 cursor-pointer items-center rounded-md transition-colors hover:bg-raised focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-inset focus-visible:outline-none ${
              collapsed ? 'w-[48px] justify-center' : 'w-full gap-sm pr-sm pl-[8px]'
            }`}
          >
            <span className="flex h-[32px] w-[32px] shrink-0 items-center justify-center rounded-full bg-surface text-sm font-semibold text-white">
              {avatarInitial}
            </span>
            {!collapsed && (
              <span className="flex min-w-0 flex-1 flex-col text-left">
                <span className="truncate text-sm font-semibold text-white">
                  {displayName}
                </span>
                <span className="truncate text-xs font-normal text-text-secondary">
                  {t('My account')}
                </span>
              </span>
            )}
          </button>
        )}
      </div>

      {/* Resizable handle on the right edge; the collapsed dock keeps a fixed width. */}
      {!collapsed && (
        <div className="absolute inset-y-0 right-[-4px] z-50">
          {/* The aside's border is the visible edge; the handle only adds the grab area. */}
          <ResizeHandle currentWidth={clampedWidth} className="[&>div]:bg-transparent" />
        </div>
      )}
    </aside>
  );
}
