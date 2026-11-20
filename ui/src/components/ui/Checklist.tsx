import React from 'react';
import {
  IconAlertTriangle,
  IconCircleCheck,
  IconCircleX,
  type TablerIcon,
} from '@tabler/icons-react';
import { cx } from './cx';

export type ChecklistItemKind = 'positive' | 'negative' | 'warning';
export type ChecklistVariant = 'plain' | 'inset' | 'bullet';

export interface ChecklistItem {
  kind: ChecklistItemKind;
  label: React.ReactNode;
}

export interface ChecklistProps {
  items: ChecklistItem[];
  /** `inset` adds the bordered body list (D12); `bullet` is the D10 variant. */
  variant?: ChecklistVariant;
  className?: string;
}

const KIND_STYLES: Record<
  ChecklistItemKind,
  { icon: TablerIcon; iconClassName: string; labelClassName: string }
> = {
  positive: {
    icon: IconCircleCheck,
    iconClassName: 'text-success',
    labelClassName: 'text-text-default',
  },
  negative: {
    icon: IconCircleX,
    iconClassName: 'text-text-muted',
    labelClassName: 'text-text-dimmed',
  },
  warning: {
    icon: IconAlertTriangle,
    iconClassName: 'text-warning',
    labelClassName: 'text-text-default',
  },
};

const VARIANT_CLASSES: Record<ChecklistVariant, string> = {
  plain: 'flex flex-col gap-compact',
  inset:
    'flex flex-col gap-compact rounded-md border border-border-subtle bg-body p-sm',
  bullet:
    'flex flex-col gap-[4px] rounded-md border border-border-subtle bg-body p-[10px]',
};

/**
 * Icon list from `components.md` §8: positive reasons, negative/stays-local
 * rows and warning reasons. The bullet variant used by the discard dialog
 * replaces the tone icon with the 6px brand dot.
 */
export function Checklist({
  items,
  variant = 'plain',
  className,
}: ChecklistProps) {
  const bullet = variant === 'bullet';

  return (
    <ul className={cx(VARIANT_CLASSES[variant], className)}>
      {items.map((item, index) => {
        const styles = KIND_STYLES[item.kind];
        const Icon = styles.icon;
        return (
          <li
            key={`${item.kind}-${index}`}
            className={cx(
              'flex items-center gap-compact text-xs font-normal',
              bullet ? 'text-text-default' : styles.labelClassName,
            )}
          >
            {bullet ? (
              <span
                aria-hidden="true"
                className="h-[6px] w-[6px] shrink-0 rounded-full bg-brand"
              />
            ) : (
              <Icon
                size={14}
                stroke={2}
                aria-hidden="true"
                className={cx('shrink-0', styles.iconClassName)}
              />
            )}
            <span className="min-w-0">{item.label}</span>
          </li>
        );
      })}
    </ul>
  );
}
