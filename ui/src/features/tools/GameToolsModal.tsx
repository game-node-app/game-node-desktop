import React from 'react';
import {
  IconBolt,
  IconDownload,
  IconExternalLink,
  IconFileText,
  IconLoader2,
  IconRefresh,
} from '@tabler/icons-react';
import { DialogShell } from '../../components/dialog';
import {
  Button,
  Callout,
  Checkbox,
  Field,
  IconButton,
  InsetCard,
  Pill,
  Radio,
  RadioGroup,
  SegmentedControl,
  SelectField,
  TextInput,
  type PillTone,
} from '../../components/ui';
import {
  commands,
  useTools,
  type ProxyDllName,
  type ToolChannel,
  type ToolModalView,
  type ToolOperationErrorCode,
  type TargetCandidate,
} from '../../api';
import { useI18n } from '../../i18n';
import { fallbackProviderName } from '../../i18n/providerLabel';
import { formatBytes } from '../ports/catalog/format';

export interface GameToolsModalProps {
  modal?: ToolModalView | null;
  open?: boolean;
  onClose?: () => void;
}

export const PROXY_DLL_OPTIONS: ProxyDllName[] = [
  'dxgi',
  'winmm',
  'version',
  'dbghelp',
  'd3d12',
  'wininet',
  'winhttp',
];

/** Human names for the engine's camelCase anti-cheat detections. */
const ANTICHEAT_NAMES: Record<string, string> = {
  easyAntiCheat: 'Easy Anti-Cheat',
  battlEye: 'BattlEye',
};

/** Maps typed host error codes to localized copy while preserving raw fallback messages. */
function formatToolOperationError(
  failed: { message: string; code?: ToolOperationErrorCode | null },
  t: ReturnType<typeof useI18n>['t'],
): string {
  switch (failed.code) {
    case 'pickerUnsupported':
      return t('tools.errorPickerUnsupported');
    case 'pickerFailed':
      return t('tools.errorPickerFailed');
    case 'targetOutsideInstallDir':
      return t('tools.errorTargetOutsideInstallDir');
    case 'targetExeNotFound':
      return t('tools.errorTargetExeNotFound');
    case 'installDirNotFound':
      return t('tools.errorInstallDirNotFound');
    default:
      return failed.message;
  }
}

/** Joins the game folder and a candidate's relative path with the folder's separator. */
function joinInstallPath(installDir: string, relativeExe: string): string {
  if (!installDir) return relativeExe;
  const separator = installDir.includes('\\') ? '\\' : '/';
  const relative = relativeExe
    .replace(/^[\\/]+/, '')
    .replace(/[\\/]+/g, separator);
  if (!relative) return '';
  return `${installDir.replace(/[\\/]+$/, '')}${separator}${relative}`;
}

export const GameToolsModal: React.FC<GameToolsModalProps> = ({
  modal: propModal,
  open: propOpen,
  onClose,
}) => {
  const { t, locale } = useI18n();
  const tools = useTools();

  const modal = propModal !== undefined ? propModal : tools.modal;
  const isOpen = propOpen !== undefined ? propOpen : modal != null;

  const handleClose = () => {
    commands.closeTools();
    onClose?.();
  };

  if (!isOpen || !modal) {
    return null;
  }

  const catalogItem = tools.catalog.find((c) => c.id === modal.activeToolId) ?? {
    id: 'optiscaler',
    name: 'OptiScaler',
    summary: 'Upscaler & frame generation middleware (DLSS / XeSS / FSR 2/3/4)',
    license: 'GPL-3.0',
    sourceUrl: 'https://github.com/optiscaler/OptiScaler',
  };

  const installedTool = modal.tools.find((t) => t.toolId === modal.activeToolId);
  const isInstalled = Boolean(
    installedTool && (installedTool.state === 'installed' || installedTool.state === 'updating'),
  );

  const isBusy = Boolean(
    modal.operation &&
      (modal.operation === 'uninstalling' ||
        (typeof modal.operation === 'object' &&
          ('installing' in modal.operation || 'updating' in modal.operation))),
  );

  const anticheatSystems = modal.inspection?.anticheat.detected ?? [];
  const anticheatDetected = anticheatSystems.length > 0;
  const anticheatBlocked = anticheatDetected && !modal.anticheatConfirmed;
  const anticheatEvidence = modal.inspection?.anticheat.evidence ?? [];
  const anticheatNames = anticheatSystems
    .map((system) =>
      system === 'blocklisted'
        ? t('tools.antiCheatBlocklisted')
        : (ANTICHEAT_NAMES[system] ?? system),
    )
    .join(', ');

  const candidates = modal.inspection?.candidates ?? [];
  const selectedExe =
    modal.inspection?.selectedExe ?? candidates[0]?.relativeExe ?? null;
  const targetPath = selectedExe
    ? joinInstallPath(modal.installDir, selectedExe)
    : '';

  const selectedRelease =
    modal.availableVersions.find(
      (release) => release.version === modal.selectedVersion,
    ) ??
    modal.availableVersions[0] ??
    null;
  const latestRelease = modal.availableVersions[0] ?? null;
  const updateAvailable = Boolean(
    installedTool &&
      latestRelease &&
      installedTool.version !== latestRelease.version,
  );

  let statusLabel: string;
  let statusTone: PillTone;
  if (!installedTool) {
    statusLabel = t('tools.notInstalled');
    statusTone = 'neutral';
  } else if (updateAvailable) {
    statusLabel = t('tools.updateAvailable');
    statusTone = 'warning';
  } else {
    statusLabel = t('tools.installedWithVersion', {
      version: installedTool.version,
    });
    statusTone = 'success';
  }

  let footerNote: React.ReactNode;
  if (selectedRelease) {
    const verified = Boolean(selectedRelease.sha256);
    const message = selectedRelease.cachedPath
      ? verified
        ? t('tools.footerAlreadyDownloadedVerified')
        : t('tools.footerAlreadyDownloaded')
      : verified
        ? t('tools.footerDownloadVerified', {
            size: formatBytes(selectedRelease.sizeBytes, locale),
          })
        : t('tools.footerDownload', {
            size: formatBytes(selectedRelease.sizeBytes, locale),
          });

    footerNote = (
      <>
        <IconDownload size={14} stroke={2} aria-hidden="true" />
        <span data-testid="tool-download-note">{message}</span>
      </>
    );
  }

  const versionOptions = modal.availableVersions.map((release, index) => ({
    value: release.version,
    label:
      index === 0
        ? t('tools.versionRecommended', { version: release.version })
        : release.version,
  }));

  const proxyOptions = PROXY_DLL_OPTIONS.map((dll) => ({
    value: dll,
    label: `${dll}.dll`,
  }));

  const candidateLabel = (candidate: TargetCandidate) =>
    candidate.label === 'Launcher'
      ? t('tools.candidateLauncher', { file: candidate.relativeExe })
      : candidate.relativeExe;

  const handleBrowse = () => {
    if (modal.installDir) {
      void commands.pickToolTargetExe();
    }
  };

  return (
    <DialogShell
      title={t('tools.gameTools')}
      subtitle={`${modal.title} · ${fallbackProviderName(
        modal.entryKey.split(':')[0] ?? '',
      )}`}
      geometryKey="game-tools"
      defaultSize={{ width: 600, height: 680 }}
      minSize={{ width: 500, height: 480 }}
      onClose={handleClose}
      testId="game-tools-modal"
      footerNote={footerNote}
      footerLeading={
        isInstalled ? (
          <Button
            size="sm"
            variant="danger"
            data-testid="tool-uninstall-button"
            disabled={isBusy}
            onClick={() => commands.uninstallTool()}
          >
            {modal.operation === 'uninstalling'
              ? t('tools.uninstalling')
              : t('tools.uninstall')}
          </Button>
        ) : undefined
      }
      footer={
        <>
          <Button
            size="sm"
            variant="secondary"
            data-testid="tool-cancel-button"
            onClick={handleClose}
          >
            {t('tools.cancel')}
          </Button>

          {isInstalled ? (
            <Button
              size="sm"
              variant="primary"
              leftIcon={IconDownload}
              data-testid="tool-update-button"
              disabled={isBusy || anticheatBlocked}
              onClick={() => commands.installOrUpdateTool(true)}
            >
              {modal.operation &&
              typeof modal.operation === 'object' &&
              'updating' in modal.operation
                ? t('tools.updating')
                : t('tools.updateTool', { tool: catalogItem.name })}
            </Button>
          ) : (
            <Button
              size="sm"
              variant="primary"
              leftIcon={IconDownload}
              data-testid="tool-install-button"
              disabled={isBusy || anticheatBlocked}
              onClick={() => commands.installOrUpdateTool(false)}
            >
              {modal.operation &&
              typeof modal.operation === 'object' &&
              'installing' in modal.operation
                ? t('tools.installing')
                : t('tools.installTool', { tool: catalogItem.name })}
            </Button>
          )}
        </>
      }
    >
      <div data-testid="tool-catalog-card">
        <InsetCard
          icon={IconBolt}
          iconBox
          title={
            <span className="flex items-center gap-compact">
              <span className="truncate">{catalogItem.name}</span>
              <span
                data-testid="tool-license-badge"
                className="shrink-0 text-caption font-normal text-text-dimmed"
              >
                {catalogItem.license}
              </span>
            </span>
          }
          description={catalogItem.summary}
          trailing={
            <div className="flex items-center gap-compact">
              <span data-testid={installedTool ? 'installed-version' : undefined}>
                <Pill tone={statusTone}>{statusLabel}</Pill>
              </span>
              {catalogItem.sourceUrl ? (
                <IconButton
                  icon={IconExternalLink}
                  label={t('tools.source')}
                  variant="toggle"
                  size="sm"
                  data-testid="tool-source-link"
                  onClick={() => commands.openToolSource(catalogItem.sourceUrl)}
                />
              ) : null}
            </div>
          }
        />
      </div>

      <div className="flex items-end gap-sm">
        <Field label={t('tools.channel')} className="shrink-0">
          <SegmentedControl<ToolChannel>
            aria-label={t('tools.channel')}
            data={[
              {
                value: 'stable',
                label: (
                  <span data-testid="channel-button-stable">
                    {t('tools.stable')}
                  </span>
                ),
              },
              {
                value: 'nightly',
                label: (
                  <span data-testid="channel-button-nightly">
                    {t('tools.nightly')}
                  </span>
                ),
              },
            ]}
            value={modal.selectedChannel}
            onChange={(channel) => commands.selectToolChannel(channel)}
            data-testid="channel-segmented-control"
          />
        </Field>

        <Field
          label={t('tools.version')}
          helper={
            modal.availableVersions.length === 0
              ? t('tools.noVersionsLoadedClickRefreshVersionsTo')
              : undefined
          }
          className="min-w-0 flex-1"
        >
          <SelectField
            options={versionOptions}
            value={modal.selectedVersion ?? null}
            placeholder={t('tools.select')}
            onChange={(version) => commands.selectToolVersion(version)}
            data-testid="tool-version-select"
          />
        </Field>

        <Field label={t('tools.proxyDllName')} className="min-w-0 flex-1">
          <SelectField
            options={proxyOptions}
            value={modal.selectedProxy}
            onChange={(proxy) => commands.selectToolProxy(proxy)}
            data-testid="proxy-dll-select"
          />
        </Field>

        <IconButton
          icon={IconRefresh}
          label={t('tools.refreshVersions')}
          variant="outlined"
          size="toolbar"
          data-testid="refresh-versions-button"
          onClick={() => commands.refreshToolVersions()}
        />
      </div>

      <div className="flex items-end gap-compact">
        <Field label={t('tools.targetExecutable')} className="min-w-0 flex-1">
          <TextInput
            readOnly
            leadingIcon={IconFileText}
            value={targetPath}
            placeholder={t('tools.noExecutableSelected')}
            data-testid="target-exe-input"
          />
        </Field>
        <Button
          size="md"
          variant="secondary"
          data-testid="tool-browse-button"
          disabled={!modal.installDir || isBusy}
          onClick={handleBrowse}
        >
          {t('tools.browse')}
        </Button>
      </div>

      {candidates.length > 0 && (
        <div className="flex flex-col gap-1">
          <p className="text-xs text-text-dimmed">
            {t('tools.detectedCandidateExecutablesSelectToOverride')}
          </p>
          <RadioGroup
            aria-label={t('tools.detectedCandidateExecutablesSelectToOverride')}
            value={selectedExe}
            onChange={(relativeExe) =>
              commands.selectToolTargetExe(relativeExe)
            }
            className="flex flex-col gap-0.5"
          >
            {candidates.map((candidate) => {
              const selected = candidate.relativeExe === selectedExe;
              return (
                <Radio
                  key={candidate.relativeExe}
                  value={candidate.relativeExe}
                  label={
                    <span
                      className={`text-xs font-normal ${
                        selected ? 'text-text-primary' : 'text-text-default'
                      }`}
                    >
                      {candidateLabel(candidate)}
                    </span>
                  }
                  data-testid={`candidate-radio-${candidate.relativeExe}`}
                  classNames={{
                    root: `!flex h-8 w-full items-center gap-[10px] rounded-control px-[10px] ${
                      selected ? 'bg-raised' : ''
                    }`,
                    body: 'w-full',
                  }}
                />
              );
            })}
          </RadioGroup>
        </div>
      )}

      <div>
        <Checkbox
          data-testid="reset-config-checkbox"
          onChange={(event) =>
            commands.setToolResetConfig(event.currentTarget.checked)
          }
          label={t('tools.resetOptiscalerIniToDefaultsOnUpdate')}
        />
      </div>

      {anticheatDetected && (
        <div data-testid="anticheat-warning-banner">
          <Callout
            tone="danger"
            title={t('tools.antiCheatDetectedTitle', {
              name: anticheatNames,
            })}
            confirmation={
              <Checkbox
                data-testid="anticheat-confirm-checkbox"
                checked={modal.anticheatConfirmed}
                onChange={(event) =>
                  commands.setToolAnticheatConfirmed(
                    event.currentTarget.checked,
                  )
                }
                label={
                  <span className="text-xs text-text-primary">
                    {t('tools.iUnderstandTheAntiCheatRiskAnd')}
                  </span>
                }
              />
            }
          >
            <p>{t('tools.antiCheatBody')}</p>
            {anticheatEvidence.length > 0 && (
              <ul className="mt-1 list-disc pl-4 text-caption text-text-dimmed">
                {anticheatEvidence.map((evidence, index) => (
                  <li key={`${evidence}-${index}`}>{evidence}</li>
                ))}
              </ul>
            )}
          </Callout>
        </div>
      )}

      {modal.operation && (
        <div aria-live="polite" className="flex flex-col gap-compact">
          {typeof modal.operation === 'object' &&
            'failed' in modal.operation && (
              <div
                data-testid="tool-operation-error"
                className="rounded-control border border-danger bg-danger-subtle px-sm py-compact text-xs text-text-default"
              >
                {formatToolOperationError(modal.operation.failed, t)}
              </div>
            )}

          {(modal.operation === 'uninstalling' ||
            (typeof modal.operation === 'object' &&
              ('installing' in modal.operation ||
                'updating' in modal.operation))) && (
            <div
              data-testid="tool-operation-progress"
              className="flex items-center gap-compact rounded-control border border-brand bg-accent-subtle px-sm py-compact text-xs text-text-default"
            >
              <IconLoader2
                size={16}
                stroke={2}
                className="animate-spin text-brand motion-reduce:animate-none"
                aria-hidden="true"
              />
              <span>
                {modal.operation === 'uninstalling'
                  ? t('tools.uninstalling')
                  : 'installing' in modal.operation
                    ? `${modal.operation.installing.message}${
                        modal.operation.installing.progressPercent != null
                          ? ` (${modal.operation.installing.progressPercent}%)`
                          : ''
                      }`
                    : `${modal.operation.updating.message}${
                        modal.operation.updating.progressPercent != null
                          ? ` (${modal.operation.updating.progressPercent}%)`
                          : ''
                      }`}
              </span>
            </div>
          )}
        </div>
      )}
    </DialogShell>
  );
};
