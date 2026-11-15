import React from 'react';
import { Menu } from '@mantine/core';
import {
  IconBookmark,
  IconBookmarkPlus,
  IconPlus,
  IconStar,
  IconX,
} from '@tabler/icons-react';
import {
  commands,
  type Actions,
  type CollectionCount,
  type Row,
} from '../../../api';
import {
  Button,
  IconButton,
  MenuSurface,
  SectionLabel,
  StatCard,
  Tooltip,
} from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { formatDuration, formatI18n, formatReason, formatStatus } from '../formatters';
import { CoverImage } from '../CoverImage';
import { ENTRY_STATUSES } from './format';
import { summarizeSelection } from './multiSelection';

export interface MultiSelectionPaneProps {
  selected: string[];
  rows: Row[];
  actions: Actions | undefined;
  collections: CollectionCount[];
}

/** The detail panel with several entries selected: stacked covers, summaries and batch actions. */
export function MultiSelectionPane({
  selected,
  rows,
  actions,
  collections,
}: MultiSelectionPaneProps) {
  const { t } = useI18n();
  const summary = summarizeSelection(selected, rows);
  const count = summary.rows.length;
  const storeSummary =
    summary.stores.length <= 1
      ? summary.stores[0] ?? ''
      : formatI18n(t('Detail::{} stores'), summary.stores.length);
  const subtitle = summary.allInstalled
    ? formatI18n(t('Detail::{} · all installed'), storeSummary)
    : formatI18n(
        t('Detail::{} · {} installed'),
        storeSummary,
        summary.installedCount,
      );
  const addReady = actions?.addToGamenode === 'available';
  const addReason =
    actions && actions.addToGamenode !== 'available'
      ? formatReason(actions.addToGamenode.disabled, t)
      : '';
  const noLibrary = summary.inGameNode === 0;
  const batchLibraryReason = noLibrary ? t('Library::Add to GameNode first') : '';
  const covers = summary.rows.slice(0, 3);
  const coverOffsets = ['left-0 z-10', 'left-[37px] z-20', 'left-[74px] z-30'];

  const withReason = (
    disabled: boolean,
    reason: string,
    button: React.ReactElement,
  ): React.ReactElement => {
    if (!disabled || !reason) return button;
    return (
      <Tooltip label={reason}>
        <span className="block w-full">{button}</span>
      </Tooltip>
    );
  };

  return (
    <div className="flex flex-1 flex-col overflow-y-auto">
      <div className="flex flex-col gap-[12px] border-b border-border-subtle p-[16px]">
        <div data-testid="detail-multi-covers" className="relative h-[110px] w-[150px]">
          {covers.map((row, index) => (
            <div
              key={row.key}
              data-testid={`detail-multi-cover-${row.key}`}
              className={`absolute top-0 h-[102px] w-[76px] overflow-hidden rounded-control border-2 border-secondary bg-surface ${coverOffsets[index] ?? 'left-0'}`}
            >
              <CoverImage coverFile={row.coverFile} title={row.title} detectedName={row.detectedName} />
            </div>
          ))}
        </div>

        <div className="flex flex-col gap-[4px]">
          <h2 className="m-0 text-lg font-semibold text-text-primary">
            {formatI18n(t('Detail::{} games selected'), count)}
          </h2>
          <p data-testid="detail-multi-summary" className="m-0 text-xs text-text-dimmed">
            {subtitle}
          </p>
        </div>

        <div data-testid="detail-stats" className="flex items-stretch gap-[8px]">
          <StatCard
            label={t('Detail::Total time')}
            value={
              summary.totalSeconds == null
                ? '—'
                : formatDuration(summary.totalSeconds, t)
            }
            hint={t('Detail::Sum of stores')}
          />
          <StatCard
            label={t('Detail::On GameNode')}
            value={formatI18n(
              t('Detail::{} of {}'),
              summary.inGameNode,
              count,
            )}
            hint={
              summary.notOnGameNode > 0
                ? formatI18n(
                    t('Detail::{} not on GameNode'),
                    summary.notOnGameNode,
                  )
                : undefined
            }
          />
          <StatCard
            label={t('Library::Favorites')}
            value={formatI18n(t('Detail::{} of {}'), summary.favorites, count)}
          />
        </div>

        <div className="flex flex-col gap-[8px]">
          {withReason(
            !addReady,
            addReason,
            <Button
              variant="primary"
              fullWidth
              leftIcon={IconPlus}
              disabled={!addReady}
              data-testid="detail-multi-add"
              onClick={() => commands.libraryInput('openAdd')}
            >
              {t('Library::Add to GameNode…')}
            </Button>,
          )}

          <MenuSurface position="bottom-start" width={220} withinPortal hideDetached={false} transitionProps={{ duration: 0 }}>
            <Menu.Target>
              <Button
                variant="secondary"
                fullWidth
                leftIcon={IconBookmark}
                disabled={noLibrary}
                data-testid="detail-multi-status"
              >
                {t('Detail::Change status on GameNode')}
              </Button>
            </Menu.Target>
            <Menu.Dropdown>
              {ENTRY_STATUSES.map((status) => (
                <Menu.Item
                  key={status}
                  onClick={() =>
                    commands.libraryInput({
                      setStatus: { keys: selected, status },
                    })
                  }
                >
                  {formatStatus(status, t)}
                </Menu.Item>
              ))}
            </Menu.Dropdown>
          </MenuSurface>

          {withReason(
            summary.matched === 0,
            formatReason('noMatchedGames', t),
            <Button
              variant="secondary"
              fullWidth
              leftIcon={IconStar}
              disabled={summary.matched === 0}
              data-testid="detail-multi-favorite"
              onClick={() =>
                commands.libraryInput({
                  setFavorites: {
                    keys: selected,
                    favorite: !summary.allFavorites,
                    now: Math.floor(Date.now() / 1000),
                  },
                })
              }
            >
              {summary.allFavorites
                ? t('Detail::Unfavorite all')
                : t('Detail::Favorite all')}
            </Button>,
          )}

          <MenuSurface position="bottom-start" width={220} withinPortal hideDetached={false} transitionProps={{ duration: 0 }}>
            <Menu.Target>
              <Button
                variant="secondary"
                fullWidth
                leftIcon={IconBookmarkPlus}
                disabled={noLibrary}
                data-testid="detail-multi-collection"
              >
                {t('Detail::Save to collection')}
              </Button>
            </Menu.Target>
            <Menu.Dropdown>
              {collections.length === 0 ? (
                <Menu.Item disabled>{t('Library::No collections')}</Menu.Item>
              ) : (
                collections.map((collection) => (
                  <Menu.Item
                    key={collection.id}
                    onClick={() =>
                      commands.libraryInput({
                        addToCollection: {
                          keys: selected,
                          collectionId: collection.id,
                        },
                      })
                    }
                  >
                    {collection.name}
                  </Menu.Item>
                ))
              )}
            </Menu.Dropdown>
          </MenuSurface>

          {noLibrary && batchLibraryReason && (
            <div data-testid="detail-multi-library-reason" className="text-xs text-warning">
              {batchLibraryReason}
            </div>
          )}
        </div>
      </div>

      <div className="flex flex-1 flex-col gap-[4px] p-[16px]">
        <SectionLabel
          action={
            <button
              type="button"
              data-testid="detail-selection-clear"
              onClick={() => commands.libraryInput('clearSelection')}
              className="rounded-xs text-xs font-medium text-brand transition-colors hover:text-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
            >
              {t('Library::Clear')}
            </button>
          }
        >
          {t('Detail::Selection')}
        </SectionLabel>

        <div data-testid="detail-selection-list" className="flex flex-col">
          {summary.rows.map((row) => (
            <div
              key={row.key}
              data-testid={`detail-selection-row-${row.key}`}
              className="flex h-[41px] items-center gap-[10px] border-b border-border-subtle px-1.5 last:border-b-0"
            >
              <div className="h-[29px] w-[22px] shrink-0 overflow-hidden rounded-thumb bg-surface">
                <CoverImage coverFile={row.coverFile} title={row.title} detectedName={row.detectedName} />
              </div>
              <span className="min-w-0 flex-1 truncate text-[13px] text-text-default">
                {row.title}
              </span>
              <IconButton
                icon={IconX}
                label={formatI18n(
                  t('Detail::Remove {} from selection'),
                  row.title,
                )}
                variant="toggle"
                size="sm"
                data-testid={`detail-selection-remove-${row.key}`}
                onClick={() =>
                  commands.libraryInput({ select: { key: row.key, mode: 'toggle' } })
                }
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
