import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { FilterChip } from './FilterChip';
import { renderUi } from './testUtils';

describe('FilterChip', () => {
  it('renders the label, value and the required remove label', async () => {
    const onRemove = vi.fn();
    renderUi(
      <FilterChip
        label="Store:"
        value="Steam, Epic"
        removeLabel="Remove filter Store"
        onRemove={onRemove}
      />,
    );

    expect(screen.getByText('Store:')).toHaveClass('text-text-secondary');
    expect(screen.getByText('Steam, Epic')).toHaveClass('font-semibold');
    const remove = screen.getByRole('button', { name: 'Remove filter Store' });
    expect(remove).toHaveClass('text-text-secondary');
    await userEvent.click(remove);
    expect(onRemove).toHaveBeenCalledTimes(1);
  });

  it('renders a label-only chip with the 28px height', () => {
    renderUi(
      <FilterChip
        label="Favorites only"
        removeLabel="Remove filter Favorites only"
        onRemove={() => undefined}
      />,
    );
    const chip = screen.getByText('Favorites only').parentElement;
    expect(chip).toHaveClass('h-[28px]', 'bg-accent-subtle', 'border-brand');
  });
});
