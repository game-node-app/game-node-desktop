import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { Button } from '../../../components/ui/Button';
import { renderUi } from '../../../components/ui/testUtils';
import { SettingsCard } from './SettingsCard';

describe('SettingsCard', () => {
  it('renders the card shell, uppercase header and children', () => {
    renderUi(
      <SettingsCard title="Automatic protection">
        <div>Row content</div>
      </SettingsCard>,
    );

    const heading = screen.getByRole('heading', {
      level: 3,
      name: 'Automatic protection',
    });
    expect(heading).toHaveClass(
      'text-caption',
      'font-semibold',
      'uppercase',
      'tracking-label',
      'text-text-muted',
    );
    expect(heading.parentElement).toHaveClass('px-md', 'pt-row', 'pb-xs');
    expect(heading.closest('.rounded-card')).toHaveClass(
      'rounded-card',
      'border',
      'border-border-subtle',
      'bg-secondary',
    );
    expect(screen.getByText('Row content')).toBeInTheDocument();
  });

  it('uses the 56px header variant with an action', () => {
    renderUi(
      <SettingsCard
        title="Download cache"
        action={<Button size="sm">Clear cache</Button>}
      />,
    );

    const heading = screen.getByRole('heading', {
      level: 3,
      name: 'Download cache',
    });
    expect(heading.parentElement).toHaveClass(
      'flex',
      'items-center',
      'gap-sm',
      'px-md',
      'py-sm',
    );
    expect(
      screen.getByRole('button', { name: 'Clear cache' }),
    ).toBeInTheDocument();
  });

  it('supports warning and danger tone borders', () => {
    const { rerender } = renderUi(
      <SettingsCard title="Steam" tone="warning">
        <div>warning</div>
      </SettingsCard>,
    );
    const warningCard = screen
      .getByRole('heading', { level: 3, name: 'Steam' })
      .closest('.rounded-card');
    expect(warningCard).toHaveClass('border-warning');
    expect(warningCard).not.toHaveClass('border-border-subtle');

    rerender(
      <SettingsCard title="Steam" tone="danger">
        <div>danger</div>
      </SettingsCard>,
    );
    const dangerCard = screen
      .getByRole('heading', { level: 3, name: 'Steam' })
      .closest('.rounded-card');
    expect(dangerCard).toHaveClass('border-danger');
  });
});
