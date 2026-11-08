import React, { useEffect, useMemo, useState } from 'react';
import { IconLoader2, IconPackage, IconX } from '@tabler/icons-react';
import { FloatingModal } from '../../../components/FloatingModal';
import { commands } from '../../../api';
import {
  usePortDetail,
  usePortProgress,
  usePortsView,
  type PortEntryView,
  type PortInstallBlockReason,
  type PortPackageOption,
  type PortProgress,
  type PortProgressPhase,
  type PortReleaseView,
  type PortSetupSlot,
  type PortSuppliedChoice,
} from '../../../api/ports';
import { useI18n } from '../../../i18n';
import { formatI18n } from '../../library/formatters';
import { formatPortDate } from '../catalog/format';
import { closePortSetup, createSetupId, usePortSetupStore } from './setupStore';

interface SlotSelection {
  mode: 'direct' | 'package';
  /** Direct pick, or the base package in package mode. */
  path: string | null;
  /** Update package in package mode; null when the extractor needs none. */
  updatePath: string | null;
}

interface SlotPickError {
  message: string;
  acceptedLabels: string[];
}

const cardClass =
  'flex flex-col gap-sm rounded-sm border border-border-subtle bg-secondary p-md';
const sectionTitleClass = 'text-sm font-semibold text-text-default';
const hintClass = 'text-xs text-text-dimmed';
const secondaryButtonClass =
  'inline-flex items-center justify-center gap-1.5 rounded-sm border border-border-subtle bg-raised px-md py-compact text-sm font-medium text-text-default transition-colors hover:bg-surface focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50';
const primaryButtonClass =
  'inline-flex items-center justify-center gap-1.5 rounded-sm border border-transparent bg-brand px-md py-compact text-sm font-semibold text-on-primary transition-colors hover:bg-brand-4 focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50';
const chipClass =
  'inline-flex items-center rounded-full border border-border-subtle bg-raised px-2 py-0.5 text-[11px] font-medium text-text-dimmed';
const chipStrongClass =
  'inline-flex items-center rounded-full border border-brand bg-accent-subtle px-2 py-0.5 text-[11px] font-semibold text-text-default';

function blockReasonMessage(
  reason: PortInstallBlockReason,
  t: (key: string) => string,
): string {
  switch (reason) {
    case 'withdrawn':
      return t('Ports::This port was withdrawn upstream.');
    case 'noVerifiedRelease':
      return t('Ports::No verified release is available, so Install is unavailable.');
    case 'installerOnly':
      return t('Ports::The Windows download is an installer, which GameNode does not run.');
    case 'noWindowsAsset':
      return t('Ports::No Windows download is available.');
    case 'missingAsset':
      return t('Ports::The Windows download is missing from the release.');
    case 'noExecutable':
      return t("Ports::None of the port's expected executables is available.");
    default:
      return t('Ports::Install is unavailable for this entry.');
  }
}

function progressLabel(
  phase: PortProgressPhase,
  t: (key: string) => string,
): string {
  switch (phase) {
    case 'hashing':
      return t('Ports::Hashing the selection…');
    case 'extractingPackage':
      return t('Ports::Extracting the package…');
    case 'copying':
      return t('Ports::Copying the selection…');
    default:
      return t('Ports::Checking the selection…');
  }
}

function releaseFromEntry(entry: PortEntryView | null): PortReleaseView | null {
  if (!entry?.version) return null;
  return {
    version: entry.version,
    releasedAt: null,
    verified: entry.verified,
    withdrawn: entry.withdrawn,
    assetName: null,
    assetFormat: null,
    sizeBytes: null,
    checksumVerified: entry.verified,
    requiresChecksumConfirmation: false,
    malwareFlag: null,
  };
}

function selectionIsComplete(
  slot: PortSetupSlot,
  selection: SlotSelection | undefined,
): boolean {
  if (!selection?.path) return false;
  if (selection.mode === 'direct') return true;
  if (!slot.packageOption) return false;
  return !slot.packageOption.requiresUpdate || Boolean(selection.updatePath);
}

function buildSupplied(
  slots: PortSetupSlot[],
  selections: Record<string, SlotSelection | undefined>,
): PortSuppliedChoice[] {
  const supplied: PortSuppliedChoice[] = [];
  for (const slot of slots) {
    const selection = selections[slot.id];
    if (!selection?.path) continue;
    if (selection.mode === 'package') {
      if (!selectionIsComplete(slot, selection)) continue;
      supplied.push({ slotId: slot.id, path: selection.path, package: true });
      if (selection.updatePath) {
        supplied.push({
          slotId: slot.id,
          path: selection.updatePath,
          package: true,
          update: true,
        });
      }
    } else {
      supplied.push({ slotId: slot.id, path: selection.path });
    }
  }
  return supplied;
}

interface RequirementRowProps {
  slot: PortSetupSlot;
  selection: SlotSelection | undefined;
  error: SlotPickError | undefined;
  busy: boolean;
  /** True while any slot is being hashed, so no second pick can start. */
  anyBusy: boolean;
  progress: PortProgress | null;
  onPick: (slot: PortSetupSlot, kind: 'file' | 'folder' | 'package', update?: boolean) => void;
  onModeChange: (slot: PortSetupSlot, mode: 'direct' | 'package') => void;
  onClear: (slotId: string) => void;
}

const RequirementRow: React.FC<RequirementRowProps> = ({
  slot,
  selection,
  error,
  busy,
  anyBusy,
  progress,
  onPick,
  onModeChange,
  onClear,
}) => {
  const { t } = useI18n();
  const packageMode = selection?.mode === 'package';
  const packageOption: PortPackageOption | null = slot.packageOption;
  const percent =
    progress && progress.bytesTotal > 0
      ? Math.min(100, Math.round((progress.bytesDone / progress.bytesTotal) * 100))
      : null;

  return (
    <div
      data-testid={`port-setup-slot-${slot.id}`}
      data-slot-state={busy ? 'busy' : selection?.path ? 'chosen' : 'empty'}
      className={cardClass}
    >
      <div className="flex flex-wrap items-start justify-between gap-compact">
        <div className="min-w-0 flex-1">
          <h4 className="break-words text-sm font-semibold text-text-default">{slot.label}</h4>
          {slot.prompt && <p className="mt-0.5 break-words text-xs text-text-dimmed">{slot.prompt}</p>}
        </div>
        <span className={slot.required ? chipStrongClass : chipClass}>
          {slot.required ? t('Ports::Required') : t('Ports::Optional')}
        </span>
      </div>

      {slot.acceptedLabels.length > 0 && (
        <p className="break-words text-xs text-text-dimmed">
          {formatI18n(t('Ports::Accepted: {}'), slot.acceptedLabels.join(', '))}
        </p>
      )}
      {!packageMode && slot.destination && (
        <p className="break-words text-xs text-text-dimmed">
          {formatI18n(t('Ports::Placed at {}'), slot.destination)}
        </p>
      )}
      {!packageMode && !slot.destination && slot.argument && (
        <p className="break-words text-xs text-text-dimmed">
          {formatI18n(t('Ports::Passed as {}'), slot.argument.name)}
        </p>
      )}

      {packageMode && packageOption ? (
        <div className="flex flex-col gap-compact rounded-sm border border-border-subtle bg-body p-sm">
          <div className="flex min-w-0 items-center gap-compact text-xs text-text-dimmed">
            <IconPackage size={15} stroke={1.75} aria-hidden="true" />
            <span className="break-words">
              {formatI18n(t('Ports::Extracted with {}'), packageOption.toolName)}
            </span>
          </div>
          {packageOption.requiresUpdate && (
            <p className="text-xs text-text-dimmed">
              {formatI18n(
                t('Ports::Update {} is required.'),
                packageOption.updateVersion ?? '?',
              )}
            </p>
          )}

          <div className="flex flex-col gap-1">
            <span className="text-xs text-text-dimmed">
              {t('Ports::Base package')}
            </span>
            <div className="flex flex-wrap items-center gap-compact">
              <span className="min-w-0 flex-1 break-all text-xs text-text-default">
                {selection?.path ?? t('Ports::Not chosen yet')}
              </span>
              <button
                type="button"
                data-testid={`port-setup-pick-${slot.id}-base`}
                aria-label={`${t('Ports::Choose base package')}: ${slot.label}`}
                onClick={() => onPick(slot, 'package', false)}
                disabled={anyBusy}
                className={secondaryButtonClass}
              >
                {t('Ports::Choose base package')}
              </button>
            </div>
          </div>

          {packageOption.requiresUpdate && (
            <div className="flex flex-col gap-1">
              <span className="text-xs text-text-dimmed">
                {t('Ports::Update package')}
              </span>
              <div className="flex flex-wrap items-center gap-compact">
                <span className="min-w-0 flex-1 break-all text-xs text-text-default">
                  {selection?.updatePath ?? t('Ports::Not chosen yet')}
                </span>
                <button
                  type="button"
                  data-testid={`port-setup-pick-${slot.id}-update`}
                  aria-label={`${t('Ports::Choose update package')}: ${slot.label}`}
                  onClick={() => onPick(slot, 'package', true)}
                  disabled={anyBusy}
                  className={secondaryButtonClass}
                >
                  {t('Ports::Choose update package')}
                </button>
              </div>
            </div>
          )}
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-compact">
          <span className="min-w-0 flex-1 break-all text-xs text-text-default">
            {selection?.path ?? t('Ports::Not chosen yet')}
          </span>
          <button
            type="button"
            data-testid={`port-setup-pick-${slot.id}`}
            aria-label={`${slot.kind === 'folder' ? t('Ports::Choose folder') : t('Ports::Choose file')}: ${slot.label}`}
            onClick={() => onPick(slot, slot.kind)}
            disabled={anyBusy}
            className={secondaryButtonClass}
          >
            {slot.kind === 'folder' ? t('Ports::Choose folder') : t('Ports::Choose file')}
          </button>
          {selection?.path && (
            <button
              type="button"
              data-testid={`port-setup-clear-${slot.id}`}
              aria-label={`${t('Ports::Remove')}: ${slot.label}`}
              onClick={() => onClear(slot.id)}
              disabled={anyBusy}
              className={secondaryButtonClass}
            >
              {t('Ports::Remove')}
            </button>
          )}
        </div>
      )}

      {packageOption && (
        <button
          type="button"
          data-testid={`port-setup-mode-${slot.id}`}
          aria-label={`${packageMode ? t('Ports::Use a folder instead') : t('Ports::Use packages instead')}: ${slot.label}`}
          onClick={() => onModeChange(slot, packageMode ? 'direct' : 'package')}
          disabled={anyBusy}
          className="self-start text-xs font-medium text-brand underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
        >
          {packageMode ? t('Ports::Use a folder instead') : t('Ports::Use packages instead')}
        </button>
      )}

      {busy && (
        <div
          data-testid={`port-setup-progress-${slot.id}`}
          role="status"
          aria-live="polite"
          className="flex flex-col gap-1 rounded-sm border border-border-subtle bg-body p-sm"
        >
          <div className="flex items-center gap-compact text-xs text-text-default">
            <IconLoader2
              size={14}
              stroke={1.75}
              className="animate-spin text-brand"
              aria-hidden="true"
            />
            <span>{progressLabel(progress?.phase ?? 'hashing', t)}</span>
            {percent != null && (
              <span className="ml-auto text-text-dimmed">
                {formatI18n(t('Ports::{}%'), percent)}
              </span>
            )}
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-raised" aria-hidden="true">
            <div
              className="h-full rounded-full bg-brand transition-[width] duration-200"
              style={{ width: `${percent ?? 15}%` }}
            />
          </div>
        </div>
      )}

      {error && (
        <div
          data-testid={`port-setup-rejected-${slot.id}`}
          role="alert"
          className="rounded-sm border border-danger bg-danger-subtle px-sm py-compact text-xs text-text-default"
        >
          <p className="break-words">{error.message || t('Ports::The selection was rejected.')}</p>
          {error.acceptedLabels.length > 0 && (
            <p className="mt-1 break-words text-text-dimmed">
              {formatI18n(
                t('Ports::Accepted versions: {}'),
                error.acceptedLabels.join(', '),
              )}
            </p>
          )}
        </div>
      )}
    </div>
  );
};

/**
 * Guided setup for one catalog entry.
 *
 * Steps: confirm the release, choose the parent folder, satisfy each recipe slot (with the
 * console-package alternative) and accept the safety confirmations. Start closes the modal and
 * hands the request to the host; the job keeps running and its progress lives in PC ports.
 */
export const PortSetupModal: React.FC = () => {
  const { t, locale } = useI18n();
  const { open, catalogId, retryJobId, session } = usePortSetupStore();
  const ports = usePortsView();
  const detailQuery = usePortDetail(open ? catalogId : null);

  const entry = catalogId
    ? (ports.entries.find((candidate) => candidate.catalogId === catalogId) ?? null)
    : null;
  const detail = detailQuery.data ?? null;
  const requirements = detail?.requirements ?? entry?.requirements ?? [];
  const release = detail?.selectedRelease ?? releaseFromEntry(entry);

  const [parentFolder, setParentFolder] = useState<string | null>(
    () => ports.preferences.lastParentFolder,
  );
  const [selections, setSelections] = useState<
    Record<string, SlotSelection | undefined>
  >({});
  const [pickErrors, setPickErrors] = useState<Record<string, SlotPickError | undefined>>({});
  const [busySlot, setBusySlot] = useState<string | null>(null);
  const [acceptUnverified, setAcceptUnverified] = useState(false);
  const [acceptMalware, setAcceptMalware] = useState(false);
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  // A fresh id per setup session so hashing progress never leaks between sessions.
  const setupId = useMemo(createSetupId, [session]);
  const progress = usePortProgress(open ? setupId : null);

  // The snapshot may arrive after the modal opened; offer the remembered folder then.
  useEffect(() => {
    if (!parentFolder && ports.preferences.lastParentFolder) {
      setParentFolder(ports.preferences.lastParentFolder);
    }
  }, [parentFolder, ports.preferences.lastParentFolder]);

  // A new session (another entry) starts from a clean form and a fresh progress key.
  useEffect(() => {
    setSelections({});
    setPickErrors({});
    setBusySlot(null);
    setAcceptUnverified(false);
    setAcceptMalware(false);
    setStartError(null);
    setStarting(false);
  }, [session]);

  if (!open) return null;

  const withdrawn = entry?.withdrawn === true || detail?.selectedRelease?.withdrawn === true;
  const blockReason: PortInstallBlockReason | null = !entry
    ? null
    : withdrawn
      ? 'withdrawn'
      : entry.installable
        ? null
        : (entry.installBlockReason ?? 'noVerifiedRelease');
  const prerelease =
    entry?.prerelease === true || (release != null && !release.verified);
  const releaseDate = formatPortDate(release?.releasedAt, locale);
  const missingRequired = retryJobId
    ? []
    : requirements.filter(
        (slot) => slot.required && !selectionIsComplete(slot, selections[slot.id]),
      );
  const needsChecksumGate =
    release?.requiresChecksumConfirmation === true && !acceptUnverified;
  const needsMalwareGate = Boolean(release?.malwareFlag) && !acceptMalware;
  const canStart =
    Boolean(entry) &&
    blockReason == null &&
    Boolean(parentFolder) &&
    missingRequired.length === 0 &&
    !needsChecksumGate &&
    !needsMalwareGate &&
    !starting &&
    busySlot == null;

  const handleClose = () => {
    closePortSetup();
  };

  const handlePick = async (
    slot: PortSetupSlot,
    kind: 'file' | 'folder' | 'package',
    update = false,
  ) => {
    if (busySlot) return;
    setPickErrors((previous) => {
      if (!previous[slot.id]) return previous;
      const next = { ...previous };
      delete next[slot.id];
      return next;
    });
    setBusySlot(slot.id);
    try {
      const result = await commands.portsPickSupplied({
        setupId,
        catalogId: entry?.catalogId ?? null,
        slotId: slot.id,
        kind,
        ...(kind === 'package' ? { update } : {}),
        ...(kind === 'package' ? {} : { extensions: slot.extensions }),
      });
      if (!result) return;
      const status = result.status;
      if (status === 'accepted') {
        setSelections((previous) => {
          const current = previous[slot.id];
          if (kind === 'package') {
            return {
              ...previous,
              [slot.id]: {
                mode: 'package',
                path: update ? (current?.path ?? null) : result.path,
                updatePath: update ? result.path : (current?.updatePath ?? null),
              },
            };
          }
          return {
            ...previous,
            [slot.id]: { mode: 'direct', path: result.path, updatePath: null },
          };
        });
      } else {
        const rejected = status.rejected;
        setPickErrors((previous) => ({
          ...previous,
          [slot.id]: {
            message: rejected.message || t('Ports::The selection was rejected.'),
            acceptedLabels:
              rejected.acceptedLabels.length > 0
                ? rejected.acceptedLabels
                : slot.acceptedLabels,
          },
        }));
      }
    } catch {
      setPickErrors((previous) => ({
        ...previous,
        [slot.id]: {
          message: t('Ports::The file could not be checked.'),
          acceptedLabels: slot.acceptedLabels,
        },
      }));
    } finally {
      setBusySlot(null);
    }
  };

  const handleModeChange = (slot: PortSetupSlot, mode: 'direct' | 'package') => {
    setPickErrors((previous) => {
      if (!previous[slot.id]) return previous;
      const next = { ...previous };
      delete next[slot.id];
      return next;
    });
    setSelections((previous) => {
      if (mode === 'direct') {
        const current = previous[slot.id];
        return {
          ...previous,
          [slot.id]: {
            mode: 'direct',
            path: current?.mode === 'direct' ? current.path : null,
            updatePath: null,
          },
        };
      }
      return {
        ...previous,
        [slot.id]: {
          mode: 'package',
          path: previous[slot.id]?.mode === 'package' ? previous[slot.id]?.path ?? null : null,
          updatePath:
            previous[slot.id]?.mode === 'package'
              ? previous[slot.id]?.updatePath ?? null
              : null,
        },
      };
    });
  };

  const handleClear = (slotId: string) => {
    setSelections((previous) => {
      const next = { ...previous };
      delete next[slotId];
      return next;
    });
  };

  const handleChooseParent = async () => {
    try {
      const folder = await commands.portsPickParent(parentFolder);
      if (!folder) return;
      setParentFolder(folder);
      void commands
        .portsSetPreferences({ lastParentFolder: folder })
        .catch(() => undefined);
    } catch {
      // The native picker is unavailable in a plain browser; the row keeps the old value.
    }
  };

  const handleStart = async () => {
    if (!canStart || !parentFolder) return;
    setStarting(true);
    setStartError(null);
    try {
      const jobId = await commands.portsInstall({
        catalogId: catalogId as string,
        parentFolder,
        supplied: buildSupplied(requirements, selections),
        ...(acceptUnverified ? { acceptUnverifiedChecksum: true } : {}),
        ...(acceptMalware ? { acceptMalwareFlag: true } : {}),
        ...(retryJobId ? { retryJobId } : {}),
      });
      if (!jobId) {
        setStartError(t('Ports::The install could not start.'));
        return;
      }
      handleClose();
    } catch {
      setStartError(t('Ports::The install could not start.'));
    } finally {
      setStarting(false);
    }
  };

  const title = entry
    ? formatI18n(t('Ports::Set up {}'), entry.name)
    : t('Ports::Guided setup');

  return (
    <FloatingModal.Root
      opened
      onClose={handleClose}
      geometryKey={`port-setup-${catalogId ?? 'entry'}`}
      defaultSize={{ width: 760, height: 660 }}
      minSize={{ width: 560, height: 440 }}
      zIndex={1000}
    >
      <FloatingModal.Overlay backgroundOpacity={0.62} />
      <FloatingModal.Content className="flex flex-col bg-chrome">
        <FloatingModal.Header className="bg-chrome px-control-inline py-md">
          <div className="min-w-0">
            <FloatingModal.Title
              title={title}
              className="line-clamp-2 break-words text-lg font-semibold text-text-default"
            >
              {title}
            </FloatingModal.Title>
            <div className="line-clamp-2 break-words text-xs text-text-dimmed">
              {t('Ports::GameNode downloads the port. You supply your own game files.')}
            </div>
          </div>
          <FloatingModal.CloseButton
            icon={<IconX size={18} stroke={1.5} />}
            className="rounded-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default"
          />
        </FloatingModal.Header>

        <FloatingModal.Body className="gap-md p-control-inline">
          {!entry && (
            <div
              data-testid="port-setup-missing-entry"
              className="rounded-sm border border-danger bg-danger-subtle px-md py-sm text-sm text-text-default"
            >
              {t('Ports::This entry is no longer in the catalog.')}
            </div>
          )}

          {/* Release confirmation */}
          <section data-testid="port-setup-release" className={cardClass}>
            <div className="flex flex-wrap items-start justify-between gap-sm">
              <div className="min-w-0 flex-1">
                <h3 className={sectionTitleClass}>{t('Ports::Release')}</h3>
                <p
                  data-testid="port-setup-release-version"
                  className="break-words text-sm text-text-default"
                >
                  {release?.version ?? t('Ports::No release information is available yet.')}
                </p>
                {releaseDate && (
                  <p className={hintClass}>
                    {formatI18n(t('Ports::Released: {}'), releaseDate)}
                  </p>
                )}
              </div>
              <div className="flex flex-wrap gap-1.5">
                {release?.verified && (
                  <span data-testid="port-setup-verified" className={chipStrongClass}>
                    {t('Ports::Verified release')}
                  </span>
                )}
                {prerelease && (
                  <span data-testid="port-setup-prerelease" className={chipClass}>
                    {t('Ports::Pre-release')}
                  </span>
                )}
                {withdrawn && (
                  <span
                    data-testid="port-setup-withdrawn"
                    className="inline-flex items-center rounded-full border border-danger bg-danger-subtle px-2 py-0.5 text-[11px] font-semibold text-text-default"
                  >
                    {t('Ports::Withdrawn')}
                  </span>
                )}
              </div>
            </div>

            {blockReason && (
              <p
                data-testid="port-setup-blocked"
                role="alert"
                className="break-words rounded-sm border border-warning bg-warning-subtle px-sm py-compact text-xs text-text-default"
              >
                {blockReasonMessage(blockReason, t)}
                {detail?.withdrawalReason ? ` ${detail.withdrawalReason}` : ''}
              </p>
            )}

            {retryJobId && (
              <p data-testid="port-setup-retry-note" className={hintClass}>
                {t('Ports::The files you chose last time are reused. Pick a slot only to replace one.')}
              </p>
            )}
          </section>

          {/* Parent folder */}
          <section data-testid="port-setup-folder" className={cardClass}>
            <div className="flex flex-wrap items-center justify-between gap-sm">
              <div className="min-w-0 flex-1">
                <h3 className={sectionTitleClass}>{t('Ports::Parent folder')}</h3>
                <p
                  data-testid="port-setup-folder-value"
                  className="break-all text-sm text-text-default"
                >
                  {parentFolder ?? t('Ports::No folder chosen')}
                </p>
              </div>
              <button
                type="button"
                data-testid="port-setup-choose-folder"
                onClick={handleChooseParent}
                className={secondaryButtonClass}
              >
                {parentFolder ? t('Ports::Change folder') : t('Ports::Choose folder')}
              </button>
            </div>
            <p className={hintClass}>
              {t('Ports::The port is installed in a new subfolder inside this folder.')}
            </p>
          </section>

          {/* Recipe slots: release-only entries show just the release and folder steps. */}
          {requirements.length > 0 && (
            <section data-testid="port-setup-requirements" className="flex flex-col gap-sm">
              <h3 className={sectionTitleClass}>{t('Ports::Files you need to supply')}</h3>
              {requirements.map((slot) => (
                <RequirementRow
                  key={slot.id}
                  slot={slot}
                  selection={selections[slot.id]}
                  error={pickErrors[slot.id]}
                  busy={busySlot === slot.id}
                  anyBusy={busySlot != null}
                  progress={busySlot === slot.id ? progress : null}
                  onPick={(target, kind, update) => {
                    void handlePick(target, kind, update);
                  }}
                  onModeChange={handleModeChange}
                  onClear={handleClear}
                />
              ))}
            </section>
          )}

          {/* Safety confirmations */}
          {release?.requiresChecksumConfirmation && (
            <section data-testid="port-setup-checksum-gate" className={cardClass}>
              <h3 className={sectionTitleClass}>
                {t('Ports::This download only has an unverified checksum.')}
              </h3>
              <label className="flex cursor-pointer items-start gap-2.5 text-sm text-text-default">
                <input
                  type="checkbox"
                  data-testid="port-setup-accept-unverified"
                  checked={acceptUnverified}
                  onChange={(event) => setAcceptUnverified(event.currentTarget.checked)}
                  className="mt-0.5 h-4 w-4 shrink-0 cursor-pointer accent-brand"
                />
                <span className="break-words">
                  {t('Ports::Download it anyway and check the file against that checksum.')}
                </span>
              </label>
            </section>
          )}

          {release?.malwareFlag && (
            <section
              data-testid="port-setup-malware-gate"
              className="flex flex-col gap-sm rounded-sm border border-danger bg-danger-subtle p-md"
            >
              <h3 className={sectionTitleClass}>
                {formatI18n(t('Ports::A malware scan flagged this download: {}'), release.malwareFlag)}
              </h3>
              <label className="flex cursor-pointer items-start gap-2.5 text-sm text-text-default">
                <input
                  type="checkbox"
                  data-testid="port-setup-accept-malware"
                  checked={acceptMalware}
                  onChange={(event) => setAcceptMalware(event.currentTarget.checked)}
                  className="mt-0.5 h-4 w-4 shrink-0 cursor-pointer accent-brand"
                />
                <span className="break-words">
                  {t('Ports::I understand the risk and want to continue.')}
                </span>
              </label>
            </section>
          )}

          {missingRequired.length > 0 && (
            <p data-testid="port-setup-missing-required" className="text-xs text-text-dimmed">
              {formatI18n(
                t('Ports::Missing required files: {}'),
                missingRequired.map((slot) => slot.label).join(', '),
              )}
            </p>
          )}

          {startError && (
            <p
              data-testid="port-setup-start-error"
              role="alert"
              className="rounded-sm border border-danger bg-danger-subtle px-md py-sm text-sm text-text-default"
            >
              {startError}
            </p>
          )}
        </FloatingModal.Body>

        <FloatingModal.Footer className="flex-wrap justify-between gap-sm bg-chrome px-control-inline py-md">
          <span className="min-w-0 flex-1 break-words text-xs text-text-dimmed">
            {t('Ports::You can close this window while the install runs.')}
          </span>
          <div className="flex flex-wrap justify-end gap-compact">
            <button type="button" onClick={handleClose} className={secondaryButtonClass}>
              {t('Close')}
            </button>
            <button
              type="button"
              data-testid="port-setup-start"
              onClick={() => {
                void handleStart();
              }}
              disabled={!canStart}
              className={primaryButtonClass}
            >
              {starting
                ? t('Ports::Starting…')
                : retryJobId
                  ? t('Ports::Retry install')
                  : t('Ports::Start install')}
            </button>
          </div>
        </FloatingModal.Footer>

        <FloatingModal.ResizeHandle />
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
