import { Menu, type MenuProps, type MenuStylesNames } from '@mantine/core';
import { IconCheck } from '@tabler/icons-react';
import { menuClassNames } from './menuClasses';
import { mergeClassNames } from './cx';

export interface MenuSurfaceProps extends Omit<MenuProps, 'classNames'> {
  classNames?: Partial<Record<MenuStylesNames, string>>;
}

export type MenuSurfaceMiddlewares = NonNullable<MenuProps['middlewares']>;

/**
 * Default Floating UI behavior for every menu surface: keep the dropdown 8 px
 * clear of the window edges, and cap only its height to the room that is left.
 *
 * The custom `size.apply` intentionally writes `max-height` alone. Mantine's
 * default size middleware also writes `max-width`, which would squeeze menus
 * that declare a fixed width (260–280 px) as soon as the trigger sits near a
 * narrow window edge. Widths stay as authored; `flip`/`shift` keep the surface
 * on screen instead. The height is clamped at zero so a cramped viewport never
 * produces a negative CSS value.
 */
export const MENU_SURFACE_MIDDLEWARES: MenuSurfaceMiddlewares = {
  // Mantine defaults to initialPlacement when neither side fits. Prefer the
  // side with more room before size constrains a tall menu for scrolling.
  flip: { padding: 8, fallbackStrategy: 'bestFit' },
  shift: { padding: 8 },
  size: {
    padding: { top: 8, bottom: 8 },
    apply({ availableHeight, elements }) {
      const maxHeight = Math.max(0, Math.floor(availableHeight));
      elements.floating.style.maxHeight = `${maxHeight}px`;
    },
  },
};

/**
 * Mantine Menu with the shared redesign surface applied. Call sites keep using
 * `Menu.Target`, `Menu.Dropdown`, `Menu.Item`, `Menu.CheckboxItem`, `Menu.Label`
 * and `Menu.Divider`; the wrapper only supplies the common styling, scrolling
 * bounds, default positioning middlewares and the brand check icon for checked
 * items. Callers with special positioning needs can pass their own
 * `middlewares`, which replaces the defaults.
 */
export function MenuSurface({
  classNames,
  checkIcon,
  middlewares,
  width,
  ...props
}: MenuSurfaceProps) {
  // Mantine converts numeric widths to rem, and the app root keeps its 14px
  // font size; wrap the px intent so a 260px menu stays 260px.
  const menuWidth =
    typeof width === 'number' ? `calc(${width}px)` : width;

  return (
    <Menu
      {...props}
      middlewares={middlewares ?? MENU_SURFACE_MIDDLEWARES}
      width={menuWidth}
      classNames={mergeClassNames(menuClassNames, classNames)}
      checkIcon={
        checkIcon ?? <IconCheck size={16} stroke={2} aria-hidden="true" />
      }
    />
  );
}
