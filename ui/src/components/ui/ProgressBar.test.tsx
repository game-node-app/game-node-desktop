import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { ProgressBar } from './ProgressBar';
import { renderUi } from './testUtils';

describe('ProgressBar', () => {
  it('exposes the accessible range and the runtime fill width', () => {
    renderUi(<ProgressBar value={2} max={5} label="Adding games" />);

    const bar = screen.getByRole('progressbar', { name: 'Adding games' });
    expect(bar).toHaveAttribute('aria-valuemin', '0');
    expect(bar).toHaveAttribute('aria-valuemax', '5');
    expect(bar).toHaveAttribute('aria-valuenow', '2');
    expect(bar.firstElementChild).toHaveStyle({ width: '40%' });
  });

  it('renders the 6px raised track and the brand fill', () => {
    renderUi(<ProgressBar value={53} label="Achievements" />);

    const bar = screen.getByRole('progressbar', { name: 'Achievements' });
    expect(bar).toHaveClass(
      'h-[6px]',
      'w-full',
      'overflow-hidden',
      'rounded-thumb',
      'bg-raised',
    );
    expect(bar.firstElementChild).toHaveClass(
      'h-full',
      'rounded-thumb',
      'bg-brand',
    );
    expect(bar.firstElementChild).toHaveStyle({ width: '53%' });
  });

  it('clamps out-of-range values instead of announcing an impossible percent', () => {
    const { rerender } = renderUi(
      <ProgressBar value={-10} max={100} label="Progress" />,
    );
    const bar = screen.getByRole('progressbar', { name: 'Progress' });
    expect(bar).toHaveAttribute('aria-valuenow', '0');
    expect(bar.firstElementChild).toHaveStyle({ width: '0%' });

    rerender(<ProgressBar value={120} max={100} label="Progress" />);
    expect(bar).toHaveAttribute('aria-valuenow', '100');
    expect(bar.firstElementChild).toHaveStyle({ width: '100%' });
  });
});
