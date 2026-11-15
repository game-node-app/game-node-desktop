import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { Kbd, KbdCombo } from './Kbd';
import { renderUi } from './testUtils';

describe('Kbd', () => {
  it('renders an 18px key cap with the redesign surface', () => {
    renderUi(<Kbd>Ctrl</Kbd>);
    const key = screen.getByText('Ctrl');
    expect(key.tagName).toBe('KBD');
    expect(key).toHaveClass(
      'h-[18px]',
      'min-w-[18px]',
      'rounded-sm',
      'bg-raised',
      'border-border-subtle',
      'text-[11px]',
      'font-medium',
      'text-text-secondary',
    );
  });

  it('renders a combo with an accessible joined name', () => {
    renderUi(<KbdCombo keys={['Ctrl', 'K']} />);
    expect(screen.getByText('Ctrl').closest('[aria-label="Ctrl+K"]')).not.toBeNull();
    expect(screen.getByText('K')).toBeInTheDocument();
  });

  it('allows an explicit accessible name and returns null when empty', () => {
    const { rerender } = renderUi(
      <KbdCombo keys={['Shift', 'F10']} aria-label="Context menu" />,
    );
    expect(
      screen.getByText('Shift').closest('[aria-label="Context menu"]'),
    ).not.toBeNull();

    rerender(<KbdCombo keys={[]} />);
    expect(screen.queryByRole('group')).not.toBeInTheDocument();
  });
});
