import React from 'react';
import { IconRefresh } from '@tabler/icons-react';
import type { PortEntryView, PortJobView } from '../../../api/ports';
import { usePortProgress } from '../../../api/ports';
import { commands } from '../../../api/commands';
import { useI18n } from '../../../i18n';
import { formatI18n } from '../../library/formatters';
import { PortFailureDetails, openPortSetup } from '../setup';
import { formatBytes, isRunningJob, jobPhaseLabel } from '../catalog/format';

export interface PortProgressRowProps {
  entry: PortEntryView;
  job: PortJobView;
}

const terminal = new Set(['cancelled', 'failed', 'interrupted']);

/**
 * The install row of an entry: the current phase, the coalesced progress stream for that
 * job, Cancel while the job is interruptible, and the shared failure details plus Retry
 * for a terminal job.
 */
export const PortProgressRow: React.FC<PortProgressRowProps> = ({ entry, job }) => {
  const { t, locale } = useI18n();
  const progress = usePortProgress(job.jobId);
  const running = isRunningJob(job);
  const failed = job.phase === 'failed' || job.phase === 'interrupted';

  const total = progress?.bytesTotal ?? 0;
  const done = progress?.bytesDone ?? 0;
  const hasFigure = total > 0;
  const percent = hasFigure ? Math.max(0, Math.min(100, Math.floor((done / total) * 100))) : 0;
  const canCancel = job.interruptible && running;
  const canRetry = job.retryable && terminal.has(job.phase);

  const containerClass = failed
    ? 'border-danger/40 bg-danger-subtle'
    : job.phase === 'completed'
      ? 'border-success/40 bg-success-subtle'
      : 'border-border-subtle bg-raised';

  return (
    <div
      data-testid="ports-progress-row"
      data-phase={job.phase}
      className={`flex flex-col gap-compact rounded-sm border px-sm py-compact ${containerClass}`}
    >
      <div className="flex items-start justify-between gap-2">
        <span
          data-testid="ports-progress-phase"
          className={`text-xs font-medium ${
            failed ? 'text-danger' : job.phase === 'completed' ? 'text-success' : 'text-text-default'
          }`}
        >
          {jobPhaseLabel(job, entry, t)}
        </span>
        {hasFigure && running && (
          <span className="shrink-0 text-[11px] tabular-nums text-text-dimmed">
            {formatI18n(
              t('Ports::{} of {}'),
              formatBytes(done, locale),
              formatBytes(total, locale),
            )}
          </span>
        )}
      </div>

      {running && hasFigure && (
        <div
          data-testid="ports-progress-bar"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
          aria-label={jobPhaseLabel(job, entry, t)}
          className="h-1.5 w-full overflow-hidden rounded-full bg-surface"
        >
          <div
            className="h-full rounded-full bg-brand transition-[width] duration-150"
            style={{ width: `${percent}%` }}
          />
        </div>
      )}

      {job.phase === 'cancelling' && (
        <div className="h-1.5 w-full overflow-hidden rounded-full bg-surface">
          <div className="h-full w-1/3 rounded-full bg-warning" />
        </div>
      )}

      {(job.failure || failed) && <PortFailureDetails job={job} />}

      {(canCancel || canRetry) && (
        <div className="flex flex-wrap items-center gap-compact">
          {canCancel && (
            <button
              type="button"
              data-testid="ports-progress-cancel"
              onClick={() => commands.portsCancel(job.jobId)}
              className="rounded-xs border border-border-subtle bg-transparent px-2.5 py-1 text-xs text-text-default transition-colors hover:bg-surface"
            >
              {t('Library::Cancel')}
            </button>
          )}
          {canRetry && (
            <button
              type="button"
              data-testid="ports-progress-retry"
              onClick={() => openPortSetup(entry.catalogId, { retryJobId: job.jobId })}
              className="flex items-center gap-1.5 rounded-xs border border-brand bg-transparent px-2.5 py-1 text-xs font-medium text-brand transition-colors hover:bg-accent-subtle"
            >
              <IconRefresh size={12} stroke={1.75} aria-hidden />
              {t('Ports::Retry install')}
            </button>
          )}
        </div>
      )}
    </div>
  );
};
