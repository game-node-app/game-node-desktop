import React, { useEffect, useRef } from 'react';
import { useI18n } from '../../i18n';

export interface DiscardChangesDialogProps {
  onCancel: () => void;
  onConfirm: () => void;
}

export const DiscardChangesDialog: React.FC<DiscardChangesDialogProps> = ({
  onCancel,
  onConfirm,
}) => {
  const { t, locale } = useI18n();
  const dialogRef = useRef<HTMLDivElement>(null);
  const cancelBtnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    cancelBtnRef.current?.focus();
  }, []);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onCancel();
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
  }, [onCancel]);

  const title =
    locale === 'pt-BR'
      ? 'Descartar alterações não salvas?'
      : 'Discard unsaved changes?';

  const message =
    t('You have unsaved changes. Do you want to discard them?') ||
    (locale === 'pt-BR'
      ? 'Você tem alterações não salvas. Deseja descartá-las?'
      : 'You have unsaved changes. Do you want to discard them?');

  return (
    <div
      className="settings-dialog-overlay"
      onClick={(e) => {
        if (e.target === e.currentTarget) {
          onCancel();
        }
      }}
    >
      <div
        ref={dialogRef}
        className="settings-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="discard-dialog-title"
      >
        <h2 id="discard-dialog-title" className="settings-header-title">
          {title}
        </h2>
        <p className="settings-section-description">{message}</p>
        <div
          style={{
            display: 'flex',
            justifyContent: 'flex-end',
            gap: '10px',
            marginTop: '8px',
          }}
        >
          <button
            ref={cancelBtnRef}
            type="button"
            className="settings-button settings-button-secondary"
            onClick={onCancel}
            aria-label={locale === 'pt-BR' ? 'Continuar editando' : 'Keep editing'}
          >
            {t('Cancel') || 'Cancel'}
          </button>
          <button
            type="button"
            className="settings-button settings-button-danger"
            onClick={onConfirm}
          >
            {t('Discard') || 'Discard'}
          </button>
        </div>
      </div>
    </div>
  );
};
