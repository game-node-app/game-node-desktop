import React, { forwardRef } from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { Tooltip } from './Tooltip';
import { cx } from './cx';

export type IconButtonVariant = 'outlined' | 'toggle' | 'overlay';
export type IconButtonSize = 'md' | 'toolbar' | 'sm';

export interface IconButtonProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'aria-label'> {
  icon: TablerIcon;
  /** Mandatory: used as the aria-label and as the tooltip text. */
  label: string;
  /** Keys shown in the tooltip after the label. */
  shortcut?: string[];
  variant?: IconButtonVariant;
  size?: IconButtonSize;
  /** `aria-pressed` for `variant="toggle"`. */
  pressed?: boolean;
  /** Overrides the default 16px icon. */
  iconSize?: number;
  /** Disables the tooltip while keeping the aria-label. */
  withTooltip?: boolean;
}

const BASE =
  'inline-flex shrink-0 items-center justify-center rounded-control transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-2 focus-visible:ring-offset-body disabled:cursor-not-allowed disabled:opacity-50';

/** Reference sizes in px; the 14px root would shrink the rem utilities. */
const SIZES: Record<IconButtonSize, string> = {
  md: 'h-[36px] w-[36px]',
  toolbar: 'h-[34px] w-[34px]',
  sm: 'h-[28px] w-[28px]',
};

const VARIANTS: Record<IconButtonVariant, string> = {
  outlined:
    'border border-border-subtle bg-raised text-text-secondary hover:bg-surface hover:text-text-default',
  toggle:
    'bg-transparent text-text-secondary hover:bg-raised hover:text-text-default',
  overlay: 'bg-black/45 text-text-primary hover:bg-black/60',
};

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
  function IconButton(
    {
      icon: Icon,
      label,
      shortcut,
      variant = 'outlined',
      size = 'md',
      pressed,
      iconSize = 16,
      withTooltip = true,
      type = 'button',
      className,
      disabled,
      ...props
    },
    ref,
  ) {
    const button = (
      <button
        ref={ref}
        type={type}
        aria-label={label}
        aria-pressed={variant === 'toggle' ? Boolean(pressed) : undefined}
        disabled={disabled}
        className={cx(
          BASE,
          SIZES[size],
          VARIANTS[variant],
          variant === 'toggle' && pressed && 'bg-raised text-text-primary',
          className,
        )}
        {...props}
      >
        <Icon size={iconSize} stroke={2} aria-hidden="true" />
      </button>
    );

    if (!withTooltip) return button;

    // A disabled button does not emit hover/focus events; wrapping it keeps
    // the tooltip reachable through the wrapper element.
    return (
      <Tooltip label={label} shortcut={shortcut}>
        {disabled ? <span className="inline-flex">{button}</span> : button}
      </Tooltip>
    );
  },
);
