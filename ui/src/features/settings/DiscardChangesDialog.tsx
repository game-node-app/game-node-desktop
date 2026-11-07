import React, { useEffect, useRef } from 'react';
import { AppModal } from '../../components/AppModal';
import { useI18n } from '../../i18n';
import {
  settingsButtonDanger,
  settingsButtonSecondary,
} from './settingsClasses';
import { PlainModalScrollArea } from './PlainModalScrollArea';

export interface DiscardChangesDialogProps {
  onCancel: () => void;
  onConfirm: () => void;
}

export const DiscardChangesDialog: React.FC<DiscardChangesDialogProps> = ({
  onCancel,
  onConfirm,
}) => {
  const { t, locale } = useI18n();
  const cancelBtnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    cancelBtnRef.current?.focus();
  }, []);

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
    <AppModal.Root
      opened
      onClose={onCancel}
      centered
      zIndex={1010}
      scrollAreaComponent={PlainModalScrollArea}
      withinPortal
    >
      <AppModal.Overlay backgroundOpacity={0.65} />
      <AppModal.Content
        classNames={{
          content:
            'w-[90%] max-w-[480px] flex-none rounded-md border border-border-subtle bg-secondary p-6 text-white shadow-alert',
        }}
      >
        <div
          role="alertdialog"
          aria-labelledby="discard-dialog-title"
          className="flex flex-col gap-md"
        >
          <h2 id="discard-dialog-title" className="text-lg font-bold text-white">
            {title}
          </h2>
          <p className="text-sm text-[var(--settings-text-secondary)]">{message}</p>
          <div className="mt-compact flex justify-end gap-[10px]">
            <button
              ref={cancelBtnRef}
              type="button"
              className={settingsButtonSecondary}
              onClick={onCancel}
              aria-label={locale === 'pt-BR' ? 'Continuar editando' : 'Keep editing'}
            >
              {t('Cancel') || 'Cancel'}
            </button>
            <button
              type="button"
              className={settingsButtonDanger}
              onClick={onConfirm}
            >
              {t('Discard') || 'Discard'}
            </button>
          </div>
        </div>
      </AppModal.Content>
    </AppModal.Root>
  );
};
