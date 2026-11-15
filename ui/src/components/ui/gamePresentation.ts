import type { EntryStatus, MatchStatus } from '../../api';
import type { InterpolationParams } from '../../i18n';
import type { PillTone } from './Pill';

/** The subset of `useI18n().t` the game primitives need. */
export type Translate = (key: string, params?: InterpolationParams) => string;

/**
 * Fills the positional `{}` placeholders used by the copy table. `t` itself
 * only fills named/indexed placeholders, so this keeps the bare form working.
 */
export function formatCopy(
  template: string,
  ...args: Array<string | number>
): string {
  let result = template;
  args.forEach((arg, index) => {
    const indexed = `{${index}}`;
    if (result.includes(indexed)) {
      result = result.replaceAll(indexed, String(arg));
    } else {
      result = result.replace('{}', String(arg));
    }
  });
  return result;
}

export type MatchTone = 'matched' | 'review' | 'unmatched' | 'noGame';

/** True for a row whose match has a GameNode game id. */
export function isMatchedStatus(status: MatchStatus): boolean {
  return typeof status === 'object' && 'matched' in status;
}

export function matchTone(status: MatchStatus): MatchTone {
  if (isMatchedStatus(status)) return 'matched';
  if (status === 'ambiguous') return 'review';
  if (typeof status === 'object' && 'ambiguous' in status) return 'review';
  if (status === 'noGame') return 'noGame';
  return 'unmatched';
}

/**
 * Match dot fills from `components.md`: success matched, warning ambiguous,
 * dimmed unmatched and muted no-game. Classes keep the color on Tailwind tokens.
 */
export const MATCH_DOT_CLASSES: Record<MatchTone, string> = {
  matched: 'bg-success',
  review: 'bg-warning',
  unmatched: 'bg-text-dimmed',
  noGame: 'bg-text-muted',
};

/** Row and tile match labels from `copy.md`. */
export function matchLabel(status: MatchStatus, t: Translate): string {
  switch (matchTone(status)) {
    case 'matched':
      return t('Library::Matched');
    case 'review':
      return t('Library::Review');
    case 'noGame':
      return t('Library::Not a game');
    default:
      return t('Library::No match');
  }
}

export interface GameNodeStatusView {
  label: string;
  tone: PillTone;
}

/**
 * The status pill label and tone from `copy.md`. A cached entry without a known
 * status means the game is in the library but this build cannot name the status.
 */
export function gameNodeStatusView(
  status: EntryStatus | null | undefined,
  t: Translate,
): GameNodeStatusView {
  switch (status) {
    case 'playing':
      return { label: t('Library::Playing'), tone: 'brand' };
    case 'finished':
      return { label: t('Library::Finished'), tone: 'success' };
    case 'planned':
      return { label: t('Library::Planned'), tone: 'neutral' };
    case 'ongoing':
      return { label: t('Library::Ongoing'), tone: 'neutral' };
    case 'dropped':
      return { label: t('Library::Dropped'), tone: 'danger' };
    default:
      return { label: t('Library::In library'), tone: 'neutral' };
  }
}

/** Short playtime for the tile meta row: "42 h" / "45 min". */
export function formatShortPlaytime(
  seconds: number | null | undefined,
  t: Translate,
): string {
  if (seconds == null || seconds < 0) return '—';
  const hours = Math.floor(seconds / 3600);
  if (hours > 0) return formatCopy(t('Library::{} h'), hours);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (minutes > 0) return formatCopy(t('Library::{} min'), minutes);
  const secs = seconds % 60;
  if (secs > 0) return formatCopy(t('Library::{} s'), secs);
  return formatCopy(t('Library::{} min'), 0);
}

/** Long playtime for the list: "42 h 18 min", "118 h", "45 min". */
export function formatLongPlaytime(
  seconds: number | null | undefined,
  t: Translate,
): string {
  if (seconds == null || seconds < 0) return '—';
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours > 0) {
    return minutes > 0
      ? formatCopy(t('Library::{} h {} min'), hours, minutes)
      : formatCopy(t('Library::{} h'), hours);
  }
  if (minutes > 0) return formatCopy(t('Library::{} min'), minutes);
  const secs = seconds % 60;
  if (secs > 0) return formatCopy(t('Library::{} s'), secs);
  return formatCopy(t('Library::{} min'), 0);
}

const DAY_MS = 24 * 60 * 60 * 1000;

function startOfLocalDay(seconds: number): number {
  const date = new Date(seconds * 1000);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

function formatMonthYear(seconds: number, locale: string): string {
  const formatted = new Intl.DateTimeFormat(locale, {
    month: 'short',
    year: 'numeric',
  })
    .format(new Date(seconds * 1000))
    .replace(/\s+de\s+/i, ' ');
  return formatted.charAt(0).toLocaleUpperCase(locale) + formatted.slice(1);
}

/**
 * Relative last-session label from `copy.md`: Now, Yesterday, "3 days ago",
 * Last week, then a localized month and year or a bare year. `nowSeconds`
 * keeps the primitive deterministic in tests; callers can refresh it on render.
 */
export function formatLastSession(
  lastPlayedAt: number | null | undefined,
  t: Translate,
  nowSeconds: number,
  locale: string,
): string {
  if (lastPlayedAt == null || lastPlayedAt < 0) return t('Library::Never');

  const days = Math.round(
    (startOfLocalDay(nowSeconds) - startOfLocalDay(lastPlayedAt)) / DAY_MS,
  );
  if (days <= 0) return t('Library::Now');
  if (days === 1) return t('Library::Yesterday');
  if (days < 7) return formatCopy(t('Library::{} days ago'), days);
  if (days < 14) return t('Library::Last week');

  const played = new Date(lastPlayedAt * 1000);
  const now = new Date(nowSeconds * 1000);
  if (played.getFullYear() === now.getFullYear()) {
    return formatMonthYear(lastPlayedAt, locale);
  }
  return String(played.getFullYear());
}
