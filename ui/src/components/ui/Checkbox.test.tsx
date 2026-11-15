import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Checkbox } from './Checkbox';
import { renderUi } from './testUtils';

describe('Checkbox', () => {
  it('toggles checked state and exposes it through the native input', async () => {
    const onChange = vi.fn();
    renderUi(<Checkbox label="Select all" onChange={onChange} />);
    const input = screen.getByRole('checkbox', { name: 'Select all' });
    expect(input).not.toBeChecked();

    await userEvent.click(input);
    expect(input).toBeChecked();
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it('renders the indeterminate state as aria-checked="mixed"', () => {
    renderUi(<Checkbox label="Select all" indeterminate />);
    const input = screen.getByRole('checkbox', { name: 'Select all' });
    expect(input).toBePartiallyChecked();
    expect(input).toHaveAttribute('aria-checked', 'mixed');
    expect(input).toHaveAttribute('data-indeterminate', 'true');
  });

  it('toggles with the keyboard and keeps the 16px redesign box', async () => {
    renderUi(<Checkbox label="Select all" />);
    const input = screen.getByRole('checkbox', { name: 'Select all' });
    expect(input).toHaveClass('h-[16px]', 'w-[16px]', 'rounded-sm', 'bg-canvas');
    input.focus();
    await userEvent.keyboard(' ');
    expect(input).toBeChecked();
  });

  it('supports disabled', () => {
    renderUi(<Checkbox label="Select all" disabled />);
    expect(screen.getByRole('checkbox', { name: 'Select all' })).toBeDisabled();
  });
});
