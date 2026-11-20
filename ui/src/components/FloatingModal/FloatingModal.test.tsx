import { describe, expect, it, vi } from 'vitest';
import { useState } from 'react';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { FloatingModal } from './FloatingModal';
import { renderUiWithI18n } from '../ui/testUtils';

function FloatingModalExample({
  onClose,
  draggable = true,
}: {
  onClose: () => void;
  draggable?: boolean;
}) {
  return (
    <FloatingModal.Root
      opened
      onClose={onClose}
      defaultSize={{ width: 640, height: 480 }}
      minSize={{ width: 420, height: 320 }}
      draggable={draggable}
    >
      <FloatingModal.Overlay />
      <FloatingModal.Content data-testid="floating-modal-content">
        <FloatingModal.Header data-testid="floating-modal-header">
          <FloatingModal.Title>Dialog title</FloatingModal.Title>
          <FloatingModal.CloseButton />
        </FloatingModal.Header>
        <FloatingModal.Body>Dialog body</FloatingModal.Body>
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
}

describe('FloatingModal defaults', () => {
  it('uses the dialog surface, drag icon and close button styles', () => {
    renderUiWithI18n(<FloatingModalExample onClose={() => undefined} />);

    const content = screen.getByTestId('floating-modal-content');
    expect(content).toHaveClass(
      'rounded-dialog',
      'border-border-overlay',
      'bg-secondary',
      'shadow-dialog',
    );

    const header = screen.getByTestId('floating-modal-header');
    expect(header).toHaveAttribute('data-floating-modal-handle', '');

    const dragIcon = header.querySelector(
      '[data-floating-modal-drag-icon]',
    );
    expect(dragIcon).toBeInTheDocument();
    expect(dragIcon).toHaveAttribute('aria-hidden', 'true');
    expect(dragIcon).toHaveAttribute('width', '16');

    const closeButton = screen.getByRole('button', { name: 'Close' });
    expect(closeButton).toHaveClass(
      'h-[28px]',
      'w-[28px]',
      'text-text-secondary',
    );
    expect(closeButton.querySelector('svg')).toHaveAttribute('width', '16');
  });

  it('hides the drag affordance when the dialog is not draggable', () => {
    renderUiWithI18n(
      <FloatingModalExample
        onClose={() => undefined}
        draggable={false}
      />,
    );

    const header = screen.getByTestId('floating-modal-header');
    expect(header).not.toHaveAttribute('data-floating-modal-handle');
    expect(
      header.querySelector('[data-floating-modal-drag-icon]'),
    ).not.toBeInTheDocument();
  });

  it('closes through the close button', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    renderUiWithI18n(<FloatingModalExample onClose={onClose} />);

    await user.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('closes on Escape and restores focus to the trigger', async () => {
    const user = userEvent.setup();

    function Harness() {
      const [opened, setOpened] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setOpened(true)}>
            Open dialog
          </button>
          {opened && (
            <FloatingModalExample onClose={() => setOpened(false)} />
          )}
        </>
      );
    }

    renderUiWithI18n(<Harness />);
    const trigger = screen.getByRole('button', { name: 'Open dialog' });
    await user.click(trigger);
    expect(
      screen.getByTestId('floating-modal-content'),
    ).toBeInTheDocument();

    await user.keyboard('{Escape}');
    await waitFor(() =>
      expect(
        screen.queryByTestId('floating-modal-content'),
      ).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });
});
