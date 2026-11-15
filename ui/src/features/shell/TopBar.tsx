import React from 'react';
import {
  IconCloudCheck,
  IconCloudOff,
  IconLoader2,
  IconSearch,
} from '@tabler/icons-react';
import type { TablerIcon } from '@tabler/icons-react';
import wordmark from '../../assets/icons/gamenode-logo.svg';
import type { Access, AuthView } from '../../api/types';
import { useAuth, useLibraryView, useSnapshot } from '../../api/useSnapshot';
import { openPalette, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { KbdCombo } from '../../components/ui/Kbd';
import { cx } from '../../components/ui/cx';
import { AppMenu } from './AppMenu';
import { toggleWindowMaximize, WindowControls } from './WindowControls';

export interface TopBarProps {
  auth?: AuthView;
  className?: string;
}

export type TopBarSyncState = 'synced' | 'syncing' | 'off' | 'offline';

export interface TopBarSyncInput {
  signedIn: boolean;
  syncEnabled: boolean;
  access?: Access;
  checking: boolean;
}

/**
 * The top bar's sync indicator, derived from the snapshot: the sync preference,
 * the library's verified/offline access and whether a store or scan is running.
 */
export function topBarSyncState({
  signedIn,
  syncEnabled,
  access,
  checking,
}: TopBarSyncInput): TopBarSyncState | null {
  if (!signedIn) return null;
  if (!syncEnabled) return 'off';
  if (access === 'offline') return 'offline';
  if (!access || access === 'unverified' || checking) return 'syncing';
  return 'synced';
}

const SYNC_PRESENTATION: Record<
  TopBarSyncState,
  { label: string; icon: TablerIcon; iconClass: string }
> = {
  synced: {
    label: 'Shell::Synced',
    icon: IconCloudCheck,
    iconClass: 'text-success',
  },
  syncing: {
    label: 'Shell::Syncing…',
    icon: IconLoader2,
    iconClass: 'text-text-secondary animate-spin',
  },
  off: {
    label: 'Shell::Sync off',
    icon: IconCloudOff,
    iconClass: 'text-text-dimmed',
  },
  offline: {
    label: 'Shell::Offline',
    icon: IconCloudOff,
    iconClass: 'text-warning',
  },
};

/**
 * The redesigned 48 px top bar, per D9 and
 * `raw/01-shell-topbar-sidebar-statusbar.txt`. Only non-interactive areas are
 * drag regions; interactive controls are excluded from the double-click
 * maximize.
 */
export function TopBar({ auth: propAuth, className }: TopBarProps = {}) {
  const authState = useAuth();
  const library = useLibraryView();
  const storeChecking = useSnapshot((snapshot) =>
    snapshot.storeAccounts.some(
      (account) =>
        account.state === 'checking' || account.state === 'signingIn',
    ),
  ).data;
  const paletteOpen = useUiStore((state) => state.paletteOpen);
  const { t } = useI18n();

  const auth = propAuth !== undefined ? propAuth : authState.auth;
  const isSignedIn =
    typeof auth === 'object' && auth !== null && 'signedIn' in auth;
  const user = isSignedIn ? auth.signedIn.user : null;
  const displayName = user
    ? user.username || user.email || user.id
    : '';

  const syncState = topBarSyncState({
    signedIn: isSignedIn,
    syncEnabled: authState.sync.enabled,
    access: library?.access,
    checking: storeChecking || Boolean(library?.scanning),
  });

  const handleDoubleClick = (event: React.MouseEvent<HTMLElement>) => {
    const target = event.target as HTMLElement | null;
    if (
      target?.closest(
        'button, a, input, textarea, select, [role="button"], [role="menu"], [data-no-drag]',
      )
    ) {
      return;
    }
    toggleWindowMaximize();
  };

  /**
   * Tauri's injected drag script toggles maximize itself on the second
   * mousedown (Windows/Linux) or mouseup (macOS) of a drag region. Stop that
   * event before it reaches the document listener so the explicit
   * `toggleWindowMaximize` in `handleDoubleClick` is the only toggle.
   */
  const handleSecondPress = (event: React.MouseEvent<HTMLElement>) => {
    if (event.detail !== 2) return;
    const target = event.target as HTMLElement | null;
    if (
      target?.closest(
        'button, a, input, textarea, select, [role="button"], [role="menu"], [data-no-drag]',
      )
    ) {
      return;
    }
    event.stopPropagation();
  };

  const sync = syncState ? SYNC_PRESENTATION[syncState] : null;
  const SyncIcon = sync?.icon;

  return (
    <header
      role="banner"
      data-testid="top-bar"
      data-tauri-drag-region="deep"
      onDoubleClick={handleDoubleClick}
      onMouseDown={handleSecondPress}
      onMouseUp={handleSecondPress}
      className={cx(
        'relative z-[100] flex h-topbar w-full select-none items-center border-b border-chrome-border bg-chrome pl-md',
        className,
      )}
    >
      <div
        data-testid="brand-logo"
        className="flex shrink-0 items-center gap-compact"
      >
        <img
          src={wordmark}
          width={62}
          height={24}
          alt="GameNode"
          draggable={false}
          className="block"
        />
        <span
          data-testid="desktop-badge"
          className="rounded-[2px] bg-surface px-[5px] py-[2px] text-[10px] font-bold leading-none tracking-label text-text-dimmed"
        >
          {t('Shell::DESKTOP')}
        </span>
      </div>

      <div className="flex min-w-0 flex-1 items-center justify-center px-md">
        <button
          type="button"
          data-testid="command-search"
          aria-label={t('Shell::Search games, actions and settings')}
          aria-expanded={paletteOpen}
          aria-haspopup="dialog"
          onClick={openPalette}
          className="hidden h-[32px] w-[440px] max-w-full cursor-pointer items-center gap-[8px] rounded-control border border-border-subtle bg-surface px-[10px] text-left transition-colors hover:border-border-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand md:flex"
        >
          <IconSearch
            size={16}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-text-dimmed"
          />
          <span className="min-w-0 flex-1 truncate text-[13px] text-text-dimmed">
            {t('Shell::Search games, actions and settings')}
          </span>
          <KbdCombo keys={['Ctrl', 'K']} />
        </button>
      </div>

      <div className="flex shrink-0 items-center gap-md">
        {sync && SyncIcon && (
          <div
            data-testid="sync-status"
            role="status"
            className="flex h-[16px] items-center gap-[6px]"
          >
            <SyncIcon
              size={16}
              stroke={2}
              aria-hidden="true"
              className={sync.iconClass}
            />
            <span className="whitespace-nowrap text-[12px] text-text-secondary">
              {t(sync.label)}
            </span>
          </div>
        )}

        <AppMenu isSignedIn={isSignedIn} userName={isSignedIn ? displayName : null} />

        <WindowControls />
      </div>
    </header>
  );
}
