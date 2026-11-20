import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { RadioCard } from './RadioCard';
import { RadioGroup } from './Radio';
import { renderUi } from './testUtils';

function RemoveChoices() {
  const [value, setValue] = useState('ignore');
  return (
    <RadioGroup aria-label="Remove choice" value={value} onChange={setValue}>
      <RadioCard
        value="keep"
        title="Just remove"
        description="It can come back at the next watched-folder scan."
      />
      <RadioCard
        value="ignore"
        title="Remove and don't import again"
        description="The folder will be ignored."
      />
    </RadioGroup>
  );
}

describe('RadioCard', () => {
  it('renders the unselected and selected card surfaces', () => {
    renderUi(<RemoveChoices />);

    const keep = screen.getByText('Just remove').closest('label');
    const ignore = screen
      .getByText("Remove and don't import again")
      .closest('label');

    expect(keep).toHaveClass(
      'gap-sm',
      'rounded-md',
      'border',
      'p-sm',
      'border-border-subtle',
      'bg-surface',
    );
    expect(ignore).toHaveClass('border-brand', 'bg-accent-subtle');
    expect(screen.getByText('Just remove')).toHaveClass(
      'text-body',
      'font-semibold',
      'text-text-primary',
    );
    expect(
      screen.getByText('It can come back at the next watched-folder scan.'),
    ).toHaveClass('text-xs', 'leading-note', 'text-text-dimmed');
  });

  it('selects a card by clicking anywhere on it', async () => {
    const user = userEvent.setup();
    renderUi(<RemoveChoices />);

    await user.click(screen.getByText('Just remove'));

    const keepRadio = screen.getByRole('radio', { name: /Just remove/ });
    await waitFor(() => expect(keepRadio).toBeChecked());
    expect(screen.getByText('Just remove').closest('label')).toHaveClass(
      'border-brand',
      'bg-accent-subtle',
    );
  });

  it('moves between cards with the arrow keys', async () => {
    const user = userEvent.setup();
    renderUi(<RemoveChoices />);

    const ignoreRadio = screen.getByRole('radio', {
      name: /Remove and don't import again/,
    });
    ignoreRadio.focus();
    await user.keyboard('{ArrowUp}');

    const keepRadio = screen.getByRole('radio', { name: /Just remove/ });
    await waitFor(() => expect(keepRadio).toBeChecked());
    expect(keepRadio).toHaveFocus();
  });

  it('works as a directly controlled card outside a group', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { rerender } = renderUi(
      <RadioCard
        value="keep"
        title="Just remove"
        name="remove-choice"
        checked
        onChange={onChange}
      />,
    );

    expect(screen.getByRole('radio', { name: /Just remove/ })).toBeChecked();
    expect(screen.getByText('Just remove').closest('label')).toHaveClass(
      'border-brand',
    );

    rerender(
      <RadioCard
        value="keep"
        title="Just remove"
        name="remove-choice"
        checked={false}
        onChange={onChange}
      />,
    );
    await user.click(screen.getByRole('radio', { name: /Just remove/ }));
    expect(onChange).toHaveBeenCalledTimes(1);
  });
});
