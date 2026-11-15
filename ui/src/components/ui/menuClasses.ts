import type { MenuStylesNames, PopoverStylesNames } from '@mantine/core';

export type MenuClassNames = Partial<Record<MenuStylesNames, string>>;
export type PopoverClassNames = Partial<Record<PopoverStylesNames, string>>;

/**
 * Common Mantine Menu surface from
 * `design-reference/raw/06-L4-menus-popovers.txt` and `components.md`:
 * 6px padding, 1px #333 border, 8px radius, toast shadow, 32px items and
 * 10/600 uppercase section labels.
 */
export const menuClassNames: MenuClassNames = {
  dropdown:
    'flex flex-col gap-px rounded-md border border-border-overlay bg-secondary p-[6px] text-text-default shadow-toast',
  label:
    'px-[10px] pb-[4px] pt-[6px] text-[10px] font-semibold uppercase leading-none tracking-label text-text-muted',
  item: [
    'flex min-h-[32px] w-full items-center gap-[10px] rounded-control px-[10px] py-[2px] text-[13px] font-normal text-text-default transition-colors',
    'hover:bg-raised hover:text-text-primary focus:bg-raised focus:text-text-primary',
    'data-[menu-active]:bg-raised data-[menu-active]:text-text-primary',
    'data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50',
    'data-[disabled]:hover:bg-transparent data-[disabled]:hover:text-text-default',
  ].join(' '),
  itemLabel: 'min-w-0 flex-1 truncate [[data-checked]_&]:font-medium',
  // Mantine reuses this style for both sections. Only the right section is
  // pushed to the end, so leading icons stay aligned with the label.
  itemSection:
    'flex shrink-0 items-center gap-[8px] text-text-dimmed data-[position=right]:ml-auto',
  itemIndicator: 'flex w-[16px] shrink-0 items-center justify-center text-brand',
  divider: 'my-[4px] h-px bg-border-subtle',
  chevron: 'shrink-0 text-text-dimmed',
};

/** Popover surface used by the 760×565 filter popover and catalog menus. */
export const popoverClassNames: PopoverClassNames = {
  dropdown:
    'rounded-popover border border-border-overlay bg-secondary p-0 text-text-default shadow-popover',
  arrow: 'border border-border-overlay bg-secondary',
};

/**
 * Appended to a danger `Menu.Item`'s `className`. Tailwind's `!` modifiers make
 * the danger color win over the shared item colors regardless of utility order.
 */
export const menuItemDangerClass =
  '!text-danger hover:!bg-danger-subtle hover:!text-danger data-[menu-active]:!bg-danger-subtle data-[menu-active]:!text-danger [&_svg]:!text-danger';

/** Two-line item content: label plus a dimmed reason for a disabled action. */
export const menuItemStackClass = 'flex min-w-0 flex-col gap-px';

/** Dimmed sub-label under a menu item, for example "Add to GameNode first". */
export const menuItemReasonClass =
  'text-[11px] font-normal leading-tight text-text-dimmed';

/** Kbd hint row inside a menu item's right section. */
export const menuItemShortcutClass = 'flex items-center gap-1';
