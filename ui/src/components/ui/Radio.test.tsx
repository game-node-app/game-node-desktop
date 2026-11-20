import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Radio, RadioGroup } from './Radio';
import { renderUi } from './testUtils';

function Choices() {
  const [value, setValue] = useState('alpha');
  return (
    <RadioGroup aria-label="Choice" value={value} onChange={setValue}>
      <Radio value="alpha" label="Alpha" />
      <Radio value="beta" label="Beta" />
      <Radio value="gamma" label="Gamma" />
    </RadioGroup>
  );
}

describe('Radio', () => {
  it('renders the 16px ring with the checked and unchecked treatments', () => {
    renderUi(<Choices />);

    const alpha = screen.getByRole('radio', { name: 'Alpha' });
    const beta = screen.getByRole('radio', { name: 'Beta' });

    for (const radio of [alpha, beta]) {
      expect(radio).toHaveClass(
        'h-[16px]',
        'w-[16px]',
        'rounded-full',
        'border-[1.5px]',
        'border-text-muted',
        'bg-transparent',
        'checked:border-[5px]',
        'checked:border-brand',
        'checked:bg-transparent',
      );
    }

    expect(alpha).toBeChecked();
    expect(beta).not.toBeChecked();
  });

  it('moves selection and focus with the arrow keys', async () => {
    const user = userEvent.setup();
    renderUi(<Choices />);

    screen.getByRole('radio', { name: 'Alpha' }).focus();
    await user.keyboard('{ArrowDown}');

    const beta = screen.getByRole('radio', { name: 'Beta' });
    await waitFor(() => expect(beta).toBeChecked());
    expect(beta).toHaveFocus();
  });

  it('selects a radio with a click and reports the group value', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderUi(
      <RadioGroup aria-label="Choice" defaultValue="alpha" onChange={onChange}>
        <Radio value="alpha" label="Alpha" />
        <Radio value="gamma" label="Gamma" />
      </RadioGroup>,
    );

    await user.click(screen.getByRole('radio', { name: 'Gamma' }));
    expect(onChange).toHaveBeenCalledWith('gamma');
    expect(screen.getByRole('radio', { name: 'Gamma' })).toBeChecked();
  });

  it('keeps a disabled group non-interactive', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderUi(
      <RadioGroup
        aria-label="Choice"
        value="alpha"
        onChange={onChange}
        disabled
      >
        <Radio value="alpha" label="Alpha" />
        <Radio value="beta" label="Beta" />
      </RadioGroup>,
    );

    const beta = screen.getByRole('radio', { name: 'Beta' });
    expect(beta).toBeDisabled();
    await user.click(beta);
    expect(onChange).not.toHaveBeenCalled();
  });
});
