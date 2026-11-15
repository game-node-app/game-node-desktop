import React, { useEffect, useRef, useState } from 'react';
import { Notification, Progress } from '@mantine/core';
import {
  IconCircleCheck,
  IconCircleX,
  IconDownload,
  IconInfoCircle,
  IconX,
} from '@tabler/icons-react';
import { dismissToast, useUiStore, type ToastItem, type ToastKind } from '../../store/uiStore';
import { useI18n } from '../../i18n';

/** D10: success and info dismiss themselves; danger and progress stay. */
export const TOAST_AUTO_CLOSE_MS = 6000;

/** The card shows at most two actions; the store already caps the list. */
const MAX_VISIBLE_ACTIONS = 2;

const accentBarClass: Record<ToastKind, string> = {
  success: 'bg-success',
  danger: 'bg-danger',
  info: 'bg-brand',
  progress: 'bg-brand',
};

const iconClass: Record<ToastKind, string> = {
  success: 'text-success',
  danger: 'text-danger',
  info: 'text-text-secondary',
  progress: 'text-brand',
};

function ToastIcon({ kind }: { kind: ToastKind }) {
  if (kind === 'success') {
    return <IconCircleCheck size={18} stroke={1.75} className={iconClass.success} aria-hidden="true" />;
  }
  if (kind === 'danger') {
    return <IconCircleX size={18} stroke={1.75} className={iconClass.danger} aria-hidden="true" />;
  }
  if (kind === 'progress') {
    return <IconDownload size={18} stroke={1.75} className={iconClass.progress} aria-hidden="true" />;
  }
  return <IconInfoCircle size={18} stroke={1.75} className={iconClass.info} aria-hidden="true" />;
}

function isAutoClosing(toast: ToastItem): boolean {
  return (toast.kind === 'success' || toast.kind === 'info') && toast.persistent !== true;
}

/**
 * One notification's 6 s timer. The remaining time survives hover/focus pauses, and only
 * changes to the auto-close eligibility or the pause state touch the timer, so unrelated
 * re-renders (a title or progress update) never restart it.
 */
function useAutoCloseToast(toast: ToastItem, paused: boolean): void {
  const autoClose = isAutoClosing(toast);
  const remainingMs = useRef(TOAST_AUTO_CLOSE_MS);

  useEffect(() => {
    if (!autoClose || paused) return;
    const startedAt = Date.now();
    const timer = window.setTimeout(() => {
      remainingMs.current = 0;
      dismissToast(toast.id);
    }, remainingMs.current);
    return () => {
      window.clearTimeout(timer);
      const elapsed = Date.now() - startedAt;
      remainingMs.current = Math.max(0, remainingMs.current - elapsed);
    };
  }, [autoClose, paused, toast.id]);
}

interface ToastCardProps {
  toast: ToastItem;
}

const ToastCard: React.FC<ToastCardProps> = ({ toast }) => {
  const { t } = useI18n();
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);

  useAutoCloseToast(toast, hovered || focused);

  const actions = (toast.actions ?? []).slice(0, MAX_VISIBLE_ACTIONS);
  const progress =
    toast.progress == null ? null : Math.max(0, Math.min(100, Math.round(toast.progress)));

  return (
    <Notification
      role="status"
      aria-live="polite"
      data-toast-kind={toast.kind}
      withCloseButton
      onClose={() => dismissToast(toast.id)}
      closeButtonProps={{
        'aria-label': t('Dismiss') || 'Dismiss',
        icon: <IconX size={14} stroke={1.5} />,
      }}
      icon={
        <span className="flex items-center gap-[10px]">
          <span
            aria-hidden="true"
            className={`h-9 w-[3px] rounded-xs ${accentBarClass[toast.kind]}`}
          />
          <ToastIcon kind={toast.kind} />
        </span>
      }
      title={
        <span className="text-body font-semibold leading-[1.2] text-text-primary">
          {toast.title}
        </span>
      }
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onFocus={() => setFocused(true)}
      onBlur={(event) => {
        const next = event.relatedTarget as Node | null;
        if (!next || !event.currentTarget.contains(next)) {
          setFocused(false);
        }
      }}
      className="pointer-events-auto w-[380px] max-w-[calc(100vw_-_48px)] rounded-md border border-border-overlay bg-secondary shadow-notification"
      classNames={{
        root: '!p-sm',
        icon: '!me-[10px] !h-auto !w-auto !rounded-none !bg-transparent',
        body: '!me-0 flex min-w-0 flex-col gap-[2px]',
        title: '!mb-0',
        description: '!text-xs !text-text-secondary',
        closeButton: '!text-text-dimmed hover:!bg-raised hover:!text-text-primary',
      }}
    >
      {toast.body ? (
        <div className="text-xs leading-[1.4] text-text-secondary">{toast.body}</div>
      ) : null}

      {actions.length > 0 ? (
        <div className="mt-compact flex flex-wrap items-center gap-sm pl-[13px]">
          {actions.map((action, index) => (
            <button
              key={`${action.label}-${index}`}
              type="button"
              onClick={action.onClick}
              className="rounded-xs text-xs font-semibold text-brand transition-colors hover:text-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
            >
              {action.label}
            </button>
          ))}
        </div>
      ) : null}

      {progress != null ? (
        <div className="mt-compact pl-[13px]">
          <Progress
            value={progress}
            size={4}
            radius="xs"
            aria-label={`${toast.title} ${progress}%`}
            classNames={{ root: 'bg-raised', section: 'bg-brand' }}
          />
        </div>
      ) : null}
    </Notification>
  );
};

export const ToastContainer: React.FC = () => {
  const toasts = useUiStore((s) => s.toasts);

  if (toasts.length === 0) {
    return null;
  }

  return (
    <div
      className="pointer-events-none fixed bottom-6 right-6 z-[10000] flex flex-col items-end gap-[10px]"
      role="region"
      aria-label="Notifications"
      aria-live="polite"
    >
      {toasts.map((toast) => (
        <ToastCard key={toast.id} toast={toast} />
      ))}
    </div>
  );
};
