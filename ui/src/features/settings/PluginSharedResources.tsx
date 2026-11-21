import React, { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { commands, type PluginSharedResourceReview } from '../../api';
import { useI18n } from '../../i18n';
import { settingsButtonSecondary, settingsRowDescription, settingsRowTitle } from './settingsClasses';

const queryKey = (pluginId: string, publisher: string) =>
  ['plugin-shared-resources', pluginId, publisher] as const;

const reviewKey = (review: PluginSharedResourceReview) => `${review.profileId}:${review.resourceId}`;

/**
 * Reviews the shared emulator resources a plugin declared. The host resolves the backing folder
 * and every affected game; this surface only accepts or forgets the exact resolved scope.
 */
export const PluginSharedResources: React.FC<{ pluginId: string; publisherFingerprint: string }> = ({
  pluginId,
  publisherFingerprint,
}) => {
  const { t } = useI18n();
  const client = useQueryClient();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState(false);
  const { data = [], isPending, isError } = useQuery({
    queryKey: queryKey(pluginId, publisherFingerprint),
    queryFn: () => commands.pluginSharedResourceReviews(pluginId, publisherFingerprint),
  });

  const refresh = async () => client.invalidateQueries({ queryKey: queryKey(pluginId, publisherFingerprint) });
  const accept = async (review: PluginSharedResourceReview) => {
    setBusy(reviewKey(review)); setError(false);
    try {
      await commands.pluginSharedResourceAccept(
        pluginId, publisherFingerprint, review.profileId, review.resourceId,
      );
      await refresh();
    } catch { setError(true); }
    finally { setBusy(null); }
  };
  const revoke = async (review: PluginSharedResourceReview) => {
    setBusy(reviewKey(review)); setError(false);
    try {
      await commands.pluginSharedResourceRevoke(
        pluginId, publisherFingerprint, review.profileId, review.resourceId,
      );
      await refresh();
    } catch { setError(true); }
    finally { setBusy(null); }
  };

  return (
    <section className="mt-3 border-t border-border-subtle pt-3" aria-label={t('plugins.sharedResources.title')}>
      <h4 className={settingsRowTitle}>{t('plugins.sharedResources.title')}</h4>
      <p className={settingsRowDescription}>{t('plugins.sharedResources.disclosure')}</p>
      {isPending && <p className={settingsRowDescription}>{t('plugins.sharedResources.loading')}</p>}
      {(isError || error) && <p role="alert" className="mt-1 text-xs text-danger">{t('plugins.sharedResources.error')}</p>}
      {!isPending && !isError && data.length === 0 && <p className={settingsRowDescription}>{t('plugins.sharedResources.empty')}</p>}
      <ul className="mt-2 flex flex-col gap-2">
        {data.map((review) => {
          const key = reviewKey(review);
          return (
            <li key={key} className="rounded border border-border-subtle bg-primary/40 p-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="text-xs font-semibold">{review.displayName} · {review.profileName}</p>
                <span className="rounded bg-raised px-1.5 py-0.5 text-xs text-text-dimmed">{t(`plugins.sharedResources.consent.${review.consent}`)}</span>
              </div>
              <p className="mt-1 text-xs text-text-dimmed">{t('plugins.sharedResources.runtime')}: {review.runtimeId}</p>
              <p className="mt-1 text-xs">
                {t('plugins.sharedResources.games')}: {review.affectedGames.length
                  ? review.affectedGames.map((game) => game.name).join(', ')
                  : t('plugins.sharedResources.noGames')}
              </p>
              {review.unavailable && <p role="note" className="mt-1 text-xs text-text-dimmed">{t('plugins.sharedResources.unavailable')}</p>}
              <div className="mt-2 flex flex-wrap gap-2">
                <button
                  type="button"
                  className={settingsButtonSecondary}
                  disabled={busy === key || review.unavailable || review.consent === 'current'}
                  onClick={() => void accept(review)}
                >{t('plugins.sharedResources.accept')}</button>
                <button
                  type="button"
                  className={settingsButtonSecondary}
                  disabled={busy === key || review.consent === 'pending'}
                  onClick={() => void revoke(review)}
                >{t('plugins.sharedResources.revoke')}</button>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
};
