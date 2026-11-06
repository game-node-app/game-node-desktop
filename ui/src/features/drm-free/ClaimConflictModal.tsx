import React, { useEffect } from 'react';
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

  useEffect(() => {
    if (!isOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        handleCancel();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isOpen]);

  if (!isOpen || !claim) {
    return null;
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="claim-conflict-heading"
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
        if (e.target === e.currentTarget) handleCancel();
      }}
    >
      <div
        style={{
          width: '100%',
          maxWidth: '600px',
          backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
          border: '1px solid var(--settings-border-subtle, #2A2A2A)',
          borderRadius: 'var(--radius-md, 8px)',
          boxShadow: 'var(--shadow-alert)',
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
            <h2 id="claim-conflict-heading" style={{ fontSize: 'var(--font-size-lg, 18px)', fontWeight: 600 }}>
              {t('drmFree.thisGameIsAlreadyTrackedByA')}
            </h2>
            <div style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}>
              {claim.registration.title}
            </div>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={handleCancel}
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

        {/* Body */}
        <div
          style={{
            padding: '24px',
            display: 'flex',
            flexDirection: 'column',
            gap: '16px',
          }}
        >
          <div style={{ fontSize: 'var(--font-size-sm, 14px)', color: 'var(--color-text-default, #D9D9D9)' }}>
            <p style={{ marginBottom: '8px' }}>
              <strong>{claim.registration.title}</strong> was matched with store entry{' '}
              <strong>{claim.storeTitle}</strong>.
            </p>
            <p style={{ color: 'var(--color-text-dimmed, #808080)' }}>
              {t('drmFree.useTheExistingEntryOrTransferTracking')}
            </p>
          </div>

          <div
            style={{
              padding: '12px 16px',
              backgroundColor: 'var(--settings-bg-raised, #222222)',
              border: '1px solid var(--settings-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              fontSize: 'var(--font-size-xs, 12px)',
              display: 'flex',
              flexDirection: 'column',
              gap: '4px',
            }}
          >
            <div>
              <span style={{ color: 'var(--color-text-dimmed, #808080)' }}>Manual Registration: </span>
              <span>{claim.registration.title}</span>
            </div>
            <div>
              <span style={{ color: 'var(--color-text-dimmed, #808080)' }}>Matched Store Entry: </span>
              <span>{claim.storeTitle}</span>
            </div>
          </div>
        </div>

        {/* Footer */}
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
            onClick={handleCancel}
            style={{
              padding: '8px 16px',
              backgroundColor: 'transparent',
              border: 'none',
              borderRadius: 'var(--radius-sm, 4px)',
              color: 'var(--color-text-dimmed, #808080)',
              cursor: 'pointer',
            }}
          >
            {t('drmFree.cancel')}
          </button>
          <button
            type="button"
            data-testid="claim-resolve-separate"
            onClick={() => handleResolve(false)}
            style={{
              padding: '8px 16px',
              backgroundColor: 'var(--settings-bg-raised, #222222)',
              border: '1px solid var(--settings-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              color: 'var(--color-text-default, #D9D9D9)',
              cursor: 'pointer',
            }}
          >
            {t('drmFree.useExistingEntry')}
          </button>
          <button
            type="button"
            data-testid="claim-resolve-transfer"
            onClick={() => handleResolve(true)}
            style={{
              padding: '8px 20px',
              backgroundColor: 'var(--color-primary, #F15025)',
              border: 'none',
              borderRadius: 'var(--radius-sm, 4px)',
              color: 'var(--color-on-primary, #EDEDED)',
              fontWeight: 600,
              cursor: 'pointer',
            }}
          >
            {t('drmFree.transferTracking')}
          </button>
        </div>
      </div>
    </div>
  );
};
