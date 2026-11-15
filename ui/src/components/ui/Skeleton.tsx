import React from 'react';
import { cx } from './cx';

export interface SkeletonProps extends React.HTMLAttributes<HTMLDivElement> {
  /** Rounded shape; rectangles use the 6px control radius. */
  shape?: 'rect' | 'circle';
}

/** Pulsing placeholder block used by the scanning panel and catalog loading state. */
export function Skeleton({ shape = 'rect', className, ...props }: SkeletonProps) {
  return (
    <div
      aria-hidden="true"
      className={cx(
        'animate-pulse bg-raised',
        shape === 'circle' ? 'rounded-full' : 'rounded-control',
        className,
      )}
      {...props}
    />
  );
}
