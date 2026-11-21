import React, { useMemo, useState } from 'react';
import { useQueries, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  commands,
  type AvailableBundleRollback,
  type AvailableBundleUpdate,
  type BundleComponentPermissionSubmission,
  type BundleUninstallOutcome,
  type BundleUpdateReview,
  type PluginBundleComponent,
  type PluginInfo,
  type PluginPermissionSnapshot,
} from '../../api';
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

function statusClass(status: PluginBundleComponent['status']) {
  return status === 'installed'
    ? settingsPillSuccess
    : status === 'unavailable' || status === 'incompatible' || status === 'missing'
      ? settingsPillWarning
      : settingsPill;
}

function scopeChoiceKey(
  snapshot: PluginPermissionSnapshot,
  permissionId: string,
  scope: unknown,
): string {
  return JSON.stringify([
    snapshot.pluginId,
    snapshot.publisherFingerprint,
    snapshot.profileId,
    snapshot.grantRevision,
    permissionId,
    scope,
  ]);
}

interface PluginBundleDetailsProps {
  bundle: PluginInfo;
  profileId: string | null;
  busy: boolean;
  onToggleEnabled: (pluginId: string, enabled: boolean) => Promise<void>;
  onRequestUninstall: () => void;
  uninstallOutcome: BundleUninstallOutcome | null;
  uninstallError: boolean;
}

/**
 * Details of one aggregated bundle entry. Every component keeps its own identity and lifecycle:
 * activation is toggled per component, permissions are reviewed and saved per component in one
 * grouped submission, and the single uninstall lives on the bundle itself.
 */
export const PluginBundleDetails: React.FC<PluginBundleDetailsProps> = ({
  bundle,
  profileId,
  busy,
  onToggleEnabled,
  onRequestUninstall,
  uninstallOutcome,
  uninstallError,
}) => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const components = useMemo(() => bundle.bundleComponents ?? [], [bundle.bundleComponents]);
  const [scopeChoices, setScopeChoices] = useState<Record<string, boolean>>({});
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const [componentErrors, setComponentErrors] = useState<Record<string, string>>({});
  const [saved, setSaved] = useState(false);
  const [bundleUpdateReview, setBundleUpdateReview] = useState<BundleUpdateReview | null>(null);
  const [bundleUpdateOperation, setBundleUpdateOperation] = useState<
    'check' | 'review' | 'approve' | 'rollbackReview' | 'rollbackApprove' | null
  >(null);
  const [bundleUpdateError, setBundleUpdateError] = useState<string | null>(null);
  const [bundleUpdateResult, setBundleUpdateResult] = useState<{
    version: string;
    rollback: boolean;
  } | null>(null);
  const [rollbackConfirmed, setRollbackConfirmed] = useState(false);

  const bundleId = bundle.bundleId ?? '';
  const { data: availableBundleUpdates = [], refetch: refetchBundleUpdates } = useQuery<
    AvailableBundleUpdate[]
  >({
    queryKey: ['plugin-bundle-updates', bundleId],
    queryFn: commands.pluginBundleUpdateAvailable,
    enabled: profileId !== null && bundleId.length > 0,
  });
  const { data: availableBundleRollbacks = [], refetch: refetchBundleRollbacks } = useQuery<
    AvailableBundleRollback[]
  >({
    queryKey: ['plugin-bundle-rollbacks', bundleId],
    queryFn: commands.pluginBundleRollbackAvailable,
    enabled: profileId !== null && bundleId.length > 0,
  });
  const bundleUpdate = availableBundleUpdates.find((update) => update.bundleId === bundleId);
  const bundleRollback = availableBundleRollbacks.find((rollback) => rollback.bundleId === bundleId);

  const snapshotQueries = useQueries({
    queries: components.map((component) => ({
      queryKey: pluginQueryKeys.permissions(
        component.pluginId,
        profileId ?? '',
        component.publisherFingerprint,
      ),
      queryFn: async () => {
        if (!profileId) throw new Error('plugin permission profile is unavailable');
        const snapshot = await commands.pluginPermissionSnapshot(component.pluginId, profileId);
        if (snapshot.publisherFingerprint !== component.publisherFingerprint) {
          throw new Error('plugin permission publisher changed');
        }
        return snapshot;
      },
      enabled: profileId !== null && Boolean(component.publisherFingerprint),
    })),
  });

  const savePermissions = async () => {
    const snapshots = snapshotQueries.flatMap((query) => (query.data ? [query.data] : []));
    if (snapshots.length === 0) return;
    const submissions: BundleComponentPermissionSubmission[] = snapshots.map((snapshot) => ({
      pluginId: snapshot.pluginId,
      expectedProfileId: snapshot.profileId,
      expectedPublisherFingerprint: snapshot.publisherFingerprint,
      expectedRevision: snapshot.grantRevision,
      decisions: snapshot.requestedPermissions.flatMap((request) =>
        request.allowed_scopes.map((scope) => {
          const key = scopeChoiceKey(snapshot, request.permission_id, scope);
          const persisted = snapshot.effectiveGrants.some(
            (grant) =>
              grant.permission_id === request.permission_id
              && JSON.stringify(grant.scope) === JSON.stringify(scope)
              && grant.state === 'granted',
          );
          return {
            permission_id: request.permission_id,
            scope,
            granted: scopeChoices[key] ?? persisted,
          };
        }),
      ),
    }));
    setSaving(true);
    setSaveError(false);
    setSaved(false);
    setComponentErrors({});
    try {
      const result = await commands.pluginBundlePermissionsReplace(submissions);
      const errors: Record<string, string> = {};
      for (const component of result.components) {
        if (component.error) errors[component.pluginId] = component.error;
      }
      setComponentErrors(errors);
      setScopeChoices({});
      setSaved(true);
      await Promise.all(
        snapshots.flatMap((snapshot) => [
          queryClient.invalidateQueries({
            queryKey: pluginQueryKeys.permissions(
              snapshot.pluginId,
              snapshot.profileId,
              snapshot.publisherFingerprint,
            ),
          }),
          queryClient.invalidateQueries({ queryKey: pluginQueryKeys.inventory }),
        ]),
      );
    } catch {
      setSaveError(true);
      setScopeChoices({});
      await Promise.all(
        snapshots.map((snapshot) =>
          queryClient.invalidateQueries({
            queryKey: pluginQueryKeys.permissions(
              snapshot.pluginId,
              snapshot.profileId,
              snapshot.publisherFingerprint,
            ),
          }),
        ),
      );
    } finally {
      setSaving(false);
    }
  };

  const hasPermissions = snapshotQueries.some(
    (query) => (query.data?.requestedPermissions.length ?? 0) > 0,
  );

  const refreshBundleState = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: pluginQueryKeys.inventory }),
      refetchBundleUpdates(),
      refetchBundleRollbacks(),
    ]);
  };

  const checkBundleUpdates = async () => {
    setBundleUpdateOperation('check');
    setBundleUpdateError(null);
    setBundleUpdateResult(null);
    try {
      await commands.pluginBundleUpdateCheck();
      await refreshBundleState();
    } catch {
      setBundleUpdateError('plugins.bundle.updateError.check');
      await refreshBundleState();
    } finally {
      setBundleUpdateOperation(null);
    }
  };

  const reviewBundleUpdate = async (version: string, rollback: boolean) => {
    setBundleUpdateOperation(rollback ? 'rollbackReview' : 'review');
    setBundleUpdateError(null);
    setBundleUpdateResult(null);
    setRollbackConfirmed(false);
    try {
      setBundleUpdateReview(
        rollback
          ? await commands.pluginBundleRollbackReview(bundleId, version)
          : await commands.pluginBundleUpdateReview(bundleId, version),
      );
    } catch {
      setBundleUpdateError('plugins.bundle.updateError.review');
      await refreshBundleState();
    } finally {
      setBundleUpdateOperation(null);
    }
  };

  const approveBundleUpdate = async () => {
    if (!bundleUpdateReview) return;
    setBundleUpdateOperation('approve');
    setBundleUpdateError(null);
    setBundleUpdateResult(null);
    try {
      const outcome = await commands.pluginBundleUpdateApprove(
        bundleUpdateReview.bundleId,
        bundleUpdateReview.candidateVersion,
      );
      setBundleUpdateReview(null);
      setBundleUpdateResult({ version: outcome.version, rollback: false });
      await refreshBundleState();
    } catch {
      // A failed persistence leaves the displayed bundle and its saved controls untouched; the
      // host state is re-read rather than showing an optimistic version.
      setBundleUpdateError('plugins.bundle.updateError.save');
      await refreshBundleState();
    } finally {
      setBundleUpdateOperation(null);
    }
  };

  const approveBundleRollback = async () => {
    if (!bundleUpdateReview || !rollbackConfirmed) return;
    setBundleUpdateOperation('rollbackApprove');
    setBundleUpdateError(null);
    setBundleUpdateResult(null);
    try {
      const outcome = await commands.pluginBundleRollbackApprove(
        bundleUpdateReview.bundleId,
        bundleUpdateReview.candidateVersion,
        true,
      );
      setBundleUpdateReview(null);
      setRollbackConfirmed(false);
      setBundleUpdateResult({ version: outcome.version, rollback: true });
      await refreshBundleState();
    } catch {
      setBundleUpdateError('plugins.bundle.updateError.save');
      await refreshBundleState();
    } finally {
      setBundleUpdateOperation(null);
    }
  };

  return (
    <article
      className={`${settingsCard} flex flex-col gap-compact`}
      aria-label={t('plugins.bundle.details')}
    >
      <div className="flex flex-wrap items-center gap-compact">
        <h3 className={settingsRowTitle}>{bundle.bundleName ?? bundle.name}</h3>
        <span className={settingsPill}>{t('plugins.community')}</span>
        <span className={statusClass(bundle.status)}>{t(`plugins.status.${bundle.status}`)}</span>
      </div>
      <p className={settingsRowDescription}>
        {t('plugins.bundle.version')}: {bundle.bundleVersion ?? bundle.version}
      </p>
      {bundle.bundlePublisherFingerprint && (
        <p className={`${settingsRowDescription} break-all`}>
          {t('plugins.publisherFingerprint')}: {bundle.bundlePublisherFingerprint}
        </p>
      )}
      {profileId === null && (
        <p className={settingsRowDescription} role="status">{t('plugins.signInRequired')}</p>
      )}

      <ul className="flex flex-col gap-compact" aria-label={t('plugins.bundle.components')}>
        {components.map((component, index) => {
          const snapshot = snapshotQueries[index]?.data;
          const canToggle = profileId !== null
            && component.status !== 'incompatible'
            && component.runtimeAvailable !== false;
          const enabled = component.enabled === true;
          return (
            <li key={component.pluginId} className="flex flex-col gap-1 border-t border-border-subtle pt-compact">
              <div className="flex flex-wrap items-center gap-compact">
                <h4 className={settingsRowTitle}>{component.name}</h4>
                <span className={settingsPill}>
                  {t(`plugins.bundle.kind.${component.pluginKind}`)}
                </span>
                <span className={statusClass(component.status)}>
                  {t(`plugins.status.${component.status}`)}
                </span>
                {component.pluginKind === 'extension' && component.runtimeAvailable === false && (
                  <span className={settingsPillWarning} role="status">
                    {t('plugins.bundle.runtimeUnavailable')}
                  </span>
                )}
              </div>
              <p className={settingsRowDescription}>
                {t('plugins.identifier')}: {component.pluginId}
              </p>
              <p className={settingsRowDescription}>
                {t('plugins.import.version')}: {component.version}
              </p>
              {component.publisherFingerprint && (
                <p className={`${settingsRowDescription} break-all`}>
                  {t('plugins.publisherFingerprint')}: {component.publisherFingerprint}
                </p>
              )}
              {profileId !== null && (
                <div className="flex flex-wrap gap-compact">
                  <button
                    type="button"
                    className="rounded-sm border border-border-subtle px-sm py-1 text-sm disabled:opacity-50"
                    disabled={busy || saving || !canToggle}
                    onClick={() => void onToggleEnabled(component.pluginId, !enabled)}
                  >
                    {t(enabled ? 'plugins.disable' : 'plugins.enable')}
                  </button>
                  <span className={settingsRowDescription}>
                    {t(enabled ? 'plugins.bundle.enabledFor' : 'plugins.bundle.disabledFor', {
                      name: component.name,
                    })}
                  </span>
                </div>
              )}
              {profileId !== null && snapshot && (
                <div className="mt-compact flex flex-col gap-compact" aria-label={t('plugins.permissionScopes')}>
                  {snapshot.requestedPermissions.length === 0 ? (
                    <p className={settingsRowDescription}>{t('plugins.bundle.noPermissions')}</p>
                  ) : snapshot.requestedPermissions.map((request) => (
                    <fieldset key={request.permission_id} className="flex flex-col gap-1">
                      <legend className={settingsRowDescription}>
                        {request.permission_id}{request.required ? ` · ${t('plugins.required')}` : ''}
                      </legend>
                      {request.allowed_scopes.map((scope) => {
                        const key = scopeChoiceKey(snapshot, request.permission_id, scope);
                        const effective = snapshot.effectiveGrants.find(
                          (grant) =>
                            grant.permission_id === request.permission_id
                            && JSON.stringify(grant.scope) === JSON.stringify(scope),
                        );
                        const active = effective?.state === 'granted';
                        return (
                          <label key={key} className="flex items-center gap-compact py-1 text-sm">
                            <input
                              type="checkbox"
                              disabled={saving || busy}
                              checked={scopeChoices[key] ?? active}
                              onChange={(event) => {
                                const checked = event.currentTarget.checked;
                                setScopeChoices((current) => ({ ...current, [key]: checked }));
                              }}
                            />
                            <span>
                              {scope.scope}
                              {'category' in scope ? ` · ${scope.category}` : ''}
                              {'value' in scope ? ` · ${scope.value}` : ''}
                              {' · '}
                              {t(`plugins.permissionState.${effective?.state ?? 'denied'}`)}
                            </span>
                          </label>
                        );
                      })}
                    </fieldset>
                  ))}
                  {componentErrors[component.pluginId] && (
                    <p className="text-sm text-warning" role="alert">
                      {component.name}: {componentErrors[component.pluginId]}
                    </p>
                  )}
                </div>
              )}
              {component.pluginId && bundle.bundlePublisherFingerprint && component.publisherFingerprint && (
                <RememberedConfirmations
                  pluginId={component.pluginId}
                  publisherFingerprint={component.publisherFingerprint}
                />
              )}
            </li>
          );
        })}
      </ul>

      {profileId !== null && hasPermissions && (
        <button
          type="button"
          className="mt-compact self-start rounded-sm border border-border-subtle px-sm py-1 text-sm disabled:opacity-50"
          disabled={saving || busy}
          onClick={() => void savePermissions()}
        >
          {t(saving ? 'plugins.bundle.savingPermissions' : 'plugins.bundle.savePermissions')}
        </button>
      )}
      {saved && !saveError && (
        <p className={settingsRowDescription} role="status">{t('plugins.bundle.permissionsSaved')}</p>
      )}
      {saveError && (
        <p className="text-sm text-warning" role="alert">{t('plugins.management.failed')}</p>
      )}

      <section
        className="mt-compact flex flex-col gap-compact border-t border-border-subtle pt-compact"
        aria-label={t('plugins.bundle.updates.title')}
      >
        <h4 className={settingsRowTitle}>{t('plugins.bundle.updates.title')}</h4>
        {profileId === null ? (
          <p className={settingsRowDescription} role="status">{t('plugins.signInRequired')}</p>
        ) : (
          <>
            <button
              type="button"
              className="self-start rounded-sm border border-border-subtle px-sm py-1 text-sm disabled:opacity-50"
              disabled={bundleUpdateOperation !== null}
              onClick={() => void checkBundleUpdates()}
            >
              {t(bundleUpdateOperation === 'check'
                ? 'plugins.bundle.updates.checking'
                : 'plugins.bundle.updates.check')}
            </button>

            {bundleUpdate && bundleUpdate.versions.length > 0 && (
              <div className="flex flex-col gap-1">
                <p className={settingsRowTitle}>
                  {t('plugins.bundle.updates.available', {
                    version: bundleUpdate.installedVersion,
                  })}
                </p>
                <div className="flex flex-wrap gap-1">
                  {bundleUpdate.versions.map((version) => (
                    <button
                      key={version}
                      type="button"
                      className="rounded-sm border border-border-subtle px-sm py-1 text-sm disabled:opacity-50"
                      disabled={bundleUpdateOperation !== null}
                      onClick={() => void reviewBundleUpdate(version, false)}
                    >
                      {t('plugins.bundle.updates.reviewVersion', { version })}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {bundleRollback && bundleRollback.versions.length > 0 && (
              <div className="flex flex-col gap-1">
                <p className={settingsRowTitle}>
                  {t('plugins.bundle.updates.rollbackTitle', {
                    version: bundleRollback.activeVersion,
                  })}
                </p>
                <div className="flex flex-wrap gap-1">
                  {bundleRollback.versions.map((version) => (
                    <button
                      key={version}
                      type="button"
                      className="rounded-sm border border-border-subtle px-sm py-1 text-sm disabled:opacity-50"
                      disabled={bundleUpdateOperation !== null}
                      onClick={() => void reviewBundleUpdate(version, true)}
                    >
                      {t('plugins.bundle.updates.reviewRollback', { version })}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {bundleUpdateOperation !== null && bundleUpdateOperation !== 'check' && (
              <p className={settingsRowDescription} role="status">
                {t('plugins.bundle.updates.working')}
              </p>
            )}
            {bundleUpdateError && (
              <p className="text-sm text-warning" role="alert">{t(bundleUpdateError)}</p>
            )}
            {bundleUpdateResult && (
              <p className={settingsRowDescription} role="status">
                {t(
                  bundleUpdateResult.rollback
                    ? 'plugins.bundle.updates.rollbackComplete'
                    : 'plugins.bundle.updates.updateComplete',
                  { version: bundleUpdateResult.version },
                )}
              </p>
            )}

            {bundleUpdateReview && (
              <article
                className="flex flex-col gap-compact rounded-sm border border-border-subtle p-sm"
                aria-label={t(
                  bundleUpdateReview.isRollback
                    ? 'plugins.bundle.updates.rollbackReviewTitle'
                    : 'plugins.bundle.updates.reviewTitle',
                )}
              >
                <p className={settingsRowTitle}>
                  {bundleUpdateReview.name}: {bundleUpdateReview.installedVersion} →{' '}
                  {bundleUpdateReview.candidateVersion}
                </p>
                <p className={`${settingsRowDescription} break-all`}>
                  {t('plugins.bundle.updates.bundleHash')}: {bundleUpdateReview.bundleSha256}
                </p>
                <p className={settingsRowDescription}>
                  {t('plugins.bundle.updates.permissionDiff')}: +{bundleUpdateReview.addedPermissions.join(', ') || '—'}
                  {' · '}±{bundleUpdateReview.changedPermissions.join(', ') || '—'}
                  {' · '}−{bundleUpdateReview.removedPermissions.join(', ') || '—'}
                </p>
                <ul className="flex flex-col gap-compact" aria-label={t('plugins.bundle.components')}>
                  {bundleUpdateReview.components.map((component) => (
                    <li key={component.pluginId} className="border-t border-border-subtle pt-compact">
                      <div className="flex flex-wrap items-center gap-compact">
                        <span className={settingsRowTitle}>{component.pluginId}</span>
                        <span className={settingsPill}>{t(`plugins.bundle.kind.${component.kind}`)}</span>
                        <span className={settingsPill}>
                          {component.installedVersion} → {component.candidateVersion}
                        </span>
                        <span className={
                          component.availability.status === 'available'
                            ? settingsPillSuccess
                            : settingsPillWarning
                        }>
                          {t(`plugins.bundle.availability.${component.availability.status}`)}
                        </span>
                      </div>
                      {component.availability.status === 'unavailable' && (
                        <p className={settingsRowDescription} role="status">
                          {t(`plugins.bundle.unavailable.${component.availability.reason}`)}
                        </p>
                      )}
                      <p className={settingsRowDescription}>
                        {t('plugins.import.compatibility')}: {t(`plugins.import.${component.compatibility}`)}
                      </p>
                      <p className={settingsRowDescription}>
                        {t('plugins.bundle.updates.permissionDiff')}: +{component.addedPermissions.join(', ') || '—'}
                        {' · '}±{component.changedPermissions.join(', ') || '—'}
                        {' · '}−{component.removedPermissions.join(', ') || '—'}
                      </p>
                      {component.permissionChanges.map((change) => (
                        <p key={change.id} className={settingsRowDescription}>
                          {change.id}: {change.installedScope} → {change.candidateScope}
                        </p>
                      ))}
                    </li>
                  ))}
                </ul>
                {bundleUpdateReview.isRollback && (
                  <label className="flex items-start gap-compact text-sm">
                    <input
                      type="checkbox"
                      checked={rollbackConfirmed}
                      disabled={bundleUpdateOperation !== null}
                      onChange={(event) => setRollbackConfirmed(event.currentTarget.checked)}
                    />
                    <span>{t('plugins.bundle.updates.rollbackConfirm')}</span>
                  </label>
                )}
                <div className="flex flex-wrap gap-compact">
                  <button
                    type="button"
                    className="rounded-sm border border-border-subtle px-sm py-1 text-sm disabled:opacity-50"
                    disabled={bundleUpdateOperation !== null}
                    onClick={() => {
                      setBundleUpdateReview(null);
                      setRollbackConfirmed(false);
                      void commands
                        .pluginBundleUpdateDecline(bundleUpdateReview.bundleId)
                        .catch(() => setBundleUpdateError('plugins.bundle.updateError.review'));
                    }}
                  >
                    {t('plugins.bundle.updates.decline')}
                  </button>
                  <button
                    type="button"
                    className="rounded-sm border border-brand bg-brand px-sm py-1 text-sm text-white disabled:opacity-50"
                    disabled={
                      bundleUpdateOperation !== null
                      || (bundleUpdateReview.isRollback && !rollbackConfirmed)
                    }
                    onClick={() => void (
                      bundleUpdateReview.isRollback ? approveBundleRollback() : approveBundleUpdate()
                    )}
                  >
                    {t(
                      bundleUpdateReview.isRollback
                        ? 'plugins.bundle.updates.rollbackAction'
                        : 'plugins.bundle.updates.updateAction',
                    )}
                  </button>
                </div>
              </article>
            )}
          </>
        )}
      </section>

      {profileId !== null && (
        <div className="mt-compact flex flex-col gap-compact border-t border-border-subtle pt-compact">
          <button
            type="button"
            className="self-start rounded-sm border border-warning px-sm py-1 text-sm disabled:opacity-50"
            disabled={busy}
            onClick={onRequestUninstall}
          >
            {t('plugins.bundle.uninstall')}
          </button>
          <p className={settingsRowDescription}>{t('plugins.bundle.uninstallDescription')}</p>
        </div>
      )}

      {uninstallOutcome && (
        <div className="flex flex-col gap-1 text-sm" role="status">
          {uninstallOutcome.failures.length === 0 ? (
            <p>{t('plugins.bundle.uninstallComplete', {
              retention: t(uninstallOutcome.dataRetention === 'deleted'
                ? 'plugins.bundle.retention.deleted'
                : 'plugins.bundle.retention.kept'),
            })}</p>
          ) : (
            <>
              <p className="text-warning" role="alert">{t('plugins.bundle.uninstallPartial')}</p>
              <ul className="ml-4 list-disc">
                {uninstallOutcome.failures.map((failure) => (
                  <li key={`${failure.pluginId}:${failure.version}`}>
                    {failure.pluginId} {failure.version}: {failure.error}
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}
      {uninstallError && (
        <p className="text-sm text-warning" role="alert">{t('plugins.bundle.uninstallFailed')}</p>
      )}
    </article>
  );
};
