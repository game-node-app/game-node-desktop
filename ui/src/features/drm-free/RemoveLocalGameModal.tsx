import React, { useState, useEffect } from 'react';
import { commands, useLocalGames, type Registration } from '../../api';
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

  const rawId = effectiveEntryKey ? effectiveEntryKey.replace(/^manual:/, '') : '';
  const registration =
    propRegistration ??
    localGames.registrations.find(
      (r) => r.id === rawId || r.id === effectiveEntryKey || `manual:${r.id}` === effectiveEntryKey,
    );

  const title = registration?.title ?? 'this game';

  const handleClose = () => {
    uiStore.setState((s) => ({ ...s, removeModalEntryKey: null }));
    onClose?.();
  };

  const handleConfirm = () => {
    if (effectiveEntryKey) {
      commands.removeEntry(effectiveEntryKey, allowRediscovery);
    }
    if (rawId) {
      commands.removeLocalGame(rawId, allowRediscovery);
    }
    handleClose();
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

  if (!isOpen || !effectiveEntryKey) {
    return null;
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="remove-game-heading"
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
          maxWidth: '560px',
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
            <h2 id="remove-game-heading" style={{ fontSize: 'var(--font-size-lg, 18px)', fontWeight: 600 }}>
              {t('drmFree.removeFromLibrary')}
            </h2>
            <div
              data-testid="remove-game-title-header"
              style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}
            >
              {title}
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
              Are you sure you want to remove <strong data-testid="remove-game-target-title">{title}</strong> from your library?
            </p>
            <p style={{ color: 'var(--color-text-dimmed, #808080)' }}>
              Game files and local save backups will not be deleted from your computer.
            </p>
          </div>

          <label
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: '10px',
              padding: '12px 14px',
              backgroundColor: 'var(--settings-bg-raised, #222222)',
              border: '1px solid var(--settings-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              cursor: 'pointer',
              fontSize: 'var(--font-size-sm, 14px)',
            }}
          >
            <input
              type="checkbox"
              id="allow-rediscovery-checkbox"
              data-testid="allow-rediscovery-checkbox"
              checked={allowRediscovery}
              onChange={(e) => setAllowRediscovery(e.target.checked)}
              style={{ cursor: 'pointer' }}
            />
            <span>Allow rediscovery in watched folders</span>
          </label>
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
            onClick={handleClose}
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
            data-testid="confirm-remove-button"
            aria-label={t('drmFree.removeFromLibrary')}
            onClick={handleConfirm}
            style={{
              padding: '8px 20px',
              backgroundColor: 'var(--color-danger, #E5484D)',
              border: 'none',
              borderRadius: 'var(--radius-sm, 4px)',
              color: '#FFFFFF',
              fontWeight: 600,
              cursor: 'pointer',
            }}
          >
            {t('drmFree.removeFromLibrary')}
          </button>
        </div>
      </div>
    </div>
  );
};
