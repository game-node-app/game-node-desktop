import { useEffect, useRef } from 'react';
import { Store, useStore } from '@tanstack/react-store';
import { commands, type LibraryView, type Row } from '../../api';
import { useI18n } from '../../i18n';
import { pushToast } from '../../store/uiStore';

/**
 * Sequential match over a multi-selection.
 *
 * The engine opens the match dialog for the single selected entry (`Input::OpenMatch`), so the
 * batch flow can only walk the unmatched and ambiguous entries one at a time. The queue keeps
 * the original keys, selects each entry in turn and opens the dialog. The dialog's own close -
 * after a pick, "not a game" or skip - advances the queue, so an ignored pick leaves the dialog
 * open instead of skipping an entry.
 *
 * Every continuation is guarded by the run id, the entry key and the account the run started
 * under. The engine answers an open asynchronously, so an open that is refused (no dialog after
 * a bounded wait) or rejected ends the run with feedback instead of leaving it `opening` forever.
 */
export type BatchMatchPhase = 'idle' | 'opening' | 'waiting';

export interface BatchMatchQueueState {
  /** Distinguishes runs so a restart with the same first key dispatches again. */
  run: number;
  /** The signed-in account the run belongs to; set by the driver before the first dispatch. */
  accountId: string | null;
  /** The selection the batch started from, restored when the queue finishes. */
  originalKeys: string[];
  /** Entries after the current one, in selection order. */
  pending: string[];
  /** The entry whose dialog is being opened or is open. */
  current: string | null;
  phase: BatchMatchPhase;
}

/**
 * How long the driver waits for the engine to publish the match dialog before treating the
 * open as refused. The engine acknowledges `openMatch` through the next snapshot, not the
 * command's return value.
 */
export const BATCH_MATCH_OPEN_TIMEOUT_MS = 1500;

/** The latest account and rows the mounted driver observed, for async continuations. */
let observedAccountId: string | null = null;
let observedRows: Row[] = [];

/**
 * Identifies the only restore continuation still allowed to touch the selection. Starting a new
 * queue, resetting, cancelling or unmounting the driver bumps it, so a restore left awaiting an
 * engine answer can never wake up and select rows for the next queue.
 */
let restoreEpoch = 0;

function invalidateRestores(): void {
  restoreEpoch += 1;
}

function idleQueue(run: number): BatchMatchQueueState {
  return {
    run,
    accountId: null,
    originalKeys: [],
    pending: [],
    current: null,
    phase: 'idle',
  };
}

export const batchMatchQueue = new Store<BatchMatchQueueState>(idleQueue(0));

export function useBatchMatchQueue<TSelected = BatchMatchQueueState>(
  selector?: (state: BatchMatchQueueState) => TSelected,
): TSelected {
  return useStore(batchMatchQueue, selector ?? ((state) => state as unknown as TSelected));
}

export function currentBatchMatchQueue(): BatchMatchQueueState {
  return batchMatchQueue.state;
}

/** Test and sign-out helper: forgets the run without touching the selection. */
export function resetBatchMatchQueue(): void {
  invalidateRestores();
  observedAccountId = null;
  observedRows = [];
  batchMatchQueue.setState((prev) => idleQueue(prev.run));
}

function visibleKey(rows: Row[], key: string): boolean {
  return rows.some((row) => row.key === key);
}

function accountMatches(accountId: string | null): boolean {
  return accountId == null || observedAccountId === accountId;
}

/**
 * Restores the batch selection, first visible key with replace and the rest with toggle.
 * Restoring is best effort and bound to the finished run, its account and the restore epoch:
 * every command checks that no new queue, reset, cancel or unmount superseded the restore, that
 * the finished run still owns the idle queue, that the run's account is still the signed-in one
 * and that the key is still shown. A continuation that resumes after any of those changes never
 * selects a row for the next queue or another account.
 */
function restoreSelection(
  keys: string[],
  accountId: string | null,
  run: number,
): void {
  if (keys.length < 2) return;
  const epoch = restoreEpoch;
  const isCurrent = () =>
    restoreEpoch === epoch &&
    batchMatchQueue.state.run === run &&
    batchMatchQueue.state.phase === 'idle' &&
    accountMatches(accountId);

  void (async () => {
    try {
      const visible = () => keys.filter((key) => visibleKey(observedRows, key));
      if (!isCurrent()) return;
      const first = visible()[0];
      if (first == null) return;
      await commands.libraryInput({ select: { key: first, mode: 'replace' } });
      if (!isCurrent()) return;
      for (const key of visible()) {
        if (key === first) continue;
        if (!isCurrent()) return;
        if (!visibleKey(observedRows, key)) continue;
        await commands.libraryInput({ select: { key, mode: 'toggle' } });
        if (!isCurrent()) return;
      }
    } catch {
      // Restoring the selection is best-effort; the engine ignores keys it no longer shows.
    }
  })();
}

/**
 * Starts a run at the first key. Duplicate keys are opened once, in order. `originalKeys` is the
 * selection to restore at the end; it defaults to the queue itself and may include matched rows
 * that are not part of the match run.
 */
export function startBatchMatch(keys: string[], originalKeys: string[] = keys): void {
  const unique = [...new Set(keys)];
  if (unique.length === 0) return;
  // A restore from the previous run must not wake up inside this one.
  invalidateRestores();
  const restore = [...new Set(originalKeys)];
  batchMatchQueue.setState((prev) => ({
    run: prev.run + 1,
    // The always-mounted driver stamps the signed-in account before the first dispatch, so a
    // start fired during an account transition can never inherit the previous account's id.
    accountId: null,
    originalKeys: restore.length > 0 ? restore : unique,
    pending: unique.slice(1),
    current: unique[0],
    phase: 'opening',
  }));
}

/**
 * Moves past the current entry after its dialog closed or it left the view. Pending keys that
 * are no longer shown are skipped safely and without duplicates. The original keys are still
 * restored at the end.
 */
export function advanceBatchMatch(rows: Row[]): void {
  const state = batchMatchQueue.state;
  if (state.phase === 'idle' || state.current == null) return;

  const remaining = [...new Set(state.pending)].filter(
    (key) => key !== state.current && visibleKey(rows, key),
  );
  const next = remaining.length > 0 ? remaining[0] : null;
  if (next == null) {
    const originalKeys = state.originalKeys;
    let finished = false;
    batchMatchQueue.setState((prev) => {
      if (prev.phase === 'idle' || prev.run !== state.run) return prev;
      finished = true;
      return idleQueue(prev.run);
    });
    if (finished) restoreSelection(originalKeys, state.accountId, state.run);
    return;
  }

  batchMatchQueue.setState((prev) => {
    if (prev.phase === 'idle' || prev.run !== state.run) return prev;
    return {
      ...prev,
      pending: remaining.slice(1),
      current: next,
      phase: 'opening',
    };
  });
}

/**
 * Stops the run. A cancelled queue never opens another dialog; the caller decides whether the
 * selection should be restored.
 */
export function cancelBatchMatch(
  options: { restoreSelection?: boolean; accountId?: string | null } = {},
): void {
  const state = batchMatchQueue.state;
  // Cancelling ends every pending continuation, including a restore still owed by an earlier
  // finished queue. A cancel that asks for its own restore starts a fresh, superseding one.
  invalidateRestores();
  if (state.phase === 'idle') return;
  batchMatchQueue.setState((prev) => idleQueue(prev.run));
  if (options.restoreSelection) {
    restoreSelection(
      state.originalKeys,
      options.accountId === undefined ? state.accountId : options.accountId,
      state.run,
    );
  }
}

/** Whether a continuation still belongs to the live run, its entry and its account. */
function isCurrent(run: number, current: string, accountId: string | null): boolean {
  const state = batchMatchQueue.state;
  if (state.phase === 'idle' || state.run !== run || state.current !== current) {
    return false;
  }
  if (accountId != null) {
    if (observedAccountId !== accountId) return false;
    if (state.accountId != null && state.accountId !== accountId) return false;
  }
  return true;
}

/**
 * Drives the queue from the mounted match dialog. It opens the current entry, waits for the
 * engine's dialog to appear and advances when that dialog closes. Mount it once, in the always
 * mounted `MatchDialog`, so the run survives the batch bar unmounting when the selection drops
 * to one row.
 *
 * `accountId` is the signed-in GameNode account. A run started under one account is cancelled
 * when another account takes over, even when both are verified. The engine acknowledges an open
 * through its snapshot rather than the command result, so a bounded, cleaned-up timer ends an
 * open that never produced a dialog instead of leaving the queue `opening` forever.
 */
export function useBatchMatchDriver(
  library: LibraryView | null | undefined,
  accountId: string | null = null,
): void {
  const state = useBatchMatchQueue();
  const { t } = useI18n();
  const dispatched = useRef<string | null>(null);
  const rows = useRef<Row[]>(library?.rows ?? []);
  const libraryRef = useRef(library);
  const accountRef = useRef(accountId);
  const translate = useRef(t);
  const mounted = useRef(true);
  const ackTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  rows.current = library?.rows ?? [];
  observedRows = rows.current;
  observedAccountId = accountId;
  libraryRef.current = library;
  accountRef.current = accountId;
  translate.current = t;

  const dialog = library?.matchDialog ?? null;
  const access = library?.access;
  const currentVisible =
    state.current != null && visibleKey(rows.current, state.current);

  const clearAckTimer = () => {
    if (ackTimer.current == null) return;
    clearTimeout(ackTimer.current);
    ackTimer.current = null;
  };

  const failOpen = (account: string | null) => {
    pushToast(
      'danger',
      translate.current('Library::The match dialog could not open. Try again.'),
    );
    cancelBatchMatch({ restoreSelection: true, accountId: account });
  };

  // Every timer belongs to this mounted driver and is cleared on unmount. An unmounted driver
  // can no longer observe rows or accounts, so any restore it started stays superseded.
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      invalidateRestores();
      if (ackTimer.current != null) {
        clearTimeout(ackTimer.current);
        ackTimer.current = null;
      }
    };
  }, []);

  // A run started before the driver observed the account adopts it before dispatching.
  useEffect(() => {
    if (accountId == null || state.phase === 'idle' || state.accountId != null) return;
    batchMatchQueue.setState((prev) =>
      prev.phase === 'idle' || prev.accountId != null
        ? prev
        : { ...prev, accountId },
    );
  }, [accountId, state.phase, state.accountId]);

  // A session that stopped being verified, signed out or changed account can no longer match:
  // stop instead of waiting forever. The previous account's selection is not restored on an
  // account switch, because that selection belongs to the account that is gone.
  useEffect(() => {
    if (state.phase === 'idle') {
      clearAckTimer();
      return;
    }
    const accountChanged =
      state.accountId != null && (accountId == null || accountId !== state.accountId);
    if (library == null || access !== 'verified' || accountChanged) {
      clearAckTimer();
      cancelBatchMatch({ restoreSelection: !accountChanged });
    }
  }, [state.phase, state.accountId, accountId, access, library == null]);

  // The timer only helps while an open is pending.
  useEffect(() => {
    if (state.phase === 'opening') return;
    clearAckTimer();
  }, [state.phase]);

  // Open the current entry. Rows that disappeared before dispatch are skipped.
  useEffect(() => {
    if (state.phase !== 'opening' || state.current == null) return;
    const run = state.run;
    const current = state.current;
    const account = state.accountId ?? accountRef.current;
    const token = `${run}:${current}`;
    if (dispatched.current === token) return;
    if (!visibleKey(rows.current, current)) {
      advanceBatchMatch(rows.current);
      return;
    }
    dispatched.current = token;
    void (async () => {
      try {
        await commands.libraryInput({ select: { key: current, mode: 'replace' } });
        if (!mounted.current || !isCurrent(run, current, account)) return;
        await commands.libraryInput('openMatch');
        if (!mounted.current || !isCurrent(run, current, account)) return;
        if (batchMatchQueue.state.phase !== 'opening') return;
        clearAckTimer();
        ackTimer.current = setTimeout(() => {
          ackTimer.current = null;
          if (!mounted.current || !isCurrent(run, current, account)) return;
          if (batchMatchQueue.state.phase !== 'opening') return;
          if (libraryRef.current?.matchDialog?.entryKey === current) return;
          failOpen(account);
        }, BATCH_MATCH_OPEN_TIMEOUT_MS);
      } catch {
        // The IPC rejected the selection or the open. The engine never acknowledged this
        // attempt, so end the run with feedback instead of waiting for a dialog that will not
        // come. A stale or foreign-account rejection is ignored.
        if (!mounted.current || !isCurrent(run, current, account)) return;
        failOpen(account);
      }
    })();
  }, [state.phase, state.current, state.run, state.accountId]);

  // Wait for the dialog, then advance when it closes for any reason (pick, not a game, skip).
  // A row that disappears while the open is pending also advances instead of hanging.
  useEffect(() => {
    if (state.phase === 'idle' || state.current == null) return;
    const run = state.run;
    const current = state.current;
    const account = state.accountId ?? accountRef.current;
    if (!isCurrent(run, current, account)) return;
    if (state.phase === 'opening') {
      if (dialog?.entryKey === current) {
        batchMatchQueue.setState((prev) =>
          prev.phase === 'opening' && prev.run === run && prev.current === current
            ? { ...prev, phase: 'waiting' }
            : prev,
        );
      } else if (!currentVisible) {
        advanceBatchMatch(rows.current);
      }
      return;
    }
    if (!currentVisible || dialog?.entryKey !== current) {
      advanceBatchMatch(rows.current);
    }
  }, [dialog, state.phase, state.current, state.run, state.accountId, currentVisible]);
}
