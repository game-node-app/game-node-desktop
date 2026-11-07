import { invoke } from '@tauri-apps/api/core';
import { getMockHost, mockEnabled } from './mock';
import type {
  AppInfo,
  AuthInput,
  LaunchKind,
  LibraryInput,
  PlaytimeSource,
  Registration,
  Snapshot,
  ToolChannel,
  WindowGeometry,
} from './types';
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

  // Store accounts (Steam)
  steamSignIn: () =>
    safeInvoke<void>('steam_sign_in', undefined, () => undefined),

  steamCancelSignIn: () =>
    safeInvoke<void>('steam_cancel_sign_in', undefined, () => undefined),

  steamFocusSignIn: () =>
    safeInvoke<void>('steam_focus_sign_in', undefined, () => undefined),

  steamSignOut: () =>
    safeInvoke<void>('steam_sign_out', undefined, () => undefined),

  steamCheck: () =>
    safeInvoke<void>('steam_check', undefined, () => undefined),

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
};
