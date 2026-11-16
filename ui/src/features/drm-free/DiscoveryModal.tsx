import React, { useEffect, useState } from 'react';
import {
  IconAlertTriangle,
  IconLink,
  IconLoader2,
  IconPlus,
} from '@tabler/icons-react';
import { commands, useLocalGames } from '../../api';
import type { Candidate } from '../../api/types';
import { DialogShell } from '../../components/dialog';
import { Button, Checkbox, GameCover, Pill, SelectField } from '../../components/ui';
import { uiStore, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';

export interface DiscoveryModalProps {
  open?: boolean;
  onClose?: () => void;
}

function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export const DiscoveryModal: React.FC<DiscoveryModalProps> = ({ open: propOpen, onClose }) => {
  const { t } = useI18n();
  const localGames = useLocalGames();
  const discoveryModalOpen = useUiStore((s) => s.discoveryModalOpen);
  const [allowRediscovery, setAllowRediscovery] = useState(true);

  const isOpen = propOpen !== undefined ? propOpen : discoveryModalOpen;

  useEffect(() => {
    if (isOpen) {
      setAllowRediscovery(true);
    }
  }, [isOpen]);

  const handleClose = () => {
    commands.dismissLocalDiscovered();
    uiStore.setState((s) => ({ ...s, discoveryModalOpen: false }));
    onClose?.();
  };

  if (!isOpen) {
    return null;
  }

  const activeCandidates = localGames.candidates.filter((candidate) => !candidate.ignored);
  const selected = new Set(localGames.reviewSelected);
  const selectedCount = activeCandidates.filter((candidate) =>
    selected.has(candidate.fingerprint),
  ).length;
  const allSelected = activeCandidates.length > 0 && selectedCount === activeCandidates.length;
  const someSelected = selectedCount > 0 && !allSelected;

  const handleToggle = (fingerprint: string) => {
    commands.selectLocalCandidate(fingerprint);
  };

  const handleToggleAll = (checked: boolean) => {
    for (const candidate of activeCandidates) {
      const isSelected = selected.has(candidate.fingerprint);
      if (checked !== isSelected) {
        commands.selectLocalCandidate(candidate.fingerprint);
      }
    }
  };

  const handleTarget = (fingerprint: string, executable: string) => {
    commands.setLocalCandidateExecutable(fingerprint, executable);
  };

  const handleAdd = () => {
    commands.reviewSelectedLocalCandidates(true, allowRediscovery);
  };

  const handleIgnore = () => {
    commands.reviewSelectedLocalCandidates(false, allowRediscovery);
  };

  const gamenodeCell = (candidate: Candidate) => {
    const suggestion = candidate.suggestion;
    if (candidate.likelyNonGame) {
      return (
        <Pill tone="neutral" icon={IconAlertTriangle}>
          {t('drmFree.probablyNotAGame')}
        </Pill>
      );
    }
    if (suggestion) {
      const label = suggestion.year
        ? `${suggestion.title} (${suggestion.year})`
        : suggestion.title;
      return (
        <Pill tone={suggestion.tone} icon={IconLink}>
          {label}
        </Pill>
      );
    }
    return (
      <Pill tone="warning" icon={IconAlertTriangle}>
        {t('drmFree.noMatch')}
      </Pill>
    );
  };

  const subtitle =
    activeCandidates.length === 1
      ? t('drmFree.oneNewGameInWatchedFolders')
      : t('drmFree.newGamesInWatchedFolders', { n: activeCandidates.length });
  const addLabel =
    selectedCount === 1
      ? t('drmFree.addOneGame')
      : t('drmFree.addNGames', { n: selectedCount });

  return (
    <DialogShell
      title={t('drmFree.discoveredGames')}
      subtitle={activeCandidates.length > 0 ? subtitle : undefined}
      onClose={handleClose}
      geometryKey="discovery"
      defaultSize={{ width: 720, height: 460 }}
      minSize={{ width: 600, height: 320 }}
      testId="discovery-modal"
      closeTestId="discovery-close-button"
      bodyClassName="!gap-sm !px-lg !py-compact"
      footerLeading={
        <label className="flex items-center gap-compact text-xs text-text-secondary">
          <Checkbox
            checked={allowRediscovery}
            disabled={selectedCount === 0}
            onChange={(event) => setAllowRediscovery(event.currentTarget.checked)}
            aria-label={t('drmFree.allowRediscoveryFutureScans')}
            data-testid="allow-rediscovery-checkbox"
          />
          <span>{t('drmFree.allowRediscoveryFutureScans')}</span>
        </label>
      }
      footer={
        <div className="flex items-center gap-compact">
          <Button
            variant="ghost"
            size="sm"
            onClick={handleIgnore}
            disabled={selectedCount === 0}
            data-testid="ignore-selected-button"
          >
            {t('drmFree.ignoreSelected')}
          </Button>
          <Button
            variant="primary"
            size="sm"
            leftIcon={IconPlus}
            onClick={handleAdd}
            disabled={selectedCount === 0}
            data-testid="add-selected-button"
          >
            {addLabel}
          </Button>
        </div>
      }
    >
      {localGames.discoveryScanning && (
        <div
          data-testid="discovery-scanning-banner"
          className="flex items-center gap-compact rounded-sm border border-brand bg-accent-subtle px-md py-sm text-text-default"
        >
          <IconLoader2
            size={16}
            stroke={1.5}
            className="animate-spin text-brand motion-reduce:animate-none"
            aria-hidden="true"
          />
          <span>{t('drmFree.checkingWatchedFolders')}</span>
        </div>
      )}

      {localGames.discoveryError && (
        <div
          data-testid="discovery-error-banner"
          className="rounded-sm border border-danger bg-danger-subtle px-md py-sm text-text-default"
        >
          {localGames.discoveryError}
        </div>
      )}

      {localGames.discoveryIncomplete.length > 0 && (
        <div
          data-testid="discovery-incomplete-banner"
          className="rounded-sm border border-warning bg-warning-subtle px-md py-sm text-text-default"
        >
          {t('drmFree.discoveryIncompleteIn', {
            folders: localGames.discoveryIncomplete.join(', '),
          })}
        </div>
      )}

      {activeCandidates.length === 0 ? (
        <div className="py-xl text-center text-xs text-text-dimmed">
          {t('drmFree.noDiscoveredGames')}
        </div>
      ) : (
        <div className="min-w-0 overflow-x-auto">
          <table
            data-testid="discovery-table"
            className="w-full table-fixed border-collapse"
          >
            <colgroup>
              <col className="w-4" />
              <col className="w-[42px]" />
              <col />
              <col className="w-[190px]" />
              <col className="w-[170px]" />
            </colgroup>
            <thead>
              <tr className="border-b border-border-subtle">
                <th scope="col" className="py-compact pr-compact text-left align-middle">
                  <Checkbox
                    checked={allSelected}
                    indeterminate={someSelected}
                    disabled={activeCandidates.length === 0}
                    onChange={(event) => handleToggleAll(event.currentTarget.checked)}
                    aria-label={t('drmFree.selectAllCandidates')}
                    data-testid="candidate-select-all"
                  />
                </th>
                <th scope="col" className="py-compact pr-compact" aria-hidden="true" />
                <th scope="col" className="py-compact pr-compact text-left text-caption font-semibold uppercase tracking-overline text-text-muted">
                  {t('drmFree.gameColumn')}
                </th>
                <th scope="col" className="py-compact pr-compact text-left text-caption font-semibold uppercase tracking-overline text-text-muted">
                  {t('drmFree.executableColumn')}
                </th>
                <th scope="col" className="py-compact text-left text-caption font-semibold uppercase tracking-overline text-text-muted">
                  {t('drmFree.gamenodeColumn')}
                </th>
              </tr>
            </thead>
            <tbody>
              {activeCandidates.map((candidate) => {
                const currentExe =
                  candidate.selectedTarget ?? candidate.targets[0] ?? '';
                return (
                  <tr
                    key={candidate.fingerprint}
                    data-testid={`candidate-row-${candidate.fingerprint}`}
                    className="border-b border-border-subtle last:border-b-0"
                  >
                    <td className="py-[10px] pr-compact align-middle">
                      <Checkbox
                        checked={selected.has(candidate.fingerprint)}
                        onChange={() => handleToggle(candidate.fingerprint)}
                        aria-label={t('drmFree.selectCandidate', {
                          game: candidate.title,
                        })}
                        data-testid={`candidate-checkbox-${candidate.fingerprint}`}
                      />
                    </td>
                    <td className="py-[10px] pr-compact align-middle">
                      <div className="h-10 w-[30px] overflow-hidden rounded-thumb">
                        <GameCover
                          src={candidate.suggestion?.coverUrl ?? null}
                          title={candidate.title}
                        />
                      </div>
                    </td>
                    <td className="min-w-0 py-[10px] pr-compact align-middle">
                      <div className="flex min-w-0 flex-col gap-0.5">
                        <span className="truncate text-body font-semibold text-text-primary">
                          {candidate.title}
                        </span>
                        <span
                          className="truncate text-caption text-text-dimmed"
                          title={candidate.root}
                        >
                          {candidate.root}
                        </span>
                      </div>
                    </td>
                    <td className="py-[10px] pr-compact align-middle">
                      <SelectField
                        size="compact"
                        options={candidate.targets.map((target) => ({
                          value: target,
                          label: fileName(target),
                        }))}
                        value={currentExe}
                        onChange={(value) =>
                          handleTarget(candidate.fingerprint, value)
                        }
                        aria-label={t('drmFree.targetExecutableFor', {
                          game: candidate.title,
                        })}
                        data-testid={`candidate-target-select-${candidate.fingerprint}`}
                      />
                    </td>
                    <td className="min-w-0 py-[10px] align-middle">
                      <div
                        data-testid={`candidate-gamenode-${candidate.fingerprint}`}
                        className="min-w-0"
                      >
                        {gamenodeCell(candidate)}
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </DialogShell>
  );
};
