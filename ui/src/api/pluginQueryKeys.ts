/** Shared React Query keys for host-owned plugin settings state. */
export const pluginQueryKeys = {
  inventory: ['plugin-inventory'] as const,
  updatePreferences: ['plugin-update-preferences'] as const,
  availableUpdates: ['plugin-update-available'] as const,
  permissions: (pluginId: string, profileId: string) => ['plugin-permissions', profileId, pluginId] as const,
  sharedData: (pluginId: string, profileId: string) => ['plugin-shared-data-policies', profileId, pluginId] as const,
  configuration: (pluginId: string) => ['plugin-configuration', pluginId] as const,
};
