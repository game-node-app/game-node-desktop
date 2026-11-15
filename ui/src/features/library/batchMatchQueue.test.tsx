import { describe, expect, it, beforeEach, vi } from 'vitest';
import { act, render, waitFor } from '@testing-library/react';
import type { LibraryView, MatchDialogView, Row } from '../../api';
import { I18nProvider } from '../../i18n';
import { resetUiStore, uiStore } from '../../store/uiStore';
import {
  BATCH_MATCH_OPEN_TIMEOUT_MS,
  advanceBatchMatch,
  cancelBatchMatch,
  currentBatchMatchQueue,
  resetBatchMatchQueue,
  startBatchMatch,
  useBatchMatchDriver,
} from './batchMatchQueue';
import { makeLibrary, spyLibraryInput } from './controlsTestUtils';

function row(key: string): Row {
  return {
    key,
    title: key,
    detectedName: key,
    matchedName: key,
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    running: false,
    status: 'unmatched',
    coverFile: null,
    playtime: { seconds: null, source: 'store', lastPlayedAt: null },
    releaseYear: null,
    addedAt: 0,
    isFavorite: false,
    hidden: false,
    gamenode: null,
  };
}

function dialogFor(entryKey: string): MatchDialogView {
  return {
    entryKey,
    detectedName: entryKey,
    providerName: 'Steam',
    query: entryKey,
    searching: false,
    candidates: [],
    results: [],
    noResults: false,
    current: 'unmatched',
    canClear: false,
  };
}

function Harness({
  library,
  account = null,
}: {
  library: LibraryView;
  account?: string | null;
}) {
  useBatchMatchDriver(library, account);
  return null;
}

function view(
  rows: string[],
  matchDialog: MatchDialogView | null = null,
  overrides: Partial<LibraryView> = {},
): LibraryView {
  return makeLibrary({
    access: 'verified',
    rows: rows.map(row),
    selected: rows,
    matchDialog,
    ...overrides,
  });
}

function Wrapped({ library, account = null }: { library: LibraryView; account?: string | null }) {
  return (
    <I18nProvider initialLocale="en">
      <Harness library={library} account={account} />
    </I18nProvider>
  );
}

function renderHarness(library: LibraryView, account: string | null = null) {
  return render(<Wrapped library={library} account={account} />);
}

function inputs(spy: ReturnType<typeof spyLibraryInput>) {
  return spy.mock.calls.map(([input]) => input);
}

function deferred() {
  let resolve!: () => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function flushMicrotasks() {
  await act(async () => {
    for (let index = 0; index < 8; index += 1) await Promise.resolve();
  });
}

const ALL_ROWS = ['steam:1', 'steam:2', 'steam:3', 'steam:4'];

/**
 * Walks a two-entry queue to its normal end and leaves the restore's first `replace` awaiting
 * the engine. Call 5 is that restore select: 1-4 are the two select/open pairs.
 */
async function finishQueueWithPendingRestore() {
  const spy = spyLibraryInput();
  const restoreGate = deferred();
  let calls = 0;
  spy.mockImplementation(() => {
    calls += 1;
    return calls === 5 ? restoreGate.promise : Promise.resolve();
  });
  act(() => startBatchMatch(['steam:1', 'steam:2']));

  const rendered = renderHarness(view(ALL_ROWS), 'ana');
  await waitFor(() => expect(spy).toHaveBeenCalledTimes(2));
  rendered.rerender(
    <Wrapped library={view(ALL_ROWS, dialogFor('steam:1'))} account="ana" />,
  );
  await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('waiting'));
  rendered.rerender(<Wrapped library={view(ALL_ROWS)} account="ana" />);
  await waitFor(() => expect(spy).toHaveBeenCalledTimes(4));
  rendered.rerender(
    <Wrapped library={view(ALL_ROWS, dialogFor('steam:2'))} account="ana" />,
  );
  await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('waiting'));
  rendered.rerender(<Wrapped library={view(ALL_ROWS)} account="ana" />);
  await waitFor(() => expect(spy).toHaveBeenCalledTimes(5));
  expect(currentBatchMatchQueue().phase).toBe('idle');

  return { spy, restoreGate, rendered };
}

describe('batch match queue', () => {
  beforeEach(() => {
    resetBatchMatchQueue();
    resetUiStore();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it('opens the first dialog, then advances after a confirmation', async () => {
    const spy = spyLibraryInput();
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    const { rerender } = renderHarness(view(['steam:1', 'steam:2']));
    await waitFor(() =>
      expect(inputs(spy)).toEqual([
        { select: { key: 'steam:1', mode: 'replace' } },
        'openMatch',
      ]),
    );

    rerender(<Wrapped library={view(['steam:1', 'steam:2'], dialogFor('steam:1'))} />);
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('waiting'));

    // Confirming closes the engine's dialog; the driver opens the next entry.
    rerender(<Wrapped library={view(['steam:1', 'steam:2'])} />);
    await waitFor(() =>
      expect(inputs(spy).slice(2)).toEqual([
        { select: { key: 'steam:2', mode: 'replace' } },
        'openMatch',
      ]),
    );
    expect(currentBatchMatchQueue().current).toBe('steam:2');
  });

  it('advances after a skip (close) without matching the entry', async () => {
    const spy = spyLibraryInput();
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    const { rerender } = render(
      <Wrapped library={view(['steam:1', 'steam:2'], dialogFor('steam:1'))} />,
    );
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('waiting'));

    // No pick input was sent: closing the dialog is the skip path.
    rerender(<Wrapped library={view(['steam:1', 'steam:2'])} />);
    await waitFor(() => expect(currentBatchMatchQueue().current).toBe('steam:2'));
    expect(spy.mock.calls.some(([input]) => typeof input === 'object' && 'pickMatch' in input)).toBe(
      false,
    );
  });

  it('finishes and restores the original selection', async () => {
    const spy = spyLibraryInput();
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    const { rerender } = render(
      <Wrapped library={view(['steam:1', 'steam:2'], dialogFor('steam:1'))} />,
    );
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('waiting'));
    rerender(<Wrapped library={view(['steam:1', 'steam:2'], dialogFor('steam:2'))} />);
    await waitFor(() => expect(currentBatchMatchQueue().current).toBe('steam:2'));
    rerender(<Wrapped library={view(['steam:1', 'steam:2'])} />);

    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('idle'));
    await waitFor(() =>
      expect(inputs(spy)).toEqual([
        { select: { key: 'steam:1', mode: 'replace' } },
        'openMatch',
        { select: { key: 'steam:2', mode: 'replace' } },
        'openMatch',
        { select: { key: 'steam:1', mode: 'replace' } },
        { select: { key: 'steam:2', mode: 'toggle' } },
      ]),
    );
    expect(currentBatchMatchQueue().originalKeys).toEqual([]);
  });

  it('skips a queued row that left the view', async () => {
    const spy = spyLibraryInput();
    act(() => startBatchMatch(['steam:1', 'steam:2', 'steam:3']));

    const { rerender } = render(
      <Wrapped library={view(['steam:1', 'steam:3'], dialogFor('steam:1'))} />,
    );
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('waiting'));

    // steam:2 is no longer shown, so the queue goes straight to steam:3.
    rerender(<Wrapped library={view(['steam:1', 'steam:3'])} />);
    await waitFor(() => expect(currentBatchMatchQueue().current).toBe('steam:3'));
    expect(
      spy.mock.calls.some(
        ([input]) =>
          typeof input === 'object' &&
          'select' in input &&
          input.select.key === 'steam:2',
      ),
    ).toBe(false);
  });

  it('advances when the current row disappears before its dialog opens', async () => {
    const spy = spyLibraryInput();
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    renderHarness(view(['steam:2']));

    await waitFor(() =>
      expect(inputs(spy)).toEqual([
        { select: { key: 'steam:2', mode: 'replace' } },
        'openMatch',
      ]),
    );
    expect(currentBatchMatchQueue().current).toBe('steam:2');
  });

  it('never opens another dialog after cancel', async () => {
    const spy = spyLibraryInput();
    act(() => startBatchMatch(['steam:1', 'steam:2']));
    const { rerender } = renderHarness(view(['steam:1', 'steam:2']));
    await waitFor(() => expect(inputs(spy)).toHaveLength(2));

    act(() => cancelBatchMatch());
    rerender(<Wrapped library={view(['steam:1', 'steam:2'], dialogFor('steam:1'))} />);
    rerender(<Wrapped library={view(['steam:1', 'steam:2'])} />);

    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('idle'));
    expect(inputs(spy)).toEqual([
      { select: { key: 'steam:1', mode: 'replace' } },
      'openMatch',
    ]);
  });

  it('stops when the session is no longer verified', async () => {
    spyLibraryInput();
    act(() => startBatchMatch(['steam:1', 'steam:2']));
    const { rerender } = renderHarness(view(['steam:1', 'steam:2']));
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('opening'));

    rerender(<Wrapped library={view(['steam:1', 'steam:2'], null, { access: 'offline' })} />);
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('idle'));
  });

  it('advance ignores missing rows and drops the run when nothing is left', () => {
    startBatchMatch(['steam:1', 'steam:2']);
    advanceBatchMatch([row('steam:1')]);
    expect(currentBatchMatchQueue().phase).toBe('idle');
  });

  it('ends a rejected selection with feedback instead of hanging', async () => {
    const spy = spyLibraryInput();
    spy.mockRejectedValueOnce(new Error('ipc down'));
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    renderHarness(view(['steam:1', 'steam:2']));

    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('idle'));
    expect(inputs(spy)).not.toContain('openMatch');
    await waitFor(() => expect(uiStore.state.toasts).toHaveLength(1));
    expect(uiStore.state.toasts[0]?.title).toBe(
      'The match dialog could not open. Try again.',
    );
  });

  it('ends a rejected open with feedback instead of leaving opening forever', async () => {
    const spy = spyLibraryInput();
    spy.mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('no dialog'));
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    renderHarness(view(['steam:1', 'steam:2']));

    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('idle'));
    expect(inputs(spy).filter((input) => input === 'openMatch')).toHaveLength(1);
    await waitFor(() => expect(uiStore.state.toasts).toHaveLength(1));
  });

  it('does not open after cancel while the selection is still in flight', async () => {
    const spy = spyLibraryInput();
    const gate = deferred();
    spy.mockImplementationOnce(() => gate.promise);
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    renderHarness(view(['steam:1', 'steam:2']), 'ana');
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));

    act(() => cancelBatchMatch());
    await act(async () => {
      gate.resolve();
      await gate.promise;
    });
    await flushMicrotasks();

    expect(inputs(spy)).not.toContain('openMatch');
    expect(currentBatchMatchQueue().phase).toBe('idle');
  });

  it('does not continue after cancel while openMatch is still in flight', async () => {
    const spy = spyLibraryInput();
    const gate = deferred();
    spy.mockResolvedValueOnce(undefined).mockImplementationOnce(() => gate.promise);
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    renderHarness(view(['steam:1', 'steam:2']), 'ana');
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(2));

    act(() => cancelBatchMatch());
    await act(async () => {
      gate.resolve();
      await gate.promise;
    });
    await flushMicrotasks();

    expect(inputs(spy)).toHaveLength(2);
    expect(currentBatchMatchQueue().phase).toBe('idle');
    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('does not continue into another account after a selection await', async () => {
    const spy = spyLibraryInput();
    const gate = deferred();
    spy.mockImplementationOnce(() => gate.promise);
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    const { rerender } = renderHarness(view(['steam:1', 'steam:2']), 'ana');
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));

    // Another signed-in, verified account takes over while the select is in flight.
    rerender(<Wrapped library={view(['steam:1', 'steam:2'])} account="bo" />);
    await act(async () => {
      gate.resolve();
      await gate.promise;
    });
    await flushMicrotasks();

    expect(inputs(spy)).not.toContain('openMatch');
    expect(
      spy.mock.calls.every(
        ([input]) =>
          typeof input === 'object' &&
          'select' in input &&
          input.select.key === 'steam:1',
      ),
    ).toBe(true);
    expect(currentBatchMatchQueue().phase).toBe('idle');
  });

  it('does not restore or open after sign-out during a selection await', async () => {
    const spy = spyLibraryInput();
    const gate = deferred();
    spy.mockImplementationOnce(() => gate.promise);
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    const { rerender } = renderHarness(view(['steam:1', 'steam:2']), 'ana');
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));

    rerender(<Wrapped library={view(['steam:1', 'steam:2'])} account={null} />);
    await act(async () => {
      gate.resolve();
      await gate.promise;
    });
    await flushMicrotasks();

    expect(inputs(spy)).toHaveLength(1);
    expect(currentBatchMatchQueue().phase).toBe('idle');
  });

  it('restores the selection only for the account that started the run', async () => {
    const spy = spyLibraryInput();
    const restoreGate = deferred();
    // Call order: select steam:1, openMatch, select steam:2, openMatch, restore replace.
    let calls = 0;
    spy.mockImplementation(() => {
      calls += 1;
      return calls === 5 ? restoreGate.promise : Promise.resolve();
    });
    act(() => startBatchMatch(['steam:1', 'steam:2']));

    const { rerender } = renderHarness(view(['steam:1', 'steam:2']), 'ana');
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(2));
    rerender(
      <Wrapped library={view(['steam:1', 'steam:2'], dialogFor('steam:1'))} account="ana" />,
    );
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('waiting'));
    rerender(<Wrapped library={view(['steam:1', 'steam:2'])} account="ana" />);
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(4));
    rerender(
      <Wrapped library={view(['steam:1', 'steam:2'], dialogFor('steam:2'))} account="ana" />,
    );
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('waiting'));
    rerender(<Wrapped library={view(['steam:1', 'steam:2'])} account="ana" />);
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(5));

    rerender(<Wrapped library={view(['steam:1', 'steam:2'])} account="bo" />);
    await act(async () => {
      restoreGate.resolve();
      await restoreGate.promise;
    });
    await flushMicrotasks();

    // The replace went out under the original account; no toggle follows for the new one.
    expect(inputs(spy).slice(5)).toEqual([]);
  });

  it('never resumes a finished restore inside the next run', async () => {
    const { spy, restoreGate, rendered } = await finishQueueWithPendingRestore();
    expect(inputs(spy)).toHaveLength(5);

    // The finished run is idle while its restore waits for the engine. A new queue starts in the
    // same account before that answer returns.
    act(() => startBatchMatch(['steam:3', 'steam:4']));
    rendered.rerender(<Wrapped library={view(ALL_ROWS)} account="ana" />);
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(7));
    expect(inputs(spy).slice(5)).toEqual([
      { select: { key: 'steam:3', mode: 'replace' } },
      'openMatch',
    ]);

    await act(async () => {
      restoreGate.resolve();
      await restoreGate.promise;
    });
    await flushMicrotasks();

    // The old restore cannot toggle steam:2 over queue 2, and queue 2 is untouched.
    expect(inputs(spy).slice(5)).toEqual([
      { select: { key: 'steam:3', mode: 'replace' } },
      'openMatch',
    ]);
    expect(currentBatchMatchQueue().phase).toBe('opening');
    expect(currentBatchMatchQueue().current).toBe('steam:3');
    expect(currentBatchMatchQueue().pending).toEqual(['steam:4']);
  });

  it('still restores the selection when a cancel explicitly asks for it', async () => {
    const spy = spyLibraryInput();
    act(() => startBatchMatch(['steam:1', 'steam:2']));
    renderHarness(view(['steam:1', 'steam:2']), 'ana');
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(2));

    act(() => cancelBatchMatch({ restoreSelection: true }));

    await waitFor(() =>
      expect(inputs(spy)).toEqual([
        { select: { key: 'steam:1', mode: 'replace' } },
        'openMatch',
        { select: { key: 'steam:1', mode: 'replace' } },
        { select: { key: 'steam:2', mode: 'toggle' } },
      ]),
    );
    expect(currentBatchMatchQueue().phase).toBe('idle');
  });

  it('drops a finished restore when the queue is cancelled while it awaits', async () => {
    const { spy, restoreGate } = await finishQueueWithPendingRestore();
    expect(inputs(spy)).toHaveLength(5);

    act(() => cancelBatchMatch());
    await act(async () => {
      restoreGate.resolve();
      await restoreGate.promise;
    });
    await flushMicrotasks();

    expect(inputs(spy)).toHaveLength(5);
  });

  it('drops a finished restore when the driver unmounts while it awaits', async () => {
    const { spy, restoreGate, rendered } = await finishQueueWithPendingRestore();
    expect(inputs(spy)).toHaveLength(5);

    rendered.unmount();
    await act(async () => {
      restoreGate.resolve();
      await restoreGate.promise;
    });
    await flushMicrotasks();

    expect(inputs(spy)).toHaveLength(5);
  });

  it('ends an open that never produces a dialog after the bounded wait', async () => {
    vi.useFakeTimers();
    try {
      const spy = spyLibraryInput();
      act(() => startBatchMatch(['steam:1', 'steam:2']));
      renderHarness(view(['steam:1', 'steam:2']), 'ana');
      await flushMicrotasks();
      expect(inputs(spy)).toEqual([
        { select: { key: 'steam:1', mode: 'replace' } },
        'openMatch',
      ]);
      expect(currentBatchMatchQueue().phase).toBe('opening');

      await act(async () => {
        await vi.advanceTimersByTimeAsync(BATCH_MATCH_OPEN_TIMEOUT_MS + 10);
      });

      expect(currentBatchMatchQueue().phase).toBe('idle');
      expect(uiStore.state.toasts[0]?.title).toBe(
        'The match dialog could not open. Try again.',
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it('clears the open timer on unmount', async () => {
    vi.useFakeTimers();
    try {
      spyLibraryInput();
      act(() => startBatchMatch(['steam:1', 'steam:2']));
      const { unmount } = renderHarness(view(['steam:1', 'steam:2']), 'ana');
      await flushMicrotasks();
      unmount();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(BATCH_MATCH_OPEN_TIMEOUT_MS + 10);
      });

      expect(uiStore.state.toasts).toHaveLength(0);
    } finally {
      vi.useRealTimers();
    }
  });
});
