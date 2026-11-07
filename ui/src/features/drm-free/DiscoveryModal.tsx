import React from 'react';
import { IconLoader2, IconX } from '@tabler/icons-react';
import { FloatingModal } from '../../components/FloatingModal';
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

  if (!isOpen) {
    return null;
  }

  const activeCandidates = localGames.candidates.filter((c) => !c.ignored);

  return (
    <FloatingModal.Root
      opened
      onClose={handleClose}
      geometryKey="discovery"
      defaultSize={{ width: 720, height: 560 }}
      minSize={{ width: 520, height: 400 }}
      zIndex={1000}
      closeOnClickOutside
    >
      <FloatingModal.Overlay backgroundOpacity={0.62} />
      <FloatingModal.Content className="flex flex-col bg-chrome">
        {/* Header */}
        <FloatingModal.Header className="bg-chrome px-control-inline py-md">
          <div>
            <FloatingModal.Title className="text-lg font-semibold text-text-default">
              {t('drmFree.watchedFolders')}
            </FloatingModal.Title>
            <div className="text-xs text-text-dimmed">
              {activeCandidates.length === 1
                ? '1 game discovered'
                : `${activeCandidates.length} games discovered`}
            </div>
          </div>
          <FloatingModal.CloseButton
            aria-label="Close"
            icon={<IconX size={18} stroke={1.5} />}
            className="rounded-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default"
          />
        </FloatingModal.Header>

        {/* Content */}
        <FloatingModal.Body className="gap-md p-control-inline">
          {/* Scanning banner */}
          {localGames.discoveryScanning && (
            <div
              data-testid="discovery-scanning-banner"
              className="flex items-center gap-compact rounded-sm border border-brand bg-accent-subtle px-md py-sm text-text-default"
            >
              <IconLoader2 size={16} stroke={1.5} className="animate-spin text-brand" aria-hidden="true" />
              <span>{t('drmFree.checkingWatchedFolders')}</span>
            </div>
          )}

          {/* Error banner */}
          {localGames.discoveryError && (
            <div
              data-testid="discovery-error-banner"
              className="rounded-sm border border-danger bg-danger-subtle px-md py-sm text-text-default"
            >
              {localGames.discoveryError}
            </div>
          )}

          {/* Incomplete warning banner */}
          {localGames.discoveryIncomplete.length > 0 && (
            <div
              data-testid="discovery-incomplete-banner"
              className="rounded-sm border border-warning bg-warning-subtle px-md py-sm text-text-default"
            >
              Discovery incomplete in: {localGames.discoveryIncomplete.join(', ')}
            </div>
          )}

          {/* Candidates list */}
          {activeCandidates.length === 0 ? (
            <div className="rounded-sm py-xl text-center text-text-dimmed">
              No discovered games found in watched folders.
            </div>
          ) : (
            <div className="flex flex-col gap-2.5">
              {activeCandidates.map((candidate) => {
                const isSelected = localGames.reviewSelected.includes(candidate.fingerprint);
                const currentExe = candidate.selectedTarget ?? candidate.targets[0] ?? '';

                return (
                  <div
                    key={candidate.fingerprint}
                    data-testid={`candidate-row-${candidate.fingerprint}`}
                    className="flex flex-col gap-compact rounded-sm border border-border-subtle bg-secondary px-md py-3.5"
                  >
                    <div className="flex items-start gap-sm">
                      <input
                        type="checkbox"
                        aria-label={`Select ${candidate.title}`}
                        data-testid={`candidate-checkbox-${candidate.fingerprint}`}
                        checked={isSelected}
                        onChange={() => commands.selectLocalCandidate(candidate.fingerprint)}
                        className="mt-1 h-4 w-4 cursor-pointer accent-brand"
                      />
                      <div className="flex-1">
                        <div className="text-md font-semibold text-text-default">
                          {candidate.title}
                        </div>
                        <div className="break-all text-xs text-text-dimmed">
                          {candidate.root}
                        </div>
                      </div>
                    </div>

                    {/* Target Executable selection */}
                    {candidate.targets.length > 0 && (
                      <div className="flex items-center gap-compact pl-7">
                        <span className="whitespace-nowrap text-xs text-text-dimmed">
                          {t('drmFree.executable')}:
                        </span>
                        {candidate.targets.length === 1 ? (
                          <span className="text-xs text-text-default">{candidate.targets[0]}</span>
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
                            className="flex-1 cursor-pointer rounded-sm border border-border-subtle bg-raised px-compact py-1 text-xs text-text-default focus:border-brand focus:outline-none focus:ring-1 focus:ring-brand"
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
        </FloatingModal.Body>

        {/* Footer */}
        <FloatingModal.Footer className="justify-between px-control-inline py-md">
          <button
            type="button"
            data-testid="rescan-folders-button"
            aria-label={t('drmFree.rescanWatchedFolders')}
            onClick={() => commands.rescanLocalFolders()}
            className="inline-flex items-center justify-center rounded-sm border border-border-subtle bg-raised px-md py-compact text-sm font-medium text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            {t('drmFree.rescanWatchedFolders')}
          </button>

          <div className="flex gap-compact">
            <button
              type="button"
              data-testid="ignore-selected-button"
              onClick={() => commands.reviewSelectedLocalCandidates(false)}
              disabled={localGames.reviewSelected.length === 0}
              className="inline-flex items-center justify-center rounded-sm border border-border-subtle bg-raised px-md py-compact text-sm font-medium text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-not-allowed disabled:opacity-50"
            >
              {t('drmFree.ignoreSelected')}
            </button>
            <button
              type="button"
              data-testid="add-selected-button"
              onClick={() => commands.reviewSelectedLocalCandidates(true)}
              disabled={localGames.reviewSelected.length === 0}
              className="inline-flex items-center justify-center rounded-sm border border-transparent bg-brand px-md py-compact text-sm font-semibold text-on-primary transition-colors hover:bg-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-not-allowed disabled:opacity-50"
            >
              {t('drmFree.addSelected')}
            </button>
            <button
              type="button"
              data-testid="discovery-close-button"
              onClick={handleClose}
              className="inline-flex items-center justify-center rounded-sm border border-border-subtle bg-raised px-md py-compact text-sm font-medium text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
            >
              {t('drmFree.close')}
            </button>
          </div>
        </FloatingModal.Footer>

        <FloatingModal.ResizeHandle />
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
