import React, { useEffect, useRef } from 'react';
import { FloatingModal } from '../../components/FloatingModal';
import { commands, useLibraryView, coverAssetUrl, type MatchOption } from '../../api';
import { useI18n } from '../../i18n';
import { formatI18n, formatReason } from '../library/formatters';

export const MatchDialog: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();
  const inputRef = useRef<HTMLInputElement>(null);

  const dialog = library?.matchDialog;

  useEffect(() => {
    if (!dialog) return;

    // Focus query input when dialog opens
    inputRef.current?.focus();
  }, [dialog]);

  if (!dialog) return null;

  const handleSearchSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    commands.libraryInput('searchMatch');
  };

  const handlePickMatch = (gameId: number) => {
    commands.libraryInput({
      pickMatch: {
        gameId,
        now: Math.floor(Date.now() / 1000),
      },
    });
  };

  const handleMarkNoGame = () => {
    commands.libraryInput({
      markNoGame: {
        now: Math.floor(Date.now() / 1000),
      },
    });
  };

  const handleClearMatch = () => {
    commands.libraryInput('clearMatch');
  };

  const handleClose = () => {
    commands.libraryInput('closeMatch');
  };

  const renderOptionRow = (option: MatchOption, testIdPrefix: string) => {
    // Covers are served from the local cache over the asset protocol. A search result whose
    // cover is still downloading shows its title until the next snapshot arrives.
    const coverUrl = coverAssetUrl(option.coverFile);

    return (
      <div
        key={option.gameId}
        data-testid={`${testIdPrefix}-${option.gameId}`}
        onClick={() => handlePickMatch(option.gameId)}
        className={`flex cursor-pointer select-none items-center gap-sm rounded-sm border px-xs py-compact transition-colors duration-[120ms] ${
          option.current
            ? 'border-brand bg-accent-subtle'
            : 'border-transparent bg-raised'
        }`}
      >
        {/* Cover thumbnail */}
        <div className="flex h-[42px] w-[32px] shrink-0 items-center justify-center overflow-hidden rounded-xs bg-surface">
          {coverUrl ? (
            <img
              src={coverUrl}
              alt={option.name}
              loading="lazy"
              decoding="async"
              className="h-full w-full object-cover"
            />
          ) : (
            <span className="p-[2px] text-center text-[8px] text-text-dimmed">
              {option.name}
            </span>
          )}
        </div>

        {/* Title and Year */}
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium text-text-default">
            {option.name}
          </div>
          {option.releaseYear && (
            <div className="text-xs text-text-dimmed">{option.releaseYear}</div>
          )}
        </div>

        {/* Current badge */}
        {option.current && (
          <span
            data-testid="match-current-badge"
            className="shrink-0 text-xs font-semibold text-brand"
          >
            {t('Library::Current')}
          </span>
        )}

        {/* Pick Button */}
        <button
          type="button"
          data-testid={`match-pick-btn-${option.gameId}`}
          onClick={(e) => {
            e.stopPropagation();
            handlePickMatch(option.gameId);
          }}
          className="shrink-0 rounded-xs border border-border-subtle px-xs py-1 text-xs text-text-default hover:bg-surface"
        >
          {t('Library::Pick match') !== 'Library::Pick match' ? t('Library::Pick match') : 'Pick match'}
        </button>
      </div>
    );
  };

  return (
    <FloatingModal.Root
      opened
      onClose={handleClose}
      geometryKey="match-dialog"
      defaultSize={{ width: 620, height: 560 }}
      minSize={{ width: 480, height: 360 }}
      zIndex={2000}
      closeOnClickOutside
    >
      <FloatingModal.Overlay
        data-testid="match-dialog-overlay"
        backgroundOpacity={0.75}
      />
      <FloatingModal.Content
        data-testid="match-dialog"
        className="flex flex-col bg-secondary"
      >
        {/* Header */}
        <FloatingModal.Header className="px-lg pb-sm pt-lg">
          <div className="min-w-0">
            <FloatingModal.Title
              data-testid="match-dialog-title"
              className="text-lg font-semibold text-text-default"
            >
              {t('Library::Match to a GameNode game')}
            </FloatingModal.Title>
            <div
              data-testid="match-dialog-subtitle"
              className="mt-1 text-xs text-text-dimmed"
            >
              {formatI18n(
                t('Library::{} from {}') !== 'Library::{} from {}'
                  ? t('Library::{} from {}')
                  : '{0} from {1}',
                dialog.detectedName,
                dialog.providerName,
              )}
            </div>
          </div>
        </FloatingModal.Header>

        {/* Search input form */}
        <form onSubmit={handleSearchSubmit} className="flex gap-compact px-lg pb-sm">
          <input
            ref={inputRef}
            type="text"
            data-testid="match-search-input"
            data-autofocus
            value={dialog.query}
            onChange={(e) =>
              commands.libraryInput({ setMatchQuery: e.target.value })
            }
            placeholder={t('Library::Search GameNode')}
            className="h-[34px] flex-1 rounded-sm border border-border-subtle bg-surface px-xs text-sm text-text-default outline-none focus:border-brand"
          />
          <button
            type="submit"
            data-testid="match-search-submit-btn"
            className="h-[34px] rounded-sm bg-brand px-md text-sm font-medium text-on-primary hover:bg-brand-4"
          >
            {t('Library::Search') !== 'Library::Search' ? t('Library::Search') : 'Search'}
          </button>
        </form>

        {/* Status / Searching / Error notices */}
        <div className="px-lg">
          {dialog.searching && (
            <div
              data-testid="match-searching-notice"
              className="mb-compact text-xs text-text-dimmed"
            >
              {t('Library::Searching…')}
            </div>
          )}

          {dialog.noResults && (
            <div
              data-testid="match-no-results-notice"
              className="mb-compact text-xs text-warning"
            >
              {t('Library::No games found. Try different words.')}
            </div>
          )}

          {dialog.error && (
            <div
              data-testid="match-error-notice"
              className="mb-compact text-xs text-danger"
            >
              {dialog.error === 'rejected' &&
                t('Library::GameNode rejected your session. Sign in again.')}
              {dialog.error === 'failed' &&
                t('Library::The search failed. Check your connection and try again.')}
              {typeof dialog.error === 'object' &&
                'blocked' in dialog.error &&
                formatReason(dialog.error.blocked, t)}
            </div>
          )}

          {dialog.searchBlocked && (
            <div
              data-testid="match-search-blocked-notice"
              className="mb-compact text-xs text-warning"
            >
              {formatReason(dialog.searchBlocked, t)}
            </div>
          )}
        </div>

        {/* Scrollable list of Candidates and Results */}
        <FloatingModal.Body
          data-testid="match-options-container"
          className="gap-sm px-lg pb-md"
        >
          {/* Candidates */}
          {dialog.candidates.length > 0 && (
            <div className="flex flex-col gap-[6px]">
              <div className="text-xs font-semibold text-text-dimmed">
                {t('Library::GameNode suggests')}
              </div>
              <div className="flex flex-col gap-1">
                {dialog.candidates.map((c) => renderOptionRow(c, 'candidate'))}
              </div>
            </div>
          )}

          {/* Results */}
          {dialog.results.length > 0 && (
            <div className="flex flex-col gap-[6px]">
              <div className="text-xs font-semibold text-text-dimmed">
                {t('Library::Search results')}
              </div>
              <div className="flex flex-col gap-1">
                {dialog.results.map((r) => renderOptionRow(r, 'result'))}
              </div>
            </div>
          )}
        </FloatingModal.Body>

        {/* Footer Actions */}
        <FloatingModal.Footer className="flex-wrap justify-between bg-chrome px-lg py-sm">
          <div className="flex gap-compact">
            {/* Not a game */}
            <button
              type="button"
              data-testid="match-not-a-game-btn"
              aria-label="Not a game"
              onClick={handleMarkNoGame}
              className="rounded-sm border border-border-subtle px-[14px] py-compact text-xs text-text-default hover:bg-raised"
            >
              {t('Library::Not on GameNode')}
            </button>

            {/* Clear match */}
            {dialog.canClear && (
              <button
                type="button"
                data-testid="match-clear-match-btn"
                aria-label="Clear match"
                onClick={handleClearMatch}
                className="rounded-sm border border-border-subtle px-[14px] py-compact text-xs text-text-default hover:bg-raised"
              >
                {t('Library::Use the automatic match')}
              </button>
            )}
          </div>

          {/* Close */}
          <button
            type="button"
            data-testid="match-close-btn"
            aria-label="Close"
            onClick={handleClose}
            className="ml-auto rounded-sm border border-border-subtle px-md py-compact text-xs text-text-default hover:bg-raised"
          >
            {t('Library::Close')}
          </button>
        </FloatingModal.Footer>

        <FloatingModal.ResizeHandle />
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
