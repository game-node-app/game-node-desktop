import React, { useMemo } from 'react';
import { IconBookmarkPlus } from '@tabler/icons-react';
import { commands, createDefaultFilters, useLibraryView } from '../../api';
import { Button, FilterChip } from '../../components/ui';
import { openSaveView } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { formatI18n } from './formatters';
import { buildFilterChips } from './libraryControlLabels';

/**
 * Active-filter chips, the filtered count and Save as view. It is a standalone
 * row: the integration screen mounts it directly below `<LibraryToolbar />`;
 * the toolbar does not render it, so it is never duplicated.
 */
export const FilterChipsRow: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();
  const chips = useMemo(() => buildFilterChips(library, t), [library, t]);

  if (chips.length === 0) return null;

  const filtered = library?.rows.length ?? 0;
  const total = library?.total ?? 0;

  return (
    <div
      data-testid="library-active-filters"
      className="flex shrink-0 flex-wrap items-center gap-[8px] px-[24px] pb-[14px]"
    >
      {chips.map((chip) => (
        <span key={chip.id} data-testid={`library-filter-chip-${chip.id}`}>
          <FilterChip
            label={chip.label}
            value={chip.value}
            removeLabel={formatI18n(
              t('Library::Remove filter {}'),
              `${chip.label} ${chip.value}`.trim(),
            )}
            onRemove={() => commands.libraryInput({ setFilters: chip.remove() })}
          />
        </span>
      ))}

      <button
        type="button"
        data-testid="library-clear-filters-btn"
        onClick={() => commands.libraryInput({ setFilters: createDefaultFilters() })}
        className="rounded-control px-1 text-body font-medium text-brand transition-colors hover:text-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
      >
        {t('Library::Clear filters')}
      </button>

      <div className="ml-auto flex items-center gap-[12px]">
        <span data-testid="library-filtered-count" className="text-xs font-normal text-text-dimmed">
          {formatI18n(t('Library::Showing {} of {}'), filtered, total)}
        </span>
        <Button
          variant="ghost"
          size="sm"
          leftIcon={IconBookmarkPlus}
          data-testid="library-save-view-btn"
          onClick={openSaveView}
        >
          {t('Library::Save as view')}
        </Button>
      </div>
    </div>
  );
};
