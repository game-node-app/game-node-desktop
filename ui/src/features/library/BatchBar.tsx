import React from 'react';
import { Menu } from '@mantine/core';
import { IconBookmark, IconLink, IconPlus, IconStar, IconStarFilled } from '@tabler/icons-react';
import {
  commands,
  getMatchedGameId,
  useLibraryView,
  type EntryStatus,
  type Row,
} from '../../api';
import { Button, Kbd, MenuSurface, Tooltip } from '../../components/ui';
import { useI18n } from '../../i18n';
import { ENTRY_STATUSES } from './detail/format';
import { summarizeSelection } from './detail/multiSelection';
import { formatI18n, formatReason, formatStatus } from './formatters';
import { startBatchMatch } from './batchMatchQueue';

/** Unmatched and ambiguous entries are the ones the batch Match action walks in turn. */
export function isMatchCandidate(row: Row): boolean {
  return (
    row.status === 'unmatched' ||
    row.status === 'ambiguous' ||
    (typeof row.status === 'object' && 'ambiguous' in row.status)
  );
}

interface DisabledReasonProps {
  reason: string;
  children: React.ReactElement;
}

/** Tooltip plus a screen-reader reason for a disabled action, per the redesign toolbar pattern. */
function DisabledReason({ reason, children }: DisabledReasonProps) {
  if (!reason) return children;
  return (
    <>
      <Tooltip label={reason}>
        <span className="inline-flex">{children}</span>
      </Tooltip>
      <span className="sr-only">{reason}</span>
    </>
  );
}

export const BatchBar: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();

  const selected = library?.selected ?? [];
  const rows = library?.rows ?? [];
  const summary = summarizeSelection(selected, rows);
  if (selected.length < 2) return null;

  const actions = library?.actions;
  const matchedKeys = summary.rows
    .filter((row) => getMatchedGameId(row.status) != null)
    .map((row) => row.key);
  const matchKeys = summary.rows.filter(isMatchCandidate).map((row) => row.key);
  const canChangeStatus = summary.rows.some(
    (row) => row.gamenode != null && row.gamenode.status != null,
  );
  const allFavorites = matchedKeys.length > 0 && summary.allFavorites;

  const addReason = formatReason(
    typeof actions?.addToGamenode === 'object' ? actions.addToGamenode.disabled : null,
    t,
  );
  const favoriteReason = matchedKeys.length === 0 ? formatReason('notMatched', t) : '';
  const statusReason = canChangeStatus ? '' : t('Library::Add to GameNode first');
  const matchReason = !library
    ? ''
    : library.access !== 'verified'
      ? formatReason(library.access === 'offline' ? 'offline' : 'needsVerifiedSession', t)
      : matchKeys.length === 0
        ? t('Library::Not matched')
        : '';

  const changeStatus = (status: EntryStatus) => {
    commands.libraryInput({ setStatus: { keys: selected, status } });
  };

  const toggleFavorite = () => {
    if (matchedKeys.length === 0) return;
    commands.libraryInput({
      setFavorites: {
        keys: matchedKeys,
        favorite: !allFavorites,
        now: Math.floor(Date.now() / 1000),
      },
    });
  };

  const countLabel = formatI18n(t('Library::{} games selected'), selected.length);

  return (
    <div
      role="toolbar"
      aria-label={countLabel}
      data-testid="batch-bar"
      className="absolute inset-x-0 bottom-[24px] z-20 mx-auto flex min-h-[52px] w-fit max-w-[calc(100%-32px)] flex-wrap items-center justify-center gap-[8px] rounded-popover border border-border-floating bg-raised py-[10px] pl-[16px] pr-[8px] shadow-toast"
    >
      <span
        data-testid="batch-bar-count"
        className="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-full bg-brand text-xs font-bold tabular-nums text-text-primary"
      >
        {selected.length}
      </span>
      <span className="shrink-0 whitespace-nowrap text-[13px] font-medium text-text-primary">
        {t('Library::games selected')}
      </span>

      <span aria-hidden className="mx-[4px] h-[24px] w-px shrink-0 bg-border-subtle" />

      <DisabledReason reason={addReason}>
        <Button
          variant="primary"
          size="sm"
          leftIcon={IconPlus}
          disabled={actions?.addToGamenode !== 'available'}
          data-testid="batch-bar-add"
          onClick={() => commands.libraryInput('openAdd')}
        >
          {t('Library::Add to GameNode…')}
        </Button>
      </DisabledReason>

      <MenuSurface position="top-start" offset={8} withinPortal>
        <Menu.Target>
          <span className="inline-flex">
            <DisabledReason reason={statusReason}>
              <Button
                variant="secondary"
                size="sm"
                leftIcon={IconBookmark}
                disabled={!canChangeStatus}
                data-testid="batch-bar-status"
                aria-haspopup="menu"
              >
                {t('Library::Change status')}
              </Button>
            </DisabledReason>
          </span>
        </Menu.Target>
        <Menu.Dropdown>
          {ENTRY_STATUSES.map((status) => (
            <Menu.Item
              key={status}
              data-testid={`batch-bar-status-${status}`}
              onClick={() => changeStatus(status)}
            >
              {formatStatus(status, t)}
            </Menu.Item>
          ))}
        </Menu.Dropdown>
      </MenuSurface>

      <DisabledReason reason={favoriteReason}>
        <Button
          variant="secondary"
          size="sm"
          leftIcon={allFavorites ? IconStarFilled : IconStar}
          disabled={matchedKeys.length === 0}
          data-testid="batch-bar-favorite"
          aria-pressed={allFavorites}
          onClick={toggleFavorite}
        >
          {allFavorites ? t('Library::Remove from favorites') : t('Library::Add to favorites')}
        </Button>
      </DisabledReason>

      <DisabledReason reason={matchReason}>
        <Button
          variant="secondary"
          size="sm"
          leftIcon={IconLink}
          disabled={matchReason !== ''}
          data-testid="batch-bar-match"
          onClick={() => startBatchMatch(matchKeys, selected)}
        >
          {t('Library::Match…')}
        </Button>
      </DisabledReason>

      <span aria-hidden className="mx-[4px] h-[24px] w-px shrink-0 bg-border-subtle" />

      <Button
        variant="ghost"
        size="sm"
        data-testid="batch-bar-clear"
        className="gap-[6px]"
        onClick={() => commands.libraryInput('clearSelection')}
      >
        {t('Library::Clear')}
        <Kbd>Esc</Kbd>
      </Button>
    </div>
  );
};
