import React, { forwardRef } from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { cx } from './cx';
import { useFieldControl } from './Field';

export interface TextInputProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'size'> {
  /** 14px leading icon (search, folder, file-text, …). */
  leadingIcon?: TablerIcon;
  /** Classes for the 34px field box, not the `<input>` itself. */
  className?: string;
  inputClassName?: string;
}

/**
 * 34px single-line input box from `components.md` §3. It picks up the
 * surrounding `Field` label and helper wiring from context. Read-only inputs
 * keep the box but drop the brand focus border, per the same table.
 */
export const TextInput = forwardRef<HTMLInputElement, TextInputProps>(
  function TextInput(
    {
      leadingIcon: LeadingIcon,
      className,
      inputClassName,
      id,
      readOnly,
      'aria-describedby': ariaDescribedBy,
      ...props
    },
    ref,
  ) {
    const field = useFieldControl();
    const controlId = id ?? field?.controlId;
    const describedBy =
      cx(ariaDescribedBy, field?.describedBy) || undefined;

    return (
      <div
        className={cx(
          'flex h-[34px] items-center gap-compact rounded-control border border-border-subtle bg-surface px-xs',
          readOnly ? 'cursor-default' : 'focus-within:border-brand',
          className,
        )}
      >
        {LeadingIcon ? (
          <LeadingIcon
            size={14}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-text-dimmed"
          />
        ) : null}
        <input
          ref={ref}
          id={controlId}
          readOnly={readOnly}
          aria-readonly={readOnly || undefined}
          aria-describedby={describedBy}
          className={cx(
            'min-w-0 flex-1 truncate bg-transparent text-body text-text-primary outline-none placeholder:text-text-dimmed',
            inputClassName,
          )}
          {...props}
        />
      </div>
    );
  },
);
