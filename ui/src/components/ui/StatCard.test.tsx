import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { StatCard } from './StatCard';
import { renderUi } from './testUtils';

describe('StatCard', () => {
  it('renders the overline label, title value and hint', () => {
    renderUi(<StatCard label="Playtime" value="42 h" hint="Steam counter" />);
    expect(screen.getByText('Playtime')).toHaveClass(
      'text-[10px]',
      'uppercase',
      'tracking-overline',
      'text-text-muted',
    );
    expect(screen.getByText('42 h')).toHaveClass('text-title', 'text-text-primary');
    expect(screen.getByText('Steam counter')).toHaveClass('text-text-dimmed');
  });

  it('renders a progress bar with ARIA values instead of a hint', () => {
    renderUi(<StatCard label="Achievements" value="12 / 40" progress={0.3} hint="ignored" />);
    const bar = screen.getByRole('progressbar', { name: 'Achievements' });
    expect(bar).toHaveAttribute('aria-valuenow', '30');
    expect(bar).toHaveAttribute('aria-valuemin', '0');
    expect(bar).toHaveAttribute('aria-valuemax', '100');
    expect(screen.queryByText('ignored')).not.toBeInTheDocument();
  });

  it('clamps the progress ratio', () => {
    const { rerender } = renderUi(<StatCard label="Progress" value="x" progress={2} />);
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '100');
    rerender(<StatCard label="Progress" value="x" progress={-1} />);
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '0');
  });
});
