import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Switch } from './Switch';
import { renderUi } from './testUtils';

describe('Switch', () => {
  it('renders Mantine’s switch role with the redesign track classes', () => {
    renderUi(<Switch label="Sync" defaultChecked />);
    const input = screen.getByRole('switch', { name: 'Sync' });
    expect(input).toBeChecked();
    expect(input).toHaveClass('peer');
    expect(input.nextElementSibling).toHaveClass(
      'peer-checked:[--switch-thumb-bg:#FFFFFF]',
    );
  });

  it('toggles with a click and with the keyboard', async () => {
    const onChange = vi.fn();
    renderUi(<Switch label="Sync" onChange={onChange} />);
    const input = screen.getByRole('switch', { name: 'Sync' });

    await userEvent.click(input);
    expect(input).toBeChecked();
    expect(onChange).toHaveBeenCalledTimes(1);

    input.focus();
    await userEvent.keyboard(' ');
    expect(input).not.toBeChecked();
    expect(onChange).toHaveBeenCalledTimes(2);
  });

  it('keeps the label and disabled state accessible', () => {
    renderUi(<Switch label="Sync" disabled />);
    const input = screen.getByRole('switch', { name: 'Sync' });
    expect(input).toBeDisabled();
    expect(screen.getByText('Sync')).toHaveClass('text-text-default');
  });
});
