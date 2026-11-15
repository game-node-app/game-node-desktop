import React from 'react';
import { IconChevronRight } from '@tabler/icons-react';
import { cx } from './cx';

export interface KeyValueRowProps {
  /** Key/label of the row. */
  label: React.ReactNode;
  /** Value; when absent, `emptyLabel` renders dimmed. */
  value?: React.ReactNode;
  /** Localized empty value, for example "None". */
  emptyLabel?: React.ReactNode;
  /** Makes the whole row a button with hover feedback and a chevron. */
  onClick?: () => void;
  /** Shows the trailing chevron even when the row is not interactive. */
  chevron?: boolean;
  /** Omits the bottom border on the last row of a section. */
  last?: boolean;
  className?: string;
}

export function KeyValueRow({
  label,
  value,
  emptyLabel,
  onClick,
  chevron = false,
  last = false,
  className,
}: KeyValueRowProps) {
  const hasValue = value !== undefined && value !== null && value !== '';
  const content = (
    <>
      <span className="min-w-0 text-[13px] font-normal text-text-secondary">{label}</span>
      <span className="flex-1" />
      {hasValue ? (
        <span className="min-w-0 truncate text-[13px] font-medium text-text-default">
          {value}
        </span>
      ) : emptyLabel !== undefined ? (
        <span className="min-w-0 truncate text-[13px] font-normal text-text-dimmed">
          {emptyLabel}
        </span>
      ) : null}
      {onClick || chevron ? (
        <IconChevronRight size={14} stroke={2} aria-hidden="true" className="shrink-0 text-text-dimmed" />
      ) : null}
    </>
  );

  const classes = cx(
    'flex w-full items-center gap-[8px] py-[6px] text-left',
    !last && 'border-b border-border-subtle',
    onClick &&
      'rounded-control transition-colors hover:bg-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-2 focus-visible:ring-offset-body',
    className,
  );

  if (onClick) {
    return (
      <button type="button" onClick={onClick} className={classes}>
        {content}
      </button>
    );
  }

  return <div className={classes}>{content}</div>;
}
