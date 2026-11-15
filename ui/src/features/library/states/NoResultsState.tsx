import { IconArrowBackUp, IconCompass, IconSearch } from '@tabler/icons-react';
import { Button, EmptyState } from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { formatI18n } from '../formatters';

export interface NoResultsStateProps {
  /** Current library search text; the GameNode action is offered only with one. */
  search: string;
  /** Number of active facets, excluding the search text. */
  activeFilterCount: number;
  onClearFilters: () => void;
  onSearchGameNode: () => void;
}

/**
 * No match from the search text or filters, per `raw/07-L5-states.txt`. The
 * title distinguishes a search term from filters, the body counts the active
 * facets, and the GameNode action prefills the catalog with the search term.
 */
export function NoResultsState({
  search,
  activeFilterCount,
  onClearFilters,
  onSearchGameNode,
}: NoResultsStateProps) {
  const { t } = useI18n();
  const term = search.trim();
  const showFilters = activeFilterCount > 0;

  return (
    <div
      data-testid="library-empty-state"
      className="flex flex-1 items-center justify-center p-[24px]"
    >
      <EmptyState
        icon={IconSearch}
        title={
          term
            ? formatI18n(t('Library::No game matches “{}”'), term)
            : t('Library::No game matches the filters')
        }
        body={
          showFilters
            ? activeFilterCount === 1
              ? t('Library::1 active filter may be hiding results.')
              : formatI18n(
                  t('Library::{} active filters may be hiding results.'),
                  activeFilterCount,
                )
            : undefined
        }
        actions={
          <>
            {showFilters ? (
              <Button
                variant="secondary"
                size="sm"
                leftIcon={IconArrowBackUp}
                data-testid="library-empty-clear-filters-btn"
                onClick={onClearFilters}
              >
                {t('Library::Clear filters')}
              </Button>
            ) : null}
            {term ? (
              <Button
                variant="ghost"
                size="sm"
                leftIcon={IconCompass}
                data-testid="library-empty-search-gamenode-btn"
                onClick={onSearchGameNode}
              >
                {t('Library::Search GameNode')}
              </Button>
            ) : null}
          </>
        }
      />
    </div>
  );
}
