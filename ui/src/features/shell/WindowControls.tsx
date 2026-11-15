import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { IconCopy, IconMinus, IconSquare, IconX } from '@tabler/icons-react';
import { isTauriRuntime } from '../../api/commands';
import { useI18n } from '../../i18n';
import { cx } from '../../components/ui/cx';

export interface WindowControlsProps {
  className?: string;
}

/**
 * Whether the controls can reach the native window. Outside Tauri (Vite or
 * `VITE_UI_MOCK=1`) they still render the full design but every action is a
 * no-op, so the browser preview never crashes. A partial `__TAURI_INTERNALS__`
 * (for example a fake host in tests) is treated as no native window too.
 */
function nativeWindow() {
  if (!isTauriRuntime()) return null;
  try {
    return getCurrentWindow();
  } catch {
    return null;
  }
}

/** Maximize or restore the current window. Exported for the top bar's double-click. */
export function toggleWindowMaximize(): void {
  const win = nativeWindow();
  if (!win) return;
  win.toggleMaximize().catch(() => undefined);
}

/**
 * The custom window chrome, per D9. Close goes through the same window close
 * request as before, so the existing close-to-background lifecycle applies.
 */
export function WindowControls({ className }: WindowControlsProps = {}) {
  const { t } = useI18n();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = nativeWindow();
    if (!win) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const refresh = () => {
      win
        .isMaximized()
        .then((value) => {
          if (!disposed) setMaximized(value);
        })
        .catch(() => undefined);
    };

    refresh();
    win
      .onResized(() => refresh())
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const minimize = () => {
    nativeWindow()?.minimize().catch(() => undefined);
  };

  const toggleMaximize = () => {
    toggleWindowMaximize();
  };

  const close = () => {
    nativeWindow()?.close().catch(() => undefined);
  };

  const cell =
    'flex h-topbar w-window-control shrink-0 items-center justify-center text-text-secondary transition-colors hover:bg-raised hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-inset';

  return (
    <div
      data-testid="window-controls"
      className={cx('flex h-topbar shrink-0 items-center', className)}
    >
      <button
        type="button"
        aria-label={t('Shell::Minimize')}
        onClick={minimize}
        className={cell}
      >
        <IconMinus size={16} stroke={2} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label={maximized ? t('Shell::Restore') : t('Shell::Maximize')}
        onClick={toggleMaximize}
        className={cell}
      >
        {maximized ? (
          <IconCopy size={12} stroke={2} aria-hidden="true" />
        ) : (
          <IconSquare size={12} stroke={2} aria-hidden="true" />
        )}
      </button>
      <button
        type="button"
        aria-label={t('Shell::Close')}
        onClick={close}
        className={cx(cell, 'hover:bg-danger')}
      >
        <IconX size={16} stroke={2} aria-hidden="true" />
      </button>
    </div>
  );
}
