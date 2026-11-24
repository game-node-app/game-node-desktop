import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getMockHost, mockEnabled } from './mock';
import { plainText } from './pluginNotices';

export const PLUGIN_RUNTIME_SELECT_EVENT = 'plugin:runtime-approval-select';
export const PLUGIN_RUNTIME_CLOSED_EVENT = 'plugin:runtime-approval-closed';

export interface RuntimeExecutableCandidate {
  displayName: string;
  packageManager: string;
  packageId: string;
  version: string;
  /** The exact program path the user would approve; shown, never sent back to the plugin. */
  path: string;
}

export interface RuntimeSelectionPrompt {
  id: string;
  pluginId: string;
  publisherFingerprint: string;
  runtimeName: string;
  runtimeVersion: string;
  platform: string;
  supportedFormats: string[];
  hints: string[];
  candidates: RuntimeExecutableCandidate[];
}

/** One active runtime executable approval, as the plugin settings list shows it. */
export interface PluginRuntimeApproval {
  /** The owned runtime this approval belongs to. */
  runtimeId: string;
  /** The canonical executable path the user approved. */
  path: string;
  /** Unix seconds. */
  approvedAt: number;
}

export function cleanRuntimeSelection(raw: unknown): RuntimeSelectionPrompt | null {
  if (!raw || typeof raw !== 'object') return null;
  const row = raw as Record<string, unknown>;
  if (typeof row.id !== 'string' || row.id.length < 16 || row.id.length > 128) return null;
  const pluginId = plainText(row.pluginId, 256);
  if (!pluginId) return null;
  const publisherFingerprint = typeof row.publisherFingerprint === 'string'
    && /^[0-9a-f]{64}$/.test(row.publisherFingerprint)
    ? row.publisherFingerprint
    : '';
  const candidates = Array.isArray(row.candidates)
    ? row.candidates.slice(0, 32).flatMap((entry) => {
      if (!entry || typeof entry !== 'object') return [];
      const candidate = entry as Record<string, unknown>;
      const path = plainText(candidate.path, 4096);
      if (!path) return [];
      return [{
        displayName: plainText(candidate.displayName, 256),
        packageManager: plainText(candidate.packageManager, 128),
        packageId: plainText(candidate.packageId, 256),
        version: plainText(candidate.version, 128),
        path,
      }];
    })
    : [];
  return {
    id: row.id,
    pluginId,
    publisherFingerprint,
    runtimeName: plainText(row.runtimeName, 256),
    runtimeVersion: plainText(row.runtimeVersion, 128),
    platform: plainText(row.platform, 64),
    supportedFormats: Array.isArray(row.supportedFormats)
      ? row.supportedFormats
        .filter((value): value is string => typeof value === 'string')
        .map((value) => plainText(value, 128))
        .filter(Boolean)
        .slice(0, 32)
      : [],
    hints: Array.isArray(row.hints)
      ? row.hints
        .filter((value): value is string => typeof value === 'string')
        .map((value) => plainText(value, 256))
        .filter(Boolean)
        .slice(0, 32)
      : [],
    candidates,
  };
}

export async function subscribeToRuntimeSelections(handler: (prompt: unknown) => void): Promise<UnlistenFn> {
  if (mockEnabled) return (await getMockHost()).subscribeRuntimeSelections(handler);
  return listen<unknown>(PLUGIN_RUNTIME_SELECT_EVENT, (event) => handler(event.payload));
}

export async function subscribeToRuntimePromptsClosed(handler: (id: string) => void): Promise<UnlistenFn> {
  if (mockEnabled) return (await getMockHost()).subscribeRuntimePromptsClosed(handler);
  return listen<string>(PLUGIN_RUNTIME_CLOSED_EVENT, (event) => handler(event.payload));
}
