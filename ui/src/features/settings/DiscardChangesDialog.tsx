import React, { useRef } from 'react';
import { IconAlertTriangle } from '@tabler/icons-react';
import { ConfirmDialog } from '../../components/dialog';
import { Button, Checklist } from '../../components/ui';
import { useI18n } from '../../i18n';

export interface DiscardChangesDialogProps {
  /** Localized labels of the fields with pending edits, in the order they appear. */
  pendingChanges: string[];
  onCancel: () => void;
  onConfirm: () => void;
}

/**
 * D10: the discard confirmation lists what will be lost and starts focus on
 * the non-destructive action, so a stray Enter keeps editing.
 */
export const DiscardChangesDialog: React.FC<DiscardChangesDialogProps> = ({
  pendingChanges,
  onCancel,
  onConfirm,
}) => {
  const { t } = useI18n();
  const keepEditingRef = useRef<HTMLButtonElement>(null);

  return (
    <ConfirmDialog
      tone="warning"
      icon={IconAlertTriangle}
      size={400}
      title={t('settings.discardChanges')}
      description={t('settings.youHaveUnsavedChangesDoYouWant')}
      initialFocusRef={keepEditingRef}
      onClose={onCancel}
      testId="discard-changes-dialog"
      actions={
        <>
          <Button
            ref={keepEditingRef}
            variant="secondary"
            size="sm"
            data-testid="discard-keep-editing"
            onClick={onCancel}
          >
            {t('settings.keepEditing')}
          </Button>
          <Button
            variant="danger"
            size="sm"
            data-testid="discard-confirm"
            onClick={onConfirm}
          >
            {t('settings.discard')}
          </Button>
        </>
      }
    >
      <Checklist
        variant="bullet"
        items={pendingChanges.map((label) => ({
          kind: 'positive' as const,
          label,
        }))}
      />
    </ConfirmDialog>
  );
};
