import React from 'react';
import { IconX } from '@tabler/icons-react';
import { cx } from './cx';

export interface FilterChipProps {
  /** Facet name, for example "Store:". */
  label: React.ReactNode;
  /** Selected value(s), joined by the caller with ", ". */
  value?: React.ReactNode;
  /** Accessible name for the remove button, for example "Remove filter Store". */
  removeLabel: string;
  onRemove: () => void;
  className?: string;
}

/** Active-filter chip: label + value + remove button. */
export function FilterChip({
  label,
  value,
  removeLabel,
  onRemove,
  className,
}: FilterChipProps) {
  return (
    <span
      className={cx(
        'inline-flex h-[28px] items-center gap-[6px] rounded-full border border-brand bg-accent-subtle py-0 pl-[10px] pr-[6px]',
        className,
      )}
    >
      <span className="text-xs font-normal text-text-secondary">{label}</span>
      {value !== undefined && value !== null && value !== '' ? (
        <span className="text-xs font-semibold text-text-primary">{value}</span>
      ) : null}
      <button
        type="button"
        aria-label={removeLabel}
        onClick={onRemove}
        className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center rounded-full text-text-secondary transition-colors hover:bg-raised hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
      >
        <IconX size={14} stroke={2} aria-hidden="true" />
      </button>
    </span>
  );
}
