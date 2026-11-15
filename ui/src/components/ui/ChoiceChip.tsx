import React from 'react';
import { cx } from './cx';

export interface ChoiceChipProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'aria-pressed'> {
  /** Selected state exposed through `aria-pressed`. */
  selected?: boolean;
  children: React.ReactNode;
}

/**
 * Toggle chip for single-choice facets (Last session, Release, Added). The
 * caller clears the facet when the selected chip is activated, per
 * `components.md`.
 */
export function ChoiceChip({
  selected = false,
  children,
  className,
  type = 'button',
  ...props
}: ChoiceChipProps) {
  return (
    <button
      type={type}
      aria-pressed={selected}
      className={cx(
        'inline-flex h-[26px] items-center gap-[6px] rounded-full border px-[10px] text-xs transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-2 focus-visible:ring-offset-body disabled:cursor-not-allowed disabled:opacity-50',
        selected
          ? 'border-brand bg-accent-subtle font-semibold text-text-primary'
          : 'border-border-subtle bg-surface font-normal text-text-default hover:bg-raised',
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
}
