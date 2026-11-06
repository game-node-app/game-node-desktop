import { describe, expect, it, vi } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, act } from '@testing-library/react';
import {
  ENGINE_SNAPSHOT_EVENT,
  SNAPSHOT_QUERY_KEY,
  createDefaultSnapshot,
  subscribeToEngineSnapshots,
  useAuth,
  useEngineSnapshotSubscription,
  type Snapshot,
} from './index';

type EventCallback = (event: { payload: Snapshot }) => void;
const listeners = new Map<string, Set<EventCallback>>();

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (eventName: string, handler: EventCallback) => {
    if (!listeners.has(eventName)) {
      listeners.set(eventName, new Set());
    }
    listeners.get(eventName)!.add(handler);
    return () => {
      listeners.get(eventName)?.delete(handler);
    };
  }),
}));

function emitMockEvent(eventName: string, payload: Snapshot) {
  const set = listeners.get(eventName);
  if (set) {
    for (const cb of set) {
      cb({ payload });
    }
  }
}

function AuthStatusProbe() {
  useEngineSnapshotSubscription();
  const { auth } = useAuth();
  if (typeof auth === 'object' && 'signedIn' in auth) {
    return <div data-testid="auth-state">Signed in as {auth.signedIn.user.username}</div>;
  }
  return <div data-testid="auth-state">{String(auth)}</div>;
}

describe('useSnapshot and engine:snapshot event synchronization', () => {
  it('updates QueryClient cache when engine:snapshot event is emitted', async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });

    const unlisten = await subscribeToEngineSnapshots(queryClient);

    const updated = createDefaultSnapshot();
    updated.auth = {
      signedIn: {
        user: {
          id: 'u-1',
          username: 'Cassiano',
          email: 'cassi@gamenode.app',
        },
      },
    };

    emitMockEvent(ENGINE_SNAPSHOT_EVENT, updated);

    const cached = queryClient.getQueryData<Snapshot>(SNAPSHOT_QUERY_KEY);
    expect(cached).toEqual(updated);

    unlisten();
  });

  it('re-renders subscribed React components when engine:snapshot updates', async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <AuthStatusProbe />
      </QueryClientProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByTestId('auth-state')).toHaveTextContent('signedOut');

    const nextSnapshot = createDefaultSnapshot();
    nextSnapshot.auth = {
      signedIn: {
        user: {
          id: 'u-42',
          username: 'PlayerOne',
          email: null,
        },
      },
    };

    expect(listeners.get(ENGINE_SNAPSHOT_EVENT)?.size).toBe(1);

    await act(async () => {
      emitMockEvent(ENGINE_SNAPSHOT_EVENT, nextSnapshot);
    });

    expect(queryClient.getQueryData<Snapshot>(SNAPSHOT_QUERY_KEY)?.auth).toEqual({
      signedIn: {
        user: {
          id: 'u-42',
          username: 'PlayerOne',
          email: null,
        },
      },
    });

    expect(screen.getByTestId('auth-state')).toHaveTextContent('Signed in as PlayerOne');
  });
});
