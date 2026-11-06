import React, { useEffect } from 'react';
import { dismissToast, useUiStore, type ToastItem } from '../../store/uiStore';
import { useI18n } from '../../i18n';

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
    <div className="toast-container" role="region" aria-label="Notifications">
      {toasts.map((toast: ToastItem) => (
        <div
          key={toast.id}
          className={`toast-item ${toast.kind}`}
          role="status"
          aria-live="polite"
        >
          <span className="toast-message">{toast.message}</span>
          <button
            type="button"
            className="toast-dismiss"
            onClick={() => dismissToast(toast.id)}
            aria-label={t('Dismiss') || 'Dismiss'}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
};
