import React, { useEffect, useRef, useState } from 'react';
import { IconX } from '@tabler/icons-react';
import { AppModal } from '../../../components/AppModal';
import { commands } from '../../../api';
import { usePortsView, type PortJobView } from '../../../api/ports';
import { useI18n } from '../../../i18n';
import { pushToast } from '../../../store/uiStore';
import { formatI18n } from '../../library/formatters';
import { PortFailureDetails } from './PortFailureDetails';
import { PortSetupModal } from './PortSetupModal';
import { openPortSetup } from './setupStore';

const dialogContentClass =
  'flex w-full flex-col overflow-hidden rounded-md border border-border-subtle bg-secondary shadow-alert';
const secondaryButtonClass =
  'inline-flex items-center justify-center rounded-sm border border-border-subtle bg-raised px-md py-compact text-sm font-medium text-text-default transition-colors hover:bg-surface focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50';
const primaryButtonClass =
  'inline-flex items-center justify-center rounded-sm border border-transparent bg-brand px-md py-compact text-sm font-semibold text-on-primary transition-colors hover:bg-brand-4 focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50';

function resultSignature(job: PortJobView): string {
  return `${job.jobId}:${job.outcome ?? ''}:${job.finishedAt ?? ''}`;
}

/**
 * Mounted once inside `PortsView`.
 *
 * Owns every long-lived PC ports surface: the guided setup modal, the result toasts, the
 * missing-executable follow-up and the failure details with Retry. The host must mount it
 * before the user can open a setup modal through `openPortSetup`.
 */
export const PortSetupHost: React.FC = () => {
  const { t } = useI18n();
  const ports = usePortsView();
  const announced = useRef<Set<string>>(new Set());
  const initialized = useRef(false);
  const [failureJobId, setFailureJobId] = useState<string | null>(null);
  const [executableJobId, setExecutableJobId] = useState<string | null>(null);
  const [choosingExecutable, setChoosingExecutable] = useState(false);
  const [executableNotice, setExecutableNotice] = useState<string | null>(null);

  const failureJob =
    ports.jobs.find((job) => job.jobId === failureJobId) ?? null;
  const executableJob =
    ports.jobs.find((job) => job.jobId === executableJobId) ?? null;

  // One toast per terminal result transition. `seen` keeps results the user already opened
  // out, and the signature keeps repeated snapshots from announcing the same result again.
  // Results that are already terminal in the first snapshot stay in the badge and the detail
  // pane; only results that land while PC ports is open are announced.
  useEffect(() => {
    const firstPass = !initialized.current;
    initialized.current = true;
    for (const job of ports.jobs) {
      if (!job.outcome || job.outcome === 'cancelled') continue;
      const signature = resultSignature(job);
      if (announced.current.has(signature)) continue;
      announced.current.add(signature);
      if (firstPass) {
        // A pending executable choice still needs an answer even if the result predates this
        // mount; failure details and Retry stay in the detail pane.
        if (job.outcome === 'completed' && job.needsExecutable && !job.seen) {
          setExecutableJobId(job.jobId);
        }
        continue;
      }
      if (job.seen) continue;

      if (job.outcome === 'completed') {
        pushToast(
          'success',
          formatI18n(t('Ports::{} was installed.'), job.name),
        );
        if (job.needsExecutable) {
          setExecutableNotice(null);
          setExecutableJobId(job.jobId);
        }
      } else if (job.outcome === 'interrupted') {
        pushToast(
          'danger',
          formatI18n(t('Ports::{} was interrupted.'), job.name),
        );
        setFailureJobId(job.jobId);
      } else {
        pushToast(
          'danger',
          formatI18n(t('Ports::{} could not be installed.'), job.name),
        );
        setFailureJobId(job.jobId);
      }
    }
  }, [ports.jobs, t]);

  const dismissFailure = () => setFailureJobId(null);

  const handleRetry = () => {
    const job = failureJob;
    if (!job) return;
    setFailureJobId(null);
    openPortSetup(job.catalogId, { retryJobId: job.jobId });
  };

  const dismissExecutable = () => {
    setExecutableJobId(null);
    setExecutableNotice(null);
  };

  const handleChooseExecutable = async () => {
    const job = executableJob;
    if (!job || choosingExecutable) return;
    setChoosingExecutable(true);
    setExecutableNotice(null);
    try {
      const path = await commands.portsPickExecutable(job.jobId);
      if (path) {
        pushToast(
          'success',
          formatI18n(t('Ports::Executable chosen: {}'), path),
        );
        setExecutableJobId(null);
      } else {
        setExecutableNotice(t('Ports::No executable was chosen.'));
      }
    } catch {
      setExecutableNotice(t('Ports::No executable was chosen.'));
    } finally {
      setChoosingExecutable(false);
    }
  };

  return (
    <>
      <PortSetupModal />

      {failureJob && (
        <AppModal.Root
          opened
          onClose={dismissFailure}
          centered
          size={560}
          transitionProps={{ duration: 0 }}
        >
          <AppModal.Overlay backgroundOpacity={0.62} />
          <AppModal.Content classNames={{ content: `${dialogContentClass} bg-secondary` }}>
            <AppModal.Header className="flex shrink-0 items-center justify-between gap-md border-b border-border-subtle bg-secondary px-control-inline py-md">
              <AppModal.Title className="text-lg font-semibold text-text-default">
                {failureJob.outcome === 'interrupted'
                  ? t('Ports::Install interrupted')
                  : t('Ports::Install failed')}
              </AppModal.Title>
              <AppModal.CloseButton
                aria-label={t('Close')}
                icon={<IconX size={18} stroke={1.5} />}
                className="rounded-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default"
              />
            </AppModal.Header>

            <AppModal.Body className="flex min-h-0 flex-1 flex-col gap-md overflow-y-auto p-control-inline">
              <p className="text-sm text-text-default">{failureJob.name}</p>
              <PortFailureDetails job={failureJob} />
            </AppModal.Body>

            <div className="flex flex-wrap justify-end gap-compact border-t border-border-subtle px-control-inline py-md">
              <button type="button" onClick={dismissFailure} className={secondaryButtonClass}>
                {t('Close')}
              </button>
              {failureJob.retryable && (
                <button
                  type="button"
                  data-testid="port-failure-retry"
                  onClick={handleRetry}
                  className={primaryButtonClass}
                >
                  {t('Ports::Retry install')}
                </button>
              )}
            </div>
          </AppModal.Content>
        </AppModal.Root>
      )}

      {executableJob && (
        <AppModal.Root
          opened
          onClose={dismissExecutable}
          centered
          size={520}
          transitionProps={{ duration: 0 }}
        >
          <AppModal.Overlay backgroundOpacity={0.62} />
          <AppModal.Content classNames={{ content: `${dialogContentClass} bg-secondary` }}>
            <AppModal.Header className="flex shrink-0 items-center justify-between gap-md border-b border-border-subtle bg-secondary px-control-inline py-md">
              <AppModal.Title className="text-lg font-semibold text-text-default">
                {t('Ports::Choose the executable')}
              </AppModal.Title>
              <AppModal.CloseButton
                aria-label={t('Close')}
                icon={<IconX size={18} stroke={1.5} />}
                className="rounded-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default"
              />
            </AppModal.Header>

            <AppModal.Body className="flex min-h-0 flex-1 flex-col gap-md overflow-y-auto p-control-inline">
              <p className="break-words text-sm text-text-default">
                {t(
                  "Ports::The install finished, but none of the expected executables was found. Choose the port's executable inside the install folder.",
                )}
              </p>
              <p className="break-words text-sm font-medium text-text-default">{executableJob.name}</p>
              {executableNotice && (
                <p
                  data-testid="port-executable-notice"
                  role="alert"
                  className="rounded-sm border border-warning bg-warning-subtle px-sm py-compact text-xs text-text-default"
                >
                  {executableNotice}
                </p>
              )}
            </AppModal.Body>

            <div className="flex flex-wrap justify-end gap-compact border-t border-border-subtle px-control-inline py-md">
              <button
                type="button"
                onClick={dismissExecutable}
                className={secondaryButtonClass}
              >
                {t('Ports::Not now')}
              </button>
              <button
                type="button"
                data-testid="port-pick-executable"
                onClick={() => {
                  void handleChooseExecutable();
                }}
                disabled={choosingExecutable}
                className={primaryButtonClass}
              >
                {t('Ports::Choose executable')}
              </button>
            </div>
          </AppModal.Content>
        </AppModal.Root>
      )}
    </>
  );
};
