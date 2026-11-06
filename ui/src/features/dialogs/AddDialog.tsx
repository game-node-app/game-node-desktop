import React, { useEffect } from 'react';
import { commands, useLibraryView, type EntryStatus } from '../../api';
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

  useEffect(() => {
    if (!dialog) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        commands.libraryInput('closeAdd');
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [dialog]);

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

  const renderOutcomeText = (outcome: string, itemStatus: EntryStatus) => {
    switch (outcome) {
      case 'added':
        return formatI18n(t('Library::Saved as {}'), formatStatus(itemStatus, t));
      case 'alreadyPresent':
        return t('Library::Already in your library');
      case 'skipped':
        return t('Library::Skipped: no GameNode match');
      case 'failed':
      default:
        return t('Library::Failed');
    }
  };

  return (
    <div
      data-testid="add-dialog-overlay"
      style={{
        position: 'fixed',
        inset: 0,
        backgroundColor: 'rgba(0, 0, 0, 0.75)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 2000,
        padding: '16px',
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-dialog-title"
        data-testid="add-dialog"
        style={{
          width: '620px',
          maxWidth: '100%',
          maxHeight: '90vh',
          backgroundColor: 'var(--color-secondary, #1A1A1A)',
          borderRadius: 'var(--radius-md, 8px)',
          border: '1px solid var(--color-border-subtle, #2A2A2A)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
          boxShadow: 'var(--shadow-modal, 0 32px 80px rgba(0, 0, 0, 0.55))',
        }}
      >
        {/* Header */}
        <div style={{ padding: '20px 20px 12px 20px' }}>
          <h2
            id="add-dialog-title"
            data-testid="add-dialog-title"
            style={{
              fontSize: 'var(--font-size-lg, 18px)',
              fontWeight: 600,
              color: 'var(--color-text-default, #D9D9D9)',
            }}
          >
            {dialog.editingExisting
              ? t('Library::Edit in GameNode')
              : t('Library::Add to GameNode')}
          </h2>
        </div>

        {/* Dialog Body */}
        <div
          style={{
            flex: 1,
            overflowY: 'auto',
            padding: '0 20px 20px 20px',
            display: 'flex',
            flexDirection: 'column',
            gap: '16px',
          }}
        >
          {/* Summary Mode */}
          {dialog.summary ? (
            <div
              data-testid="add-dialog-summary"
              style={{ display: 'flex', flexDirection: 'column', gap: '14px' }}
            >
              <div
                data-testid="add-summary-headline"
                style={{
                  fontSize: 'var(--font-size-sm, 14px)',
                  color: 'var(--color-text-default, #D9D9D9)',
                }}
              >
                {formatI18n(
                  t('Library::Saved {} · already in your library {} · skipped {} · failed {}'),
                  dialog.summary.added,
                  dialog.summary.alreadyPresent,
                  dialog.summary.skipped,
                  dialog.summary.failed,
                )}
              </div>

              {/* Items List */}
              <div
                data-testid="add-summary-items-list"
                style={{
                  maxHeight: '260px',
                  overflowY: 'auto',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '6px',
                  backgroundColor: 'var(--color-raised, #222222)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  padding: '10px',
                }}
              >
                {dialog.summary.items.map((item) => (
                  <div
                    key={item.entryKey}
                    data-testid={`add-summary-item-${item.entryKey}`}
                    style={{
                      display: 'flex',
                      flexDirection: 'column',
                      padding: '4px 0',
                      borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
                    }}
                  >
                    <span
                      style={{
                        fontWeight: 500,
                        color: 'var(--color-text-default, #D9D9D9)',
                        fontSize: 'var(--font-size-sm, 14px)',
                      }}
                    >
                      {item.title}
                    </span>
                    <span
                      style={{
                        fontSize: 'var(--font-size-xs, 12px)',
                        color: 'var(--color-text-dimmed, #808080)',
                        marginTop: '2px',
                      }}
                    >
                      {renderOutcomeText(item.outcome, dialog.status)}
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
                  style={{
                    fontSize: 'var(--font-size-md, 16px)',
                    fontWeight: 500,
                    color: 'var(--color-text-default, #D9D9D9)',
                  }}
                >
                  {dialog.targets[0].title}
                </div>
              )}

              {dialog.targets.length > 1 && (
                <div
                  data-testid="add-multiple-targets"
                  style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}
                >
                  <div
                    style={{
                      fontSize: 'var(--font-size-xs, 12px)',
                      color: 'var(--color-text-dimmed, #808080)',
                    }}
                  >
                    {formatI18n(t('Library::{} games'), dialog.targets.length)}
                  </div>
                  <div
                    style={{
                      maxHeight: '110px',
                      overflowY: 'auto',
                      backgroundColor: 'var(--color-raised, #222222)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      padding: '8px',
                      display: 'flex',
                      flexDirection: 'column',
                      gap: '4px',
                    }}
                  >
                    {dialog.targets.map((tgt) => (
                      <div
                        key={tgt.entryKey}
                        data-testid={`add-target-${tgt.entryKey}`}
                        style={{
                          fontSize: 'var(--font-size-xs, 12px)',
                          color: tgt.matched
                            ? 'var(--color-text-default, #D9D9D9)'
                            : 'var(--color-text-dimmed, #808080)',
                        }}
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
              <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
                <span
                  style={{
                    fontSize: 'var(--font-size-xs, 12px)',
                    color: 'var(--color-text-dimmed, #808080)',
                  }}
                >
                  {t('Library::Status') !== 'Library::Status' ? t('Library::Status') : 'Status'}
                </span>
                <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap' }}>
                  {STATUS_LIST.map((st) => {
                    const isSelected = dialog.status === st;
                    return (
                      <button
                        key={st}
                        type="button"
                        data-testid={`add-status-btn-${st}`}
                        onClick={() => handleStatusClick(st)}
                        style={{
                          padding: '6px 12px',
                          backgroundColor: isSelected
                            ? 'var(--color-primary, #F15025)'
                            : 'var(--color-surface, #262525)',
                          color: isSelected
                            ? 'var(--color-on-primary, #EDEDED)'
                            : 'var(--color-text-default, #D9D9D9)',
                          border: `1px solid ${
                            isSelected
                              ? 'var(--color-primary, #F15025)'
                              : 'var(--color-border-subtle, #2A2A2A)'
                          }`,
                          borderRadius: 'var(--radius-sm, 4px)',
                          fontSize: 'var(--font-size-xs, 12px)',
                          cursor: 'pointer',
                        }}
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
                    style={{
                      fontSize: 'var(--font-size-xs, 12px)',
                      color: 'var(--color-warning, #F5A524)',
                      marginTop: '4px',
                    }}
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
              <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
                <span
                  style={{
                    fontSize: 'var(--font-size-xs, 12px)',
                    color: 'var(--color-text-dimmed, #808080)',
                  }}
                >
                  {t('Library::Collections (optional)')}
                </span>

                {dialog.collections === 'loading' && (
                  <div
                    data-testid="add-collections-loading"
                    style={{
                      fontSize: 'var(--font-size-xs, 12px)',
                      color: 'var(--color-text-dimmed, #808080)',
                    }}
                  >
                    {t('Library::Loading your collections…')}
                  </div>
                )}

                {typeof dialog.collections === 'object' &&
                  'failed' in dialog.collections && (
                    <div
                      data-testid="add-collections-failed"
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        gap: '8px',
                      }}
                    >
                      <span
                        style={{
                          fontSize: 'var(--font-size-xs, 12px)',
                          color: 'var(--color-warning, #F5A524)',
                        }}
                      >
                        {t('Library::Could not load your collections. Try again.')}
                      </span>
                      <button
                        type="button"
                        data-testid="add-collections-retry-btn"
                        onClick={handleRetryCollections}
                        style={{
                          padding: '4px 8px',
                          fontSize: 'var(--font-size-xs, 12px)',
                          backgroundColor: 'transparent',
                          border: '1px solid var(--color-border-subtle, #2A2A2A)',
                          borderRadius: 'var(--radius-xs, 2px)',
                          color: 'var(--color-text-default, #D9D9D9)',
                          cursor: 'pointer',
                        }}
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
                          style={{
                            fontSize: 'var(--font-size-xs, 12px)',
                            color: 'var(--color-text-dimmed, #808080)',
                          }}
                        >
                          {t('Library::You have no collections.')}
                        </div>
                      ) : (
                        <div
                          data-testid="add-collections-list"
                          style={{
                            maxHeight: '160px',
                            overflowY: 'auto',
                            display: 'flex',
                            flexDirection: 'column',
                            gap: '4px',
                            backgroundColor: 'var(--color-raised, #222222)',
                            borderRadius: 'var(--radius-sm, 4px)',
                            padding: '6px',
                          }}
                        >
                          {dialog.collections.loaded.map((col) => (
                            <label
                              key={col.id}
                              data-testid={`add-collection-item-${col.id}`}
                              style={{
                                display: 'flex',
                                alignItems: 'center',
                                gap: '8px',
                                padding: '6px 8px',
                                borderRadius: 'var(--radius-xs, 2px)',
                                cursor: 'pointer',
                                backgroundColor: col.picked
                                  ? 'var(--color-accent-subtle, #F150251F)'
                                  : 'transparent',
                              }}
                            >
                              <input
                                type="checkbox"
                                data-testid={`add-collection-checkbox-${col.id}`}
                                checked={col.picked}
                                onChange={() => handleToggleCollection(col.id)}
                              />
                              <div style={{ display: 'flex', flexDirection: 'column' }}>
                                <span
                                  style={{
                                    fontSize: 'var(--font-size-sm, 14px)',
                                    color: 'var(--color-text-default, #D9D9D9)',
                                  }}
                                >
                                  {col.name}
                                </span>
                                {col.defaultStatus && (
                                  <span
                                    style={{
                                      fontSize: '11px',
                                      color: 'var(--color-text-dimmed, #808080)',
                                    }}
                                  >
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
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: '4px',
                  }}
                >
                  <div
                    data-testid="add-progress-caption"
                    style={{
                      fontSize: 'var(--font-size-xs, 12px)',
                      color: 'var(--color-text-dimmed, #808080)',
                    }}
                  >
                    {formatI18n(
                      t('Library::Saving {} of {}…'),
                      Math.min(dialog.progress.done + 1, dialog.progress.total),
                      dialog.progress.total,
                    )}
                  </div>
                  <div
                    style={{
                      width: '100%',
                      height: '6px',
                      backgroundColor: 'var(--color-surface, #262525)',
                      borderRadius: 'var(--radius-full, 9999px)',
                      overflow: 'hidden',
                    }}
                  >
                    <div
                      data-testid="add-progress-bar-fill"
                      style={{
                        height: '100%',
                        backgroundColor: 'var(--color-primary, #F15025)',
                        width: `${Math.round(
                          ((dialog.progress.done + 1) / dialog.progress.total) * 100,
                        )}%`,
                        transition: 'width 200ms ease',
                      }}
                    />
                  </div>
                </div>
              )}

              {/* Error & Blocked notes */}
              {dialog.error && (
                <div
                  data-testid="add-error-notice"
                  style={{
                    fontSize: 'var(--font-size-xs, 12px)',
                    color: 'var(--color-danger, #E5484D)',
                  }}
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
                  style={{
                    fontSize: 'var(--font-size-xs, 12px)',
                    color: 'var(--color-warning, #F5A524)',
                  }}
                >
                  {formatReason(dialog.blocked, t)}
                </div>
              )}
            </>
          )}
        </div>

        {/* Footer Actions */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'flex-end',
            padding: '12px 20px',
            borderTop: '1px solid var(--color-border-subtle, #2A2A2A)',
            backgroundColor: 'var(--color-chrome, #111111)',
            gap: '8px',
          }}
        >
          {dialog.summary ? (
            <button
              type="button"
              data-testid="add-dialog-close-btn"
              aria-label="Close"
              onClick={handleClose}
              style={{
                padding: '8px 16px',
                fontSize: 'var(--font-size-xs, 12px)',
                backgroundColor: 'var(--color-primary, #F15025)',
                color: 'var(--color-on-primary, #EDEDED)',
                border: 'none',
                borderRadius: 'var(--radius-sm, 4px)',
                cursor: 'pointer',
                fontWeight: 500,
              }}
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
                style={{
                  padding: '8px 16px',
                  fontSize: 'var(--font-size-xs, 12px)',
                  backgroundColor: 'transparent',
                  border: '1px solid var(--color-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  color: 'var(--color-text-default, #D9D9D9)',
                  cursor: 'pointer',
                }}
              >
                {t('Library::Cancel') !== 'Library::Cancel' ? t('Library::Cancel') : 'Cancel'}
              </button>

              <button
                type="button"
                data-testid="add-dialog-confirm-btn"
                aria-label="Confirm"
                disabled={!dialog.canConfirm || dialog.progress != null}
                onClick={handleConfirm}
                style={{
                  padding: '8px 16px',
                  fontSize: 'var(--font-size-xs, 12px)',
                  backgroundColor: 'var(--color-primary, #F15025)',
                  color: 'var(--color-on-primary, #EDEDED)',
                  border: 'none',
                  borderRadius: 'var(--radius-sm, 4px)',
                  cursor:
                    dialog.canConfirm && dialog.progress == null
                      ? 'pointer'
                      : 'default',
                  opacity:
                    dialog.canConfirm && dialog.progress == null ? 1 : 0.5,
                  fontWeight: 500,
                }}
              >
                {dialog.editingExisting
                  ? t('Library::Save') !== 'Library::Save'
                    ? t('Library::Save')
                    : 'Save'
                  : t('Library::Add')}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
};
