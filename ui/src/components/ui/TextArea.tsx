import React, { forwardRef } from 'react';
import { cx } from './cx';
import { useFieldControl } from './Field';

export interface TextAreaProps
  extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {
  /** Classes for the textarea surface itself. */
  className?: string;
}

/**
 * 64px, non-resizable multiline field from `components.md` §3: 13px text with
 * the 19px `leading-field` line height and a brand focus border. Label and
 * helper wiring come from the surrounding `Field`.
 */
export const TextArea = forwardRef<HTMLTextAreaElement, TextAreaProps>(
  function TextArea(
    { className, id, 'aria-describedby': ariaDescribedBy, ...props },
    ref,
  ) {
    const field = useFieldControl();
    const controlId = id ?? field?.controlId;
    const describedBy =
      cx(ariaDescribedBy, field?.describedBy) || undefined;

    return (
      <textarea
        ref={ref}
        id={controlId}
        aria-describedby={describedBy}
        className={cx(
          'h-[64px] resize-none rounded-control border border-border-subtle bg-surface px-xs pb-0 pt-compact text-body leading-field text-text-primary outline-none placeholder:text-text-dimmed focus:border-brand',
          className,
        )}
        {...props}
      />
    );
  },
);
