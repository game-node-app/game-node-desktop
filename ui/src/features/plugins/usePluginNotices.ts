import { useEffect } from 'react';
import {
  noticeSource,
  pickText,
  subscribeToPluginNotices,
  type PluginNotice,
  type PluginNoticeSeverity,
} from '../../api';
import { useI18n } from '../../i18n';
import { pushToast, type ToastKind } from '../../store/uiStore';

const KIND: Record<PluginNoticeSeverity, ToastKind> = {
  info: 'info',
  success: 'success',
  warning: 'warning',
  error: 'danger',
};

/** A plugin's toast stays longer than Core's own: it has a body to read. */
const PLUGIN_TOAST_MS = 8000;

/**
 * Shows what plugins ask Core to show. The text is untrusted: it is picked for the user's language
 * (their locale, then the same language, then the plugin's main text), made plain, and shown as
 * text attributed to the plugin the host authenticated.
 */
export function usePluginNotices(): void {
  const { locale } = useI18n();

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;

    subscribeToPluginNotices((notice: PluginNotice) => {
      if (!active || !Array.isArray(notice?.texts)) return;
      const text = pickText(notice, locale);
      if (!text.title) return;
      pushToast(KIND[notice.severity] ?? 'info', text.title, {
        body: text.body || undefined,
        source: noticeSource(notice),
        durationMs: PLUGIN_TOAST_MS,
      });
    })
      .then((fn) => {
        if (active) {
          unlisten = fn;
        } else {
          fn();
        }
      })
      .catch(() => {
        // A plain browser without the event bridge has no plugin notifications to show.
      });

    return () => {
      active = false;
      unlisten?.();
    };
  }, [locale]);
}
