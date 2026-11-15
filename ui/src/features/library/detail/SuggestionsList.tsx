import { commands, type SuggestionView } from '../../../api';
import { Button, Pill } from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { formatI18n } from '../formatters';
import { suggestionTone } from './format';

export interface SuggestionsListProps {
  entryKey: string;
  suggestions: SuggestionView[];
}

/** The scored GameNode suggestions with confidence pills and a Use action per row. */
export function SuggestionsList({ entryKey, suggestions }: SuggestionsListProps) {
  const { t } = useI18n();
  if (suggestions.length === 0) return null;

  return (
    <div data-testid="detail-suggestions" className="flex flex-col">
      <div className="pb-[4px] text-[11px] font-semibold uppercase tracking-label text-text-muted">
        {t('Detail::Suggestions')}
      </div>
      {suggestions.map((suggestion) => (
        <div
          key={suggestion.gameId}
          data-testid={`detail-suggestion-${suggestion.gameId}`}
          className="flex items-center gap-[10px] border-b border-border-subtle py-[8px] last:border-b-0"
        >
          {suggestion.coverUrl ? (
            <img
              src={suggestion.coverUrl}
              alt=""
              loading="lazy"
              decoding="async"
              className="h-[37px] w-[28px] shrink-0 rounded-thumb bg-surface object-cover"
            />
          ) : (
            <div className="h-[37px] w-[28px] shrink-0 rounded-thumb bg-surface" aria-hidden="true" />
          )}
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="truncate text-[13px] font-medium text-text-default">
              {suggestion.title}
            </span>
            {suggestion.year != null && (
              <span className="truncate text-[11px] text-text-dimmed">
                {suggestion.year}
              </span>
            )}
          </div>
          <Pill tone={suggestionTone(suggestion.tone)}>
            {formatI18n('{}%', suggestion.confidence)}
          </Pill>
          <Button
            variant="secondary"
            size="sm"
            data-testid={`detail-suggestion-use-${suggestion.gameId}`}
            onClick={() =>
              commands.libraryInput({
                useSuggestion: {
                  key: entryKey,
                  gameId: suggestion.gameId,
                  now: Math.floor(Date.now() / 1000),
                },
              })
            }
          >
            {t('Detail::Use')}
          </Button>
        </div>
      ))}
    </div>
  );
}
