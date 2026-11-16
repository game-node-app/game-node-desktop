import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getMockHost, mockEnabled } from './mock';
import { plainText } from './pluginNotices';

/** The Tauri event the host emits when a plugin's request needs the user's confirmation. */
export const PLUGIN_CONFIRMATION_EVENT = 'plugin:confirmation';
/** The event that says a confirmation stopped waiting (withdrawn by the plugin, or timed out). */
export const PLUGIN_CONFIRMATION_CLOSED_EVENT = 'plugin:confirmation-closed';

export type PluginConfirmationOperation = 'launch' | 'custom_launch' | 'install' | 'backup_restore';

export interface LaunchPlanReviewComponent {
  kind: string;
  id: string;
  ownerPluginId: string;
  ownerPublisher: string;
  version: string;
  revision: number;
}

export interface LaunchPlanReview {
  sourceId: string;
  requesterPluginId: string;
  requesterPublisher: string;
  requesterPackageVersion: string;
  target: string;
  components: LaunchPlanReviewComponent[];
  arguments: string[];
  workingDirectory: string;
  environmentSet: Record<string, string>;
  environmentUnset: string[];
  differences: string[];
  fingerprint: string;
}

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
  /** Complete, host-resolved launch parameters; values are initially masked in the modal. */
  launchPlan?: LaunchPlanReview;
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
  return value === 'launch' || value === 'custom_launch' || value === 'install' || value === 'backup_restore';
}

// Display control and bidirectional characters visibly so a literal launch value cannot disguise
// its boundaries in the review modal. React still renders the result only as text.
function launchLiteral(raw: unknown, max: number): string {
  if (typeof raw !== 'string') return '';
  let escaped = '';
  for (const char of raw) {
    const code = char.codePointAt(0) ?? 0;
    if (
      code < 0x20 ||
      (code >= 0x7f && code <= 0x9f) ||
      [0x061c, 0x200b, 0x200e, 0x200f, 0x2028, 0x2029, 0x202a, 0x202e, 0x2060, 0x2066, 0x2069, 0xfeff].includes(code)
    ) {
      escaped += `\\u{${code.toString(16)}}`;
    } else {
      escaped += char;
    }
    if (Array.from(escaped).length >= max) break;
  }
  return Array.from(escaped).slice(0, max).join('');
}

function cleanLaunchPlanReview(raw: unknown): LaunchPlanReview | null {
  if (!raw || typeof raw !== 'object') return null;
  const value = raw as Record<string, unknown>;
  const environment = value.environmentSet;
  if (
    !environment ||
    typeof environment !== 'object' ||
    Array.isArray(environment) ||
    !Array.isArray(value.components) ||
    !Array.isArray(value.arguments) ||
    !Array.isArray(value.environmentUnset) ||
    !Array.isArray(value.differences)
  ) return null;
  const components = value.components.slice(0, 128);
  const cleanedComponents = components.map((rawComponent) => {
    if (!rawComponent || typeof rawComponent !== 'object') return null;
    const component = rawComponent as Record<string, unknown>;
    return {
      kind: launchLiteral(component.kind, 128),
      id: launchLiteral(component.id, 512),
      ownerPluginId: launchLiteral(component.ownerPluginId, 256),
      ownerPublisher: launchLiteral(component.ownerPublisher, 256),
      version: launchLiteral(component.version, 256),
      revision: typeof component.revision === 'number' && Number.isFinite(component.revision)
        ? Math.max(0, Math.floor(component.revision))
        : 0,
    };
  });
  if (cleanedComponents.some((component) => component === null)) return null;
  const environmentSet = Object.fromEntries(
    Object.entries(environment as Record<string, unknown>)
      .slice(0, 256)
      .map(([key, item]) => [launchLiteral(key, 256), launchLiteral(item, 16 * 1024)]),
  );
  const strings = (item: unknown, maxLength: number, maxItems: number) =>
    Array.isArray(item)
      ? item.slice(0, maxItems).map((part) => launchLiteral(part, maxLength))
      : [];
  const fingerprint = typeof value.fingerprint === 'string' ? value.fingerprint : '';
  if (!/^v\d+:[0-9a-f]{64}$/.test(fingerprint)) return null;
  if (
    !launchLiteral(value.sourceId, 256) ||
    !launchLiteral(value.requesterPluginId, 256) ||
    !launchLiteral(value.requesterPublisher, 256) ||
    !launchLiteral(value.requesterPackageVersion, 128) ||
    !launchLiteral(value.target, 8192)
  ) return null;
  return {
    sourceId: launchLiteral(value.sourceId, 256),
    requesterPluginId: launchLiteral(value.requesterPluginId, 256),
    requesterPublisher: launchLiteral(value.requesterPublisher, 256),
    requesterPackageVersion: launchLiteral(value.requesterPackageVersion, 128),
    target: launchLiteral(value.target, 8192),
    components: cleanedComponents as LaunchPlanReviewComponent[],
    arguments: strings(value.arguments, 16 * 1024, 256),
    workingDirectory: launchLiteral(value.workingDirectory, 8192),
    environmentSet,
    environmentUnset: strings(value.environmentUnset, 256, 256),
    differences: strings(value.differences, 1024, 256),
    fingerprint,
  };
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
  let launchPlan: LaunchPlanReview | undefined;
  if (value.launchPlan !== undefined) {
    const cleaned = cleanLaunchPlanReview(value.launchPlan);
    if (!cleaned) return null;
    launchPlan = cleaned;
  }
  const launchOperation = value.operation === 'launch' || value.operation === 'custom_launch';
  if (launchOperation !== Boolean(launchPlan)) return null;
  return {
    id: value.id,
    pluginId,
    publisherFingerprint: FINGERPRINT.test(fingerprint) ? fingerprint : '',
    gameName: plainText(value.gameName, CONFIRMATION_GAME_MAX),
    operation: value.operation,
    details: Array.isArray(value.details)
      ? value.details.filter((item): item is string => typeof item === 'string').slice(0, 1002)
      : undefined,
    launchPlan,
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
