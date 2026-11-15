import React, { useContext, useMemo, useRef } from 'react';
import { MantineContext, MantineProvider } from '@mantine/core';
import {
  IconAdjustmentsHorizontal,
  IconLayoutGrid,
  IconLayoutList,
  IconLayoutSidebarRight,
  IconSearch,
  IconX,
} from '@tabler/icons-react';
import {
  commands,
  useLibraryView,
  type ViewMode,
} from '../../api';
import {
  IconButton,
  Kbd,
  SegmentedControl,
} from '../../components/ui';
import { appTheme } from '../../AppProviders';
import { useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { buildFilterChips } from './libraryControlLabels';
import { FilterPopover } from './FilterPopover';
import { SortMenu } from './SortMenu';
import { GroupMenu } from './GroupMenu';

/**
 * Library toolbar: search, the filter popover trigger, sort and group menus,
 * the grid/list segmented control and the details toggle. The active-filter
 * chips and banners deliberately live outside this component (chips in
 * `FilterChipsRow`, banners in group 17) so they are mounted exactly once.
 */
export const LibraryToolbar: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();
  const filtersOpen = useUiStore((state) => state.filtersOpen);
  const searchRef = useRef<HTMLInputElement>(null);
  const mantineContext = useContext(MantineContext);

  const search = library?.search ?? '';
  const view = library?.layout.view ?? 'grid';
  const detailOpen = library?.layout.detailOpen ?? false;
  const chips = useMemo(() => buildFilterChips(library, t), [library, t]);
  const filtersActive = chips.length > 0;

  const changeSearch = (event: React.ChangeEvent<HTMLInputElement>) => {
    commands.libraryInput({ setSearch: event.target.value });
  };

  const clearSearch = () => {
    commands.libraryInput({ setSearch: '' });
    searchRef.current?.focus();
  };

  const filtersButton = (
    <button
      type="button"
      data-testid="library-filters-btn"
      aria-label={t('Library::Filters')}
      aria-haspopup="dialog"
      aria-expanded={filtersOpen}
      className={`flex h-[34px] shrink-0 cursor-pointer items-center gap-[6px] rounded-control border px-[12px] text-body transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand ${
        filtersActive
          ? 'border-brand bg-accent-subtle font-medium text-text-primary'
          : 'border-border-subtle bg-raised font-normal text-text-default hover:bg-surface'
      }`}
    >
      <IconAdjustmentsHorizontal
        size={16}
        stroke={1.75}
        aria-hidden="true"
        className={filtersActive ? 'text-brand' : 'text-text-secondary'}
      />
      <span>{t('Library::Filters')}</span>
      {filtersActive ? (
        <span
          data-testid="library-filters-count"
          className="flex h-[18px] min-w-[18px] items-center justify-center rounded-full bg-brand px-1 text-caption font-bold tabular-nums text-text-primary"
        >
          {chips.length}
        </span>
      ) : null}
    </button>
  );

  const content = (
    <div
      data-testid="library-toolbar"
      className="flex shrink-0 flex-wrap items-center gap-[8px] bg-chrome px-[24px] pb-[12px]"
    >
      <div className="relative flex h-[34px] min-w-[210px] flex-1 basis-[210px] items-center gap-[8px] rounded-control border border-border-subtle bg-surface px-[10px]">
        <IconSearch size={16} stroke={2} aria-hidden="true" className="shrink-0 text-text-dimmed" />
        <input
          ref={searchRef}
          type="text"
          data-testid="library-search-input"
          data-library-search
          aria-label={t('Library::Search your games')}
          placeholder={t('Library::Search your games')}
          value={search}
          onChange={changeSearch}
          className="min-w-0 flex-1 bg-transparent text-body font-normal text-text-default outline-none placeholder:text-text-dimmed"
        />
        {search.length > 0 ? (
          <button
            type="button"
            data-testid="library-clear-search-btn"
            aria-label={t('Library::Clear search')}
            title={t('Library::Clear search')}
            onClick={clearSearch}
            className="shrink-0 rounded-control p-[2px] text-text-dimmed transition-colors hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            <IconX size={14} stroke={2} aria-hidden="true" />
          </button>
        ) : (
          <span className="flex shrink-0 items-center gap-[4px]" aria-hidden="true">
            <Kbd>Ctrl</Kbd>
            <Kbd>F</Kbd>
          </span>
        )}
      </div>

      <FilterPopover>{filtersButton}</FilterPopover>
      <SortMenu />
      <GroupMenu />

      <SegmentedControl<ViewMode>
        aria-label={`${t('Library::Grid')} / ${t('Library::List')}`}
        data={[
          {
            value: 'grid',
            label: t('Library::Grid'),
            icon: IconLayoutGrid,
            iconOnly: true,
          },
          {
            value: 'list',
            label: t('Library::List'),
            icon: IconLayoutList,
            iconOnly: true,
          },
        ]}
        value={view}
        onChange={(next) => commands.libraryInput({ setView: next })}
      />

      <IconButton
        icon={IconLayoutSidebarRight}
        label={t('Library::Details panel')}
        shortcut={['Ctrl', 'I']}
        variant="toggle"
        size="toolbar"
        pressed={detailOpen}
        data-testid="library-toggle-details-btn"
        onClick={() => commands.libraryInput({ setDetailOpen: !detailOpen })}
      />
    </div>
  );

  // Mantine v9 primitives require a provider context. The app mounts one in
  // AppProviders; keep standalone renders (component tests) working as well.
  if (mantineContext) return content;

  return (
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      {content}
    </MantineProvider>
  );
};
