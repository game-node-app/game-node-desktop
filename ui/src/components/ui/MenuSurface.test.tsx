import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { Menu } from '@mantine/core';
import { MenuSurface } from './MenuSurface';
import {
  menuClassNames,
  menuItemDangerClass,
  menuItemReasonClass,
  menuItemStackClass,
  popoverClassNames,
} from './menuClasses';
import { renderUi } from './testUtils';

function renderSurface() {
  return renderUi(
    <MenuSurface
      opened
      onClose={() => undefined}
      withinPortal={false}
      transitionProps={{ duration: 0 }}
    >
      <Menu.Target>
        <button type="button">Open menu</button>
      </Menu.Target>
      <Menu.Dropdown>
        <Menu.Label>Sort by</Menu.Label>
        <Menu.CheckboxItem checked>Recently played</Menu.CheckboxItem>
        <Menu.Divider />
        <Menu.Item disabled>
          <span className={menuItemStackClass}>
            Add to collection
            <span className={menuItemReasonClass}>Add to GameNode first</span>
          </span>
        </Menu.Item>
        <Menu.Item className={menuItemDangerClass}>Remove from library…</Menu.Item>
      </Menu.Dropdown>
    </MenuSurface>,
  );
}

describe('MenuSurface', () => {
  it('keeps a numeric width in px instead of letting Mantine convert it to rem', () => {
    renderUi(
      <MenuSurface
        opened
        onClose={() => undefined}
        withinPortal={false}
        transitionProps={{ duration: 0 }}
        width={260}
      >
        <Menu.Target>
          <button type="button">Open menu</button>
        </Menu.Target>
        <Menu.Dropdown data-testid="sized-dropdown">
          <Menu.Item>Item</Menu.Item>
        </Menu.Dropdown>
      </MenuSurface>,
    );

    expect(screen.getByTestId('sized-dropdown').style.width).toBe('calc(260px)');
  });

  it('renders a checked item with aria-checked and the check indicator', () => {
    renderSurface();
    const checked = screen.getByRole('menuitemcheckbox', {
      name: 'Recently played',
    });
    expect(checked).toHaveAttribute('aria-checked', 'true');
    expect(checked).toHaveClass('rounded-control', 'min-h-[32px]');
    expect(checked.querySelector('.text-brand')).not.toBeNull();
    expect(checked.querySelector('[data-menu-item-label]')).toHaveClass(
      '[[data-checked]_&]:font-medium',
    );
  });

  it('renders a disabled item with its reason sub-label', () => {
    renderSurface();
    const disabled = screen.getByRole('menuitem', { name: /Add to collection/ });
    expect(disabled).toHaveAttribute('aria-disabled', 'true');
    expect(disabled).toBeDisabled();
    expect(screen.getByText('Add to GameNode first')).toHaveClass(
      'text-[11px]',
      'text-text-dimmed',
    );
  });

  it('renders a danger item with the danger color', () => {
    renderSurface();
    const danger = screen.getByRole('menuitem', { name: 'Remove from library…' });
    expect(danger).toHaveClass('!text-danger');
  });

  it('renders the section label with the overline treatment', () => {
    renderSurface();
    expect(screen.getByText('Sort by')).toHaveClass(
      'text-[10px]',
      'font-semibold',
      'uppercase',
      'tracking-label',
      'text-text-muted',
    );
  });

  it('exports Mantine class presets for menu and popover surfaces', () => {
    expect(menuClassNames.dropdown).toContain('bg-secondary');
    expect(menuClassNames.dropdown).toContain('rounded-md');
    expect(menuClassNames.dropdown).toContain('shadow-toast');
    expect(menuClassNames.item).toContain('min-h-[32px]');
    // Mantine reuses `itemSection` for both slots; only the right one is
    // pushed to the end so leading icons stay aligned with the label.
    expect(menuClassNames.itemSection).toContain('data-[position=right]:ml-auto');
    expect(menuClassNames.itemSection?.split(' ')).not.toContain('ml-auto');
    expect(popoverClassNames.dropdown).toContain('rounded-popover');
    expect(popoverClassNames.dropdown).toContain('shadow-popover');
  });

  it('forwards caller class names alongside the preset', () => {
    renderUi(
      <MenuSurface
        opened
        onClose={() => undefined}
        withinPortal={false}
        transitionProps={{ duration: 0 }}
        classNames={{ item: 'custom-item' }}
      >
        <Menu.Target>
          <button type="button">Open menu</button>
        </Menu.Target>
        <Menu.Dropdown>
          <Menu.Item>Item</Menu.Item>
        </Menu.Dropdown>
      </MenuSurface>,
    );
    expect(screen.getByRole('menuitem', { name: 'Item' })).toHaveClass(
      'custom-item',
      'min-h-[32px]',
    );
  });
});
