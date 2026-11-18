import { invoke } from '@tauri-apps/api/core';
import { getMockHost, mockEnabled } from './mock';
import type {
  AchievementFilter,
  AchievementListView,
  AppInfo,
  AuthInput,
  GameNodePage,
  LaunchKind,
  LaunchRecipeChoices,
  LibraryInput,
  PluginInfo,
  PluginPermissionSnapshot,
  PluginPermissionDecision,
  PluginSharedDataSnapshot,
  PluginSharedDataPolicyUpdate,
  PluginImportReview,
  PluginConfigurationState,
  PluginPlaytimeInfo,
  PluginUpdatePreferences,
  AvailablePluginUpdate,
  AvailablePluginRollback,
  PluginUpdateReview,
  PlaytimeSource,
  Registration,
  Snapshot,
  ToolChannel,
  WindowGeometry,
} from './types';
import type {
  PortDetailView,
  PortInstallRequest,
  PortPickSuppliedRequest,
  PortPreferencesPatch,
  PortSuppliedPick,
  PortToolsCacheView,
} from './ports';
import type { RememberedConfirmation } from './pluginConfirmations';
import type { PluginFolderApproval } from './pluginFolderApprovals';
import type { PluginConfigurationModes, PluginConfigurationStatus, PluginSettingsContext, PluginSettingsReply, PluginSettingsValue } from './pluginSettings';
import { createDefaultSnapshot } from './types';

export function isTauriRuntime(): boolean {
  return (
    typeof window !== 'undefined' &&
    ('__TAURI_INTERNALS__' in window || '__TAURI_IPC__' in window)
  );
}

export async function safeInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
  fallback?: () => T,
): Promise<T> {
  if (mockEnabled) {
    return (await (await getMockHost()).invoke(cmd, args)) as T;
  }
  if (!isTauriRuntime() && fallback) {
    return fallback();
  }
  return invoke<T>(cmd, args);
}

export const commands = {
  getSnapshot: () =>
    safeInvoke<Snapshot>('get_snapshot', undefined, () => createDefaultSnapshot()),

  getAppInfo: () =>
    safeInvoke<AppInfo>('get_app_info', undefined, () => ({
      locale: 'en',
      version: '0.1.0',
      backgroundSupported: false,
      logDirectory: null,
    })),

  getRecentLogs: () =>
    safeInvoke<string>('get_recent_logs', undefined, () => '(no log available in the web preview)'),

  openLogsFolder: () =>
    safeInvoke<void>('open_logs_folder', undefined, () => undefined),

  getLicenseNotices: () =>
    safeInvoke<string>(
      'get_license_notices',
      undefined,
      () => 'GameNode Desktop - GPL-3.0-only',
    ),

  dispatchAuth: (input: AuthInput) =>
    safeInvoke<void>('dispatch_auth', { input }, () => undefined),

  setSyncEnabled: (enabled: boolean) =>
    safeInvoke<void>('set_sync_enabled', { enabled }, () => undefined),

  setPlaytimeSource: (source: PlaytimeSource) =>
    safeInvoke<void>('set_playtime_source', { source }, () => undefined),

  dismissSyncExplanation: () =>
    safeInvoke<void>('dismiss_sync_explanation', undefined, () => undefined),

  updateWindowGeometry: (geometry: WindowGeometry) =>
    safeInvoke<void>('update_window_geometry', { geometry }, () => undefined),

  quitApp: () =>
    safeInvoke<void>('quit_app', undefined, () => undefined),

  libraryInput: (input: LibraryInput) =>
    safeInvoke<void>('library_input', { input }, () => undefined),

  openGameNodePage: (page: GameNodePage) =>
    safeInvoke<void>('open_gamenode_page', { page }, () => undefined),

  activateEntry: () =>
    safeInvoke<void>('activate_entry', undefined, () => undefined),

  openEntry: (entryKey: string, kind: LaunchKind) =>
    safeInvoke<void>('open_entry', { entryKey, kind }, () => undefined),

  reviewDetectedTarget: (entryKey: string, expectedRevision: number) =>
    safeInvoke<void>(
      'review_detected_target',
      { entryKey, expectedRevision },
      () => undefined,
    ),

  launchRecipeChoices: (entryKey: string) =>
    safeInvoke<LaunchRecipeChoices>(
      'launch_recipe_choices',
      { entryKey },
      () => ({
        targetRevision: 0,
        detectedDefaultAvailable: false,
        selected: null,
        recipes: [],
      }),
    ),

  selectLaunchRecipe: (
    entryKey: string,
    recipe: {
      pluginId: string;
      publisherFingerprint: string;
      recipeId: string;
      recipeRevision: number;
    },
    expectedTargetRevision: number,
  ) =>
    safeInvoke<void>(
      'select_launch_recipe',
      { entryKey, ...recipe, expectedTargetRevision },
      () => undefined,
    ),

  useDetectedDefault: (entryKey: string, expectedTargetRevision: number) =>
    safeInvoke<void>(
      'use_detected_default',
      { entryKey, expectedTargetRevision },
      () => undefined,
    ),

  repairDetectedTarget: (entryKey: string, expectedRevision: number) =>
    safeInvoke<void>(
      'repair_detected_target',
      { entryKey, expectedRevision },
      () => undefined,
    ),

  launchSelected: (kind: LaunchKind) =>
    safeInvoke<void>('launch_selected', { kind }, () => undefined),

  startExtensionAction: (entryKey: string, pluginId: string, operation: 'play' | 'install') =>
    safeInvoke<void>('start_extension_action', { entryKey, pluginId, operation }, () => undefined),

  availablePluginExtensions: (operation: 'play' | 'install') =>
    safeInvoke<PluginInfo[]>('available_plugin_extensions', { operation }, () => []),

  dismissLaunch: () =>
    safeInvoke<void>('dismiss_launch', undefined, () => undefined),

  rescanLibrary: () =>
    safeInvoke<void>('rescan_library', undefined, () => undefined),

  pluginInventory: () =>
    safeInvoke<PluginInfo[]>('plugin_inventory', undefined, () => []),
  pluginImportReview: (archivePath: string) =>
    safeInvoke<PluginImportReview>('plugin_import_review', { archivePath }),
  pluginImportConfirmPublisher: (token: string, confirmedFingerprint: string) =>
    safeInvoke<PluginImportReview>('plugin_import_confirm_publisher', { token, confirmedFingerprint }),
  pluginImportInstall: (token: string, nativeCodeConsent: boolean) =>
    safeInvoke<string>('plugin_import_install', { token, nativeCodeConsent }),
  pluginImportLicenseText: (token: string, file: string) =>
    safeInvoke<string>('plugin_import_license_text', { token, file }),

  pluginPermissionSnapshot: (pluginId: string) =>
    safeInvoke<PluginPermissionSnapshot>(
      'plugin_permission_snapshot',
      { pluginId },
      () => ({
        pluginId,
        publisherFingerprint: '',
        profileId: '',
        grantRevision: 0,
        requestedPermissions: [],
        effectiveGrants: [],
      }),
    ),

  pluginPermissionsReplace: (
    pluginId: string,
    expectedProfileId: string,
    expectedRevision: number,
    decisions: PluginPermissionDecision[],
  ) => safeInvoke<PluginPermissionSnapshot>('plugin_permissions_replace', {
    pluginId,
    expectedProfileId,
    expectedRevision,
    decisions,
  }),
  pluginSharedDataSnapshot: (pluginId: string, expectedProfileId: string) =>
    safeInvoke<PluginSharedDataSnapshot>(
      'plugin_shared_data_snapshot',
      { pluginId, expectedProfileId },
      () => ({
        pluginId,
        publisherFingerprint: '',
        profileId: expectedProfileId,
        policyRevision: 0,
        commonCategories: [],
        selectedRecordIds: [],
        catalog: [],
        sourcePolicies: [],
      }),
    ),
  pluginSharedDataReplace: (pluginId: string, update: PluginSharedDataPolicyUpdate) =>
    safeInvoke<PluginSharedDataSnapshot>('plugin_shared_data_replace', { pluginId, update }),
  pluginEnable: (pluginId: string) => safeInvoke<void>('plugin_enable', { pluginId }),
  pluginDisable: (pluginId: string) => safeInvoke<void>('plugin_disable', { pluginId }),
  pluginRemove: (pluginId: string) => safeInvoke<void>('plugin_remove', { pluginId }),

  pluginConfigurationState: (pluginId: string) =>
    safeInvoke<PluginConfigurationState>(
      'plugin_configuration_state',
      { pluginId },
      () => ({ pluginId, available: false, revision: null }),
    ),

  pluginConfigurationModes: (providerId: string) =>
    safeInvoke<PluginConfigurationModes>('plugin_configuration_modes', { providerId }, () => ({
      externalDeclared: false, externalAuthorized: false,
      declarativeDeclared: false, declarativeAuthorized: false,
    })),
  pluginConfigurationOpen: (providerId: string, focus: boolean) =>
    safeInvoke<PluginConfigurationStatus>('plugin_configuration_open', { providerId, focus }, () => ({
      status: 'unavailable', windowId: null, message: 'Native plugin configuration is unavailable in the web preview',
    })),
  pluginSettingsDescribe: (providerId: string) =>
    safeInvoke<PluginSettingsReply>('plugin_settings_describe', { providerId }, () => {
      throw new Error('Plugin settings are unavailable in the web preview');
    }),
  pluginSettingsSubmit: (
    context: PluginSettingsContext,
    schemaRevision: string,
    configurationRevision: string,
    values: Record<string, PluginSettingsValue>,
  ) => safeInvoke<PluginSettingsReply>('plugin_settings_submit', {
    context, schemaRevision, configurationRevision, values,
  }, () => { throw new Error('Plugin settings are unavailable in the web preview'); }),
  pluginSettingsCancel: (context: PluginSettingsContext) =>
    safeInvoke<PluginSettingsReply>('plugin_settings_cancel', { context }, () => {
      throw new Error('Plugin settings are unavailable in the web preview');
    }),

  pluginPlaytimeInventory: () =>
    safeInvoke<PluginPlaytimeInfo[]>('plugin_playtime_inventory', undefined, () => []),
  pluginPlaytimeSetAccounting: (pluginId: string, enabled: boolean) =>
    safeInvoke<void>('plugin_playtime_set_accounting', { pluginId, enabled }, () => undefined),

  pluginUpdatePreferences: () =>
    safeInvoke<PluginUpdatePreferences>('plugin_update_preferences', undefined, () => ({ periodicChecks: false })),
  pluginUpdateAvailable: () =>
    safeInvoke<AvailablePluginUpdate[]>('plugin_update_available', undefined, () => []),
  pluginUpdateSetPeriodicChecks: (enabled: boolean) =>
    safeInvoke<PluginUpdatePreferences>('plugin_update_set_periodic_checks', { enabled }, () => ({ periodicChecks: enabled })),
  pluginUpdateCheck: () =>
    safeInvoke<AvailablePluginUpdate[]>('plugin_update_check', undefined, () => []),
  pluginUpdateReview: (pluginId: string, version: string) =>
    safeInvoke<PluginUpdateReview>('plugin_update_review', { pluginId, version }, () => ({
      pluginId, installedVersion: '1.0.0', candidateVersion: version,
      addedPermissions: [], removedPermissions: [], changedPermissions: [], permissionChanges: [],
      addedNativeAccess: [], removedNativeAccess: [], apiChanged: false, installedApi: '1.0.0 – 1.0.0',
      candidateApi: '1.0.0 – 1.0.0', licenseChanged: false, installedLicense: 'MIT',
      candidateLicense: 'MIT', installedLicenseFiles: [], candidateLicenseFiles: [],
      installedLicenseNotices: [], candidateLicenseNotices: [], publisherChanged: false,
      installedPlaytime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' },
      candidatePlaytime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' },
      playtimeChanged: false,
      installedPublisherKeyId: 'publisher', candidatePublisherKeyId: 'publisher', requiresManualApproval: true,
      publisherFingerprint: '', changedDataSchemas: [], preservesPriorData: true, isDowngrade: false,
    })),
  pluginRollbackAvailable: () =>
    safeInvoke<AvailablePluginRollback[]>('plugin_rollback_available', undefined, () => []),
  pluginRollbackReview: (pluginId: string, version: string) =>
    safeInvoke<PluginUpdateReview>('plugin_rollback_review', { pluginId, version }, () => ({
      pluginId, installedVersion: '2.0.0', candidateVersion: version,
      addedPermissions: [], removedPermissions: [], changedPermissions: [], permissionChanges: [],
      addedNativeAccess: [], removedNativeAccess: [], apiChanged: false, installedApi: '1.0.0 – 1.0.0',
      candidateApi: '1.0.0 – 1.0.0', licenseChanged: false, installedLicense: 'MIT', candidateLicense: 'MIT',
      installedLicenseFiles: [], candidateLicenseFiles: [], installedLicenseNotices: [], candidateLicenseNotices: [],
      installedPlaytime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' },
      candidatePlaytime: { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' },
      playtimeChanged: false, publisherChanged: false, installedPublisherKeyId: '', candidatePublisherKeyId: '', publisherFingerprint: '',
      changedDataSchemas: [], preservesPriorData: true, isDowngrade: true, requiresManualApproval: true,
    })),
  pluginUpdateRetrustPublisher: (pluginId: string, version: string, fingerprint: string) =>
    safeInvoke<void>('plugin_update_retrust_publisher', { pluginId, version, fingerprint }, () => undefined),
  pluginRollbackApprove: (pluginId: string, version: string, confirmed: boolean) =>
    safeInvoke<string>('plugin_rollback_approve', { pluginId, version, confirmed }, () => version),
  pluginUpdateApprove: (pluginId: string, version: string) =>
    safeInvoke<string>('plugin_update_approve', { pluginId, version }, () => version),
  pluginUpdateDecline: (pluginId: string) =>
    safeInvoke<void>('plugin_update_decline', { pluginId }, () => undefined),

  /** Answers a plugin's confirmation. Late, repeated or unknown answers are ignored by the host. */
  pluginConfirmationAnswer: (id: string, approved: boolean, remember: boolean) =>
    safeInvoke<void>('plugin_confirmation_answer', { id, approved, remember }, () => undefined),

  /** Answers a host-owned plugin folder chooser; null means decline. */
  pluginFolderSelectionAnswer: (id: string, selectedRoot: string | null) =>
    safeInvoke<void>('plugin_folder_selection_answer', { id, selectedRoot }, () => undefined),

  /** Confirms or declines the exact folder scope shown in the approval modal. */
  pluginFolderApprovalAnswer: (id: string, approved: boolean) =>
    safeInvoke<void>('plugin_folder_approval_answer', { id, approved }, () => undefined),

  pluginFolderApprovals: (pluginId: string, publisherFingerprint: string) =>
    safeInvoke<PluginFolderApproval[]>(
      'plugin_folder_approvals', { pluginId, publisherFingerprint }, () => [],
    ),

  pluginFolderRevoke: (pluginId: string, publisherFingerprint: string, id: string) =>
    safeInvoke<boolean>(
      'plugin_folder_revoke', { pluginId, publisherFingerprint, id }, () => false,
    ),

  pluginFolderChange: (pluginId: string, publisherFingerprint: string, id: string) =>
    safeInvoke<void>(
      'plugin_folder_change', { pluginId, publisherFingerprint, id }, () => undefined,
    ),

  pluginLibraryRootSetWatching: (pluginId: string, publisherFingerprint: string, id: string, watching: boolean) =>
    safeInvoke<boolean>(
      'plugin_library_root_set_watching', { pluginId, publisherFingerprint, id, watching }, () => false,
    ),

  /** What the user allowed one plugin to start without asking, in the active profile. */
  pluginRememberedConfirmations: (pluginId: string, publisherFingerprint: string) =>
    safeInvoke<RememberedConfirmation[]>(
      'plugin_remembered_confirmations',
      { pluginId, publisherFingerprint },
      () => [],
    ),

  /** Forgets one remembered confirmation, so the next request asks the user again. */
  pluginRevokeConfirmation: (pluginId: string, publisherFingerprint: string, id: string) =>
    safeInvoke<boolean>(
      'plugin_revoke_confirmation',
      { pluginId, publisherFingerprint, id },
      () => false,
    ),

  providerSignIn: (providerId: string) =>
    safeInvoke<void>('provider_sign_in', { providerId }, () => undefined),

  providerCancelSignIn: (providerId: string) =>
    safeInvoke<void>('provider_cancel_sign_in', { providerId }, () => undefined),

  providerFocusSignIn: (providerId: string) =>
    safeInvoke<void>('provider_focus_sign_in', { providerId }, () => undefined),

  providerSignOut: (providerId: string) =>
    safeInvoke<void>('provider_sign_out', { providerId }, () => undefined),

  providerCheck: (providerId: string) =>
    safeInvoke<void>('provider_check', { providerId }, () => undefined),

  /** The full achievement list, read from the local cache (no GameNode request). */
  achievementList: (key: string, filter: AchievementFilter) =>
    safeInvoke<AchievementListView | null>(
      'achievement_list',
      { key, filter },
      () => ({ connected: false, achievements: [] }),
    ),

  removeEntry: (key: string, allowRediscovery: boolean) =>
    safeInvoke<void>('remove_entry', { key, allowRediscovery }, () => undefined),

  // DRM-Free & Backups
  pickLocalExecutable: (repair?: string | null) =>
    safeInvoke<void>('pick_local_executable', { repair: repair ?? null }, () => undefined),

  addLocalExecutable: (path?: string | null) =>
    safeInvoke<void>('add_local_executable', { path: path ?? null }, () => undefined),

  editLocalGame: (value: Registration) =>
    safeInvoke<void>('edit_local_game', { value }, () => undefined),

  repairLocalGame: (id: string, path?: string | null) =>
    safeInvoke<void>('repair_local_game', { id, path: path ?? null }, () => undefined),

  removeLocalGame: (id: string, allowRediscovery: boolean) =>
    safeInvoke<void>('remove_local_game', { id, allowRediscovery }, () => undefined),

  backupLocalNow: (id: string) =>
    safeInvoke<void>('backup_local_now', { id }, () => undefined),

  setLocalBackupSettings: (automatic: boolean, destination: string) =>
    safeInvoke<void>('set_local_backup_settings', { automatic, destination }, () => undefined),

  pickSaveDestination: () =>
    safeInvoke<string | null>('pick_save_destination', undefined, () => null),

  setLocalProfileAutomatic: (id: string, automatic: boolean) =>
    safeInvoke<void>('set_local_profile_automatic', { id, automatic }, () => undefined),

  configureLocalSaves: (id: string, location: string) =>
    safeInvoke<void>('configure_local_saves', { id, location }, () => undefined),

  pickLocalSaves: (id: string, folder: boolean) =>
    safeInvoke<void>('pick_local_saves', { id, folder }, () => undefined),

  chooseLocalSaveProfile: (id: string, title: string) =>
    safeInvoke<void>('choose_local_save_profile', { id, title }, () => undefined),

  watchLocalFolder: (path: string) =>
    safeInvoke<void>('watch_local_folder', { path }, () => undefined),

  pickLocalWatchFolder: () =>
    safeInvoke<void>('pick_local_watch_folder', undefined, () => undefined),

  removeLocalWatch: (id: string) =>
    safeInvoke<void>('remove_local_watch', { id }, () => undefined),

  setLocalCandidateExecutable: (fingerprint: string, executable: string) =>
    safeInvoke<void>('set_local_candidate_executable', { fingerprint, executable }, () => undefined),

  rescanLocalFolders: () =>
    safeInvoke<void>('rescan_local_folders', undefined, () => undefined),

  selectLocalCandidate: (fingerprint: string) =>
    safeInvoke<void>('select_local_candidate', { fingerprint }, () => undefined),

  reviewSelectedLocalCandidates: (add: boolean) =>
    safeInvoke<void>('review_selected_local_candidates', { add }, () => undefined),

  resetLocalIgnores: () =>
    safeInvoke<void>('reset_local_ignores', undefined, () => undefined),

  loadLocalHistory: (id: string) =>
    safeInvoke<void>('load_local_history', { id }, () => undefined),

  previewLocalRestore: (key: string) =>
    safeInvoke<void>('preview_local_restore', { key }, () => undefined),

  confirmLocalRestore: () =>
    safeInvoke<void>('confirm_local_restore', undefined, () => undefined),

  cancelLocalRestore: () =>
    safeInvoke<void>('cancel_local_restore', undefined, () => undefined),

  resolveLocalClaim: (transfer: boolean) =>
    safeInvoke<void>('resolve_local_claim', { transfer }, () => undefined),

  cancelLocalClaim: () =>
    safeInvoke<void>('cancel_local_claim', undefined, () => undefined),

  dismissLocalDiscovered: () =>
    safeInvoke<void>('dismiss_local_discovered', undefined, () => undefined),

  // Game Tools (OptiScaler)
  openToolsForSelected: () =>
    safeInvoke<void>('open_tools_for_selected', undefined, () => undefined),

  openTools: (entryKey?: string | null) =>
    safeInvoke<void>('open_tools', { entryKey: entryKey ?? null }, () => undefined),

  closeTools: () =>
    safeInvoke<void>('close_tools', undefined, () => undefined),

  selectToolChannel: (channel: ToolChannel) =>
    safeInvoke<void>('select_tool_channel', { channel }, () => undefined),

  refreshToolVersions: () =>
    safeInvoke<void>('refresh_tool_versions', undefined, () => undefined),

  selectToolVersion: (version: string) =>
    safeInvoke<void>('select_tool_version', { version }, () => undefined),

  selectToolTargetExe: (relativeExe: string) =>
    safeInvoke<void>('select_tool_target_exe', { relativeExe }, () => undefined),

  selectToolProxy: (proxy: string) =>
    safeInvoke<void>('select_tool_proxy', { proxy }, () => undefined),

  setToolAnticheatConfirmed: (confirmed: boolean) =>
    safeInvoke<void>('set_tool_anticheat_confirmed', { confirmed }, () => undefined),

  setToolResetConfig: (reset: boolean) =>
    safeInvoke<void>('set_tool_reset_config', { reset }, () => undefined),

  installOrUpdateTool: (isUpdate: boolean) =>
    safeInvoke<void>('install_or_update_tool', { isUpdate }, () => undefined),

  uninstallTool: () =>
    safeInvoke<void>('uninstall_tool', undefined, () => undefined),

  openToolSource: (url: string) =>
    safeInvoke<void>('open_tool_source', { url }, () => undefined),

  clearToolCache: () =>
    safeInvoke<void>('clear_tool_cache', undefined, () => undefined),

  // PC ports. Every command is async in the host and never blocks the main thread.
  /** Starts the first catalog fetch when the user opens PC ports. Idempotent. */
  portsOpen: () =>
    safeInvoke<void>('ports_open', undefined, () => undefined),

  /** Manual Refresh. The current snapshot stays visible until a validated replacement lands. */
  portsRefresh: () =>
    safeInvoke<void>('ports_refresh', undefined, () => undefined),

  /** Lazy entry detail (releases, assets, withdrawal state). Arg: `{ catalogId }`. */
  portsDetail: (catalogId: string) =>
    safeInvoke<PortDetailView | null>('ports_detail', { catalogId }, () => null),

  /** Native parent-folder picker. Arg: `{ initialFolder }`; returns null when cancelled. */
  portsPickParent: (initialFolder?: string | null) =>
    safeInvoke<string | null>(
      'ports_pick_parent',
      { initialFolder: initialFolder ?? null },
      () => null,
    ),

  /**
   * Native picker plus host validation of one guided-setup slot. Arg: `{ request }`.
   * Hashing progress is sent on `ports:progress` with `request.setupId` in `jobId`.
   */
  portsPickSupplied: (request: PortPickSuppliedRequest) =>
    safeInvoke<PortSuppliedPick | null>(
      'ports_pick_supplied',
      { request },
      () => null,
    ),

  /**
   * Starts a background install. Arg: `{ request }`. Returns the job id, or null when the
   * request is invalid or Install is unavailable.
   */
  portsDismissPluginSetup: (requestId: string) =>
    safeInvoke<void>('ports_dismiss_plugin_setup', { requestId }, () => undefined),
  portsInstall: (request: PortInstallRequest) =>
    safeInvoke<string | null>('ports_install', { request }, () => null),

  /** Cancels a job. Arg: `{ jobId }`. */
  portsCancel: (jobId: string) =>
    safeInvoke<void>('ports_cancel', { jobId }, () => undefined),

  /** Marks the entry's current result seen, dropping it from the sidebar badge. Arg: `{ catalogId }`. */
  portsMarkSeen: (catalogId: string) =>
    safeInvoke<void>('ports_mark_seen', { catalogId }, () => undefined),

  /** Persists remembered view/filters/last folder. Arg: `{ patch }`. */
  portsSetPreferences: (patch: PortPreferencesPatch) =>
    safeInvoke<void>('ports_set_preferences', { patch }, () => undefined),

  /**
   * Follow-up for a completed install whose preferred executable is missing. Arg:
   * `{ jobId }`; returns the chosen executable inside the install folder or null.
   */
  portsPickExecutable: (jobId: string) =>
    safeInvoke<string | null>('ports_pick_executable', { jobId }, () => null),

  /** Opens an install folder in the OS file manager. Arg: `{ path }`. */
  openInstallFolder: (path: string) =>
    safeInvoke<void>('open_install_folder', { path }, () => undefined),

  /**
   * Settings Tools panel: pinned extraction tools with their licence, source, pinned version and
   * cache size, plus the cached release downloads. No arguments.
   */
  portsToolsCache: () =>
    safeInvoke<PortToolsCacheView>('ports_tools_cache', undefined, () => ({
      downloadsBytes: 0,
      helperTools: [],
      installActive: false,
    })),

  /**
   * Settings Tools panel: deletes the cached release downloads and pinned extraction tools.
   * Installed ports, their registrations and provenance are untouched. The host refuses with
   * `{ code: 'installInProgress' }` while a port install is queued, running or waiting for an
   * executable pick. Returns the recomputed cache view.
   */
  portsClearCache: () =>
    safeInvoke<PortToolsCacheView>('ports_clear_cache', undefined, () => ({
      downloadsBytes: 0,
      helperTools: [],
      installActive: false,
    })),
};
