import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IconPlus, IconStar } from '@tabler/icons-react';
import { Button } from './Button';
import { renderUi } from './testUtils';

describe('Button', () => {
  it('renders each variant with its redesign surface classes', () => {
    const { rerender } = renderUi(<Button variant="primary">Primary</Button>);
    expect(screen.getByRole('button', { name: 'Primary' })).toHaveClass('bg-brand');
    expect(screen.getByRole('button', { name: 'Primary' })).toHaveClass('font-semibold');

    rerender(<Button variant="secondary">Secondary</Button>);
    expect(screen.getByRole('button', { name: 'Secondary' })).toHaveClass('bg-raised');

    rerender(<Button variant="ghost">Ghost</Button>);
    expect(screen.getByRole('button', { name: 'Ghost' })).toHaveClass('text-text-secondary');

    rerender(<Button variant="danger">Danger</Button>);
    expect(screen.getByRole('button', { name: 'Danger' })).toHaveClass('bg-danger');
  });

  it('uses the md and sm heights and full-width layout', () => {
    const { rerender } = renderUi(<Button size="md">MD</Button>);
    expect(screen.getByRole('button')).toHaveClass('h-[36px]', 'px-[16px]');

    rerender(<Button size="sm">SM</Button>);
    expect(screen.getByRole('button')).toHaveClass('h-[32px]', 'px-[12px]');

    rerender(<Button fullWidth>Wide</Button>);
    expect(screen.getByRole('button')).toHaveClass('w-full');
  });

  it('renders leading and trailing icons', () => {
    renderUi(
      <Button leftIcon={IconPlus} rightIcon={IconStar}>
        Add
      </Button>,
    );
    const button = screen.getByRole('button', { name: 'Add' });
    expect(button.querySelectorAll('svg')).toHaveLength(2);
  });

  it('is disabled and does not fire when disabled', async () => {
    const onClick = vi.fn();
    renderUi(
      <Button disabled onClick={onClick}>
        Disabled
      </Button>,
    );
    const button = screen.getByRole('button', { name: 'Disabled' });
    expect(button).toBeDisabled();
    expect(button).toHaveClass('disabled:opacity-50');
    await userEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('shows a spinner, marks busy and blocks clicks while loading', async () => {
    const onClick = vi.fn();
    renderUi(
      <Button loading leftIcon={IconPlus} onClick={onClick}>
        Saving
      </Button>,
    );
    const button = screen.getByRole('button', { name: 'Saving' });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute('aria-busy', 'true');
    expect(button.querySelector('.animate-spin')).not.toBeNull();
    await userEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });
});
