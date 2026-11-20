import { describe, expect, it } from 'vitest';
import { useState } from 'react';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { DialogShell } from './DialogShell';
import { renderUiWithI18n } from '../ui/testUtils';

function ShellExample({ onClose }: { onClose: () => void }) {
  return (
    <DialogShell
      title="Match game"
      subtitle="Hollow Mountain · Steam"
      leading={<span data-testid="shell-leading">cover</span>}
      footerNote={<span>Footer note</span>}
      footerLeading={<button type="button">Not on GameNode</button>}
      footer={
        <>
          <button type="button">Cancel</button>
          <button type="button">Match</button>
        </>
      }
      onClose={onClose}
      geometryKey="dialog-shell-test"
      defaultSize={{ width: 640, height: 480 }}
      minSize={{ width: 420, height: 320 }}
      testId="dialog-shell"
    >
      <p>Body content</p>
    </DialogShell>
  );
}

describe('DialogShell', () => {
  it('renders the title, subtitle, leading slot and body', () => {
    renderUiWithI18n(<ShellExample onClose={() => undefined} />);

    expect(screen.getByTestId('dialog-shell')).toBeInTheDocument();
    expect(screen.getByTestId('dialog-shell')).toHaveClass('flex', 'flex-col');
    expect(screen.getByRole('heading', { name: 'Match game' })).toBeVisible();
    expect(
      screen.getByText('Hollow Mountain · Steam'),
    ).toBeInTheDocument();
    expect(screen.getByTestId('shell-leading')).toBeInTheDocument();
    expect(screen.getByText('Body content')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Close' }),
    ).toBeInTheDocument();
  });

  it('renders the footer note and actions', () => {
    renderUiWithI18n(<ShellExample onClose={() => undefined} />);

    const footer = document.querySelector('[data-dialog-footer]');
    expect(footer).toBeInTheDocument();
    expect(screen.getByText('Footer note')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Not on GameNode' }),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Match' })).toBeVisible();
  });

  it('omits the footer when neither actions nor a note are given', () => {
    renderUiWithI18n(
      <DialogShell
        title="No footer"
        onClose={() => undefined}
        defaultSize={{ width: 640, height: 480 }}
        minSize={{ width: 420, height: 320 }}
        testId="dialog-shell-no-footer"
      >
        Body
      </DialogShell>,
    );

    expect(
      document.querySelector('[data-dialog-footer]'),
    ).not.toBeInTheDocument();
  });

  it('closes on Escape and restores focus to the trigger', async () => {
    const user = userEvent.setup();

    function Harness() {
      const [opened, setOpened] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setOpened(true)}>
            Open shell
          </button>
          {opened && <ShellExample onClose={() => setOpened(false)} />}
        </>
      );
    }

    renderUiWithI18n(<Harness />);
    const trigger = screen.getByRole('button', { name: 'Open shell' });
    await user.click(trigger);
    expect(screen.getByTestId('dialog-shell')).toBeInTheDocument();

    await user.keyboard('{Escape}');
    await waitFor(() =>
      expect(screen.queryByTestId('dialog-shell')).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });
});
