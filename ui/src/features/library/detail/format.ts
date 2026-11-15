import type {
  EntryStatus,
  GameToolView,
  Membership,
  PlaytimeView,
  SuggestionTone,
} from '../../../api';

import type { InterpolationParams } from '../../../i18n';

export type Translator = (key: string, params?: InterpolationParams) => string;

/** The provider id whose entries are local DRM-free games rather than store entries. */
export const DRM_FREE_PROVIDER = 'manual';

/** The engine keys DRM-free library rows `manual:<registration id>`. */
export function isDrmFreeKey(key: string): boolean {
  return key.startsWith(`${DRM_FREE_PROVIDER}:`);
}

/** The registration id an entry key carries, without the provider prefix. */
export function entryRegistrationId(key: string): string {
  const at = key.indexOf(':');
  return at < 0 ? key : key.slice(at + 1);
}

/** The GameNode membership status when the entry is in the library, else `null`. */
export function membershipStatus(membership: Membership): EntryStatus | null | undefined {
  if (typeof membership === 'object' && 'inLibrary' in membership) {
    return membership.inLibrary;
  }
  return undefined;
}

/** Whether a membership answer says the game is in the GameNode library. */
export function isInGameNodeLibrary(membership: Membership): boolean {
  return typeof membership === 'object' && 'inLibrary' in membership;
}

const STATUS_LABELS: Record<EntryStatus, string> = {
  playing: 'Detail::Playing on GameNode',
  finished: 'Detail::Finished on GameNode',
  planned: 'Detail::Planned on GameNode',
  ongoing: 'Detail::Ongoing on GameNode',
  dropped: 'Detail::Dropped on GameNode',
};

const STATUS_TONES: Record<EntryStatus, 'brand' | 'success' | 'neutral' | 'danger'> = {
  playing: 'brand',
  finished: 'success',
  planned: 'neutral',
  ongoing: 'neutral',
  dropped: 'danger',
};

/** The detail-pane pill label for a known GameNode status. */
export function detailStatusLabel(status: EntryStatus, t: Translator): string {
  return t(STATUS_LABELS[status]);
}

/** The detail-pane pill tone for a known GameNode status, from `copy.md`. */
export function detailStatusTone(status: EntryStatus): 'brand' | 'success' | 'neutral' | 'danger' {
  return STATUS_TONES[status];
}

/** The ordered GameNode statuses the detail select offers. */
export const ENTRY_STATUSES: EntryStatus[] = [
  'playing',
  'finished',
  'planned',
  'ongoing',
  'dropped',
];

/** The confidence pill tone the view sends. */
export function suggestionTone(tone: SuggestionTone): 'success' | 'warning' | 'neutral' {
  return tone;
}

const DAY_SECONDS = 86_400;

function capitalize(value: string): string {
  return value.length === 0 ? value : value[0].toUpperCase() + value.slice(1);
}

/**
 * The list/detail relative date: "Now", "Yesterday", "3 days ago", "Last week", then a
 * month-year, then a year, always in the active locale.
 */
export function formatRelativeDate(
  at: number | null | undefined,
  nowSeconds: number,
  locale: string,
  t: Translator,
): string {
  if (at == null) return t('Library::Never');
  const elapsed = Math.max(0, nowSeconds - at);
  if (elapsed < DAY_SECONDS) return t('Library::Now');
  const days = Math.floor(elapsed / DAY_SECONDS);
  if (days <= 1) return t('Library::Yesterday');
  if (days < 7) return t('Library::{} days ago').replace('{}', String(days));
  if (days < 14) return t('Library::Last week');
  const date = new Date(at * 1000);
  if (date.getFullYear() !== new Date(nowSeconds * 1000).getFullYear()) {
    return String(date.getFullYear());
  }
  const formatted = new Intl.DateTimeFormat(locale, {
    month: 'short',
    year: 'numeric',
  }).format(date);
  return capitalize(formatted.replace(/\s+de\s+/i, ' '));
}

/** A clock time for the save-protection stat hint. */
export function formatClock(at: number | null | undefined, locale: string): string | null {
  if (at == null) return null;
  return new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(at * 1000));
}

/** The playtime source line under the detail Time card. */
export function playtimeSourceLabel(
  playtime: PlaytimeView | undefined,
  providerName: string,
  t: Translator,
): string {
  if (!playtime || playtime.source !== 'store') {
    return t('Detail::Internal counter');
  }
  if (providerName.toLowerCase() === 'steam') {
    return t('Detail::Steam counter');
  }
  return t('Library::Imported from {}').replace('{}', providerName);
}

/** The tool summary shown on the detail row, for example `OptiScaler 0.7.4 (dxgi.dll)`. */
export function toolSummary(tools: GameToolView[] | undefined): string | null {
  const installed = tools ?? [];
  if (installed.length === 0) return null;
  return installed
    .map((tool) => {
      const proxy = tool.proxyDll
        ? ` (${tool.proxyDll.endsWith('.dll') ? tool.proxyDll : `${tool.proxyDll}.dll`})`
        : '';
      return `${tool.name} ${tool.version}${proxy}`.trim();
    })
    .join(', ');
}
