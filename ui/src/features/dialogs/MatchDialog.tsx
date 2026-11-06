import React, { useEffect, useRef } from 'react';
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

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        commands.libraryInput('closeMatch');
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
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
    const coverUrl = coverAssetUrl(option.coverFile) || option.coverUrl;

    return (
      <div
        key={option.gameId}
        data-testid={`${testIdPrefix}-${option.gameId}`}
        onClick={() => handlePickMatch(option.gameId)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: '12px',
          padding: '8px 10px',
          backgroundColor: option.current
            ? 'var(--color-accent-subtle, #F150251F)'
            : 'var(--color-raised, #222222)',
          border: option.current
            ? '1px solid var(--color-primary, #F15025)'
            : '1px solid transparent',
          borderRadius: 'var(--radius-sm, 4px)',
          cursor: 'pointer',
          userSelect: 'none',
          transition: 'background-color 120ms ease',
        }}
      >
        {/* Cover thumbnail */}
        <div
          style={{
            width: '32px',
            height: '42px',
            borderRadius: 'var(--radius-xs, 2px)',
            overflow: 'hidden',
            backgroundColor: 'var(--color-surface, #262525)',
            flexShrink: 0,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          {coverUrl ? (
            <img
              src={coverUrl}
              alt={option.name}
              loading="lazy"
              decoding="async"
              style={{ width: '100%', height: '100%', objectFit: 'cover' }}
            />
          ) : (
            <span
              style={{
                fontSize: '8px',
                color: 'var(--color-text-dimmed, #808080)',
                padding: '2px',
                textAlign: 'center',
              }}
            >
              {option.name}
            </span>
          )}
        </div>

        {/* Title and Year */}
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            style={{
              color: 'var(--color-text-default, #D9D9D9)',
              fontWeight: 500,
              fontSize: 'var(--font-size-sm, 14px)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {option.name}
          </div>
          {option.releaseYear && (
            <div
              style={{
                color: 'var(--color-text-dimmed, #808080)',
                fontSize: 'var(--font-size-xs, 12px)',
              }}
            >
              {option.releaseYear}
            </div>
          )}
        </div>

        {/* Current badge */}
        {option.current && (
          <span
            data-testid="match-current-badge"
            style={{
              color: 'var(--color-primary, #F15025)',
              fontSize: 'var(--font-size-xs, 12px)',
              fontWeight: 600,
              flexShrink: 0,
            }}
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
          style={{
            padding: '4px 10px',
            fontSize: 'var(--font-size-xs, 12px)',
            backgroundColor: 'transparent',
            border: '1px solid var(--color-border-subtle, #2A2A2A)',
            borderRadius: 'var(--radius-xs, 2px)',
            color: 'var(--color-text-default, #D9D9D9)',
            cursor: 'pointer',
            flexShrink: 0,
          }}
        >
          {t('Library::Pick match') !== 'Library::Pick match' ? t('Library::Pick match') : 'Pick match'}
        </button>
      </div>
    );
  };

  return (
    <div
      data-testid="match-dialog-overlay"
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
        aria-labelledby="match-dialog-title"
        data-testid="match-dialog"
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
            id="match-dialog-title"
            data-testid="match-dialog-title"
            style={{
              fontSize: 'var(--font-size-lg, 18px)',
              fontWeight: 600,
              color: 'var(--color-text-default, #D9D9D9)',
            }}
          >
            {t('Library::Match to a GameNode game')}
          </h2>
          <div
            data-testid="match-dialog-subtitle"
            style={{
              fontSize: 'var(--font-size-xs, 12px)',
              color: 'var(--color-text-dimmed, #808080)',
              marginTop: '4px',
            }}
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

        {/* Search input form */}
        <form
          onSubmit={handleSearchSubmit}
          style={{
            display: 'flex',
            gap: '8px',
            padding: '0 20px 12px 20px',
          }}
        >
          <input
            ref={inputRef}
            type="text"
            data-testid="match-search-input"
            value={dialog.query}
            onChange={(e) =>
              commands.libraryInput({ setMatchQuery: e.target.value })
            }
            placeholder={t('Library::Search GameNode')}
            style={{
              flex: 1,
              height: '34px',
              padding: '0 10px',
              backgroundColor: 'var(--color-surface, #262525)',
              color: 'var(--color-text-default, #D9D9D9)',
              border: '1px solid var(--color-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              fontSize: 'var(--font-size-sm, 14px)',
              outline: 'none',
            }}
          />
          <button
            type="submit"
            data-testid="match-search-submit-btn"
            style={{
              padding: '0 16px',
              height: '34px',
              backgroundColor: 'var(--color-primary, #F15025)',
              color: 'var(--color-on-primary, #EDEDED)',
              border: 'none',
              borderRadius: 'var(--radius-sm, 4px)',
              fontSize: 'var(--font-size-sm, 14px)',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            {t('Library::Search') !== 'Library::Search' ? t('Library::Search') : 'Search'}
          </button>
        </form>

        {/* Status / Searching / Error notices */}
        <div style={{ padding: '0 20px' }}>
          {dialog.searching && (
            <div
              data-testid="match-searching-notice"
              style={{
                fontSize: 'var(--font-size-xs, 12px)',
                color: 'var(--color-text-dimmed, #808080)',
                marginBottom: '8px',
              }}
            >
              {t('Library::Searching…')}
            </div>
          )}

          {dialog.noResults && (
            <div
              data-testid="match-no-results-notice"
              style={{
                fontSize: 'var(--font-size-xs, 12px)',
                color: 'var(--color-warning, #F5A524)',
                marginBottom: '8px',
              }}
            >
              {t('Library::No games found. Try different words.')}
            </div>
          )}

          {dialog.error && (
            <div
              data-testid="match-error-notice"
              style={{
                fontSize: 'var(--font-size-xs, 12px)',
                color: 'var(--color-danger, #E5484D)',
                marginBottom: '8px',
              }}
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
              style={{
                fontSize: 'var(--font-size-xs, 12px)',
                color: 'var(--color-warning, #F5A524)',
                marginBottom: '8px',
              }}
            >
              {formatReason(dialog.searchBlocked, t)}
            </div>
          )}
        </div>

        {/* Scrollable list of Candidates and Results */}
        <div
          data-testid="match-options-container"
          style={{
            flex: 1,
            overflowY: 'auto',
            padding: '0 20px 16px 20px',
            display: 'flex',
            flexDirection: 'column',
            gap: '12px',
            minHeight: '140px',
            maxHeight: '340px',
          }}
        >
          {/* Candidates */}
          {dialog.candidates.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
              <div
                style={{
                  fontSize: 'var(--font-size-xs, 12px)',
                  fontWeight: 600,
                  color: 'var(--color-text-dimmed, #808080)',
                }}
              >
                {t('Library::GameNode suggests')}
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                {dialog.candidates.map((c) => renderOptionRow(c, 'candidate'))}
              </div>
            </div>
          )}

          {/* Results */}
          {dialog.results.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
              <div
                style={{
                  fontSize: 'var(--font-size-xs, 12px)',
                  fontWeight: 600,
                  color: 'var(--color-text-dimmed, #808080)',
                }}
              >
                {t('Library::Search results')}
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                {dialog.results.map((r) => renderOptionRow(r, 'result'))}
              </div>
            </div>
          )}
        </div>

        {/* Footer Actions */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            padding: '12px 20px',
            borderTop: '1px solid var(--color-border-subtle, #2A2A2A)',
            backgroundColor: 'var(--color-chrome, #111111)',
            gap: '8px',
            flexWrap: 'wrap',
          }}
        >
          <div style={{ display: 'flex', gap: '8px' }}>
            {/* Not a game */}
            <button
              type="button"
              data-testid="match-not-a-game-btn"
              aria-label="Not a game"
              onClick={handleMarkNoGame}
              style={{
                padding: '8px 14px',
                fontSize: 'var(--font-size-xs, 12px)',
                backgroundColor: 'transparent',
                border: '1px solid var(--color-border-subtle, #2A2A2A)',
                borderRadius: 'var(--radius-sm, 4px)',
                color: 'var(--color-text-default, #D9D9D9)',
                cursor: 'pointer',
              }}
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
                style={{
                  padding: '8px 14px',
                  fontSize: 'var(--font-size-xs, 12px)',
                  backgroundColor: 'transparent',
                  border: '1px solid var(--color-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  color: 'var(--color-text-default, #D9D9D9)',
                  cursor: 'pointer',
                }}
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
            style={{
              padding: '8px 16px',
              fontSize: 'var(--font-size-xs, 12px)',
              backgroundColor: 'transparent',
              border: '1px solid var(--color-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              color: 'var(--color-text-default, #D9D9D9)',
              cursor: 'pointer',
              marginLeft: 'auto',
            }}
          >
            {t('Library::Close')}
          </button>
        </div>
      </div>
    </div>
  );
};
