import React from 'react';
import { IconX } from '@tabler/icons-react';
import { AppModal } from '../../components/AppModal';
import { commands, useLocalGames, type PendingClaim } from '../../api';
import { useI18n } from '../../i18n';

export interface ClaimConflictModalProps {
  claim?: PendingClaim | null;
  onClose?: () => void;
}

export const ClaimConflictModal: React.FC<ClaimConflictModalProps> = ({ claim: propClaim, onClose }) => {
  const { t } = useI18n();
  const localGames = useLocalGames();

  const claim = propClaim !== undefined ? propClaim : localGames.pendingClaim;
  const isOpen = claim != null;

  const handleCancel = () => {
    commands.cancelLocalClaim();
    onClose?.();
  };

  const handleResolve = (transfer: boolean) => {
    commands.resolveLocalClaim(transfer);
    onClose?.();
  };

  if (!isOpen || !claim) {
    return null;
  }

  return (
    <AppModal.Root opened onClose={handleCancel} centered size={600} transitionProps={{ duration: 0 }}>
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
              {t('drmFree.thisGameIsAlreadyTrackedByA')}
            </AppModal.Title>
            <div className="text-xs text-text-dimmed">{claim.registration.title}</div>
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
              <strong>{claim.registration.title}</strong> was matched with store entry{' '}
              <strong>{claim.storeTitle}</strong>.
            </p>
            <p className="text-text-dimmed">
              {t('drmFree.useTheExistingEntryOrTransferTracking')}
            </p>
          </div>

          <div className="flex flex-col gap-1 rounded-sm border border-border-subtle bg-raised px-md py-sm text-xs">
            <div>
              <span className="text-text-dimmed">Manual Registration: </span>
              <span>{claim.registration.title}</span>
            </div>
            <div>
              <span className="text-text-dimmed">Matched Store Entry: </span>
              <span>{claim.storeTitle}</span>
            </div>
          </div>
        </AppModal.Body>

        {/* Footer */}
        <div className="flex shrink-0 justify-end gap-compact border-t border-border-subtle px-control-inline py-md">
          <button
            type="button"
            onClick={handleCancel}
            className="inline-flex items-center justify-center rounded-sm px-md py-compact text-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            {t('drmFree.cancel')}
          </button>
          <button
            type="button"
            data-testid="claim-resolve-separate"
            onClick={() => handleResolve(false)}
            className="inline-flex items-center justify-center rounded-sm border border-border-subtle bg-raised px-md py-compact text-sm font-medium text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            {t('drmFree.useExistingEntry')}
          </button>
          <button
            type="button"
            data-testid="claim-resolve-transfer"
            onClick={() => handleResolve(true)}
            className="inline-flex items-center justify-center rounded-sm border border-transparent bg-brand px-md py-compact text-sm font-semibold text-on-primary transition-colors hover:bg-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            {t('drmFree.transferTracking')}
          </button>
        </div>
      </AppModal.Content>
    </AppModal.Root>
  );
};
