import { providerLabel } from '../../i18n/providerLabel';
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useAuth } from '../../api/useSnapshot';
import { commands, isTauriRuntime, type AvailablePluginUpdate, type ExtensionReadinessSelection, type PluginImportReview, type PluginInfo, type PluginPermissionSnapshot, type PluginPlaytimeInfo, type PluginSharedDataSnapshot, type PluginUpdateReview } from '../../api';
import { AppModal } from '../../components/AppModal';
import { pluginQueryKeys } from '../../api/pluginQueryKeys';
import { useI18n } from '../../i18n';
import { RememberedConfirmations } from './RememberedConfirmations';
import { PluginFolderApprovals } from './PluginFolderApprovals';
import { PluginSharedResources } from './PluginSharedResources';
import { PluginCompatibilityReview } from './PluginCompatibilityReview';
import { selectPluginConfiguration } from '../../api/pluginSettings';
import { PluginConfigurationSurface } from './PluginConfigurationSurface';
import {
  PluginActivationPermissions,
  type PluginActivationPermissionsValue,
} from './PluginActivationPermissions';
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

function scopeChoiceKey(snapshot: PluginPermissionSnapshot, permissionId: string, scope: unknown): string {
  return JSON.stringify([snapshot.pluginId, snapshot.publisherFingerprint, snapshot.profileId, snapshot.grantRevision, permissionId, scope]);
}

interface PluginInventoryProps {
  renderCards?: (options: {
    selectedPluginId: string | null;
    selectPlugin: (pluginId: string | null) => void;
    details: React.ReactNode;
  }) => React.ReactNode;
}

export const PluginInventory: React.FC<PluginInventoryProps> = ({ renderCards }) => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const [review, setReview] = useState<PluginUpdateReview | null>(null);
  const [importReview, setImportReview] = useState<PluginImportReview | null>(null);
  const [nativeCodeConsent, setNativeCodeConsent] = useState(false);
  const [licenseText, setLicenseText] = useState<Record<string, string>>({});
  const [archivePath, setArchivePath] = useState('');
  const [scopeChoices, setScopeChoices] = useState<Record<string, boolean>>({});
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [busy, setBusy] = useState(false);
  const [updateError, setUpdateError] = useState(false);
  const [updateOperation, setUpdateOperation] = useState<'check' | 'review' | 'trust' | 'approve' | 'decline' | null>(null);
  const [activationResult, setActivationResult] = useState<
    { version: string; rollback: boolean; failed: false } |
    { version: string | undefined; reason: string; failed: true } | null
  >(null);
  const [managementError, setManagementError] = useState(false);
  const [savingPreferences, setSavingPreferences] = useState(false);
  const [playtimeError, setPlaytimeError] = useState(false);
  const updateEventRevision = useRef(0);
  const auth = useAuth().auth;
  const profileId = typeof auth === 'object' && auth !== null && 'signedIn' in auth
    ? auth.signedIn.user.id
    : null;
  const [choosingReadinessTarget, setChoosingReadinessTarget] = useState(false);
  const [readinessTargetId, setReadinessTargetId] = useState('');
  const [reviewIsRollback, setReviewIsRollback] = useState(false);
  const [rollbackConfirmed, setRollbackConfirmed] = useState(false);
  const [retrustFingerprint, setRetrustFingerprint] = useState('');
  const [publisherRetrusted, setPublisherRetrusted] = useState(false);
  const [reviewingPluginId, setReviewingPluginId] = useState<string | null>(null);
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
  const {
    data: playtimePlugins = [],
    isPending: playtimePending,
    isError: playtimeInventoryError,
    refetch: refetchPlaytimePlugins,
  } = useQuery({
    queryKey: ['plugin-playtime-accounting'],
    queryFn: commands.pluginPlaytimeInventory,
    staleTime: 30_000,
  });
  const selectedPlugin = data.find((plugin) => plugin.pluginId === selectedPluginId) ?? (renderCards ? undefined : data[0]);
  const { data: rollbackPackages = [], refetch: refetchRollbacks } = useQuery({
    queryKey: ['plugin-rollback-available'],
    queryFn: commands.pluginRollbackAvailable,
  });
  const managedPluginId = selectedPlugin?.publisherFingerprint ? selectedPlugin.pluginId : null;
  const publisherFingerprint = selectedPlugin?.publisherFingerprint;
  const { data: permissionSnapshot } = useQuery({
    queryKey: pluginQueryKeys.permissions(managedPluginId ?? '', profileId ?? '', publisherFingerprint),
    queryFn: async () => {
      if (!managedPluginId || !profileId) throw new Error('plugin permission profile is unavailable');
      const snapshot = await commands.pluginPermissionSnapshot(managedPluginId, profileId);
      if (snapshot.publisherFingerprint !== publisherFingerprint) throw new Error('plugin permission publisher changed');
      return snapshot;
    },
    enabled: managedPluginId !== null && profileId !== null,
  });
  useQuery({
    queryKey: pluginQueryKeys.configuration(managedPluginId ?? ''),
    queryFn: () => commands.pluginConfigurationState(managedPluginId!),
    enabled: managedPluginId !== null,
  });
  const { data: sharedDataSnapshot, isError: sharedDataError } = useQuery({
    queryKey: pluginQueryKeys.sharedData(managedPluginId ?? '', profileId ?? '', publisherFingerprint),
    queryFn: async () => {
      if (!managedPluginId || !profileId) throw new Error('plugin data-sharing profile is unavailable');
      const snapshot = await commands.pluginSharedDataSnapshot(managedPluginId, profileId);
      if (snapshot.profileId !== profileId) throw new Error('plugin data-sharing profile changed');
      if (snapshot.publisherFingerprint !== publisherFingerprint) throw new Error('plugin data-sharing publisher changed');
      return snapshot;
    },
    enabled: managedPluginId !== null && profileId !== null && selectedPlugin?.pluginKind === 'provider',
  });
  const periodicChecks = updatePreferences?.periodicChecks ?? false;
  const managed = Boolean(selectedPlugin?.publisherFingerprint);
  const canManageRuntime = managed && selectedPlugin?.runtimeAvailable === true && profileId !== null;
  const canManageAccount = managed && profileId !== null;
  const configurationQuery = useQuery({
    queryKey: ['plugin-configuration-modes', selectedPlugin?.providerId],
    queryFn: () => commands.pluginConfigurationModes(selectedPlugin!.providerId),
    enabled: Boolean(selectedPlugin && selectedPlugin.status === 'installed' && selectedPlugin.session === 'active'),
    staleTime: 5_000,
  });
  const configurationModes = configurationQuery.data;
  const configurationChoice = configurationModes ? selectPluginConfiguration(configurationModes) : undefined;
  const sharedDataInitialValue = useMemo(() => sharedDataSnapshot ? ({
    commonDataRead: sharedDataSnapshot.commonCategories,
    selectedRecordIds: sharedDataSnapshot.selectedRecordIds,
    sourcePolicies: sharedDataSnapshot.sourcePolicies,
  }) : undefined, [sharedDataSnapshot]);


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
        refreshes.push(queryClient.invalidateQueries({ queryKey: pluginQueryKeys.sharedData(pluginId, profileId) }));
      }
      refreshes.push(queryClient.invalidateQueries({ queryKey: pluginQueryKeys.configuration(pluginId) }));
    }
    await Promise.all(refreshes);
  };

  const checkUpdates = async () => {
    setUpdateOperation('check');
    setActivationResult(null);
    setUpdateError(false);
    const eventRevisionAtStart = updateEventRevision.current;
    try {
      const available = await commands.pluginUpdateCheck();
      if (eventRevisionAtStart === updateEventRevision.current) {
        queryClient.setQueryData(pluginQueryKeys.availableUpdates, available);
      }
    }
    catch { setUpdateError(true); }
    finally { setUpdateOperation(null); }
  };

  const reviewUpdate = async (pluginId: string, version: string) => {
    setUpdateOperation('review');
    setActivationResult(null);
    setUpdateError(false);
    setReview(null);
    setReviewIsRollback(false);
    setRollbackConfirmed(false);
    setPublisherRetrusted(false);
    setRetrustFingerprint('');
    try { setReview(await commands.pluginUpdateReview(pluginId, version)); }
    catch { setUpdateError(true); }
    finally { setUpdateOperation(null); }
  };

  const reviewRollback = async (pluginId: string, version: string) => {
    setUpdateOperation('review');
    setActivationResult(null);
    setUpdateError(false);
    setReview(null);
    setReviewIsRollback(true);
    setRollbackConfirmed(false);
    setPublisherRetrusted(false);
    setRetrustFingerprint('');
    try { setReview(await commands.pluginRollbackReview(pluginId, version)); }
    catch { setUpdateError(true); }
    finally { setUpdateOperation(null); }
  };

  const retrustPublisher = async () => {
    if (!review || retrustFingerprint !== review.publisherFingerprint) return;
    setUpdateOperation('trust');
    setActivationResult(null);
    setUpdateError(false);
    try {
      await commands.pluginUpdateRetrustPublisher(review.pluginId, review.candidateVersion, retrustFingerprint);
      setPublisherRetrusted(true);
    } catch { setUpdateError(true); }
    finally { setUpdateOperation(null); }
  };

  const readinessTarget = review?.extensionTargets?.find(
    ({ selection }) => selection.installationId === readinessTargetId,
  )?.selection;
  const approveLabel = t(reviewIsRollback ? 'plugins.updates.rollbackApprove' : 'plugins.updates.approve');

  const approveUpdate = async (target?: ExtensionReadinessSelection) => {
    if (!review || (review.extensionTargets && !target)) return;
    setUpdateOperation('approve');
    setActivationResult(null);
    setUpdateError(false);
    try {
      let version: string;
      if (reviewIsRollback) {
        const args = [review.pluginId, review.candidateVersion, rollbackConfirmed] as const;
        version = await (target ? commands.pluginRollbackApprove(...args, target) : commands.pluginRollbackApprove(...args));
      } else {
        const args = [review.pluginId, review.candidateVersion] as const;
        version = await (target ? commands.pluginUpdateApprove(...args, target) : commands.pluginUpdateApprove(...args));
      }
      setActivationResult({ version, rollback: reviewIsRollback, failed: false });
    } catch (error) {
      const reason = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
      const active = await queryClient.fetchQuery({
        queryKey: pluginQueryKeys.inventory,
        queryFn: commands.pluginInventory,
        staleTime: 0,
      }).catch(() => undefined);
      setActivationResult({ failed: true, reason, version: active?.find((plugin) => plugin.pluginId === review.pluginId)?.version });
    } finally {
      setChoosingReadinessTarget(false);
      setReview(null);
      setReviewIsRollback(false);
      setRollbackConfirmed(false);
      setPublisherRetrusted(false);
      await Promise.all([refetchRollbacks(), refreshPluginHostState(review.pluginId)]);
      setUpdateOperation(null);
    }
  };

  const declineUpdate = async () => {
    if (!review) return;
    setChoosingReadinessTarget(false);
    setUpdateOperation('decline');
    setActivationResult(null);
    setUpdateError(false);
    try {
      await commands.pluginUpdateDecline(review.pluginId);
      setReview(null);
    } catch {
      setUpdateError(true);
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.availableUpdates });
    } finally { setUpdateOperation(null); }
  };

  const reviewImport = async () => {
    setBusy(true);
    setManagementError(false);
    setImportReview(null);
    setNativeCodeConsent(false);
    setLicenseText({});
    try { setImportReview(await commands.pluginImportReview(archivePath.trim())); }
    catch { setManagementError(true); }
    finally { setBusy(false); }
  };

  const installImport = async () => {
    if (!importReview) return;
    setBusy(true);
    setManagementError(false);
    try {
      await commands.pluginImportInstall(importReview.token, nativeCodeConsent);
      setImportReview(null);
      setNativeCodeConsent(false);
      setLicenseText({});
      setArchivePath('');
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.inventory });
    } catch { setManagementError(true); }
    finally { setBusy(false); }
  };

  const savePermissions = async (snapshot: PluginPermissionSnapshot) => {
    if (!profileId || snapshot.pluginId !== managedPluginId || snapshot.publisherFingerprint !== publisherFingerprint) return;
    const queryKey = pluginQueryKeys.permissions(snapshot.pluginId, profileId, snapshot.publisherFingerprint);
    const decisions = snapshot.requestedPermissions.flatMap((request) => request.allowed_scopes.map((scope) => {
      const key = scopeChoiceKey(snapshot, request.permission_id, scope);
      const persisted = snapshot.effectiveGrants.some((grant) => grant.permission_id === request.permission_id && JSON.stringify(grant.scope) === JSON.stringify(scope) && grant.state === 'granted');
      return { permission_id: request.permission_id, scope, granted: scopeChoices[key] ?? persisted };
    }));
    setBusy(true);
    setManagementError(false);
    try {
      const saved = await commands.pluginPermissionsReplace(snapshot.pluginId, snapshot.profileId, snapshot.publisherFingerprint, snapshot.grantRevision, decisions);
      setScopeChoices({});
      queryClient.setQueryData(queryKey, saved);
      await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.inventory });
    } catch {
      setManagementError(true);
      setScopeChoices({});
      await queryClient.invalidateQueries({ queryKey });
    } finally { setBusy(false); }
  };

  const saveSharedDataPermissions = async (
    snapshot: PluginSharedDataSnapshot,
    value: PluginActivationPermissionsValue,
  ) => {
    if (!profileId || snapshot.profileId !== profileId || snapshot.pluginId !== managedPluginId || snapshot.publisherFingerprint !== publisherFingerprint) return;
    const queryKey = pluginQueryKeys.sharedData(snapshot.pluginId, profileId, snapshot.publisherFingerprint);
    setBusy(true);
    setManagementError(false);
    try {
      const saved = await commands.pluginSharedDataReplace(snapshot.pluginId, {
        profileId,
        expectedPublisherFingerprint: snapshot.publisherFingerprint,
        expectedRevision: snapshot.policyRevision,
        commonCategories: value.commonDataRead,
        selectedRecordIds: value.selectedRecordIds,
        sourcePolicies: value.sourcePolicies,
      });
      queryClient.setQueryData(queryKey, saved);
    } catch {
      setManagementError(true);
      await queryClient.invalidateQueries({ queryKey });
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

  const setPlaytimeAccounting = async (plugin: PluginPlaytimeInfo, enabled: boolean) => {
    setPlaytimeError(false);
    try {
      await commands.pluginPlaytimeSetAccounting(plugin.pluginId, enabled);
      await refetchPlaytimePlugins();
    } catch {
      setPlaytimeError(true);
    }
  };

  const stateClass = (status: PluginInfo['status']) => status === 'installed'
    ? settingsPillSuccess
    : status === 'missing' || status === 'failed' || status === 'incompatible'
      ? settingsPillWarning
      : settingsPill;

  const selectPlugin = (pluginId: string | null) => {
    setScopeChoices({});
    setManagementError(false);
    setSelectedPluginId(pluginId);
  };

  const details = selectedPlugin && (
    <article className={renderCards ? "flex flex-col gap-sm" : `${settingsCard} self-start`} aria-label={t('plugins.details')}>
      {!renderCards && <>
      <div className="flex flex-wrap items-center gap-compact">
        <h3 className={settingsRowTitle}>{providerLabel(selectedPlugin.providerId, selectedPlugin.name, t)}</h3>
        <span className={settingsPill}>{selectedPlugin.source === 'official' ? t('plugins.official') : t('plugins.community')}</span>
        <span className={stateClass(selectedPlugin.status)}>{statusLabel(selectedPlugin.status, t)}</span>
      </div>
      <p className={settingsRowDescription}>{t('plugins.identifier')}: {selectedPlugin.pluginId}</p>
      <p className={settingsRowDescription}>{t('plugins.session')}: {selectedPlugin.session === 'active' ? t('plugins.sessionActive') : t('plugins.sessionStopped')}</p>
      {managed && selectedPlugin.version && <p className={settingsRowDescription}>{t('plugins.import.version')}: {selectedPlugin.version}</p>}
      </>}
      {managed && selectedPlugin.compatibilityDetails && <PluginCompatibilityReview details={selectedPlugin.compatibilityDetails} />}
      {managed && selectedPlugin.pluginKind === 'extension' && selectedPlugin.runtimeAvailable !== true && <p className={settingsRowDescription} role="status">{t('plugins.import.extensionRuntimeUnavailable')}</p>}
      {managed && selectedPlugin.pluginKind !== 'extension' && selectedPlugin.runtimeAvailable === false && <p className={settingsRowDescription} role="status">{t('plugins.import.restartRequired')}</p>}
      {managed && profileId === null && <p className={settingsRowDescription} role="status">{t('plugins.signInRequired')}</p>}
      {(canManageRuntime || canManageAccount) && <div className="mt-compact flex flex-wrap gap-compact">
        {canManageRuntime && (selectedPlugin.enabled ? <button type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm" disabled={busy} onClick={() => void setPluginEnabled(selectedPlugin.pluginId, false)}>{t('plugins.disable')}</button> : <button type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm" disabled={busy || selectedPlugin.status === 'incompatible'} onClick={() => void setPluginEnabled(selectedPlugin.pluginId, true)}>{t('plugins.enable')}</button>)}
        {canManageAccount && <button type="button" className="rounded-sm border border-warning px-sm py-1 text-sm" disabled={busy} onClick={() => setConfirmRemove(true)}>{t('plugins.remove')}</button>}
      </div>}
      {managed && profileId && permissionSnapshot && permissionSnapshot.pluginId === selectedPlugin.pluginId && <div className="mt-compact border-t border-border-subtle pt-compact" aria-label={t('plugins.permissionScopes')}>
        <p className={settingsRowTitle}>{t('plugins.permissionScopes')}</p>
        <p className={`${settingsRowDescription} mt-1 break-all`}>{t('plugins.publisherFingerprint')}: {permissionSnapshot.publisherFingerprint}</p>
        <p className={`${settingsRowDescription} mt-1`}>{t('plugins.requestedGrantedEffectiveDescription')}</p>
        {permissionSnapshot.requestedPermissions.map((request) => <fieldset key={request.permission_id} className="mt-compact">
          <legend className={settingsRowDescription}>{request.permission_id}{request.required ? ` · ${t('plugins.required')}` : ''} · {t('plugins.permissionRequested')}</legend>
          {request.allowed_scopes.map((scope) => {
            const key = scopeChoiceKey(permissionSnapshot, request.permission_id, scope);
            const effective = permissionSnapshot.effectiveGrants.find((grant) => grant.permission_id === request.permission_id && JSON.stringify(grant.scope) === JSON.stringify(scope));
            const active = effective?.state === 'granted';
            return <label key={key} className="flex items-center gap-compact py-1 text-sm">
              <input type="checkbox" disabled={!canManageRuntime || busy} checked={scopeChoices[key] ?? active} onChange={(event) => {
                const checked = event.currentTarget.checked;
                setScopeChoices((current) => ({ ...current, [key]: checked }));
              }} />
              <span>{scope.scope}{'category' in scope ? ` · ${scope.category}` : 'value' in scope ? ` · ${scope.value}` : ''} · {t('plugins.permissionEffective')}: {t(`plugins.permissionState.${effective?.state ?? 'denied'}`)}</span>
            </label>;
          })}
        </fieldset>)}
        <button type="button" className="mt-compact rounded-sm border border-border-subtle px-sm py-1 text-sm" disabled={!canManageRuntime || busy} onClick={() => void savePermissions(permissionSnapshot)}>{t('plugins.permissions.save')}</button>
      </div>}

      {managed && selectedPlugin.pluginKind === 'provider' && <div className="mt-compact border-t border-border-subtle pt-compact">
        <button
          type="button"
          aria-expanded={reviewingPluginId === selectedPlugin.pluginId}
          aria-controls={`plugin-activation-review-${selectedPlugin.pluginId}`}
          className="rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]"
          onClick={() => setReviewingPluginId((current) => current === selectedPlugin.pluginId ? null : selectedPlugin.pluginId)}
        >
          {t('plugins.activation.openReview')}
        </button>
        {reviewingPluginId === selectedPlugin.pluginId && (
          <div id={`plugin-activation-review-${selectedPlugin.pluginId}`} className="mt-compact border-t border-border-subtle pt-compact">
            <p className={`${settingsRowDescription} mb-compact`}>
              {t('plugins.activation.reviewFor', { plugin: providerLabel(selectedPlugin.providerId, selectedPlugin.name, t) })}
            </p>
            {sharedDataError && <p className="mb-compact text-sm text-warning" role="alert">{t('plugins.management.failed')}</p>}
            {sharedDataSnapshot && sharedDataInitialValue && sharedDataSnapshot.pluginId === selectedPlugin.pluginId ? (
              <PluginActivationPermissions
                key={JSON.stringify([sharedDataSnapshot.profileId, sharedDataSnapshot.pluginId, sharedDataSnapshot.publisherFingerprint])}
                pluginId={selectedPlugin.pluginId}
                catalog={sharedDataSnapshot.catalog}
                initialValue={sharedDataInitialValue}
                busy={busy}
                onSave={(value) => void saveSharedDataPermissions(sharedDataSnapshot, value)}
              />
            ) : <p className={settingsRowDescription} role="status">{profileId ? t('plugins.activation.loading') : t('plugins.signInRequired')}</p>}
          </div>
        )}
      </div>}

      {!renderCards && <>
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
      </>}
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
          <PluginSharedResources
            pluginId={selectedPlugin.pluginId}
            publisherFingerprint={selectedPlugin.publisherFingerprint}
          />
        </>
      )}
    </article>
  );

  return (
    <section className="flex flex-col gap-sm" aria-label={t('plugins.inventoryTitle')}>
      {!renderCards && <p className={`${settingsRowDescription} mb-compact`}>{t('plugins.inventoryDescription')}</p>}
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
          <section className={settingsRowDescription} aria-label={t('plugins.playtime.title')}>
            <p>{t('plugins.import.playtimeDisclosure')}</p>
            <p>{t('plugins.playtime.gameplayDetection')}: {importReview.playtime.identifiesGameplay ? t('plugins.playtime.yes') : t('plugins.playtime.no')}</p>
            <p>{t('plugins.playtime.pauseDetection')}: {importReview.playtime.identifiesPauses ? t('plugins.playtime.yes') : t('plugins.playtime.no')}</p>
            <p>{t('plugins.playtime.modeLabel')}: {t(`plugins.playtime.mode.${importReview.playtime.withoutPauseDetection}`)}</p>
            {importReview.playtime.identifiesGameplay && !importReview.playtime.identifiesPauses && importReview.playtime.withoutPauseDetection === 'application_open' && (
              <p className="mt-1" role="note">{t('plugins.playtime.applicationOpenWarning')}</p>
            )}
          </section>
          <p className={settingsRowDescription}>{t('plugins.nativeAccess')}: {importReview.nativeAccess.join(', ') || '—'}</p>
          {importReview.nativeAccess.length > 0 && <label className="mt-compact flex items-start gap-compact text-sm">
            <input type="checkbox" checked={nativeCodeConsent} disabled={busy} onChange={(event) => setNativeCodeConsent(event.currentTarget.checked)} />
            <span>{t('plugins.import.nativeConsent')}</span>
          </label>}
          <p className={settingsRowDescription}>{t('plugins.import.license')}: {importReview.license}</p>
          {[...new Set([...importReview.licenseFiles, ...importReview.licenseNotices])].map((file) => (
            <details key={file} className="mt-1 text-sm">
              <summary className="cursor-pointer">{file}</summary>
              {licenseText[file] === undefined ? <button type="button" className="mt-1 underline" disabled={busy} onClick={() => {
                setBusy(true);
                void commands.pluginImportLicenseText(importReview.token, file)
                  .then((text) => setLicenseText((current) => ({ ...current, [file]: text })))
                  .catch(() => setManagementError(true))
                  .finally(() => setBusy(false));
              }}>{t('plugins.import.viewLicense')}</button> : <pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap rounded-sm bg-raised p-sm">{licenseText[file]}</pre>}
            </details>
          ))}
          <p className={settingsRowDescription}>{t('plugins.import.compatibility')}: {t(`plugins.import.${importReview.compatibility}`)} {importReview.compatibilityReason ?? ''}</p>
          {importReview.compatibilityDetails && <PluginCompatibilityReview details={importReview.compatibilityDetails} />}
          {importReview.trust !== 'trusted' ? <button type="button" className="mt-compact rounded-sm border border-border-subtle px-sm py-1 text-sm" disabled={busy || profileId === null || importReview.compatibility !== 'compatible'} onClick={() => {
            setBusy(true);
            setManagementError(false);
            void commands.pluginImportConfirmPublisher(importReview.token, importReview.publisherFingerprint).then(setImportReview).catch(() => setManagementError(true)).finally(() => setBusy(false));
          }}>{importReview.trust === 'changed' ? t('plugins.import.retrust') : t('plugins.import.trust')}</button> : null}
          <button type="button" className="ml-compact mt-compact rounded-sm border border-brand bg-brand px-sm py-1 text-sm text-white" disabled={busy || profileId === null || importReview.trust !== 'trusted' || importReview.compatibility !== 'compatible' || (importReview.nativeAccess.length > 0 && !nativeCodeConsent)} onClick={() => void installImport()}>{t('plugins.import.install')}</button>
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
        <button type="button" className="mt-compact rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]" disabled={updateOperation !== null} onClick={() => void checkUpdates()}>
          {updateOperation === 'check' ? t('plugins.updates.checking') : t('plugins.updates.checkNow')}
        </button>
        {updates.map((update) => (
          <div key={update.pluginId} className="mt-compact border-t border-border-subtle pt-compact">
            <p className={settingsRowTitle}>{update.name}: {update.installedVersion}</p>
            <div className="flex flex-wrap gap-1">
              {update.versions.map((version) => (
                <button key={version} type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]" disabled={updateOperation !== null} onClick={() => void reviewUpdate(update.pluginId, version)}>
                  {t('plugins.updates.reviewVersion', { version })}
                </button>
              ))}
            </div>
          </div>
        ))}
        {rollbackPackages.map((pkg) => (
          <div key={pkg.pluginId} className="mt-compact border-t border-border-subtle pt-compact">
            <p className={settingsRowTitle}>{t('plugins.updates.rollbackTitle')}: {pkg.name} ({pkg.activeVersion})</p>
            <p className={settingsRowDescription}>{t('plugins.updates.rollbackDescription')}</p>
            <div className="flex flex-wrap gap-1">
              {pkg.versions.map((version) => (
                <button key={version} type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]" disabled={updateOperation !== null} onClick={() => void reviewRollback(pkg.pluginId, version)}>
                  {t('plugins.updates.reviewRollback', { version })}
                </button>
              ))}
            </div>
          </div>
        ))}
        {updateOperation && updateOperation !== 'check' && (
          <p role="status" className={settingsRowDescription}>{t('plugins.updates.working')}</p>
        )}
        {updateError && <p className="mt-compact text-sm text-warning" role="alert">{t('plugins.updates.failed')}</p>}
        {activationResult && (activationResult.failed ? (
          <div role="alert" className="mt-compact text-sm text-warning">
            <p>{t('plugins.updates.activationFailed')}</p>
            <p>{activationResult.version
              ? t('plugins.updates.activeVersion', { version: activationResult.version })
              : t('plugins.updates.activeVersionUnavailable')}</p>
            {activationResult.reason && <p>{activationResult.reason}</p>}
          </div>
        ) : (
          <p role="status" className={settingsRowDescription}>{t(activationResult.rollback ? 'plugins.updates.rollbackComplete' : 'plugins.updates.updateComplete', { version: activationResult.version })}</p>
        ))}
        {review && (
          <article className="mt-compact border-t border-border-subtle pt-compact" aria-label={reviewIsRollback ? t('plugins.updates.rollbackReviewTitle') : t('plugins.updates.reviewTitle')}>
            <h3 className={settingsRowTitle}>{reviewIsRollback ? t('plugins.updates.rollbackReviewTitle') : t('plugins.updates.reviewTitle')}: {review.installedVersion} → {review.candidateVersion}</h3>
            {review.compatibilityDetails && <PluginCompatibilityReview details={review.compatibilityDetails} />}
            {reviewIsRollback && (
              <label className="my-compact flex items-start gap-compact text-sm">
                <input type="checkbox" checked={rollbackConfirmed} onChange={(event) => setRollbackConfirmed(event.currentTarget.checked)} />
                <span>{t('plugins.updates.rollbackConfirm')}</span>
              </label>
            )}
            <p className={settingsRowDescription}>{t('plugins.grants')}: +{review.addedPermissions.join(', ') || '—'}; {t('plugins.updates.changed')}: {review.changedPermissions.join(', ') || '—'}; −{review.removedPermissions.join(', ') || '—'}</p>
            {review.permissionChanges.map((change) => <p key={change.id} className={settingsRowDescription}>{change.id}: {change.installedScope} → {change.candidateScope}</p>)}
            <p className={settingsRowDescription}>{t('plugins.nativeAccess')}: +{review.addedNativeAccess.join(', ') || '—'}; −{review.removedNativeAccess.join(', ') || '—'}</p>
            <p className={settingsRowDescription}>{t('plugins.updates.api')}: {review.installedApi} → {review.candidateApi}</p>
            <p className={settingsRowDescription}>{t('plugins.updates.license')}: {review.installedLicense} → {review.candidateLicense}</p>
            <p className={settingsRowDescription}>{t('plugins.updates.licenseFiles')}: {review.installedLicenseFiles.join(', ') || '—'} → {review.candidateLicenseFiles.join(', ') || '—'}</p>
            <p className={settingsRowDescription}>{t('plugins.updates.licenseNotices')}: {review.installedLicenseNotices.join('; ') || '—'} → {review.candidateLicenseNotices.join('; ') || '—'}</p>
            <p className={settingsRowDescription}>{t('plugins.updates.publisher')}: {review.installedPublisherKeyId} → {review.candidatePublisherKeyId}</p>
            {review.publisherChanged && (
              <div className="my-compact border border-border-subtle p-sm">
                <p className={settingsRowDescription}>{t('plugins.updates.publisherChanged')}: {review.installedPublisherKeyId} → {review.candidatePublisherKeyId}</p>
                <p className={`${settingsRowDescription} break-all`}>{t('plugins.updates.publisherFingerprint')}: {review.publisherFingerprint}</p>
                {!publisherRetrusted ? (
                  <div className="mt-compact flex flex-wrap items-center gap-compact">
                    <input aria-label={t('plugins.updates.publisherFingerprintConfirm')} className="min-w-64 rounded-sm border border-border-subtle bg-raised px-sm py-1 text-sm" value={retrustFingerprint} onChange={(event) => setRetrustFingerprint(event.currentTarget.value)} />
                    <button type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]" disabled={updateOperation !== null || retrustFingerprint !== review.publisherFingerprint} onClick={() => void retrustPublisher()}>{t('plugins.updates.publisherRetrust')}</button>
                  </div>
                ) : <p className={`${settingsRowDescription} mt-compact`} role="status">{t('plugins.updates.publisherRetrusted')}</p>}
              </div>
            )}
            {review.playtimeChanged && <p className={settingsRowDescription}>{t('plugins.playtime.updatePolicy')}: {t(`plugins.playtime.mode.${review.installedPlaytime.withoutPauseDetection}`)} → {t(`plugins.playtime.mode.${review.candidatePlaytime.withoutPauseDetection}`)}</p>}
            <p className={settingsRowDescription}>{t('plugins.updates.dataPreserved')}</p>
            {review.changedDataSchemas.length > 0 && <p className={settingsRowDescription}>{t('plugins.updates.dataSchemaChanged')}: {review.changedDataSchemas.join(', ')}. {t('plugins.updates.versionedDataKeysRequired')}</p>}
            <p className={settingsRowDescription}>{t('plugins.updates.newPermissionsDenied')}</p>
            <div className="mt-compact flex gap-compact">
              <button type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm hover:bg-[#2e2e2e]" disabled={updateOperation !== null} onClick={() => void declineUpdate()}>{t('plugins.updates.decline')}</button>
              <button type="button" className="rounded-sm border border-brand bg-brand px-sm py-1 text-sm text-white hover:brightness-110" disabled={updateOperation !== null || (reviewIsRollback && !rollbackConfirmed) || (review.publisherChanged && !publisherRetrusted)} onClick={() => {
                if (review.extensionTargets) {
                  setReadinessTargetId('');
                  setChoosingReadinessTarget(true);
                } else {
                  void approveUpdate();
                }
              }}>{review.extensionTargets ? t('plugins.updates.chooseTarget') : approveLabel}</button>
            </div>
          </article>
        )}
      </div>
      {managementError && <p className="mt-compact text-sm text-warning" role="alert">{t('plugins.management.failed')}</p>}
      {renderCards ? renderCards({ selectedPluginId, selectPlugin, details }) : isPending ? (
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
                onClick={() => selectPlugin(plugin.pluginId)}
              >
                <span className={settingsRowTitle}>{providerLabel(plugin.providerId, plugin.name, t)}</span>
                <span className="flex flex-wrap gap-1">
                  <span className={settingsPill}>{plugin.source === 'official' ? t('plugins.official') : t('plugins.community')}</span>
                  <span className={stateClass(plugin.status)}>{statusLabel(plugin.status, t)}</span>
                </span>
              </button>
            ))}
          </div>
          {details}
        </div>
      )}
      <AppModal.Root opened={choosingReadinessTarget} onClose={() => void declineUpdate()} closeOnEscape={updateOperation === null} closeOnClickOutside={updateOperation === null}>
        <AppModal.Overlay />
        <AppModal.Content className="max-w-md bg-[var(--settings-bg-base)] p-md">
          <AppModal.Header>
            <AppModal.Title>{t('plugins.updates.chooseTarget')}</AppModal.Title>
            <AppModal.CloseButton aria-label={t('plugins.cancel')} disabled={updateOperation !== null} />
          </AppModal.Header>
          <AppModal.Body className="pt-md">
            <p className={settingsRowDescription}>{t('plugins.updates.targetDescription')}</p>
            {review?.extensionTargets?.length ? (
              <label className="flex flex-col gap-compact text-sm">
                {t('plugins.updates.targetGame')}
                <select
                  className="w-full rounded-sm border border-border-subtle bg-raised px-sm py-1 text-sm"
                  value={readinessTargetId}
                  disabled={updateOperation !== null}
                  onChange={(event) => setReadinessTargetId(event.currentTarget.value)}
                >
                  <option value="">{t('plugins.updates.selectGame')}</option>
                  {review.extensionTargets.map(({ name, providerId, selection }) => (
                    <option key={selection.installationId} value={selection.installationId}>
                      {name} ({providerLabel(providerId, undefined, t)})
                    </option>
                  ))}
                </select>
              </label>
            ) : <p role="status" className={settingsRowDescription}>{t('plugins.updates.noTargets')}</p>}
            <div className="flex justify-end gap-compact">
              <button type="button" className="rounded-sm border border-border-subtle px-sm py-1 text-sm" disabled={updateOperation !== null} onClick={() => void declineUpdate()}>{t('plugins.cancel')}</button>
              <button type="button" className="rounded-sm border border-brand bg-brand px-sm py-1 text-sm text-white" disabled={updateOperation !== null || !readinessTarget} onClick={() => void approveUpdate(readinessTarget)}>{approveLabel}</button>
            </div>
          </AppModal.Body>
        </AppModal.Content>
      </AppModal.Root>

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
      <section className="mt-md" aria-label={t('plugins.playtime.title')}>
        <h3 className={settingsRowTitle}>{t('plugins.playtime.title')}</h3>
        <p className={`${settingsRowDescription} mb-compact`}>{t('plugins.playtime.description')}</p>
        {playtimePending ? (
          <p className={settingsRowDescription} role="status">{t('plugins.loading')}</p>
        ) : playtimeInventoryError ? (
          <p className={settingsRowDescription} role="alert">{t('plugins.playtime.loadFailed')}</p>
        ) : playtimePlugins.length === 0 ? (
          <p className={settingsRowDescription}>{t('plugins.playtime.empty')}</p>
        ) : (
          <div className="grid grid-cols-1 gap-compact">
            {playtimePlugins.map((plugin) => (
              <article key={plugin.pluginId} className={settingsCard} aria-label={plugin.name}>
                <h4 className={settingsRowTitle}>{plugin.name}</h4>
                <p className={settingsRowDescription}>{plugin.pluginId} · {plugin.version}</p>
                <p className={settingsRowDescription}>{t('plugins.playtime.gameplayDetection')}: {plugin.playtime.identifiesGameplay ? t('plugins.playtime.yes') : t('plugins.playtime.no')}</p>
                <p className={settingsRowDescription}>{t('plugins.playtime.pauseDetection')}: {plugin.playtime.identifiesPauses ? t('plugins.playtime.yes') : t('plugins.playtime.no')}</p>
                <p className={settingsRowDescription}>{t('plugins.playtime.modeLabel')}: {t(`plugins.playtime.mode.${plugin.playtime.withoutPauseDetection}`)}</p>
                {plugin.playtime.identifiesGameplay && !plugin.playtime.identifiesPauses && plugin.playtime.withoutPauseDetection === 'application_open' && (
                  <p className={`${settingsRowDescription} mt-1`} role="note">{t('plugins.playtime.applicationOpenWarning')}</p>
                )}
                <label className="mt-compact flex items-center gap-compact text-sm">
                  <input
                    type="checkbox"
                    checked={plugin.accountingEnabled}
                    disabled={!plugin.playtime.identifiesGameplay || (plugin.playtime.withoutPauseDetection === 'disabled' && !plugin.playtime.identifiesPauses)}
                    onChange={(event) => void setPlaytimeAccounting(plugin, event.currentTarget.checked)}
                  />
                  <span>{t('plugins.playtime.enabled')}</span>
                </label>
              </article>
            ))}
          </div>
        )}
        {playtimeError && <p className="mt-compact text-sm text-warning" role="alert">{t('plugins.playtime.saveFailed')}</p>}
      </section>
    </section>
  );
};
