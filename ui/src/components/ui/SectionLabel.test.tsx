import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { SectionLabel } from './SectionLabel';
import { renderUi } from './testUtils';

describe('SectionLabel', () => {
  it('renders the uppercase overline treatment with sidebar inset', () => {
    renderUi(<SectionLabel inset>Your space</SectionLabel>);
    const label = screen.getByText('Your space');
    expect(label).toHaveClass(
      'text-[11px]',
      'font-semibold',
      'uppercase',
      'tracking-label',
      'text-text-muted',
    );
    expect(label.parentElement).toHaveClass('px-[10px]');
  });

  it('omits the sidebar inset in panels', () => {
    renderUi(<SectionLabel>On GameNode</SectionLabel>);
    expect(screen.getByText('On GameNode').parentElement).toHaveClass('px-0');
  });

  it('renders an optional trailing action', () => {
    renderUi(
      <SectionLabel action={<button type="button">New view</button>}>
        Collections and views
      </SectionLabel>,
    );
    expect(screen.getByRole('button', { name: 'New view' })).toBeInTheDocument();
  });
});
