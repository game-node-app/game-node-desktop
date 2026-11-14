import React, { useEffect } from 'react';
import { Notification } from '@mantine/core';
import { IconAlertTriangle, IconCircleCheck, IconInfoCircle } from '@tabler/icons-react';
import { dismissToast, useUiStore, type ToastItem } from '../../store/uiStore';
import { useI18n } from '../../i18n';

const toastBaseClass =
  'pointer-events-auto min-w-[240px] max-w-[480px] rounded-sm px-md py-[10px] text-sm font-medium text-white shadow-toast';

const toastKindClass: Record<ToastItem['kind'], string> = {
  success: 'border border-success bg-[#1e3a2f]',
  danger: 'border border-danger bg-[#3d1c1d]',
  warning: 'border border-warning bg-[#3a2c12]',
  info: 'border border-border-subtle bg-raised',
};

const DEFAULT_TOAST_MS = 4000;

const ToastIcon: React.FC<{ kind: ToastItem['kind'] }> = ({ kind }) => {
  if (kind === 'success') {
    return <IconCircleCheck size={18} stroke={1.75} className="text-success" aria-hidden="true" />;
  }
  if (kind === 'info') {
    return <IconInfoCircle size={18} stroke={1.75} className="text-text-dimmed" aria-hidden="true" />;
  }
  return (
    <IconAlertTriangle
      size={18}
      stroke={1.75}
      className={kind === 'warning' ? 'text-warning' : 'text-danger'}
      aria-hidden="true"
    />
  );
};

export const ToastContainer: React.FC = () => {
  const toasts = useUiStore((s) => s.toasts);
  const { t } = useI18n();

  useEffect(() => {
    if (toasts.length === 0) return;
    const oldest = toasts[0];
    const timer = setTimeout(() => {
      dismissToast(oldest.id);
    }, oldest.durationMs ?? DEFAULT_TOAST_MS);
    return () => clearTimeout(timer);
  }, [toasts]);

  if (toasts.length === 0) {
    return null;
  }

  return (
    <div
      className="pointer-events-none fixed bottom-6 left-1/2 z-[10000] flex -translate-x-1/2 flex-col items-center gap-compact"
      role="region"
      aria-label="Notifications"
    >
      {toasts.map((toast: ToastItem) => (
        <Notification
          key={toast.id}
          role="status"
          aria-live="polite"
          withCloseButton
          onClose={() => dismissToast(toast.id)}
          closeButtonProps={{ 'aria-label': t('Dismiss') || 'Dismiss' }}
          icon={<ToastIcon kind={toast.kind} />}
          className={`${toastBaseClass} ${toastKindClass[toast.kind]}`}
        >
          {/* Every string here may come from a plugin: React renders it as text, never as markup,
              and nothing in a toast is a link. */}
          {toast.source ? (
            <span className="block text-xs font-normal text-text-dimmed">
              {t('plugins.noticeFrom', { plugin: toast.source })}
            </span>
          ) : null}
          <span className="block break-words">{toast.message}</span>
          {toast.body ? (
            <span className="mt-1 block whitespace-pre-line break-words text-xs font-normal">
              {toast.body}
            </span>
          ) : null}
        </Notification>
      ))}
    </div>
  );
};
