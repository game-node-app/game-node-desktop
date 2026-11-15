import { IconAlertTriangle } from '@tabler/icons-react';
import { commands, type Actions, type DetailView } from '../../../api';
import { Button, SectionLabel } from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { CoverImage } from '../CoverImage';
import { formatReason } from '../formatters';
import { SuggestionsList } from './SuggestionsList';

export interface MatchSectionProps {
  detail: DetailView;
  actions: Actions | undefined;
}

type MatchKind = 'matched' | 'ambiguous' | 'unmatched' | 'noGame';

function matchKind(detail: DetailView): MatchKind {
  const status = detail.row.status;
  if (typeof status === 'object' && 'matched' in status) return 'matched';
  if (status === 'ambiguous' || (typeof status === 'object' && 'ambiguous' in status)) {
    return 'ambiguous';
  }
  if (status === 'noGame') return 'noGame';
  return 'unmatched';
}

/** The Match section: provenance for a match, warnings for unmatched entries and suggestions. */
export function MatchSection({ detail, actions }: MatchSectionProps) {
  const { t } = useI18n();
  const kind = matchKind(detail);
  const status = detail.row.status;
  const source =
    typeof status === 'object' && 'matched' in status ? status.matched.source : null;
  const matchReady = actions?.matchGame === 'available';
  const matchReason =
    actions && actions.matchGame !== 'available'
      ? formatReason(actions.matchGame.disabled, t)
      : '';
  const showHeaderAction = kind === 'matched' || kind === 'noGame';
  const warning = kind === 'unmatched' || kind === 'ambiguous';

  return (
    <section data-testid="detail-match" className="flex flex-col gap-[8px]">
      <SectionLabel
        action={
          showHeaderAction ? (
            <button
              type="button"
              data-testid="detail-match-btn"
              disabled={!matchReady}
              onClick={() => commands.libraryInput('openMatch')}
              className={`rounded-xs text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand ${
                matchReady
                  ? 'text-brand hover:text-brand-4'
                  : 'cursor-not-allowed text-text-dimmed'
              }`}
            >
              {actions?.changeMatch
                ? t('Detail::Change…')
                : t('Detail::Match…')}
            </button>
          ) : undefined
        }
      >
        {t('Detail::Match')}
      </SectionLabel>

      {showHeaderAction && !matchReady && matchReason && (
        <div data-testid="detail-match-reason" className="text-xs text-warning">
          {matchReason}
        </div>
      )}

      {warning && (
        <div
          data-testid="detail-match-warning"
          className="flex flex-col gap-[10px] rounded-lg border border-warning bg-warning-subtle p-[12px]"
        >
          <div className="flex items-center gap-[8px]">
            <IconAlertTriangle
              size={16}
              stroke={2}
              aria-hidden="true"
              className="shrink-0 text-warning"
            />
            <span className="text-[13px] font-semibold text-text-primary">
              {kind === 'ambiguous'
                ? t('Detail::Match needs review')
                : t('Detail::No match on GameNode')}
            </span>
          </div>
          <p className="m-0 text-xs leading-[17px] text-text-secondary">
            {t('Detail::Match to sync status, collections and playtime with your profile.')}
          </p>
          <div className="flex items-center gap-[8px]">
            <Button
              variant="primary"
              size="sm"
              data-testid="detail-match-cta"
              disabled={!matchReady}
              onClick={() => commands.libraryInput('openMatch')}
            >
              {kind === 'ambiguous' ? t('Detail::Review…') : t('Detail::Match…')}
            </Button>
            {kind === 'unmatched' && (
              <Button
                variant="ghost"
                size="sm"
                data-testid="detail-not-a-game"
                onClick={() =>
                  commands.libraryInput({
                    markEntryNoGame: {
                      key: detail.row.key,
                      now: Math.floor(Date.now() / 1000),
                    },
                  })
                }
              >
                {t('Detail::Not a game')}
              </Button>
            )}
          </div>
          {!matchReady && matchReason && (
            <div data-testid="detail-match-reason" className="text-xs text-warning">
              {matchReason}
            </div>
          )}
        </div>
      )}

      {kind === 'matched' && (
        <div className="flex items-center gap-[10px]">
          <div className="h-[37px] w-[28px] shrink-0 overflow-hidden rounded-thumb bg-surface">
            <CoverImage
              coverFile={detail.row.coverFile}
              title={detail.row.matchedName ?? detail.row.title}
            />
          </div>
          <div className="flex min-w-0 flex-col gap-0.5">
            <span
              data-testid="detail-match-name"
              className="truncate text-[13px] font-medium text-text-default"
            >
              {detail.row.matchedName ?? detail.row.title}
            </span>
            <span className="truncate text-xs text-text-dimmed">
              {source === 'chosen'
                ? t('Detail::Chosen by you')
                : t('Detail::Matched automatically by title and year')}
            </span>
          </div>
        </div>
      )}

      {kind === 'noGame' && (
        <div data-testid="detail-not-a-game-note" className="text-[13px] text-text-dimmed">
          {t('Detail::Not a game')}
        </div>
      )}

      {warning && <SuggestionsList entryKey={detail.row.key} suggestions={detail.suggestions} />}
    </section>
  );
}
