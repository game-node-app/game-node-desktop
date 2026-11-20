import { describe, expect, it, vi } from 'vitest';
import { useState } from 'react';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { AppModal } from './AppModal';
import { renderUiWithI18n } from '../ui/testUtils';

function AppModalExample({ onClose }: { onClose: () => void }) {
  return (
    <AppModal.Root opened onClose={onClose} size={420}>
      <AppModal.Overlay />
      <AppModal.Content data-testid="app-modal-content">
        <AppModal.Body>Confirmation body</AppModal.Body>
      </AppModal.Content>
    </AppModal.Root>
  );
}

describe('AppModal defaults', () => {
  it('uses the dialog surface styles', () => {
    renderUiWithI18n(<AppModalExample onClose={() => undefined} />);

    expect(screen.getByTestId('app-modal-content')).toHaveClass(
      'rounded-dialog',
      'border-border-overlay',
      'bg-secondary',
      'shadow-dialog',
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
          {opened && <AppModalExample onClose={() => setOpened(false)} />}
        </>
      );
    }

    renderUiWithI18n(<Harness />);
    const trigger = screen.getByRole('button', {
      name: 'Open confirmation',
    });
    await user.click(trigger);
    expect(screen.getByTestId('app-modal-content')).toBeInTheDocument();

    await user.keyboard('{Escape}');
    await waitFor(() =>
      expect(
        screen.queryByTestId('app-modal-content'),
      ).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('keeps a consumer surface class override', () => {
    renderUiWithI18n(
      <AppModal.Root opened onClose={vi.fn()} size={420}>
        <AppModal.Overlay />
        <AppModal.Content
          data-testid="app-modal-content"
          classNames={{ content: 'rounded-md shadow-modal' }}
        >
          <AppModal.Body>Body</AppModal.Body>
        </AppModal.Content>
      </AppModal.Root>,
    );

    const content = screen.getByTestId('app-modal-content');
    expect(content).toHaveClass('rounded-md', 'shadow-modal');
  });
});
