import { describe, expect, it } from 'vitest';
import { useRef, useState } from 'react';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IconTrash } from '@tabler/icons-react';
import { ConfirmDialog, type ConfirmDialogTone } from './ConfirmDialog';
import { renderUiWithI18n } from '../ui/testUtils';

function ConfirmExample({
  tone = 'danger',
  onClose,
}: {
  tone?: ConfirmDialogTone;
  onClose: () => void;
}) {
  const cancelRef = useRef<HTMLButtonElement>(null);

  return (
    <ConfirmDialog
      tone={tone}
      icon={IconTrash}
      title="Remove game?"
      description="Game files on disk are not deleted."
      initialFocusRef={cancelRef}
      onClose={onClose}
      testId="confirm-dialog"
      actions={
        <>
          <button type="button">Remove</button>
          <button ref={cancelRef} type="button">
            Cancel
          </button>
        </>
      }
    >
      <button type="button">Delete</button>
    </ConfirmDialog>
  );
}

const TONES: Array<{
  tone: ConfirmDialogTone;
  background: string;
  foreground: string;
}> = [
  { tone: 'accent', background: 'bg-accent-subtle', foreground: 'text-brand' },
  {
    tone: 'success',
    background: 'bg-success-subtle',
    foreground: 'text-success',
  },
  {
    tone: 'warning',
    background: 'bg-warning-subtle',
    foreground: 'text-warning',
  },
  {
    tone: 'danger',
    background: 'bg-danger-subtle',
    foreground: 'text-danger',
  },
  {
    tone: 'neutral',
    background: 'bg-raised',
    foreground: 'text-text-muted',
  },
];

describe('ConfirmDialog', () => {
  it.each(TONES)(
    'renders the $tone tone circle',
    ({ tone, background, foreground }) => {
      renderUiWithI18n(
        <ConfirmExample tone={tone} onClose={() => undefined} />,
      );

      const circle = screen.getByTestId('confirm-dialog-tone');
      expect(circle).toHaveClass(
        'h-[40px]',
        'w-[40px]',
        'rounded-full',
        background,
        foreground,
      );
    },
  );

  it('renders the title, description, content slot and actions', () => {
    renderUiWithI18n(<ConfirmExample onClose={() => undefined} />);

    expect(screen.getByTestId('confirm-dialog')).toHaveClass(
      'rounded-dialog',
      'border-border-overlay',
      'bg-secondary',
      'shadow-dialog',
    );
    const content = screen.getByTestId('confirm-dialog');
    expect(content).toHaveStyle('--modal-size: 460px');
    expect(
      screen.getByRole('heading', { name: 'Remove game?' }),
    ).toBeVisible();
    expect(
      screen.getByText('Game files on disk are not deleted.'),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Delete' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Remove' }),
    ).toBeInTheDocument();
  });

  it('focuses the element passed through initialFocusRef', async () => {
    renderUiWithI18n(<ConfirmExample onClose={() => undefined} />);

    const cancel = screen.getByRole('button', { name: 'Cancel' });
    await waitFor(() => expect(document.activeElement).toBe(cancel));
    expect(document.activeElement).not.toBe(
      screen.getByRole('button', { name: 'Delete' }),
    );
  });

  it('closes on Escape and restores focus to the trigger', async () => {
    const user = userEvent.setup();

    function Harness() {
      const [opened, setOpened] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setOpened(true)}>
            Open confirmation
          </button>
          {opened && <ConfirmExample onClose={() => setOpened(false)} />}
        </>
      );
    }

    renderUiWithI18n(<Harness />);
    const trigger = screen.getByRole('button', {
      name: 'Open confirmation',
    });
    await user.click(trigger);
    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();

    await user.keyboard('{Escape}');
    await waitFor(() =>
      expect(
        screen.queryByTestId('confirm-dialog'),
      ).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });
});
