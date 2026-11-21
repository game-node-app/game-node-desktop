/** One host-resolved game/installation in a shared emulator resource's affected scope. */
export interface PluginSharedResourceGame {
  installationId: string;
  gameId: string;
  name: string;
}

/**
 * A declared shared emulator save resource awaiting or holding user consent. The host resolves
 * the affected games and backing location; a plugin cannot supply either as authority.
 */
export interface PluginSharedResourceReview {
  pluginId: string;
  publisherFingerprint: string;
  profileId: string;
  profileName: string;
  gameId: string;
  installationId: string;
  runtimeId: string;
  resourceId: string;
  displayName: string;
  affectedGames: PluginSharedResourceGame[];
  /** `pending`, `current` or `stale`. */
  consent: 'pending' | 'current' | 'stale';
  /** True when the approval or affected set can no longer be resolved. */
  unavailable: boolean;
}
