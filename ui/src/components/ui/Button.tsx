import React, { forwardRef } from 'react';
import { IconLoader2, type TablerIcon } from '@tabler/icons-react';
import { cx } from './cx';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
export type ButtonSize = 'md' | 'sm';

export interface ButtonProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'color'> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Leading icon, per `components.md`. */
  leftIcon?: TablerIcon;
  /** Trailing icon, per `components.md`. */
  rightIcon?: TablerIcon;
  /** Long-form alias for `leftIcon`. */
  leadingIcon?: TablerIcon;
  /** Long-form alias for `rightIcon`. */
  trailingIcon?: TablerIcon;
  /** Replaces the leading icon with a spinner and disables the control. */
  loading?: boolean;
  fullWidth?: boolean;
}

const BASE =
  'inline-flex shrink-0 items-center justify-center gap-[6px] whitespace-nowrap rounded-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-2 focus-visible:ring-offset-body disabled:cursor-not-allowed disabled:opacity-50';

/**
 * Reference heights are px, not rem: the app root keeps its pre-existing 14px
 * font size, so the default Tailwind scale would render 36 as 31.5 and 32 as 28.
 */
const SIZES: Record<ButtonSize, string> = {
  md: 'h-[36px] px-[16px] text-sm',
  sm: 'h-[32px] px-[12px] text-sm',
};

const VARIANTS: Record<ButtonVariant, string> = {
  primary:
    'bg-brand font-semibold text-text-primary hover:bg-[var(--color-primary-hover)] active:bg-tertiary',
  secondary:
    'border border-border-subtle bg-raised font-medium text-text-default hover:bg-surface',
  ghost:
    'bg-transparent font-medium text-text-secondary hover:bg-raised hover:text-text-default',
  danger: 'bg-danger font-medium text-text-primary hover:bg-danger/90',
};

function renderIcon(Icon: TablerIcon | undefined) {
  if (!Icon) return null;
  return <Icon size={16} stroke={2} aria-hidden="true" />;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    variant = 'secondary',
    size = 'md',
    leftIcon,
    rightIcon,
    leadingIcon,
    trailingIcon,
    loading = false,
    fullWidth = false,
    disabled,
    type = 'button',
    className,
    children,
    ...props
  },
  ref,
) {
  const Leading = leadingIcon ?? leftIcon;
  const Trailing = trailingIcon ?? rightIcon;
  const isDisabled = disabled || loading;

  return (
    <button
      ref={ref}
      type={type}
      className={cx(
        BASE,
        SIZES[size],
        VARIANTS[variant],
        fullWidth && 'w-full',
        className,
      )}
      disabled={isDisabled}
      aria-busy={loading || undefined}
      {...props}
    >
      {loading ? (
        <IconLoader2 size={16} stroke={2} aria-hidden="true" className="animate-spin" />
      ) : (
        renderIcon(Leading)
      )}
      <span className="truncate">{children}</span>
      {renderIcon(Trailing)}
    </button>
  );
});
