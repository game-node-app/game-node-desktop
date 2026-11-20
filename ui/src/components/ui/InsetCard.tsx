import React from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { cx } from './cx';

export interface InsetCardProps {
  /** Leading 20/24px icon; omit when `leading` or `iconBox` replaces it. */
  icon?: TablerIcon;
  iconSize?: number;
  /** Color utility for the icon, for example `text-warning`. */
  iconClassName?: string;
  /** Turns the icon into the 40×40 accent box used by the tool card. */
  iconBox?: boolean;
  /** Custom leading slot, rendered before the icon. */
  leading?: React.ReactNode;
  title?: React.ReactNode;
  description?: React.ReactNode;
  /** Trailing control, for example a `Switch` or a `Pill`. */
  trailing?: React.ReactNode;
  /** Extra body content below the description (progress row, detail line). */
  children?: React.ReactNode;
  className?: string;
}

/**
 * Inset/summary card from `components.md` §6: `body` surface, subtle border,
 * 8px radius, a leading icon or 40px accent icon box, a 14px title, a 12px
 * description and an optional trailing control. Used by the achievements
 * summary, save protection status and game tool cards.
 */
export function InsetCard({
  icon: Icon,
  iconSize = 20,
  iconClassName,
  iconBox = false,
  leading,
  title,
  description,
  trailing,
  children,
  className,
}: InsetCardProps) {
  return (
    <div
      className={cx(
        'flex items-center gap-sm rounded-md border border-border-subtle bg-body p-sm',
        className,
      )}
    >
      {leading}
      {Icon
        ? iconBox
          ? (
            <span className="flex h-[40px] w-[40px] shrink-0 items-center justify-center rounded-md bg-accent-subtle">
              <Icon
                size={20}
                stroke={2}
                aria-hidden="true"
                className={cx('text-brand', iconClassName)}
              />
            </span>
          )
          : (
            <Icon
              size={iconSize}
              stroke={2}
              aria-hidden="true"
              className={cx('shrink-0', iconClassName)}
            />
          )
        : null}
      <div className="flex min-w-0 flex-1 flex-col gap-[2px]">
        {title !== undefined ? (
          <div className="text-sm font-semibold text-text-primary">{title}</div>
        ) : null}
        {description !== undefined ? (
          <div className="text-xs font-normal text-text-dimmed">
            {description}
          </div>
        ) : null}
        {children !== undefined ? <div className="mt-[6px]">{children}</div> : null}
      </div>
      {trailing !== undefined ? (
        <div className="shrink-0">{trailing}</div>
      ) : null}
    </div>
  );
}
