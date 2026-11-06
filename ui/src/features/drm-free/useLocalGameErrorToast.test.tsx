import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import React from 'react';
import { act, renderHook } from '@testing-library/react';
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query';
import { createAppQueryClient } from '../../AppProviders';
import { createDefaultSnapshot, SNAPSHOT_QUERY_KEY, type Snapshot } from '../../api';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { useLocalGameErrorToast } from './useLocalGameErrorToast';

function snapshotWithError(error: string | null): Snapshot {
  const snapshot = createDefaultSnapshot();
  return { ...snapshot, localGames: { ...snapshot.localGames, error } };
}

function dangerMessages(): string[] {
  return uiStore.state.toasts.filter((t) => t.kind === 'danger').map((t) => t.message);
}

describe('useLocalGameErrorToast', () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    resetUiStore();
    queryClient = createAppQueryClient();
    queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshotWithError(null));
  });

  afterEach(() => {
    queryClient.clear();
  });

  function renderToastHook() {
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    return renderHook(() => useLocalGameErrorToast(), { wrapper });
  }

  // TanStack Query delivers observer updates on a timer, so flush it inside act before asserting.
  async function publish(error: string | null) {
    await act(async () => {
      queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshotWithError(error));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }

  it('shows no toast while the local games view has no error', () => {
    renderToastHook();
    expect(dangerMessages()).toEqual([]);
  });

  it('shows a danger toast with the error message when the view fails', async () => {
    renderToastHook();
    await publish('missing field `fileIdentity`');
    expect(dangerMessages()).toEqual(['missing field `fileIdentity`']);
  });

  it('does not repeat the same error on later snapshots', async () => {
    renderToastHook();
    await publish('bad record');
    await publish('bad record');
    await publish('bad record');
    expect(dangerMessages()).toEqual(['bad record']);
  });

  it('shows the same error again after it clears, and a new error immediately', async () => {
    renderToastHook();
    await publish('first');
    await publish(null);
    await publish('first');
    await publish('second');
    expect(dangerMessages()).toEqual(['first', 'first', 'second']);
  });
});
