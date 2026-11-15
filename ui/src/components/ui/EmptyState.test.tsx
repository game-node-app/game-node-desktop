import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IconBooks, IconSearch } from '@tabler/icons-react';
import { EmptyState } from './EmptyState';
import { Button } from './Button';
import { renderUi } from './testUtils';

describe('EmptyState', () => {
  it('renders the 64px icon circle, title, body and actions', async () => {
    const onClick = vi.fn();
    renderUi(
      <EmptyState
        icon={IconBooks}
        title="Your library is empty"
        body="Connect a store or choose a DRM-free games folder to get started."
        size="lg"
        actions={<Button onClick={onClick}>Connect a store</Button>}
      />,
    );

    const title = screen.getByRole('heading', { name: 'Your library is empty' });
    expect(title).toHaveClass('text-lg', 'font-semibold', 'text-text-primary');
    const circle = title.previousElementSibling;
    expect(circle).toHaveClass('h-16', 'w-16', 'rounded-full', 'bg-raised');
    expect(
      screen.getByText(
        'Connect a store or choose a DRM-free games folder to get started.',
      ),
    ).toHaveClass('text-text-secondary');
    await userEvent.click(screen.getByRole('button', { name: 'Connect a store' }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('uses the 16px title for the no-results variant and no body', () => {
    renderUi(<EmptyState icon={IconSearch} title="No game matches the filters" />);
    expect(screen.getByRole('heading')).toHaveClass('text-md');
    expect(screen.getByRole('heading').parentElement?.querySelector('p')).toBeNull();
  });
});
