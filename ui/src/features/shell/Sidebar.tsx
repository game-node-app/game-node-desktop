import { providerLabel } from '../../i18n/providerLabel';
import { useState, type ReactNode } from 'react';
import { Menu } from '@mantine/core';
import {
  IconBookmark,
  IconBooks,
  IconChevronLeft,
  IconChevronRight,
  IconCompass,
  IconDeviceDesktop,
  IconDeviceGamepad2,
  IconFilter,
  IconHistory,
  IconPackage,
  IconPencil,
  IconPlayerPlay,
  IconPlus,
  IconRadar,
  IconSettings,
  IconStar,
  IconTrash,
} from '@tabler/icons-react';
import {
  useAppInfo,
  useAuth,
  useLibraryView,
  useLocalGames,
  useSnapshot,
} from '../../api/useSnapshot';
import {
  detectPortPlatform,
  usePortsView,
  type PortPlatform,
  type PortsView,
} from '../../api/ports';
import { commands } from '../../api/commands';
import {
  createDefaultFilters,
  type LibraryView,
  type LocalView,
  type StoreAccountView,
  type User,
} from '../../api/types';
import {
  Button,
  IconButton,
  KbdCombo,
  MenuSurface,
  NavItem,
  SectionLabel,
  Tooltip,
  menuItemDangerClass,
} from '../../components/ui';
import {
  openRenameView,
  openSaveView,
  openSettingsModal,
  setActiveDestination,
  uiStore,
  useUiStore,
} from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { formatI18n } from '../library/formatters';
import { ResizeHandle } from './ResizeHandle';
import { SaveViewModal } from './SaveViewModal';
import { DeleteViewModal } from './DeleteViewModal';

export interface SidebarProps {
  library?: LibraryView | null;
  localGames?: LocalView | null;
  /** Injected in tests; the hook reads `Snapshot.ports` otherwise. */
  ports?: PortsView | null;
  /** Injected in tests; the snapshot auth decides otherwise. */
  signedIn?: boolean;
  /** Injected in tests; the snapshot auth user decides otherwise. */
  user?: User | null;
  /** Injected in tests; the snapshot store accounts decide otherwise. */
  storeAccounts?: StoreAccountView[] | null;
  /** Injected in tests; `AppInfo.platform` or the user agent decides otherwise. */
  platform?: PortPlatform;
  className?: string;
}

type ScopeId = 'all' | 'installed' | 'favorites' | 'recent' | 'running';

interface DestinationGroupProps {
  label: string;
  collapsed: boolean;
  action?: ReactNode;
  children: ReactNode;
}

function DestinationGroup({
  label,
  collapsed,
  action,
  children,
}: DestinationGroupProps) {
  return (
    <section className="flex flex-col gap-[2px]">
      {!collapsed && (
        <SectionLabel inset action={action}>
          {label}
        </SectionLabel>
      )}
      {children}
    </section>
  );
}

const ATTENTION_STATES = new Set(['expired', 'unreachable', 'credentialError']);

export function Sidebar({
  library: propLibrary,
  localGames: propLocalGames,
  ports: propPorts,
  signedIn: propSignedIn,
  storeAccounts: propStoreAccounts,
  platform: propPlatform,
  className,
}: SidebarProps = {}) {
  const { t } = useI18n();
  const hookLibrary = useLibraryView();
  const hookLocalGames = useLocalGames();
  const hookPorts = usePortsView();
  const snapshotStoreAccounts = useSnapshot((s) => s.storeAccounts).data;
  const { auth } = useAuth();
  const appInfo = useAppInfo();
  const dragWidth = useUiStore((s) => s.sidebarDragWidth);
  const destination = useUiStore((s) => s.activeDestination);

  const library = propLibrary !== undefined ? propLibrary : hookLibrary;
  const localGames = propLocalGames !== undefined ? propLocalGames : hookLocalGames;
  const ports = propPorts !== undefined ? propPorts : hookPorts;
  const storeAccounts =
    propStoreAccounts !== undefined ? propStoreAccounts ?? [] : snapshotStoreAccounts ?? [];

  const signedIn =
    propSignedIn ??
    (typeof auth === 'object' && auth !== null && 'signedIn' in auth);
  const platform =
    propPlatform ??
    detectPortPlatform(
      typeof navigator !== 'undefined' ? navigator.userAgent : null,
      appInfo?.data?.platform,
    );
  // The catalog and native installs exist on Windows and Linux; macOS has no install strategy.
  const showPortsDestination =
    signedIn && (platform === 'windows' || platform === 'linux');

  const collapsed = library?.layout?.sidebarCollapsed ?? false;
  const storedWidth = library?.layout?.sidebarWidth ?? 240;
  const rawWidth = dragWidth ?? storedWidth;
  const clampedWidth = Math.min(360, Math.max(160, Math.round(rawWidth)));

  const onLibraryDestination = destination === 'library';
  const onCatalogDestination = destination === 'catalog';
  const onPortsDestination = destination === 'ports';
  const scope = library?.layout?.scope ?? 'all';
  const stores = library?.layout?.filters?.stores ?? [];
  const filters = library?.layout?.filters ?? createDefaultFilters();
  const scopeCounts = library?.scopeCounts;
  const providers = library?.providers ?? [];
  const collections = library?.collections ?? [];
  const savedViews = library?.savedViews ?? [];

  const isAllActive =
    onLibraryDestination && scope === 'all' && stores.length === 0;
  const isScopeActive = (id: Exclude<ScopeId, 'all'>) =>
    onLibraryDestination && scope === id;
  const isProviderActive = (id: string) =>
    onLibraryDestination && scope === 'all' && stores.length === 1 && stores[0] === id;
  const isCollectionActive = (id: string) =>
    onLibraryDestination &&
    typeof scope === 'object' &&
    'collection' in scope &&
    scope.collection === id;
  const isSavedViewActive = (id: number, active: boolean) =>
    onLibraryDestination &&
    (active ||
      (typeof scope === 'object' &&
        'savedView' in scope &&
        scope.savedView === id));

  const failedProviders = library?.failedProviders ?? [];
  const accountsByProvider = new Map(
    storeAccounts.map((account) => [account.provider, account]),
  );
  const providerNeedsAttention = (id: string, name: string) => {
    if (failedProviders.includes(id) || failedProviders.includes(name)) return true;
    const account = accountsByProvider.get(id);
    return account
      ? ATTENTION_STATES.has(account.state) || account.persistFailed
      : false;
  };

  const candidates = (localGames?.candidates ?? []).filter(
    (candidate) => !candidate.ignored,
  );
  const hasCandidates = candidates.length > 0;
  const unseenInstalls = ports?.unseenCount ?? 0;

  const [deleteViewId, setDeleteViewId] = useState<number | null>(null);

  const handleToggleCollapsed = () => {
    commands.libraryInput({ setSidebarCollapsed: !collapsed });
  };

  const handleSelectScope = (next: ScopeId) => {
    setActiveDestination('library');
    if (next === 'all' && stores.length > 0) {
      // Leaving a store shortcut: All games means no store facet, while every other
      // facet and the search text stay exactly as they are.
      commands.libraryInput({ setScope: 'all' });
      commands.libraryInput({ setFilters: { ...filters, stores: [] } });
      return;
    }
    commands.libraryInput({ setScope: next });
  };

  const handleSelectProvider = (providerId: string) => {
    setActiveDestination('library');
    // The store shortcut is the All scope plus exactly that store. Every other facet and
    // the search text stay untouched.
    commands.libraryInput({ setScope: 'all' });
    commands.libraryInput({
      setFilters: { ...filters, stores: [providerId] },
    });
  };

  const handleSelectCollection = (collectionId: string) => {
    setActiveDestination('library');
    commands.libraryInput({ setScope: { collection: collectionId } });
  };

  const handleSelectSavedView = (id: number) => {
    setActiveDestination('library');
    commands.libraryInput({ applyView: { id } });
  };

  const handleSelectPorts = () => {
    setActiveDestination('ports');
    // The host starts the first catalog fetch only when it has none; the command is
    // idempotent, so repeated selections do not refetch.
    commands.portsOpen();
  };

  const handleOpenCatalog = () => {
    setActiveDestination('catalog');
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

  const newViewButton = (
    <Tooltip label={t('Sidebar::New view from current filters')}>
      <button
        type="button"
        data-testid="sidebar-save-view"
        aria-label={t('Sidebar::New view from current filters')}
        onClick={openSaveView}
        className="flex h-[20px] w-[20px] cursor-pointer items-center justify-center rounded-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
      >
        <IconPlus size={14} stroke={2} aria-hidden="true" />
      </button>
    </Tooltip>
  );

  return (
    <aside
      data-testid="sidebar"
      className={`group relative flex h-full shrink-0 flex-row border-r border-border-subtle bg-chrome select-none ${
        collapsed ? '' : 'min-w-[160px] max-w-[360px]'
      } ${className ?? ''}`}
      style={{ width: `${collapsed ? 72 : clampedWidth}px` }}
    >
      {!collapsed && (
        <IconButton
          data-testid="sidebar-collapse-toggle"
          icon={IconChevronLeft}
          label={t('Sidebar::Collapse sidebar')}
          aria-expanded
          variant="toggle"
          size="sm"
          onClick={handleToggleCollapsed}
          className="absolute right-[8px] top-[8px] z-10 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
        />
      )}

      <div
        className={`flex h-full min-w-0 flex-1 flex-col overflow-hidden ${
          collapsed ? 'px-sm py-md' : 'px-[12px] pb-[12px] pt-[16px]'
        }`}
      >
        {collapsed && (
          <div className="mb-[4px] flex shrink-0 justify-center">
            <IconButton
              data-testid="sidebar-collapse-toggle"
              icon={IconChevronRight}
              label={t('Sidebar::Expand sidebar')}
              aria-expanded={false}
              variant="toggle"
              size="sm"
              onClick={handleToggleCollapsed}
            />
          </div>
        )}

        {/* Scrolling destinations; the footer stays pinned below. */}
        <div className="flex min-h-0 flex-1 flex-col gap-[20px] overflow-y-auto overflow-x-hidden">
          <DestinationGroup label={t('Sidebar::Your space')} collapsed={collapsed}>
            <NavItem
              icon={IconBooks}
              label={t('Sidebar::All games')}
              count={scopeCounts?.all ?? 0}
              active={isAllActive}
              collapsed={collapsed}
              onClick={() => handleSelectScope('all')}
            />
            <NavItem
              icon={IconDeviceDesktop}
              label={t('Sidebar::Installed')}
              count={scopeCounts?.installed ?? 0}
              active={isScopeActive('installed')}
              collapsed={collapsed}
              onClick={() => handleSelectScope('installed')}
            />
            <NavItem
              icon={IconStar}
              label={t('Sidebar::Favorites')}
              count={scopeCounts?.favorites ?? 0}
              active={isScopeActive('favorites')}
              collapsed={collapsed}
              onClick={() => handleSelectScope('favorites')}
            />
            <NavItem
              icon={IconHistory}
              label={t('Sidebar::Recently played')}
              count={scopeCounts?.recent ?? 0}
              active={isScopeActive('recent')}
              collapsed={collapsed}
              onClick={() => handleSelectScope('recent')}
            />
            <NavItem
              icon={IconPlayerPlay}
              label={t('Sidebar::Running')}
              count={scopeCounts?.running ?? 0}
              active={isScopeActive('running')}
              collapsed={collapsed}
              onClick={() => handleSelectScope('running')}
            />
          </DestinationGroup>

          <DestinationGroup label={t('Sidebar::Stores')} collapsed={collapsed}>
            {providers.map((provider) => {
              const label = providerLabel(provider.id, provider.name, t);
              const attention = providerNeedsAttention(provider.id, provider.name);
              const warning = attention
                ? formatI18n(t('Sidebar::{} needs attention'), label)
                : undefined;
              return (
                <div
                  key={provider.id}
                  data-testid={`sidebar-provider-${provider.id}`}
                  className="relative"
                >
                  <NavItem
                    icon={IconDeviceGamepad2}
                    label={label}
                    count={provider.count}
                    active={isProviderActive(provider.id)}
                    collapsed={collapsed}
                    warning={warning}
                    onClick={() => handleSelectProvider(provider.id)}
                  />
                  {collapsed && warning && (
                    <span
                      data-testid={`provider-warning-${provider.id}`}
                      aria-label={warning}
                      title={warning}
                      className="absolute right-[4px] top-[4px] h-[8px] w-[8px] rounded-full bg-warning"
                    />
                  )}
                </div>
              );
            })}
          </DestinationGroup>

          <DestinationGroup
            label={t('Sidebar::Collections and views')}
            collapsed={collapsed}
            action={!collapsed && library ? newViewButton : undefined}
          >
            {collapsed && library && (
              <div className="flex justify-center">{newViewButton}</div>
            )}
            {collections.map((collection) => (
              <div
                key={collection.id}
                data-testid={`sidebar-collection-${collection.id}`}
              >
                <NavItem
                  icon={IconBookmark}
                  label={collection.name}
                  count={collection.count}
                  active={isCollectionActive(collection.id)}
                  collapsed={collapsed}
                  onClick={() => handleSelectCollection(collection.id)}
                />
              </div>
            ))}
            {savedViews.map((view) => (
              <MenuSurface key={view.id} withinPortal position="bottom-start">
                <Menu.ContextMenu>
                  <div data-testid={`sidebar-saved-view-${view.id}`}>
                    <NavItem
                      icon={IconFilter}
                      label={view.name}
                      count={view.count}
                      active={isSavedViewActive(view.id, view.active)}
                      collapsed={collapsed}
                      onClick={() => handleSelectSavedView(view.id)}
                    />
                  </div>
                </Menu.ContextMenu>
                <Menu.Dropdown>
                  <Menu.Item
                    leftSection={
                      <IconPencil size={16} stroke={2} aria-hidden="true" />
                    }
                    onClick={() => openRenameView(view.id)}
                  >
                    {t('Sidebar::Rename view')}
                  </Menu.Item>
                  <Menu.Item
                    leftSection={<IconTrash size={16} stroke={2} aria-hidden="true" />}
                    className={menuItemDangerClass}
                    onClick={() => setDeleteViewId(view.id)}
                  >
                    {t('Sidebar::Delete view')}
                  </Menu.Item>
                </Menu.Dropdown>
              </MenuSurface>
            ))}
            {!collapsed && collections.length === 0 && savedViews.length === 0 && (
              <p
                data-testid="sidebar-views-empty"
                className="px-[10px] py-[4px] text-xs font-normal text-text-dimmed"
              >
                {t('Sidebar::No collections or views yet')}
              </p>
            )}
          </DestinationGroup>

          <DestinationGroup label={t('Sidebar::Browse')} collapsed={collapsed}>
            {hasCandidates && (
              <div data-testid="sidebar-discovery-button">
                <NavItem
                  icon={IconRadar}
                  label={t('Sidebar::Discovered games')}
                  count={candidates.length}
                  countTone="brand"
                  collapsed={collapsed}
                  onClick={handleOpenDiscovery}
                />
              </div>
            )}
            {signedIn && (
              <NavItem
                icon={IconCompass}
                label={t('Sidebar::Explore GameNode')}
                active={onCatalogDestination}
                collapsed={collapsed}
                onClick={handleOpenCatalog}
              />
            )}
            {showPortsDestination && (
              <div data-testid="sidebar-ports">
                <NavItem
                  icon={IconPackage}
                  label={t('Sidebar::PC ports')}
                  active={onPortsDestination}
                  collapsed={collapsed}
                  count={collapsed && unseenInstalls > 0 ? unseenInstalls : undefined}
                  countTone="brand"
                  trailing={
                    !collapsed && unseenInstalls > 0 ? (
                      <span
                        data-testid="sidebar-ports-badge"
                        aria-label={formatI18n(
                          t('Ports::{} installs running or unseen'),
                          unseenInstalls,
                        )}
                        className="rounded-full bg-accent-subtle px-[6px] text-[10px] font-medium leading-[16px] text-brand"
                      >
                        {unseenInstalls}
                      </span>
                    ) : undefined
                  }
                  onClick={handleSelectPorts}
                />
              </div>
            )}
          </DestinationGroup>
        </div>

        {/* Footer: Add DRM-free game and Settings. */}
        <div className="mt-[12px] flex shrink-0 flex-col gap-[2px]">
          {collapsed ? (
            <div className="flex justify-center">
              <IconButton
                data-testid="sidebar-add-game"
                icon={IconPlus}
                label={t('Sidebar::Add DRM-free game')}
                variant="outlined"
                size="md"
                onClick={handleAddGame}
              />
            </div>
          ) : (
            <Button
              data-testid="sidebar-add-game"
              variant="secondary"
              size="sm"
              leftIcon={IconPlus}
              fullWidth
              onClick={handleAddGame}
            >
              {t('Sidebar::Add DRM-free game')}
            </Button>
          )}
          <div data-testid="sidebar-settings-button">
            <NavItem
              icon={IconSettings}
              label={t('Sidebar::Settings')}
              collapsed={collapsed}
              onClick={handleOpenSettings}
              trailing={!collapsed ? <KbdCombo keys={['Ctrl', ',']} /> : undefined}
            />
          </div>
        </div>
      </div>

      {/* Resizable handle on the right edge; the collapsed dock keeps a fixed width. */}
      {!collapsed && (
        <div className="absolute inset-y-0 right-[-4px] z-50">
          {/* The aside's border is the visible edge; the handle only adds the grab area. */}
          <ResizeHandle currentWidth={clampedWidth} className="[&>div]:bg-transparent" />
        </div>
      )}

      <SaveViewModal library={library ?? null} />
      <DeleteViewModal
        open={deleteViewId != null}
        viewId={deleteViewId}
        library={library ?? null}
        onClose={() => setDeleteViewId(null)}
      />
    </aside>
  );
}
