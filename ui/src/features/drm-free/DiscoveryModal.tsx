import React, { useEffect } from 'react';
import { commands, useLocalGames } from '../../api';
import { uiStore, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';

export interface DiscoveryModalProps {
  open?: boolean;
  onClose?: () => void;
}

export const DiscoveryModal: React.FC<DiscoveryModalProps> = ({ open: propOpen, onClose }) => {
  const { t } = useI18n();
  const localGames = useLocalGames();
  const discoveryModalOpen = useUiStore((s) => s.discoveryModalOpen);

  const isOpen = propOpen !== undefined ? propOpen : discoveryModalOpen;

  const handleClose = () => {
    commands.dismissLocalDiscovered();
    uiStore.setState((s) => ({ ...s, discoveryModalOpen: false }));
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

  if (!isOpen) {
    return null;
  }

  const activeCandidates = localGames.candidates.filter((c) => !c.ignored);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="discovery-modal-heading"
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
            <h2 id="discovery-modal-heading" style={{ fontSize: 'var(--font-size-lg, 18px)', fontWeight: 600 }}>
              {t('drmFree.watchedFolders')}
            </h2>
            <div style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}>
              {activeCandidates.length === 1
                ? '1 game discovered'
                : `${activeCandidates.length} games discovered`}
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
            gap: '16px',
          }}
        >
          {/* Scanning banner */}
          {localGames.discoveryScanning && (
            <div
              data-testid="discovery-scanning-banner"
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
              <span>{t('drmFree.checkingWatchedFolders')}</span>
            </div>
          )}

          {/* Error banner */}
          {localGames.discoveryError && (
            <div
              data-testid="discovery-error-banner"
              style={{
                padding: '12px 16px',
                backgroundColor: 'var(--color-danger-subtle, #E5484D1F)',
                border: '1px solid var(--color-danger, #E5484D)',
                borderRadius: 'var(--radius-sm, 4px)',
                color: 'var(--color-text-default, #D9D9D9)',
              }}
            >
              {localGames.discoveryError}
            </div>
          )}

          {/* Incomplete warning banner */}
          {localGames.discoveryIncomplete.length > 0 && (
            <div
              data-testid="discovery-incomplete-banner"
              style={{
                padding: '12px 16px',
                backgroundColor: 'var(--color-warning-subtle, #F5A5241F)',
                border: '1px solid var(--color-warning, #F5A524)',
                borderRadius: 'var(--radius-sm, 4px)',
                color: 'var(--color-text-default, #D9D9D9)',
              }}
            >
              Discovery incomplete in: {localGames.discoveryIncomplete.join(', ')}
            </div>
          )}

          {/* Candidates list */}
          {activeCandidates.length === 0 ? (
            <div
              style={{
                padding: '32px',
                textAlign: 'center',
                color: 'var(--color-text-dimmed, #808080)',
              }}
            >
              No discovered games found in watched folders.
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
              {activeCandidates.map((candidate) => {
                const isSelected = localGames.reviewSelected.includes(candidate.fingerprint);
                const currentExe = candidate.selectedTarget ?? candidate.targets[0] ?? '';

                return (
                  <div
                    key={candidate.fingerprint}
                    data-testid={`candidate-row-${candidate.fingerprint}`}
                    style={{
                      padding: '14px 16px',
                      backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      display: 'flex',
                      flexDirection: 'column',
                      gap: '8px',
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'flex-start', gap: '12px' }}>
                      <input
                        type="checkbox"
                        aria-label={`Select ${candidate.title}`}
                        data-testid={`candidate-checkbox-${candidate.fingerprint}`}
                        checked={isSelected}
                        onChange={() => commands.selectLocalCandidate(candidate.fingerprint)}
                        style={{ marginTop: '4px', cursor: 'pointer' }}
                      />
                      <div style={{ flex: 1 }}>
                        <div style={{ fontWeight: 600, fontSize: 'var(--font-size-md, 16px)' }}>
                          {candidate.title}
                        </div>
                        <div
                          style={{
                            fontSize: 'var(--font-size-xs, 12px)',
                            color: 'var(--color-text-dimmed, #808080)',
                            wordBreak: 'break-all',
                          }}
                        >
                          {candidate.root}
                        </div>
                      </div>
                    </div>

                    {/* Target Executable selection */}
                    {candidate.targets.length > 0 && (
                      <div
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          gap: '8px',
                          paddingLeft: '28px',
                        }}
                      >
                        <span
                          style={{
                            fontSize: 'var(--font-size-xs, 12px)',
                            color: 'var(--color-text-dimmed, #808080)',
                            whiteSpace: 'nowrap',
                          }}
                        >
                          {t('drmFree.executable')}:
                        </span>
                        {candidate.targets.length === 1 ? (
                          <span style={{ fontSize: 'var(--font-size-xs, 12px)' }}>
                            {candidate.targets[0]}
                          </span>
                        ) : (
                          <select
                            aria-label={`Target executable for ${candidate.title}`}
                            data-testid={`candidate-target-select-${candidate.fingerprint}`}
                            value={currentExe}
                            onChange={(e) =>
                              commands.setLocalCandidateExecutable(
                                candidate.fingerprint,
                                e.target.value,
                              )
                            }
                            style={{
                              flex: 1,
                              backgroundColor: 'var(--settings-bg-raised, #222222)',
                              border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                              borderRadius: 'var(--radius-sm, 4px)',
                              padding: '4px 8px',
                              color: 'var(--color-text-default, #D9D9D9)',
                              fontSize: 'var(--font-size-xs, 12px)',
                            }}
                          >
                            {candidate.targets.map((tgt) => (
                              <option key={tgt} value={tgt}>
                                {tgt}
                              </option>
                            ))}
                          </select>
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* Footer */}
        <div
          style={{
            padding: '16px 24px',
            borderTop: '1px solid var(--settings-border-subtle, #2A2A2A)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: '8px',
          }}
        >
          <button
            type="button"
            data-testid="rescan-folders-button"
            aria-label={t('drmFree.rescanWatchedFolders')}
            onClick={() => commands.rescanLocalFolders()}
            style={{
              padding: '8px 16px',
              backgroundColor: 'var(--settings-bg-raised, #222222)',
              border: '1px solid var(--settings-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              color: 'var(--color-text-default, #D9D9D9)',
              cursor: 'pointer',
            }}
          >
            {t('drmFree.rescanWatchedFolders')}
          </button>

          <div style={{ display: 'flex', gap: '8px' }}>
            <button
              type="button"
              data-testid="ignore-selected-button"
              onClick={() => commands.reviewSelectedLocalCandidates(false)}
              disabled={localGames.reviewSelected.length === 0}
              style={{
                padding: '8px 16px',
                backgroundColor: 'var(--settings-bg-raised, #222222)',
                border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                borderRadius: 'var(--radius-sm, 4px)',
                color: 'var(--color-text-default, #D9D9D9)',
                cursor: localGames.reviewSelected.length === 0 ? 'not-allowed' : 'pointer',
                opacity: localGames.reviewSelected.length === 0 ? 0.5 : 1,
              }}
            >
              {t('drmFree.ignoreSelected')}
            </button>
            <button
              type="button"
              data-testid="add-selected-button"
              onClick={() => commands.reviewSelectedLocalCandidates(true)}
              disabled={localGames.reviewSelected.length === 0}
              style={{
                padding: '8px 16px',
                backgroundColor: 'var(--color-primary, #F15025)',
                border: 'none',
                borderRadius: 'var(--radius-sm, 4px)',
                color: 'var(--color-on-primary, #EDEDED)',
                fontWeight: 600,
                cursor: localGames.reviewSelected.length === 0 ? 'not-allowed' : 'pointer',
                opacity: localGames.reviewSelected.length === 0 ? 0.5 : 1,
              }}
            >
              {t('drmFree.addSelected')}
            </button>
            <button
              type="button"
              data-testid="discovery-close-button"
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
              {t('drmFree.close')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
