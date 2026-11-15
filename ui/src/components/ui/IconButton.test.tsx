import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IconLayoutSidebarRight } from '@tabler/icons-react';
import { IconButton } from './IconButton';
import { renderUi } from './testUtils';

describe('IconButton', () => {
  it('requires and applies the accessible label', () => {
    renderUi(
      <IconButton
        icon={IconLayoutSidebarRight}
        label="Details panel"
        onClick={() => undefined}
      />,
    );
    expect(
      screen.getByRole('button', { name: 'Details panel' }),
    ).toHaveAttribute('aria-label', 'Details panel');
  });

  it('renders the outlined, overlay and toggle variants', () => {
    const { rerender } = renderUi(
      <IconButton icon={IconLayoutSidebarRight} label="Outlined" variant="outlined" />,
    );
    expect(screen.getByRole('button', { name: 'Outlined' })).toHaveClass(
      'bg-raised',
      'border-border-subtle',
    );

    rerender(
      <IconButton icon={IconLayoutSidebarRight} label="Overlay" variant="overlay" />,
    );
    expect(screen.getByRole('button', { name: 'Overlay' })).toHaveClass('bg-black/45');

    rerender(
      <IconButton
        icon={IconLayoutSidebarRight}
        label="Toggle"
        variant="toggle"
        pressed
      />,
    );
    const toggle = screen.getByRole('button', { name: 'Toggle' });
    expect(toggle).toHaveAttribute('aria-pressed', 'true');
    expect(toggle).toHaveClass('text-text-primary');
  });

  it('defaults toggle aria-pressed to false', () => {
    renderUi(
      <IconButton icon={IconLayoutSidebarRight} label="Toggle" variant="toggle" />,
    );
    expect(screen.getByRole('button', { name: 'Toggle' })).toHaveAttribute(
      'aria-pressed',
      'false',
    );
  });

  it('renders the shortcut inside the tooltip', async () => {
    renderUi(
      <IconButton
        icon={IconLayoutSidebarRight}
        label="Details panel"
        shortcut={['Ctrl', 'I']}
      />,
    );
    await userEvent.hover(screen.getByRole('button', { name: 'Details panel' }));
    expect(await screen.findByText('Ctrl')).toBeInTheDocument();
    expect(screen.getByText('I')).toBeInTheDocument();
  });

  it('fires onClick and respects disabled', async () => {
    const onClick = vi.fn();
    const { rerender } = renderUi(
      <IconButton icon={IconLayoutSidebarRight} label="Action" onClick={onClick} />,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Action' }));
    expect(onClick).toHaveBeenCalledTimes(1);

    rerender(
      <IconButton
        icon={IconLayoutSidebarRight}
        label="Action"
        onClick={onClick}
        disabled
      />,
    );
    const button = screen.getByRole('button', { name: 'Action' });
    expect(button).toBeDisabled();
    await userEvent.click(button);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('uses the requested size classes', () => {
    const { rerender } = renderUi(
      <IconButton icon={IconLayoutSidebarRight} label="Size" size="md" />,
    );
    expect(screen.getByRole('button')).toHaveClass('h-[36px]', 'w-[36px]');

    rerender(<IconButton icon={IconLayoutSidebarRight} label="Size" size="toolbar" />);
    expect(screen.getByRole('button')).toHaveClass('h-[34px]', 'w-[34px]');

    rerender(<IconButton icon={IconLayoutSidebarRight} label="Size" size="sm" />);
    expect(screen.getByRole('button')).toHaveClass('h-[28px]', 'w-[28px]');
  });
});
