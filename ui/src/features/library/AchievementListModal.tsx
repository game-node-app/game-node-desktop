import React, { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Menu } from '@mantine/core';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  IconArrowsSort,
  IconCheck,
  IconChevronDown,
  IconClock,
  IconExternalLink,
  IconLock,
  IconTrophy,
} from '@tabler/icons-react';
import {
  achievementIconAssetUrl,
  commands,
  type AchievementFilter,
  type AchievementRecord,
  type AchievementSort,
} from '../../api';
import { DialogShell } from '../../components/dialog';
import {
  Button,
  InsetCard,
  MenuSurface,
  ProgressBar,
  SegmentedControl,
} from '../../components/ui';
import { useI18n } from '../../i18n';
import { formatI18n } from '../library/formatters';

/** Fixed row height from `components.md` §9. */
const ROW_HEIGHT = 65;
/**
 * The host downloads an icon after returning the list, so the first request for a fresh icon is
 * a 404. Retry the stable route for a bounded window; each attempt busts the 404 cache. A game
 * with hundreds of achievements downloads its icons concurrently, so the window is generous.
 */
const ICON_RETRY_DELAYS_MS = [750, 1_500, 3_000, 6_000, 12_000, 20_000, 30_000, 30_000];

function useIconSrc(src: string | null): {
  src: string | null;
  failed: boolean;
  onError: () => void;
} {
  const [attempt, setAttempt] = useState(0);
  const [failed, setFailed] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    setAttempt(0);
    setFailed(false);
    return () => window.clearTimeout(timer.current);
  }, [src]);
  const onError = () => {
    if (!src) return;
    // Keep the placeholder instead of the browser's broken-image glyph while retrying.
    setFailed(true);
    if (attempt >= ICON_RETRY_DELAYS_MS.length) return;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      setFailed(false);
      setAttempt((value) => value + 1);
    }, ICON_RETRY_DELAYS_MS[attempt]);
  };
  const resolved =
    src && attempt > 0
      ? `${src}${src.includes('?') ? '&' : '?'}retry=${attempt}`
      : src;
  return { src: resolved, failed: failed && resolved !== null, onError };
}

/** A hundredths-of-a-percent value as a locale-formatted percentage without the sign. */
function formatPercent(hundredths: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    maximumFractionDigits: 2,
  }).format(hundredths / 100);
}

function formatUnlockDate(
  at: number | null | undefined,
  locale: string,
): string | null {
  if (at == null) return null;
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium' }).format(
    new Date(at * 1000),
  );
}

/** "5 minutes ago" / "há 5 minutos", for the footer's last refresh. */
function formatRelativeTime(
  at: number | null | undefined,
  locale: string,
  nowSeconds: number,
): string | null {
  if (at == null) return null;
  const elapsed = Math.max(0, nowSeconds - at);
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  if (elapsed < 60) return formatter.format(0, 'second');
  if (elapsed < 3_600) return formatter.format(-Math.floor(elapsed / 60), 'minute');
  if (elapsed < 86_400) return formatter.format(-Math.floor(elapsed / 3_600), 'hour');
  if (elapsed < 30 * 86_400) return formatter.format(-Math.floor(elapsed / 86_400), 'day');
  if (elapsed < 365 * 86_400) {
    return formatter.format(-Math.floor(elapsed / (30 * 86_400)), 'month');
  }
  return formatter.format(-Math.floor(elapsed / (365 * 86_400)), 'year');
}

const SORT_KEYS: Record<AchievementSort, string> = {
  mostRecent: 'Library::Most recent',
  rarest: 'Library::Rarest',
  mostCommon: 'Library::Most common',
  name: 'Library::Name',
};

function AchievementRow({
  achievement,
  locale,
}: {
  achievement: AchievementRecord;
  locale: string;
}) {
  const { t } = useI18n();
  const hidden = achievement.hidden && !achievement.unlocked;
  const title = hidden ? t('Library::Hidden achievement') : achievement.name;
  const description = hidden
    ? t('Library::Unlock this achievement to reveal it.')
    : achievement.description;
  const unlockedAt = achievement.unlocked
    ? formatUnlockDate(achievement.unlockedAt, locale)
    : null;
  const icon = achievementIconAssetUrl(
    !hidden && achievement.unlocked ? achievement.iconUrl : achievement.iconGrayUrl,
  );
  const iconSrc = useIconSrc(icon);
  const locked = !achievement.unlocked;
  const rarity =
    achievement.globalPercent != null
      ? achievement.globalPercent < 1_000
        ? t('Library::Rare · {percent}% of players', {
            percent: formatPercent(achievement.globalPercent, locale),
          })
        : t('Library::{percent}% of players', {
            percent: formatPercent(achievement.globalPercent, locale),
          })
      : null;
  return (
    <div
      data-testid="achievement-row"
      data-achievement-id={achievement.id}
      className="flex h-[65px] items-center gap-3 border-b border-border-subtle"
    >
      <div className="flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-md bg-raised text-text-muted">
        {iconSrc.src && !iconSrc.failed ? (
          <img
            src={iconSrc.src}
            alt=""
            loading="lazy"
            decoding="async"
            onError={iconSrc.onError}
            className="h-11 w-11 object-cover"
          />
        ) : hidden || locked ? (
          <IconLock size={18} stroke={1.75} aria-hidden />
        ) : (
          <IconTrophy size={18} stroke={1.75} aria-hidden />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div
          className={`truncate text-[13px] font-semibold ${
            locked ? 'text-text-secondary' : 'text-text-primary'
          }`}
        >
          {title}
        </div>
        <div className="line-clamp-2 max-w-[340px] text-xs font-normal text-text-dimmed">
          {description}
        </div>
      </div>
      <div className="ml-auto flex shrink-0 flex-col items-end gap-[2px] text-right">
        <span className="text-xs text-text-default">
          {achievement.unlocked
            ? (unlockedAt ?? '')
            : t('Library::Locked achievement')}
        </span>
        {rarity && (
          <span className="text-[11px] text-text-dimmed">{rarity}</span>
        )}
      </div>
    </div>
  );
}

export interface AchievementListModalProps {
  opened: boolean;
  onClose: () => void;
  entryKey: string;
  gameTitle: string;
  /** The entry provider's display name, used by provider-named status messages. */
  providerName: string;
}

export const AchievementListModal: React.FC<AchievementListModalProps> = ({
  opened,
  onClose,
  entryKey,
  gameTitle,
  providerName,
}) => {
  const { t, locale } = useI18n();
  const [filter, setFilter] = useState<AchievementFilter>('all');
  const [sort, setSort] = useState<AchievementSort>('mostRecent');
  const [pageError, setPageError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (opened) {
      setFilter('all');
      setSort('mostRecent');
      setPageError(null);
    }
  }, [opened]);

  const query = useQuery({
    queryKey: ['achievement-list', entryKey, filter, sort],
    queryFn: () => commands.achievementList(entryKey, filter, sort),
    enabled: opened,
    staleTime: 0,
  });

  const view = query.data;
  const achievements = view?.achievements ?? [];
  const counts = view?.counts ?? { all: 0, unlocked: 0, locked: 0 };
  const percent =
    counts.all > 0 ? Math.round((counts.unlocked / counts.all) * 100) : 0;
  const hasRarity = view?.hasRarity ?? false;
  const hasProviderPage = view?.hasProviderPage ?? false;
  const resolvedProvider = view?.providerName ?? providerName;
  const refreshedAt = view?.refreshedAt ?? view?.lastSuccessAt ?? null;
  const refreshed = formatRelativeTime(
    refreshedAt,
    locale,
    Math.floor(Date.now() / 1000),
  );
  const sorts: AchievementSort[] = hasRarity
    ? ['mostRecent', 'rarest', 'mostCommon', 'name']
    : ['mostRecent', 'name'];

  const virtualizer = useVirtualizer({
    count: achievements.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 4,
    initialRect: { width: 640, height: 480 },
  });
  const items = virtualizer.getVirtualItems();
  useEffect(() => {
    if (opened) virtualizer.measure();
  }, [opened, achievements.length, virtualizer]);

  if (!opened) return null;

  const openPage = () => {
    setPageError(null);
    void commands.openAchievementsPage(entryKey).catch(() => {
      setPageError(t('Library::Could not open the achievements page.'));
    });
  };

  const footerNote =
    refreshed != null || pageError != null ? (
      <>
        {refreshed != null && (
          <>
            <IconClock size={14} aria-hidden />
            <span className="truncate">
              {t('Library::Updated from {provider} {relative}', {
                provider: resolvedProvider,
                relative: refreshed,
              })}
            </span>
          </>
        )}
        {pageError != null && (
          <span
            data-testid="achievement-page-error"
            className="text-danger"
            role="status"
          >
            {pageError}
          </span>
        )}
      </>
    ) : undefined;

  return (
    <DialogShell
      title={t('Library::Achievements')}
      subtitle={
        <>
          <span>{gameTitle}</span>
          <span>{` · ${resolvedProvider}`}</span>
        </>
      }
      geometryKey="achievement-list"
      defaultSize={{ width: 640, height: 600 }}
      minSize={{ width: 480, height: 360 }}
      testId="achievement-list-modal"
      onClose={onClose}
      className="bg-secondary"
      bodyClassName="min-h-0 gap-md p-lg"
      footerNote={footerNote}
      footer={
        hasProviderPage ? (
          <Button
            size="sm"
            variant="secondary"
            rightIcon={IconExternalLink}
            onClick={openPage}
          >
            {t('Library::Open in {provider}', { provider: resolvedProvider })}
          </Button>
        ) : undefined
      }
    >
      {view != null && (
        <InsetCard
          icon={IconTrophy}
          iconSize={24}
          iconClassName="text-warning"
          title={formatI18n(
            t('Library::{} of {} unlocked'),
            counts.unlocked,
            counts.all,
          )}
          trailing={
            <span className="text-[13px] font-semibold text-brand">
              {percent}%
            </span>
          }
        >
          <ProgressBar
            value={counts.unlocked}
            max={counts.all}
            label={t('Library::Achievement progress')}
          />
        </InsetCard>
      )}

      <div className="flex items-center gap-2">
        <SegmentedControl
          value={filter}
          onChange={(value) => setFilter(value as AchievementFilter)}
          data={[
            {
              value: 'all',
              label: `${t('Library::All achievements')} · ${counts.all}`,
            },
            {
              value: 'unlocked',
              label: `${t('Library::Unlocked')} · ${counts.unlocked}`,
            },
            {
              value: 'locked',
              label: `${t('Library::Locked')} · ${counts.locked}`,
            },
          ]}
        />
        <div className="flex-1" />
        <MenuSurface
          position="bottom-end"
          width={180}
          withinPortal={false}
          hideDetached={false}
          transitionProps={{ duration: 0 }}
        >
          <Menu.Target>
            <button
              type="button"
              aria-label={t('Library::Sort')}
              className="flex h-[32px] items-center gap-[6px] rounded-control border border-border-subtle bg-raised px-[10px] text-xs font-normal text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
            >
              <IconArrowsSort
                size={14}
                stroke={2}
                aria-hidden
                className="text-text-secondary"
              />
              <span>{t(SORT_KEYS[sort])}</span>
              <IconChevronDown
                size={12}
                stroke={2}
                aria-hidden
                className="text-text-dimmed"
              />
            </button>
          </Menu.Target>
          <Menu.Dropdown>
            {sorts.map((value) => (
              <Menu.Item
                key={value}
                onClick={() => setSort(value)}
                leftSection={
                  sort === value ? <IconCheck size={14} aria-hidden /> : undefined
                }
              >
                {t(SORT_KEYS[value])}
              </Menu.Item>
            ))}
          </Menu.Dropdown>
        </MenuSurface>
      </div>

      <div
        ref={scrollRef}
        data-testid="achievement-list-scroll"
        className="min-h-0 flex-1 overflow-y-auto"
      >
        {query.isLoading ? (
          <div data-testid="achievement-list-loading" className="text-sm text-text-dimmed">
            {t('Library::Loading achievements…')}
          </div>
        ) : view?.state === 'none' ? (
          <div data-testid="achievement-list-none" className="text-sm text-text-dimmed">
            {t('Library::This game has no achievements.')}
          </div>
        ) : view?.state === 'failed' && achievements.length === 0 ? (
          <div data-testid="achievement-list-failed" className="text-sm text-warning">
            {view.failure === 'privacy'
              ? t(
                  'Library::{store} privacy settings prevent reading these achievements.',
                  { store: providerName },
                )
              : t('Library::Achievements could not be loaded.')}
          </div>
        ) : achievements.length === 0 ? (
          <div data-testid="achievement-list-empty" className="text-sm text-text-dimmed">
            {t('Library::No achievements match this filter.')}
          </div>
        ) : (
          <div
            data-testid="achievement-list"
            role="list"
            className="relative"
            style={{ height: `${virtualizer.getTotalSize()}px` }}
          >
            {items.map((item) => {
              const achievement = achievements[item.index];
              return (
                <div
                  key={achievement.id}
                  role="listitem"
                  className="absolute top-0 left-0 w-full"
                  style={{
                    height: `${item.size}px`,
                    transform: `translateY(${item.start}px)`,
                  }}
                >
                  <AchievementRow achievement={achievement} locale={locale} />
                </div>
              );
            })}
          </div>
        )}
      </div>
    </DialogShell>
  );
};
