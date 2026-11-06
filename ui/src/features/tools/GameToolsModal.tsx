import React, { useEffect } from 'react';
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
  'dxgi.dll',
  'winmm.dll',
  'version.dll',
  'dbghelp.dll',
  'd3d12.dll',
  'wininet.dll',
  'winhttp.dll',
  'OptiScaler.asi',
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

  useEffect(() => {
    if (!isOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        handleClose();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isOpen]);

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

  const anticheatDetected = Boolean(modal.inspection?.anticheat?.detected);
  const anticheatBlocked = anticheatDetected && !modal.anticheatConfirmed;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="game-tools-heading"
      style={{
        position: 'fixed',
        inset: 0,
        backgroundColor: 'var(--settings-overlay, rgba(0, 0, 0, 0.62))',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
        padding: 'var(--modal-margin, 24px)',
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) handleClose();
      }}
    >
      <div
        style={{
          width: '100%',
          maxWidth: '720px',
          maxHeight: '90vh',
          backgroundColor: 'var(--settings-bg-base, #111111)',
          border: '1px solid var(--settings-border-subtle, #2A2A2A)',
          borderRadius: 'var(--radius-md, 8px)',
          boxShadow: 'var(--shadow-modal)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
      >
        {/* Header */}
        <div
          style={{
            padding: '16px 24px',
            borderBottom: '1px solid var(--settings-border-subtle, #2A2A2A)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <div>
            <h2 id="game-tools-heading" style={{ fontSize: 'var(--font-size-lg, 18px)', fontWeight: 600 }}>
              {t('tools.gameTools')}
            </h2>
            <div style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}>
              {modal.title} {modal.installDir ? `(${modal.installDir})` : ''}
            </div>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={handleClose}
            style={{
              padding: '6px 12px',
              backgroundColor: 'transparent',
              color: 'var(--color-text-dimmed, #808080)',
              borderRadius: 'var(--radius-sm, 4px)',
              cursor: 'pointer',
            }}
          >
            ✕
          </button>
        </div>

        {/* Content */}
        <div
          style={{
            padding: '24px',
            overflowY: 'auto',
            display: 'flex',
            flexDirection: 'column',
            gap: '20px',
          }}
        >
          {/* Operation Status / Error Banner */}
          {modal.operation && typeof modal.operation === 'object' && 'failed' in modal.operation && (
            <div
              data-testid="tool-operation-error"
              style={{
                padding: '12px 16px',
                backgroundColor: 'var(--color-danger-subtle, #E5484D1F)',
                border: '1px solid var(--color-danger, #E5484D)',
                borderRadius: 'var(--radius-sm, 4px)',
                color: 'var(--color-text-default, #D9D9D9)',
              }}
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
                style={{
                  padding: '12px 16px',
                  backgroundColor: 'var(--color-accent-subtle, #F150251F)',
                  border: '1px solid var(--color-primary, #F15025)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  color: 'var(--color-text-default, #D9D9D9)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: '8px',
                }}
              >
                <span>⏳</span>
                <span>
                  {modal.operation === 'uninstalling'
                    ? 'Uninstalling…'
                    : 'installing' in modal.operation
                      ? `Installing: ${modal.operation.installing.message}${
                          modal.operation.installing.progressPercent != null
                            ? ` (${modal.operation.installing.progressPercent}%)`
                            : ''
                        }`
                      : `Updating: ${modal.operation.updating.message}${
                          modal.operation.updating.progressPercent != null
                            ? ` (${modal.operation.updating.progressPercent}%)`
                            : ''
                        }`}
                </span>
              </div>
            )}

          {/* Anti-cheat warning banner */}
          {anticheatDetected && (
            <div
              data-testid="anticheat-warning-banner"
              style={{
                padding: '16px',
                backgroundColor: 'var(--color-warning-subtle, #F5A5241F)',
                border: '1px solid var(--color-warning, #F5A524)',
                borderRadius: 'var(--radius-md, 8px)',
                display: 'flex',
                flexDirection: 'column',
                gap: '12px',
              }}
            >
              <div style={{ fontWeight: 600, color: 'var(--color-warning, #F5A524)' }}>
                {(
                  t('tools.antiCheatDetectedInstallingProxyDllsIn') ||
                  'Anti-cheat detected ({}). Installing proxy DLLs in online anti-cheat games may cause a ban.'
                ).replace('{}', modal.inspection?.anticheat.systems.join(', ') || 'Online protection')}
              </div>

              {modal.inspection?.anticheat.evidence && modal.inspection.anticheat.evidence.length > 0 && (
                <div style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-default, #D9D9D9)' }}>
                  <div style={{ fontWeight: 500, marginBottom: '4px' }}>Detected evidence:</div>
                  <ul style={{ paddingLeft: '20px', margin: 0 }}>
                    {modal.inspection.anticheat.evidence.map((ev, i) => (
                      <li key={i}>{ev}</li>
                    ))}
                  </ul>
                </div>
              )}

              <label
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: '8px',
                  cursor: 'pointer',
                  fontSize: 'var(--font-size-sm, 14px)',
                }}
              >
                <input
                  type="checkbox"
                  data-testid="anticheat-confirm-checkbox"
                  checked={modal.anticheatConfirmed}
                  onChange={(e) => commands.setToolAnticheatConfirmed(e.target.checked)}
                  style={{ cursor: 'pointer' }}
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
            style={{
              padding: '16px',
              backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
              border: '1px solid var(--settings-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-md, 8px)',
              display: 'flex',
              flexDirection: 'column',
              gap: '12px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                  <span style={{ fontWeight: 700, fontSize: 'var(--font-size-lg, 18px)' }}>
                    {catalogItem.name}
                  </span>
                  <span
                    data-testid="tool-license-badge"
                    style={{
                      padding: '2px 8px',
                      backgroundColor: 'var(--settings-bg-raised, #222222)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      fontSize: 'var(--font-size-xs, 12px)',
                      fontWeight: 600,
                      color: 'var(--color-primary, #F15025)',
                    }}
                  >
                    {catalogItem.license}
                  </span>
                </div>
                <div
                  style={{
                    fontSize: 'var(--font-size-sm, 14px)',
                    color: 'var(--color-text-dimmed, #808080)',
                    marginTop: '4px',
                  }}
                >
                  {catalogItem.summary}
                </div>
              </div>

              {catalogItem.sourceUrl && (
                <button
                  type="button"
                  data-testid="tool-source-link"
                  onClick={() => commands.openToolSource(catalogItem.sourceUrl)}
                  style={{
                    padding: '6px 12px',
                    backgroundColor: 'var(--settings-bg-raised, #222222)',
                    border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                    borderRadius: 'var(--radius-sm, 4px)',
                    color: 'var(--color-text-default, #D9D9D9)',
                    fontSize: 'var(--font-size-xs, 12px)',
                    cursor: 'pointer',
                    whiteSpace: 'nowrap',
                  }}
                >
                  Source repository
                </button>
              )}
            </div>

            {/* Channel selector & Refresh versions */}
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                paddingTop: '8px',
                borderTop: '1px solid var(--settings-border-subtle, #2A2A2A)',
              }}
            >
              <div style={{ display: 'flex', gap: '4px' }}>
                {(['stable', 'nightly'] as ToolChannel[]).map((ch) => (
                  <button
                    key={ch}
                    type="button"
                    data-testid={`channel-button-${ch}`}
                    onClick={() => commands.selectToolChannel(ch)}
                    style={{
                      padding: '6px 14px',
                      borderRadius: 'var(--radius-sm, 4px)',
                      fontSize: 'var(--font-size-xs, 12px)',
                      fontWeight: modal.selectedChannel === ch ? 600 : 400,
                      backgroundColor:
                        modal.selectedChannel === ch
                          ? 'var(--color-primary, #F15025)'
                          : 'var(--settings-bg-raised, #222222)',
                      color:
                        modal.selectedChannel === ch
                          ? 'var(--color-on-primary, #EDEDED)'
                          : 'var(--color-text-default, #D9D9D9)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      cursor: 'pointer',
                      textTransform: 'capitalize',
                    }}
                  >
                    {ch}
                  </button>
                ))}
              </div>

              <button
                type="button"
                data-testid="refresh-versions-button"
                onClick={() => commands.refreshToolVersions()}
                style={{
                  padding: '6px 12px',
                  backgroundColor: 'var(--settings-bg-raised, #222222)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  color: 'var(--color-text-default, #D9D9D9)',
                  fontSize: 'var(--font-size-xs, 12px)',
                  cursor: 'pointer',
                }}
              >
                {t('tools.refreshVersions')}
              </button>
            </div>

            {/* Version picker */}
            <div>
              <label
                htmlFor="tool-version-select"
                style={{
                  display: 'block',
                  fontSize: 'var(--font-size-xs, 12px)',
                  fontWeight: 600,
                  color: 'var(--color-text-dimmed, #808080)',
                  marginBottom: '6px',
                }}
              >
                {t('tools.availableVersions')}
              </label>
              {modal.availableVersions.length === 0 ? (
                <div style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}>
                  {t('tools.noVersionsLoadedClickRefreshVersionsTo')}
                </div>
              ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
                  <select
                    id="tool-version-select"
                    data-testid="tool-version-select"
                    aria-label="Available versions"
                    value={modal.selectedVersion ?? ''}
                    onChange={(e) => commands.selectToolVersion(e.target.value)}
                    style={{
                      width: '100%',
                      backgroundColor: 'var(--settings-bg-base, #111111)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      padding: '8px 12px',
                      color: 'var(--color-text-default, #D9D9D9)',
                      fontSize: 'var(--font-size-sm, 14px)',
                    }}
                  >
                    {modal.availableVersions.map((v) => (
                      <option key={v.version} value={v.version}>
                        {v.tagName || v.version} {v.publishedAt ? `(${v.publishedAt})` : ''}
                        {v.cachedPath ? ' [Cached]' : ''}
                      </option>
                    ))}
                  </select>

                  <div style={{ display: 'flex', flexDirection: 'column', gap: '4px', maxHeight: '140px', overflowY: 'auto' }}>
                    {modal.availableVersions.map((v) => {
                      const isSelected = v.version === modal.selectedVersion;
                      return (
                        <div
                          key={v.version}
                          data-testid={`version-option-${v.version}`}
                          onClick={() => commands.selectToolVersion(v.version)}
                          style={{
                            padding: '6px 10px',
                            backgroundColor: isSelected
                              ? 'var(--settings-bg-raised, #222222)'
                              : 'var(--settings-bg-base, #111111)',
                            border: `1px solid ${
                              isSelected ? 'var(--color-primary, #F15025)' : 'var(--settings-border-subtle, #2A2A2A)'
                            }`,
                            borderRadius: 'var(--radius-sm, 4px)',
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'space-between',
                            cursor: 'pointer',
                          }}
                        >
                          <div>
                            <span style={{ fontWeight: isSelected ? 600 : 400, fontSize: 'var(--font-size-xs, 12px)' }}>
                              {v.tagName || v.version}
                            </span>
                            {v.publishedAt && (
                              <span
                                style={{
                                  fontSize: 'var(--font-size-xs, 12px)',
                                  color: 'var(--color-text-dimmed, #808080)',
                                  marginLeft: '8px',
                                }}
                              >
                                {v.publishedAt}
                              </span>
                            )}
                          </div>
                          {v.cachedPath && (
                            <span
                              data-testid={`cached-badge-${v.version}`}
                              style={{
                                padding: '1px 6px',
                                borderRadius: 'var(--radius-xs, 2px)',
                                fontSize: '10px',
                                backgroundColor: 'var(--color-success-subtle, #3DD68C1F)',
                                color: 'var(--color-success, #3DD68C)',
                                fontWeight: 600,
                              }}
                            >
                              Cached
                            </span>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>

            {/* Target executable selector */}
            {modal.inspection?.candidates && modal.inspection.candidates.length > 0 && (
              <div>
                <label
                  htmlFor="target-exe-select"
                  style={{
                    display: 'block',
                    fontSize: 'var(--font-size-xs, 12px)',
                    fontWeight: 600,
                    color: 'var(--color-text-dimmed, #808080)',
                    marginBottom: '6px',
                  }}
                >
                  Target executable
                </label>
                <select
                  id="target-exe-select"
                  data-testid="target-exe-select"
                  aria-label="Target executable"
                  value={modal.inspection.selectedExe ?? modal.inspection.candidates[0]?.relativeExe ?? ''}
                  onChange={(e) => commands.selectToolTargetExe(e.target.value)}
                  style={{
                    width: '100%',
                    backgroundColor: 'var(--settings-bg-base, #111111)',
                    border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                    borderRadius: 'var(--radius-sm, 4px)',
                    padding: '8px 12px',
                    color: 'var(--color-text-default, #D9D9D9)',
                    fontSize: 'var(--font-size-sm, 14px)',
                    marginBottom: '6px',
                  }}
                >
                  {modal.inspection.candidates.map((cand) => (
                    <option key={cand.relativeExe} value={cand.relativeExe}>
                      {cand.relativeExe} {cand.architecture ? `(${cand.architecture})` : ''}
                      {cand.preferred ? ' [Recommended]' : ''}
                    </option>
                  ))}
                </select>

                <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px' }}>
                  {modal.inspection.candidates.map((cand) => {
                    const isSelected =
                      cand.relativeExe ===
                      (modal.inspection?.selectedExe ?? modal.inspection?.candidates[0]?.relativeExe);
                    return (
                      <button
                        key={cand.relativeExe}
                        type="button"
                        data-testid={`target-candidate-${cand.relativeExe}`}
                        onClick={() => commands.selectToolTargetExe(cand.relativeExe)}
                        style={{
                          padding: '6px 10px',
                          borderRadius: 'var(--radius-sm, 4px)',
                          fontSize: 'var(--font-size-xs, 12px)',
                          backgroundColor: isSelected
                            ? 'var(--color-primary, #F15025)'
                            : 'var(--settings-bg-base, #111111)',
                          color: isSelected
                            ? 'var(--color-on-primary, #EDEDED)'
                            : 'var(--color-text-default, #D9D9D9)',
                          border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                          cursor: 'pointer',
                          display: 'flex',
                          alignItems: 'center',
                          gap: '6px',
                        }}
                      >
                        <span>{cand.label || cand.relativeExe}</span>
                        {cand.architecture && (
                          <span
                            style={{
                              fontSize: '10px',
                              opacity: 0.8,
                              padding: '1px 4px',
                              backgroundColor: 'rgba(0,0,0,0.2)',
                              borderRadius: '2px',
                            }}
                          >
                            {cand.architecture}
                          </span>
                        )}
                        {cand.preferred && (
                          <span
                            data-testid={`preferred-badge-${cand.relativeExe}`}
                            style={{
                              fontSize: '10px',
                              fontWeight: 600,
                              padding: '1px 4px',
                              backgroundColor: isSelected ? '#FFFFFF33' : 'var(--color-primary, #F15025)',
                              color: '#FFFFFF',
                              borderRadius: '2px',
                            }}
                          >
                            Preferred
                          </span>
                        )}
                      </button>
                    );
                  })}
                </div>
              </div>
            )}

            {/* Proxy DLL selector */}
            <div>
              <label
                htmlFor="proxy-dll-select"
                style={{
                  display: 'block',
                  fontSize: 'var(--font-size-xs, 12px)',
                  fontWeight: 600,
                  color: 'var(--color-text-dimmed, #808080)',
                  marginBottom: '6px',
                }}
              >
                {t('tools.proxyDllName')}
              </label>
              <select
                id="proxy-dll-select"
                data-testid="proxy-dll-select"
                aria-label="Proxy DLL"
                value={modal.selectedProxy}
                onChange={(e) => commands.selectToolProxy(e.target.value)}
                style={{
                  width: '100%',
                  backgroundColor: 'var(--settings-bg-base, #111111)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  padding: '8px 12px',
                  color: 'var(--color-text-default, #D9D9D9)',
                  fontSize: 'var(--font-size-sm, 14px)',
                  marginBottom: '6px',
                }}
              >
                {PROXY_DLL_OPTIONS.map((dll) => (
                  <option key={dll} value={dll}>
                    {dll} {dll === 'dxgi.dll' ? '(Recommended)' : ''}
                  </option>
                ))}
              </select>

              <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px' }}>
                {PROXY_DLL_OPTIONS.map((dll) => {
                  const isSelected = dll === modal.selectedProxy;
                  return (
                    <button
                      key={dll}
                      type="button"
                      data-testid={`proxy-option-${dll}`}
                      onClick={() => commands.selectToolProxy(dll)}
                      style={{
                        padding: '4px 10px',
                        borderRadius: 'var(--radius-sm, 4px)',
                        fontSize: 'var(--font-size-xs, 12px)',
                        backgroundColor: isSelected
                          ? 'var(--color-primary, #F15025)'
                          : 'var(--settings-bg-base, #111111)',
                        color: isSelected
                          ? 'var(--color-on-primary, #EDEDED)'
                          : 'var(--color-text-default, #D9D9D9)',
                        border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                        cursor: 'pointer',
                      }}
                    >
                      {dll}
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Reset configuration checkbox */}
            <div style={{ paddingTop: '8px', borderTop: '1px solid var(--settings-border-subtle, #2A2A2A)' }}>
              <label
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: '8px',
                  cursor: 'pointer',
                  fontSize: 'var(--font-size-sm, 14px)',
                }}
              >
                <input
                  type="checkbox"
                  data-testid="reset-config-checkbox"
                  onChange={(e) => commands.setToolResetConfig(e.target.checked)}
                  style={{ cursor: 'pointer' }}
                />
                <span>
                  {t('tools.resetOptiscalerIniToDefaultsOnUpdate')}
                </span>
              </label>
            </div>
          </div>
        </div>

        {/* Footer actions */}
        <div
          style={{
            padding: '16px 24px',
            borderTop: '1px solid var(--settings-border-subtle, #2A2A2A)',
            display: 'flex',
            justifyContent: 'flex-end',
            gap: '8px',
          }}
        >
          <button
            type="button"
            onClick={handleClose}
            style={{
              padding: '8px 16px',
              backgroundColor: 'var(--settings-bg-raised, #222222)',
              border: '1px solid var(--settings-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              color: 'var(--color-text-default, #D9D9D9)',
              cursor: 'pointer',
            }}
          >
            {t('tools.close')}
          </button>

          {isInstalled && (
            <button
              type="button"
              data-testid="tool-uninstall-button"
              onClick={() => commands.uninstallTool()}
              disabled={isBusy}
              style={{
                padding: '8px 16px',
                backgroundColor: 'var(--color-danger, #E5484D)',
                border: 'none',
                borderRadius: 'var(--radius-sm, 4px)',
                color: '#FFFFFF',
                fontWeight: 600,
                cursor: isBusy ? 'not-allowed' : 'pointer',
                opacity: isBusy ? 0.5 : 1,
              }}
            >
              {t('tools.uninstall')}
            </button>
          )}

          {isInstalled ? (
            <button
              type="button"
              data-testid="tool-update-button"
              onClick={() => commands.installOrUpdateTool(true)}
              disabled={isBusy || anticheatBlocked}
              style={{
                padding: '8px 20px',
                backgroundColor: 'var(--color-primary, #F15025)',
                border: 'none',
                borderRadius: 'var(--radius-sm, 4px)',
                color: 'var(--color-on-primary, #EDEDED)',
                fontWeight: 600,
                cursor: isBusy || anticheatBlocked ? 'not-allowed' : 'pointer',
                opacity: isBusy || anticheatBlocked ? 0.5 : 1,
              }}
            >
              {t('tools.updateApply')}
            </button>
          ) : (
            <button
              type="button"
              data-testid="tool-install-button"
              onClick={() => commands.installOrUpdateTool(false)}
              disabled={isBusy || anticheatBlocked}
              style={{
                padding: '8px 20px',
                backgroundColor: 'var(--color-primary, #F15025)',
                border: 'none',
                borderRadius: 'var(--radius-sm, 4px)',
                color: 'var(--color-on-primary, #EDEDED)',
                fontWeight: 600,
                cursor: isBusy || anticheatBlocked ? 'not-allowed' : 'pointer',
                opacity: isBusy || anticheatBlocked ? 0.5 : 1,
              }}
            >
              {t('tools.installOptiscaler')}
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
