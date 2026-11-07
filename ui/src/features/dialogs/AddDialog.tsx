import React from 'react';
import { FloatingModal } from '../../components/FloatingModal';
import { commands, useLibraryView, type AddOutcome, type EntryStatus } from '../../api';
import { useI18n } from '../../i18n';
import { formatI18n, formatReason, formatStatus } from '../library/formatters';

const STATUS_LIST: EntryStatus[] = [
  'playing',
  'finished',
  'planned',
  'dropped',
  'ongoing',
];

export const AddDialog: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();

  const dialog = library?.addDialog;

  if (!dialog) return null;

  const handleStatusClick = (status: EntryStatus) => {
    commands.libraryInput({ setAddStatus: status });
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
    commands.libraryInput('closeAdd');
  };

  // Rust serializes AddSummary as items only, with externally tagged outcomes.
  const counts = { added: 0, alreadyPresent: 0, skipped: 0, failed: 0 };
  for (const { outcome } of dialog.summary?.items ?? []) {
    if (outcome === 'skippedUnmatched') counts.skipped++;
    else if ('added' in outcome) counts.added++;
    else if ('alreadyPresent' in outcome) counts.alreadyPresent++;
    else counts.failed++;
  }

  const renderOutcomeText = (outcome: AddOutcome) => {
    if (outcome === 'skippedUnmatched') {
      return t('Library::Skipped: no GameNode match');
    }
    if ('added' in outcome) {
      return formatI18n(t('Library::Saved as {}'), formatStatus(outcome.added, t));
    }
    if ('alreadyPresent' in outcome) return t('Library::Already in your library');
    return t('Library::Failed');
  };

  return (
    <FloatingModal.Root
      opened
      onClose={handleClose}
      geometryKey="add-dialog"
      defaultSize={{ width: 620, height: 600 }}
      minSize={{ width: 480, height: 360 }}
      zIndex={2000}
      closeOnClickOutside
    >
      <FloatingModal.Overlay data-testid="add-dialog-overlay" backgroundOpacity={0.75} />
      <FloatingModal.Content
        data-testid="add-dialog"
        className="flex flex-col bg-secondary"
      >
        {/* Header */}
        <FloatingModal.Header className="px-lg pb-sm pt-lg">
          <FloatingModal.Title
            data-testid="add-dialog-title"
            className="text-lg font-semibold text-text-default"
          >
            {dialog.editingExisting
              ? t('Library::Edit in GameNode')
              : t('Library::Add to GameNode')}
          </FloatingModal.Title>
        </FloatingModal.Header>

        {/* Dialog Body */}
        <FloatingModal.Body className="gap-md px-lg pb-lg">
          {/* Summary Mode */}
          {dialog.summary ? (
            <div
              data-testid="add-dialog-summary"
              className="flex flex-col gap-[14px]"
            >
              <div
                data-testid="add-summary-headline"
                className="text-sm text-text-default"
              >
                {formatI18n(
                  t('Library::Saved {} · already in your library {} · skipped {} · failed {}'),
                  counts.added,
                  counts.alreadyPresent,
                  counts.skipped,
                  counts.failed,
                )}
              </div>

              {/* Items List */}
              <div
                data-testid="add-summary-items-list"
                className="flex max-h-[260px] flex-col gap-[6px] overflow-y-auto rounded-sm bg-raised p-xs"
              >
                {dialog.summary.items.map((item) => (
                  <div
                    key={item.entryKey}
                    data-testid={`add-summary-item-${item.entryKey}`}
                    className="flex flex-col border-b border-border-subtle py-1"
                  >
                    <span className="text-sm font-medium text-text-default">
                      {item.title}
                    </span>
                    <span className="mt-0.5 text-xs text-text-dimmed">
                      {renderOutcomeText(item.outcome)}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          ) : (
            /* Editing / Choosing Mode */
            <>
              {/* Targets list */}
              {dialog.targets.length === 1 && (
                <div
                  data-testid="add-single-target"
                  className="text-md font-medium text-text-default"
                >
                  {dialog.targets[0].title}
                </div>
              )}

              {dialog.targets.length > 1 && (
                <div
                  data-testid="add-multiple-targets"
                  className="flex flex-col gap-[6px]"
                >
                  <div className="text-xs text-text-dimmed">
                    {formatI18n(t('Library::{} games'), dialog.targets.length)}
                  </div>
                  <div className="flex max-h-[110px] flex-col gap-1 overflow-y-auto rounded-sm bg-raised p-compact">
                    {dialog.targets.map((tgt) => (
                      <div
                        key={tgt.entryKey}
                        data-testid={`add-target-${tgt.entryKey}`}
                        className={`text-xs ${
                          tgt.matched ? 'text-text-default' : 'text-text-dimmed'
                        }`}
                      >
                        {tgt.matched
                          ? tgt.title
                          : formatI18n(
                              t('Library::{} (no GameNode match, will be skipped)'),
                              tgt.title,
                            )}
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Status Picker */}
              <div className="flex flex-col gap-[6px]">
                <span className="text-xs text-text-dimmed">
                  {t('Library::Status') !== 'Library::Status' ? t('Library::Status') : 'Status'}
                </span>
                <div className="flex flex-wrap gap-[6px]">
                  {STATUS_LIST.map((st) => {
                    const isSelected = dialog.status === st;
                    return (
                      <button
                        key={st}
                        type="button"
                        data-testid={`add-status-btn-${st}`}
                        onClick={() => handleStatusClick(st)}
                        className={`rounded-sm border px-sm py-[6px] text-xs ${
                          isSelected
                            ? 'border-brand bg-brand text-on-primary'
                            : 'border-border-subtle bg-surface text-text-default hover:bg-raised'
                        }`}
                      >
                        {formatStatus(st, t)}
                      </button>
                    );
                  })}
                </div>

                {/* Applied status override notice */}
                {dialog.appliedFrom && (
                  <div
                    data-testid="add-applied-from-notice"
                    className="mt-1 text-xs text-warning"
                  >
                    {formatI18n(
                      t('Library::GameNode will use {}, the default status of {}.'),
                      formatStatus(dialog.appliedStatus, t),
                      dialog.appliedFrom,
                    )}
                  </div>
                )}
              </div>

              {/* Collections Checklist */}
              <div className="flex flex-col gap-[6px]">
                <span className="text-xs text-text-dimmed">
                  {t('Library::Collections (optional)')}
                </span>

                {dialog.collections === 'loading' && (
                  <div
                    data-testid="add-collections-loading"
                    className="text-xs text-text-dimmed"
                  >
                    {t('Library::Loading your collections…')}
                  </div>
                )}

                {typeof dialog.collections === 'object' &&
                  'failed' in dialog.collections && (
                    <div
                      data-testid="add-collections-failed"
                      className="flex items-center justify-between gap-compact"
                    >
                      <span className="text-xs text-warning">
                        {t('Library::Could not load your collections. Try again.')}
                      </span>
                      <button
                        type="button"
                        data-testid="add-collections-retry-btn"
                        onClick={handleRetryCollections}
                        className="rounded-xs border border-border-subtle px-compact py-1 text-xs text-text-default hover:bg-raised"
                      >
                        {t('Library::Retry') !== 'Library::Retry' ? t('Library::Retry') : 'Retry'}
                      </button>
                    </div>
                  )}

                {typeof dialog.collections === 'object' &&
                  'loaded' in dialog.collections && (
                    <>
                      {dialog.collections.loaded.length === 0 ? (
                        <div
                          data-testid="add-collections-empty"
                          className="text-xs text-text-dimmed"
                        >
                          {t('Library::You have no collections.')}
                        </div>
                      ) : (
                        <div
                          data-testid="add-collections-list"
                          className="flex max-h-[160px] flex-col gap-1 overflow-y-auto rounded-sm bg-raised p-[6px]"
                        >
                          {dialog.collections.loaded.map((col) => (
                            <label
                              key={col.id}
                              data-testid={`add-collection-item-${col.id}`}
                              className={`flex cursor-pointer items-center gap-compact rounded-xs px-compact py-[6px] ${
                                col.picked ? 'bg-accent-subtle' : 'bg-transparent'
                              }`}
                            >
                              <input
                                type="checkbox"
                                data-testid={`add-collection-checkbox-${col.id}`}
                                checked={col.picked}
                                onChange={() => handleToggleCollection(col.id)}
                              />
                              <div className="flex flex-col">
                                <span className="text-sm text-text-default">
                                  {col.name}
                                </span>
                                {col.defaultStatus && (
                                  <span className="text-[11px] text-text-dimmed">
                                    {formatI18n(
                                      t('Library::Sets the status to {}'),
                                      formatStatus(col.defaultStatus, t),
                                    )}
                                  </span>
                                )}
                              </div>
                            </label>
                          ))}
                        </div>
                      )}
                    </>
                  )}
              </div>

              {/* Progress bar */}
              {dialog.progress && (
                <div
                  data-testid="add-progress-container"
                  className="flex flex-col gap-1"
                >
                  <div
                    data-testid="add-progress-caption"
                    className="text-xs text-text-dimmed"
                  >
                    {formatI18n(
                      t('Library::Saving {} of {}…'),
                      Math.min(dialog.progress.done + 1, dialog.progress.total),
                      dialog.progress.total,
                    )}
                  </div>
                  <div className="h-[6px] w-full overflow-hidden rounded-full bg-surface">
                    <div
                      data-testid="add-progress-bar-fill"
                      className="h-full bg-brand transition-[width] duration-200 ease-out"
                      style={{
                        width: `${Math.round(
                          ((dialog.progress.done + 1) / dialog.progress.total) * 100,
                        )}%`,
                      }}
                    />
                  </div>
                </div>
              )}

              {/* Error & Blocked notes */}
              {dialog.error && (
                <div
                  data-testid="add-error-notice"
                  className="text-xs text-danger"
                >
                  {dialog.error === 'unauthorized' &&
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
            </>
          )}
        </FloatingModal.Body>

        {/* Footer Actions */}
        <FloatingModal.Footer className="bg-chrome px-lg py-sm">
          {dialog.summary ? (
            <button
              type="button"
              data-testid="add-dialog-close-btn"
              aria-label="Close"
              onClick={handleClose}
              className="rounded-sm bg-brand px-md py-compact text-xs font-medium text-on-primary hover:bg-brand-4"
            >
              {t('Library::Close')}
            </button>
          ) : (
            <>
              <button
                type="button"
                data-testid="add-dialog-cancel-btn"
                aria-label="Cancel"
                onClick={handleClose}
                className="rounded-sm border border-border-subtle px-md py-compact text-xs text-text-default hover:bg-raised"
              >
                {t('Library::Cancel') !== 'Library::Cancel' ? t('Library::Cancel') : 'Cancel'}
              </button>

              <button
                type="button"
                data-testid="add-dialog-confirm-btn"
                aria-label="Confirm"
                disabled={!dialog.canConfirm || dialog.progress != null}
                onClick={handleConfirm}
                className="rounded-sm bg-brand px-md py-compact text-xs font-medium text-on-primary hover:bg-brand-4 disabled:cursor-default disabled:opacity-50"
              >
                {dialog.editingExisting
                  ? t('Library::Save') !== 'Library::Save'
                    ? t('Library::Save')
                    : 'Save'
                  : t('Library::Add')}
              </button>
            </>
          )}
        </FloatingModal.Footer>

        <FloatingModal.ResizeHandle />
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
