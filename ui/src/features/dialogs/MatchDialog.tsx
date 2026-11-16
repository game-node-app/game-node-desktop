import React, { useEffect, useRef } from 'react';
import {
  IconCircleX,
  IconLink,
  IconSearch,
  IconSparkles,
} from '@tabler/icons-react';
import { DialogShell } from '../../components/dialog';
import {
  Button,
  Checklist,
  Field,
  OptionCard,
  OptionCardGroup,
  Pill,
  SectionLabel,
  TextInput,
  type ChecklistItem,
} from '../../components/ui';
import {
  commands,
  coverAssetUrl,
  useAuth,
  useLibraryView,
  type MatchOption,
  type MatchReason,
} from '../../api';
import { useI18n } from '../../i18n';
import { formatI18n, formatReason } from '../library/formatters';
import { useBatchMatchDriver } from '../library/batchMatchQueue';

const DEFAULT_SIZE = { width: 820, height: 540 };
const MIN_SIZE = { width: 640, height: 420 };

function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

/** The option meta line: `<year> · <platforms>`, omitting whatever is unknown. */
function optionMeta(
  option: Pick<MatchOption, 'releaseYear' | 'platforms'>,
): string | undefined {
  const parts = [
    option.releaseYear != null ? String(option.releaseYear) : null,
    option.platforms.length > 0 ? option.platforms.join(', ') : null,
  ].filter((part): part is string => part != null && part.length > 0);
  return parts.length > 0 ? parts.join(' · ') : undefined;
}

export const MatchDialog: React.FC = () => {
  const library = useLibraryView();
  const { auth } = useAuth();
  const { t } = useI18n();
  const inputRef = useRef<HTMLInputElement>(null);
  const accountId =
    typeof auth === 'object' && auth !== null && 'signedIn' in auth
      ? auth.signedIn.user.id
      : null;
  // A batch Match walks the selection one entry at a time. The driver lives here because the
  // batch bar unmounts as soon as the selection shrinks to the current entry. The account keeps
  // a run from continuing into another signed-in account.
  useBatchMatchDriver(library, accountId);

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

  const handleSelect = (gameId: number) => {
    commands.libraryInput({ selectMatch: { gameId } });
  };

  const handleConfirmOption = (gameId: number) => {
    // Confirming an option also selects it, so Enter or a double-click on any row matches that
    // row even when the preview pointed elsewhere.
    handleSelect(gameId);
    commands.libraryInput({ confirmMatch: { now: nowSeconds() } });
  };

  const handleConfirm = () => {
    commands.libraryInput({ confirmMatch: { now: nowSeconds() } });
  };

  const handleMarkNoGame = () => {
    commands.libraryInput({
      markNoGame: {
        now: nowSeconds(),
      },
    });
  };

  const handleClearMatch = () => {
    commands.libraryInput('clearMatch');
  };

  const handleClose = () => {
    commands.libraryInput('closeMatch');
  };

  const reasonItems = (option: MatchOption): ChecklistItem[] =>
    option.reasons.map((reason: MatchReason) => {
      if (typeof reason === 'string') {
        if (reason === 'sameYear') {
          return {
            kind: 'positive',
            label: t('Library::Same release year'),
          };
        }
        if (reason === 'storeIdRecognized') {
          return {
            kind: 'positive',
            label: t('Library::Store ID recognized'),
          };
        }
        return {
          kind: 'positive',
          label: t('Detail::Chosen by you'),
        };
      }
      if ('titleSimilarity' in reason) {
        const percent = reason.titleSimilarity.percent;
        return {
          kind: percent >= 80 ? 'positive' : 'warning',
          label: formatI18n(t('Library::Title {}% similar'), percent),
        };
      }
      return {
        kind: 'warning',
        label: formatI18n(
          t('Library::Different release year ({})'),
          reason.differentYear.year,
        ),
      };
    });

  const renderOptionRow = (option: MatchOption, testIdPrefix: string) => {
    // Covers are served from the local cache over the asset protocol. A search result whose
    // cover is still downloading shows its title until the next snapshot arrives.
    const coverUrl = coverAssetUrl(option.coverFile);
    const meta = optionMeta(option);

    return (
      <OptionCard
        key={option.gameId}
        data-testid={`${testIdPrefix}-${option.gameId}`}
        value={String(option.gameId)}
        variant="row"
        title={option.name}
        meta={meta}
        onDoubleClick={() => handleConfirmOption(option.gameId)}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            handleConfirmOption(option.gameId);
          }
        }}
        leading={
          <span className="flex h-12 w-9 shrink-0 items-center justify-center overflow-hidden rounded-sm bg-surface">
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
          </span>
        }
        trailing={
          <span className="flex items-center gap-1.5">
            {option.current && (
              <span data-testid="match-current-badge">
                <Pill tone="brand">{t('Library::Current match')}</Pill>
              </span>
            )}
            <Pill tone={option.tone}>{option.confidence}%</Pill>
          </span>
        }
      />
    );
  };

  const optionCount = dialog.candidates.length + dialog.results.length;
  const listLabel =
    dialog.candidates.length > 0
      ? t('Library::Several possible matches')
      : t('Library::Search results');
  const countLabel =
    optionCount === 1
      ? formatI18n(t('Library::1 result'), optionCount)
      : formatI18n(t('Library::{} results'), optionCount);
  const selected = dialog.selected;
  const selectedCoverUrl = coverAssetUrl(selected?.coverFile);

  return (
    <DialogShell
      testId="match-dialog"
      geometryKey="match-dialog"
      defaultSize={DEFAULT_SIZE}
      minSize={MIN_SIZE}
      onClose={handleClose}
      title={
        <span data-testid="match-dialog-title">
          {t('Library::Match to a GameNode game')}
        </span>
      }
      subtitle={
        <span data-testid="match-dialog-subtitle">
          {dialog.detectedName} · {dialog.providerName}
        </span>
      }
      bodyClassName="min-h-0"
      footerLeading={
        <>
          <Button
            variant="ghost"
            size="sm"
            leftIcon={IconCircleX}
            data-testid="match-not-a-game-btn"
            onClick={handleMarkNoGame}
          >
            {t('Library::Not on GameNode')}
          </Button>
          {dialog.canClear && (
            <Button
              variant="ghost"
              size="sm"
              leftIcon={IconSparkles}
              data-testid="match-clear-match-btn"
              onClick={handleClearMatch}
            >
              {t('Library::Use the automatic match')}
            </Button>
          )}
        </>
      }
      footer={
        <>
          <Button
            variant="secondary"
            size="sm"
            data-testid="match-cancel-btn"
            onClick={handleClose}
          >
            {t('Library::Cancel')}
          </Button>
          <Button
            variant="primary"
            size="sm"
            leftIcon={IconLink}
            data-testid="match-confirm-btn"
            disabled={selected == null}
            onClick={handleConfirm}
          >
            {t('Library::Match game')}
          </Button>
        </>
      }
    >
      <div className="flex min-h-0 flex-1 flex-row gap-lg">
        <div className="flex min-h-0 min-w-0 flex-[1.25] flex-col gap-sm">
          <form onSubmit={handleSearchSubmit}>
            <Field label={t('Library::Search GameNode')}>
              <TextInput
                ref={inputRef}
                data-testid="match-search-input"
                data-autofocus
                leadingIcon={IconSearch}
                value={dialog.query}
                onChange={(e) =>
                  commands.libraryInput({ setMatchQuery: e.target.value })
                }
              />
            </Field>
          </form>

          {dialog.searching && (
            <div
              data-testid="match-searching-notice"
              className="text-xs text-text-dimmed"
            >
              {t('Library::Searching…')}
            </div>
          )}

          {dialog.noResults && (
            <div
              data-testid="match-no-results-notice"
              className="text-xs text-warning"
            >
              {t('Library::No games found. Try different words.')}
            </div>
          )}

          {dialog.error && (
            <div
              data-testid="match-error-notice"
              className="text-xs text-danger"
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
              className="text-xs text-warning"
            >
              {formatReason(dialog.searchBlocked, t)}
            </div>
          )}

          <div
            data-testid="match-options-container"
            className="flex min-h-0 flex-1 flex-col gap-1"
          >
            {optionCount > 0 && (
              <SectionLabel
                action={
                  <span className="text-caption font-normal text-text-dimmed">
                    {countLabel}
                  </span>
                }
              >
                {listLabel}
              </SectionLabel>
            )}
            <OptionCardGroup
              role="listbox"
              orientation="vertical"
              aria-label={listLabel}
              value={selected != null ? String(selected.gameId) : null}
              onChange={(value) => handleSelect(Number(value))}
              className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto"
            >
              {dialog.candidates.map((candidate) =>
                renderOptionRow(candidate, 'candidate'),
              )}
              {dialog.results.map((result) => renderOptionRow(result, 'result'))}
            </OptionCardGroup>
          </div>
        </div>

        <div
          data-testid="match-preview"
          className="flex min-h-0 flex-1 flex-col gap-sm rounded-md border border-border-subtle bg-body p-[14px]"
        >
          {selected == null ? (
            <div className="flex flex-1 items-center justify-center px-sm text-center text-xs text-text-dimmed">
              {t('Library::Select a game to see its details.')}
            </div>
          ) : (
            <>
              <div className="flex items-start gap-sm">
                <span className="flex h-28 w-[84px] shrink-0 items-center justify-center overflow-hidden rounded-control bg-surface">
                  {selectedCoverUrl ? (
                    <img
                      src={selectedCoverUrl}
                      alt={selected.name}
                      loading="lazy"
                      decoding="async"
                      className="h-full w-full object-cover"
                    />
                  ) : null}
                </span>
                <div className="flex min-w-0 flex-col gap-[6px]">
                  <div className="text-title font-semibold text-text-primary">
                    {selected.name}
                  </div>
                  {selected.releaseYear != null && (
                    <div className="text-xs text-text-dimmed">
                      {selected.releaseYear}
                    </div>
                  )}
                  {selected.platforms.length > 0 && (
                    <Pill tone="neutral">{selected.platforms.join(', ')}</Pill>
                  )}
                </div>
              </div>

              <SectionLabel>{t('Library::Why we suggest it')}</SectionLabel>
              <Checklist items={reasonItems(selected)} />
            </>
          )}
        </div>
      </div>
    </DialogShell>
  );
};
