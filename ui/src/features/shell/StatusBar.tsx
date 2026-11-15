import {
  IconAlertTriangle,
  IconCircleCheck,
  IconFolderSearch,
  IconLoader2,
  IconPlayerPlayFilled,
} from '@tabler/icons-react';
import type { TablerIcon } from '@tabler/icons-react';
import type {
  LibraryView,
  Snapshot,
  StoreAccountView,
} from '../../api/types';
import { useSnapshot } from '../../api/useSnapshot';
import { useI18n } from '../../i18n';
import { providerLabel } from '../../i18n/providerLabel';
import { formatI18n } from '../library/formatters';
import {
  formatSessionTime,
  sessionElapsedSeconds,
  useNowSeconds,
} from '../../hooks/useSessionClock';
import { cx } from '../../components/ui/cx';

export interface StatusBarProps {
  /** Overrides the live snapshot, for tests and embedded previews. */
  snapshot?: Snapshot;
  className?: string;
  /** Unix seconds used as "now"; the 60 s timer advances it. */
  now?: number;
  /**
   * Test override for the running session's elapsed seconds. Production reads
   * the authoritative start from `tracking.sessionStartedAt` (or the row).
   */
  runningSessionSeconds?: number | null;
}

/** The relative-time wording from `copy.md`: just now, N min ago, N h ago. */
export function formatRelativeTime(
  at: number,
  now: number,
  t: (key: string) => string,
): string {
  const elapsed = Math.max(0, now - at);
  if (elapsed < 60) return t('StatusBar::just now');
  if (elapsed < 3600) {
    return formatI18n(t('StatusBar::{} min ago'), Math.floor(elapsed / 60));
  }
  return formatI18n(t('StatusBar::{} h ago'), Math.floor(elapsed / 3600));
}

type StoreTone = 'success' | 'warning' | 'neutral';

interface StoreItem {
  provider: string;
  name: string;
  label: string;
  icon: TablerIcon;
  tone: StoreTone;
  spinning?: boolean;
}

function storeItems(
  accounts: StoreAccountView[],
  library: LibraryView | null | undefined,
  now: number,
  t: (key: string) => string,
): StoreItem[] {
  return accounts
    .filter((account) => account.state !== 'notConnected')
    .map((account) => {
      const manifestName = library?.providers.find(
        (provider) => provider.id === account.provider,
      )?.name;
      const name = providerLabel(account.provider, manifestName, t);
      const unreachable =
        account.state === 'unreachable' ||
        account.catalogError === 'unreachable' ||
        account.catalogError === 'server' ||
        account.catalogError === 'rateLimited';

      if (account.state === 'checking' || account.state === 'signingIn') {
        return {
          provider: account.provider,
          name,
          label: formatI18n(t('StatusBar::{} · checking…'), name),
          icon: IconLoader2,
          tone: 'neutral' as const,
          spinning: true,
        };
      }
      if (
        account.state === 'expired' ||
        account.state === 'credentialError' ||
        account.catalogError === 'expired' ||
        account.catalogError === 'invalidCredential' ||
        account.catalogError === 'invalidCookies'
      ) {
        return {
          provider: account.provider,
          name,
          label: formatI18n(t('StatusBar::{} · session expired'), name),
          icon: IconAlertTriangle,
          tone: 'warning' as const,
        };
      }
      if (unreachable) {
        return {
          provider: account.provider,
          name,
          label: formatI18n(t('StatusBar::{} · unreachable'), name),
          icon: IconAlertTriangle,
          tone: 'warning' as const,
        };
      }
      if (account.lastSyncedAt != null) {
        return {
          provider: account.provider,
          name,
          label: formatI18n(
            t('StatusBar::{} · synced {}'),
            name,
            formatRelativeTime(account.lastSyncedAt, now, t),
          ),
          icon: IconCircleCheck,
          tone: 'success' as const,
        };
      }
      return {
        provider: account.provider,
        name,
        label: formatI18n(t('StatusBar::{} · not synced yet'), name),
        icon: IconCircleCheck,
        tone: 'neutral' as const,
      };
    });
}

const TONE_CLASSES: Record<StoreTone, string> = {
  success: 'text-success',
  warning: 'text-warning',
  neutral: 'text-text-dimmed',
};

/**
 * The 26 px status bar from D12: one icon plus text per connected store, the
 * watched folders' last check, and the running game with its session time.
 * Both relative timestamps and the session clock refresh on one 60 s timer.
 */
export function StatusBar({
  snapshot,
  className,
  now: initialNow,
  runningSessionSeconds,
}: StatusBarProps = {}) {
  const live = useSnapshot();
  const source = snapshot ?? live.data;
  const { t } = useI18n();
  const now = useNowSeconds(60_000, true, initialNow);

  const runningTitle = source.tracking.title.trim();
  const runningRow = source.library?.rows.find(
    (row) => row.running || source.tracking.running.includes(row.key),
  );
  const runningKey =
    source.tracking.running[0] ?? runningRow?.key ?? null;
  const runningName = runningTitle || runningRow?.title || '';
  // The tracker's start is authoritative; the row is the fallback for a
  // snapshot whose tracking map predates this field. Never time from mount.
  const sessionStartedAt =
    (runningKey ? source.tracking.sessionStartedAt?.[runningKey] : null) ??
    runningRow?.sessionStartedAt ??
    null;
  const sessionSeconds =
    runningSessionSeconds ??
    (sessionStartedAt == null
      ? null
      : sessionElapsedSeconds(sessionStartedAt, now));

  const stores = storeItems(source.storeAccounts, source.library, now, t);
  const lastCheckedAt = source.localGames.discoveryLastCheckedAt;
  const discoveryScanning = source.localGames.discoveryScanning;

  return (
    <footer
      data-testid="status-bar"
      className={cx(
        'flex h-statusbar shrink-0 select-none items-center gap-md border-t border-border-subtle bg-chrome px-[12px] text-[11px] leading-none text-text-secondary',
        className,
      )}
    >
      <div className="flex min-w-0 items-center gap-md overflow-hidden">
        {stores.map((store) => {
          const Icon = store.icon;
          return (
            <div
              key={store.provider}
              data-testid={`status-store-${store.provider}`}
              className="flex shrink-0 items-center gap-[6px]"
            >
              <Icon
                size={12}
                stroke={2}
                aria-hidden="true"
                className={cx(TONE_CLASSES[store.tone], store.spinning && 'animate-spin')}
              />
              <span className="whitespace-nowrap">{store.label}</span>
            </div>
          );
        })}

        {(discoveryScanning || lastCheckedAt != null) && (
          <div
            data-testid="status-watched-folders"
            className="flex shrink-0 items-center gap-[6px]"
          >
            {discoveryScanning ? (
              <IconLoader2
                size={12}
                stroke={2}
                aria-hidden="true"
                className="animate-spin text-text-dimmed"
              />
            ) : (
              <IconFolderSearch
                size={12}
                stroke={2}
                aria-hidden="true"
                className="text-text-dimmed"
              />
            )}
            <span className="whitespace-nowrap">
              {discoveryScanning
                ? t('StatusBar::Watched folders · checking…')
                : formatI18n(
                    t('StatusBar::Watched folders · last check {}'),
                    formatRelativeTime(lastCheckedAt as number, now, t),
                  )}
            </span>
          </div>
        )}
      </div>

      <div className="flex min-w-0 flex-1 items-center justify-end">
        {runningKey && runningName && (
          <div
            data-testid="status-running"
            className="flex min-w-0 items-center gap-[6px]"
          >
            <IconPlayerPlayFilled
              size={12}
              aria-hidden="true"
              className="shrink-0 text-success"
            />
            <span className="truncate">
              {formatI18n(
                t('StatusBar::Running: {} · {}'),
                runningName,
                sessionSeconds == null
                  ? t('Library::Unknown')
                  : formatSessionTime(sessionSeconds),
              )}
            </span>
          </div>
        )}
      </div>
    </footer>
  );
}
