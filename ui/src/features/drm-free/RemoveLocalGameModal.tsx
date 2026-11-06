import React, { useState } from 'react';
import { IconX } from '@tabler/icons-react';
import { AppModal } from '../../components/AppModal';
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

  return (
    <AppModal.Root opened onClose={handleClose} centered size={560} transitionProps={{ duration: 0 }}>
      <AppModal.Overlay backgroundOpacity={0.62} />
      <AppModal.Content
        classNames={{
          content:
            'flex w-full flex-col overflow-hidden rounded-md border border-border-subtle bg-secondary shadow-alert',
        }}
      >
        {/* Header */}
        <AppModal.Header className="flex shrink-0 items-center justify-between gap-md border-b border-border-subtle bg-secondary px-control-inline py-md">
          <div>
            <AppModal.Title className="text-lg font-semibold text-text-default">
              {t('drmFree.removeFromLibrary')}
            </AppModal.Title>
            <div data-testid="remove-game-title-header" className="text-xs text-text-dimmed">
              {title}
            </div>
          </div>
          <AppModal.CloseButton
            aria-label="Close"
            icon={<IconX size={18} stroke={1.5} />}
            className="rounded-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default"
          />
        </AppModal.Header>

        {/* Body */}
        <AppModal.Body className="flex min-h-0 flex-1 flex-col gap-md overflow-y-auto p-control-inline">
          <div className="text-sm text-text-default">
            <p className="mb-compact">
              Are you sure you want to remove{' '}
              <strong data-testid="remove-game-target-title">{title}</strong> from your library?
            </p>
            <p className="text-text-dimmed">
              Game files and local save backups will not be deleted from your computer.
            </p>
          </div>

          <label className="flex cursor-pointer items-center gap-2.5 rounded-sm border border-border-subtle bg-raised px-3.5 py-sm text-sm text-text-default">
            <input
              type="checkbox"
              id="allow-rediscovery-checkbox"
              data-testid="allow-rediscovery-checkbox"
              checked={allowRediscovery}
              onChange={(e) => setAllowRediscovery(e.target.checked)}
              className="h-4 w-4 cursor-pointer accent-brand"
            />
            <span>
              {isManual
                ? t('drmFree.allowRediscoveryInWatchedFolders')
                : t('drmFree.allowRediscoveryOnFutureScans')}
            </span>
          </label>
        </AppModal.Body>

        {/* Footer */}
        <div className="flex shrink-0 justify-end gap-compact border-t border-border-subtle px-control-inline py-md">
          <button
            type="button"
            onClick={handleClose}
            className="inline-flex items-center justify-center rounded-sm px-md py-compact text-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            {t('drmFree.cancel')}
          </button>
          <button
            type="button"
            data-testid="confirm-remove-button"
            aria-label={t('drmFree.removeFromLibrary')}
            onClick={handleConfirm}
            className="inline-flex items-center justify-center rounded-sm border border-transparent bg-danger px-md py-compact text-sm font-semibold text-white transition-colors hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-danger"
          >
            {t('drmFree.removeFromLibrary')}
          </button>
        </div>
      </AppModal.Content>
    </AppModal.Root>
  );
};
