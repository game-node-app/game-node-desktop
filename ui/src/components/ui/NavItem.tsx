import React from 'react';
import { IconAlertTriangle, type TablerIcon } from '@tabler/icons-react';
import { Tooltip } from './Tooltip';
import { cx } from './cx';

export type NavItemCountTone = 'default' | 'brand';

export interface NavItemProps {
  icon: TablerIcon;
  label: string;
  count?: number;
  countTone?: NavItemCountTone;
  /** Reason announced by the warning icon, for example "{} needs attention". */
  warning?: string;
  /** Slot rendered before the count in the expanded row. */
  trailing?: React.ReactNode;
  active?: boolean;
  /** Collapsed 72px rail: icon only, tooltip label, count as a badge. */
  collapsed?: boolean;
  onClick?: () => void;
  className?: string;
  'aria-controls'?: string;
}

export function NavItem({
  icon: Icon,
  label,
  count,
  countTone = 'default',
  warning,
  trailing,
  active = false,
  collapsed = false,
  onClick,
  className,
  'aria-controls': ariaControls,
}: NavItemProps) {
  const button = (
    <button
      type="button"
      aria-label={warning ? `${label}: ${warning}` : label}
      aria-current={active ? 'page' : undefined}
      aria-controls={ariaControls}
      onClick={onClick}
      title={collapsed ? undefined : label}
      className={cx(
        'relative flex h-[32px] w-full items-center rounded-control text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-2 focus-visible:ring-offset-chrome',
        collapsed ? 'justify-center px-0' : 'gap-[10px] px-[10px]',
        active
          ? 'bg-accent-subtle font-semibold text-text-primary'
          : 'font-normal text-text-default hover:bg-raised',
        className,
      )}
    >
      <Icon
        size={16}
        stroke={2}
        aria-hidden="true"
        className={cx('shrink-0', active ? 'text-brand' : 'text-text-secondary')}
      />
      {!collapsed ? (
        <>
          <span className="min-w-0 flex-1 truncate text-left">{label}</span>
          {trailing ? <span className="shrink-0">{trailing}</span> : null}
          {warning ? (
            <span className="shrink-0 text-warning" title={warning}>
              <IconAlertTriangle size={14} stroke={2} aria-hidden="true" />
              <span className="sr-only">{warning}</span>
            </span>
          ) : null}
          {count !== undefined ? (
            <span
              className={cx(
                'shrink-0 text-right text-xs',
                countTone === 'brand'
                  ? 'text-brand'
                  : active
                    ? 'text-text-default'
                    : 'text-text-dimmed',
              )}
            >
              {count}
            </span>
          ) : null}
        </>
      ) : count !== undefined ? (
        <span
          aria-hidden="true"
          className={cx(
            'absolute right-[4px] top-[2px] min-w-[16px] rounded-full px-[4px] text-center text-[10px] font-medium leading-[16px]',
            countTone === 'brand'
              ? 'bg-accent-subtle text-brand'
              : 'bg-raised text-text-default',
          )}
        >
          {count}
        </span>
      ) : null}
    </button>
  );

  if (!collapsed) return button;

  return <Tooltip label={label}>{button}</Tooltip>;
}
