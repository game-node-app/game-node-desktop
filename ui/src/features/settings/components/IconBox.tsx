import React from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { cx } from '../../../components/ui/cx';

export type IconBoxTone = 'neutral' | 'accent';

export interface IconBoxProps extends React.HTMLAttributes<HTMLDivElement> {
  icon: TablerIcon;
  /** Neutral is the raised bordered box; accent is the brand-toned tool box. */
  tone?: IconBoxTone;
  iconSize?: number;
}

/** 40×40 icon box for store, plugin and tool cards (`components.md` §6). */
export function IconBox({
  icon: Icon,
  tone = 'neutral',
  iconSize = 20,
  className,
  ...props
}: IconBoxProps) {
  return (
    <div
      {...props}
      className={cx(
        'flex h-[40px] w-[40px] shrink-0 items-center justify-center rounded-md',
        tone === 'accent'
          ? 'bg-accent-subtle'
          : 'border border-border-subtle bg-raised',
        className,
      )}
    >
      <Icon
        size={iconSize}
        stroke={2}
        aria-hidden="true"
        className={tone === 'accent' ? 'text-brand' : 'text-text-primary'}
      />
    </div>
  );
}
