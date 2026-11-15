import React from 'react';
import { cx } from './cx';

export interface SectionLabelProps {
  children: React.ReactNode;
  /** Sidebar sections pad their label by 10px; panel sections do not. */
  inset?: boolean;
  /** Trailing action, for example the sidebar "+" or a panel link. */
  action?: React.ReactNode;
  className?: string;
  id?: string;
}

/** Uppercase 11/600 section label with 0.8px letter spacing. */
export function SectionLabel({
  children,
  inset = false,
  action,
  className,
  id,
}: SectionLabelProps) {
  return (
    <div
      id={id}
      className={cx(
        'flex items-center gap-[8px]',
        inset ? 'px-[10px]' : 'px-0',
        className,
      )}
    >
      <span className="min-w-0 flex-1 truncate text-[11px] font-semibold uppercase tracking-label text-text-muted">
        {children}
      </span>
      {action ? <span className="shrink-0">{action}</span> : null}
    </div>
  );
}
