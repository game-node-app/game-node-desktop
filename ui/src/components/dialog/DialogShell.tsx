import React from 'react';
import { FloatingModal, type ModalSize } from '../FloatingModal';
import { cx } from '../ui/cx';

export interface DialogShellProps {
  /** Dialog title, rendered as the accessible dialog label. */
  title: React.ReactNode;
  /** Optional second line under the title. */
  subtitle?: React.ReactNode;
  /** Optional leading slot (for example a 30×40 game cover). */
  leading?: React.ReactNode;
  children?: React.ReactNode;
  /** Right-aligned footer actions. */
  footer?: React.ReactNode;
  /** Optional left-aligned footer actions, before the spacer. */
  footerLeading?: React.ReactNode;
  /** Optional note shown at the start of the footer. */
  footerNote?: React.ReactNode;
  onClose: () => void;
  /** Stable key used by FloatingModal to persist position and size. */
  geometryKey?: string;
  defaultSize: ModalSize;
  minSize: ModalSize;
  testId?: string;
  /** Optional `data-testid` for the header close button. */
  closeTestId?: string;
  className?: string;
  bodyClassName?: string;
  footerClassName?: string;
  closeOnEscape?: boolean;
}

/**
 * Work-surface shell shared by the floating dialogs (D01–D08).
 *
 * It only composes `FloatingModal`, so dragging, resizing, the modal layer,
 * focus trapping, Escape handling and focus restoration stay in the existing
 * primitives. The classes come from `components.md` §1.
 */
export const DialogShell: React.FC<DialogShellProps> = ({
  title,
  subtitle,
  leading,
  children,
  footer,
  footerLeading,
  footerNote,
  onClose,
  geometryKey,
  defaultSize,
  minSize,
  testId,
  closeTestId,
  className,
  bodyClassName,
  footerClassName,
  closeOnEscape = true,
}) => {
  const hasFooter =
    footer != null || footerLeading != null || footerNote != null;

  return (
    <FloatingModal.Root
      opened
      onClose={onClose}
      geometryKey={geometryKey}
      defaultSize={defaultSize}
      minSize={minSize}
      closeOnEscape={closeOnEscape}
    >
      <FloatingModal.Overlay />
      <FloatingModal.Content
        data-testid={testId}
        className={cx('flex flex-col', className)}
      >
        <FloatingModal.Header>
          {leading}
          <div className="flex min-w-0 flex-1 flex-col gap-[2px]">
            <FloatingModal.Title className="truncate text-md font-semibold text-text-primary">
              {title}
            </FloatingModal.Title>
            {subtitle != null && (
              <p className="truncate text-xs text-text-dimmed">{subtitle}</p>
            )}
          </div>
          <FloatingModal.CloseButton data-testid={closeTestId} />
        </FloatingModal.Header>

        <FloatingModal.Body className={cx('gap-md p-lg', bodyClassName)}>
          {children}
        </FloatingModal.Body>

        {hasFooter && (
          <FloatingModal.Footer
            data-dialog-footer=""
            className={cx(
              'justify-start gap-compact bg-surface px-lg !py-[14px]',
              footerClassName,
            )}
          >
            {footerNote != null && (
              <div
                data-dialog-footer-note=""
                className="flex min-w-0 items-center gap-[6px] text-xs text-text-dimmed"
              >
                {footerNote}
              </div>
            )}
            {footerLeading != null && (
              <div className="flex shrink-0 items-center gap-compact">
                {footerLeading}
              </div>
            )}
            <div className="flex-1" />
            {footer}
          </FloatingModal.Footer>
        )}

        <FloatingModal.ResizeHandle />
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
