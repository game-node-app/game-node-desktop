import React, { useEffect, useRef, useState } from 'react';
import { IconFolderOpen, IconTrash } from '@tabler/icons-react';
import { ConfirmDialog } from '../../components/dialog';
import { Button, RadioCard, RadioGroup } from '../../components/ui';
import {
  commands,
  useLibraryView,
  useLocalGames,
  type Registration,
} from '../../api';
import { uiStore, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';

export interface RemoveLocalGameModalProps {
  open?: boolean;
  entryKey?: string | null;
  registration?: Registration | null;
  onClose?: () => void;
}

/**
 * D09: removing a game is one destructive action behind two explicit radio
 * cards. "Remove and don't import again" stays preselected
 * (`allowRediscovery = false`), matching the safer default and the existing
 * specification scenario; Figma's frame shows sample state.
 */
export const RemoveLocalGameModal: React.FC<RemoveLocalGameModalProps> = ({
  open: propOpen,
  entryKey: propEntryKey,
  registration: propRegistration,
  onClose,
}) => {
  const { t } = useI18n();
  const library = useLibraryView();
  const localGames = useLocalGames();
  const removeModalEntryKey = useUiStore((s) => s.removeModalEntryKey);
  const cancelRef = useRef<HTMLButtonElement>(null);

  const effectiveEntryKey =
    propEntryKey ??
    (propRegistration
      ? propRegistration.id.startsWith('manual:')
        ? propRegistration.id
        : `manual:${propRegistration.id}`
      : removeModalEntryKey);

  const isOpen = propOpen !== undefined ? propOpen : effectiveEntryKey != null;

  const [allowRediscovery, setAllowRediscovery] = useState(false);

  const isManual = effectiveEntryKey?.startsWith('manual:') ?? false;
  const localId =
    isManual && effectiveEntryKey ? effectiveEntryKey.replace(/^manual:/, '') : null;
  const registration =
    propRegistration ??
    (localId
      ? localGames.registrations.find((r) => r.id === localId)
      : undefined);

  // Store entries have no local registration, so fall back to the visible row title.
  const rowTitle = effectiveEntryKey
    ? library?.rows.find((r) => r.key === effectiveEntryKey)?.title
    : undefined;
  const title = registration?.title ?? rowTitle ?? 'this game';

  // Every opening starts on the default choice again, even after a cancelled attempt.
  useEffect(() => {
    if (isOpen) setAllowRediscovery(false);
  }, [isOpen, effectiveEntryKey]);

  const handleClose = () => {
    uiStore.setState((s) => ({ ...s, removeModalEntryKey: null }));
    onClose?.();
  };

  const handleConfirm = () => {
    if (effectiveEntryKey) {
      commands.removeEntry(effectiveEntryKey, allowRediscovery);
    }
    // Only DRM-free registrations have a local record to mark as removed.
    if (localId) {
      commands.removeLocalGame(localId, allowRediscovery);
    }
    handleClose();
  };

  if (!isOpen || !effectiveEntryKey) {
    return null;
  }

  const choice = allowRediscovery ? 'rediscover' : 'suppress';
  const justRemoveDescription = isManual
    ? t('drmFree.justRemoveDescription')
    : t('drmFree.storeJustRemoveDescription');
  const suppressDescription = isManual
    ? t('drmFree.removeAndDontImportAgainDescription')
    : t('drmFree.storeRemoveAndDontImportAgainDescription');

  return (
    <ConfirmDialog
      tone="danger"
      icon={IconTrash}
      size={460}
      title={t('drmFree.removeGameTitle', { game: title })}
      description={t('drmFree.removeGameDescription')}
      initialFocusRef={cancelRef}
      onClose={handleClose}
      testId="remove-local-game-dialog"
      actions={
        <>
          {registration?.root ? (
            <Button
              variant="ghost"
              size="sm"
              leftIcon={IconFolderOpen}
              data-testid="remove-open-folder"
              onClick={() => void commands.openInstallFolder(registration.root)}
            >
              {t('drmFree.openFolder')}
            </Button>
          ) : null}
          <span aria-hidden="true" className="flex-1" />
          <Button
            ref={cancelRef}
            variant="secondary"
            size="sm"
            data-testid="remove-cancel-button"
            onClick={handleClose}
          >
            {t('drmFree.cancel')}
          </Button>
          <Button
            variant="danger"
            size="sm"
            leftIcon={IconTrash}
            data-testid="confirm-remove-button"
            onClick={handleConfirm}
          >
            {t('drmFree.remove')}
          </Button>
        </>
      }
    >
      <RadioGroup
        aria-label={t('drmFree.removeFromLibrary')}
        value={choice}
        onChange={(next) => setAllowRediscovery(next === 'rediscover')}
        className="flex flex-col gap-compact"
      >
        <RadioCard
          value="rediscover"
          title={t('drmFree.justRemove')}
          description={justRemoveDescription}
          data-testid="remove-choice-rediscovery"
        />
        <RadioCard
          value="suppress"
          title={t('drmFree.removeAndDontImportAgain')}
          description={suppressDescription}
          data-testid="remove-choice-suppress"
        />
      </RadioGroup>
    </ConfirmDialog>
  );
};
