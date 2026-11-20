import React from 'react';
import { cx } from '../../../components/ui/cx';

export type SettingsCardTone = 'default' | 'warning' | 'danger';

export const SETTINGS_CARD_TONE_BORDER: Record<SettingsCardTone, string> = {
  default: 'border-border-subtle',
  warning: 'border-warning',
  danger: 'border-danger',
};

export interface SettingsCardProps
  extends Omit<React.HTMLAttributes<HTMLDivElement>, 'title'> {
  /** 11/600 uppercase card header text. */
  title: React.ReactNode;
  /** Trailing header action; switches the header to the 56px variant. */
  action?: React.ReactNode;
  /** Optional tone border for attention (expired/credential) cards. */
  tone?: SettingsCardTone;
  /** Overrides the generated id of the `h3` header. */
  headerId?: string;
}

/**
 * Card surface for one Settings group (`components.md` §4). Rows are
 * `SettingsRow` children, whose own top border separates them from the header
 * and from each other.
 */
export function SettingsCard({
  title,
  action,
  tone = 'default',
  headerId,
  className,
  children,
  ...props
}: SettingsCardProps) {
  const generatedId = React.useId();
  const resolvedHeaderId = headerId ?? `${generatedId}-header`;

  return (
    <div
      {...props}
      className={cx(
        'flex flex-col overflow-hidden rounded-card border bg-secondary',
        SETTINGS_CARD_TONE_BORDER[tone],
        className,
      )}
    >
      <div
        className={cx(
          action
            ? 'flex items-center gap-sm px-md py-sm'
            : 'px-md pt-row pb-xs',
        )}
      >
        <h3
          id={resolvedHeaderId}
          className={cx(
            'min-w-0 text-caption font-semibold uppercase tracking-label text-text-muted',
            action ? 'flex-1' : undefined,
          )}
        >
          {title}
        </h3>
        {action ? <div className="shrink-0">{action}</div> : null}
      </div>
      {children}
    </div>
  );
}
