import React, { createContext, useContext, useRef } from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { cx } from './cx';

export type OptionCardVariant = 'status' | 'row';
export type OptionCardRole = 'radio' | 'option';

interface OptionCardGroupContextValue {
  value: string | null;
  select: (value: string) => void;
  role: OptionCardRole;
}

const OptionCardGroupContext =
  createContext<OptionCardGroupContextValue | null>(null);

export interface OptionCardGroupProps {
  value?: string | null;
  onChange?: (value: string) => void;
  /** `radiogroup` for status cards, `listbox` for match rows. */
  role?: 'radiogroup' | 'listbox';
  /** Arrow keys always work both ways; this only documents the visual axis. */
  orientation?: 'horizontal' | 'vertical';
  'aria-label'?: string;
  'aria-labelledby'?: string;
  className?: string;
  children: React.ReactNode;
}

const GROUP_KEYS = new Set([
  'ArrowDown',
  'ArrowUp',
  'ArrowLeft',
  'ArrowRight',
  'Home',
  'End',
]);

/**
 * Group semantics for `OptionCard` from `components.md` §4. It owns the
 * radiogroup/listbox role and moves selection + focus with the arrow keys, Home
 * and End, so the status cards and match rows behave like native radios without
 * depending on browser-only radio navigation.
 */
export function OptionCardGroup({
  value,
  onChange,
  role = 'radiogroup',
  orientation = 'horizontal',
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
  className,
  children,
}: OptionCardGroupProps) {
  const groupRef = useRef<HTMLDivElement>(null);
  const itemRole: OptionCardRole = role === 'listbox' ? 'option' : 'radio';

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (!GROUP_KEYS.has(event.key)) return;
    const items = Array.from(
      groupRef.current?.querySelectorAll<HTMLButtonElement>(
        '[role="radio"], [role="option"]',
      ) ?? [],
    ).filter((item) => !item.disabled && item.getAttribute('aria-disabled') !== 'true');
    if (items.length === 0) return;

    const target = event.target as Node;
    const currentIndex = items.findIndex(
      (item) => item === target || item.contains(target),
    );
    if (currentIndex === -1) return;

    event.preventDefault();

    let nextIndex: number;
    if (event.key === 'Home') {
      nextIndex = 0;
    } else if (event.key === 'End') {
      nextIndex = items.length - 1;
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowRight') {
      nextIndex = (currentIndex + 1) % items.length;
    } else {
      nextIndex = (currentIndex - 1 + items.length) % items.length;
    }

    const next = items[nextIndex];
    const nextValue = next.dataset.value;
    if (nextValue !== undefined) onChange?.(nextValue);
    next.focus();
  };

  return (
    <div
      ref={groupRef}
      role={role}
      aria-label={ariaLabel}
      aria-labelledby={ariaLabelledBy}
      aria-orientation={orientation}
      className={className}
      onKeyDown={handleKeyDown}
    >
      <OptionCardGroupContext.Provider
        value={{ value: value ?? null, select: onChange ?? (() => undefined), role: itemRole }}
      >
        {children}
      </OptionCardGroupContext.Provider>
    </div>
  );
}

export interface OptionCardProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'title'> {
  /** Value reported by the surrounding `OptionCardGroup`. */
  value: string;
  variant?: OptionCardVariant;
  selected?: boolean;
  /** Defaults to `radio` for status cards and `option` for match rows. */
  role?: OptionCardRole;
  /** 18px status icon shown above the label. */
  icon?: TablerIcon;
  /** Leading slot, for example a 36×48 cover in a match row. */
  leading?: React.ReactNode;
  title: React.ReactNode;
  description?: React.ReactNode;
  /** 11px meta line used by match rows. */
  meta?: React.ReactNode;
  /** Trailing slot, for example the confidence `Pill`. */
  trailing?: React.ReactNode;
}

const BASE =
  'group/option flex w-full text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-0 disabled:cursor-not-allowed disabled:opacity-50';

/**
 * Selectable card from `components.md` §4, covering the two layouts the
 * dialogs need: the stacked status card (D02) and the 66px match row (D01).
 * Selection styling, focus ring and hover state are shared; the caller chooses
 * the layout with `variant`.
 */
export const OptionCard = React.forwardRef<HTMLButtonElement, OptionCardProps>(
  function OptionCard(
    {
      value,
      variant = 'status',
      selected,
      role,
      icon: Icon,
      leading,
      title,
      description,
      meta,
      trailing,
      className,
      onClick,
      tabIndex,
      ...props
    },
    ref,
  ) {
    const group = useContext(OptionCardGroupContext);
    const isSelected = group ? group.value === value : Boolean(selected);
    const resolvedRole = role ?? (variant === 'row' ? 'option' : 'radio');
    const groupTabIndex = group
      ? isSelected || group.value === null
        ? 0
        : -1
      : tabIndex;

    return (
      <button
        ref={ref}
        type="button"
        role={resolvedRole}
        aria-checked={resolvedRole === 'radio' ? isSelected : undefined}
        aria-selected={resolvedRole === 'option' ? isSelected : undefined}
        data-value={value}
        data-selected={isSelected ? 'true' : undefined}
        tabIndex={groupTabIndex}
        onClick={(event) => {
          onClick?.(event);
          if (!event.defaultPrevented) group?.select(value);
        }}
        className={cx(
          BASE,
          'rounded-md border',
          variant === 'status'
            ? 'flex-col items-start gap-[6px] p-[10px]'
            : 'min-h-[66px] flex-row items-center gap-[10px] p-compact',
          isSelected
            ? 'border-brand bg-accent-subtle'
            : 'border-border-subtle hover:bg-raised',
          variant === 'row' && !isSelected ? 'bg-transparent' : null,
          variant === 'status' && !isSelected ? 'bg-surface' : null,
          className,
        )}
        {...props}
      >
        {variant === 'status' && Icon ? (
          <Icon
            size={18}
            stroke={2}
            aria-hidden="true"
            className={cx(
              'shrink-0',
              isSelected ? 'text-brand' : 'text-text-secondary',
            )}
          />
        ) : null}
        {variant === 'row' ? leading : null}
        <span
          className={cx(
            'flex min-w-0 flex-col',
            variant === 'status' ? 'gap-[2px]' : 'flex-1 gap-[2px]',
          )}
        >
          <span
            className={cx(
              'truncate',
              variant === 'row' || isSelected
                ? 'text-body font-semibold text-text-primary'
                : 'text-body font-normal text-text-default',
            )}
          >
            {title}
          </span>
          {meta !== undefined ? (
            <span className="truncate text-caption font-normal text-text-dimmed">
              {meta}
            </span>
          ) : null}
          {description !== undefined ? (
            <span className="text-xs leading-note text-text-dimmed">
              {description}
            </span>
          ) : null}
        </span>
        {trailing ? <span className="shrink-0">{trailing}</span> : null}
      </button>
    );
  },
);
