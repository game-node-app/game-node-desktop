import { Menu, type MenuProps, type MenuStylesNames } from '@mantine/core';
import { IconCheck } from '@tabler/icons-react';
import { menuClassNames } from './menuClasses';
import { mergeClassNames } from './cx';

export interface MenuSurfaceProps extends Omit<MenuProps, 'classNames'> {
  classNames?: Partial<Record<MenuStylesNames, string>>;
}

/**
 * Mantine Menu with the shared redesign surface applied. Call sites keep using
 * `Menu.Target`, `Menu.Dropdown`, `Menu.Item`, `Menu.CheckboxItem`, `Menu.Label`
 * and `Menu.Divider`; the wrapper only supplies the common styling and the
 * brand check icon for checked items.
 */
export function MenuSurface({ classNames, checkIcon, width, ...props }: MenuSurfaceProps) {
  // Mantine converts numeric widths to rem, and the app root keeps its 14px
  // font size; wrap the px intent so a 260px menu stays 260px.
  const menuWidth =
    typeof width === 'number' ? `calc(${width}px)` : width;

  return (
    <Menu
      {...props}
      width={menuWidth}
      classNames={mergeClassNames(menuClassNames, classNames)}
      checkIcon={
        checkIcon ?? <IconCheck size={16} stroke={2} aria-hidden="true" />
      }
    />
  );
}
