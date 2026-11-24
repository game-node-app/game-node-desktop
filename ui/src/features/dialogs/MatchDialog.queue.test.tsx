import { describe, expect, it, beforeEach, vi } from 'vitest';
import { act, render, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  SNAPSHOT_QUERY_KEY,
  createDefaultSnapshot,
  type LibraryView,
  type MatchDialogView,
  type Row,
} from '../../api';
import { I18nProvider } from '../../i18n';
import { MatchDialog } from './MatchDialog';
import { makeLibrary, spyLibraryInput } from '../library/controlsTestUtils';
import { currentBatchMatchQueue, resetBatchMatchQueue, startBatchMatch } from '../library/batchMatchQueue';

function row(key: string, status: Row['status'] = 'unmatched'): Row {
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
    status,
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
    selected: null,
    noResults: false,
    platformFilter: null,
    filteredOut: 0,
    current: 'unmatched',
    canClear: false,
  };
}

function view(matchDialog: MatchDialogView | null): LibraryView {
  return makeLibrary({
    access: 'verified',
    rows: [row('steam:1'), row('steam:2')],
    selected: ['steam:1', 'steam:2'],
    matchDialog,
  });
}

function renderDialog(library: LibraryView) {
  const snapshot = createDefaultSnapshot();
  snapshot.library = library;
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);

  render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale="en">
          <MatchDialog />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );

  return {
    update(next: LibraryView) {
      const nextSnapshot = createDefaultSnapshot();
      nextSnapshot.library = next;
      act(() => queryClient.setQueryData(SNAPSHOT_QUERY_KEY, nextSnapshot));
    },
  };
}

function inputs(spy: ReturnType<typeof spyLibraryInput>) {
  return spy.mock.calls.map(([input]) => input);
}

describe('MatchDialog batch queue wiring', () => {
  beforeEach(() => {
    resetBatchMatchQueue();
    vi.restoreAllMocks();
  });

  it('walks the queued entries and stops after the last dialog closes', async () => {
    const spy = spyLibraryInput();
    act(() => startBatchMatch(['steam:1', 'steam:2']));
    const { update } = renderDialog(view(null));

    await waitFor(() =>
      expect(inputs(spy)).toEqual([
        { select: { key: 'steam:1', mode: 'replace' } },
        'openMatch',
      ]),
    );

    update(view(dialogFor('steam:1')));
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('waiting'));

    // Confirm or skip closes the engine dialog; the dialog component notices and opens the next.
    update(view(null));
    await waitFor(() =>
      expect(inputs(spy).slice(2)).toEqual([
        { select: { key: 'steam:2', mode: 'replace' } },
        'openMatch',
      ]),
    );

    update(view(dialogFor('steam:2')));
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('waiting'));

    update(view(null));
    await waitFor(() => expect(currentBatchMatchQueue().phase).toBe('idle'));
  });
});
