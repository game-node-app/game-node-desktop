import React, { useEffect } from 'react';
import { Notification } from '@mantine/core';
import { IconAlertTriangle, IconCircleCheck } from '@tabler/icons-react';
import { dismissToast, useUiStore, type ToastItem } from '../../store/uiStore';
import { useI18n } from '../../i18n';

const toastBaseClass =
  'pointer-events-auto min-w-[240px] max-w-[480px] rounded-sm px-md py-[10px] text-sm font-medium text-white shadow-toast';

const toastKindClass: Record<ToastItem['kind'], string> = {
  success: 'border border-success bg-[#1e3a2f]',
  danger: 'border border-danger bg-[#3d1c1d]',
};

export const ToastContainer: React.FC = () => {
  const toasts = useUiStore((s) => s.toasts);
  const { t } = useI18n();

  useEffect(() => {
    if (toasts.length === 0) return;
    const oldest = toasts[0];
    const timer = setTimeout(() => {
      dismissToast(oldest.id);
    }, 4000);
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
          icon={
            toast.kind === 'success' ? (
              <IconCircleCheck
                size={18}
                stroke={1.75}
                className="text-success"
                aria-hidden="true"
              />
            ) : (
              <IconAlertTriangle
                size={18}
                stroke={1.75}
                className="text-danger"
                aria-hidden="true"
              />
            )
          }
          className={`${toastBaseClass} ${toastKindClass[toast.kind]}`}
        >
          <span>{toast.message}</span>
        </Notification>
      ))}
    </div>
  );
};
