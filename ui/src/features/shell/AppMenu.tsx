import {
  IconBooks,
  IconChevronDown,
  IconInfoCircle,
  IconKeyboard,
  IconLogout,
  IconPlus,
  IconPower,
  IconRefresh,
  IconSettings,
  IconUser,
} from '@tabler/icons-react';
import type { TablerIcon } from '@tabler/icons-react';
import { Menu } from '@mantine/core';
import { commands } from '../../api/commands';
import {
  openSettingsModal,
  openShortcuts,
  setActiveDestination,
  uiStore,
  useUiStore,
} from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { KbdCombo } from '../../components/ui/Kbd';
import {
  menuItemShortcutClass,
  menuItemStackClass,
} from '../../components/ui/menuClasses';
import { MenuSurface } from '../../components/ui/MenuSurface';

export interface AppMenuItem {
  id: string;
  label: string;
  action: () => void;
  separatorBefore?: boolean;
}

export interface AppMenuProps {
  isSignedIn?: boolean;
  /** The signed-in user's display name; omitted when signed out. */
  userName?: string | null;
  onClose?: () => void;
  className?: string;
}

/** Closes the menu before running an action that is not a modal opener itself. */
function closeMenu(): void {
  uiStore.setState((state) =>
    state.appMenuOpen ? { ...state, appMenuOpen: false } : state,
  );
}

function MenuIcon({ icon: Icon }: { icon: TablerIcon }) {
  return <Icon size={16} stroke={2} aria-hidden="true" className="text-text-secondary" />;
}

function MenuShortcut({ keys }: { keys: string[] }) {
  return (
    <span className={menuItemShortcutClass}>
      <KbdCombo keys={keys} />
    </span>
  );
}

/**
 * The account menu opened by the top-bar avatar, per D9 and
 * `raw/06-L4-menus-popovers.txt`: a connection header, the Library section,
 * Settings/About and the session section. Mantine Menu supplies focus,
 * keyboard navigation and outside-click handling; `MenuSurface` supplies the
 * shared redesign surface.
 */
export function AppMenu({
  isSignedIn = false,
  userName,
  onClose,
  className,
}: AppMenuProps = {}) {
  const { t } = useI18n();
  const appMenuOpen = useUiStore((state) => state.appMenuOpen);
  const name = userName?.trim() ?? '';
  const initial = name ? name.charAt(0).toUpperCase() : '';
  const connectedLabel = t('AppMenu::Connected to GameNode');

  const run = (action: () => void) => {
    closeMenu();
    onClose?.();
    action();
  };

  return (
    <MenuSurface
      opened={appMenuOpen}
      onChange={(open) => {
        uiStore.setState((state) => ({ ...state, appMenuOpen: open }));
        if (!open) onClose?.();
      }}
      position="bottom-end"
      offset={4}
      width={260}
      transitionProps={{ duration: 0 }}
      classNames={{
        dropdown: className,
      }}
      closeOnItemClick
    >
      <Menu.Target>
        <button
          type="button"
          data-testid="app-menu-trigger"
          aria-label={t('Shell::Account menu')}
          className="flex h-[32px] shrink-0 items-center gap-[8px] rounded-control px-[8px] text-text-default transition-colors hover:bg-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
        >
          <span
            aria-hidden="true"
            className="flex h-[24px] w-[24px] shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-brand to-tertiary text-[11px] font-semibold leading-none text-text-primary"
          >
            {initial || <IconUser size={14} stroke={2} />}
          </span>
          {isSignedIn && name && (
            <span
              data-testid="user-display-name"
              className="max-w-[120px] truncate text-[13px] font-medium text-text-default"
            >
              {name}
            </span>
          )}
          <IconChevronDown
            size={14}
            stroke={2}
            aria-hidden="true"
            className="text-text-dimmed"
          />
        </button>
      </Menu.Target>

      <Menu.Dropdown data-testid="app-menu" aria-label={t('Application menu')}>
        <div
          data-testid="app-menu-header"
          className="flex items-center gap-[10px] p-[10px]"
        >
          <span
            aria-hidden="true"
            className="flex h-[32px] w-[32px] shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-brand to-tertiary text-[13px] font-semibold leading-none text-text-primary"
          >
            {initial || <IconUser size={16} stroke={2} />}
          </span>
          <div className={menuItemStackClass}>
            <span className="truncate text-[13px] font-semibold text-text-primary">
              {name || (isSignedIn ? connectedLabel : t('AppMenu::Offline'))}
            </span>
            {isSignedIn && name && (
              <span className="truncate text-[11px] text-text-dimmed">
                {connectedLabel}
              </span>
            )}
          </div>
        </div>

        <Menu.Divider />

        <Menu.Item
          leftSection={<MenuIcon icon={IconBooks} />}
          onClick={() => run(() => setActiveDestination('library'))}
        >
          {t('AppMenu::Library')}
        </Menu.Item>
        <Menu.Item
          leftSection={<MenuIcon icon={IconPlus} />}
          onClick={() => run(() => commands.pickLocalExecutable(null))}
        >
          {t('AppMenu::Add a game')}
        </Menu.Item>
        <Menu.Item
          leftSection={<MenuIcon icon={IconRefresh} />}
          onClick={() => run(() => commands.rescanLibrary())}
        >
          {t('AppMenu::Rescan library')}
        </Menu.Item>
        <Menu.Item
          leftSection={<MenuIcon icon={IconSettings} />}
          rightSection={<MenuShortcut keys={['Ctrl', ',']} />}
          onClick={() => run(() => openSettingsModal('account'))}
        >
          {t('AppMenu::Settings')}
        </Menu.Item>
        <Menu.Item
          leftSection={<MenuIcon icon={IconKeyboard} />}
          rightSection={<MenuShortcut keys={['?']} />}
          onClick={() => run(() => openShortcuts())}
        >
          {t('AppMenu::Keyboard shortcuts')}
        </Menu.Item>
        <Menu.Item
          leftSection={<MenuIcon icon={IconInfoCircle} />}
          onClick={() => run(() => openSettingsModal('about'))}
        >
          {t('AppMenu::About and licenses')}
        </Menu.Item>

        <Menu.Divider />

        {isSignedIn && (
          <Menu.Item
            leftSection={<MenuIcon icon={IconLogout} />}
            onClick={() => run(() => commands.dispatchAuth('signOut'))}
          >
            {t('AppMenu::Sign out')}
          </Menu.Item>
        )}
        <Menu.Item
          leftSection={<MenuIcon icon={IconPower} />}
          rightSection={<MenuShortcut keys={['Ctrl', 'Q']} />}
          onClick={() => run(() => commands.quitApp())}
        >
          {t('AppMenu::Quit GameNode')}
        </Menu.Item>
      </Menu.Dropdown>
    </MenuSurface>
  );
}
