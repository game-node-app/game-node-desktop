import React from 'react';
import { IconFolder } from '@tabler/icons-react';
import { Button } from '../../../components/ui/Button';
import { cx } from '../../../components/ui/cx';
import { useI18n } from '../../../i18n';

export interface PathFieldProps {
  /** Current path; the full value is also exposed as the input tooltip. */
  value: string;
  onChange?: (value: string) => void;
  /** Opens the native folder picker when provided. */
  onBrowse?: () => void;
  /** Defaults to the shared "Choose folder…" copy. */
  browseLabel?: string;
  /** Brand border while the draft differs from the saved path. */
  changed?: boolean;
  /** Validation message rendered below the field. */
  error?: React.ReactNode;
  id?: string;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  'aria-label'?: string;
  'aria-labelledby'?: string;
  'aria-describedby'?: string;
}

/**
 * Editable destination path from `components.md` §7. Paths can be typed or
 * pasted into the 230×32 box; the adjacent button opens the native picker.
 */
export function PathField({
  value,
  onChange,
  onBrowse,
  browseLabel,
  changed = false,
  error,
  id,
  placeholder,
  disabled = false,
  className,
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
  'aria-describedby': ariaDescribedBy,
}: PathFieldProps) {
  const { t } = useI18n();
  const generatedId = React.useId();
  const inputId = id ?? `${generatedId}-path`;
  const errorId = `${generatedId}-error`;
  const describedBy =
    [ariaDescribedBy, error ? errorId : null].filter(Boolean).join(' ') ||
    undefined;

  return (
    <div className={cx('flex flex-col gap-[4px]', className)}>
      <div className="flex flex-wrap items-center gap-xs">
        <div
          className={cx(
            'flex h-[32px] w-[230px] max-w-full min-w-0 items-center gap-[6px] rounded-control border bg-canvas px-xs focus-within:border-brand',
            changed ? 'border-brand' : 'border-border-subtle',
            disabled && 'opacity-50',
          )}
        >
          <IconFolder
            size={14}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-text-dimmed"
          />
          <input
            id={inputId}
            type="text"
            value={value}
            placeholder={placeholder}
            title={value}
            disabled={disabled}
            aria-label={ariaLabel}
            aria-labelledby={ariaLabelledBy}
            aria-describedby={describedBy}
            aria-invalid={error ? true : undefined}
            onChange={(event) => onChange?.(event.currentTarget.value)}
            className="min-w-0 flex-1 truncate bg-transparent text-xs text-text-primary outline-none placeholder:text-text-dimmed"
          />
        </div>
        {onBrowse ? (
          <Button
            size="sm"
            variant="secondary"
            onClick={onBrowse}
            disabled={disabled}
          >
            {browseLabel ?? t('settings.common.chooseFolder')}
          </Button>
        ) : null}
      </div>
      {error ? (
        <p id={errorId} role="alert" className="text-xs text-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}
