import { useCallback, useEffect, useRef, useState } from 'react';
import { commands, type CatalogView as CatalogViewState, type LibraryInput } from '../../api';

/** The catalog search debounce from `design-reference/catalog-view.md`. */
export const CATALOG_SEARCH_DEBOUNCE_MS = 300;

export interface CatalogSearchController {
  /** The text currently in the search field. */
  draft: string;
  /** A local edit. The debounce decides when the engine hears about it. */
  setDraft: (value: string) => void;
  /** Clears the field and returns the engine's catalog state to idle. */
  clear: () => void;
  /** Repeats the current query after an error or offline result. */
  retry: () => void;
}

/**
 * Keeps the search field and the engine's catalog query in sync.
 *
 * The user's draft is authoritative while they type: engine echoes for an older
 * query never overwrite it, and a stale answer is never adopted as a new
 * external prefill. External prefill (command palette, no-results action) does
 * update the field, but only when no local edit or request is in flight, and it
 * is never sent again on mount because its opener already ran the search.
 */
export function useCatalogSearch(
  catalog: CatalogViewState | null | undefined,
  onError: () => void,
): CatalogSearchController {
  const initial = catalog?.query ?? '';
  const [draft, setDraftState] = useState(initial);

  const draftRef = useRef(draft);
  draftRef.current = draft;
  const catalogRef = useRef(catalog);
  catalogRef.current = catalog;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

  const seenViewQueryRef = useRef(initial);
  const lastRequestedRef = useRef(initial);
  const requestedRef = useRef<Set<string>>(new Set([initial]));
  /** A debounced local edit that has not been sent yet. */
  const pendingRef = useRef<string | null>(null);
  /** The query of the latest request whose engine echo is still expected. */
  const awaitingRef = useRef<string | null>(null);
  const didMountRef = useRef(false);
  const skipSyncRef = useRef(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const send = useCallback((input: LibraryInput) => {
    void Promise.resolve(commands.libraryInput(input)).catch(() => {
      onErrorRef.current();
    });
  }, []);

  const sendSearch = useCallback(
    (query: string) => {
      // A retry of the query already in the view changes the status, not the
      // query, so there will be no echo to wait for.
      awaitingRef.current = catalogRef.current?.query === query ? null : query;
      lastRequestedRef.current = query;
      requestedRef.current.add(query);
      send({ catalogSearch: query });
    },
    [send],
  );

  const sendClear = useCallback(() => {
    awaitingRef.current = catalogRef.current?.query === '' ? null : '';
    lastRequestedRef.current = '';
    requestedRef.current.add('');
    send('catalogClear');
  }, [send]);

  const viewQuery = catalog?.query ?? '';

  useEffect(() => {
    if (viewQuery === seenViewQueryRef.current) return;
    seenViewQueryRef.current = viewQuery;

    // While a request is in flight, only its own echo is meaningful. Anything
    // else is an older snapshot and must not replace the local draft.
    if (awaitingRef.current != null) {
      if (viewQuery === awaitingRef.current) {
        awaitingRef.current = null;
      }
      return;
    }

    // The user is typing. Keep the local draft and let the debounce finish.
    if (pendingRef.current != null) return;

    // Echo of the latest request (also catches a retried unchanged query).
    if (viewQuery === lastRequestedRef.current) return;

    // A slower answer or snapshot for a query this view already requested must
    // not replace a newer local draft. A loading view is an explicit external
    // search (palette/no-results) and may legitimately repeat an old query.
    if (
      requestedRef.current.has(viewQuery) &&
      draftRef.current.trim().length > 0 &&
      catalogRef.current?.status !== 'loading'
    ) {
      return;
    }

    // A real external prefill: the palette or the no-results action asked for
    // this query and already sent the search. Adopt it without re-sending.
    lastRequestedRef.current = viewQuery;
    requestedRef.current.add(viewQuery);
    if (viewQuery === draftRef.current) return;
    skipSyncRef.current = true;
    setDraftState(viewQuery);
  }, [viewQuery]);

  useEffect(() => {
    if (!didMountRef.current) {
      didMountRef.current = true;
      return;
    }
    if (skipSyncRef.current) {
      skipSyncRef.current = false;
      return;
    }

    const trimmed = draft.trim();
    const current = catalogRef.current;

    if (trimmed.length < 2) {
      // No search request is sent for a short query. The engine is reset so an
      // in-flight answer for the previous query cannot resurface. Offline is
      // left untouched: only Try again may leave that state.
      if (
        current &&
        current.status !== 'offline' &&
        (current.query !== '' || current.status !== 'idle')
      ) {
        sendClear();
      }
      return;
    }

    // Offline/unverified stays quiet until the user explicitly retries.
    if (current?.status === 'offline') return;

    // Already searched, loading, or ready for exactly this text.
    if (
      current &&
      current.query.trim() === trimmed &&
      current.status !== 'error'
    ) {
      return;
    }

    pendingRef.current = draft;
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      if (pendingRef.current === draft) pendingRef.current = null;
      sendSearch(draft);
    }, CATALOG_SEARCH_DEBOUNCE_MS);

    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      if (pendingRef.current === draft) pendingRef.current = null;
    };
  }, [draft, sendClear, sendSearch]);

  const setDraft = useCallback((value: string) => {
    setDraftState(value);
  }, []);

  const clear = useCallback(() => {
    setDraftState('');
  }, []);

  const retry = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    pendingRef.current = null;
    sendSearch(draftRef.current);
  }, [sendSearch]);

  return { draft, setDraft, clear, retry };
}
