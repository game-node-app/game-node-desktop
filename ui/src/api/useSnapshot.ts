/// <reference types="vite/client" />
import {
  createContext,
  createElement,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from 'react';
import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { commands, isTauriRuntime } from './commands';
import { getMockHost, mockEnabled } from './mock';
import {
  createDefaultSnapshot,
  type AppInfo,
  type AuthView,
  type LibraryView,
  type LocalView,
  type PlaytimeSource,
  type Settings,
  type Snapshot,
  type ToolsView,
} from './types';

export const SNAPSHOT_QUERY_KEY = ['snapshot'] as const;
export const APP_INFO_QUERY_KEY = ['app-info'] as const;
export const LICENSE_NOTICES_QUERY_KEY = ['license-notices'] as const;
export const ENGINE_SNAPSHOT_EVENT = 'engine:snapshot';

/** Shared placeholder snapshot. Read-only: never mutate. */
const DEFAULT_SNAPSHOT: Snapshot = createDefaultSnapshot();

/**
 * Subscribes a QueryClient to coalesced `engine:snapshot` Tauri events.
 * Returns an async cleanup function.
 */
export async function subscribeToEngineSnapshots(
  queryClient: QueryClient,
): Promise<UnlistenFn> {
  if (mockEnabled) {
    return (await getMockHost()).subscribe((snapshot) => {
      queryClient.setQueryData<Snapshot>(SNAPSHOT_QUERY_KEY, snapshot);
    });
  }
  return listen<Snapshot>(ENGINE_SNAPSHOT_EVENT, (event) => {
    queryClient.setQueryData<Snapshot>(SNAPSHOT_QUERY_KEY, event.payload);
  });
}

/**
 * React hook that attaches the `engine:snapshot` listener for the active QueryClient.
 * Returns true once the listener is registered (or the attempt failed), so callers can
 * avoid fetching a snapshot that the event stream might overtake.
 */
export function useEngineSnapshotSubscription(): boolean {
  const queryClient = useQueryClient();
  const [ready, setReady] = useState(false);

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
      .catch((error) => {
        // A plain browser without the event mock is expected to fail; only report
        // failures inside the real Tauri runtime.
        if (isTauriRuntime() && import.meta.env.DEV) {
          console.error('[snapshot] failed to subscribe to engine:snapshot', error);
        }
      })
      .finally(() => {
        if (active) {
          setReady(true);
        }
      });

    return () => {
      active = false;
      unlisten?.();
    };
  }, [queryClient]);

  return ready;
}

const SnapshotStreamContext = createContext(true);

/** Attaches the engine bridge before any snapshot query runs. */
export function SnapshotStreamProvider({ children }: { children: ReactNode }) {
  const ready = useEngineSnapshotSubscription();
  return createElement(
    SnapshotStreamContext.Provider,
    { value: ready },
    children,
  );
}

export function useSnapshot<TSelected = Snapshot>(
  select?: (snapshot: Snapshot) => TSelected,
) {
  const queryClient = useQueryClient();
  const streamReady = useContext(SnapshotStreamContext);

  const query = useQuery({
    queryKey: SNAPSHOT_QUERY_KEY,
    queryFn: async () => {
      const before = queryClient.getQueryData<Snapshot>(SNAPSHOT_QUERY_KEY);
      const snapshot = await commands.getSnapshot();
      // An engine:snapshot landing while this IPC call was in flight replaces the cache
      // entry with a new object. That event is newer than this command reply.
      const after = queryClient.getQueryData<Snapshot>(SNAPSHOT_QUERY_KEY);
      return after !== before ? (after ?? snapshot) : snapshot;
    },
    staleTime: Infinity,
    gcTime: Infinity,
    placeholderData: DEFAULT_SNAPSHOT,
    select,
    enabled: streamReady,
  });

  useEffect(() => {
    if (query.error && import.meta.env.DEV) {
      console.error('[snapshot] get_snapshot failed', query.error);
    }
  }, [query.error]);

  return {
    ...query,
    data: (query.data ?? (select ? select(DEFAULT_SNAPSHOT) : DEFAULT_SNAPSHOT)) as TSelected,
  };
}

export function useAppInfo() {
  return useQuery<AppInfo>({
    queryKey: APP_INFO_QUERY_KEY,
    queryFn: () => commands.getAppInfo(),
    staleTime: Infinity,
    gcTime: Infinity,
  });
}

/**
 * Mount only from the notices viewer: this transfers ~4.8 MB over IPC.
 * `gcTime` stays at the client default (5 min) so closing the modal frees it.
 */
export function useLicenseNotices() {
  return useQuery<string>({
    queryKey: LICENSE_NOTICES_QUERY_KEY,
    queryFn: () => commands.getLicenseNotices(),
    staleTime: Infinity,
  });
}

const selectLibrary = (s: Snapshot) => s.library;
const selectLocalGames = (s: Snapshot) => s.localGames;
const selectTools = (s: Snapshot) => s.tools;
const selectSettings = (s: Snapshot) => s.settings;
const selectAuth = (s: Snapshot) => ({
  auth: s.auth,
  notice: s.notice,
  storage: s.storage,
  sync: s.sync,
  playtimeSource: s.playtimeSource,
  playtimeError: s.playtimeError,
});

export function useAuth(): {
  auth: AuthView;
  notice: Snapshot['notice'];
  storage: Snapshot['storage'];
  sync: Snapshot['sync'];
  playtimeSource: PlaytimeSource;
  playtimeError: boolean;
} {
  const { data } = useSnapshot(selectAuth);
  return data;
}

export function useLibraryView(): LibraryView | null | undefined {
  const { data } = useSnapshot(selectLibrary);
  return data;
}

export function useLocalGames(): LocalView {
  const { data } = useSnapshot(selectLocalGames);
  return data;
}

export function useTools(): ToolsView {
  const { data } = useSnapshot(selectTools);
  return data;
}

export function useSettings(): Settings {
  const { data } = useSnapshot(selectSettings);
  return data;
}
