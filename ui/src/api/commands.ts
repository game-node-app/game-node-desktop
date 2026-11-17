import { invoke } from '@tauri-apps/api/core';
import { getMockHost, mockEnabled } from './mock';
import type {
  AchievementFilter,
  AchievementListView,
  AchievementSort,
  AppInfo,
  AppPreferencePatch,
  AuthInput,
  BackupUsage,
  GameNodeLink,
  GameNodePage,
  LaunchKind,
  LibraryInput,
  PluginInfo,
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
      systemLocale: 'en',
      version: '0.1.0',
      buildDate: '2026-01-01',
      osVersion: 'unknown',
      arch: 'unknown',
      backgroundSupported: false,
      logDirectory: null,
    })),

  getRecentLogs: () =>
    safeInvoke<string>('get_recent_logs', undefined, () => '(no log available in the web preview)'),

  /** Build, OS/arch, locale, plugins and detected stores, already redacted by the host. */
  systemInfoText: () =>
    safeInvoke<string>(
      'system_info_text',
      undefined,
      () => 'GameNode Desktop (web preview)',
    ),

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

  /** Cached cover file names for the signed-out screen's mural, most recent first. */
  signInCoverWall: () =>
    safeInvoke<string[]>('sign_in_cover_wall', undefined, () => []),

  setSyncEnabled: (enabled: boolean) =>
    safeInvoke<void>('set_sync_enabled', { enabled }, () => undefined),

  /** Rescan the library and submit every held total; refused when sync is off or unverified. */
  syncNow: () => safeInvoke<void>('sync_now', undefined, () => undefined),

  setPlaytimeSource: (source: PlaytimeSource) =>
    safeInvoke<void>('set_playtime_source', { source }, () => undefined),

  /** Applies one Settings preference; the host validates, persists and publishes a snapshot. */
  setAppPreference: (patch: AppPreferencePatch) =>
    safeInvoke<void>('set_app_preference', { patch }, () => undefined),

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

  /** Opens one fixed GameNode website link; the UI never passes a URL. */
  openGameNodeLink: (kind: GameNodeLink) =>
    safeInvoke<void>('open_gamenode_link', { kind }, () => undefined),

  activateEntry: () =>
    safeInvoke<void>('activate_entry', undefined, () => undefined),

  openEntry: (entryKey: string, kind: LaunchKind) =>
    safeInvoke<void>('open_entry', { entryKey, kind }, () => undefined),

  launchSelected: (kind: LaunchKind) =>
    safeInvoke<void>('launch_selected', { kind }, () => undefined),

  dismissLaunch: () =>
    safeInvoke<void>('dismiss_launch', undefined, () => undefined),

  rescanLibrary: () =>
    safeInvoke<void>('rescan_library', undefined, () => undefined),

  pluginInventory: () =>
    safeInvoke<PluginInfo[]>('plugin_inventory', undefined, () => []),

  /** Stops one plugin process for this run; a pending sign-in is cancelled first. */
  pluginStop: (providerId: string) =>
    safeInvoke<void>('plugin_stop', { providerId }, () => undefined),

  /** Starts a stopped plugin with a fresh session and restores its account. */
  pluginStart: (providerId: string) =>
    safeInvoke<void>('plugin_start', { providerId }, () => undefined),

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
  achievementList: (key: string, filter: AchievementFilter, sort: AchievementSort) =>
    safeInvoke<AchievementListView | null>(
      'achievement_list',
      { key, filter, sort },
      () => null,
    ),

  /** Opens a provider's public achievements page. The host resolves the URL. */
  openAchievementsPage: (entryKey: string) =>
    safeInvoke<void>('open_achievements_page', { entryKey }, () => undefined),

  removeEntry: (key: string, allowRediscovery: boolean) =>
    safeInvoke<void>('remove_entry', { key, allowRediscovery }, () => undefined),

  // DRM-Free & Backups
  pickLocalExecutable: (repair?: string | null) =>
    safeInvoke<void>('pick_local_executable', { repair: repair ?? null }, () => undefined),

  /** Opens the image picker; resolves to the pending cover file, or null when cancelled. */
  pickLocalCover: (entryKey: string) =>
    safeInvoke<string | null>('pick_local_cover', { entryKey }, () => null),

  /** Drops an uncommitted cover after the editor closes without saving. */
  discardLocalCover: (entryKey: string) =>
    safeInvoke<void>('discard_local_cover', { entryKey }, () => undefined),

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

  setLocalBackupSettings: (automatic: boolean, destination: string, retention: number) =>
    safeInvoke<void>(
      'set_local_backup_settings',
      { automatic, destination, retention },
      () => undefined,
    ),

  /** Space used by local backups per game, computed on a blocking worker. */
  localBackupUsage: () =>
    safeInvoke<BackupUsage>('local_backup_usage', undefined, () => ({
      totalBytes: 0,
      totalVersions: 0,
      games: [],
    })),

  setLocalBackupRetention: (entryKey: string, retention: number) =>
    safeInvoke<void>('set_local_backup_retention', { entryKey, retention }, () => undefined),

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

  /** Check one watched folder on the discovery worker. */
  checkLocalWatch: (id: string) =>
    safeInvoke<void>('check_local_watch', { id }, () => undefined),

  selectLocalCandidate: (fingerprint: string) =>
    safeInvoke<void>('select_local_candidate', { fingerprint }, () => undefined),

  reviewSelectedLocalCandidates: (add: boolean, allowRediscovery: boolean) =>
    safeInvoke<void>(
      'review_selected_local_candidates',
      { add, allowRediscovery },
      () => undefined,
    ),

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

  /** Opens the executable picker for the open game's folder and selects the chosen file. */
  pickToolTargetExe: () =>
    safeInvoke<void>('pick_tool_target_exe', undefined, () => undefined),

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
