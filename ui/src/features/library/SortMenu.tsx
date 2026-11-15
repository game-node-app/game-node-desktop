import React, { useEffect, useState } from 'react';
import { Menu } from '@mantine/core';
import {
  IconArrowsSort,
  IconChevronDown,
  IconSortAscending,
  IconSortDescending,
} from '@tabler/icons-react';
import {
  commands,
  defaultSortDirection,
  useLibraryView,
  type SortDirection,
  type SortKey,
} from '../../api';
import { KbdCombo, MenuSurface } from '../../components/ui';
import { useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { sortLabel } from './libraryControlLabels';

const triggerClass =
  'flex h-[34px] max-w-[195px] cursor-pointer items-center gap-[6px] rounded-control border border-border-subtle bg-raised px-[12px] text-body font-normal text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand';

interface SortChoice {
  key: SortKey;
  direction?: SortDirection;
}

/**
 * Sort menu. Choosing a key applies that key's natural direction; the two
 * Title entries are one key with an explicit direction. The direction item
 * keeps the menu open while it flips. Shift+S is handled by the focused
 * library container so it cannot fire twice or outside the library.
 */
export const SortMenu: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();
  const [opened, setOpened] = useState(false);
  const filtersOpen = useUiStore((state) => state.filtersOpen);

  useEffect(() => {
    if (filtersOpen) setOpened(false);
  }, [filtersOpen]);

  const sort = library?.layout.sort ?? { key: 'title' as SortKey, direction: 'ascending' as SortDirection };
  const currentLabel = sortLabel(sort.key, sort.direction, t);
  const descending = sort.direction === 'descending';

  const choose = (choice: SortChoice) => {
    commands.libraryInput({
      setSort: {
        key: choice.key,
        direction: choice.direction ?? defaultSortDirection(choice.key),
      },
    });
  };

  const isChecked = (choice: SortChoice) =>
    choice.direction ? sort.key === choice.key && sort.direction === choice.direction : sort.key === choice.key;

  const toggleDirection = () => {
    commands.libraryInput({
      setSort: {
        key: sort.key,
        direction: descending ? 'ascending' : 'descending',
      },
    });
  };

  const choices: SortChoice[] = [
    { key: 'title', direction: 'ascending' },
    { key: 'title', direction: 'descending' },
    { key: 'lastPlayed' },
    { key: 'playtime' },
    { key: 'dateAdded' },
    { key: 'releaseYear' },
    { key: 'matchStatus' },
    { key: 'provider' },
    { key: 'gameNodeStatus' },
  ];

  return (
    <MenuSurface
      opened={opened}
      onChange={setOpened}
      position="bottom-start"
      withinPortal
      hideDetached={false}
      transitionProps={{ duration: 0 }}
      classNames={{ dropdown: 'w-[260px]' }}
    >
      <Menu.Target>
        <button
          type="button"
          data-testid="library-sort-btn"
          aria-haspopup="menu"
          aria-label={`${t('Library::Sort')}: ${currentLabel}`}
          className={triggerClass}
        >
          <IconArrowsSort size={16} stroke={1.75} aria-hidden="true" className="shrink-0 text-text-secondary" />
          <span className="min-w-0 flex-1 truncate text-left">{currentLabel}</span>
          <IconChevronDown size={14} stroke={1.75} aria-hidden="true" className="shrink-0 text-text-dimmed" />
        </button>
      </Menu.Target>

      <Menu.Dropdown data-testid="library-sort-menu">
        <Menu.Label>{t('Library::Sort by')}</Menu.Label>
        {choices.map((choice) => (
          <Menu.CheckboxItem
            key={`${choice.key}:${choice.direction ?? 'natural'}`}
            checked={isChecked(choice)}
            closeMenuOnClick
            data-testid={`sort-option-${choice.key}-${choice.direction ?? 'natural'}`}
            onClick={() => choose(choice)}
          >
            {sortLabel(choice.key, choice.direction ?? defaultSortDirection(choice.key), t)}
          </Menu.CheckboxItem>
        ))}
        <Menu.Divider />
        <Menu.Item
          data-testid="sort-direction-toggle"
          closeMenuOnClick={false}
          leftSection={
            descending ? (
              <IconSortDescending size={16} stroke={2} aria-hidden="true" />
            ) : (
              <IconSortAscending size={16} stroke={2} aria-hidden="true" />
            )
          }
          rightSection={<KbdCombo keys={['Shift', 'S']} />}
          aria-pressed={descending}
          onClick={toggleDirection}
        >
          {t('Library::Descending order')}
        </Menu.Item>
      </Menu.Dropdown>
    </MenuSurface>
  );
};
