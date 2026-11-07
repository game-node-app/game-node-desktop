import { describe, expect, it, vi } from 'vitest';
import { QueryClientProvider } from '@tanstack/react-query';
import { render, screen, act, waitFor } from '@testing-library/react';
import { createAppQueryClient } from '../AppProviders';
import {
  ENGINE_SNAPSHOT_EVENT,
  LICENSE_NOTICES_QUERY_KEY,
  SNAPSHOT_QUERY_KEY,
  createDefaultSnapshot,
  subscribeToEngineSnapshots,
  useAuth,
  useEngineSnapshotSubscription,
  useLicenseNotices,
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

type AuthSelection = ReturnType<typeof useAuth>;

function AuthIdentityProbe({ onValue }: { onValue: (value: AuthSelection) => void }) {
  useEngineSnapshotSubscription();
  onValue(useAuth());
  return null;
}

function NoticesProbe() {
  const { data } = useLicenseNotices();
  return <div data-testid="notices">{data ?? ''}</div>;
}

describe('useSnapshot and engine:snapshot event synchronization', () => {
  it('updates QueryClient cache when engine:snapshot event is emitted', async () => {
    const queryClient = createAppQueryClient();

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
    const queryClient = createAppQueryClient();

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

    await act(async () => {
      emitMockEvent(ENGINE_SNAPSHOT_EVENT, nextSnapshot);
    });

    await waitFor(() =>
      expect(screen.getByTestId('auth-state')).toHaveTextContent('Signed in as PlayerOne'),
    );
  });

  it('keeps the auth selection identity when unrelated snapshot fields change', async () => {
    const queryClient = createAppQueryClient();
    const seen: AuthSelection[] = [];

    render(
      <QueryClientProvider client={queryClient}>
        <AuthIdentityProbe onValue={(value) => seen.push(value)} />
      </QueryClientProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });

    const signedInSnapshot = createDefaultSnapshot();
    signedInSnapshot.auth = {
      signedIn: {
        user: {
          id: 'u-1',
          username: 'Cassiano',
          email: null,
        },
      },
    };

    await act(async () => {
      emitMockEvent(ENGINE_SNAPSHOT_EVENT, signedInSnapshot);
    });
    const afterSignIn = seen.at(-1)!;
    const rendersAfterSignIn = seen.length;

    // A local-games-only update leaves every auth field untouched. The query cache
    // structurally shares unchanged nested objects, so their references stay equal.
    const unrelated = createDefaultSnapshot();
    unrelated.auth = signedInSnapshot.auth;
    unrelated.localGames = { ...unrelated.localGames, busy: true };

    await act(async () => {
      emitMockEvent(ENGINE_SNAPSHOT_EVENT, unrelated);
    });

    expect(seen.at(-1)).toBe(afterSignIn);
    expect(seen).toHaveLength(rendersAfterSignIn);

    const signedOutSnapshot = createDefaultSnapshot();
    await act(async () => {
      emitMockEvent(ENGINE_SNAPSHOT_EVENT, signedOutSnapshot);
    });

    expect(seen.at(-1)).not.toBe(afterSignIn);
    expect(seen.at(-1)!.auth).toBe('signedOut');
  });

  it('drops the license notices cache when the viewer unmounts', async () => {
    const queryClient = createAppQueryClient();

    const view = render(
      <QueryClientProvider client={queryClient}>
        <NoticesProbe />
      </QueryClientProvider>,
    );

    await waitFor(() =>
      expect(screen.getByTestId('notices')).toHaveTextContent('GPL-3.0-only'),
    );
    expect(queryClient.getQueryData(LICENSE_NOTICES_QUERY_KEY)).toBeDefined();

    view.unmount();

    await waitFor(() =>
      expect(
        queryClient.getQueryCache().find({ queryKey: LICENSE_NOTICES_QUERY_KEY }),
      ).toBeUndefined(),
    );
  });
});
