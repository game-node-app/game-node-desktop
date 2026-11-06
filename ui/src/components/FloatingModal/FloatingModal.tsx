import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useState,
} from 'react';
import {
  FloatingWindow,
  FocusTrap,
  OptionalPortal,
  Overlay,
} from '@mantine/core';
import type { UseFloatingWindowOptions } from '@mantine/hooks';
import { IconResize, IconX } from '@tabler/icons-react';
import { useFocusRestore } from '../../hooks/useFocusRestore';
import { useI18n } from '../../i18n';
import { useModalLayer } from '../modalLayer';
import { useFloatingModalGeometry } from './useFloatingModalGeometry';
import type { ModalSize } from './geometry';

export type { ModalGeometry, ModalSize } from './geometry';

const DRAG_HANDLE_SELECTOR = '[data-floating-modal-handle]';
const EXCLUDE_DRAG_HANDLE_SELECTOR =
  'button, a, input, select, textarea, [data-floating-modal-no-drag]';

const DEFAULT_CONTENT_CLASS =
  'overflow-hidden rounded-md border border-border-subtle shadow-modal focus:outline-none';

function cx(...values: Array<string | false | null | undefined>): string {
  return values.filter(Boolean).join(' ');
}

export interface FloatingModalRootProps {
  opened: boolean;
  onClose: () => void;
  /** Stable key used to persist the window position and size. */
  geometryKey?: string;
  defaultSize: ModalSize;
  minSize: ModalSize;
  maxSize?: Partial<ModalSize>;
  zIndex?: number;
  draggable?: boolean;
  resizable?: boolean;
  modal?: boolean;
  closeOnEscape?: boolean;
  closeOnClickOutside?: boolean;
  persistGeometry?: boolean;
  constrainOffset?: number;
  withinPortal?: boolean;
  children?: React.ReactNode;
}

interface FloatingModalContextValue {
  opened: boolean;
  onClose: () => void;
  zIndex: number;
  draggable: boolean;
  resizable: boolean;
  modal: boolean;
  closeOnClickOutside: boolean;
  constrainOffset: number;
  geometryKey?: string;
  persistGeometry: boolean;
  defaultSize: ModalSize;
  minSize: ModalSize;
  maxSize?: Partial<ModalSize>;
  titleId: string;
  titleMounted: boolean;
  registerTitle: () => void;
}

const FloatingModalContext = createContext<FloatingModalContextValue | null>(
  null,
);

function useFloatingModalContext(): FloatingModalContextValue {
  const context = useContext(FloatingModalContext);
  if (!context) {
    throw new Error(
      'FloatingModal components must be used within FloatingModal.Root',
    );
  }
  return context;
}

const FloatingModalRoot: React.FC<FloatingModalRootProps> = ({
  opened,
  onClose,
  geometryKey,
  defaultSize,
  minSize,
  maxSize,
  zIndex = 1000,
  draggable = true,
  resizable = true,
  modal = true,
  closeOnEscape = true,
  closeOnClickOutside = true,
  persistGeometry = true,
  constrainOffset = 24,
  withinPortal = true,
  children,
}) => {
  const layer = useModalLayer({ opened, closeOnEscape, onClose, zIndex });
  const titleId = useId();
  const [titleMounted, setTitleMounted] = useState(false);
  const registerTitle = useCallback(() => setTitleMounted(true), []);
  useFocusRestore(opened);

  const context = useMemo<FloatingModalContextValue>(
    () => ({
      opened,
      onClose,
      zIndex: layer.zIndex,
      draggable,
      resizable,
      modal,
      closeOnClickOutside,
      constrainOffset,
      geometryKey,
      persistGeometry,
      defaultSize,
      minSize,
      maxSize,
      titleId,
      titleMounted,
      registerTitle,
    }),
    [
      opened,
      onClose,
      layer.zIndex,
      draggable,
      resizable,
      modal,
      closeOnClickOutside,
      constrainOffset,
      geometryKey,
      persistGeometry,
      defaultSize,
      minSize,
      maxSize,
      titleId,
      titleMounted,
      registerTitle,
    ],
  );

  if (!opened) return null;

  return (
    <OptionalPortal withinPortal={withinPortal}>
      <FloatingModalContext.Provider value={context}>
        {children}
      </FloatingModalContext.Provider>
    </OptionalPortal>
  );
};

export interface FloatingModalOverlayProps
  extends Omit<React.ComponentPropsWithoutRef<'div'>, 'color'> {
  backgroundOpacity?: number;
  color?: string;
  blur?: number | string;
}

const FloatingModalOverlay: React.FC<FloatingModalOverlayProps> = ({
  backgroundOpacity = 0.62,
  color,
  blur,
  className,
  ...props
}) => {
  const context = useFloatingModalContext();
  const handleClick = context.closeOnClickOutside
    ? context.onClose
    : undefined;

  return (
    <Overlay
      {...props}
      fixed
      aria-hidden
      zIndex={context.zIndex}
      backgroundOpacity={backgroundOpacity}
      color={color}
      blur={blur}
      onClick={handleClick}
      data-floating-modal-overlay=""
      className={className}
    />
  );
};

export interface FloatingModalContentProps
  extends Omit<
    React.ComponentPropsWithoutRef<'div'>,
    'ref' | keyof UseFloatingWindowOptions
  > {
  children?: React.ReactNode;
}

const FloatingModalContent: React.FC<FloatingModalContentProps> = ({
  className,
  children,
  style,
  ...props
}) => {
  const context = useFloatingModalContext();
  const geometry = useFloatingModalGeometry({
    geometryKey: context.geometryKey,
    persist: context.persistGeometry,
    defaultSize: context.defaultSize,
    minSize: context.minSize,
    maxSize: context.maxSize,
    offset: context.constrainOffset,
  });

  return (
    <FocusTrap active={context.opened} innerRef={geometry.rootRef}>
      <FloatingWindow
        {...props}
        withinPortal={false}
        zIndex={context.zIndex}
        enabled={context.draggable}
        constrainToViewport
        constrainOffset={context.constrainOffset}
        dragHandleSelector={DRAG_HANDLE_SELECTOR}
        excludeDragHandleSelector={EXCLUDE_DRAG_HANDLE_SELECTOR}
        initialPosition={geometry.initialPosition}
        dimensions={geometry.dimensions}
        setPositionRef={geometry.setPositionRef}
        onPositionChange={geometry.onPositionChange}
        onDragEnd={geometry.onDragEnd}
        onSizeChange={geometry.onSizeChange}
        onResizeEnd={geometry.onResizeEnd}
        role={props.role ?? 'dialog'}
        aria-modal={context.modal ? true : undefined}
        aria-labelledby={
          props['aria-labelledby'] ??
          (!props['aria-label'] && context.titleMounted
            ? context.titleId
            : undefined)
        }
        tabIndex={props.tabIndex ?? -1}
        style={style}
        className={cx(DEFAULT_CONTENT_CLASS, className)}
      >
        {children}
      </FloatingWindow>
    </FocusTrap>
  );
};

export type FloatingModalHeaderProps = React.ComponentPropsWithoutRef<'div'>;

const FloatingModalHeader: React.FC<FloatingModalHeaderProps> = ({
  className,
  children,
  ...props
}) => {
  const context = useFloatingModalContext();

  return (
    <div
      {...props}
      data-floating-modal-handle={context.draggable ? '' : undefined}
      className={cx(
        'flex shrink-0 items-center justify-between gap-md border-b border-border-subtle px-control-inline py-md',
        context.draggable &&
          'cursor-grab touch-none select-none active:cursor-grabbing',
        className,
      )}
    >
      {children}
    </div>
  );
};

export type FloatingModalTitleProps = React.ComponentPropsWithoutRef<'h2'>;

const FloatingModalTitle: React.FC<FloatingModalTitleProps> = ({
  className,
  children,
  id,
  ...props
}) => {
  const context = useFloatingModalContext();
  const { registerTitle } = context;

  useEffect(() => {
    registerTitle();
  }, [registerTitle]);

  return (
    <h2
      {...props}
      id={id ?? context.titleId}
      className={cx('text-lg font-semibold text-text-default', className)}
    >
      {children}
    </h2>
  );
};

export interface FloatingModalCloseButtonProps
  extends React.ComponentPropsWithoutRef<'button'> {
  icon?: React.ReactNode;
}

const FloatingModalCloseButton: React.FC<FloatingModalCloseButtonProps> = ({
  icon,
  className,
  onClick,
  'aria-label': ariaLabel,
  ...props
}) => {
  const context = useFloatingModalContext();
  const { t } = useI18n();

  const handleClick = (event: React.MouseEvent<HTMLButtonElement>) => {
    onClick?.(event);
    if (!event.defaultPrevented) {
      context.onClose();
    }
  };

  return (
    <button
      {...props}
      type="button"
      data-floating-modal-no-drag=""
      aria-label={ariaLabel ?? t('Close')}
      onClick={handleClick}
      className={cx(
        'flex shrink-0 items-center justify-center rounded-sm p-compact text-text-dimmed transition-colors hover:bg-raised hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand',
        className,
      )}
    >
      {icon ?? <IconX size={18} stroke={1.75} aria-hidden="true" />}
    </button>
  );
};

export type FloatingModalBodyProps = React.ComponentPropsWithoutRef<'div'>;

const FloatingModalBody: React.FC<FloatingModalBodyProps> = ({
  className,
  children,
  ...props
}) => (
  <div
    {...props}
    className={cx(
      'flex min-h-0 flex-1 flex-col overflow-y-auto',
      className,
    )}
  >
    {children}
  </div>
);

export type FloatingModalFooterProps = React.ComponentPropsWithoutRef<'div'>;

const FloatingModalFooter: React.FC<FloatingModalFooterProps> = ({
  className,
  children,
  ...props
}) => (
  <div
    {...props}
    className={cx(
      'flex shrink-0 items-center justify-end gap-compact border-t border-border-subtle px-control-inline py-sm',
      className,
    )}
  >
    {children}
  </div>
);

export interface FloatingModalResizeHandleProps
  extends React.ComponentPropsWithoutRef<
    typeof FloatingWindow.ResizeHandle
  > {}

const FloatingModalResizeHandle: React.FC<
  FloatingModalResizeHandleProps
> = ({ className, ...props }) => {
  const context = useFloatingModalContext();
  const { t } = useI18n();

  if (!context.resizable) return null;

  return (
    <FloatingWindow.ResizeHandle
      {...props}
      data-floating-modal-no-drag=""
      aria-label={props['aria-label'] ?? t('Resize window')}
      className={cx(
        'absolute right-0 bottom-0 z-10 flex h-4 w-4 cursor-nwse-resize touch-none items-end justify-end p-[2px] text-text-dimmed opacity-70 transition-opacity hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none',
        className,
      )}
    >
      <IconResize size={10} stroke={1.5} aria-hidden="true" />
    </FloatingWindow.ResizeHandle>
  );
};

type FloatingModalComponent = React.FC<FloatingModalRootProps> & {
  Root: typeof FloatingModalRoot;
  Overlay: typeof FloatingModalOverlay;
  Content: typeof FloatingModalContent;
  Header: typeof FloatingModalHeader;
  Title: typeof FloatingModalTitle;
  CloseButton: typeof FloatingModalCloseButton;
  Body: typeof FloatingModalBody;
  Footer: typeof FloatingModalFooter;
  ResizeHandle: typeof FloatingModalResizeHandle;
};

export const FloatingModal = Object.assign(FloatingModalRoot, {
  Root: FloatingModalRoot,
  Overlay: FloatingModalOverlay,
  Content: FloatingModalContent,
  Header: FloatingModalHeader,
  Title: FloatingModalTitle,
  CloseButton: FloatingModalCloseButton,
  Body: FloatingModalBody,
  Footer: FloatingModalFooter,
  ResizeHandle: FloatingModalResizeHandle,
}) as FloatingModalComponent;

FloatingModalRoot.displayName = 'FloatingModal.Root';
FloatingModalOverlay.displayName = 'FloatingModal.Overlay';
FloatingModalContent.displayName = 'FloatingModal.Content';
FloatingModalHeader.displayName = 'FloatingModal.Header';
FloatingModalTitle.displayName = 'FloatingModal.Title';
FloatingModalCloseButton.displayName = 'FloatingModal.CloseButton';
FloatingModalBody.displayName = 'FloatingModal.Body';
FloatingModalFooter.displayName = 'FloatingModal.Footer';
FloatingModalResizeHandle.displayName = 'FloatingModal.ResizeHandle';
