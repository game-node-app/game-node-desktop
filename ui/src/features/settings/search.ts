/**
 * Settings search normalization (design D2). Matching runs on the translated
 * text of the active locale and ignores case and diacritics, so "configuracoes"
 * finds "Configurações" and "bandeja" finds "…minimiza para a bandeja".
 */
export function normalizeSettingsSearch(value: string): string {
  return value
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .trim();
}

/** True when `text` contains the already-normalized query. Empty queries never match. */
export function settingsTextMatches(
  text: string,
  normalizedQuery: string,
): boolean {
  if (normalizedQuery.length === 0) return false;
  return normalizeSettingsSearch(text).includes(normalizedQuery);
}
