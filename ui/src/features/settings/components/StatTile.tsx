import React from 'react';
import { cx } from '../../../components/ui/cx';

export interface StatTileProps
  extends Omit<React.HTMLAttributes<HTMLDivElement>, 'children'> {
  /** 16/600 value, e.g. a game count. */
  value: React.ReactNode;
  /** 11/400 caption under the value. */
  caption: React.ReactNode;
}

/**
 * Small canvas stat inside store cards (`components.md` §6). Three tiles share
 * one row with an 8px gap; each tile fills the row equally.
 */
export function StatTile({ value, caption, className, ...props }: StatTileProps) {
  return (
    <div
      {...props}
      className={cx(
        'flex min-w-0 flex-1 flex-col gap-[2px] rounded-control border border-border-subtle bg-canvas p-xs',
        className,
      )}
    >
      <span className="truncate text-md font-semibold text-text-primary">
        {value}
      </span>
      <span className="truncate text-caption text-text-dimmed">{caption}</span>
    </div>
  );
}
