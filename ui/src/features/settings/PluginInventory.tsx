import { providerLabel } from '../../i18n/providerLabel';
import React, { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { commands, isTauriRuntime, type AvailablePluginUpdate, type PluginInfo, type PluginUpdateReview } from '../../api';
import { useI18n } from '../../i18n';
import { RememberedConfirmations } from './RememberedConfirmations';
import { PluginFolderApprovals } from './PluginFolderApprovals';
import { selectPluginConfiguration } from '../../api/pluginSettings';
import { PluginConfigurationSurface } from './PluginConfigurationSurface';
import {
  settingsCard,
  settingsPill,
  settingsPillSuccess,
  settingsPillWarning,
  settingsRowDescription,
  settingsRowTitle,
} from './settingsClasses';

const inventoryQueryKey = ['plugin-inventory'] as const;

function statusLabel(status: PluginInfo['status'], t: (key: string) => string): string {
  return t(`plugins.status.${status}`);
}

export const PluginInventory: React.FC = () => {
  const { t } = useI18n();
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const [updates, setUpdates] = useState<AvailablePluginUpdate[]>([]);
  const [review, setReview] = useState<PluginUpdateReview | null>(null);
  const [busy, setBusy] = useState(false);
  const [updateError, setUpdateError] = useState(false);
  const [periodicChecks, setPeriodicChecks] = useState(false);
  const { data: updatePreferences } = useQuery({
    queryKey: ['plugin-update-preferences'],
    queryFn: commands.pluginUpdatePreferences,
  });
  const { data: savedUpdates } = useQuery({
    queryKey: ['plugin-update-available'],
    queryFn: commands.pluginUpdateAvailable,
  });
  const { data = [], isPending, isError } = useQuery({
    queryKey: inventoryQueryKey,
    queryFn: commands.pluginInventory,
    staleTime: 30_000,
  });
  const selectedPlugin = data.find((plugin) => plugin.pluginId === selectedPluginId) ?? data[0];
  const configurationQuery = useQuery({
    queryKey: ['plugin-configuration-modes', selectedPlugin?.providerId],
    queryFn: () => commands.pluginConfigurationModes(selectedPlugin!.providerId),
    enabled: Boolean(selectedPlugin && selectedPlugin.status === 'installed' && selectedPlugin.session === 'active'),
    staleTime: 5_000,
  });
  const configurationModes = configurationQuery.data;
  const configurationChoice = configurationModes ? selectPluginConfiguration(configurationModes) : undefined;

  useEffect(() => {
    if (updatePreferences) setPeriodicChecks(updatePreferences.periodicChecks);
  }, [updatePreferences]);

  useEffect(() => {
    if (savedUpdates) setUpdates(savedUpdates);
  }, [savedUpdates]);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    let unlisten: UnlistenFn | undefined;
    void listen<AvailablePluginUpdate[]>('plugin:update-available', (event) => setUpdates(event.payload))
      .then((dispose) => { unlisten = dispose; });
    return () => { unlisten?.(); };
  }, []);

  const checkUpdates = async () => {
    setBusy(true);
    setUpdateError(false);
    try { setUpdates(await commands.pluginUpdateCheck()); }
    catch { setUpdateError(true); }
    finally { setBusy(false); }
  };

  const reviewUpdate = async (pluginId: string, version: string) => {
    setBusy(true);
    setUpdateError(false);
    setReview(null);
    try { setReview(await commands.pluginUpdateReview(pluginId, version)); }
    catch { setUpdateError(true); }
    finally { setBusy(false); }
  };

  const approveUpdate = async () => {
    if (!review) return;
    setBusy(true);
    setUpdateError(false);
    try {
      await commands.pluginUpdateApprove(review.pluginId, review.candidateVersion);
      setUpdates((current) => current.filter((update) => update.pluginId !== review.pluginId));
      setReview(null);
    } catch { setUpdateError(true); }
    finally { setBusy(false); }
  };

  const declineUpdate = async () => {
    if (!review) return;
    await commands.pluginUpdateDecline(review.pluginId);
    setReview(null);
  };

  const stateClass = (status: PluginInfo['status']) => status === 'installed'
    ? settingsPillSuccess
    : status === 'missing' || status === 'failed' || status === 'incompatible'
      ? settingsPillWarning
      : settingsPill;

  return (
    <section aria-label={t('plugins.inventoryTitle')}>
      <p className={`${settingsRowDescription} mb-compact`}>{t('plugins.inventoryDescription')}</p>
      <div className={`${settingsCard} mb-compact`}>
        <p className={`${settingsRowDescription} mb-compact`}>{t('plugins.updates.description')}</p>
        <label className="flex items-center gap-compact text-sm">
          <input
            type="checkbox"
            checked={periodicChecks}
            onChange={(event) => {
              const enabled = event.currentTarget.checked;
              setPeriodicChecks(enabled);
              void commands.pluginUpdateSetPeriodicChecks(enabled)
                .then((saved) => setPeriodicChecks(saved.periodicChecks))
                .catch(() => { setPeriodicChecks(!enabled); setUpdateError(true); });
            }}
          />
          <span>{t('plugins.updates.optIn')}</span>
        </label>
        <button type="button" className="mt-compact rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]" disabled={busy} onClick={() => void checkUpdates()}>
          {busy ? t('plugins.updates.checking') : t('plugins.updates.checkNow')}
        </button>
        {updates.map((update) => (
          <div key={update.pluginId} className="mt-compact border-t border-border-subtle pt-compact">
            <p className={settingsRowTitle}>{update.name}: {update.installedVersion}</p>
            <div className="flex flex-wrap gap-1">
              {update.versions.map((version) => (
                <button key={version} type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]" disabled={busy} onClick={() => void reviewUpdate(update.pluginId, version)}>
                  {t('plugins.updates.reviewVersion', { version })}
                </button>
              ))}
            </div>
          </div>
        ))}
        {updateError && <p className="mt-compact text-sm text-warning" role="alert">{t('plugins.updates.failed')}</p>}
        {review && (
          <article className="mt-compact border-t border-border-subtle pt-compact" aria-label={t('plugins.updates.reviewTitle')}>
            <h3 className={settingsRowTitle}>{t('plugins.updates.reviewTitle')}: {review.installedVersion} → {review.candidateVersion}</h3>
            <p className={settingsRowDescription}>{t('plugins.grants')}: +{review.addedPermissions.join(', ') || '—'}; {t('plugins.updates.changed')}: {review.changedPermissions.join(', ') || '—'}; −{review.removedPermissions.join(', ') || '—'}</p>
            {review.permissionChanges.map((change) => <p key={change.id} className={settingsRowDescription}>{change.id}: {change.installedScope} → {change.candidateScope}</p>)}
            <p className={settingsRowDescription}>{t('plugins.nativeAccess')}: +{review.addedNativeAccess.join(', ') || '—'}; −{review.removedNativeAccess.join(', ') || '—'}</p>
            <p className={settingsRowDescription}>{t('plugins.updates.api')}: {review.installedApi} → {review.candidateApi}</p>
            <p className={settingsRowDescription}>{t('plugins.updates.license')}: {review.installedLicense} → {review.candidateLicense}</p>
            <p className={settingsRowDescription}>{t('plugins.updates.licenseFiles')}: {review.installedLicenseFiles.join(', ') || '—'} → {review.candidateLicenseFiles.join(', ') || '—'}</p>
            <p className={settingsRowDescription}>{t('plugins.updates.licenseNotices')}: {review.installedLicenseNotices.join('; ') || '—'} → {review.candidateLicenseNotices.join('; ') || '—'}</p>
            <p className={settingsRowDescription}>{t('plugins.updates.publisher')}: {review.installedPublisherKeyId} → {review.candidatePublisherKeyId}</p>
            {review.publisherChanged && <p className={settingsRowDescription}>{t('plugins.updates.publisherChanged')}: {review.installedPublisherKeyId} → {review.candidatePublisherKeyId}</p>}
            <p className={settingsRowDescription}>{t('plugins.updates.newPermissionsDenied')}</p>
            <div className="mt-compact flex gap-compact">
              <button type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]" disabled={busy} onClick={() => void declineUpdate()}>{t('plugins.updates.decline')}</button>
              <button type="button" className="rounded-sm border border-brand bg-brand px-sm py-1 text-sm text-white hover:brightness-110" disabled={busy} onClick={() => void approveUpdate()}>{t('plugins.updates.approve')}</button>
            </div>
          </article>
        )}
      </div>
      {isPending ? (
        <div className={settingsCard}>
          <p className={settingsRowDescription} role="status">{t('plugins.loading')}</p>
        </div>
      ) : isError ? (
        <div className={settingsCard}>
          <p className={settingsRowDescription} role="alert">{t('plugins.loadFailed')}</p>
        </div>
      ) : data.length === 0 ? (
        <div className={settingsCard}>
          <p className={settingsRowDescription}>{t('plugins.empty')}</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-md lg:grid-cols-[minmax(180px,0.8fr)_minmax(0,1.6fr)]">
          <div className={settingsCard} role="group" aria-label={t('plugins.inventoryTitle')}>
            {data.map((plugin) => (
              <button
                key={plugin.pluginId}
                type="button"
                aria-pressed={selectedPlugin?.pluginId === plugin.pluginId}
                className={`flex w-full flex-col items-start gap-1 rounded-sm border px-sm py-compact text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand ${selectedPlugin?.pluginId === plugin.pluginId ? 'border-brand bg-accent-subtle' : 'border-border-subtle bg-raised hover:bg-[#2e2e2e]'}`}
                onClick={() => setSelectedPluginId(plugin.pluginId)}
              >
                <span className={settingsRowTitle}>{providerLabel(plugin.providerId, plugin.name, t)}</span>
                <span className="flex flex-wrap gap-1">
                  <span className={settingsPill}>{plugin.source === 'official' ? t('plugins.official') : t('plugins.community')}</span>
                  <span className={stateClass(plugin.status)}>{statusLabel(plugin.status, t)}</span>
                </span>
              </button>
            ))}
          </div>
          {selectedPlugin && (
            <article className={`${settingsCard} self-start`} aria-label={t('plugins.details')}>
              <div className="flex flex-wrap items-center gap-compact">
                <h3 className={settingsRowTitle}>{providerLabel(selectedPlugin.providerId, selectedPlugin.name, t)}</h3>
                <span className={settingsPill}>{selectedPlugin.source === 'official' ? t('plugins.official') : t('plugins.community')}</span>
                <span className={stateClass(selectedPlugin.status)}>{statusLabel(selectedPlugin.status, t)}</span>
              </div>
              <p className={settingsRowDescription}>{t('plugins.identifier')}: {selectedPlugin.pluginId}</p>
              <p className={settingsRowDescription}>{t('plugins.session')}: {selectedPlugin.session === 'active' ? t('plugins.sessionActive') : t('plugins.sessionStopped')}</p>
              <div>
                <p className={settingsRowDescription}>{t('plugins.grants')}</p>
                <ul className="mt-1 flex flex-wrap gap-1" aria-label={t('plugins.grants')}>
                  {selectedPlugin.capabilities.map((grant) => (
                    <li key={grant} className={settingsPill}>{grant}</li>
                  ))}
                </ul>
              </div>
              <div>
                <p className={settingsRowDescription}>{t('plugins.nativeAccess')}</p>
                <ul className="mt-1 flex flex-wrap gap-1" aria-label={t('plugins.nativeAccess')}>
                  {selectedPlugin.nativeAccess.map((access) => (
                    <li key={access} className={settingsPill}>{t(`plugins.native.${access}`)}</li>
                  ))}
                </ul>
                <p className={`${settingsRowDescription} mt-1`}>{t('plugins.nativeAccessNote')}</p>
              </div>
              <section className="mt-compact" aria-label={t('plugins.settings.title')}>
                <PluginConfigurationSurface
                  plugin={selectedPlugin}
                  choice={configurationChoice}
                  loading={configurationQuery.isPending && !configurationQuery.isError}
                />
              </section>
              {selectedPlugin.publisherFingerprint && (
                <>
                  <RememberedConfirmations
                    pluginId={selectedPlugin.pluginId}
                    publisherFingerprint={selectedPlugin.publisherFingerprint}
                  />
                  <PluginFolderApprovals
                    pluginId={selectedPlugin.pluginId}
                    publisherFingerprint={selectedPlugin.publisherFingerprint}
                  />
                </>
              )}
            </article>
          )}
        </div>
      )}
    </section>
  );
};
