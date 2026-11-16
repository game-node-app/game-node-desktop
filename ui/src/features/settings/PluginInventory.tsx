import { providerLabel } from '../../i18n/providerLabel';
import React, { useEffect, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useAuth } from '../../api/useSnapshot';
import { commands, isTauriRuntime, type AvailablePluginUpdate, type PluginInfo, type PluginUpdateReview } from '../../api';
import { pluginQueryKeys } from '../../api/pluginQueryKeys';
import { useI18n } from '../../i18n';
import { RememberedConfirmations } from './RememberedConfirmations';
import {
  settingsCard,
  settingsPill,
  settingsPillSuccess,
  settingsPillWarning,
  settingsRowDescription,
  settingsRowTitle,
} from './settingsClasses';

function statusLabel(status: PluginInfo['status'], t: (key: string) => string): string {
  return t(`plugins.status.${status}`);
}

export const PluginInventory: React.FC = () => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const [review, setReview] = useState<PluginUpdateReview | null>(null);
  const [busy, setBusy] = useState(false);
  const [updateError, setUpdateError] = useState(false);
  const [savingPreferences, setSavingPreferences] = useState(false);
  const updateEventRevision = useRef(0);
  const auth = useAuth().auth;
  const profileId = typeof auth === 'object' && auth !== null && 'signedIn' in auth
    ? auth.signedIn.user.id
    : null;
  const { data: updatePreferences } = useQuery({
    queryKey: pluginQueryKeys.updatePreferences,
    queryFn: commands.pluginUpdatePreferences,
  });
  const { data: updates = [] } = useQuery({
    queryKey: pluginQueryKeys.availableUpdates,
    queryFn: async () => {
      const eventRevisionAtStart = updateEventRevision.current;
      const available = await commands.pluginUpdateAvailable();
      return eventRevisionAtStart === updateEventRevision.current
        ? available
        : queryClient.getQueryData<AvailablePluginUpdate[]>(pluginQueryKeys.availableUpdates) ?? available;
    },
  });
  const { data = [], isPending, isError } = useQuery({
    queryKey: pluginQueryKeys.inventory,
    queryFn: commands.pluginInventory,
    staleTime: 30_000,
  });
  const selectedPlugin = data.find((plugin) => plugin.pluginId === selectedPluginId) ?? data[0];
  const managedPluginId = selectedPlugin?.publisherFingerprint ? selectedPlugin.pluginId : null;
  useQuery({
    queryKey: pluginQueryKeys.permissions(managedPluginId ?? '', profileId ?? ''),
    queryFn: async () => {
      if (!managedPluginId || !profileId) throw new Error('plugin permission profile is unavailable');
      const snapshot = await commands.pluginPermissionSnapshot(managedPluginId);
      if (snapshot.profileId !== profileId) throw new Error('plugin permission profile changed');
      return snapshot;
    },
    enabled: managedPluginId !== null && profileId !== null,
  });
  useQuery({
    queryKey: pluginQueryKeys.configuration(managedPluginId ?? ''),
    queryFn: () => commands.pluginConfigurationState(managedPluginId!),
    enabled: managedPluginId !== null,
  });
  const periodicChecks = updatePreferences?.periodicChecks ?? false;

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    let active = true;
    let unlisten: UnlistenFn | undefined;
    void listen<AvailablePluginUpdate[]>('plugin:update-available', (event) => {
      const eventRevision = ++updateEventRevision.current;
      void queryClient.cancelQueries({ queryKey: pluginQueryKeys.availableUpdates }).then(() => {
        if (eventRevision === updateEventRevision.current) {
          queryClient.setQueryData(pluginQueryKeys.availableUpdates, event.payload);
        }
      });
    }).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [queryClient]);

  const refreshPluginHostState = async (pluginId?: string) => {
    const refreshes = [
      queryClient.invalidateQueries({ queryKey: pluginQueryKeys.inventory }),
      queryClient.invalidateQueries({ queryKey: pluginQueryKeys.availableUpdates }),
    ];
    if (pluginId) {
      if (profileId) {
        refreshes.push(queryClient.invalidateQueries({ queryKey: pluginQueryKeys.permissions(pluginId, profileId) }));
      }
      refreshes.push(queryClient.invalidateQueries({ queryKey: pluginQueryKeys.configuration(pluginId) }));
    }
    await Promise.all(refreshes);
  };

  const checkUpdates = async () => {
    setBusy(true);
    setUpdateError(false);
    const eventRevisionAtStart = updateEventRevision.current;
    try {
      const available = await commands.pluginUpdateCheck();
      if (eventRevisionAtStart === updateEventRevision.current) {
        queryClient.setQueryData(pluginQueryKeys.availableUpdates, available);
      }
    }
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
      setReview(null);
      await refreshPluginHostState(review.pluginId);
    } catch {
      setUpdateError(true);
      await refreshPluginHostState(review.pluginId);
    }
    finally { setBusy(false); }
  };

  const declineUpdate = async () => {
    if (!review) return;
    setBusy(true);
    setUpdateError(false);
    try {
      await commands.pluginUpdateDecline(review.pluginId);
      setReview(null);
    } catch {
      setUpdateError(true);
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.availableUpdates });
    } finally { setBusy(false); }
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
            disabled={savingPreferences || !updatePreferences}
            onChange={(event) => {
              const enabled = event.currentTarget.checked;
              setSavingPreferences(true);
              setUpdateError(false);
              void commands.pluginUpdateSetPeriodicChecks(enabled)
                .then((saved) => {
                  queryClient.setQueryData(pluginQueryKeys.updatePreferences, saved);
                })
                .catch(() => {
                  setUpdateError(true);
                  void queryClient.invalidateQueries({ queryKey: pluginQueryKeys.updatePreferences });
                })
                .finally(() => setSavingPreferences(false));
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
              {selectedPlugin.publisherFingerprint && (
                <RememberedConfirmations
                  pluginId={selectedPlugin.pluginId}
                  publisherFingerprint={selectedPlugin.publisherFingerprint}
                />
              )}
            </article>
          )}
        </div>
      )}
    </section>
  );
};
