import type {
  PortCatalogError,
  PortEntryView,
  PortFilters,
  PortInstallBlockReason,
  PortJobView,
  PortPlacementRoot,
  PortSetupSlot,
} from '../../../api/ports';

export type Translate = (
  key: string,
  params?: Record<string, string | number> | Array<string | number>,
) => string;

/** Folds case and diacritics so `bloodborne` matches `Bloodborne` and `configuração`. */
export function normalizeSearch(value: string): string {
  return value
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLocaleLowerCase();
}

/**
 * Applies the search box and the persisted filters. Search covers the entry name and the
 * original game titles, as the desktop-port-catalog spec requires.
 */
export function filterPortEntries(
  entries: ReadonlyArray<PortEntryView>,
  search: string,
  filters: PortFilters,
): PortEntryView[] {
  const needle = normalizeSearch(search.trim());
  return entries.filter((entry) => {
    if (filters.consoles.length > 0) {
      const matchesConsole = entry.consoles.some((consoleName) =>
        filters.consoles.includes(consoleName),
      );
      if (!matchesConsole) return false;
    }
    if (filters.guided && !entry.guided) return false;
    if (filters.verified && !entry.verified) return false;
    if (filters.installed && !entry.installedRegistrationKey) return false;
    if (needle.length > 0) {
      const haystack = normalizeSearch([entry.name, ...entry.games].join(' '));
      if (!haystack.includes(needle)) return false;
    }
    return true;
  });
}

export function portFiltersAreDefault(filters: PortFilters): boolean {
  return (
    filters.consoles.length === 0 &&
    !filters.guided &&
    !filters.verified &&
    !filters.installed
  );
}

export function catalogErrorMessage(
  error: PortCatalogError | null | undefined,
  t: Translate,
): string {
  switch (error) {
    case 'rateLimited':
      return t(
        'Ports::The catalog server asked GameNode to slow down. Try again in a few minutes.',
      );
    case 'invalidResponse':
      return t('Ports::The catalog server sent a document GameNode could not read.');
    case 'emptyListing':
      return t('Ports::The catalog server returned an empty listing.');
    case 'unreachable':
      return t(
        'Ports::The catalog server cannot be reached. Check your connection and try again.',
      );
    case 'unknown':
    default:
      return t('Ports::The catalog could not be loaded.');
  }
}

export function installBlockReasonText(
  reason: PortInstallBlockReason | null | undefined,
  t: Translate,
): string | null {
  switch (reason) {
    case 'withdrawn':
      return t('Ports::This port was withdrawn upstream.');
    case 'noVerifiedRelease':
      return t('Ports::No verified release is available, so Install is unavailable.');
    case 'installerOnly':
      return t('Ports::The Windows download is an installer, which GameNode does not run.');
    case 'noWindowsAsset':
      return t('Ports::No Windows download is available.');
    case 'missingAsset':
      return t('Ports::The Windows download is missing from the release.');
    case 'noExecutable':
      return t("Ports::None of the port's expected executables is available.");
    default:
      return t('Ports::Install is unavailable for this entry.');
  }
}

export function placementRootLabel(
  root: PortPlacementRoot | null,
  t: Translate,
): string {
  switch (root) {
    case 'game':
      return t('Ports::the game folder');
    case 'data':
      return t('Ports::app data');
    case 'config':
      return t('Ports::settings');
    case 'localData':
      return t('Ports::local app data');
    default:
      return '';
  }
}

export function setupSlotKindLabel(slot: PortSetupSlot, t: Translate): string {
  return slot.kind === 'folder' ? t('Ports::Folder') : t('Ports::File');
}

/** `v0.9.38` unless the upstream value already carries the prefix. */
export function versionLabel(version: string | null | undefined): string {
  if (!version) return '';
  return /^v/i.test(version) ? version : `v${version}`;
}

export function formatPortDate(
  iso: string | null | undefined,
  locale: string,
): string {
  if (!iso) return '';
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return '';
  try {
    return new Intl.DateTimeFormat(locale, { dateStyle: 'medium' }).format(date);
  } catch {
    return date.toISOString().slice(0, 10);
  }
}

/**
 * Short age used by the stale banner and the footer. `now` is injectable so tests do not
 * depend on the wall clock.
 */
export function formatCatalogAge(
  fetchedAt: number | null | undefined,
  locale: string,
  now = Date.now(),
): string {
  if (!fetchedAt) return '';
  const seconds = Math.max(0, Math.floor(now / 1000) - fetchedAt);
  try {
    const rtf = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
    if (seconds < 60) return rtf.format(-seconds, 'second');
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return rtf.format(-minutes, 'minute');
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return rtf.format(-hours, 'hour');
    const days = Math.floor(hours / 24);
    if (days < 30) return rtf.format(-days, 'day');
    const months = Math.floor(days / 30);
    if (months < 12) return rtf.format(-months, 'month');
    return rtf.format(-Math.floor(months / 12), 'year');
  } catch {
    return new Date(fetchedAt * 1000).toISOString().slice(0, 10);
  }
}

export function formatBytes(bytes: number | null | undefined, locale: string): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return '';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const maximumFractionDigits = value >= 100 || unit === 0 ? 0 : 1;
  try {
    return `${new Intl.NumberFormat(locale, { maximumFractionDigits }).format(value)} ${
      units[unit]
    }`;
  } catch {
    return `${Math.round(value)} ${units[unit]}`;
  }
}

/** The label of the selected slot while a guided install processes supplied files. */
export function jobSlotLabel(job: PortJobView, entry: PortEntryView): string | null {
  if (!job.slotId) return null;
  return entry.requirements.find((slot) => slot.id === job.slotId)?.label ?? null;
}

export function jobPhaseLabel(job: PortJobView, entry: PortEntryView, t: Translate): string {
  const slot = jobSlotLabel(job, entry);
  switch (job.phase) {
    case 'queued':
      return t('Ports::Queued');
    case 'resolving':
      return t('Ports::Preparing');
    case 'downloading':
      return t('Ports::Downloading');
    case 'verifying':
      return t('Ports::Verifying');
    case 'extracting':
      return t('Ports::Extracting');
    case 'suppliedFiles':
      return slot
        ? `${t('Ports::Adding your files')} · ${slot}`
        : t('Ports::Adding your files');
    case 'finalizing':
      return t('Ports::Finalizing');
    case 'registering':
      return t('Ports::Adding to your library');
    case 'cancelling':
      return t('Ports::Cancelling');
    case 'cancelled':
      return t('Ports::Cancelled');
    case 'failed':
      return t('Ports::Install failed');
    case 'interrupted':
      return t('Ports::Install interrupted');
    case 'completed':
      return t('Ports::Install complete');
    default:
      return job.phase;
  }
}

export function isRunningJob(job: PortJobView): boolean {
  return (
    job.phase === 'queued' ||
    job.phase === 'resolving' ||
    job.phase === 'downloading' ||
    job.phase === 'verifying' ||
    job.phase === 'extracting' ||
    job.phase === 'suppliedFiles' ||
    job.phase === 'finalizing' ||
    job.phase === 'registering' ||
    job.phase === 'cancelling'
  );
}
