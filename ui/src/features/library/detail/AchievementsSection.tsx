import type { AchievementSummary, DetailView, StoreAccountView } from '../../../api';
import { SectionLabel } from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { formatI18n } from '../formatters';

export interface AchievementsSectionProps {
  detail: DetailView;
  providerAccount: StoreAccountView | undefined;
  onOpen: () => void;
}

function formatUpdatedAt(
  at: number | null | undefined,
  locale: string,
): string | null {
  if (at == null) return null;
  return new Intl.DateTimeFormat(locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(at * 1000));
}

/** The existing detailed achievement summary, with its See all action in the section header. */
export function AchievementsSection({
  detail,
  providerAccount,
  onOpen,
}: AchievementsSectionProps) {
  const { t, locale } = useI18n();
  const summary: AchievementSummary | null = detail.achievements;
  const connected = providerAccount?.state === 'connected';
  const updatedAt = formatUpdatedAt(summary?.lastSuccessAt, locale);
  const latest = summary?.latestUnlock;

  return (
    <section data-testid="detail-achievements" className="flex flex-col gap-[6px]">
      <SectionLabel
        action={
          <button
            type="button"
            data-testid="detail-achievements-open"
            onClick={onOpen}
            className="rounded-xs text-xs font-medium text-brand transition-colors hover:text-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            {t('Detail::See all')}
          </button>
        }
      >
        {t('Detail::Achievements')}
      </SectionLabel>

      <div className="flex flex-col gap-[8px] rounded-control bg-raised px-[12px] py-[8px]">
        {summary?.state === 'ready' ? (
          <>
            <div className="flex items-center justify-between text-sm">
              <span className="text-text-primary">
                {formatI18n(
                  t('Library::{} of {} unlocked'),
                  summary.unlocked,
                  summary.total,
                )}
              </span>
              <span className="text-xs text-text-dimmed">
                {summary.total > 0
                  ? `${Math.round((summary.unlocked / summary.total) * 100)}%`
                  : '—'}
              </span>
            </div>
            <div
              data-testid="detail-achievement-progress"
              role="progressbar"
              aria-label={t('Library::Achievement progress')}
              aria-valuemin={0}
              aria-valuemax={summary.total}
              aria-valuenow={summary.unlocked}
              className="h-1.5 w-full overflow-hidden rounded-full bg-surface"
            >
              <div
                className="h-full rounded-full bg-brand"
                style={{
                  width: `${
                    summary.total > 0
                      ? (summary.unlocked / summary.total) * 100
                      : 0
                  }%`,
                }}
              />
            </div>
            {latest && (
              <div
                data-testid="detail-achievement-latest"
                className="text-xs text-text-dimmed"
              >
                {t('Library::Latest unlock')}:{' '}
                <span className="text-text-default">{latest.name}</span>
                {formatUpdatedAt(latest.unlockedAt, locale) && (
                  <span> · {formatUpdatedAt(latest.unlockedAt, locale)}</span>
                )}
              </div>
            )}
            {summary.failure && (
              <div
                data-testid="detail-achievement-error"
                className="text-xs text-warning"
              >
                {summary.failure === 'privacy'
                  ? t(
                      'Library::{store} privacy settings prevent reading these achievements.',
                      { store: detail.row.providerName },
                    )
                  : t(
                      'Library::Achievements could not be updated. Showing the last saved data.',
                    )}
              </div>
            )}
            {updatedAt && (
              <div
                data-testid="detail-achievement-updated"
                className="text-[11px] text-text-dimmed"
              >
                {formatI18n(t('Library::Updated {}'), updatedAt)}
              </div>
            )}
          </>
        ) : summary?.state === 'none' ? (
          <div data-testid="detail-achievements-none" className="text-sm text-text-dimmed">
            {t('Library::This game has no achievements.')}
          </div>
        ) : summary?.state === 'failed' ? (
          <div data-testid="detail-achievements-failed" className="text-sm text-warning">
            {summary.failure === 'privacy'
              ? t(
                  'Library::{store} privacy settings prevent reading these achievements.',
                  { store: detail.row.providerName },
                )
              : t('Library::Achievements could not be loaded.')}
          </div>
        ) : connected ? (
          <div
            data-testid="detail-achievements-loading"
            className="text-sm text-text-dimmed"
          >
            {t('Library::Loading achievements…')}
          </div>
        ) : providerAccount?.state === 'expired' ? (
          <div
            data-testid="detail-achievements-expired"
            className="text-sm text-text-dimmed"
          >
            {t('Library::Sign in to {store} again to see achievements.', {
              store: detail.row.providerName,
            })}
          </div>
        ) : (
          <div
            data-testid="detail-achievements-not-connected"
            className="text-sm text-text-dimmed"
          >
            {t('Library::Connect {store} to see achievements.', {
              store: detail.row.providerName,
            })}
          </div>
        )}
      </div>
    </section>
  );
}
