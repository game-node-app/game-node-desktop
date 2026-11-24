import React, { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { commands, type PluginFolderApproval } from '../../api';
import { useI18n } from '../../i18n';
import { settingsButtonSecondary, settingsRowDescription, settingsRowTitle } from './settingsClasses';

const queryKey = (pluginId: string, publisher: string) => ['plugin-folder-approvals', pluginId, publisher] as const;

export const PluginFolderApprovals: React.FC<{ pluginId: string; publisherFingerprint: string }> = ({
  pluginId,
  publisherFingerprint,
}) => {
  const { t } = useI18n();
  const client = useQueryClient();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState(false);
  const { data = [], isPending, isError } = useQuery({
    queryKey: queryKey(pluginId, publisherFingerprint),
    queryFn: () => commands.pluginFolderApprovals(pluginId, publisherFingerprint),
  });

  const refresh = async () => client.invalidateQueries({ queryKey: queryKey(pluginId, publisherFingerprint) });
  const revoke = async (approval: PluginFolderApproval) => {
    setBusy(approval.id); setError(false);
    try { await commands.pluginFolderRevoke(pluginId, publisherFingerprint, approval.id); await refresh(); }
    catch { setError(true); }
    finally { setBusy(null); }
  };
  const change = async (approval: PluginFolderApproval) => {
    setBusy(approval.id); setError(false);
    try { await commands.pluginFolderChange(pluginId, publisherFingerprint, approval.id); await refresh(); }
    catch { setError(true); }
    finally { setBusy(null); }
  };
  const setWatching = async (approval: PluginFolderApproval) => {
    setBusy(approval.id); setError(false);
    try {
      await commands.pluginLibraryRootSetWatching(pluginId, publisherFingerprint, approval.id, approval.watching !== true);
      await refresh();
    } catch { setError(true); }
    finally { setBusy(null); }
  };

  return (
    <section className="mt-3 border-t border-border-subtle pt-3" aria-label={t('plugins.folders.title')}>
      <h4 className={settingsRowTitle}>{t('plugins.folders.title')}</h4>
      {isPending && <p className={settingsRowDescription}>{t('plugins.folders.loading')}</p>}
      {(isError || error) && <p role="alert" className="mt-1 text-xs text-danger">{t('plugins.folders.error')}</p>}
      {!isPending && !isError && data.length === 0 && <p className={settingsRowDescription}>{t('plugins.folders.empty')}</p>}
      <ul className="mt-2 flex flex-col gap-2">
        {data.map((approval) => (
          <li key={approval.id} className="rounded border border-border-subtle bg-primary/40 p-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <p className="text-xs font-semibold">{approval.installationName} · {t(`plugins.folder.kind.${approval.kind}`)}</p>
              {approval.revoked && <span className="rounded bg-raised px-1.5 py-0.5 text-xs text-text-dimmed">{t('plugins.folders.revoked')}</span>}
            </div>
            <p className="mt-1 break-all font-mono text-[11px] text-text-dimmed">{approval.root}</p>
            <p className="mt-1 text-xs">{approval.operations.map((operation) => t(`plugins.folder.operation.${operation}`)).join(', ')}</p>
            {approval.declaredByPlugin && <p className="mt-1 text-xs text-text-dimmed">{t('plugins.folder.declaredUnverified')}</p>}
            {approval.logicalLibrary ? (
              <>
                <p className="mt-1 text-xs text-text-dimmed">{t('plugins.folders.rootDisclosure')}</p>
                <div className="mt-2 flex flex-wrap gap-2">
                  <button type="button" className={settingsButtonSecondary} disabled={busy === approval.id} onClick={() => void setWatching(approval)}>{approval.watching ? t('plugins.folders.stopWatching') : t('plugins.folders.startWatching')}</button>
                  <button type="button" className={settingsButtonSecondary} disabled={busy === approval.id} onClick={() => void revoke(approval)}>{t('plugins.folders.revokeRoot')}</button>
                </div>
              </>
            ) : (
              <>
                {approval.affectedGames.length > 0 && <p className="mt-1 text-xs text-text-dimmed">{t('plugins.folder.affectedGames')}: {approval.affectedGames.join(', ')}</p>}
                <div className="mt-2 flex flex-wrap gap-2">
                  {!approval.revoked && <button type="button" className={settingsButtonSecondary} disabled={busy === approval.id} onClick={() => void change(approval)}>{t('plugins.folders.changeFolder')}</button>}
                  {!approval.revoked && <button type="button" className={settingsButtonSecondary} disabled={busy === approval.id} onClick={() => void revoke(approval)}>{t('plugins.folders.revoke')}</button>}
                </div>
              </>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
};
