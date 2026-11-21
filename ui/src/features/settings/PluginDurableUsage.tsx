import React, { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { commands } from '../../api';
import { AppModal } from '../../components/AppModal';
import { pluginQueryKeys } from '../../api/pluginQueryKeys';
import { useI18n } from '../../i18n';
import { formatBytes } from '../ports/catalog/format';
import {
  settingsButtonSecondary,
  settingsRowDescription,
  settingsRowTitle,
} from './settingsClasses';

interface PluginDurableUsageProps {
  pluginId: string;
  profileId: string | null;
}

/**
 * The durable public-pool usage of one producer. The host reports only logical values; this
 * surface never receives or shows a physical path, and cleanup never uninstalls the plugin.
 */
export const PluginDurableUsage: React.FC<PluginDurableUsageProps> = ({
  pluginId,
  profileId,
}) => {
  const { t, locale } = useI18n();
  const client = useQueryClient();
  const queryKey = pluginQueryKeys.durableUsage(pluginId, profileId ?? '');
  const [confirming, setConfirming] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [failed, setFailed] = useState(false);
  const [reclaimed, setReclaimed] = useState<number | null>(null);
  const { data, isPending, isError } = useQuery({
    queryKey,
    queryFn: async () => {
      const usage = await commands.pluginDurableUsage(pluginId, profileId!);
      if (usage.pluginId !== pluginId || usage.profileId !== profileId) {
        throw new Error('plugin durable usage profile changed');
      }
      return usage;
    },
    enabled: profileId !== null,
  });

  const remove = async () => {
    if (!profileId) return;
    setDeleting(true);
    setFailed(false);
    try {
      const result = await commands.pluginDurableDelete(pluginId, profileId, true);
      client.setQueryData(queryKey, result.usage);
      setReclaimed(result.reclaimedBytes);
      setConfirming(false);
    } catch {
      setFailed(true);
      await client.invalidateQueries({ queryKey });
    } finally {
      setDeleting(false);
    }
  };

  const days = data?.maxAgeSeconds
    ? Math.max(1, Math.round(data.maxAgeSeconds / 86_400))
    : null;

  return (
    <section
      className="mt-compact border-t border-border-subtle pt-compact"
      aria-label={t('plugins.durableUsage.title')}
    >
      <h4 className={settingsRowTitle}>{t('plugins.durableUsage.title')}</h4>
      <p className={settingsRowDescription}>
        {t('plugins.durableUsage.description')}
      </p>
      {profileId === null ? (
        <p className={settingsRowDescription} role="status">
          {t('plugins.signInRequired')}
        </p>
      ) : isPending ? (
        <p className={settingsRowDescription} role="status">
          {t('plugins.durableUsage.loading')}
        </p>
      ) : isError || !data ? (
        <p className="mt-1 text-xs text-warning" role="alert">
          {t('plugins.durableUsage.error')}
        </p>
      ) : data.recordCount === 0 ? (
        <p className={settingsRowDescription}>
          {t('plugins.durableUsage.empty')}
        </p>
      ) : (
        <div className="mt-1 flex flex-col gap-1 text-xs">
          <p>
            {t('plugins.durableUsage.records')}: {data.recordCount}
          </p>
          <p>
            {t('plugins.durableUsage.size')}:{' '}
            {formatBytes(data.logicalBytes, locale)}
          </p>
          <p>
            {t('plugins.durableUsage.maxBytes')}:{' '}
            {formatBytes(data.maxBytes, locale)}
          </p>
          <p>
            {days === null
              ? t('plugins.durableUsage.noAge')
              : t('plugins.durableUsage.maxAge', { days })}
          </p>
          <button
            type="button"
            className={`${settingsButtonSecondary} mt-1 self-start border-warning`}
            disabled={deleting}
            onClick={() => {
              setReclaimed(null);
              setConfirming(true);
            }}
          >
            {t('plugins.durableUsage.delete')}
          </button>
        </div>
      )}
      {failed && (
        <p className="mt-1 text-xs text-warning" role="alert">
          {t('plugins.durableUsage.deleteFailed')}
        </p>
      )}
      {reclaimed !== null && (
        <p className="mt-1 text-xs" role="status">
          {t('plugins.durableUsage.deleted', {
            size: formatBytes(reclaimed, locale),
          })}
        </p>
      )}

      <AppModal.Root
        opened={confirming}
        onClose={() => !deleting && setConfirming(false)}
        closeOnEscape={!deleting}
        closeOnClickOutside={!deleting}
      >
        <AppModal.Overlay />
        <AppModal.Content className="max-w-md bg-[var(--settings-bg-base)] p-md">
          <AppModal.Header>
            <AppModal.Title>
              {t('plugins.durableUsage.deleteTitle')}
            </AppModal.Title>
            <AppModal.CloseButton
              aria-label={t('plugins.cancel')}
              disabled={deleting}
            />
          </AppModal.Header>
          <AppModal.Body>
            <p className={settingsRowDescription}>
              {t('plugins.durableUsage.deleteConfirm')}
            </p>
            <div className="flex justify-end gap-compact">
              <button
                type="button"
                className={settingsButtonSecondary}
                disabled={deleting}
                onClick={() => setConfirming(false)}
              >
                {t('plugins.cancel')}
              </button>
              <button
                type="button"
                className="rounded-sm border border-warning px-sm py-1 text-sm"
                disabled={deleting}
                onClick={() => void remove()}
              >
                {deleting
                  ? t('plugins.durableUsage.deleting')
                  : t('plugins.durableUsage.deleteAction')}
              </button>
            </div>
          </AppModal.Body>
        </AppModal.Content>
      </AppModal.Root>
    </section>
  );
};
