import React from 'react';
import { IconLoader2, IconX } from '@tabler/icons-react';
import { FloatingModal } from '../../components/FloatingModal';
import {
  commands,
  useTools,
  type ProxyDllName,
  type ToolChannel,
  type ToolModalView,
} from '../../api';
import { useI18n } from '../../i18n';

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

export const GameToolsModal: React.FC<GameToolsModalProps> = ({
  modal: propModal,
  open: propOpen,
  onClose,
}) => {
  const { t } = useI18n();
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

  return (
    <FloatingModal.Root
      opened
      onClose={handleClose}
      geometryKey="game-tools"
      defaultSize={{ width: 760, height: 560 }}
      minSize={{ width: 560, height: 420 }}
      zIndex={1000}
      closeOnClickOutside
    >
      <FloatingModal.Overlay backgroundOpacity={0.62} color="#000000" />
      <FloatingModal.Content className="flex flex-col bg-[#0B0B0B]">
        {/* Header */}
        <FloatingModal.Header className="px-[24px] py-md">
          <div>
            <FloatingModal.Title className="text-lg font-semibold text-text-default">
              {t('tools.gameTools')}
            </FloatingModal.Title>
            <div className="text-xs text-text-dimmed">
              {modal.title} {modal.installDir ? `(${modal.installDir})` : ''}
            </div>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={handleClose}
            className="rounded-sm px-sm py-[6px] text-text-dimmed hover:bg-raised"
          >
            <IconX size={18} stroke={1.5} aria-hidden />
          </button>
        </FloatingModal.Header>

        {/* Content */}
        <FloatingModal.Body className="gap-lg p-[24px]">
          {/* Anti-cheat warning banner */}
          {anticheatDetected && (
            <div
              data-testid="anticheat-warning-banner"
              className="flex flex-col gap-sm rounded-md border border-warning bg-warning-subtle p-md"
            >
              <div className="font-semibold text-warning">
                {(
                  t('tools.antiCheatDetectedInstallingProxyDllsIn') ||
                  'Anti-cheat detected ({}). Installing proxy DLLs in online anti-cheat games may cause a ban.'
                ).replace('{}', anticheatSystems.join(', ') || 'Online protection')}
              </div>

              {modal.inspection?.anticheat.evidence && modal.inspection.anticheat.evidence.length > 0 && (
                <div className="text-xs text-text-default">
                  <div className="mb-1 font-medium">Detected evidence:</div>
                  <ul className="m-0 list-disc pl-[20px]">
                    {modal.inspection.anticheat.evidence.map((ev, i) => (
                      <li key={i}>{ev}</li>
                    ))}
                  </ul>
                </div>
              )}

              <label className="flex cursor-pointer items-center gap-compact text-sm">
                <input
                  type="checkbox"
                  data-testid="anticheat-confirm-checkbox"
                  checked={modal.anticheatConfirmed}
                  onChange={(e) => commands.setToolAnticheatConfirmed(e.target.checked)}
                  className="cursor-pointer"
                />
                <span>
                  {t('tools.iUnderstandTheAntiCheatRiskAnd')}
                </span>
              </label>
            </div>
          )}

          {/* Catalog Tool Card for OptiScaler */}
          <div
            data-testid="tool-catalog-card"
            className="flex flex-col gap-sm rounded-md border border-border-subtle bg-secondary p-md"
          >
            <div className="flex items-start justify-between">
              <div>
                <div className="flex items-center gap-compact">
                  <span className="text-lg font-bold">
                    {catalogItem.name}
                  </span>
                  <span
                    data-testid="tool-license-badge"
                    className="rounded-sm border border-border-subtle bg-raised px-compact py-[2px] text-xs font-semibold text-brand"
                  >
                    {catalogItem.license}
                  </span>
                </div>
                <div className="mt-1 text-sm text-text-dimmed">
                  {catalogItem.summary}
                </div>
              </div>

              {catalogItem.sourceUrl && (
                <button
                  type="button"
                  data-testid="tool-source-link"
                  onClick={() => commands.openToolSource(catalogItem.sourceUrl)}
                  className="whitespace-nowrap rounded-sm border border-border-subtle bg-raised px-sm py-[6px] text-xs text-text-default hover:bg-paper-4"
                >
                  Source repository
                </button>
              )}
            </div>

            {isInstalled && installedTool && (
              <div data-testid="installed-version">
                {t('tools.installedVersion', { version: installedTool.version })} ({t(`tools.${installedTool.channel}`)})
              </div>
            )}

            {/* Channel selector & Refresh versions */}
            <div className="flex items-center justify-between border-t border-border-subtle pt-compact">
              <div className="flex gap-1">
                {(['stable', 'nightly'] as ToolChannel[]).map((ch) => (
                  <button
                    key={ch}
                    type="button"
                    data-testid={`channel-button-${ch}`}
                    onClick={() => commands.selectToolChannel(ch)}
                    className={`rounded-sm border border-border-subtle px-[14px] py-[6px] text-xs capitalize ${
                      modal.selectedChannel === ch
                        ? 'bg-brand font-semibold text-on-primary'
                        : 'bg-raised font-normal text-text-default hover:bg-paper-4'
                    }`}
                  >
                    {ch}
                  </button>
                ))}
              </div>

              <button
                type="button"
                data-testid="refresh-versions-button"
                onClick={() => commands.refreshToolVersions()}
                className="rounded-sm border border-border-subtle bg-raised px-sm py-[6px] text-xs text-text-default hover:bg-paper-4"
              >
                {t('tools.refreshVersions')}
              </button>
            </div>

            {/* Version picker */}
            <div>
              <label
                htmlFor="tool-version-select"
                className="mb-[6px] block text-xs font-semibold text-text-dimmed"
              >
                {t('tools.availableVersions')}
              </label>
              {modal.availableVersions.length === 0 ? (
                <div className="text-xs text-text-dimmed">
                  {t('tools.noVersionsLoadedClickRefreshVersionsTo')}
                </div>
              ) : (
                <div className="flex flex-col gap-[6px]">
                  <select
                    id="tool-version-select"
                    data-testid="tool-version-select"
                    aria-label={t('tools.availableVersions')}
                    value={modal.selectedVersion ?? ''}
                    onChange={(e) => commands.selectToolVersion(e.target.value)}
                    className="w-full rounded-sm border border-border-subtle bg-[#0B0B0B] px-sm py-compact text-sm text-text-default"
                  >
                    {modal.availableVersions.map((v) => (
                      <option key={v.version} value={v.version}>
                        {v.tagName || v.version} {v.publishedAt ? `(${v.publishedAt})` : ''}
                        {v.cachedPath ? ` [${t('tools.cached')}]` : ''}
                      </option>
                    ))}
                  </select>
                </div>
              )}
            </div>

            {/* Target executable selector */}
            {modal.inspection?.candidates && modal.inspection.candidates.length > 0 && (
              <div>
                <label
                  htmlFor="target-exe-select"
                  className="mb-[6px] block text-xs font-semibold text-text-dimmed"
                >
                  {t('tools.targetExecutable')}
                </label>
                <select
                  id="target-exe-select"
                  data-testid="target-exe-select"
                  aria-label={t('tools.targetExecutable')}
                  value={modal.inspection.selectedExe ?? modal.inspection.candidates[0]?.relativeExe ?? ''}
                  onChange={(e) => commands.selectToolTargetExe(e.target.value)}
                  className="mb-[6px] w-full rounded-sm border border-border-subtle bg-[#0B0B0B] px-sm py-compact text-sm text-text-default"
                >
                  {modal.inspection.candidates.map((cand) => (
                    <option key={cand.relativeExe} value={cand.relativeExe}>
                      {cand.relativeExe} {cand.architecture ? `(${cand.architecture})` : ''}
                      {cand.preferred ? ` [${t('tools.recommended')}]` : ''}
                    </option>
                  ))}
                </select>
              </div>
            )}

            {/* Proxy DLL selector */}
            <div>
              <label
                htmlFor="proxy-dll-select"
                className="mb-[6px] block text-xs font-semibold text-text-dimmed"
              >
                {t('tools.proxyDllName')}
              </label>
              <select
                id="proxy-dll-select"
                data-testid="proxy-dll-select"
                aria-label={t('tools.proxyDllName')}
                value={modal.selectedProxy}
                onChange={(e) => commands.selectToolProxy(e.target.value)}
                className="mb-[6px] w-full rounded-sm border border-border-subtle bg-[#0B0B0B] px-sm py-compact text-sm text-text-default"
              >
                {PROXY_DLL_OPTIONS.map((dll) => (
                  <option key={dll} value={dll}>
                    {dll}.dll {dll === 'dxgi' ? `(${t('tools.recommended')})` : ''}
                  </option>
                ))}
              </select>
            </div>

            {/* Reset configuration checkbox */}
            <div className="border-t border-border-subtle pt-compact">
              <label className="flex cursor-pointer items-center gap-compact text-sm">
                <input
                  type="checkbox"
                  data-testid="reset-config-checkbox"
                  onChange={(e) => commands.setToolResetConfig(e.target.checked)}
                  className="cursor-pointer"
                />
                <span>
                  {t('tools.resetOptiscalerIniToDefaultsOnUpdate')}
                </span>
              </label>
            </div>
          </div>
        </FloatingModal.Body>

        <div
          className={modal.operation ? 'px-[24px] pt-sm' : 'p-0'}
          aria-live="polite"
        >
          {/* Operation Status / Error Banner */}
          {modal.operation && typeof modal.operation === 'object' && 'failed' in modal.operation && (
            <div
              data-testid="tool-operation-error"
              className="rounded-sm border border-danger bg-danger-subtle px-md py-sm text-text-default"
            >
              {modal.operation.failed.message}
            </div>
          )}

          {modal.operation &&
            (modal.operation === 'uninstalling' ||
              (typeof modal.operation === 'object' &&
                ('installing' in modal.operation || 'updating' in modal.operation))) && (
              <div
                data-testid="tool-operation-progress"
                className="flex items-center gap-compact rounded-sm border border-brand bg-accent-subtle px-md py-sm text-text-default"
              >
                <IconLoader2 size={16} stroke={1.5} className="animate-spin text-brand" aria-hidden />
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

        {/* Footer actions */}
        <FloatingModal.Footer className="px-[24px] py-md">
          <button
            type="button"
            onClick={handleClose}
            className="rounded-sm border border-border-subtle bg-raised px-md py-compact text-text-default hover:bg-paper-4"
          >
            {t('tools.close')}
          </button>

          {isInstalled && (
            <button
              type="button"
              data-testid="tool-uninstall-button"
              onClick={() => commands.uninstallTool()}
              disabled={isBusy}
              className="rounded-sm bg-danger px-md py-compact font-semibold text-white disabled:cursor-not-allowed disabled:opacity-50"
            >
              {modal.operation === 'uninstalling' ? t('tools.uninstalling') : t('tools.uninstall')}
            </button>
          )}

          {isInstalled ? (
            <button
              type="button"
              data-testid="tool-update-button"
              onClick={() => commands.installOrUpdateTool(true)}
              disabled={isBusy || anticheatBlocked}
              className="rounded-sm bg-brand px-lg py-compact font-semibold text-on-primary hover:bg-brand-4 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {modal.operation && typeof modal.operation === 'object' && 'updating' in modal.operation ? t('tools.updating') : t('tools.updateApply')}
            </button>
          ) : (
            <button
              type="button"
              data-testid="tool-install-button"
              onClick={() => commands.installOrUpdateTool(false)}
              disabled={isBusy || anticheatBlocked}
              className="rounded-sm bg-brand px-lg py-compact font-semibold text-on-primary hover:bg-brand-4 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {modal.operation && typeof modal.operation === 'object' && 'installing' in modal.operation ? t('tools.installing') : t('tools.installOptiscaler')}
            </button>
          )}
        </FloatingModal.Footer>

        <FloatingModal.ResizeHandle />
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
