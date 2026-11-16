import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { IconBolt, IconTrophy } from '@tabler/icons-react';
import { InsetCard } from './InsetCard';
import { renderUi } from './testUtils';

describe('InsetCard', () => {
  it('renders the inset surface, title and description', () => {
    renderUi(
      <InsetCard
        icon={IconTrophy}
        iconClassName="text-warning"
        iconSize={24}
        title="31 of 58 unlocked"
        description="Steam achievements"
      />,
    );

    const title = screen.getByText('31 of 58 unlocked');
    expect(title).toHaveClass('text-sm', 'font-semibold', 'text-text-primary');
    expect(screen.getByText('Steam achievements')).toHaveClass(
      'text-xs',
      'text-text-dimmed',
    );

    const card = title.closest('div.flex.items-center');
    expect(card).toHaveClass(
      'gap-sm',
      'rounded-md',
      'border',
      'border-border-subtle',
      'bg-body',
      'p-sm',
    );
    const icon = card?.querySelector('svg');
    expect(icon).toHaveAttribute('width', '24');
    expect(icon).toHaveClass('text-warning');
  });

  it('renders the 40px accent icon box for the tool card', () => {
    renderUi(
      <InsetCard
        icon={IconBolt}
        iconBox
        title="OptiScaler"
        description="Upscaling FSR, XeSS and DLSS"
      />,
    );

    const icon = screen.getByText('OptiScaler').closest('div.flex.items-center')?.querySelector('svg');
    const box = icon?.parentElement;
    expect(box).toHaveClass(
      'h-[40px]',
      'w-[40px]',
      'rounded-md',
      'bg-accent-subtle',
    );
    expect(icon).toHaveClass('text-brand');
    expect(icon).toHaveAttribute('width', '20');
  });

  it('renders leading, trailing and children slots', () => {
    renderUi(
      <InsetCard
        leading={<span data-testid="leading" />}
        title="Protection on"
        trailing={<span data-testid="trailing" />}
      >
        <div data-testid="body-extra" />
      </InsetCard>,
    );

    expect(screen.getByTestId('leading')).toBeInTheDocument();
    expect(screen.getByTestId('trailing')).toBeInTheDocument();
    expect(screen.getByTestId('body-extra').parentElement).toHaveClass('mt-[6px]');
  });
});
