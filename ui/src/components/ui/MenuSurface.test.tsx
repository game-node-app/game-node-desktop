import { describe, expect, it } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { Menu } from '@mantine/core';
import { MENU_SURFACE_MIDDLEWARES, MenuSurface } from './MenuSurface';
import {
  menuClassNames,
  menuItemDangerClass,
  menuItemReasonClass,
  menuItemStackClass,
  popoverClassNames,
} from './menuClasses';
import { renderUi } from './testUtils';

type SizeApply = (args: {
  availableHeight: number;
  elements: { floating: HTMLElement };
}) => void;

/** Narrows the exported constant's `size` entry to its apply callback. */
function menuSizeApply(): SizeApply {
  const size = MENU_SURFACE_MIDDLEWARES.size;
  if (!size || typeof size !== 'object' || !size.apply) {
    throw new Error('MenuSurface must configure a size middleware with apply');
  }
  return size.apply as SizeApply;
}

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

  it('scrolls long menus inside the window without squashing rows', () => {
    renderSurface();
    const dropdown = screen.getByRole('menu');
    expect(dropdown).toHaveClass(
      'max-h-[calc(100vh-16px)]',
      'overflow-y-auto',
      'overscroll-contain',
      'flex',
      'flex-col',
    );

    // A constrained flex column must scroll rather than compress its children.
    for (const className of [
      menuClassNames.item,
      menuClassNames.label,
      menuClassNames.divider,
    ]) {
      expect(className?.split(' ')).toContain('shrink-0');
    }
  });

  it('keeps the dropdown clear of the viewport edges by 8px', () => {
    expect(MENU_SURFACE_MIDDLEWARES.flip).toEqual({
      padding: 8,
      fallbackStrategy: 'bestFit',
    });
    expect(MENU_SURFACE_MIDDLEWARES.shift).toEqual({ padding: 8 });
    expect(MENU_SURFACE_MIDDLEWARES.size).toMatchObject({
      padding: { top: 8, bottom: 8 },
    });
  });

  it('applies the default height cap to the rendered dropdown', async () => {
    renderSurface();
    const dropdown = screen.getByRole('menu');

    // The default size middleware writes a height, never a max-width: a fixed
    // 260–280 px menu must keep its authored width.
    await waitFor(() => expect(dropdown.style.maxHeight).not.toBe(''));
    expect(dropdown.style.maxWidth).toBe('');
  });

  it('caps only the menu height so fixed widths are not squeezed', () => {
    const apply = menuSizeApply();
    const floating = document.createElement('div');
    floating.style.width = '260px';

    apply({ availableHeight: 240, elements: { floating } });

    expect(floating.style.maxHeight).toBe('240px');
    expect(floating.style.maxWidth).toBe('');
    expect(floating.style.width).toBe('260px');

    // A cramped viewport must not produce a negative height, and an existing
    // max-width authored by a caller must survive.
    floating.style.maxWidth = '280px';
    apply({ availableHeight: -12, elements: { floating } });

    expect(floating.style.maxHeight).toBe('0px');
    expect(floating.style.maxWidth).toBe('280px');
    expect(floating.style.width).toBe('260px');
  });

  it('lets a caller replace the default middlewares', async () => {
    const customMiddlewares = {
      flip: false,
      shift: false,
      size: {
        apply({ elements }: { elements: { floating: HTMLElement } }) {
          elements.floating.style.maxHeight = '123px';
        },
      },
    };

    renderUi(
      <MenuSurface
        opened
        onClose={() => undefined}
        middlewares={customMiddlewares}
        withinPortal={false}
        transitionProps={{ duration: 0 }}
      >
        <Menu.Target>
          <button type="button">Open menu</button>
        </Menu.Target>
        <Menu.Dropdown>
          <Menu.Item>Item</Menu.Item>
        </Menu.Dropdown>
      </MenuSurface>,
    );

    const dropdown = screen.getByRole('menu');
    // Only the caller's size middleware runs, so its sentinel height proves
    // the default middlewares were replaced rather than merged in.
    await waitFor(() => expect(dropdown.style.maxHeight).toBe('123px'));
    expect(dropdown.style.maxWidth).toBe('');
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
