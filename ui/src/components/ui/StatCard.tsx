import React from 'react';
import { cx } from './cx';

export interface StatCardProps {
  label: string;
  value: React.ReactNode;
  /** Secondary line under the value. */
  hint?: React.ReactNode;
  /** 0..1 progress ratio; renders a 4px bar instead of a hint. */
  progress?: number;
  className?: string;
}

/** Detail-pane stat card: label, value, and a hint or progress bar. */
export function StatCard({ label, value, hint, progress, className }: StatCardProps) {
  const ratio =
    progress === undefined ? undefined : Math.min(1, Math.max(0, progress));

  return (
    <div
      className={cx(
        'flex min-w-0 flex-1 flex-col gap-[4px] rounded-control border border-border-subtle bg-body p-[10px]',
        className,
      )}
    >
      <span className="truncate text-[10px] font-semibold uppercase tracking-overline text-text-muted">
        {label}
      </span>
      <span className="truncate text-title font-semibold text-text-primary">{value}</span>
      {ratio !== undefined ? (
        <span
          role="progressbar"
          aria-label={label}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(ratio * 100)}
          className="mt-[2px] block h-[4px] overflow-hidden rounded-sm bg-raised"
        >
          <span
            className="block h-full rounded-sm bg-brand"
            style={{ width: `${ratio * 100}%` }}
          />
        </span>
      ) : hint !== undefined ? (
        <span className="truncate text-[11px] font-normal text-text-dimmed">{hint}</span>
      ) : null}
    </div>
  );
}
