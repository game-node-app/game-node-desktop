import { useEffect } from 'react';
import { commands } from '../../api/commands';
import { useAuth, useBootstrapState } from '../../api/useSnapshot';
import { openSettingsModal } from '../../store/uiStore';
import { useI18n } from '../../i18n';

export interface SyncExplanationModalProps {
  forceOpen?: boolean;
  onDismiss?: () => void;
  className?: string;
}

export function SyncExplanationModal({
  forceOpen,
  onDismiss,
  className,
}: SyncExplanationModalProps = {}) {
  const { auth, sync } = useAuth();
  const bootstrap = useBootstrapState();
  const { t } = useI18n();

  const isSignedIn =
    typeof auth === 'object' && auth !== null && 'signedIn' in auth;
  const isOpen =
    forceOpen !== undefined ? forceOpen : isSignedIn && !sync?.explained;

  const handleDismiss = () => {
    commands.dismissSyncExplanation();
    onDismiss?.();
  };

  const handleOpenSettings = () => {
    commands.dismissSyncExplanation();
    openSettingsModal('account');
    onDismiss?.();
  };

  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        handleDismiss();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen]);

  if (!isOpen) {
    return null;
  }

  const backgroundSupported = bootstrap?.data?.backgroundSupported ?? false;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="sync-explanation-title"
      data-testid="sync-explanation-modal"
      className={className}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 1000,
        backgroundColor: 'rgba(0, 0, 0, 0.7)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '16px',
        boxSizing: 'border-box',
      }}
    >
      <div
        style={{
          width: '100%',
          maxWidth: '520px',
          backgroundColor: 'var(--color-secondary, #1A1A1A)',
          border: '1px solid var(--color-border-subtle, #2A2A2A)',
          borderRadius: 'var(--radius-md, 8px)',
          padding: '24px',
          boxShadow: 'var(--shadow-modal, 0 32px 80px rgba(0, 0, 0, 0.55))',
          display: 'flex',
          flexDirection: 'column',
          gap: '16px',
        }}
      >
        <h2
          id="sync-explanation-title"
          style={{
            fontSize: '18px',
            fontWeight: 600,
            margin: 0,
            color: 'var(--color-text-default, #D9D9D9)',
          }}
        >
          {t('Sync with GameNode')}
        </h2>

        <p
          style={{
            fontSize: '14px',
            lineHeight: 1.5,
            color: 'var(--color-text-dimmed, #808080)',
            margin: 0,
          }}
        >
          {t(
            'Sync is on. Store playtime totals are sent to your GameNode account. You can turn sync off in Settings and keep using your library.',
          )}
        </p>

        {backgroundSupported && (
          <p
            style={{
              fontSize: '13px',
              lineHeight: 1.4,
              color: 'var(--color-text-dimmed, #808080)',
              margin: 0,
            }}
          >
            {t(
              'Closing the window keeps GameNode running in the tray. Use Quit to exit.',
            )}
          </p>
        )}

        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: '12px',
            marginTop: '8px',
          }}
        >
          <button
            type="button"
            data-testid="sync-primary-button"
            onClick={handleDismiss}
            style={{
              height: '36px',
              padding: '0 16px',
              backgroundColor: 'var(--color-primary, #F15025)',
              color: 'var(--color-on-primary, #EDEDED)',
              border: 'none',
              borderRadius: 'var(--radius-sm, 4px)',
              fontSize: '14px',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            {t('Got it')}
          </button>

          <button
            type="button"
            data-testid="sync-secondary-button"
            onClick={handleOpenSettings}
            style={{
              height: '36px',
              padding: '0 16px',
              backgroundColor: 'transparent',
              color: 'var(--color-text-default, #D9D9D9)',
              border: '1px solid var(--color-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              fontSize: '14px',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            {t('Open settings')}
          </button>
        </div>
      </div>
    </div>
  );
}
