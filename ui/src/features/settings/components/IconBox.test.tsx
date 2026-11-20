import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { IconBolt, IconBrandSteam } from '@tabler/icons-react';
import { renderUi } from '../../../components/ui/testUtils';
import { IconBox } from './IconBox';

describe('IconBox', () => {
  it('renders the neutral 40px raised box with a 20px decorative icon', () => {
    renderUi(<IconBox data-testid="icon-box" icon={IconBrandSteam} />);

    const box = screen.getByTestId('icon-box');
    expect(box).toHaveClass(
      'h-[40px]',
      'w-[40px]',
      'rounded-md',
      'border',
      'border-border-subtle',
      'bg-raised',
    );
    const icon = box.querySelector('svg');
    expect(icon).toHaveAttribute('aria-hidden', 'true');
    expect(icon).toHaveAttribute('width', '20');
    expect(icon).toHaveClass('text-text-primary');
  });

  it('renders the accent tone without a border and with the brand icon', () => {
    renderUi(
      <IconBox
        data-testid="icon-box"
        icon={IconBolt}
        tone="accent"
        iconSize={22}
      />,
    );

    const box = screen.getByTestId('icon-box');
    expect(box).toHaveClass('bg-accent-subtle');
    expect(box).not.toHaveClass('border');
    const icon = box.querySelector('svg');
    expect(icon).toHaveAttribute('width', '22');
    expect(icon).toHaveClass('text-brand');
  });
});
