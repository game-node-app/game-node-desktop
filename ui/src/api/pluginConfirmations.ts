import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getMockHost, mockEnabled } from './mock';
import { plainText } from './pluginNotices';

/** The Tauri event the host emits when a plugin's request needs the user's confirmation. */
export const PLUGIN_CONFIRMATION_EVENT = 'plugin:confirmation';
/** The event that says a confirmation stopped waiting (withdrawn by the plugin, or timed out). */
export const PLUGIN_CONFIRMATION_CLOSED_EVENT = 'plugin:confirmation-closed';

export type PluginConfirmationOperation = 'launch' | 'install' | 'backup_restore';

/**
 * A plugin asks to start something on the user's computer. `id` is the host's. `pluginId` is the
 * plugin the host authenticated; `gameName` is the name the plugin's provider gave the game. Both
 * are untrusted text and are shown as plain text.
 */
export interface PluginConfirmation {
  id: string;
  pluginId: string;
  publisherFingerprint: string;
  gameName: string;
  operation: PluginConfirmationOperation;
  timeoutSeconds: number;
  /** Full host-resolved restore targets; never interpreted as markup or commands. */
  details?: string[];
}

/** A confirmation the user asked the host to remember for one plugin and one game. */
export interface RememberedConfirmation {
  id: string;
  gameName: string;
  operation: PluginConfirmationOperation;
  /** Unix seconds. */
  approvedAt: number;
}

export const CONFIRMATION_PLUGIN_MAX = 128;
export const CONFIRMATION_GAME_MAX = 120;
const FINGERPRINT = /^[0-9a-f]{64}$/;

function isOperation(value: unknown): value is PluginConfirmationOperation {
  return value === 'launch' || value === 'install' || value === 'backup_restore';
}

/**
 * Checks and cleans what the host sent before anything is shown: a malformed request is dropped,
 * and the text is made plain (no controls or disguising characters, a bounded length, one line),
 * so the dialog never depends on the host's own checks alone.
 */
export function cleanConfirmation(raw: unknown): PluginConfirmation | null {
  if (!raw || typeof raw !== 'object') return null;
  const value = raw as Record<string, unknown>;
  if (typeof value.id !== 'string' || value.id === '' || !isOperation(value.operation)) return null;
  const pluginId = plainText(value.pluginId, CONFIRMATION_PLUGIN_MAX);
  if (!pluginId) return null;
  const fingerprint = typeof value.publisherFingerprint === 'string' ? value.publisherFingerprint : '';
  return {
    id: value.id,
    pluginId,
    publisherFingerprint: FINGERPRINT.test(fingerprint) ? fingerprint : '',
    gameName: plainText(value.gameName, CONFIRMATION_GAME_MAX),
    operation: value.operation,
    details: Array.isArray(value.details)
      ? value.details.filter((item): item is string => typeof item === 'string').slice(0, 1002)
      : undefined,
    timeoutSeconds:
      typeof value.timeoutSeconds === 'number' && Number.isFinite(value.timeoutSeconds)
        ? Math.max(0, Math.floor(value.timeoutSeconds))
        : 0,
  };
}

/** A publisher fingerprint shortened for people: the first and last bytes, in groups. */
export function shortFingerprint(fingerprint: string): string {
  if (!FINGERPRINT.test(fingerprint)) return '';
  const groups = fingerprint.match(/.{2}/g) ?? [];
  return `${groups.slice(0, 4).join(':')}…${groups.slice(-2).join(':')}`;
}

/** Subscribes to confirmations that need the user; the mock host feeds the same handler. */
export async function subscribeToPluginConfirmations(
  handler: (confirmation: unknown) => void,
): Promise<UnlistenFn> {
  if (mockEnabled) {
    return (await getMockHost()).subscribePluginConfirmations(handler);
  }
  return listen<unknown>(PLUGIN_CONFIRMATION_EVENT, (event) => handler(event.payload));
}

/** Subscribes to the ids of confirmations that stopped waiting. */
export async function subscribeToPluginConfirmationsClosed(
  handler: (id: string) => void,
): Promise<UnlistenFn> {
  if (mockEnabled) {
    return (await getMockHost()).subscribePluginConfirmationsClosed(handler);
  }
  return listen<string>(PLUGIN_CONFIRMATION_CLOSED_EVENT, (event) => {
    if (typeof event.payload === 'string') handler(event.payload);
  });
}
