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
      className={className}
      style={{
        position: 'relative',
        height: 'var(--size-topbar-height, 54px)',
        backgroundColor: 'var(--color-chrome, #111111)',
        borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '0 16px',
        boxSizing: 'border-box',
        userSelect: 'none',
        zIndex: 100,
      }}
    >
      {/* Brand logo & DESKTOP badge */}
      <div
        data-testid="brand-logo"
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: '8px',
        }}
      >
        <span
          style={{
            fontSize: '18px',
            fontWeight: 700,
            color: 'var(--color-primary, #F15025)',
            letterSpacing: '-0.3px',
          }}
        >
          GameNode
        </span>
        <span
          data-testid="desktop-badge"
          style={{
            fontSize: '10px',
            fontWeight: 700,
            letterSpacing: '0.8px',
            padding: '2px 6px',
            borderRadius: 'var(--radius-xs, 2px)',
            backgroundColor: 'var(--color-surface, #262525)',
            color: 'var(--color-text-dimmed, #808080)',
          }}
        >
          DESKTOP
        </span>
      </div>

      {/* Right side controls: status / user display name / menu toggle */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: '12px',
        }}
      >
        {isSignedIn && (
          <div
            data-testid="user-profile"
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: '8px',
            }}
          >
            <span
              data-testid="online-dot"
              aria-hidden="true"
              style={{
                width: '8px',
                height: '8px',
                borderRadius: '50%',
                backgroundColor: 'var(--color-success, #3DD68C)',
                display: 'inline-block',
              }}
            />
            <span
              data-testid="user-display-name"
              style={{
                fontSize: '13px',
                fontWeight: 500,
                color: 'var(--color-text-default, #D9D9D9)',
                maxWidth: '160px',
                whiteSpace: 'nowrap',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
              }}
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
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            height: '32px',
            padding: '0 12px',
            backgroundColor: appMenuOpen
              ? 'var(--color-surface, #262525)'
              : 'transparent',
            color: 'var(--color-text-default, #D9D9D9)',
            border: '1px solid var(--color-border-subtle, #2A2A2A)',
            borderRadius: 'var(--radius-sm, 4px)',
            fontSize: '13px',
            cursor: 'pointer',
            transition: 'background-color 0.15s',
          }}
        >
          ☰
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
