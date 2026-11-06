import React, { useEffect, useRef } from 'react';
import { useBootstrapState } from '../../api';
import { useI18n } from '../../i18n';

export interface LicenseNoticesModalProps {
  onClose: () => void;
}

export const LicenseNoticesModal: React.FC<LicenseNoticesModalProps> = ({ onClose }) => {
  const { t } = useI18n();
  const { data: bootstrap } = useBootstrapState();
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeBtnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    closeBtnRef.current?.focus();
  }, []);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onClose();
        return;
      }

      if (event.key === 'Tab' && dialogRef.current) {
        const focusables = dialogRef.current.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
        );
        if (focusables.length === 0) return;
        const first = focusables[0];
        const last = focusables[focusables.length - 1];

        if (event.shiftKey) {
          if (document.activeElement === first || !dialogRef.current.contains(document.activeElement)) {
            event.preventDefault();
            last.focus();
          }
        } else {
          if (document.activeElement === last || !dialogRef.current.contains(document.activeElement)) {
            event.preventDefault();
            first.focus();
          }
        }
      }
    }

    window.addEventListener('keydown', handleKeyDown, true);
    return () => {
      window.removeEventListener('keydown', handleKeyDown, true);
    };
  }, [onClose]);

  const notices = bootstrap?.licenseNotices ?? 'GameNode Desktop - GPL-3.0-only';

  return (
    <div
      className="settings-dialog-overlay"
      onClick={(e) => {
        if (e.target === e.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        ref={dialogRef}
        className="settings-license-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={t('License notices')}
      >
        <div className="settings-header" style={{ padding: '16px 20px' }}>
          <div className="settings-header-titles">
            <h2 className="settings-header-title">{t('License notices')}</h2>
          </div>
          <button
            ref={closeBtnRef}
            type="button"
            className="settings-close-button"
            onClick={onClose}
            aria-label={t('Close') || 'Close'}
          >
            ×
          </button>
        </div>

        <div className="settings-license-content" tabIndex={0}>
          {notices}
        </div>

        <div
          style={{
            display: 'flex',
            justifyContent: 'flex-end',
            padding: '12px 20px',
            borderTop: '1px solid var(--settings-border-subtle, #2a2a2a)',
            backgroundColor: 'var(--settings-bg-base, #0b0b0b)',
          }}
        >
          <button
            type="button"
            className="settings-button settings-button-secondary"
            onClick={onClose}
          >
            {t('Close') || 'Close'}
          </button>
        </div>
      </div>
    </div>
  );
};
