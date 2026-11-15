import React from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { cx } from './cx';

export interface EmptyStateProps {
  icon: TablerIcon;
  title: React.ReactNode;
  body?: React.ReactNode;
  /** Buttons or links, laid out in a row with an 8px gap. */
  actions?: React.ReactNode;
  /** `lg` is the first-run 18px title; `md` is the 16px no-results title. */
  size?: 'md' | 'lg';
  className?: string;
}

/** Centered icon-circle empty state shared by the library, catalog and scanning states. */
export function EmptyState({
  icon: Icon,
  title,
  body,
  actions,
  size = 'md',
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cx(
        'flex flex-col items-center justify-center gap-3.5 text-center',
        className,
      )}
    >
      <span className="flex h-16 w-16 items-center justify-center rounded-full border border-border-subtle bg-raised">
        <Icon
          size={28}
          stroke={2}
          aria-hidden="true"
          className="text-brand"
        />
      </span>
      <h2
        className={cx(
          'm-0 font-semibold text-text-primary',
          size === 'lg' ? 'text-lg' : 'text-md',
        )}
      >
        {title}
      </h2>
      {body !== undefined ? (
        <p className="m-0 max-w-[420px] text-[13px] font-normal text-text-secondary">
          {body}
        </p>
      ) : null}
      {actions ? <div className="flex flex-wrap items-center justify-center gap-2">{actions}</div> : null}
    </div>
  );
}
