import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IconBooks, IconRadar } from '@tabler/icons-react';
import { NavItem } from './NavItem';
import { renderUi } from './testUtils';

describe('NavItem', () => {
  it('marks the active item with aria-current="page" and the brand surface', () => {
    renderUi(
      <NavItem icon={IconBooks} label="All games" count={42} active />,
    );
    const item = screen.getByRole('button', { name: /All games/ });
    expect(item).toHaveAttribute('aria-current', 'page');
    expect(item).toHaveClass('bg-accent-subtle', 'text-text-primary');
  });

  it('renders the count, brand count tone and an optional trailing slot', () => {
    const { rerender } = renderUi(
      <NavItem icon={IconBooks} label="All games" count={42} />,
    );
    expect(screen.getByText('42')).toHaveClass('text-text-dimmed');

    rerender(
      <NavItem icon={IconBooks} label="All games" count={42} countTone="brand" />,
    );
    expect(screen.getByText('42')).toHaveClass('text-brand');

    rerender(
      <NavItem
        icon={IconBooks}
        label="All games"
        trailing={<span data-testid="trailing">x</span>}
      />,
    );
    expect(screen.getByTestId('trailing')).toBeInTheDocument();
  });

  it('announces the warning reason and shows the warning icon', () => {
    renderUi(
      <NavItem
        icon={IconRadar}
        label="Steam"
        warning="Steam needs attention"
        count={3}
      />,
    );
    expect(screen.getByText('Steam needs attention')).toHaveClass('sr-only');
    expect(screen.getByRole('button', { name: /Steam/ }).querySelector('svg')).not.toBeNull();
  });

  it('collapses to an icon rail with a tooltip label and count badge', async () => {
    renderUi(
      <NavItem icon={IconBooks} label="All games" count={42} collapsed />,
    );
    const item = screen.getByRole('button', { name: 'All games' });
    expect(item).toHaveClass('justify-center');
    expect(screen.queryByText('All games')).not.toBeInTheDocument();
    expect(screen.getByText('42')).toHaveClass('absolute');

    await userEvent.hover(item);
    expect(await screen.findByText('All games')).toBeInTheDocument();
  });

  it('fires onClick', async () => {
    const onClick = vi.fn();
    renderUi(<NavItem icon={IconBooks} label="All games" onClick={onClick} />);
    await userEvent.click(screen.getByRole('button', { name: /All games/ }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
