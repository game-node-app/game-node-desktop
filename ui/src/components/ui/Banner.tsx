import React from 'react';
import { IconX, type TablerIcon } from '@tabler/icons-react';
import { Button, type ButtonVariant } from './Button';
import { cx } from './cx';

export type BannerTone = 'warning' | 'danger' | 'neutral' | 'brand';

export interface BannerAction {
  label: string;
  onClick: () => void;
  variant?: ButtonVariant;
  icon?: TablerIcon;
}

export interface BannerProps {
  tone: BannerTone;
  icon: TablerIcon;
  title: React.ReactNode;
  body?: React.ReactNode;
  action?: BannerAction;
  onDismiss?: () => void;
  /** Localized accessible name for the dismiss button; required when dismissible. */
  dismissLabel?: string;
  className?: string;
}

const TONE_CLASSES: Record<
  BannerTone,
  { container: string; icon: string; role: 'status' | 'alert' }
> = {
  warning: {
    container: 'border-warning bg-warning-subtle',
    icon: 'text-warning',
    role: 'alert',
  },
  danger: {
    container: 'border-danger bg-danger-subtle',
    icon: 'text-danger',
    role: 'alert',
  },
  neutral: {
    container: 'border-text-muted bg-surface',
    icon: 'text-text-muted',
    role: 'status',
  },
  brand: {
    container: 'border-brand bg-accent-subtle',
    icon: 'text-brand',
    role: 'status',
  },
};

/** Dismissible library banner for store, offline and discovery problems. */
export function Banner({
  tone,
  icon: Icon,
  title,
  body,
  action,
  onDismiss,
  dismissLabel,
  className,
}: BannerProps) {
  const styles = TONE_CLASSES[tone];

  return (
    <div
      role={styles.role}
      className={cx(
        'flex items-center gap-[12px] rounded-md border p-[12px]',
        styles.container,
        className,
      )}
    >
      <Icon size={18} stroke={2} aria-hidden="true" className={cx('shrink-0', styles.icon)} />
      <div className="flex min-w-0 flex-1 flex-col gap-[2px]">
        <span className="text-[13px] font-semibold text-text-primary">{title}</span>
        {body !== undefined ? (
          <span className="text-xs font-normal text-text-secondary">{body}</span>
        ) : null}
      </div>
      {action ? (
        <Button
          size="sm"
          variant={action.variant ?? 'secondary'}
          leftIcon={action.icon}
          onClick={action.onClick}
        >
          {action.label}
        </Button>
      ) : null}
      {onDismiss && dismissLabel ? (
        <button
          type="button"
          aria-label={dismissLabel}
          onClick={onDismiss}
          className="inline-flex h-[24px] w-[24px] shrink-0 items-center justify-center rounded-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
        >
          <IconX size={14} stroke={2} aria-hidden="true" />
        </button>
      ) : null}
    </div>
  );
}
