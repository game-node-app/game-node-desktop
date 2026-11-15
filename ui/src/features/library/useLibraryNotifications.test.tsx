import { beforeEach, describe, expect, it, vi } from 'vitest';
import React from 'react';
import { act, renderHook } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { createAppQueryClient } from '../../AppProviders';
import { SNAPSHOT_QUERY_KEY } from '../../api/useSnapshot';
import { commands } from '../../api/commands';
import {
  createDefaultLibraryView,
  createDefaultSnapshot,
  type LaunchStatus,
  type LibraryView,
  type Snapshot,
} from '../../api/types';
import { I18nProvider } from '../../i18n';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { useLibraryNotifications } from './useLibraryNotifications';

function makeLibrary(overrides: Partial<LibraryView> = {}): LibraryView {
  return { ...createDefaultLibraryView(), ...overrides };
}

function makeSnapshot(
  library: Partial<LibraryView> = {},
  options: { account?: string | null; launch?: LaunchStatus | null } = {},
): Snapshot {
  const account = options.account === undefined ? 'account-1' : options.account;
  return {
    ...createDefaultSnapshot(),
    auth: account == null ? 'signedOut' : { signedIn: { user: { id: account } } },
    library: makeLibrary(library),
    launch: options.launch ?? null,
  };
}

function renderNotifications(queryClient: ReturnType<typeof createAppQueryClient>) {
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <I18nProvider initialLocale="en">{children}</I18nProvider>
    </QueryClientProvider>
  );
  return renderHook(() => useLibraryNotifications(), { wrapper });
}

function addSummary(overrides: Partial<NonNullable<LibraryView['addSummary']>> = {}) {
  return {
    runId: 7,
    undoable: true,
    items: [],
    added: 3,
    alreadyPresent: 0,
    skipped: 0,
    failed: 0,
    ...overrides,
  };
}

function setup(snapshot: Snapshot = makeSnapshot()) {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  const view = renderNotifications(queryClient);
  return { queryClient, view };
}

async function publish(
  queryClient: ReturnType<typeof createAppQueryClient>,
  snapshot: Snapshot,
) {
  await act(async () => {
    queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
    // React Query batches observer notifications; flush the scheduled callback.
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

describe('useLibraryNotifications', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('announces a legitimate add once per run with Undo and View on GameNode', async () => {
    const libraryInput = vi.spyOn(commands, 'libraryInput').mockResolvedValue(undefined);
    const openPage = vi.spyOn(commands, 'openGameNodePage').mockResolvedValue(undefined);
    const { queryClient } = setup();

    await publish(queryClient, makeSnapshot({ addSummary: addSummary() }));

    expect(uiStore.state.toasts).toHaveLength(1);
    const toast = uiStore.state.toasts[0];
    expect(toast.kind).toBe('success');
    expect(toast.title).toBe('3 games added to GameNode');
    expect(toast.actions?.map((action) => action.label)).toEqual(['Undo', 'View on GameNode']);

    act(() => toast.actions![0].onClick());
    expect(libraryInput).toHaveBeenCalledWith({ undoAdd: { run: 7 } });

    act(() => toast.actions![1].onClick());
    expect(openPage).toHaveBeenCalledWith('library');
  });

  it('uses the singular title for one added game and reports skips and failures', async () => {
    const { queryClient } = setup();

    await publish(
      queryClient,
      makeSnapshot({
        addSummary: addSummary({ added: 1, skipped: 2, failed: 1 }),
      }),
    );

    const toast = uiStore.state.toasts[0];
    expect(toast.title).toBe('1 game added to GameNode');
    expect(toast.body).toContain('2 not on GameNode were skipped');
    expect(toast.body).toContain("Couldn't update GameNode");
  });

  it('stays silent when the add only edited or found an existing entry', async () => {
    const { queryClient } = setup();

    await publish(
      queryClient,
      makeSnapshot({
        addSummary: addSummary({ added: 0, alreadyPresent: 1, undoable: false }),
      }),
    );

    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('reports the already-present count alongside the added games', async () => {
    const { queryClient } = setup();

    await publish(
      queryClient,
      makeSnapshot({
        addSummary: addSummary({ added: 2, alreadyPresent: 1, skipped: 0, failed: 0 }),
      }),
    );

    const toast = uiStore.state.toasts[0];
    expect(toast.title).toBe('2 games added to GameNode');
    expect(toast.body).toContain('already in your library 1');
  });

  it('does not replay the latest summary on a later snapshot or a remount', async () => {
    const { queryClient, view } = setup();
    await publish(queryClient, makeSnapshot({ addSummary: addSummary() }));
    expect(uiStore.state.toasts).toHaveLength(1);

    await publish(queryClient, makeSnapshot({ generation: 9, addSummary: addSummary() }));
    expect(uiStore.state.toasts).toHaveLength(1);

    view.unmount();
    resetUiStore();
    renderNotifications(queryClient);
    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('starts a new notification for a new run id', async () => {
    const { queryClient } = setup();
    await publish(queryClient, makeSnapshot({ addSummary: addSummary() }));
    await publish(queryClient, makeSnapshot({ addSummary: addSummary({ runId: 8, added: 1 }) }));

    expect(uiStore.state.toasts.map((toast) => toast.title)).toEqual([
      '3 games added to GameNode',
      '1 game added to GameNode',
    ]);
  });

  it('reports undo done and failed results only for the announced add run', async () => {
    const { queryClient } = setup();
    await publish(queryClient, makeSnapshot({ addSummary: addSummary() }));

    await publish(
      queryClient,
      makeSnapshot({
        addSummary: addSummary(),
        undo: { run: 7, state: 'done', deleted: 3, failed: 0 },
      }),
    );
    expect(uiStore.state.toasts.some((toast) => toast.title === 'Add undone')).toBe(true);

    await publish(
      queryClient,
      makeSnapshot({
        addSummary: addSummary({ runId: 8, added: 1 }),
      }),
    );
    await publish(
      queryClient,
      makeSnapshot({
        addSummary: addSummary({ runId: 8, added: 1 }),
        undo: { run: 8, state: 'partial', deleted: 0, failed: 1 },
      }),
    );

    const failed = uiStore.state.toasts.find(
      (toast) => toast.title === "Couldn't undo the add",
    );
    expect(failed?.kind).toBe('danger');
  });

  it('ignores a stale undo result', async () => {
    const { queryClient } = setup();

    await publish(
      queryClient,
      makeSnapshot({ undo: { run: 99, state: 'done', deleted: 3, failed: 0 } }),
    );

    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('clears its refs on an account change without replaying the previous account', async () => {
    const { queryClient } = setup();
    await publish(queryClient, makeSnapshot({ addSummary: addSummary() }));
    expect(uiStore.state.toasts).toHaveLength(1);

    await publish(
      queryClient,
      makeSnapshot({ addSummary: addSummary() }, { account: 'account-2' }),
    );
    expect(uiStore.state.toasts).toHaveLength(1);

    await publish(
      queryClient,
      makeSnapshot({ addSummary: addSummary({ runId: 9, added: 2 }) }, { account: 'account-2' }),
    );
    expect(uiStore.state.toasts.map((toast) => toast.title)).toEqual([
      '3 games added to GameNode',
      '2 games added to GameNode',
    ]);
  });

  it('reports a status run with its real counts and skip reason', async () => {
    const { queryClient } = setup();

    await publish(
      queryClient,
      makeSnapshot({
        statusRun: {
          run: 11,
          status: 'finished',
          changed: 2,
          skipped: 1,
          failed: 0,
          skippedUnmatched: 1,
          skippedNotInLibrary: 0,
          skippedUnknownStatus: 0,
        },
      }),
    );

    const toast = uiStore.state.toasts[0];
    expect(toast.kind).toBe('success');
    expect(toast.title).toBe('Status changed to Finished for 2 games');
    expect(toast.body).toBe('1 not on GameNode were skipped');
  });

  it('reports a collection run by name and stays silent for an already-present game', async () => {
    const { queryClient } = setup();

    await publish(
      queryClient,
      makeSnapshot({
        collectionRun: {
          run: 12,
          collectionId: 'coop',
          collectionName: 'Coop no sofá',
          changed: 2,
          skipped: 0,
          failed: 0,
          skippedUnmatched: 0,
          skippedNotInLibrary: 0,
          skippedUnknownStatus: 0,
          skippedAlreadyInCollection: 0,
        },
      }),
    );

    expect(uiStore.state.toasts[0].title).toBe('Added to Coop no sofá');
    expect(uiStore.state.toasts[0].kind).toBe('success');
  });

  it('confirms a saved view by name once per save run', async () => {
    const { queryClient } = setup();

    await publish(
      queryClient,
      makeSnapshot({ lastSavedView: { id: 5, name: 'Steam 10 h+', run: 13 } }),
    );
    expect(uiStore.state.toasts[0].title).toBe('View “Steam 10 h+” saved');

    await publish(
      queryClient,
      makeSnapshot({ lastSavedView: { id: 5, name: 'Steam 10 h+', run: 13 } }),
    );
    expect(uiStore.state.toasts).toHaveLength(1);
  });

  it('offers Show again for a hide run and dispatches setHidden false', async () => {
    const libraryInput = vi.spyOn(commands, 'libraryInput').mockResolvedValue(undefined);
    const { queryClient } = setup();

    await publish(
      queryClient,
      makeSnapshot({ lastHidden: { keys: ['steam:10'], hidden: true, run: 14 } }),
    );

    const toast = uiStore.state.toasts[0];
    expect(toast.title).toBe('1 hidden from the library');
    expect(toast.actions?.[0].label).toBe('Show again');

    act(() => toast.actions![0].onClick());
    expect(libraryInput).toHaveBeenCalledWith({
      setHidden: { keys: ['steam:10'], hidden: false },
    });
  });

  it('does not notify when entries are revealed', async () => {
    const { queryClient } = setup();

    await publish(
      queryClient,
      makeSnapshot({ lastHidden: { keys: ['steam:10'], hidden: false, run: 15 } }),
    );

    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('offers Locate executable and View log for a missing DRM-free executable', async () => {
    const pick = vi.spyOn(commands, 'pickLocalExecutable').mockResolvedValue(undefined);
    const logs = vi.spyOn(commands, 'openLogsFolder').mockResolvedValue(undefined);
    const launch: LaunchStatus = {
      entryKey: 'manual:reg-1',
      kind: 'play',
      state: { failed: 'incomplete' },
    };
    const { queryClient } = setup(
      makeSnapshot({
        paletteIndex: [
          {
            key: 'manual:reg-1',
            title: 'Neon Courier 2088',
            providerName: 'DRM-free',
            installed: true,
            playtimeLabel: null,
          },
        ],
      }),
    );

    await publish(
      queryClient,
      makeSnapshot(
        {
          paletteIndex: [
            {
              key: 'manual:reg-1',
              title: 'Neon Courier 2088',
              providerName: 'DRM-free',
              installed: true,
              playtimeLabel: null,
            } as LibraryView['paletteIndex'][number],
          ],
        },
        { launch },
      ),
    );

    const toast = uiStore.state.toasts[0];
    expect(toast.kind).toBe('danger');
    expect(toast.title).toBe("Couldn't start Neon Courier 2088");
    expect(toast.body).toBe('The executable was not found.');
    expect(toast.actions?.map((action) => action.label)).toEqual([
      'Locate executable',
      'View log',
    ]);

    act(() => toast.actions![0].onClick());
    expect(pick).toHaveBeenCalledWith('reg-1');

    act(() => toast.actions![1].onClick());
    expect(logs).toHaveBeenCalled();
  });

  it('does not turn a store launch failure into an executable notification', async () => {
    const launch: LaunchStatus = {
      entryKey: 'steam:1',
      kind: 'play',
      state: { failed: 'noHandler' },
    };
    const { queryClient } = setup();

    await publish(queryClient, makeSnapshot({}, { launch }));

    expect(uiStore.state.toasts).toHaveLength(0);
  });
});
