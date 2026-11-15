import { describe, expect, it } from 'vitest';
import { Skeleton } from './Skeleton';
import { renderUi } from './testUtils';

describe('Skeleton', () => {
  it('renders a pulsing rounded block hidden from assistive technology', () => {
    const { container } = renderUi(<Skeleton className="h-4 w-32" />);
    const skeleton = container.querySelector('.animate-pulse') as HTMLElement;
    expect(skeleton).toHaveClass('bg-raised', 'rounded-control', 'h-4', 'w-32');
    expect(skeleton).toHaveAttribute('aria-hidden', 'true');
  });

  it('supports the circle shape', () => {
    const { container } = renderUi(<Skeleton shape="circle" className="h-8 w-8" />);
    expect(container.querySelector('.rounded-full')).not.toBeNull();
  });
});
