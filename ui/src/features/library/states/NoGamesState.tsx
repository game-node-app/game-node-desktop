import { IconBooks, IconRefresh } from '@tabler/icons-react';
import { Button, EmptyState } from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { formatI18n } from '../formatters';

export interface NoGamesStateProps {
  /** Store display names the finished scan looked at, from `EmptyState.noGames`. */
  stores: string[];
  onRescan: () => void;
}

/**
 * Empty library after a finished scan. The first-run checklist covers the
 * unconfigured case; this state is the connected provider whose account owns
 * and shares no games, keeping the existing "No games found" copy and Rescan.
 */
export function NoGamesState({ stores, onRescan }: NoGamesStateProps) {
  const { t } = useI18n();
  const lookedIn = stores.filter((store) => store.trim().length > 0).join(', ');

  return (
    <div
      data-testid="library-empty-state"
      className="flex flex-1 items-center justify-center p-[24px]"
    >
      <EmptyState
        icon={IconBooks}
        title={t('Library::No games found.')}
        body={
          lookedIn
            ? formatI18n(t('Library::Looked in: {}'), lookedIn)
            : undefined
        }
        actions={
          <Button
            variant="secondary"
            size="sm"
            leftIcon={IconRefresh}
            data-testid="library-empty-rescan-btn"
            onClick={onRescan}
          >
            {t('Library::Rescan')}
          </Button>
        }
      />
    </div>
  );
}
