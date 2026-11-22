import React, { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { commands, type PluginRuntimeApproval } from '../../api';
import { useI18n } from '../../i18n';
import { settingsButtonSecondary, settingsRowDescription, settingsRowTitle } from './settingsClasses';

const queryKey = (pluginId: string, publisher: string) => ['plugin-runtime-approvals', pluginId, publisher] as const;

export const PluginRuntimeApprovals: React.FC<{ pluginId: string; publisherFingerprint: string }> = ({
  pluginId,
  publisherFingerprint,
}) => {
  const { t, locale } = useI18n();
  const client = useQueryClient();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState(false);
  const { data = [], isPending, isError } = useQuery({
    queryKey: queryKey(pluginId, publisherFingerprint),
    queryFn: () => commands.pluginRuntimeApprovals(pluginId, publisherFingerprint),
  });

  const refresh = async () => client.invalidateQueries({ queryKey: queryKey(pluginId, publisherFingerprint) });
  const revoke = async (approval: PluginRuntimeApproval) => {
    setBusy(approval.runtimeId); setError(false);
    try {
      await commands.pluginRuntimeRevoke(pluginId, publisherFingerprint, approval.runtimeId);
      await refresh();
    } catch { setError(true); }
    finally { setBusy(null); }
  };

  return (
    <section className="mt-3 border-t border-border-subtle pt-3" aria-label={t('plugins.runtimeApprovals.title')}>
      <h4 className={settingsRowTitle}>{t('plugins.runtimeApprovals.title')}</h4>
      <p className={`${settingsRowDescription} mt-1`}>{t('plugins.runtimeApprovals.disclosure')}</p>
      {isPending && <p className={settingsRowDescription}>{t('plugins.runtimeApprovals.loading')}</p>}
      {(isError || error) && <p role="alert" className="mt-1 text-xs text-danger">{t('plugins.runtimeApprovals.error')}</p>}
      {!isPending && !isError && data.length === 0 && <p className={settingsRowDescription}>{t('plugins.runtimeApprovals.empty')}</p>}
      <ul className="mt-2 flex flex-col gap-2">
        {data.map((approval) => (
          <li key={approval.runtimeId} className="rounded border border-border-subtle bg-primary/40 p-2">
            <p className="break-all font-mono text-[11px] text-text-dimmed">{approval.path}</p>
            <p className="mt-1 text-xs">
              {t('plugins.runtimeApprovals.approvedAt', {
                date: new Intl.DateTimeFormat(locale, { dateStyle: 'medium' })
                  .format(new Date(approval.approvedAt * 1000)),
              })}
            </p>
            <div className="mt-2 flex flex-wrap gap-2">
              <button
                type="button"
                className={settingsButtonSecondary}
                aria-label={t('plugins.runtimeApprovals.removeFor', { path: approval.path })}
                disabled={busy === approval.runtimeId}
                onClick={() => void revoke(approval)}
              >
                {busy === approval.runtimeId
                  ? t('plugins.runtimeApprovals.removing')
                  : t('plugins.runtimeApprovals.remove')}
              </button>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
};
