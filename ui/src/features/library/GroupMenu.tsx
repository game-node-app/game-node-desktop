import React, { useEffect, useState } from 'react';
import { Menu } from '@mantine/core';
import { IconCategory, IconChevronDown, IconMinus, IconPlus } from '@tabler/icons-react';
import { commands, useLibraryView, type GroupBy } from '../../api';
import { MenuSurface, menuItemReasonClass, menuItemStackClass } from '../../components/ui';
import { useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { formatI18n } from './formatters';
import { groupByLabel } from './libraryControlLabels';

const triggerClass =
  'flex h-[34px] max-w-[180px] cursor-pointer items-center gap-[6px] rounded-control border px-[12px] text-body font-normal transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand';

const GROUPINGS: GroupBy[] = [
  'none',
  'store',
  'gameNodeStatus',
  'matchState',
  'installation',
  'releaseDecade',
];

/**
 * Grouping menu. Every option shows the checked state the engine reports;
 * Collapse all becomes Expand all when every rendered group is collapsed and
 * is disabled with a reason while no grouping is active.
 */
export const GroupMenu: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();
  const [opened, setOpened] = useState(false);
  const filtersOpen = useUiStore((state) => state.filtersOpen);

  useEffect(() => {
    if (filtersOpen) setOpened(false);
  }, [filtersOpen]);

  const group = library?.layout.group ?? 'none';
  const groups = library?.groups ?? [];
  const allCollapsed = groups.length > 0 && groups.every((entry) => entry.collapsed);
  const label = group === 'none' ? t('Library::No grouping') : formatI18n(t('Library::Group: {}'), groupByLabel(group, t));
  const active = group !== 'none';
  const collapseLabel = allCollapsed
    ? t('Library::Expand all groups')
    : t('Library::Collapse all groups');

  const setGroup = (next: GroupBy) => {
    commands.libraryInput({ setGroup: next });
  };

  return (
    <MenuSurface
      opened={opened}
      onChange={setOpened}
      position="bottom-start"
      withinPortal
      hideDetached={false}
      transitionProps={{ duration: 0 }}
      classNames={{ dropdown: 'w-[240px]' }}
    >
      <Menu.Target>
        <button
          type="button"
          data-testid="library-group-btn"
          aria-haspopup="menu"
          aria-label={`${t('Library::Group by')}: ${label}`}
          className={`${triggerClass} ${
            active
              ? 'border-brand bg-accent-subtle text-text-primary'
              : 'border-border-subtle bg-raised text-text-default hover:bg-surface'
          }`}
        >
          <IconCategory
            size={16}
            stroke={1.75}
            aria-hidden="true"
            className={active ? 'shrink-0 text-brand' : 'shrink-0 text-text-secondary'}
          />
          <span className="min-w-0 flex-1 truncate text-left">{label}</span>
          <IconChevronDown size={14} stroke={1.75} aria-hidden="true" className="shrink-0 text-text-dimmed" />
        </button>
      </Menu.Target>

      <Menu.Dropdown data-testid="library-group-menu">
        <Menu.Label>{t('Library::Group by')}</Menu.Label>
        {GROUPINGS.map((value) => (
          <Menu.CheckboxItem
            key={value}
            checked={group === value}
            closeMenuOnClick
            data-testid={`group-option-${value}`}
            onClick={() => setGroup(value)}
          >
            {groupByLabel(value, t)}
          </Menu.CheckboxItem>
        ))}
        <Menu.Divider />
        <Menu.Item
          data-testid="group-collapse-all"
          disabled={group === 'none'}
          title={group === 'none' ? t('Library::No grouping') : undefined}
          leftSection={
            allCollapsed ? (
              <IconPlus size={16} stroke={2} aria-hidden="true" />
            ) : (
              <IconMinus size={16} stroke={2} aria-hidden="true" />
            )
          }
          onClick={() => commands.libraryInput({ setAllGroupsCollapsed: !allCollapsed })}
        >
          <span className={menuItemStackClass}>
            {collapseLabel}
            {group === 'none' ? (
              <span className={menuItemReasonClass}>{t('Library::No grouping')}</span>
            ) : null}
          </span>
        </Menu.Item>
      </Menu.Dropdown>
    </MenuSurface>
  );
};
