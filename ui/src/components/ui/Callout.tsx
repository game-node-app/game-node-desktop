import React, { useId } from 'react';
import { IconAlertTriangle, type TablerIcon } from '@tabler/icons-react';
import { cx } from './cx';

export type CalloutTone = 'accent' | 'success' | 'warning' | 'danger' | 'neutral';

export const CALLOUT_TONE_CLASSES: Record<
  CalloutTone,
  { container: string; icon: string }
> = {
  accent: { container: 'border-brand bg-accent-subtle', icon: 'text-brand' },
  success: { container: 'border-success bg-success-subtle', icon: 'text-success' },
  warning: { container: 'border-warning bg-warning-subtle', icon: 'text-warning' },
  danger: { container: 'border-danger bg-danger-subtle', icon: 'text-danger' },
  neutral: { container: 'border-border-subtle bg-raised', icon: 'text-text-muted' },
};

export interface CalloutProps {
  title: React.ReactNode;
  tone?: CalloutTone;
  /** Header icon; defaults to the 16px alert triangle. */
  icon?: TablerIcon;
  /** 12px body copy. */
  children?: React.ReactNode;
  /** Confirmation row, for example a `Checkbox` with its label. */
  confirmation?: React.ReactNode;
  className?: string;
}

/**
 * Tone callout from `components.md` §7. It is a labelled `region`, never an
 * `alert`: the anti-cheat warning is re-rendered on every engine snapshot and
 * `role="alert"` would re-announce it each time.
 */
export function Callout({
  title,
  tone = 'danger',
  icon: Icon = IconAlertTriangle,
  children,
  confirmation,
  className,
}: CalloutProps) {
  const titleId = useId();
  const styles = CALLOUT_TONE_CLASSES[tone];

  return (
    <section
      role="region"
      aria-labelledby={titleId}
      className={cx(
        'flex flex-col gap-compact rounded-md border p-sm',
        styles.container,
        className,
      )}
    >
      <div className="flex items-center gap-compact">
        <Icon
          size={16}
          stroke={2}
          aria-hidden="true"
          className={cx('shrink-0', styles.icon)}
        />
        <span
          id={titleId}
          className="text-body font-semibold text-text-primary"
        >
          {title}
        </span>
      </div>
      {children !== undefined ? (
        <div className="text-xs leading-note text-text-secondary">
          {children}
        </div>
      ) : null}
      {confirmation !== undefined ? (
        <div className="flex items-center gap-[10px] text-xs font-normal text-text-primary">
          {confirmation}
        </div>
      ) : null}
    </section>
  );
}
