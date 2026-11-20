import React from 'react';
import {
  IconAlertTriangle,
  IconArchive,
  IconBookmark,
  IconCircleCheck,
  IconCircleX,
  IconDeviceGamepad2,
  IconInfinity,
  IconLoader2,
  IconPlus,
  IconTrophy,
  IconX,
  type TablerIcon,
} from '@tabler/icons-react';
import { DialogShell } from '../../components/dialog';
import {
  Button,
  Checkbox,
  GameCover,
  IconButton,
  OptionCard,
  OptionCardGroup,
  ProgressBar,
  SectionLabel,
  TextInput,
} from '../../components/ui';
import {
  commands,
  coverAssetUrl,
  useLibraryView,
  type AddItem,
  type EntryStatus,
  type NewCollectionError,
} from '../../api';
import { useI18n } from '../../i18n';
import { formatI18n, formatReason, formatStatus } from '../library/formatters';

const DEFAULT_SIZE = { width: 520, height: 560 };
const MIN_SIZE = { width: 440, height: 420 };

const STATUS_LIST: EntryStatus[] = [
  'playing',
  'finished',
  'planned',
  'dropped',
  'ongoing',
];

const STATUS_ICONS: Record<EntryStatus, TablerIcon> = {
  playing: IconDeviceGamepad2,
  finished: IconTrophy,
  planned: IconBookmark,
  dropped: IconArchive,
  ongoing: IconInfinity,
};

function itemStatusText(item: AddItem, t: (key: string) => string): string {
  switch (item.state) {
    case 'waiting':
      return t('Library::Waiting');
    case 'adding':
      return t('Library::Adding…');
    case 'added':
      return t('Library::Added');
    case 'alreadyPresent':
      return t('Library::Already in the library');
    case 'skipped':
      return t('Library::Skipped: no GameNode match');
    case 'failed':
    default:
      return formatI18n(
        t('Library::Failed: {}'),
        item.error === 'rejected'
          ? t('Library::GameNode rejected the session')
          : item.error === 'failed'
            ? t('Library::The request could not be completed')
            : t('Library::Failed'),
      );
  }
}

function itemToneClass(item: AddItem): string {
  switch (item.state) {
    case 'added':
      return 'text-success';
    case 'skipped':
      return 'text-warning';
    case 'failed':
      return 'text-danger';
    case 'adding':
      return 'text-text-secondary';
    case 'waiting':
    case 'alreadyPresent':
    default:
      return 'text-text-dimmed';
  }
}

function itemIcon(item: AddItem): TablerIcon | null {
  switch (item.state) {
    case 'adding':
      return IconLoader2;
    case 'added':
    case 'alreadyPresent':
      return IconCircleCheck;
    case 'skipped':
      return IconAlertTriangle;
    case 'failed':
      return IconCircleX;
    case 'waiting':
    default:
      return null;
  }
}

function newCollectionErrorText(
  error: NewCollectionError,
  t: (key: string) => string,
): string {
  switch (error) {
    case 'limitReached':
      return t("Library::You reached GameNode's collection limit.");
    case 'rejected':
      return t('Library::GameNode rejected your session. Sign in again.');
    case 'offline':
      return t('Library::GameNode cannot be reached right now.');
    case 'failed':
    default:
      return t('Library::The collection could not be created.');
  }
}

export const AddDialog: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();

  const dialog = library?.addDialog;

  if (!dialog) return null;

  const running = dialog.progress != null && dialog.summary == null;
  const done = dialog.summary != null;
  const batchRunning = running && dialog.targets.length > 1;

  const handleStatusClick = (status: string) => {
    commands.libraryInput({ setAddStatus: status as EntryStatus });
  };

  const handleToggleCollection = (id: string) => {
    commands.libraryInput({ toggleAddCollection: id });
  };

  const handleRetryCollections = () => {
    commands.libraryInput('retryCollections');
  };

  const handleConfirm = () => {
    commands.libraryInput('confirmAdd');
  };

  const handleClose = () => {
    commands.libraryInput(batchRunning ? 'hideAdd' : 'closeAdd');
  };

  const handleHide = () => {
    commands.libraryInput('hideAdd');
  };

  const handleStartNewCollection = () => {
    commands.libraryInput('startNewCollection');
  };

  const handleNewCollectionName = (name: string) => {
    commands.libraryInput({ setNewCollectionName: name });
  };

  const handleCreateCollection = () => {
    commands.libraryInput('createCollection');
  };

  const handleCancelNewCollection = () => {
    commands.libraryInput('cancelNewCollection');
  };

  const collections = dialog.collections;
  const loadedCollections =
    typeof collections === 'object' && 'loaded' in collections
      ? collections.loaded
      : null;
  const pickedNames = loadedCollections
    ? loadedCollections
        .filter((collection) => collection.picked)
        .map((collection) => collection.name)
    : [];

  // The provider whose default status the note names. A catalog game has no store provider.
  const providerId = dialog.targets[0]?.entryKey?.split(':')[0] ?? null;
  const providerName =
    (providerId
      ? library?.providers.find((provider) => provider.id === providerId)?.name
      : null) ?? 'GameNode';
  const defaultStatusFrom = dialog.appliedFrom ?? providerName;

  const heading = done
    ? dialog.targets.length === 1
      ? t('Library::Add to GameNode')
      : formatI18n(
          t('Library::Adding {} games to GameNode'),
          dialog.targets.length,
        )
    : running
      ? dialog.targets.length === 1
        ? t('Library::Add to GameNode')
        : formatI18n(
            t('Library::Adding {} games to GameNode'),
            dialog.targets.length,
          )
      : dialog.editingExisting
        ? t('Library::Edit in GameNode')
        : t('Library::Add to GameNode');

  const subtitle = running || done
    ? pickedNames.length > 0
      ? formatI18n(
          t('Library::Status: {} · Collection: {}'),
          formatStatus(dialog.appliedStatus, t),
          pickedNames.join(', '),
        )
      : formatI18n(t('Library::Status: {}'), formatStatus(dialog.appliedStatus, t))
    : dialog.targets.length === 1
      ? dialog.targets[0].title
      : formatI18n(t('Library::{} games'), dialog.targets.length);

  const primaryCover = dialog.targets[0]?.coverFile ?? null;

  const footer = (() => {
    if (done) {
      return (
        <Button
          variant="secondary"
          size="sm"
          data-testid="add-dialog-close-btn"
          onClick={handleClose}
        >
          {t('Library::Close')}
        </Button>
      );
    }
    if (running) {
      if (batchRunning) {
        return (
          <Button
            variant="secondary"
            size="sm"
            data-testid="add-progress-hide-btn"
            onClick={handleHide}
          >
            {t('Library::Hide')}
          </Button>
        );
      }
      return (
        <Button
          variant="secondary"
          size="sm"
          data-testid="add-dialog-cancel-btn"
          onClick={handleClose}
        >
          {t('Library::Cancel')}
        </Button>
      );
    }
    return (
      <>
        <Button
          variant="secondary"
          size="sm"
          data-testid="add-dialog-cancel-btn"
          onClick={handleClose}
        >
          {t('Library::Cancel')}
        </Button>
        <Button
          variant="primary"
          size="sm"
          leftIcon={dialog.editingExisting ? undefined : IconPlus}
          data-testid="add-dialog-confirm-btn"
          disabled={!dialog.canConfirm}
          onClick={handleConfirm}
        >
          {dialog.editingExisting
            ? t('Library::Save')
            : t('Library::Add to GameNode')}
        </Button>
      </>
    );
  })();

  return (
    <DialogShell
      testId="add-dialog"
      geometryKey="add-dialog"
      defaultSize={DEFAULT_SIZE}
      minSize={MIN_SIZE}
      onClose={handleClose}
      title={<span data-testid="add-dialog-title">{heading}</span>}
      subtitle={<span data-testid="add-dialog-subtitle">{subtitle}</span>}
      leading={
        <span className="flex h-10 w-[30px] shrink-0 items-center justify-center overflow-hidden rounded-sm bg-surface">
          <GameCover
            data-testid="add-dialog-cover"
            src={coverAssetUrl(primaryCover)}
            title={dialog.targets[0]?.title ?? ''}
            className="rounded-sm"
          />
        </span>
      }
      bodyClassName="min-h-0"
      footerNote={
        batchRunning ? (
          <span data-testid="add-progress-note">
            {t('Library::You can close this: adding continues in the background.')}
          </span>
        ) : undefined
      }
      footer={footer}
    >
      {running || done ? (
        <div
          data-testid="add-progress-container"
          className="flex min-h-0 flex-1 flex-col gap-md"
        >
          {done && dialog.summary && (
            <div
              data-testid="add-summary-headline"
              className="text-xs text-text-secondary"
            >
              {formatI18n(
                t('Library::Saved {} · already in your library {} · skipped {} · failed {}'),
                dialog.summary.added,
                dialog.summary.alreadyPresent,
                dialog.summary.skipped,
                dialog.summary.failed,
              )}
            </div>
          )}

          <div className="flex items-center gap-[10px]">
            <ProgressBar
              value={dialog.progress?.done ?? 0}
              max={dialog.progress?.total ?? 0}
              label={formatI18n(
                t('Library::{} of {}'),
                dialog.progress?.done ?? 0,
                dialog.progress?.total ?? 0,
              )}
              className="flex-1"
            />
            <span
              data-testid="add-progress-caption"
              className="shrink-0 text-caption font-semibold text-text-primary"
            >
              {formatI18n(
                t('Library::{} of {}'),
                dialog.progress?.done ?? 0,
                dialog.progress?.total ?? 0,
              )}
            </span>
          </div>

          <div
            data-testid={done ? 'add-summary-items-list' : 'add-progress-list'}
            className="flex min-h-0 flex-1 flex-col overflow-y-auto"
          >
            {dialog.items.map((item, index) => {
              const ToneIcon = itemIcon(item);
              const key = item.entryKey ?? JSON.stringify(item.target);
              const cover = dialog.targets[index]?.coverFile ?? null;
              return (
                <div
                  key={key}
                  data-testid={`${done ? 'add-summary' : 'add-progress'}-item-${key}`}
                  className="flex items-center gap-[10px] border-b border-border-subtle px-0 py-2 last:border-b-0"
                >
                  <span className="flex h-8 w-6 shrink-0 items-center justify-center overflow-hidden rounded-sm bg-surface">
                    <GameCover src={coverAssetUrl(cover)} title={item.title} />
                  </span>
                  <div className="flex min-w-0 flex-1 flex-col gap-[2px]">
                    <span className="truncate text-body font-medium text-text-primary">
                      {item.title}
                    </span>
                    <span
                      data-testid={`${done ? 'add-summary' : 'add-progress'}-item-status-${key}`}
                      className={`truncate text-caption ${itemToneClass(item)}`}
                    >
                      {itemStatusText(item, t)}
                    </span>
                  </div>
                  {ToneIcon ? (
                    <ToneIcon
                      size={16}
                      stroke={2}
                      aria-hidden="true"
                      className={`shrink-0 ${itemToneClass(item)} ${
                        item.state === 'adding' ? 'animate-spin' : ''
                      }`}
                    />
                  ) : (
                    <span className="h-4 w-4 shrink-0" aria-hidden="true" />
                  )}
                </div>
              );
            })}
          </div>

        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-md overflow-y-auto">
          <div className="flex flex-col gap-compact">
            <SectionLabel>{t('Library::Status')}</SectionLabel>
            <OptionCardGroup
              role="radiogroup"
              orientation="horizontal"
              aria-label={t('Library::Status')}
              value={dialog.status}
              onChange={handleStatusClick}
              className="grid grid-cols-5 gap-compact"
            >
              {STATUS_LIST.map((status) => (
                <OptionCard
                  key={status}
                  data-testid={`add-status-btn-${status}`}
                  value={status}
                  icon={STATUS_ICONS[status]}
                  title={formatStatus(status, t)}
                />
              ))}
            </OptionCardGroup>
            {dialog.targets.length > 1 && (
              <div
                data-testid="add-multiple-targets"
                className="text-xs text-text-dimmed"
              >
                {formatI18n(t('Library::{} games'), dialog.targets.length)}
                {dialog.targets
                  .filter((target) => !target.matched)
                  .map((target) => (
                    <span key={target.entryKey ?? JSON.stringify(target.target)}>
                      {' · '}
                      <span
                        data-testid={`add-target-${target.entryKey ?? JSON.stringify(target.target)}`}
                      >
                        {formatI18n(
                          t('Library::{} (no GameNode match, will be skipped)'),
                          target.title,
                        )}
                      </span>
                    </span>
                  ))}
              </div>
            )}
            {dialog.targets.length === 1 && (
              <div
                data-testid="add-single-target"
                className="text-xs text-text-dimmed"
              >
                {dialog.targets[0].matched
                  ? dialog.targets[0].title
                  : formatI18n(
                      t('Library::{} (no GameNode match, will be skipped)'),
                      dialog.targets[0].title,
                    )}
              </div>
            )}
            <div
              data-testid="add-applied-from-notice"
              className="text-xs text-text-dimmed"
            >
              {formatI18n(
                t('Library::With no choice, GameNode uses {}, the default status of {}.'),
                formatStatus(dialog.appliedStatus, t),
                defaultStatusFrom,
              )}
            </div>
          </div>

          <div className="flex min-h-0 flex-col gap-compact">
            <SectionLabel
              action={
                <button
                  type="button"
                  data-testid="add-new-collection-btn"
                  onClick={handleStartNewCollection}
                  className="inline-flex items-center gap-1 text-caption font-medium text-brand hover:text-text-primary"
                >
                  <IconPlus size={14} stroke={2} aria-hidden="true" />
                  {t('Library::New collection')}
                </button>
              }
            >
              {t('Library::Collections (optional)')}
            </SectionLabel>

            {dialog.newCollection && (
              <div
                data-testid="add-new-collection-field"
                className="flex flex-col gap-1"
              >
                <div className="flex items-end gap-compact">
                  <div className="min-w-0 flex-1">
                    <TextInput
                      data-testid="add-new-collection-input"
                      aria-label={t('Library::Collection name')}
                      placeholder={t('Library::Collection name')}
                      value={dialog.newCollection.name}
                      disabled={dialog.newCollection.creating}
                      autoFocus
                      onChange={(event) =>
                        handleNewCollectionName(event.target.value)
                      }
                      onKeyDown={(event) => {
                        if (event.key === 'Enter') {
                          event.preventDefault();
                          if (dialog.newCollection?.canCreate) {
                            handleCreateCollection();
                          }
                        }
                      }}
                    />
                  </div>
                  <Button
                    variant="secondary"
                    size="md"
                    data-testid="add-new-collection-create-btn"
                    disabled={!dialog.newCollection.canCreate}
                    loading={dialog.newCollection.creating}
                    onClick={handleCreateCollection}
                  >
                    {t('Library::Create')}
                  </Button>
                  <IconButton
                    icon={IconX}
                    label={t('Library::Cancel')}
                    variant="toggle"
                    size="sm"
                    iconSize={14}
                    data-testid="add-new-collection-cancel-btn"
                    disabled={dialog.newCollection.creating}
                    onClick={handleCancelNewCollection}
                  />
                </div>
                {dialog.newCollection.error && (
                  <span
                    data-testid="add-new-collection-error"
                    className="text-xs text-warning"
                  >
                    {newCollectionErrorText(dialog.newCollection.error, t)}
                  </span>
                )}
              </div>
            )}

            {collections === 'loading' && (
              <div
                data-testid="add-collections-loading"
                className="text-xs text-text-dimmed"
              >
                {t('Library::Loading your collections…')}
              </div>
            )}

            {typeof collections === 'object' && 'failed' in collections && (
              <div
                data-testid="add-collections-failed"
                className="flex items-center justify-between gap-compact"
              >
                <span className="text-xs text-warning">
                  {collections.failed === 'rejected'
                    ? t('Library::GameNode rejected your session. Sign in again.')
                    : t('Library::Could not load your collections. Try again.')}
                </span>
                <Button
                  variant="secondary"
                  size="sm"
                  data-testid="add-collections-retry-btn"
                  onClick={handleRetryCollections}
                >
                  {t('Library::Retry')}
                </Button>
              </div>
            )}

            {loadedCollections && (
              <>
                {loadedCollections.length === 0 ? (
                  <div
                    data-testid="add-collections-empty"
                    className="text-xs text-text-dimmed"
                  >
                    {t('Library::You have no collections.')}
                  </div>
                ) : (
                  <div
                    data-testid="add-collections-list"
                    className="flex max-h-[212px] flex-col gap-[2px] overflow-y-auto rounded-md border border-border-subtle bg-body p-1"
                  >
                    {loadedCollections.map((collection) => (
                      <label
                        key={collection.id}
                        data-testid={`add-collection-item-${collection.id}`}
                        className="flex h-[34px] cursor-pointer items-center gap-[10px] rounded-xs px-2 hover:bg-raised"
                      >
                        <Checkbox
                          data-testid={`add-collection-checkbox-${collection.id}`}
                          checked={collection.picked}
                          onChange={() => handleToggleCollection(collection.id)}
                        />
                        <span
                          className={`min-w-0 flex-1 truncate text-body ${
                            collection.picked
                              ? 'text-text-primary'
                              : 'text-text-default'
                          }`}
                        >
                          {collection.name}
                        </span>
                        {collection.entryCount != null && (
                          <span
                            data-testid={`add-collection-count-${collection.id}`}
                            className="shrink-0 text-caption text-text-dimmed"
                          >
                            {formatI18n(
                              collection.entryCount === 1
                                ? t('Library::{} game')
                                : t('Library::{} games'),
                              collection.entryCount,
                            )}
                          </span>
                        )}
                      </label>
                    ))}
                  </div>
                )}
              </>
            )}
          </div>

          {dialog.error && (
            <div data-testid="add-error-notice" className="text-xs text-danger">
              {dialog.error === 'rejected' &&
                t('Library::GameNode rejected your session. Sign in again.')}
              {dialog.error === 'failed' &&
                t('Library::Could not save the game. Your choices are kept; try again.')}
            </div>
          )}

          {dialog.blocked && (
            <div
              data-testid="add-blocked-notice"
              className="text-xs text-warning"
            >
              {formatReason(dialog.blocked, t)}
            </div>
          )}
        </div>
      )}
    </DialogShell>
  );
};

export default AddDialog;
