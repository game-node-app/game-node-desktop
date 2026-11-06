import { useEffect } from 'react';
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from '@tanstack/react-query';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { commands } from './commands';
import {
  createDefaultSnapshot,
  type AuthInput,
  type AuthView,
  type BootstrapState,
  type LaunchKind,
  type LibraryInput,
  type LibraryView,
  type LocalView,
  type PlaytimeSource,
  type Registration,
  type Settings,
  type Snapshot,
  type ToolChannel,
  type ToolsView,
} from './types';

export const SNAPSHOT_QUERY_KEY = ['snapshot'] as const;
export const BOOTSTRAP_QUERY_KEY = ['bootstrap'] as const;
export const ENGINE_SNAPSHOT_EVENT = 'engine:snapshot';

/**
 * Subscribes a QueryClient to coalesced `engine:snapshot` Tauri events.
 * Returns an async cleanup function.
 */
export async function subscribeToEngineSnapshots(
  queryClient: QueryClient,
): Promise<UnlistenFn> {
  return listen<Snapshot>(ENGINE_SNAPSHOT_EVENT, (event) => {
    queryClient.setQueryData<Snapshot>(SNAPSHOT_QUERY_KEY, event.payload);
  });
}

/**
 * React hook that initializes the `engine:snapshot` listener for the active QueryClient.
 */
export function useEngineSnapshotSubscription(): void {
  const queryClient = useQueryClient();

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | undefined;

    subscribeToEngineSnapshots(queryClient)
      .then((fn) => {
        if (active) {
          unlisten = fn;
        } else {
          fn();
        }
      })
      .catch(() => {
        // Ignore outside Tauri runtime (e.g. plain browser or unit test without event mock)
      });

    return () => {
      active = false;
      if (unlisten) {
        unlisten();
      }
    };
  }, [queryClient]);
}

export function useSnapshot<TSelected = Snapshot>(
  select?: (snapshot: Snapshot) => TSelected,
) {
  const fallback = createDefaultSnapshot();
  const query = useQuery({
    queryKey: SNAPSHOT_QUERY_KEY,
    queryFn: () => commands.getSnapshot(),
    staleTime: Infinity,
    gcTime: Infinity,
    placeholderData: fallback,
    select,
  });
  return {
    ...query,
    data: (query.data ?? (select ? select(fallback) : fallback)) as TSelected,
  };
}

export function useBootstrapState() {
  return useQuery<BootstrapState>({
    queryKey: BOOTSTRAP_QUERY_KEY,
    queryFn: () => commands.getBootstrapState(),
    staleTime: Infinity,
    gcTime: Infinity,
  });
}

export function useAuth(): {
  auth: AuthView;
  notice: Snapshot['notice'];
  storage: Snapshot['storage'];
  sync: Snapshot['sync'];
  playtimeSource: PlaytimeSource;
  playtimeError: boolean;
} {
  const { data } = useSnapshot((s) => ({
    auth: s.auth,
    notice: s.notice,
    storage: s.storage,
    sync: s.sync,
    playtimeSource: s.playtimeSource,
    playtimeError: s.playtimeError,
  }));
  return data;
}

export function useLibraryView(): LibraryView | null | undefined {
  const { data } = useSnapshot((s) => s.library);
  return data;
}

export function useLocalGames(): LocalView {
  const { data } = useSnapshot((s) => s.localGames);
  return data;
}

export function useTools(): ToolsView {
  const { data } = useSnapshot((s) => s.tools);
  return data;
}

export function useSettings(): Settings {
  const { data } = useSnapshot((s) => s.settings);
  return data;
}

export function useDispatchAuthMutation() {
  return useMutation({
    mutationFn: (input: AuthInput) => commands.dispatchAuth(input),
  });
}

export function useLibraryInputMutation() {
  return useMutation({
    mutationFn: (input: LibraryInput) => commands.libraryInput(input),
  });
}

export function useActivateEntryMutation() {
  return useMutation({
    mutationFn: () => commands.activateEntry(),
  });
}

export function useLaunchEntryMutation() {
  return useMutation({
    mutationFn: ({ entryKey, kind }: { entryKey: string; kind: LaunchKind }) =>
      commands.openEntry(entryKey, kind),
  });
}

export function useLaunchSelectedMutation() {
  return useMutation({
    mutationFn: (kind: LaunchKind) => commands.launchSelected(kind),
  });
}

export function useRescanLibraryMutation() {
  return useMutation({
    mutationFn: () => commands.rescanLibrary(),
  });
}

export function useEditLocalGameMutation() {
  return useMutation({
    mutationFn: (registration: Registration) => commands.editLocalGame(registration),
  });
}

export function useSelectToolChannelMutation() {
  return useMutation({
    mutationFn: (channel: ToolChannel) => commands.selectToolChannel(channel),
  });
}
