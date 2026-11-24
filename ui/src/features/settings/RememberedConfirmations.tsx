import React from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { commands, plainText, CONFIRMATION_GAME_MAX } from '../../api';
import { useI18n } from '../../i18n';
import {
  settingsButtonSecondary,
  settingsRowDescription,
} from './settingsClasses';

const rememberedKey = (pluginId: string, publisher: string) =>
  ['plugin-remembered-confirmations', pluginId, publisher] as const;

/**
 * What the user allowed one plugin to start without asking again (the "don't ask again" choice of
 * the confirmation dialog), with a way to take each one back. The game names come from the
 * plugin's provider and are shown as plain text.
 */
export const RememberedConfirmations: React.FC<{
  pluginId: string;
  publisherFingerprint: string;
}> = ({ pluginId, publisherFingerprint }) => {
  const { t, locale } = useI18n();
  const queryClient = useQueryClient();
  const key = rememberedKey(pluginId, publisherFingerprint);
  const { data = [], isPending, isError } = useQuery({
    queryKey: key,
    queryFn: () => commands.pluginRememberedConfirmations(pluginId, publisherFingerprint),
  });
  const revoke = useMutation({
    mutationFn: (id: string) =>
      commands.pluginRevokeConfirmation(pluginId, publisherFingerprint, id),
    // The host stays authoritative: whatever happened, read the list again.
    onSettled: () => queryClient.invalidateQueries({ queryKey: key }),
  });

  return (
    <div>
      <p className={settingsRowDescription}>{t('plugins.remembered.title')}</p>
      {isPending ? (
        <p className={settingsRowDescription} role="status">
          {t('plugins.loading')}
        </p>
      ) : isError ? (
        <p className={settingsRowDescription} role="alert">
          {t('plugins.remembered.loadFailed')}
        </p>
      ) : data.length === 0 ? (
        <p className={settingsRowDescription}>{t('plugins.remembered.empty')}</p>
      ) : (
        <ul className="mt-1 flex flex-col gap-1" aria-label={t('plugins.remembered.title')}>
          {data.map((item) => {
            const game = plainText(item.gameName, CONFIRMATION_GAME_MAX) || t('plugins.confirm.unnamedGame');
            const what =
              item.operation === 'install'
                ? t('plugins.remembered.install')
                : t('plugins.remembered.launch');
            return (
              <li key={item.id} className="flex items-center justify-between gap-md text-sm text-white">
                <span className="min-w-0 break-words">
                  {what}: {game}
                  <span className="block text-xs text-[var(--settings-text-secondary)]">
                    {new Date(item.approvedAt * 1000).toLocaleDateString(locale)}
                  </span>
                </span>
                <button
                  type="button"
                  className={settingsButtonSecondary}
                  disabled={revoke.isPending}
                  aria-label={t('plugins.remembered.revokeFor', { game })}
                  onClick={() => revoke.mutate(item.id)}
                >
                  {t('plugins.remembered.revoke')}
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {revoke.isError && (
        <p className={`${settingsRowDescription} mt-1`} role="alert">
          {t('plugins.remembered.revokeFailed')}
        </p>
      )}
      <p className={`${settingsRowDescription} mt-1`}>{t('plugins.remembered.note')}</p>
    </div>
  );
};
