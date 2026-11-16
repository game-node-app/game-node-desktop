import React, { useEffect } from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { AppModal } from '../AppModal';
import { cx } from '../ui/cx';

export type ConfirmDialogTone =
  | 'accent'
  | 'success'
  | 'warning'
  | 'danger'
  | 'neutral';

const TONE_CLASSES: Record<ConfirmDialogTone, string> = {
  accent: 'bg-accent-subtle text-brand',
  success: 'bg-success-subtle text-success',
  warning: 'bg-warning-subtle text-warning',
  danger: 'bg-danger-subtle text-danger',
  neutral: 'bg-raised text-text-muted',
};

export interface ConfirmDialogProps {
  tone?: ConfirmDialogTone;
  icon: TablerIcon;
  title: React.ReactNode;
  description?: React.ReactNode;
  children?: React.ReactNode;
  actions: React.ReactNode;
  /** Dialog width in px, per the design reference. */
  size?: number;
  /**
   * Element focused when the dialog opens. Danger and warning confirmations
   * pass their non-destructive action here.
   */
  initialFocusRef?: React.RefObject<HTMLElement | null>;
  onClose: () => void;
  testId?: string;
  className?: string;
}

/**
 * Confirmation shell shared by the AppModal dialogs (D09–D13).
 *
 * It composes `AppModal`, so the modal layer, focus trap, Escape handling and
 * focus restoration stay in the existing primitives. The classes come from
 * `components.md` §2.
 */
export const ConfirmDialog: React.FC<ConfirmDialogProps> = ({
  tone = 'neutral',
  icon: Icon,
  title,
  description,
  children,
  actions,
  size = 460,
  initialFocusRef,
  onClose,
  testId,
  className,
}) => {
  useEffect(() => {
    let element: HTMLElement | null = null;
    let addedAutofocus = false;

    const focusInitial = () => {
      const target = initialFocusRef?.current;
      if (!target) return;
      element = target;
      if (!target.hasAttribute('data-autofocus')) {
        target.setAttribute('data-autofocus', '');
        addedAutofocus = true;
      }
      target.focus({ preventScroll: true });
    };

    // Mantine's focus trap focuses `[data-autofocus]` (or the first tabbable
    // element) from its own timer. Running once now and once on the next task
    // makes the requested element win both orders.
    focusInitial();
    const timer = window.setTimeout(focusInitial, 0);

    return () => {
      window.clearTimeout(timer);
      if (element && addedAutofocus) {
        element.removeAttribute('data-autofocus');
      }
    };
  }, [initialFocusRef]);

  return (
    <AppModal.Root
      opened
      onClose={onClose}
      centered
      size={size}
      padding={0}
      transitionProps={{ duration: 0 }}
    >
      <AppModal.Overlay backgroundOpacity={0.62} />
      <AppModal.Content
        data-testid={testId}
        style={{ '--modal-size': `${size}px` } as React.CSSProperties}
        classNames={{
          content: cx(
            'gap-[14px] p-control-inline rounded-dialog border border-border-overlay bg-secondary shadow-dialog',
            className,
          ),
        }}
      >
        <div
          data-testid="confirm-dialog-tone"
          aria-hidden="true"
          className={cx(
            'flex h-[40px] w-[40px] shrink-0 items-center justify-center rounded-full',
            TONE_CLASSES[tone],
          )}
        >
          <Icon size={20} stroke={2} aria-hidden="true" />
        </div>

        <AppModal.Title className="text-dialog-title font-semibold text-text-primary">
          {title}
        </AppModal.Title>

        {description != null && (
          <p className="text-body leading-body-relaxed text-text-secondary">
            {description}
          </p>
        )}

        {children}

        <div
          data-testid="confirm-dialog-actions"
          className="flex items-center justify-end gap-compact pt-[6px]"
        >
          {actions}
        </div>
      </AppModal.Content>
    </AppModal.Root>
  );
};
