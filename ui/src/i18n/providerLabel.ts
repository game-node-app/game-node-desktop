/** Known built-in names follow the UI locale; external manifests keep their names. */
export function providerLabel(id: string, manifestName: string, t: (key: string) => string): string {
  return id === 'packagekit' ? t('providers.packagekit') : manifestName;
}
