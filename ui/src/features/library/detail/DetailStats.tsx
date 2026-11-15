import type { DetailView } from '../../../api';
import { StatCard } from '../../../components/ui';
import { formatSessionTime } from '../../../hooks/useSessionClock';
import { useI18n } from '../../../i18n';
import { formatDuration, formatI18n } from '../formatters';
import { formatClock, formatRelativeDate, playtimeSourceLabel } from './format';

export interface DetailStatsProps {
  detail: DetailView;
  isDrmFree: boolean;
  protectionActive: boolean;
  backupsCount: number;
  lastBackupAt: number | null;
  /** Unix second the current execution began; null when stopped or unknown. */
  sessionStartedAt?: number | null;
  /** Live seconds since the current execution began; null when unknown. */
  sessionElapsedSeconds?: number | null;
  nowSeconds: number;
}

/** The three stat cards: playtime, last session and achievements or save backups. */
export function DetailStats({
  detail,
  isDrmFree,
  protectionActive,
  backupsCount,
  lastBackupAt,
  sessionStartedAt = null,
  sessionElapsedSeconds = null,
  nowSeconds,
}: DetailStatsProps) {
  const { t, locale } = useI18n();
  const row = detail.row;
  const running = detail.running || row.running;
  const summary = detail.achievements;
  const achievementsReady = summary?.state === 'ready';
  const total = summary?.total ?? 0;
  const unlocked = summary?.unlocked ?? 0;
  const progress = total > 0 ? unlocked / total : 0;
  const lastBackup = formatClock(lastBackupAt, locale);
  const showSaves = isDrmFree && protectionActive;
  const startedClock =
    running && sessionStartedAt != null
      ? formatClock(sessionStartedAt, locale)
      : null;

  return (
    <div data-testid="detail-stats" className="flex items-stretch gap-[8px]">
      <StatCard
        label={isDrmFree ? t('Detail::Time') : t('Detail::Playtime')}
        value={formatDuration(row.playtime.seconds, t)}
        hint={playtimeSourceLabel(row.playtime, row.providerName, t)}
      />
      <StatCard
        label={running ? t('Detail::Session') : t('Detail::Last session')}
        value={
          running
            ? sessionElapsedSeconds == null
              ? '—'
              : formatSessionTime(sessionElapsedSeconds)
            : formatRelativeDate(row.playtime.lastPlayedAt, nowSeconds, locale, t)
        }
        hint={
          startedClock
            ? formatI18n(t('Detail::Started {}'), startedClock)
            : undefined
        }
      />
      {showSaves ? (
        <StatCard
          label={t('Detail::Saves')}
          value={formatI18n(t('Detail::{} copies'), backupsCount)}
          hint={
            lastBackup ? formatI18n(t('Detail::Last {}'), lastBackup) : undefined
          }
        />
      ) : (
        <StatCard
          label={t('Detail::Achievements')}
          value={
            achievementsReady
              ? formatI18n(t('Detail::{} of {}'), unlocked, total)
              : '—'
          }
          progress={achievementsReady && total > 0 ? progress : undefined}
        />
      )}
    </div>
  );
}
