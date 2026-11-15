import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ChoiceChip } from './ChoiceChip';
import { renderUi } from './testUtils';

describe('ChoiceChip', () => {
  it('exposes the selected state through aria-pressed and the brand surface', () => {
    const { rerender } = renderUi(<ChoiceChip selected={false}>7 days</ChoiceChip>);
    const chip = screen.getByRole('button', { name: '7 days' });
    expect(chip).toHaveAttribute('aria-pressed', 'false');
    expect(chip).toHaveClass('bg-surface', 'text-text-default');

    rerender(<ChoiceChip selected>7 days</ChoiceChip>);
    expect(chip).toHaveAttribute('aria-pressed', 'true');
    expect(chip).toHaveClass('bg-accent-subtle', 'border-brand', 'font-semibold');
  });

  it('toggles with the keyboard and calls onClick', async () => {
    const onClick = vi.fn();
    const { rerender } = renderUi(<ChoiceChip onClick={onClick}>7 days</ChoiceChip>);
    const chip = screen.getByRole('button', { name: '7 days' });

    chip.focus();
    await userEvent.keyboard('{Enter}');
    expect(onClick).toHaveBeenCalledTimes(1);

    await userEvent.keyboard(' ');
    expect(onClick).toHaveBeenCalledTimes(2);

    rerender(
      <ChoiceChip selected onClick={onClick}>
        7 days
      </ChoiceChip>,
    );
    expect(screen.getByRole('button', { name: '7 days' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });
});
