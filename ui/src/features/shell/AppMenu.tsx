import { useEffect, useRef, useState } from 'react';
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
        style={{
          position: 'fixed',
          inset: 0,
          zIndex: 900,
          backgroundColor: 'transparent',
        }}
      />

      <div
        ref={menuRef}
        role="menu"
        aria-label={t('Application menu')}
        tabIndex={-1}
        onKeyDown={handleKeyDown}
        className={className}
        data-testid="app-menu"
        style={{
          position: 'absolute',
          top: 'calc(100% + 4px)',
          right: '16px',
          zIndex: 1000,
          minWidth: '220px',
          backgroundColor: 'var(--color-raised, #222222)',
          border: '1px solid var(--color-border-subtle, #2A2A2A)',
          borderRadius: 'var(--radius-md, 8px)',
          padding: '8px',
          boxShadow: 'var(--shadow-modal, 0 16px 36px rgba(0, 0, 0, 0.4))',
          display: 'flex',
          flexDirection: 'column',
          gap: '2px',
        }}
      >
        {items.map((item, index) => {
          const isFocused = index === focusedIndex;
          return (
            <div key={item.id} style={{ display: 'contents' }}>
              {item.separatorBefore && (
                <div
                  role="separator"
                  style={{
                    height: '1px',
                    backgroundColor: 'var(--color-border-subtle, #2A2A2A)',
                    margin: '6px 4px',
                  }}
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
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  width: '100%',
                  textAlign: 'left',
                  padding: '8px 12px',
                  backgroundColor: isFocused
                    ? 'var(--color-surface, #262525)'
                    : 'transparent',
                  color: isFocused
                    ? 'var(--color-on-primary, #EDEDED)'
                    : 'var(--color-text-default, #D9D9D9)',
                  border: 'none',
                  borderRadius: 'var(--radius-sm, 4px)',
                  fontSize: 'var(--font-size-sm, 14px)',
                  cursor: 'pointer',
                  outline: 'none',
                  boxSizing: 'border-box',
                }}
              >
                {item.label}
              </button>
            </div>
          );
        })}
      </div>
    </>
  );
}
