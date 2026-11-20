import React from 'react';
import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderUiWithI18n } from '../../../components/ui/testUtils';
import { SettingsSearch } from './SettingsSearch';

function Harness({ onBubble }: { onBubble?: () => void }) {
  const [value, setValue] = React.useState('');
  return (
    <div
      onKeyDown={(event) => {
        if (event.key === 'Escape') onBubble?.();
      }}
    >
      <SettingsSearch value={value} onChange={setValue} />
    </div>
  );
}

describe('SettingsSearch', () => {
  it('renders the search field with translated placeholder and label', () => {
    renderUiWithI18n(<Harness />);

    const input = screen.getByRole('searchbox', { name: 'Search settings' });
    expect(input).toHaveAttribute('type', 'search');
    expect(input).toHaveAttribute('placeholder', 'Search settings');
    expect(input.parentElement).toHaveClass(
      'h-[32px]',
      'rounded-control',
      'border-border-subtle',
      'bg-canvas',
      'focus-within:border-brand',
    );
  });

  it('clears a non-empty query on Escape without closing the modal', async () => {
    const onBubble = vi.fn();
    renderUiWithI18n(<Harness onBubble={onBubble} />);
    const input = screen.getByRole('searchbox', { name: 'Search settings' });

    await userEvent.type(input, 'bandeja');
    expect(input).toHaveValue('bandeja');

    await userEvent.keyboard('{Escape}');
    expect(input).toHaveValue('');
    expect(onBubble).not.toHaveBeenCalled();
  });

  it('lets Escape bubble when the field is empty', async () => {
    const onBubble = vi.fn();
    renderUiWithI18n(<Harness onBubble={onBubble} />);
    const input = screen.getByRole('searchbox', { name: 'Search settings' });

    input.focus();
    await userEvent.keyboard('{Escape}');
    expect(onBubble).toHaveBeenCalled();
  });

  it('reports typed text to the controlled value', async () => {
    const onChange = vi.fn();
    renderUiWithI18n(<SettingsSearch value="" onChange={onChange} />);

    await userEvent.type(
      screen.getByRole('searchbox', { name: 'Search settings' }),
      'geral',
    );
    expect(onChange).toHaveBeenCalled();
  });
});
