import React, { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { IconAlertTriangle, IconDatabase, IconShieldLock } from '@tabler/icons-react';
import { commands } from '../../api';
import type {
  StorageCleanupCandidate,
  StorageCleanupReport,
  StorageCleanupReview,
} from '../../api';
import { AppModal } from '../../components/AppModal';
import { Button, Pill } from '../../components/ui';
import { pluginQueryKeys } from '../../api/pluginQueryKeys';
import { useI18n } from '../../i18n';
import { formatBytes } from '../ports/catalog/format';
import {
  settingsButtonSecondary,
  settingsRowDescription,
  settingsRowTitle,
} from './settingsClasses';

interface PluginStorageAdminProps {
  profileId: string | null;
}

function kindLabel(kind: string, t: (key: string) => string): string {
  return kind === 'durable_shared'
    ? t('plugins.storageAdmin.kind.durable_shared')
    : t('plugins.storageAdmin.kind.private_persistence');
}

function limitText(
  maxBytes: number | null,
  maxAgeSeconds: number | null,
  locale: string,
  t: (key: string, values?: Record<string, string | number>) => string,
): string {
  const size = maxBytes === null
    ? t('plugins.storageAdmin.noSizeLimit')
    : t('plugins.storageAdmin.sizeLimit', { size: formatBytes(maxBytes, locale) });
  const age = maxAgeSeconds === null
    ? t('plugins.storageAdmin.noAgeLimit')
    : t('plugins.storageAdmin.ageLimit', {
      days: Math.max(1, Math.round(maxAgeSeconds / 86_400)),
    });
  return `${size} · ${age}`;
}

/**
 * Host administration of plugin data on this PC: the managed physical files, their measured
 * allocation against the quota-charged logical bytes, the effective limits and a reviewed,
 * confirmed cleanup of unused or over-limit datasets. The paths are shown only here; plugin IPC
 * never receives them, and the review is a host command with a single-use token.
 */
export const PluginStorageAdmin: React.FC<PluginStorageAdminProps> = ({ profileId }) => {
  const { t, locale } = useI18n();
  const client = useQueryClient();
  const locations = useQuery({
    queryKey: pluginQueryKeys.storageLocations,
    queryFn: commands.pluginStorageLocations,
    enabled: profileId !== null,
  });
  const [review, setReview] = useState<StorageCleanupReview | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [reviewing, setReviewing] = useState(false);
  const [reviewFailed, setReviewFailed] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [applying, setApplying] = useState(false);
  const [applyFailed, setApplyFailed] = useState(false);
  const [report, setReport] = useState<StorageCleanupReport | null>(null);

  const startReview = async () => {
    setReviewing(true);
    setReviewFailed(false);
    setApplyFailed(false);
    setReport(null);
    try {
      const next = await commands.pluginStorageCleanupReview();
      setReview(next);
      setSelected(next.candidates.map((candidate) => candidate.candidateId));
    } catch {
      setReviewFailed(true);
      setReview(null);
      setSelected([]);
    } finally {
      setReviewing(false);
    }
  };

  const toggleCandidate = (candidateId: string) => {
    setSelected((current) => current.includes(candidateId)
      ? current.filter((id) => id !== candidateId)
      : [...current, candidateId]);
  };

  const applyCleanup = async () => {
    if (!review) return;
    setApplying(true);
    setApplyFailed(false);
    try {
      const outcome = await commands.pluginStorageCleanupApply(review.token, selected, true);
      setReport(outcome);
      setReview(null);
      setSelected([]);
      setConfirming(false);
      await client.invalidateQueries({ queryKey: pluginQueryKeys.storageLocations });
    } catch {
      setApplyFailed(true);
      setConfirming(false);
      // A partial or failed apply consumes the review; ask for a fresh one.
      setReview(null);
      setSelected([]);
    } finally {
      setApplying(false);
    }
  };

  const candidateRow = (candidate: StorageCleanupCandidate) => (
    <li key={candidate.candidateId} className="border-t border-border-subtle pt-compact">
      <label className="flex items-start gap-compact text-sm">
        <input
          type="checkbox"
          checked={selected.includes(candidate.candidateId)}
          onChange={() => toggleCandidate(candidate.candidateId)}
        />
        <span>
          <span className={settingsRowTitle}>
            {candidate.pluginId} · {kindLabel(candidate.kind, t)}
          </span>
          <span className={`${settingsRowDescription} block`}>
            {t('plugins.storageAdmin.candidateRecords', {
              records: candidate.records,
              protected: candidate.protectedRecords,
              archived: candidate.archivedRecords,
            })}
          </span>
          <span className={`${settingsRowDescription} block`}>
            {t('plugins.storageAdmin.candidateBytes', {
              logical: formatBytes(candidate.logicalBytes, locale),
              physical: formatBytes(candidate.physicalBytesEstimate, locale),
            })}
          </span>
          <span className={`${settingsRowDescription} block`}>
            {candidate.oldestUpdatedAt === null
              ? t('plugins.storageAdmin.noOldestUse')
              : t('plugins.storageAdmin.oldestUse', {
                date: new Date(candidate.oldestUpdatedAt * 1000).toLocaleDateString(locale),
              })}
          </span>
          <span className={`${settingsRowDescription} block`}>
            {t('plugins.storageAdmin.effectiveLimits')}:{' '}
            {limitText(candidate.maxBytes, candidate.maxAgeSeconds, locale, t)}
          </span>
        </span>
      </label>
    </li>
  );

  return (
    <section
      className="mt-md flex flex-col gap-compact rounded-md border border-border-subtle bg-surface p-md"
      aria-label={t('plugins.storageAdmin.title')}
    >
      <div className="flex flex-wrap items-center gap-compact">
        <IconDatabase size={16} stroke={2} aria-hidden="true" className="text-text-secondary" />
        <h3 className={settingsRowTitle}>{t('plugins.storageAdmin.title')}</h3>
        <Pill tone="neutral" icon={IconShieldLock}>
          {t('plugins.storageAdmin.hostOnly')}
        </Pill>
      </div>
      <p className={settingsRowDescription}>{t('plugins.storageAdmin.description')}</p>
      <p className={settingsRowDescription}>{t('plugins.storageAdmin.pathsHostOnly')}</p>

      {profileId === null ? (
        <p className={settingsRowDescription} role="status">
          {t('plugins.signInRequired')}
        </p>
      ) : locations.isPending ? (
        <p className={settingsRowDescription} role="status">
          {t('plugins.storageAdmin.locationsLoading')}
        </p>
      ) : locations.isError || !locations.data ? (
        <p className="text-xs text-warning" role="alert">
          {t('plugins.storageAdmin.locationsFailed')}
        </p>
      ) : (
        <div className="flex flex-col gap-compact">
          {locations.data.locations.length === 0 ? (
            <p className={settingsRowDescription}>
              {t('plugins.storageAdmin.locationsEmpty')}
            </p>
          ) : (
            <ul className="flex flex-col gap-compact" aria-label={t('plugins.storageAdmin.locations')}>
              {locations.data.locations.map((location) => (
                <li
                  key={`${location.kind}:${location.pluginId}`}
                  className="border-t border-border-subtle pt-compact text-sm"
                >
                  <p className={settingsRowTitle}>
                    {location.pluginId} · {kindLabel(location.kind, t)}
                  </p>
                  <p className={`${settingsRowDescription} break-all font-mono`}>
                    {location.path}
                  </p>
                  <p className={settingsRowDescription}>
                    {t('plugins.storageAdmin.locationBytes', {
                      physical: formatBytes(location.fileBytes, locale),
                      logical: formatBytes(location.logicalBytes, locale),
                    })}
                  </p>
                  <p className={settingsRowDescription}>
                    {location.scope === 'namespace'
                      ? t('plugins.storageAdmin.namespaceOverhead', {
                        overhead: formatBytes(
                          Math.max(0, location.fileBytes - location.logicalBytes),
                          locale,
                        ),
                      })
                      : t('plugins.storageAdmin.poolSharedFile')}
                  </p>
                  <p className={settingsRowDescription}>
                    {t('plugins.storageAdmin.effectiveLimits')}:{' '}
                    {limitText(location.maxBytes, location.maxAgeSeconds, locale, t)}
                  </p>
                  <p className={settingsRowDescription}>
                    {t('plugins.storageAdmin.locationRecords', {
                      records: location.records,
                      protected: location.protectedRecords,
                    })}
                  </p>
                </li>
              ))}
            </ul>
          )}
          {locations.data.pool && (
            <div className="border-t border-border-subtle pt-compact text-sm">
              <p className={settingsRowTitle}>{t('plugins.storageAdmin.poolTitle')}</p>
              <p className={`${settingsRowDescription} break-all font-mono`}>
                {locations.data.pool.path}
              </p>
              <p className={settingsRowDescription}>
                {t('plugins.storageAdmin.poolTotals', {
                  physical: formatBytes(locations.data.pool.fileBytes, locale),
                  logical: formatBytes(locations.data.pool.logicalBytes, locale),
                  overhead: formatBytes(locations.data.pool.overheadBytes, locale),
                  owners: locations.data.pool.owners,
                })}
              </p>
            </div>
          )}
        </div>
      )}

      <div className="border-t border-border-subtle pt-compact">
        <h4 className={settingsRowTitle}>{t('plugins.storageAdmin.cleanupTitle')}</h4>
        <p className={settingsRowDescription}>{t('plugins.storageAdmin.cleanupDescription')}</p>
        <p className={settingsRowDescription}>{t('plugins.storageAdmin.policyReadOnly')}</p>
        <div className="mt-compact flex flex-wrap items-center gap-compact">
          <Button
            size="sm"
            variant="secondary"
            loading={reviewing}
            disabled={reviewing || profileId === null}
            onClick={() => void startReview()}
          >
            {t('plugins.storageAdmin.review')}
          </Button>
          {report === null && (
            <Button
              size="sm"
              variant="secondary"
              disabled={!review || selected.length === 0 || applying}
              onClick={() => {
                setApplyFailed(false);
                setConfirming(true);
              }}
            >
              {t('plugins.storageAdmin.apply')}
            </Button>
          )}
        </div>
        {reviewing && (
          <p className={`${settingsRowDescription} mt-1`} role="status">
            {t('plugins.storageAdmin.reviewing')}
          </p>
        )}
        {reviewFailed && (
          <p className="mt-1 text-xs text-warning" role="alert">
            {t('plugins.storageAdmin.reviewFailed')}
          </p>
        )}
        {review && (
          <div className="mt-compact" aria-label={t('plugins.storageAdmin.candidates')}>
            {review.candidates.length === 0 ? (
              <p className={settingsRowDescription}>
                {t('plugins.storageAdmin.candidatesEmpty')}
              </p>
            ) : (
              <ul className="flex flex-col gap-compact">
                {review.candidates.map(candidateRow)}
              </ul>
            )}
            <p className={`${settingsRowDescription} mt-1`} role="status">
              {t('plugins.storageAdmin.selectedCount', { count: selected.length })}
            </p>
          </div>
        )}
        {applyFailed && (
          <p className="mt-1 text-xs text-warning" role="alert">
            {t('plugins.storageAdmin.applyFailed')}
          </p>
        )}
        {report && (
          <div className="mt-compact border-t border-border-subtle pt-compact" role="status">
            <p className={settingsRowTitle}>{t('plugins.storageAdmin.reportTitle')}</p>
            <p className={settingsRowDescription}>
              {t('plugins.storageAdmin.reportTotals', {
                records: report.removedRecords,
                reclaimed: formatBytes(report.reclaimedBytes, locale),
                skipped: report.skippedProtectedRecords,
              })}
            </p>
            <ul className="mt-compact flex flex-col gap-compact">
              {report.results.map((result) => (
                <li key={result.candidateId} className="text-sm">
                  <span className={settingsRowTitle}>
                    {result.pluginId} · {kindLabel(result.kind, t)}
                  </span>
                  <span className={`${settingsRowDescription} block`}>
                    {result.status === 'completed'
                      ? t('plugins.storageAdmin.resultCompleted', {
                        records: result.removedRecords,
                        reclaimed: formatBytes(result.reclaimedBytes, locale),
                      })
                      : t('plugins.storageAdmin.resultFailed', {
                        error: result.error ?? t('plugins.storageAdmin.unknownError'),
                      })}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}
        {applyFailed && (
          <p className="mt-1 flex items-start gap-compact text-xs text-warning">
            <IconAlertTriangle size={14} stroke={2} aria-hidden="true" className="shrink-0" />
            <span>{t('plugins.storageAdmin.newReviewRequired')}</span>
          </p>
        )}
      </div>

      <AppModal.Root
        opened={confirming}
        onClose={() => !applying && setConfirming(false)}
        closeOnEscape={!applying}
        closeOnClickOutside={!applying}
      >
        <AppModal.Overlay />
        <AppModal.Content className="max-w-md bg-[var(--settings-bg-base)] p-md">
          <AppModal.Header>
            <AppModal.Title>{t('plugins.storageAdmin.confirmTitle')}</AppModal.Title>
            <AppModal.CloseButton
              aria-label={t('plugins.cancel')}
              disabled={applying}
            />
          </AppModal.Header>
          <AppModal.Body>
            <p className={settingsRowDescription}>
              {t('plugins.storageAdmin.confirmBody', { count: selected.length })}
            </p>
            <div className="flex justify-end gap-compact">
              <button
                type="button"
                className={settingsButtonSecondary}
                disabled={applying}
                onClick={() => setConfirming(false)}
              >
                {t('plugins.cancel')}
              </button>
              <button
                type="button"
                className="rounded-sm border border-warning px-sm py-1 text-sm"
                disabled={applying}
                onClick={() => void applyCleanup()}
              >
                {applying
                  ? t('plugins.storageAdmin.applying')
                  : t('plugins.storageAdmin.applyAction')}
              </button>
            </div>
          </AppModal.Body>
        </AppModal.Content>
      </AppModal.Root>
    </section>
  );
};
