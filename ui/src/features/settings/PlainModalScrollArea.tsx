import React from 'react';

/**
 * Mantine's `Modal.Content` normally wraps its children in a native scroll area.
 * The settings surfaces manage their own scrolling panes, so render the children
 * directly inside the modal paper and keep the intended flex layout.
 */
export const PlainModalScrollArea: React.FC<
  React.PropsWithChildren<{ style?: React.CSSProperties }>
> = ({ children }) => <>{children}</>;
