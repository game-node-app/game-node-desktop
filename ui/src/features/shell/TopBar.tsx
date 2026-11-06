import { IconMenu2 } from '@tabler/icons-react';
import type { AuthView } from '../../api/types';
import { useAuth } from '../../api/useSnapshot';
import { uiStore, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { AppMenu } from './AppMenu';

export interface TopBarProps {
  auth?: AuthView;
  className?: string;
}

export function TopBar({ auth: propAuth, className }: TopBarProps = {}) {
  const authState = useAuth();
  const { t } = useI18n();
  const appMenuOpen = useUiStore((s) => s.appMenuOpen);

  const auth = propAuth !== undefined ? propAuth : authState.auth;
  const isSignedIn =
    typeof auth === 'object' && auth !== null && 'signedIn' in auth;
  const user = isSignedIn ? auth.signedIn.user : null;
  const displayName = user
    ? user.username || user.email || user.id
    : '';

  const handleToggleMenu = () => {
    uiStore.setState((s) => ({ ...s, appMenuOpen: !s.appMenuOpen }));
  };

  return (
    <header
      role="banner"
      data-testid="top-bar"
      data-tauri-drag-region
      className={`relative z-[100] flex h-topbar items-center justify-between border-b border-border-subtle bg-chrome px-md select-none ${className ?? ''}`}
    >
      {/* Brand logo & DESKTOP badge */}
      <div
        data-testid="brand-logo"
        data-tauri-drag-region
        className="flex items-center gap-compact"
      >
        <span className="text-lg font-bold tracking-[-0.3px] text-brand">
          GameNode
        </span>
        <span
          data-testid="desktop-badge"
          className="rounded-xs bg-surface px-[6px] py-[2px] text-[10px] font-bold tracking-[0.8px] text-text-dimmed"
        >
          DESKTOP
        </span>
      </div>

      {/* Right side controls: status / user display name / menu toggle */}
      <div className="flex items-center gap-sm">
        {isSignedIn && (
          <div
            data-testid="user-profile"
            className="flex items-center gap-compact"
          >
            <span
              data-testid="online-dot"
              aria-hidden="true"
              className="inline-block h-[8px] w-[8px] rounded-full bg-success"
            />
            <span
              data-testid="user-display-name"
              className="max-w-[160px] truncate text-[13px] font-medium text-text-default"
            >
              {displayName}
            </span>
          </div>
        )}

        <button
          type="button"
          aria-label={t('Application menu')}
          aria-expanded={appMenuOpen}
          aria-haspopup="menu"
          onClick={handleToggleMenu}
          className={`flex h-[32px] cursor-pointer items-center justify-center rounded-sm border border-border-subtle px-sm text-text-default transition-colors hover:bg-surface ${
            appMenuOpen ? 'bg-surface' : 'bg-transparent'
          }`}
        >
          <IconMenu2 size={16} stroke={1.75} aria-hidden="true" />
        </button>
      </div>

      {appMenuOpen && (
        <AppMenu
          isSignedIn={isSignedIn}
          onClose={() => uiStore.setState((s) => ({ ...s, appMenuOpen: false }))}
        />
      )}
    </header>
  );
}
