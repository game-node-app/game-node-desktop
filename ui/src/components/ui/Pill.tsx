import React from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { cx } from './cx';

export type PillTone = 'neutral' | 'brand' | 'success' | 'warning' | 'danger';

export interface PillProps {
  tone?: PillTone;
  icon?: TablerIcon;
  children: React.ReactNode;
  className?: string;
  /** Renders the pill as a button (for example the collection "+" pill). */
  onClick?: () => void;
  title?: string;
  'aria-label'?: string;
}

export const PILL_TONE_CLASSES: Record<PillTone, string> = {
  neutral: 'bg-raised text-text-default',
  brand: 'bg-accent-subtle text-brand',
  success: 'bg-success-subtle text-success',
  warning: 'bg-warning-subtle text-warning',
  danger: 'bg-danger-subtle text-danger',
};

const BASE =
  'inline-flex h-[18px] items-center gap-[4px] rounded-full px-[8px] py-[2px] text-xs font-medium leading-none';

export function Pill({
  tone = 'neutral',
  icon: Icon,
  children,
  className,
  onClick,
  title,
  'aria-label': ariaLabel,
}: PillProps) {
  const content = (
    <>
      {Icon ? <Icon size={12} stroke={2} aria-hidden="true" /> : null}
      <span className="truncate">{children}</span>
    </>
  );

  if (onClick) {
    return (
      <button
        type="button"
        title={title}
        aria-label={ariaLabel}
        onClick={onClick}
        className={cx(
          BASE,
          PILL_TONE_CLASSES[tone],
          'cursor-pointer transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-2 focus-visible:ring-offset-body',
          className,
        )}
      >
        {content}
      </button>
    );
  }

  return (
    <span
      title={title}
      aria-label={ariaLabel}
      className={cx(BASE, PILL_TONE_CLASSES[tone], className)}
    >
      {content}
    </span>
  );
}
