import React from 'react';
import { commands } from '../../api/commands';
import { uiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';

export interface ResizeHandleProps {
  currentWidth: number;
  minWidth?: number;
  maxWidth?: number;
  step?: number;
  ariaLabel?: string;
  className?: string;
}

export function ResizeHandle({
  currentWidth,
  minWidth = 160,
  maxWidth = 360,
  step = 16,
  ariaLabel,
  className,
}: ResizeHandleProps) {
  const { t } = useI18n();

  const clamp = (val: number) =>
    Math.min(maxWidth, Math.max(minWidth, Math.round(val)));

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startX =
      typeof e.clientX === 'number' && !Number.isNaN(e.clientX)
        ? e.clientX
        : currentWidth;
    const startWidth = currentWidth;

    const onPointerMove = (moveEv: PointerEvent | MouseEvent) => {
      const clientX =
        typeof moveEv.clientX === 'number' && !Number.isNaN(moveEv.clientX)
          ? moveEv.clientX
          : ((moveEv as unknown as Record<string, unknown>).clientX as
              | number
              | undefined);
      if (clientX === undefined || Number.isNaN(clientX)) {
        return;
      }
      const deltaX = clientX - startX;
      const nextWidth = clamp(startWidth + deltaX);
      uiStore.setState((s) => ({ ...s, sidebarDragWidth: nextWidth }));
    };

    const onPointerUp = (upEv: PointerEvent | MouseEvent) => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
      window.removeEventListener('pointercancel', onPointerUp);
      window.removeEventListener('mousemove', onPointerMove);
      window.removeEventListener('mouseup', onPointerUp);

      const clientX =
        typeof upEv.clientX === 'number' && !Number.isNaN(upEv.clientX)
          ? upEv.clientX
          : ((upEv as unknown as Record<string, unknown>).clientX as
              | number
              | undefined);
      const deltaX =
        clientX !== undefined && !Number.isNaN(clientX)
          ? clientX - startX
          : (uiStore.state.sidebarDragWidth ?? startWidth) - startWidth;
      const finalWidth = clamp(startWidth + deltaX);
      uiStore.setState((s) => ({ ...s, sidebarDragWidth: null }));
      commands.libraryInput({ setSidebarWidth: finalWidth });
    };

    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    window.addEventListener('pointercancel', onPointerUp);
    window.addEventListener('mousemove', onPointerMove);
    window.addEventListener('mouseup', onPointerUp);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'ArrowLeft') {
      e.preventDefault();
      const next = clamp(currentWidth - step);
      uiStore.setState((s) => ({ ...s, sidebarDragWidth: null }));
      commands.libraryInput({ setSidebarWidth: next });
    } else if (e.key === 'ArrowRight') {
      e.preventDefault();
      const next = clamp(currentWidth + step);
      uiStore.setState((s) => ({ ...s, sidebarDragWidth: null }));
      commands.libraryInput({ setSidebarWidth: next });
    }
  };

  return (
    <div
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      aria-label={ariaLabel ?? t('Resize the sidebar')}
      aria-valuenow={currentWidth}
      aria-valuemin={minWidth}
      aria-valuemax={maxWidth}
      onPointerDown={handlePointerDown}
      onKeyDown={handleKeyDown}
      data-testid="sidebar-resize-handle"
      className={`relative z-50 flex h-full w-[8px] cursor-col-resize touch-none items-center justify-center select-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-inset focus-visible:outline-none ${className ?? ''}`}
    >
      <div className="h-full w-px bg-border-subtle" />
    </div>
  );
}
