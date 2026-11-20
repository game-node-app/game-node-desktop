import React from 'react';
import {
  IconArrowBackUp,
  IconCheck,
  IconInfoCircle,
} from '@tabler/icons-react';
import { Button } from '../../../components/ui/Button';
import { cx } from '../../../components/ui/cx';
import { useI18n } from '../../../i18n';

export interface UnsavedChangesBarProps
  extends Omit<React.HTMLAttributes<HTMLDivElement>, 'children'> {
  /** Number of changed settings; drives the pluralized count line. */
  count: number;
  /** Shows the save spinner and blocks Revert while a save is in flight. */
  saving?: boolean;
  onRevert: () => void;
  onSave: () => void;
}

/**
 * Bottom bar pinned to the Settings content area (`components.md` §9). It is a
 * polite live region so screen readers announce pending changes as they grow.
 */
export function UnsavedChangesBar({
  count,
  saving = false,
  onRevert,
  onSave,
  className,
  ...props
}: UnsavedChangesBarProps) {
  const { t } = useI18n();
  const countLabel =
    count === 1
      ? t('settings.unsaved.countOne', { n: count })
      : t('settings.unsaved.countOther', { n: count });

  return (
    <div
      {...props}
      role="region"
      aria-live="polite"
      aria-label={t('settings.unsaved.title')}
      className={cx(
        'absolute inset-x-0 bottom-0 flex h-[56px] items-center gap-xs border-t border-border-floating bg-raised px-lg',
        className,
      )}
    >
      <IconInfoCircle
        size={16}
        stroke={2}
        aria-hidden="true"
        className="shrink-0 text-brand"
      />
      <div className="flex min-w-0 flex-1 flex-col gap-px">
        <span className="truncate text-body font-semibold text-text-primary">
          {t('settings.unsaved.title')}
        </span>
        <span
          data-testid="settings-unsaved-count"
          className="truncate text-caption text-text-dimmed"
        >
          {countLabel}
        </span>
      </div>
      <div className="flex shrink-0 items-center gap-compact">
        <Button
          size="sm"
          variant="ghost"
          leftIcon={IconArrowBackUp}
          onClick={onRevert}
          disabled={saving}
        >
          {t('settings.unsaved.revert')}
        </Button>
        <Button
          size="sm"
          variant="primary"
          leftIcon={IconCheck}
          loading={saving}
          onClick={onSave}
        >
          {t('settings.unsaved.save')}
        </Button>
      </div>
    </div>
  );
}
