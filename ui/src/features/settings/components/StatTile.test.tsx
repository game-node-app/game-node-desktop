import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { renderUi } from '../../../components/ui/testUtils';
import { StatTile } from './StatTile';

describe('StatTile', () => {
  it('renders the value and caption in the canvas tile', () => {
    renderUi(<StatTile value="312" caption="in library" />);

    const value = screen.getByText('312');
    expect(value).toHaveClass('text-md', 'font-semibold', 'text-text-primary');
    expect(screen.getByText('in library')).toHaveClass(
      'text-caption',
      'text-text-dimmed',
    );
    expect(value.parentElement).toHaveClass(
      'flex-1',
      'flex-col',
      'gap-[2px]',
      'p-xs',
      'rounded-control',
      'border',
      'border-border-subtle',
      'bg-canvas',
    );
  });
});
