import React, { useEffect, useMemo, useState } from 'react';
import { IconCopy } from '@tabler/icons-react';
import type { PortJobView } from '../../../api/ports';
import { copyText } from '../../../diagnostics/crashCopy';
import { useI18n } from '../../../i18n';

/** The host caps captured tool output at 64 KiB (D8); the UI caps what it renders again. */
const MAX_DISPLAY_CHARS = 64 * 1024;

export interface PortFailureDetailsProps {
  job: PortJobView;
}

/**
 * Failure reason and captured extractor output for one install job.
 *
 * The output is rendered in a capped, scrollable block and the whole diagnostic text can be
 * copied for a bug report. It never takes focus on its own.
 */
export const PortFailureDetails: React.FC<PortFailureDetailsProps> = ({ job }) => {
  const { t } = useI18n();
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'failed'>('idle');

  const reason = job.failure?.message?.trim() ?? '';
  const output = job.failure?.toolOutput?.trim() ?? '';
  const displayedOutput = useMemo(
    () => (output.length > MAX_DISPLAY_CHARS ? output.slice(0, MAX_DISPLAY_CHARS) : output),
    [output],
  );

  useEffect(() => {
    setCopyState('idle');
  }, [job.jobId]);

  const handleCopy = async () => {
    const text = [reason, displayedOutput].filter(Boolean).join('\n\n');
    const ok = text ? await copyText(text) : false;
    setCopyState(ok ? 'copied' : 'failed');
  };

  return (
    <div data-testid="port-failure-details" className="flex min-h-0 flex-col gap-md">
      <p className="break-words text-sm text-text-default">
        {reason || t('Ports::The install stopped without a reason.')}
      </p>

      <div className="flex min-h-0 flex-col gap-compact">
        <div className="flex flex-wrap items-center justify-between gap-compact">
          <h3 className="text-xs font-semibold tracking-wide text-text-dimmed uppercase">
            {t('Ports::Tool output')}
          </h3>
          <button
            type="button"
            data-testid="port-failure-copy"
            onClick={handleCopy}
            disabled={!reason && !displayedOutput}
            className="inline-flex items-center gap-1.5 rounded-sm border border-border-subtle bg-raised px-compact py-1 text-xs font-medium text-text-default transition-colors hover:bg-surface focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
          >
            <IconCopy size={14} stroke={1.75} aria-hidden="true" />
            {t('Ports::Copy details')}
          </button>
        </div>
        <pre
          data-testid="port-failure-output"
          className="max-h-56 min-h-[72px] overflow-auto rounded-sm border border-border-subtle bg-body p-compact font-mono text-xs leading-normal break-words whitespace-pre-wrap text-text-dimmed"
        >
          {displayedOutput || t('Ports::No tool output was captured.')}
        </pre>
        <span role="status" aria-live="polite" className="text-xs text-text-dimmed">
          {copyState === 'copied'
            ? t('Copied to the clipboard.')
            : copyState === 'failed'
              ? t('Could not copy to the clipboard.')
              : ''}
        </span>
      </div>
    </div>
  );
};
