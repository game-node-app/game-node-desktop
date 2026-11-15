import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Tooltip } from './Tooltip';
import { renderUi } from './testUtils';

describe('Tooltip', () => {
  it('shows its label on hover with the tooltip surface token', async () => {
    renderUi(
      <Tooltip label="Details panel" openDelay={0}>
        <button type="button">Target</button>
      </Tooltip>,
    );

    await userEvent.hover(screen.getByRole('button', { name: 'Target' }));
    const tooltip = await screen.findByText('Details panel');
    expect(tooltip).toBeInTheDocument();
    expect(tooltip.closest('[role="tooltip"]')).toHaveClass('bg-tooltip');
  });

  it('renders a KbdCombo after the label', async () => {
    renderUi(
      <Tooltip label="Details panel" shortcut={['Ctrl', 'I']} openDelay={0}>
        <button type="button">Target</button>
      </Tooltip>,
    );

    await userEvent.hover(screen.getByRole('button', { name: 'Target' }));
    expect(await screen.findByText('Ctrl')).toBeInTheDocument();
    expect(screen.getByText('I')).toBeInTheDocument();
    expect(screen.getByText('Ctrl').closest('[aria-label="Ctrl+I"]')).not.toBeNull();
  });

  it('opens on keyboard focus', async () => {
    renderUi(
      <Tooltip label="Details panel" openDelay={0}>
        <button type="button">Target</button>
      </Tooltip>,
    );

    screen.getByRole('button', { name: 'Target' }).focus();
    expect(await screen.findByText('Details panel')).toBeInTheDocument();
  });
});
