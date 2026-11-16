import { providerLabel } from '../../i18n/providerLabel';
import React, { useEffect, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useAuth } from '../../api/useSnapshot';
import { commands, isTauriRuntime, type AvailablePluginUpdate, type PluginImportReview, type PluginInfo, type PluginPermissionSnapshot, type PluginUpdateReview } from '../../api';
import { AppModal } from '../../components/AppModal';
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

function scopeChoiceKey(pluginId: string, profileId: string, permissionId: string, scope: unknown): string {
  return `${pluginId}:${profileId}:${permissionId}:${JSON.stringify(scope)}`;
}

export const PluginInventory: React.FC = () => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const [review, setReview] = useState<PluginUpdateReview | null>(null);
  const [importReview, setImportReview] = useState<PluginImportReview | null>(null);
  const [archivePath, setArchivePath] = useState('');
  const [scopeChoices, setScopeChoices] = useState<Record<string, boolean>>({});
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [busy, setBusy] = useState(false);
  const [updateError, setUpdateError] = useState(false);
  const [managementError, setManagementError] = useState(false);
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
  const { data: permissionSnapshot } = useQuery({
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
  const managed = Boolean(selectedPlugin?.publisherFingerprint);
  const canManageRuntime = managed && selectedPlugin?.runtimeAvailable === true && profileId !== null;
  const canManageAccount = managed && profileId !== null;

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

  const reviewImport = async () => {
    setBusy(true);
    setManagementError(false);
    setImportReview(null);
    try { setImportReview(await commands.pluginImportReview(archivePath.trim())); }
    catch { setManagementError(true); }
    finally { setBusy(false); }
  };

  const installImport = async () => {
    if (!importReview) return;
    setBusy(true);
    setManagementError(false);
    try {
      await commands.pluginImportInstall(importReview.token);
      setImportReview(null);
      setArchivePath('');
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.inventory });
    } catch { setManagementError(true); }
    finally { setBusy(false); }
  };

  const savePermissions = async (snapshot: PluginPermissionSnapshot) => {
    if (!profileId || snapshot.profileId !== profileId) return;
    const decisions = snapshot.requestedPermissions.flatMap((request) => request.allowed_scopes.map((scope) => {
      const key = scopeChoiceKey(snapshot.pluginId, profileId, request.permission_id, scope);
      const persisted = snapshot.effectiveGrants.some((grant) => grant.permission_id === request.permission_id && JSON.stringify(grant.scope) === JSON.stringify(scope) && grant.state === 'granted');
      return { permission_id: request.permission_id, scope, granted: scopeChoices[key] ?? persisted };
    }));
    setBusy(true);
    setUpdateError(false);
    try {
      const saved = await commands.pluginPermissionsReplace(snapshot.pluginId, profileId, snapshot.grantRevision, decisions);
      setScopeChoices({});
      queryClient.setQueryData(pluginQueryKeys.permissions(snapshot.pluginId, profileId), saved);
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.inventory });
    } catch {
      setManagementError(true);
      setScopeChoices({});
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.permissions(snapshot.pluginId, profileId) });
    } finally { setBusy(false); }
  };

  const setPluginEnabled = async (pluginId: string, enabled: boolean) => {
    setBusy(true);
    setManagementError(false);
    try {
      if (enabled) await commands.pluginEnable(pluginId);
      else await commands.pluginDisable(pluginId);
      await refreshPluginHostState(pluginId);
    } catch {
      setManagementError(true);
      await refreshPluginHostState(pluginId);
    } finally { setBusy(false); }
  };

  const removePlugin = async () => {
    if (!selectedPlugin) return;
    setBusy(true);
    setManagementError(false);
    try {
      await commands.pluginRemove(selectedPlugin.pluginId);
      setConfirmRemove(false);
      setSelectedPluginId(null);
      await refreshPluginHostState(selectedPlugin.pluginId);
    } catch {
      setManagementError(true);
      await refreshPluginHostState(selectedPlugin.pluginId);
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
        <form aria-label={t('plugins.import.title')} onSubmit={(event) => { event.preventDefault(); void reviewImport(); }}>
          <label className="block text-sm">
            <span className={settingsRowTitle}>{t('plugins.import.title')}</span>
            <input className="mt-1 w-full rounded-sm border border-border-subtle bg-raised px-sm py-1" value={archivePath} onChange={(event) => setArchivePath(event.currentTarget.value)} placeholder={t('plugins.import.path')} aria-label={t('plugins.import.path')} />
          </label>
          {profileId === null && <p className={`${settingsRowDescription} mt-compact`} role="status">{t('plugins.signInRequired')}</p>}
          <button type="submit" className="mt-compact rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]" disabled={busy || !archivePath.trim() || profileId === null}>{t('plugins.import.review')}</button>
        </form>
        {importReview && <article className="mt-compact border-t border-border-subtle pt-compact" aria-label={t('plugins.import.reviewTitle')}>
          <h3 className={settingsRowTitle}>{importReview.name} {importReview.version}</h3>
          <p className={settingsRowDescription}>{t('plugins.identifier')}: {importReview.pluginId}</p>
          <p className={settingsRowDescription}>{t('plugins.import.fingerprint')}: {importReview.publisherFingerprint}</p>
          {importReview.pinnedFingerprint && <p className={settingsRowDescription}>{t('plugins.import.previousFingerprint')}: {importReview.pinnedFingerprint}</p>}
          <div className={settingsRowDescription}>
            <p>{t('plugins.import.permissions')}:</p>
            {importReview.requestedPermissions.length ? (
              <ul className="ml-4 list-disc">
                {importReview.requestedPermissions.map((permission) => (
                  <li key={permission.id}>
                    {permission.id} · {permission.scope} · {permission.required ? t('plugins.import.required') : t('plugins.import.optional')} · {permission.reason}
                  </li>
                ))}
              </ul>
            ) : <p>—</p>}
          </div>
          <p className={settingsRowDescription}>{t('plugins.nativeAccess')}: {importReview.nativeAccess.join(', ') || '—'}</p>
          <p className={settingsRowDescription}>{t('plugins.import.license')}: {importReview.license}; {importReview.licenseFiles.join(', ') || '—'}</p>
          <p className={settingsRowDescription}>{t('plugins.import.compatibility')}: {t(`plugins.import.${importReview.compatibility}`)} {importReview.compatibilityReason ?? ''}</p>
          {importReview.trust !== 'trusted' ? <button type="button" className="mt-compact rounded-sm border border-border-subtle px-sm py-1 text-sm" disabled={busy || profileId === null || importReview.compatibility !== 'compatible'} onClick={() => {
            setBusy(true);
            setManagementError(false);
            void commands.pluginImportConfirmPublisher(importReview.token, importReview.publisherFingerprint).then(setImportReview).catch(() => setManagementError(true)).finally(() => setBusy(false));
          }}>{importReview.trust === 'changed' ? t('plugins.import.retrust') : t('plugins.import.trust')}</button> : null}
          <button type="button" className="ml-compact mt-compact rounded-sm border border-brand bg-brand px-sm py-1 text-sm text-white" disabled={busy || profileId === null || importReview.trust !== 'trusted' || importReview.compatibility !== 'compatible'} onClick={() => void installImport()}>{t('plugins.import.install')}</button>
        </article>}
      </div>
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
        {(updateError || managementError) && <p className="mt-compact text-sm text-warning" role="alert">{t(managementError ? 'plugins.management.failed' : 'plugins.updates.failed')}</p>}
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
                onClick={() => {
                  setScopeChoices({});
                  setManagementError(false);
                  setSelectedPluginId(plugin.pluginId);
                }}
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
              {managed && selectedPlugin.version && <p className={settingsRowDescription}>{t('plugins.import.version')}: {selectedPlugin.version}</p>}
              {managed && selectedPlugin.runtimeAvailable === false && <p className={settingsRowDescription} role="status">{t('plugins.import.restartRequired')}</p>}
              {managed && profileId === null && <p className={settingsRowDescription} role="status">{t('plugins.signInRequired')}</p>}
              {(canManageRuntime || canManageAccount) && <div className="mt-compact flex flex-wrap gap-compact">
                {canManageRuntime && (selectedPlugin.enabled ? <button type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm" disabled={busy} onClick={() => void setPluginEnabled(selectedPlugin.pluginId, false)}>{t('plugins.disable')}</button> : <button type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm" disabled={busy || selectedPlugin.status === 'incompatible'} onClick={() => void setPluginEnabled(selectedPlugin.pluginId, true)}>{t('plugins.enable')}</button>)}
                {canManageAccount && <button type="button" className="rounded-sm border border-warning px-sm py-1 text-sm" disabled={busy} onClick={() => setConfirmRemove(true)}>{t('plugins.remove')}</button>}
              </div>}
              {managed && profileId && permissionSnapshot && permissionSnapshot.pluginId === selectedPlugin.pluginId && <div className="mt-compact border-t border-border-subtle pt-compact" aria-label={t('plugins.permissionScopes')}>
                <p className={settingsRowTitle}>{t('plugins.permissionScopes')}</p>
                {permissionSnapshot.requestedPermissions.map((request) => <fieldset key={request.permission_id} className="mt-compact">
                  <legend className={settingsRowDescription}>{request.permission_id}{request.required ? ` · ${t('plugins.required')}` : ''}</legend>
                  {request.allowed_scopes.map((scope) => {
                    const key = scopeChoiceKey(permissionSnapshot.pluginId, profileId!, request.permission_id, scope);
                    const active = permissionSnapshot.effectiveGrants.some((grant) => grant.permission_id === request.permission_id && JSON.stringify(grant.scope) === JSON.stringify(scope) && grant.state === 'granted');
                    return <label key={key} className="flex items-center gap-compact py-1 text-sm">
                      <input type="checkbox" disabled={!canManageRuntime || busy} checked={scopeChoices[key] ?? active} onChange={(event) => {
                        const checked = event.currentTarget.checked;
                        setScopeChoices((current) => ({ ...current, [key]: checked }));
                      }} />
                      <span>{scope.scope}{'category' in scope ? ` · ${scope.category}` : 'value' in scope ? ` · ${scope.value}` : ''}</span>
                    </label>;
                  })}
                </fieldset>)}
                <button type="button" className="mt-compact rounded-sm border border-border-subtle px-sm py-1 text-sm" disabled={!canManageRuntime || busy} onClick={() => void savePermissions(permissionSnapshot)}>{t('plugins.permissions.save')}</button>
              </div>}
              <div>
                <p className={settingsRowDescription}>{selectedPlugin.source === 'official' ? t('plugins.grants') : t('plugins.import.permissions')}</p>
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
      <AppModal.Root opened={confirmRemove} onClose={() => setConfirmRemove(false)}>
        <AppModal.Overlay />
        <AppModal.Content className="max-w-md bg-[var(--settings-bg-base)] p-md">
          <AppModal.Header>
            <AppModal.Title>{t('plugins.remove')}</AppModal.Title>
            <AppModal.CloseButton aria-label={t('plugins.cancel')} />
          </AppModal.Header>
          <AppModal.Body>
            <p className={settingsRowDescription}>{t('plugins.removeConfirm')}</p>
            <div className="flex justify-end gap-compact">
              <button type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm" onClick={() => setConfirmRemove(false)}>{t('plugins.cancel')}</button>
              <button type="button" className="rounded-sm border border-warning px-sm py-1 text-sm" disabled={busy} onClick={() => void removePlugin()}>{t('plugins.remove')}</button>
            </div>
          </AppModal.Body>
        </AppModal.Content>
      </AppModal.Root>
    </section>
  );
};
