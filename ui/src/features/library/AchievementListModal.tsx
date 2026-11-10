import React, { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { SegmentedControl } from '@mantine/core';
import { useVirtualizer } from '@tanstack/react-virtual';
import { IconLock, IconTrophy } from '@tabler/icons-react';
import {
  achievementIconAssetUrl,
  commands,
  type AchievementFilter,
  type AchievementRecord,
} from '../../api';
import { FloatingModal } from '../../components';
import { useI18n } from '../../i18n';

const ROW_HEIGHT = 72;
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

function formatUnlockTime(
  at: number | null | undefined,
  locale: string,
): string | null {
  if (at == null) return null;
  return new Intl.DateTimeFormat(locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(at * 1000));
}

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
    ? formatUnlockTime(achievement.unlockedAt, locale)
    : null;
  const icon = achievementIconAssetUrl(
    !hidden && achievement.unlocked ? achievement.iconUrl : achievement.iconGrayUrl,
  );
  const iconSrc = useIconSrc(icon);
  return (
    <div
      data-testid="achievement-row"
      data-achievement-id={achievement.id}
      className="flex items-center gap-3 rounded-sm bg-raised px-sm py-compact"
    >
      <div className="flex h-10 w-10 shrink-0 items-center justify-center overflow-hidden rounded-xs bg-surface text-text-dimmed">
        {iconSrc.src && !iconSrc.failed ? (
          <img
            src={iconSrc.src}
            alt=""
            loading="lazy"
            decoding="async"
            onError={iconSrc.onError}
            className="h-10 w-10 object-cover"
          />
        ) : hidden ? (
          <IconLock size={18} stroke={1.75} aria-hidden />
        ) : (
          <IconTrophy size={18} stroke={1.75} aria-hidden />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm font-medium text-text-default">{title}</span>
          {achievement.unlocked && (
            <span className="shrink-0 text-[11px] font-medium text-success">
              {t('Library::Unlocked')}
            </span>
          )}
        </div>
        <div className="truncate text-xs text-text-dimmed">{description}</div>
      </div>
      {unlockedAt && (
        <span className="shrink-0 text-[11px] text-text-dimmed">{unlockedAt}</span>
      )}
    </div>
  );
}

export interface AchievementListModalProps {
  opened: boolean;
  onClose: () => void;
  entryKey: string;
  gameTitle: string;
}

export const AchievementListModal: React.FC<AchievementListModalProps> = ({
  opened,
  onClose,
  entryKey,
  gameTitle,
}) => {
  const { t, locale } = useI18n();
  const [filter, setFilter] = useState<AchievementFilter>('all');
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (opened) setFilter('all');
  }, [opened]);

  const query = useQuery({
    queryKey: ['achievement-list', entryKey, filter],
    queryFn: () => commands.achievementList(entryKey, filter),
    enabled: opened,
    staleTime: 0,
  });

  const achievements = query.data?.achievements ?? [];
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

  return (
    <FloatingModal.Root
      opened={opened}
      onClose={onClose}
      geometryKey="achievement-list"
      defaultSize={{ width: 760, height: 620 }}
      minSize={{ width: 420, height: 320 }}
    >
      <FloatingModal.Overlay />
      <FloatingModal.Content
        data-testid="achievement-list-modal"
        aria-label={t('Library::Achievements')}
        className="flex flex-col bg-secondary"
      >
        <FloatingModal.Header>
          <div className="min-w-0">
            <FloatingModal.Title className="truncate">
              {t('Library::Achievements')}
            </FloatingModal.Title>
            <p className="truncate text-xs text-text-dimmed">{gameTitle}</p>
          </div>
          <FloatingModal.CloseButton />
        </FloatingModal.Header>
        <div className="shrink-0 px-control-inline pt-sm">
          <SegmentedControl
            size="xs"
            fullWidth
            value={filter}
            onChange={(value) => setFilter(value as AchievementFilter)}
            data={[
              { value: 'all', label: t('Library::All') },
              { value: 'unlocked', label: t('Library::Unlocked') },
              { value: 'locked', label: t('Library::Locked') },
            ]}
          />
        </div>
        <FloatingModal.Body className="min-h-0">
          <div
            ref={scrollRef}
            data-testid="achievement-list-scroll"
            className="min-h-0 flex-1 overflow-y-auto p-md"
          >
            {query.isLoading ? (
              <div data-testid="achievement-list-loading" className="text-sm text-text-dimmed">
                {t('Library::Loading achievements…')}
              </div>
            ) : query.data?.state === 'none' ? (
              <div data-testid="achievement-list-none" className="text-sm text-text-dimmed">
                {t('Library::This game has no achievements.')}
              </div>
            ) : query.data?.state === 'failed' && query.data.achievements.length === 0 ? (
              <div data-testid="achievement-list-failed" className="text-sm text-warning">
                {query.data.failure === 'privacy'
                  ? t('Library::Steam privacy settings prevent reading these achievements.')
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
                      className="absolute top-0 left-0 w-full pb-2"
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
        </FloatingModal.Body>
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
