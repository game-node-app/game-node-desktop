import type { EntryStatus, Reason, MatchStatus } from '../../api';

export function formatI18n(template: string, ...args: Array<string | number>): string {
  let result = template;
  for (let i = 0; i < args.length; i++) {
    if (result.includes(`{${i}}`)) {
      result = result.replaceAll(`{${i}}`, String(args[i]));
    } else if (result.includes('{}')) {
      result = result.replace('{}', String(args[i]));
    }
  }
  return result;
}

export function formatStatus(
  status: EntryStatus | string | null | undefined,
  t: (key: string, params?: any) => string,
): string {
  switch (status) {
    case 'playing':
      return t('Library::Playing');
    case 'finished':
      return t('Library::Finished');
    case 'planned':
      return t('Library::Planned');
    case 'dropped':
      return t('Library::Dropped');
    case 'ongoing':
      return t('Library::Ongoing');
    default:
      return '';
  }
}

export function formatReason(
  reason: Reason | undefined | null,
  t: (key: string, params?: any) => string,
): string {
  if (!reason) return '';
  if (typeof reason === 'string') {
    switch (reason) {
      case 'running':
        return t('Library::Running');
      case 'nothingSelected':
        return t('Library::Select a game first.');
      case 'multipleSelected':
        return t('Library::Select a single game.');
      case 'noLaunchRule':
        return t('Library::This store cannot start games from GameNode.');
      case 'noInstallRule':
        return t('Library::This store cannot install games from GameNode.');
      case 'missingField':
        return t('Library::Details this action needs are missing. Rescan to refresh them.');
      case 'needsVerifiedSession':
        return t('Library::Your GameNode session is not verified yet. Retry the connection.');
      case 'offline':
        return t('Library::GameNode cannot be reached right now.');
      case 'notMatched':
        return t('Library::Match this game to a GameNode game first.');
      case 'noMatchedGames':
        return t('Library::None of the selected games is matched to a GameNode game.');
      case 'checkingMembership':
        return t('Library::Checking your GameNode library…');
      case 'membershipUnavailable':
        return t('Library::Could not check your GameNode library. Try again.');
      default:
        return '';
    }
  }
  if (typeof reason === 'object' && 'alreadyInLibrary' in reason) {
    if (reason.alreadyInLibrary == null) {
      return t('Library::Already in your GameNode library.');
    }
    return formatI18n(
      t('Library::Already in your GameNode library as {}.'),
      formatStatus(reason.alreadyInLibrary, t),
    );
  }
  return '';
}

export function formatDuration(
  seconds: number | null | undefined,
  t: (key: string, params?: any) => string,
): string {
  if (seconds == null || seconds < 0) return '—';
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  if (hours > 0) {
    return formatI18n(t('Library::{} h {} min'), hours, minutes);
  }
  if (minutes > 0) {
    return formatI18n(t('Library::{} min'), minutes);
  }
  if (secs > 0) {
    return formatI18n(t('Library::{} s'), secs);
  }
  return formatI18n(t('Library::{} min'), 0);
}

export function formatMatchStatus(
  status: MatchStatus,
  t: (key: string, params?: any) => string,
): string {
  if (typeof status === 'object' && 'matched' in status) {
    return t('Library::Matched');
  }
  switch (status) {
    case 'ambiguous':
      return t('Library::Several possible matches');
    case 'noGame':
      return t('Library::Not on GameNode');
    case 'unmatched':
    default:
      return t('Library::Not matched');
  }
}

export function getMatchDotColor(status: MatchStatus): string {
  if (typeof status === 'object' && 'matched' in status) {
    return 'var(--color-success, #3DD68C)';
  }
  if (status === 'ambiguous') {
    return 'var(--color-warning, #F5A524)';
  }
  return 'var(--color-text-dimmed, #808080)';
}
