import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getMockHost, mockEnabled } from './mock';
import { plainText } from './pluginNotices';

export const PLUGIN_FOLDER_SELECT_EVENT = 'plugin:folder-approval-select';
export const PLUGIN_FOLDER_CONFIRM_EVENT = 'plugin:folder-approval-confirm';
export const PLUGIN_FOLDER_CLOSED_EVENT = 'plugin:folder-approval-closed';

export interface DeclaredFolderPrompt {
  path: string;
  description: string;
  exists: boolean;
}

export interface RootSelectionPrompt {
  id: string;
  pluginId: string;
  publisherFingerprint: string;
  installationName: string;
  kind: string;
  operations: string[];
  previouslyApproved: { root: string; operations: string[] } | null;
  suggested: string | null;
  preselected: string | null;
  declared: DeclaredFolderPrompt[];
}

export interface RootConfirmationPrompt {
  id: string;
  pluginId: string;
  publisherFingerprint: string;
  installationName: string;
  kind: string;
  operations: string[];
  root: string;
  declaredByPlugin: boolean;
  affected: Array<{ name: string; gameId: number | null }>;
}

export interface PluginFolderApproval {
  id: string;
  pluginId: string;
  publisherFingerprint: string;
  installationName: string;
  kind: string;
  root: string;
  operations: string[];
  affectedGames: string[];
  revoked: boolean;
  declaredByPlugin: boolean;
  logicalLibrary: boolean;
  libraryId: string | null;
  watching: boolean | null;
}

function cleanBase(raw: unknown): { id: string; pluginId: string; publisherFingerprint: string } | null {
  if (!raw || typeof raw !== 'object') return null;
  const row = raw as Record<string, unknown>;
  if (typeof row.id !== 'string' || row.id.length < 16 || row.id.length > 128) return null;
  const pluginId = plainText(row.pluginId, 256);
  if (!pluginId) return null;
  const publisherFingerprint = typeof row.publisherFingerprint === 'string'
    && /^[0-9a-f]{64}$/.test(row.publisherFingerprint)
    ? row.publisherFingerprint
    : '';
  return { id: row.id, pluginId, publisherFingerprint };
}

function operations(raw: unknown): string[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((value): value is string => typeof value === 'string')
    .filter((value) => ['read', 'write', 'delete', 'plugin_discovery', 'core_executable_discovery'].includes(value))
    .slice(0, 5);
}

export function cleanRootSelection(raw: unknown): RootSelectionPrompt | null {
  const base = cleanBase(raw);
  if (!base) return null;
  const row = raw as Record<string, unknown>;
  const declared = Array.isArray(row.declared)
    ? row.declared.slice(0, 16).flatMap((entry) => {
      if (!entry || typeof entry !== 'object') return [];
      const folder = entry as Record<string, unknown>;
      const path = plainText(folder.path, 4096);
      if (!path) return [];
      return [{ path, description: plainText(folder.description, 512), exists: folder.exists === true }];
    })
    : [];
  return {
    ...base,
    installationName: plainText(row.installationName, 256),
    kind: plainText(row.kind, 64),
    operations: operations(row.operations),
    previouslyApproved: row.previouslyApproved && typeof row.previouslyApproved === 'object'
      ? {
        root: plainText((row.previouslyApproved as Record<string, unknown>).root, 4096),
        operations: operations((row.previouslyApproved as Record<string, unknown>).operations),
      }
      : null,
    suggested: typeof row.suggested === 'string' ? plainText(row.suggested, 4096) : null,
    preselected: typeof row.preselected === 'string' ? plainText(row.preselected, 4096) : null,
    declared,
  };
}

export function cleanRootConfirmation(raw: unknown): RootConfirmationPrompt | null {
  const base = cleanBase(raw);
  if (!base) return null;
  const row = raw as Record<string, unknown>;
  return {
    ...base,
    installationName: plainText(row.installationName, 256),
    kind: plainText(row.kind, 64),
    operations: operations(row.operations),
    root: plainText(row.root, 4096),
    declaredByPlugin: row.declaredByPlugin === true,
    affected: Array.isArray(row.affected)
      ? row.affected.slice(0, 500).flatMap((entry) => {
        if (!entry || typeof entry !== 'object') return [];
        const item = entry as Record<string, unknown>;
        const name = plainText(item.name, 256);
        return name ? [{ name, gameId: typeof item.gameId === 'number' ? item.gameId : null }] : [];
      })
      : [],
  };
}

export async function subscribeToRootSelections(handler: (prompt: unknown) => void): Promise<UnlistenFn> {
  if (mockEnabled) return (await getMockHost()).subscribeRootSelections(handler);
  return listen<unknown>(PLUGIN_FOLDER_SELECT_EVENT, (event) => handler(event.payload));
}

export async function subscribeToRootConfirmations(handler: (prompt: unknown) => void): Promise<UnlistenFn> {
  if (mockEnabled) return (await getMockHost()).subscribeRootConfirmations(handler);
  return listen<unknown>(PLUGIN_FOLDER_CONFIRM_EVENT, (event) => handler(event.payload));
}

export async function subscribeToRootPromptsClosed(handler: (id: string) => void): Promise<UnlistenFn> {
  if (mockEnabled) return (await getMockHost()).subscribeRootPromptsClosed(handler);
  return listen<string>(PLUGIN_FOLDER_CLOSED_EVENT, (event) => handler(event.payload));
}
