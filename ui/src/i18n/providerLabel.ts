/** Known built-in names follow the UI locale; external manifests keep their names. */
export function providerLabel(id: string, manifestName: string | undefined, t: (key: string) => string): string {
  if (id === 'packagekit') {
    return t('providers.packagekit');
  }
  const name = manifestName?.trim();
  return name ? name : fallbackProviderName(id);
}

/** A readable name for a provider id whose manifest name is not available (for example signed out). */
export function fallbackProviderName(id: string): string {
  return id
    .split(/[-_]/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}
