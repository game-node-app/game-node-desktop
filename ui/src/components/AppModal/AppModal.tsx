import React from 'react';
import { Modal } from '@mantine/core';
import { useFocusRestore } from '../../hooks/useFocusRestore';
import { useModalLayer } from '../modalLayer';

const DEFAULT_CONTENT_CLASS =
  'flex flex-col overflow-hidden rounded-md border border-border-subtle';

function cx(...values: Array<string | false | null | undefined>): string {
  return values.filter(Boolean).join(' ');
}

export interface AppModalRootProps
  extends Omit<React.ComponentProps<typeof Modal.Root>, 'zIndex'> {
  /** Minimum layer; the last opened modal still renders on top. */
  zIndex?: number;
}

const AppModalRoot: React.FC<AppModalRootProps> = ({
  opened,
  onClose,
  zIndex = 1000,
  closeOnEscape = true,
  children,
  ...props
}) => {
  const layer = useModalLayer({ opened, onClose, closeOnEscape, zIndex });
  useFocusRestore(opened);

  return (
    <Modal.Root
      centered
      transitionProps={{ duration: 0 }}
      padding={0}
      {...props}
      opened={opened}
      onClose={onClose}
      zIndex={layer.zIndex}
      closeOnEscape={false}
      returnFocus={false}
    >
      {children}
    </Modal.Root>
  );
};

export type AppModalOverlayProps = React.ComponentProps<typeof Modal.Overlay>;

const AppModalOverlay: React.FC<AppModalOverlayProps> = ({
  backgroundOpacity = 0.62,
  ...props
}) => <Modal.Overlay backgroundOpacity={backgroundOpacity} {...props} />;

export interface AppModalContentProps
  extends Omit<React.ComponentProps<typeof Modal.Content>, 'classNames'> {
  classNames?: React.ComponentProps<typeof Modal.Content>['classNames'];
}

const AppModalContent: React.FC<AppModalContentProps> = ({
  className,
  classNames,
  ...props
}) => (
  <Modal.Content
    {...props}
    classNames={{
      ...classNames,
      content: cx(DEFAULT_CONTENT_CLASS, className, classNames?.content),
    }}
  />
);

export type AppModalHeaderProps = React.ComponentProps<typeof Modal.Header>;
const AppModalHeader: React.FC<AppModalHeaderProps> = (props) => (
  <Modal.Header
    className="flex items-center justify-between gap-md border-b border-border-subtle"
    {...props}
  />
);

export type AppModalTitleProps = React.ComponentProps<typeof Modal.Title>;
const AppModalTitle: React.FC<AppModalTitleProps> = (props) => (
  <Modal.Title className="text-lg font-semibold text-text-default" {...props} />
);

export type AppModalCloseButtonProps = React.ComponentProps<
  typeof Modal.CloseButton
>;
const AppModalCloseButton: React.FC<AppModalCloseButtonProps> = (props) => (
  <Modal.CloseButton
    className="rounded-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default"
    {...props}
  />
);

export type AppModalBodyProps = React.ComponentProps<typeof Modal.Body>;
const AppModalBody: React.FC<AppModalBodyProps> = (props) => (
  <Modal.Body className="flex min-h-0 flex-1 flex-col gap-md overflow-y-auto" {...props} />
);

type AppModalComponent = React.FC<AppModalRootProps> & {
  Root: typeof AppModalRoot;
  Overlay: typeof AppModalOverlay;
  Content: typeof AppModalContent;
  Header: typeof AppModalHeader;
  Title: typeof AppModalTitle;
  CloseButton: typeof AppModalCloseButton;
  Body: typeof AppModalBody;
};

export const AppModal = Object.assign(AppModalRoot, {
  Root: AppModalRoot,
  Overlay: AppModalOverlay,
  Content: AppModalContent,
  Header: AppModalHeader,
  Title: AppModalTitle,
  CloseButton: AppModalCloseButton,
  Body: AppModalBody,
}) as AppModalComponent;

AppModalRoot.displayName = 'AppModal.Root';
AppModalOverlay.displayName = 'AppModal.Overlay';
AppModalContent.displayName = 'AppModal.Content';
AppModalHeader.displayName = 'AppModal.Header';
AppModalTitle.displayName = 'AppModal.Title';
AppModalCloseButton.displayName = 'AppModal.CloseButton';
AppModalBody.displayName = 'AppModal.Body';
