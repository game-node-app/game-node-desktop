import { useEffect, useRef, useState } from 'react';
import {
  IconInfoCircle,
  IconLibrary,
  IconLogout,
  IconPlus,
  IconPower,
  IconRefresh,
  IconSettings,
} from '@tabler/icons-react';
import type { TablerIcon } from '@tabler/icons-react';
import { commands } from '../../api/commands';
import { openSettingsModal, uiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';

export interface AppMenuItem {
  id: string;
  label: string;
  action: () => void;
  separatorBefore?: boolean;
}

export interface AppMenuProps {
  isSignedIn?: boolean;
  onClose?: () => void;
  className?: string;
}

const MENU_ICONS: Record<string, TablerIcon> = {
  library: IconLibrary,
  rescan: IconRefresh,
  'add-game': IconPlus,
  settings: IconSettings,
  about: IconInfoCircle,
  'sign-out': IconLogout,
  quit: IconPower,
};

export function AppMenu({
  isSignedIn = false,
  onClose,
  className,
}: AppMenuProps) {
  const { t } = useI18n();
  const menuRef = useRef<HTMLDivElement>(null);
  const [focusedIndex, setFocusedIndex] = useState(0);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const handleClose = () => {
    uiStore.setState((s) => ({ ...s, appMenuOpen: false }));
    onClose?.();
  };

  const items: AppMenuItem[] = [
    {
      id: 'library',
      label: t('Library'),
      action: () => {
        handleClose();
        uiStore.setState((s) => ({ ...s, settingsOpen: false }));
      },
    },
    {
      id: 'rescan',
      label: t('Rescan library'),
      action: () => {
        handleClose();
        commands.rescanLibrary();
      },
    },
    {
      id: 'add-game',
      label: t('Add a game'),
      action: () => {
        handleClose();
        commands.pickLocalExecutable(null);
      },
    },
    {
      id: 'settings',
      label: t('Settings'),
      action: () => {
        openSettingsModal('account');
      },
    },
    {
      id: 'about',
      label: t('About & licenses'),
      action: () => {
        openSettingsModal('about');
      },
    },
    ...(isSignedIn
      ? [
          {
            id: 'sign-out',
            label: t('Sign out'),
            action: () => {
              handleClose();
              commands.dispatchAuth('signOut');
            },
            separatorBefore: true,
          },
        ]
      : []),
    {
      id: 'quit',
      label: t('Quit'),
      action: () => {
        handleClose();
        commands.quitApp();
      },
      separatorBefore: !isSignedIn,
    },
  ];

  useEffect(() => {
    itemRefs.current = itemRefs.current.slice(0, items.length);
  }, [items.length]);

  useEffect(() => {
    const btn = itemRefs.current[focusedIndex];
    if (btn) {
      btn.focus();
    }
  }, [focusedIndex]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      handleClose();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      setFocusedIndex((prev) => (prev + 1) % items.length);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setFocusedIndex((prev) => (prev - 1 + items.length) % items.length);
    } else if (e.key === 'Home') {
      e.preventDefault();
      setFocusedIndex(0);
    } else if (e.key === 'End') {
      e.preventDefault();
      setFocusedIndex(items.length - 1);
    } else if (e.key === 'Tab') {
      handleClose();
    }
  };

  return (
    <>
      <div
        data-testid="app-menu-backdrop"
        onClick={handleClose}
        className="fixed inset-0 z-[900] bg-transparent"
      />

      <div
        ref={menuRef}
        role="menu"
        aria-label={t('Application menu')}
        tabIndex={-1}
        onKeyDown={handleKeyDown}
        data-testid="app-menu"
        className={`absolute top-[calc(100%_+_4px)] right-md z-[1000] flex min-w-[220px] flex-col gap-[2px] rounded-md border border-border-subtle bg-raised p-compact shadow-modal ${className ?? ''}`}
      >
        {items.map((item, index) => {
          const isFocused = index === focusedIndex;
          const ItemIcon = MENU_ICONS[item.id];
          return (
            <div key={item.id} className="contents">
              {item.separatorBefore && (
                <div
                  role="separator"
                  className="mx-[4px] my-[6px] h-px bg-border-subtle"
                />
              )}
              <button
                ref={(el) => {
                  itemRefs.current[index] = el;
                }}
                type="button"
                role="menuitem"
                tabIndex={isFocused ? 0 : -1}
                onClick={item.action}
                onMouseEnter={() => setFocusedIndex(index)}
                className={`flex w-full cursor-pointer items-center gap-compact rounded-sm px-sm py-compact text-left text-sm outline-none ${
                  isFocused
                    ? 'bg-surface text-on-primary'
                    : 'bg-transparent text-text-default'
                }`}
              >
                {ItemIcon && (
                  <ItemIcon
                    size={16}
                    stroke={1.75}
                    aria-hidden="true"
                    className="shrink-0"
                  />
                )}
                {item.label}
              </button>
            </div>
          );
        })}
      </div>
    </>
  );
}
