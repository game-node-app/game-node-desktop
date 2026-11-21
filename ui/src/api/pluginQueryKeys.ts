/** Shared React Query keys for host-owned plugin settings state. */
export const pluginQueryKeys = {
  inventory: ['plugin-inventory'] as const,
  updatePreferences: ['plugin-update-preferences'] as const,
  availableUpdates: ['plugin-update-available'] as const,
  permissions: (pluginId: string, profileId: string, publisherFingerprint?: string) =>
    ['plugin-permissions', profileId, pluginId, ...(publisherFingerprint ? [publisherFingerprint] : [])] as const,
  sharedData: (pluginId: string, profileId: string, publisherFingerprint?: string) =>
    ['plugin-shared-data-policies', profileId, pluginId, ...(publisherFingerprint ? [publisherFingerprint] : [])] as const,
  durableUsage: (pluginId: string, profileId: string) =>
    ['plugin-durable-usage', profileId, pluginId] as const,
  configuration: (pluginId: string) => ['plugin-configuration', pluginId] as const,
};
