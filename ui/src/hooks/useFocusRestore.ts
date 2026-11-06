import { useLayoutEffect, useRef } from 'react';

/**
 * Restores keyboard focus to the element that opened a dialog when it closes.
 *
 * Mantine's focus trap keeps focus inside the modal but does not return it to the
 * trigger. The desktop-shell specification requires Escape/backdrop dismissal to
 * restore focus, so each dialog captures its trigger and focuses it again when it
 * becomes inactive or unmounts.
 *
 * `active` should follow the dialog's open state for dialogs that stay mounted;
 * conditionally-rendered dialogs can rely on the unmount cleanup alone.
 */
export function useFocusRestore(active = true): void {
  const triggerRef = useRef<HTMLElement | null>(null);

  useLayoutEffect(() => {
    if (!active) return;

    const activeElement = document.activeElement;
    if (
      activeElement instanceof HTMLElement &&
      activeElement !== document.body &&
      activeElement !== document.documentElement
    ) {
      triggerRef.current = activeElement;
    }

    return () => {
      const trigger = triggerRef.current;
      triggerRef.current = null;
      // The trigger may belong to a menu or row that closed with the dialog.
      if (trigger?.isConnected) {
        trigger.focus({ preventScroll: true });
      }
    };
  }, [active]);
}
