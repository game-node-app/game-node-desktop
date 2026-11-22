import type { PluginImportPermissionReview } from '../../api';

/** Optional-permission switches, keyed by permission id. Absent means the default (allowed). */
export type ImportPermissionDecisions = Record<string, boolean>;

type Translate = (key: string) => string;

function knownLabel(key: string, raw: string, t: Translate): string {
  const translated = t(key);
  return translated === key ? raw : translated;
}

/** Human label for a known permission id, falling back to the raw id the manifest declared. */
export function importPermissionLabel(permissionId: string, t: Translate): string {
  return knownLabel(`plugins.permissionLabel.${permissionId}`, permissionId, t);
}

/** Short label for a known scope, falling back to the raw scope the manifest declared. */
export function importScopeLabel(scope: string, t: Translate): string {
  return knownLabel(`plugins.permissionScope.${scope}`, scope, t);
}

/** Localized platform name for the platforms a signed bundle declares. */
export function importPlatformLabel(platform: string, t: Translate): string {
  return knownLabel(`plugins.import.platform.${platform}`, platform, t);
}

/** True unless the user explicitly turned this optional permission off. */
export function importPermissionAllowed(
  permission: PluginImportPermissionReview,
  decisions: ImportPermissionDecisions,
): boolean {
  return permission.required || decisions[permission.id] !== false;
}

/** The optional permissions the user turned off, as the install contract expects them. */
export function deniedPermissionIds(
  permissions: PluginImportPermissionReview[],
  decisions: ImportPermissionDecisions,
): string[] {
  return permissions
    .filter((permission) => !permission.required && decisions[permission.id] === false)
    .map((permission) => permission.id);
}
