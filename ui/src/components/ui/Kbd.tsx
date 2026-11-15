import React from 'react';
import { cx } from './cx';

export interface KbdProps extends React.HTMLAttributes<HTMLElement> {
  children: React.ReactNode;
}

/** A single key cap. Key names are product names ("Ctrl", "F5") and stay untranslated. */
export const Kbd = React.forwardRef<HTMLElement, KbdProps>(function Kbd(
  { className, children, ...props },
  ref,
) {
  return (
    <kbd
      ref={ref}
      className={cx(
        'inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-sm border border-border-subtle bg-raised px-[5px] text-[11px] font-medium leading-none text-text-secondary',
        className,
      )}
      {...props}
    >
      {children}
    </kbd>
  );
});

export interface KbdComboProps {
  /** Keys in display order, e.g. `['Ctrl', 'K']`. */
  keys: string[];
  className?: string;
  /** Overrides the accessible name; defaults to the joined key names. */
  'aria-label'?: string;
}

/** A row of key caps with the 4px inter-key gap from `components.md`. */
export function KbdCombo({
  keys,
  className,
  'aria-label': ariaLabel,
}: KbdComboProps) {
  if (keys.length === 0) return null;
  return (
    <span
      role="group"
      aria-label={ariaLabel ?? keys.join('+')}
      className={cx('inline-flex items-center gap-[4px]', className)}
    >
      {keys.map((key, index) => (
        <Kbd key={`${key}-${index}`}>{key}</Kbd>
      ))}
    </span>
  );
}
