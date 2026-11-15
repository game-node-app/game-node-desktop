import { useEffect } from 'react';
import {
  IconAlertTriangle,
  IconCloudOff,
  IconRadar,
  IconWifiOff,
  type TablerIcon,
} from '@tabler/icons-react';
import type {
  Access,
  CatalogError,
  ProviderCount,
  StoreAccountView,
} from '../../api';
import { Banner, type BannerTone } from '../../components/ui';
import {
  dismissBanner,
  openSettingsModal,
  uiStore,
  useUiStore,
} from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { fallbackProviderName } from '../../i18n/providerLabel';
import { commands } from '../../api/commands';
import { formatI18n } from './formatters';

/** Minutes the copy promises before the next automated retry (`raw/07-L5-states.txt`). */
const RETRY_MINUTES = 2;

export type LibraryBannerKind =
  | 'offline'
  | 'expired'
  | 'unreachable'
  | 'discovered';

export interface LibraryBannerDescriptor {
  kind: LibraryBannerKind;
  provider: string | null;
  /** Dismissal key: `kind:provider:problemSince`. */
  key: string;
}

export interface DeriveLibraryBannersInput {
  access: Access;
  storeAccounts: StoreAccountView[];
  recentlyAdded: string[];
}

function storeKey(
  kind: 'expired' | 'unreachable',
  account: StoreAccountView,
): string {
  // Last successful sync identifies one occurrence of the problem: a recovery
  // updates it, so a later failure gets a fresh key. The key is shared by both
  // shapes of the same problem (account state and catalog error), so flipping
  // between them does not bring a dismissed banner back.
  return `${kind}:${account.provider}:${account.lastSyncedAt ?? 0}`;
}

/** Catalog failures that mean the stored credential must be renewed. */
function isExpiredCatalogError(error: CatalogError | null): boolean {
  return (
    error === 'expired' ||
    error === 'invalidCredential' ||
    error === 'invalidCookies'
  );
}

/** Catalog failures that mean the store could not be reached. */
function isUnreachableCatalogError(error: CatalogError | null): boolean {
  return error === 'unreachable' || error === 'server' || error === 'rateLimited';
}

/**
 * The banner problem an account has, if any. A connected account whose catalog
 * sync failed with an expiry or reachability error reports the same problem the
 * equivalent state does, so D13's derivation and its recovery rule agree.
 */
export function storeAccountProblem(
  account: StoreAccountView,
): 'expired' | 'unreachable' | null {
  if (account.state === 'expired' || isExpiredCatalogError(account.catalogError)) {
    return 'expired';
  }
  if (
    account.state === 'unreachable' ||
    isUnreachableCatalogError(account.catalogError)
  ) {
    return 'unreachable';
  }
  return null;
}

/**
 * D13: at most one banner per kind, in the fixed priority order offline,
 * expired, unreachable, discovered. Each account contributes at most one
 * problem; the first one in registry order wins.
 */
export function deriveLibraryBanners({
  access,
  storeAccounts,
  recentlyAdded,
}: DeriveLibraryBannersInput): LibraryBannerDescriptor[] {
  const banners: LibraryBannerDescriptor[] = [];

  if (access === 'offline') {
    banners.push({ kind: 'offline', provider: null, key: 'offline:network:session' });
  }

  const problems = storeAccounts.map((account) => ({
    account,
    problem: storeAccountProblem(account),
  }));

  const expired = problems.find(({ problem }) => problem === 'expired');
  if (expired) {
    banners.push({
      kind: 'expired',
      provider: expired.account.provider,
      key: storeKey('expired', expired.account),
    });
  }

  const unreachable = problems.find(({ problem }) => problem === 'unreachable');
  if (unreachable) {
    banners.push({
      kind: 'unreachable',
      provider: unreachable.account.provider,
      key: storeKey('unreachable', unreachable.account),
    });
  }

  if (recentlyAdded.length > 0) {
    banners.push({
      kind: 'discovered',
      provider: null,
      key: 'discovered:folders:session',
    });
  }

  return banners;
}

const BANNER_STYLE: Record<
  LibraryBannerKind,
  { tone: BannerTone; icon: TablerIcon }
> = {
  offline: { tone: 'neutral', icon: IconWifiOff },
  expired: { tone: 'warning', icon: IconAlertTriangle },
  unreachable: { tone: 'danger', icon: IconCloudOff },
  discovered: { tone: 'brand', icon: IconRadar },
};

function providerName(id: string, providers?: ProviderCount[]): string {
  return (
    providers?.find((provider) => provider.id === id)?.name ??
    fallbackProviderName(id)
  );
}

export interface LibraryBannersProps {
  access: Access;
  storeAccounts: StoreAccountView[];
  recentlyAdded: string[];
  providers?: ProviderCount[];
  className?: string;
}

/**
 * The library's single visible banner, chosen by D13 priority and dismissed for
 * the session by `kind:provider:problemSince`. When a problem recovers, its
 * dismissal is forgotten, so the same problem returning later shows again.
 */
export function LibraryBanners({
  access,
  storeAccounts,
  recentlyAdded,
  providers,
  className,
}: LibraryBannersProps) {
  const { t } = useI18n();
  const dismissed = useUiStore((state) => state.dismissedBanners);
  const banners = deriveLibraryBanners({
    access,
    storeAccounts,
    recentlyAdded,
  });

  // Forget dismissals only after a real recovery: the library is verified
  // again, the account has no active problem under the same interpretation the
  // banner derives, or the candidates were reviewed.
  useEffect(() => {
    const prefixes: string[] = [];
    if (access === 'verified') prefixes.push('offline:');
    if (recentlyAdded.length === 0) prefixes.push('discovered:');
    for (const account of storeAccounts) {
      // A retry in progress is not a recovery: keep the dismissal until the account is
      // genuinely healthy (or disconnected), so a failed retry does not show it again.
      const settled =
        account.state !== 'checking' && account.state !== 'signingIn';
      if (settled && storeAccountProblem(account) == null) {
        prefixes.push(`expired:${account.provider}:`);
        prefixes.push(`unreachable:${account.provider}:`);
      }
    }
    if (prefixes.length === 0) return;

    uiStore.setState((prev) => {
      const next = new Set(
        [...prev.dismissedBanners].filter(
          (key) => !prefixes.some((prefix) => key.startsWith(prefix)),
        ),
      );
      return next.size === prev.dismissedBanners.size
        ? prev
        : { ...prev, dismissedBanners: next };
    });
  }, [access, recentlyAdded, storeAccounts]);

  const banner = banners.find((candidate) => !dismissed.has(candidate.key));
  if (!banner) return null;

  const account =
    banner.provider != null
      ? storeAccounts.find((candidate) => candidate.provider === banner.provider)
      : undefined;
  const name = providerName(banner.provider ?? '', providers);
  const style = BANNER_STYLE[banner.kind];

  const content =
    banner.kind === 'offline'
      ? {
          title: t("Library::You're offline"),
          body: t(
            'Library::Showing the last synced copy. Playing still works.',
          ),
          action: undefined as string | undefined,
        }
      : banner.kind === 'expired'
        ? {
            title: formatI18n(
              t('Library::The {} session expired'),
              name,
            ),
            body: formatI18n(
              t(
                'Library::Sign in again to refresh the {} games from this store.',
              ),
              (account?.ownedCount ?? 0) + (account?.sharedCount ?? 0),
            ),
            action: t('Library::Sign in again'),
          }
        : banner.kind === 'unreachable'
          ? {
              title: formatI18n(t("Library::Couldn't reach {}"), name),
              body: formatI18n(
                t(
                  "Library::We'll try again in {} min. Your games are still available.",
                ),
                RETRY_MINUTES,
              ),
              action: t('Library::Try now'),
            }
          : {
              title:
                recentlyAdded.length === 1
                  ? t('Library::1 new game discovered')
                  : formatI18n(
                      t('Library::{} new games discovered'),
                      recentlyAdded.length,
                    ),
              body: t('Library::Found in watched folders.'),
              action: t('Library::Review'),
            };

  const runAction = () => {
    switch (banner.kind) {
      case 'expired':
        openSettingsModal('stores');
        break;
      case 'unreachable':
        void commands.rescanLibrary();
        break;
      case 'discovered':
        uiStore.setState((state) => ({ ...state, discoveryModalOpen: true }));
        break;
      default:
        break;
    }
  };

  return (
    <div
      data-testid={`library-banner-${banner.kind}`}
      className={
        className
          ? `shrink-0 px-[24px] pb-[14px] ${className}`
          : 'shrink-0 px-[24px] pb-[14px]'
      }
    >
      <Banner
        tone={style.tone}
        icon={style.icon}
        title={content.title}
        body={content.body}
        action={
          content.action ? { label: content.action, onClick: runAction } : undefined
        }
        onDismiss={() => dismissBanner(banner.key)}
        dismissLabel={t('Library::Dismiss')}
      />
    </div>
  );
}
