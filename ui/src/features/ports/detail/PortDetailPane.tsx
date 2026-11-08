import React, { useEffect } from 'react';
import {
  IconArrowRight,
  IconDownload,
  IconExternalLink,
  IconX,
} from '@tabler/icons-react';
import {
  usePortDetail,
  usePortsView,
  type PortEntryView,
  type PortSetupSlot,
} from '../../../api/ports';
import { commands } from '../../../api/commands';
import { setActiveDestination } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import { formatI18n } from '../../library/formatters';
import { PortArt, PortCredit, PortPill, PortReleaseBadge } from '../catalog';
import {
  formatBytes,
  formatPortDate,
  installBlockReasonText,
  placementRootLabel,
  setupSlotKindLabel,
  versionLabel,
  type Translate,
} from '../catalog/format';
import { PortProgressRow } from './PortProgressRow';
import { openPortSetup } from '../setup';

const reasonClass = 'mt-1 text-xs text-warning';
const sectionLabelClass =
  'text-[11px] font-semibold uppercase tracking-[0.5px] text-text-dimmed';
const cardClass =
  'flex flex-col gap-1.5 rounded-sm border border-border-subtle bg-raised p-sm';

export interface PortDetailPaneProps {
  entry: PortEntryView;
  onClose: () => void;
}

function requirementPlacement(slot: PortSetupSlot, t: Translate): string | null {
  if (slot.argument) {
    switch (slot.argument.kind) {
      case 'env':
        return formatI18n(
          t('Ports::Passed as the {} environment variable.'),
          slot.argument.name,
        );
      case 'positional':
        return t('Ports::Passed to the port as a path argument.');
      case 'flag':
      default:
        return formatI18n(t('Ports::Passed as {}'), slot.argument.name);
    }
  }
  if (slot.root) {
    const root = placementRootLabel(slot.root, t);
    return slot.destination
      ? formatI18n(t('Ports::Copied to {} inside {}.'), slot.destination, root)
      : formatI18n(t('Ports::Copied to {}.'), root);
  }
  return null;
}

const RequirementRow: React.FC<{ slot: PortSetupSlot }> = ({ slot }) => {
  const { t } = useI18n();
  const placement = requirementPlacement(slot, t);
  return (
    <li
      data-testid={`port-requirement-${slot.id}`}
      className="flex flex-col gap-1 rounded-xs bg-surface px-2 py-1.5"
    >
      <div className="flex flex-wrap items-start justify-between gap-1">
        <span className="min-w-0 break-words text-sm font-medium text-text-default">
          {slot.label}
        </span>
        <PortPill
          tone={slot.required ? 'brand' : 'neutral'}
          testId={`port-requirement-required-${slot.id}`}
        >
          {slot.required ? t('Ports::Required') : t('Ports::Optional')}
        </PortPill>
      </div>
      {slot.prompt && (
        <p className="break-words text-xs text-text-dimmed">{slot.prompt}</p>
      )}
      <div className="flex flex-wrap items-center gap-1 text-[11px] text-text-dimmed">
        <span>{setupSlotKindLabel(slot, t)}</span>
        {slot.extensions.length > 0 && (
          <span>
            ·{' '}
            {formatI18n(
              t('Ports::Extensions: {}'),
              slot.extensions.join(', '),
            )}
          </span>
        )}
      </div>
      {slot.acceptedLabels.length > 0 && (
        <div data-testid={`port-requirement-accepted-${slot.id}`} className="text-[11px] text-text-default">
          {formatI18n(t('Ports::Accepted: {}'), slot.acceptedLabels.join(', '))}
        </div>
      )}
      {slot.contains.length > 0 && (
        <div className="break-words text-[11px] text-text-dimmed">
          {formatI18n(t('Ports::Must contain: {}'), slot.contains.join(', '))}
        </div>
      )}
      {placement && (
        <div className="break-words text-[11px] text-text-dimmed">{placement}</div>
      )}
      {slot.packageOption && (
        <div
          data-testid={`port-package-option-${slot.id}`}
          className="mt-0.5 flex flex-col gap-0.5 rounded-xs bg-raised p-1.5 text-[11px] text-text-dimmed"
        >
          <span className="break-words">
            {formatI18n(
              t('Ports::Extracted with {}'),
              slot.packageOption.toolName,
            )}
          </span>
          {slot.packageOption.titleId && (
            <span>
              {formatI18n(
                t('Ports::Package title: {}'),
                slot.packageOption.titleId,
              )}
            </span>
          )}
          {slot.packageOption.requiresUpdate && slot.packageOption.updateVersion && (
            <span>
              {formatI18n(
                t('Ports::Update {} is required.'),
                slot.packageOption.updateVersion,
              )}
            </span>
          )}
        </div>
      )}
    </li>
  );
};

/**
 * The catalog's detail pane. Static entry data renders immediately; the lazy release
 * detail (`ports_detail`, D3) fills in dates, sizes, withdrawal reasons and updates.
 */
export const PortDetailPane: React.FC<PortDetailPaneProps> = ({ entry, onClose }) => {
  const ports = usePortsView();
  const { t, locale } = useI18n();
  const detailQuery = usePortDetail(entry.catalogId);
  const detail = detailQuery.data ?? null;

  const provenance = entry.installedRegistrationKey
    ? ports.provenance[entry.installedRegistrationKey]
    : undefined;
  const installed = Boolean(entry.installedRegistrationKey);
  const job = ports.jobs.find((candidate) => candidate.catalogId === entry.catalogId);
  const release = detail?.selectedRelease ?? null;
  const project = detail?.project ?? entry.project;
  const source = detail?.source ?? entry.source;
  const license = entry.license ?? detail?.license ?? null;
  const requirements = detail?.requirements ?? entry.requirements;

  // Opening an entry marks its finished or failed result as seen (desktop-shell spec).
  useEffect(() => {
    commands.portsMarkSeen(entry.catalogId);
  }, [entry.catalogId]);

  const openUrl = (url: string) => {
    commands.openToolSource(url);
  };

  const handleOpenInLibrary = () => {
    setActiveDestination('library');
    commands.libraryInput({ setFilter: 'all' });
    commands.libraryInput({ setSearch: '' });
    if (entry.installedRegistrationKey) {
      commands.libraryInput({
        select: { key: entry.installedRegistrationKey, mode: 'replace' },
      });
    }
    commands.libraryInput('openDetails');
  };

  const releaseVersion = versionLabel(release?.version ?? entry.version);
  const releaseWithdrawn = release?.withdrawn ?? entry.withdrawn;
  const releaseDate = formatPortDate(release?.releasedAt, locale);
  const blockReason = installBlockReasonText(entry.installBlockReason, t);

  return (
    <aside
      data-testid="port-detail-pane"
      aria-label={t('Ports::Port details')}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.preventDefault();
          onClose();
        }
      }}
      className="flex h-full w-[clamp(300px,32vw,420px)] shrink-0 flex-col overflow-hidden border-l border-border-subtle bg-secondary"
    >
      <div className="flex shrink-0 items-center justify-between border-b border-border-subtle px-md py-sm">
        <span className="text-lg font-semibold text-text-default">
          {t('Ports::Port details')}
        </span>
        <button
          type="button"
          data-testid="port-detail-close-btn"
          aria-label={t('Ports::Close details')}
          title={t('Ports::Close details')}
          onClick={onClose}
          className="rounded-sm p-1 text-text-default transition-colors hover:bg-surface"
        >
          <IconX size={18} stroke={1.75} aria-hidden />
        </button>
      </div>

      <div className="flex flex-1 flex-col gap-md overflow-y-auto p-md">
        <div
          data-testid="port-detail-art"
          className="h-[180px] w-full shrink-0 overflow-hidden rounded-sm bg-surface"
        >
          {entry.art.hero || entry.art.capsule ? (
            <PortArt
              catalogId={entry.catalogId}
              title={entry.name}
              kind={entry.art.hero ? 'hero' : 'capsule'}
            />
          ) : (
            <div
              data-testid="port-art-placeholder"
              className="flex h-full w-full select-none items-center justify-center overflow-hidden break-words rounded-sm bg-surface p-3 text-center text-sm text-text-default"
            >
              <span className="line-clamp-3 font-medium">{entry.name}</span>
            </div>
          )}
        </div>

        <div>
          <h2
            data-testid="port-detail-title"
            className="break-words text-lg font-semibold leading-[1.3] text-text-default"
          >
            {entry.name}
          </h2>
          <div className="mt-1.5 flex flex-wrap items-center gap-1">
            <PortReleaseBadge
              entry={entry}
              withdrawn={releaseWithdrawn}
              verified={release?.verified}
            />
            {entry.guided && (
              <PortPill testId="port-detail-guided-badge">
                {t('Ports::Guided setup')}
              </PortPill>
            )}
            {installed && (
              <PortPill tone="brand" testId="port-detail-installed-badge">
                {t('Ports::Installed')}
              </PortPill>
            )}
          </div>
        </div>

        <div className="flex flex-col gap-1 text-sm">
          <div className="flex flex-wrap gap-1">
            <span className="text-text-dimmed">{t('Ports::Original game')}</span>
            <span className="min-w-0 break-words text-text-default">
              {entry.games.join(', ')}
            </span>
          </div>
          <div className="flex flex-wrap gap-1">
            <span className="text-text-dimmed">{t('Ports::Console')}</span>
            <span className="min-w-0 break-words text-text-default">
              {entry.consoles.join(', ')}
            </span>
          </div>
          {detail?.developer && (
            <div className="flex flex-wrap gap-1">
              <span className="text-text-dimmed">{t('Ports::Developer')}</span>
              <span className="min-w-0 break-words text-text-default">
                {detail.developer}
              </span>
            </div>
          )}
          {license && (
            <div className="flex flex-wrap gap-1">
              <span className="text-text-dimmed">{t('Ports::License')}</span>
              <span className="min-w-0 break-words text-text-default">{license}</span>
            </div>
          )}
        </div>

        {detail?.description && (
          <p
            data-testid="port-detail-description"
            className="break-words text-sm leading-relaxed text-text-dimmed"
          >
            {detail.description}
          </p>
        )}

        {(project || source) && (
          <div data-testid="port-detail-links" className="flex flex-col gap-1">
            {project && (
              <button
                type="button"
                data-testid="port-project-link"
                onClick={() => openUrl(project.url)}
                className="flex min-w-0 items-center gap-1.5 text-left text-sm text-brand transition-colors hover:text-brand-4"
              >
                <span className="shrink-0 text-text-dimmed">{t('Ports::Project')}</span>
                <span className="min-w-0 truncate" title={project.label}>
                  {project.label}
                </span>
                <IconExternalLink size={14} stroke={1.75} aria-hidden className="shrink-0" />
              </button>
            )}
            {source && (
              <button
                type="button"
                data-testid="port-source-link"
                onClick={() => openUrl(source.url)}
                className="flex min-w-0 items-center gap-1.5 text-left text-sm text-brand transition-colors hover:text-brand-4"
              >
                <span className="shrink-0 text-text-dimmed">{t('Ports::Source')}</span>
                <span className="min-w-0 truncate" title={source.label}>
                  {source.label}
                </span>
                <IconExternalLink size={14} stroke={1.75} aria-hidden className="shrink-0" />
              </button>
            )}
          </div>
        )}

        <div data-testid="port-release-card" className={cardClass}>
          <span className={sectionLabelClass}>{t('Ports::Release')}</span>
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-sm font-semibold text-text-default">
              {releaseVersion || t('Ports::No release information is available yet.')}
            </span>
            <PortReleaseBadge
              entry={entry}
              withdrawn={releaseWithdrawn}
              verified={release?.verified}
            />
            {!releaseVersion && detailQuery.isLoading && (
              <span className="text-[11px] text-text-dimmed">
                {t('Ports::Checking for release information…')}
              </span>
            )}
          </div>
          {releaseDate && (
            <span className="text-xs text-text-dimmed">
              {formatI18n(t('Ports::Released: {}'), releaseDate)}
            </span>
          )}
          {release && (
            <div className="flex flex-wrap items-center gap-1.5">
              {release.verified && (
                <PortPill tone="success" testId="port-release-checksum-verified">
                  {t('Ports::Checksum verified')}
                </PortPill>
              )}
              {release.requiresChecksumConfirmation && (
                <PortPill tone="warning" testId="port-release-checksum-unverified">
                  {t('Ports::Checksum not verified')}
                </PortPill>
              )}
              {release.sizeBytes != null && (
                <span className="text-[11px] text-text-dimmed">
                  {formatBytes(release.sizeBytes, locale)}
                </span>
              )}
            </div>
          )}
          {releaseWithdrawn && (
            <span className="text-xs text-danger">{t('Ports::Withdrawn')}</span>
          )}
          {detail?.withdrawalReason && (
            <span
              data-testid="port-withdrawal-reason"
              className="break-words text-xs text-warning"
            >
              {formatI18n(t('Ports::Withdrawal reason: {}'), detail.withdrawalReason)}
            </span>
          )}
          {release?.malwareFlag && (
            <span
              data-testid="port-malware-warning"
              className="break-words rounded-xs bg-warning-subtle px-2 py-1 text-xs text-warning"
            >
              {formatI18n(t('Ports::A malware scan flagged this download: {}'), release.malwareFlag)}
            </span>
          )}
          {detailQuery.isError && (
            <div className="flex flex-wrap items-center gap-compact">
              <span className="text-xs text-danger">
                {t('Ports::Could not load the release details.')}
              </span>
              <button
                type="button"
                data-testid="port-detail-retry"
                onClick={() => detailQuery.refetch()}
                className="rounded-xs border border-border-subtle bg-transparent px-2 py-0.5 text-xs text-text-default transition-colors hover:bg-surface"
              >
                {t('Library::Retry')}
              </button>
            </div>
          )}
        </div>

        <div data-testid="port-requirements" className="flex flex-col gap-1.5">
          <span className={sectionLabelClass}>{t('Ports::Requirements')}</span>
          {requirements.length === 0 ? (
            <p className="text-sm text-text-dimmed">
              {t('Ports::This port does not need any files from you.')}
            </p>
          ) : (
            <ul className="flex flex-col gap-sm">
              {requirements.map((slot) => (
                <RequirementRow key={slot.id} slot={slot} />
              ))}
            </ul>
          )}
        </div>

        {installed && provenance && (
          <div
            data-testid="port-installed-info"
            className="flex flex-col gap-0.5 rounded-sm bg-raised px-sm py-compact text-xs"
          >
            <span className="text-text-default">
              {formatI18n(
                t('Ports::Installed version {}'),
                versionLabel(provenance.installedVersion),
              )}
            </span>
            <span
              className="break-all text-text-dimmed"
              title={provenance.installFolder}
            >
              {formatI18n(t('Ports::Installed in {}'), provenance.installFolder)}
            </span>
          </div>
        )}

        {detail?.updateAvailable && (
          <div
            data-testid="port-update-notice"
            className="flex flex-col gap-1 rounded-sm border border-border-subtle bg-warning-subtle px-sm py-compact"
          >
            <span className="text-sm font-medium text-warning">
              {formatI18n(
                t('Ports::Update available: {}'),
                versionLabel(detail.updateAvailable.version),
              )}
            </span>
            <span className="text-xs text-text-default">
              {t(
                'Ports::A newer verified release is available. GameNode does not change installed ports; update it from the project page.',
              )}
            </span>
            {project && (
              <button
                type="button"
                data-testid="port-update-project-link"
                onClick={() => openUrl(project.url)}
                className="flex items-center gap-1 self-start text-xs font-medium text-brand transition-colors hover:text-brand-4"
              >
                {t('Ports::Open project page')}
                <IconExternalLink size={12} stroke={1.75} aria-hidden />
              </button>
            )}
          </div>
        )}

        <div data-testid="port-install-action" className="flex flex-col">
          {installed ? (
            <button
              type="button"
              data-testid="port-open-library-btn"
              onClick={handleOpenInLibrary}
              className="flex w-full items-center justify-center gap-1.5 rounded-sm border-0 bg-brand px-md py-xs text-sm font-semibold text-on-primary transition-colors hover:bg-brand-6"
            >
              <IconArrowRight size={16} stroke={1.75} aria-hidden />
              {t('Ports::Open in library')}
            </button>
          ) : (
            <>
              <button
                type="button"
                data-testid="port-install-btn"
                disabled={!entry.installable}
                onClick={() => openPortSetup(entry.catalogId)}
                className={`flex w-full items-center justify-center gap-1.5 rounded-sm border-0 bg-brand px-md py-xs text-sm font-semibold text-on-primary transition-colors ${
                  entry.installable
                    ? 'cursor-pointer hover:bg-brand-6'
                    : 'cursor-not-allowed opacity-45'
                }`}
              >
                <IconDownload size={16} stroke={1.75} aria-hidden />
                {t('Library::Install')}
              </button>
              {!entry.installable && (
                <div data-testid="port-install-reason" className={reasonClass}>
                  {blockReason ?? t('Ports::Install is unavailable for this entry.')}
                </div>
              )}
            </>
          )}
        </div>

        {job && <PortProgressRow entry={entry} job={job} />}
      </div>

      <div className="shrink-0 border-t border-border-subtle px-md py-compact">
        <PortCredit />
      </div>
    </aside>
  );
};
