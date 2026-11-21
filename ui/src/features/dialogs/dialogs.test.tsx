import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, act, waitFor } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  commands,
  createDefaultLibraryView,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Snapshot,
  type MatchDialogView,
  type AddDialogView,
  createDefaultFilters,
} from '../../api';
import { I18nProvider } from '../../i18n';
import { resetUiStore } from '../../store/uiStore';
import { MatchDialog, AddDialog } from './index';

function renderWithProviders(
  ui: React.ReactElement,
  snapshot: Snapshot,
  locale = 'en',
) {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);

  const result = render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );

  return { ...result, queryClient };
}

describe('Dialogs Feature Tests', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  describe('MatchDialog', () => {
    const makeMatchSnapshot = (overrides?: Partial<MatchDialogView>): Snapshot => {
      const snap = createDefaultSnapshot();
      snap.library = {
        ...createDefaultLibraryView(),
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          scope: 'all',
          group: 'none',
          collapsedGroups: {},
          filters: createDefaultFilters(),
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: true,
          sidebarCollapsed: false,
        },
        search: '',
        access: 'verified',
        total: 1,
        installedTotal: 1,
        providers: [],
        rows: [],
        scanning: false,
        failedProviders: [],
        selected: ['entry-1'],
        focused: 'entry-1',
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
        matchDialog: {
          entryKey: 'entry-1',
          detectedName: 'Hades II Early Access',
          providerName: 'Steam',
          query: 'Hades',
          searching: false,
          candidates: [
            {
              gameId: 101,
              name: 'Hades II',
              releaseYear: 2024,
              coverUrl: null,
              coverFile: 'hades2.jpg',
              current: false,
              confidence: 96,
              tone: 'success',
              platforms: ['PC', 'Switch'],
              reasons: [
                { titleSimilarity: { percent: 96 } },
                'sameYear',
                'storeIdRecognized',
              ],
            },
          ],
          results: [
            {
              gameId: 102,
              name: 'Hades',
              releaseYear: 2020,
              coverUrl: null,
              coverFile: null,
              current: true,
              confidence: 74,
              tone: 'warning',
              platforms: ['PC'],
              reasons: [
                { titleSimilarity: { percent: 74 } },
                { differentYear: { year: 2020 } },
              ],
            },
          ],
          selected: {
            gameId: 102,
            name: 'Hades',
            releaseYear: 2020,
            coverUrl: null,
            coverFile: null,
            current: true,
            confidence: 74,
            tone: 'warning',
            platforms: ['PC'],
            reasons: [
              { titleSimilarity: { percent: 74 } },
              { differentYear: { year: 2020 } },
            ],
          },
          noResults: false,
          canClear: true,
          current: { matched: { gameId: 102, source: 'automatic' } },
          ...overrides,
        },
      };
      return snap;
    };

    it('renders the search, the list with count and the preview, and dispatches the footer actions', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = makeMatchSnapshot();

      renderWithProviders(<MatchDialog />, snapshot, 'en');

      // Header info
      expect(screen.getByTestId('match-dialog-title')).toHaveTextContent(
        'Match to a GameNode game',
      );
      expect(screen.getByTestId('match-dialog-subtitle')).toHaveTextContent(
        'Hades II Early Access · Steam',
      );

      // Search input & submit
      const searchInput = screen.getByTestId('match-search-input');
      expect(searchInput).toHaveValue('Hades');

      fireEvent.change(searchInput, { target: { value: 'Hades II' } });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setMatchQuery: 'Hades II' });
      fireEvent.submit(searchInput.closest('form')!);
      expect(libraryInputSpy).toHaveBeenCalledWith('searchMatch');

      // Candidates and results with their meta and confidence
      const candidate = screen.getByTestId('candidate-101');
      expect(candidate).toHaveTextContent('Hades II');
      expect(candidate).toHaveTextContent('2024 · PC, Switch');
      expect(candidate).toHaveTextContent('96%');
      expect(candidate).toHaveAttribute('aria-selected', 'false');
      const result = screen.getByTestId('result-102');
      expect(result).toHaveTextContent('Hades');
      expect(result).toHaveTextContent('2020 · PC');
      expect(result).toHaveTextContent('74%');
      expect(result).toHaveAttribute('aria-selected', 'true');
      expect(screen.getByTestId('match-current-badge')).toHaveTextContent(
        'Current match',
      );
      expect(screen.getByTestId('match-options-container')).toHaveTextContent(
        'Several possible matches',
      );
      expect(screen.getByTestId('match-options-container')).toHaveTextContent(
        '2 results',
      );

      // Preview of the selected (current) option, with its reasons.
      const preview = screen.getByTestId('match-preview');
      expect(preview).toHaveTextContent('Hades');
      expect(preview).toHaveTextContent('2020');
      expect(preview).toHaveTextContent('Why we suggest it');
      expect(preview).toHaveTextContent('Title 74% similar');
      expect(preview).toHaveTextContent('Different release year (2020)');

      // Selecting an option only changes the preview.
      fireEvent.click(candidate);
      expect(libraryInputSpy).toHaveBeenCalledWith({
        selectMatch: { gameId: 101 },
      });
      expect(
        libraryInputSpy.mock.calls.some(([input]) =>
          JSON.stringify(input).includes('pickMatch'),
        ),
      ).toBe(false);

      // Confirming matches the selected option.
      fireEvent.click(screen.getByTestId('match-confirm-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith({
        confirmMatch: {
          now: expect.any(Number),
        },
      });

      // Mark no game
      const notAGameBtn = screen.getByTestId('match-not-a-game-btn');
      expect(notAGameBtn).toHaveTextContent('Not on GameNode');
      fireEvent.click(notAGameBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith({
        markNoGame: {
          now: expect.any(Number),
        },
      });

      // Clear match
      const clearMatchBtn = screen.getByTestId('match-clear-match-btn');
      fireEvent.click(clearMatchBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith('clearMatch');

      // Cancel closes
      fireEvent.click(screen.getByTestId('match-cancel-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith('closeMatch');

      // Escape key closes match
      fireEvent.keyDown(window, { key: 'Escape' });
      expect(libraryInputSpy).toHaveBeenCalledWith('closeMatch');
    });

    it('disables Match without a selection and does nothing when it is confirmed', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = makeMatchSnapshot({ selected: null, candidates: [], results: [] });

      renderWithProviders(<MatchDialog />, snapshot, 'en');

      expect(screen.getByTestId('match-preview')).toHaveTextContent(
        'Select a game to see its details.',
      );
      expect(screen.getByTestId('match-confirm-btn')).toBeDisabled();
      expect(libraryInputSpy).not.toHaveBeenCalledWith(
        expect.objectContaining({ confirmMatch: expect.anything() }),
      );
    });

    it('confirms with Enter and with a double-click on an option', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      renderWithProviders(<MatchDialog />, makeMatchSnapshot(), 'en');

      fireEvent.keyDown(screen.getByTestId('candidate-101'), { key: 'Enter' });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        selectMatch: { gameId: 101 },
      });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        confirmMatch: { now: expect.any(Number) },
      });

      libraryInputSpy.mockClear();
      fireEvent.doubleClick(screen.getByTestId('result-102'));
      expect(libraryInputSpy).toHaveBeenCalledWith({
        selectMatch: { gameId: 102 },
      });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        confirmMatch: { now: expect.any(Number) },
      });
    });

    it('moves the selection with the arrow keys', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      renderWithProviders(<MatchDialog />, makeMatchSnapshot(), 'en');

      const candidate = screen.getByTestId('candidate-101');
      candidate.focus();
      fireEvent.keyDown(candidate, { key: 'ArrowDown' });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        selectMatch: { gameId: 102 },
      });
    });

    it('keeps focus on the option list when a new snapshot arrives', async () => {
      const { queryClient } = renderWithProviders(
        <MatchDialog />,
        makeMatchSnapshot(),
        'en',
      );

      // Let the dialog's opening focus settle before the option list takes focus.
      await waitFor(() => {
        expect(screen.getByTestId('match-search-input')).toHaveFocus();
      });
      const candidate = screen.getByTestId('candidate-101');
      candidate.focus();
      expect(document.activeElement).toBe(candidate);

      // Selecting an option arrives as a fresh dialog view. That update must not
      // pull focus back to the search field, or the next arrow key would be lost.
      const next = makeMatchSnapshot();
      next.library!.matchDialog!.selected =
        next.library!.matchDialog!.candidates[0];
      act(() => {
        queryClient.setQueryData(SNAPSHOT_QUERY_KEY, next);
      });

      await waitFor(() => {
        expect(screen.getByTestId('match-preview')).toHaveTextContent(
          'Hades II',
        );
      });

      expect(document.activeElement).toBe(candidate);
      expect(screen.getByTestId('match-search-input')).not.toHaveFocus();
    });

    it('renders covers from the asset protocol and never from a remote result URL', () => {
      const snapshot = makeMatchSnapshot();
      const dialog = snapshot.library!.matchDialog!;
      dialog.results[0].coverUrl =
        '//images.igdb.com/igdb/image/upload/t_thumb/co1.jpg';
      dialog.results[0].coverFile = null;

      renderWithProviders(<MatchDialog />, snapshot, 'en');

      const candidateCover = screen
        .getByTestId('candidate-101')
        .querySelector('img');
      expect(candidateCover).toHaveAttribute(
        'src',
        'gamenode-asset://localhost/cover/hades2.jpg',
      );
      expect(candidateCover).toHaveAttribute('loading', 'lazy');
      expect(candidateCover).toHaveAttribute('decoding', 'async');

      // A result whose cover is still downloading shows its title, not a broken image.
      expect(screen.getByTestId('result-102').querySelector('img')).toBeNull();
      expect(screen.getByTestId('result-102')).toHaveTextContent('Hades');
    });

    it('renders localized MatchDialog in pt-BR', () => {
      const snapshot = makeMatchSnapshot({
        noResults: true,
      });

      renderWithProviders(<MatchDialog />, snapshot, 'pt-BR');

      expect(screen.getByTestId('match-dialog-title')).toHaveTextContent(
        'Associar a um jogo do GameNode',
      );
      expect(screen.getByTestId('match-options-container')).toHaveTextContent(
        'Várias correspondências possíveis',
      );
      expect(screen.getByTestId('match-preview')).toHaveTextContent(
        'Por que sugerimos',
      );
      expect(screen.getByTestId('match-no-results-notice')).toHaveTextContent(
        'Nenhum jogo encontrado. Tente outras palavras.',
      );
      expect(screen.getByTestId('match-not-a-game-btn')).toHaveTextContent(
        'Não está no GameNode',
      );
      expect(screen.getByTestId('match-clear-match-btn')).toHaveTextContent(
        'Usar a correspondência automática',
      );
      expect(screen.getByTestId('match-cancel-btn')).toHaveTextContent('Cancelar');
      expect(screen.getByTestId('match-confirm-btn')).toHaveTextContent('Associar');
      expect(screen.getByTestId('match-current-badge')).toHaveTextContent(
        'Associação atual',
      );
    });

    it('renders the list label as search results when there are no candidates', () => {
      const snapshot = makeMatchSnapshot({
        candidates: [],
        results: [
          {
            gameId: 102,
            name: 'Hades',
            releaseYear: 2020,
            coverUrl: null,
            coverFile: null,
            current: false,
            confidence: 74,
            tone: 'warning',
            platforms: ['PC'],
            reasons: [{ titleSimilarity: { percent: 74 } }],
          },
        ],
        selected: null,
      });

      renderWithProviders(<MatchDialog />, snapshot, 'en');

      expect(screen.getByTestId('match-options-container')).toHaveTextContent(
        'Search results',
      );
      expect(screen.getByTestId('match-options-container')).toHaveTextContent(
        '1 result',
      );
    });

    it('renders search error notices and searchBlocked notice in en and pt-BR', () => {
      const errorSnap = makeMatchSnapshot({
        error: 'rejected',
        searchBlocked: 'needsVerifiedSession',
      });

      // English rejected error and searchBlocked
      const { unmount } = renderWithProviders(<MatchDialog />, errorSnap, 'en');
      expect(screen.getByTestId('match-error-notice')).toHaveTextContent(
        'GameNode rejected your session. Sign in again.',
      );
      expect(screen.getByTestId('match-search-blocked-notice')).toHaveTextContent(
        'Your GameNode session is not verified yet. Retry the connection.',
      );

      unmount();

      // Portuguese rejected error and searchBlocked
      const { unmount: unmountPt } = renderWithProviders(<MatchDialog />, errorSnap, 'pt-BR');
      expect(screen.getByTestId('match-error-notice')).toHaveTextContent(
        'O GameNode recusou sua sessão. Entre novamente.',
      );
      expect(screen.getByTestId('match-search-blocked-notice')).toHaveTextContent(
        'Sua sessão do GameNode ainda não foi verificada. Tente conectar novamente.',
      );

      unmountPt();

      // Failed error
      const failedSnap = makeMatchSnapshot({
        error: 'failed',
      });
      const { unmount: unmountFailed } = renderWithProviders(<MatchDialog />, failedSnap, 'en');
      expect(screen.getByTestId('match-error-notice')).toHaveTextContent(
        'The search failed. Check your connection and try again.',
      );

      unmountFailed();

      renderWithProviders(<MatchDialog />, failedSnap, 'pt-BR');
      expect(screen.getByTestId('match-error-notice')).toHaveTextContent(
        'A busca falhou. Verifique sua conexão e tente novamente.',
      );
    });
  });

  describe('AddDialog', () => {
    const makeAddSnapshot = (overrides?: Partial<AddDialogView>): Snapshot => {
      const snap = createDefaultSnapshot();
      snap.library = {
        ...createDefaultLibraryView(),
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          scope: 'all',
          group: 'none',
          collapsedGroups: {},
          filters: createDefaultFilters(),
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: true,
          sidebarCollapsed: false,
        },
        search: '',
        access: 'verified',
        total: 2,
        installedTotal: 2,
        providers: [{ id: 'steam', name: 'Steam' } as never],
        rows: [],
        scanning: false,
        failedProviders: [],
        selected: ['entry-1', 'entry-2'],
        focused: 'entry-1',
        actions: {
          selected: 2,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
        addDialog: {
          targets: [
            {
              target: { row: 'entry-1' },
              entryKey: 'entry-1',
              title: 'Hades',
              coverFile: 'mock-1.svg',
              matched: true,
            },
            {
              target: { row: 'entry-2' },
              entryKey: 'entry-2',
              title: 'Unknown Indie Game',
              coverFile: null,
              matched: false,
            },
          ],
          mode: 'library',
          editingExisting: false,
          status: 'playing',
          appliedStatus: 'playing',
          appliedFrom: null,
          collections: {
            loaded: [
              {
                id: 'col-1',
                name: 'Favorites',
                defaultStatus: null,
                picked: false,
                entryCount: 9,
              },
              {
                id: 'col-2',
                name: 'Backlog',
                defaultStatus: 'planned',
                picked: true,
                entryCount: 14,
              },
            ],
          },
          progress: null,
          items: [],
          error: null,
          canConfirm: true,
          blocked: null,
          summary: null,
          newCollection: null,
          ...overrides,
        },
      };
      return snap;
    };

    it('renders the header, the status radiogroup, counts and the footer actions', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = makeAddSnapshot({
        appliedStatus: 'planned',
        appliedFrom: 'Backlog',
      });

      renderWithProviders(<AddDialog />, snapshot, 'en');

      expect(screen.getByTestId('add-dialog-title')).toHaveTextContent(
        'Add to GameNode',
      );
      expect(screen.getByTestId('add-dialog-subtitle')).toHaveTextContent(
        '2 games',
      );
      expect(screen.getByTestId('add-dialog-cover')).toBeInTheDocument();

      // Five status cards in a radiogroup; the picked one is checked.
      expect(screen.getByRole('radiogroup', { name: 'Status' })).toBeInTheDocument();
      expect(screen.getByTestId('add-status-btn-playing')).toHaveAttribute(
        'aria-checked',
        'true',
      );
      expect(screen.getByTestId('add-status-btn-ongoing')).toHaveAttribute(
        'aria-checked',
        'false',
      );
      fireEvent.click(screen.getByTestId('add-status-btn-finished'));
      expect(libraryInputSpy).toHaveBeenCalledWith({ setAddStatus: 'finished' });
      // Arrow keys move the selection too.
      fireEvent.keyDown(screen.getByTestId('add-status-btn-playing'), {
        key: 'ArrowRight',
      });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setAddStatus: 'finished' });

      // The default-status note always names the status and where it comes from.
      expect(screen.getByTestId('add-applied-from-notice')).toHaveTextContent(
        'With no choice, GameNode uses Planned, the default status of Backlog.',
      );

      // The unmatched target is called out and does not block the rest.
      expect(screen.getByTestId('add-multiple-targets')).toHaveTextContent(
        '2 games',
      );
      expect(screen.getByTestId('add-target-entry-2')).toHaveTextContent(
        'Unknown Indie Game (no GameNode match, will be skipped)',
      );

      // Counts come from the complete account library read.
      expect(screen.getByTestId('add-collection-count-col-1')).toHaveTextContent(
        '9 games',
      );
      expect(screen.getByTestId('add-collection-count-col-2')).toHaveTextContent(
        '14 games',
      );

      const checkbox = screen.getByTestId('add-collection-checkbox-col-1');
      expect(checkbox).not.toBeChecked();
      fireEvent.click(checkbox);
      expect(libraryInputSpy).toHaveBeenCalledWith({
        toggleAddCollection: 'col-1',
      });

      const confirm = screen.getByTestId('add-dialog-confirm-btn');
      expect(confirm).toHaveTextContent('Add to GameNode');
      fireEvent.click(confirm);
      expect(libraryInputSpy).toHaveBeenCalledWith('confirmAdd');

      fireEvent.click(screen.getByTestId('add-dialog-cancel-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith('closeAdd');
    });

    it('hides a collection count when the account library was never read', () => {
      const snapshot = makeAddSnapshot({
        collections: {
          loaded: [
            {
              id: 'col-1',
              name: 'Favorites',
              defaultStatus: null,
              picked: false,
              entryCount: null,
            },
          ],
        },
      });
      renderWithProviders(<AddDialog />, snapshot, 'en');
      expect(screen.getByTestId('add-collection-item-col-1')).toHaveTextContent(
        'Favorites',
      );
      expect(screen.queryByTestId('add-collection-count-col-1')).toBeNull();
    });

    it('uses the edit title and Save when editing an existing entry', () => {
      const snapshot = makeAddSnapshot({
        editingExisting: true,
        targets: [
          {
            target: { row: 'entry-1' },
            entryKey: 'entry-1',
            title: 'Hades',
            coverFile: 'mock-1.svg',
            matched: true,
          },
        ],
      });
      renderWithProviders(<AddDialog />, snapshot, 'en');
      expect(screen.getByTestId('add-dialog-title')).toHaveTextContent(
        'Edit on GameNode',
      );
      expect(screen.getByTestId('add-dialog-subtitle')).toHaveTextContent('Hades');
      expect(screen.getByTestId('add-dialog-confirm-btn')).toHaveTextContent(
        'Save',
      );
    });

    it('creates a collection inline and keeps the typed name on an error', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const open = makeAddSnapshot({
        newCollection: {
          name: '',
          creating: false,
          canCreate: false,
          error: null,
        },
      });
      const { unmount } = renderWithProviders(<AddDialog />, open, 'en');

      expect(screen.getByTestId('add-new-collection-input')).toHaveAttribute(
        'placeholder',
        'Collection name',
      );
      expect(
        screen.getByTestId('add-new-collection-create-btn'),
      ).toBeDisabled();
      fireEvent.change(screen.getByTestId('add-new-collection-input'), {
        target: { value: 'Coop no sofá' },
      });
      expect(libraryInputSpy).toHaveBeenCalledWith({
        setNewCollectionName: 'Coop no sofá',
      });
      fireEvent.click(screen.getByTestId('add-new-collection-cancel-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith('cancelNewCollection');
      fireEvent.click(screen.getByTestId('add-new-collection-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith('startNewCollection');
      unmount();

      const ready = renderWithProviders(
        <AddDialog />,
        makeAddSnapshot({
          newCollection: {
            name: 'Coop no sofá',
            creating: false,
            canCreate: true,
            error: null,
          },
        }),
        'en',
      );
      fireEvent.click(screen.getByTestId('add-new-collection-create-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith('createCollection');
      fireEvent.keyDown(screen.getByTestId('add-new-collection-input'), {
        key: 'Enter',
      });
      expect(libraryInputSpy).toHaveBeenCalledWith('createCollection');
      ready.unmount();

      const errors: Array<[NonNullable<AddDialogView['newCollection']>['error'], string]> = [
        ['limitReached', "You reached GameNode's collection limit."],
        ['offline', 'GameNode cannot be reached right now.'],
        ['rejected', 'GameNode rejected your session. Sign in again.'],
        ['failed', 'The collection could not be created.'],
      ];
      for (const [error, message] of errors) {
        const snapshot = makeAddSnapshot({
          newCollection: {
            name: 'Coop no sofá',
            creating: false,
            canCreate: true,
            error,
          },
        });
        const view = renderWithProviders(<AddDialog />, snapshot, 'en');
        expect(screen.getByTestId('add-new-collection-input')).toHaveValue(
          'Coop no sofá',
        );
        expect(screen.getByTestId('add-new-collection-error')).toHaveTextContent(
          message,
        );
        view.unmount();
      }
    });

    it('shows the created collection checked with no inline field afterwards', () => {
      const snapshot = makeAddSnapshot({
        collections: {
          loaded: [
            {
              id: 'col-1',
              name: 'Favorites',
              defaultStatus: null,
              picked: false,
              entryCount: 9,
            },
            {
              id: 'col-3',
              name: 'Coop no sofá',
              defaultStatus: null,
              picked: true,
              entryCount: 0,
            },
          ],
        },
      });
      renderWithProviders(<AddDialog />, snapshot, 'en');
      expect(screen.queryByTestId('add-new-collection-field')).toBeNull();
      expect(
        screen.getByTestId('add-collection-checkbox-col-3'),
      ).toBeChecked();
      expect(screen.getByTestId('add-collection-count-col-3')).toHaveTextContent(
        '0 games',
      );
    });

    it('disables Create while a collection is being created', () => {
      const snapshot = makeAddSnapshot({
        newCollection: {
          name: 'Coop',
          creating: true,
          canCreate: false,
          error: null,
        },
      });
      renderWithProviders(<AddDialog />, snapshot, 'en');
      expect(screen.getByTestId('add-new-collection-input')).toBeDisabled();
      expect(
        screen.getByTestId('add-new-collection-create-btn'),
      ).toBeDisabled();
    });

    it('handles collections loading, failed and empty states', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const loading = renderWithProviders(
        <AddDialog />,
        makeAddSnapshot({ collections: 'loading' }),
        'en',
      );
      expect(screen.getByTestId('add-collections-loading')).toHaveTextContent(
        'Loading your collections…',
      );
      loading.unmount();

      const failed = renderWithProviders(
        <AddDialog />,
        makeAddSnapshot({ collections: { failed: 'failed' } }),
        'en',
      );
      expect(screen.getByTestId('add-collections-failed')).toHaveTextContent(
        'Could not load your collections. Try again.',
      );
      fireEvent.click(screen.getByTestId('add-collections-retry-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith('retryCollections');
      failed.unmount();

      renderWithProviders(
        <AddDialog />,
        makeAddSnapshot({ collections: { loaded: [] } }),
        'en',
      );
      expect(screen.getByTestId('add-collections-empty')).toHaveTextContent(
        'You have no collections.',
      );
    });

    it('renders every batch item state and Hide during the run', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = makeAddSnapshot({
        appliedStatus: 'planned',
        progress: { done: 1, total: 3 },
        items: [
          {
            target: { row: 'entry-1' },
            entryKey: 'entry-1',
            title: 'Hades',
            state: 'added',
            status: 'planned',
            error: null,
          },
          {
            target: { row: 'entry-2' },
            entryKey: 'entry-2',
            title: 'Aster Vale',
            state: 'adding',
            status: null,
            error: null,
          },
          {
            target: { row: 'entry-3' },
            entryKey: 'entry-3',
            title: 'Neon Courier 2088',
            state: 'skipped',
            status: null,
            error: null,
          },
        ],
      });
      const { unmount } = renderWithProviders(<AddDialog />, snapshot, 'en');

      expect(screen.getByTestId('add-progress-caption')).toHaveTextContent(
        '1 of 3',
      );
      expect(screen.getByTestId('add-progress-item-status-entry-1')).toHaveTextContent(
        'Added',
      );
      expect(screen.getByTestId('add-progress-item-status-entry-2')).toHaveTextContent(
        'Adding…',
      );
      expect(screen.getByTestId('add-progress-item-status-entry-3')).toHaveTextContent(
        'Skipped: no GameNode match',
      );
      expect(screen.getByTestId('add-progress-note')).toHaveTextContent(
        'You can close this: adding continues in the background.',
      );
      fireEvent.click(screen.getByTestId('add-progress-hide-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith('hideAdd');
      unmount();

      const waiting = makeAddSnapshot({
        progress: { done: 0, total: 2 },
        items: [
          {
            target: { row: 'entry-1' },
            entryKey: 'entry-1',
            title: 'Hades',
            state: 'waiting',
            status: null,
            error: null,
          },
          {
            target: { row: 'entry-2' },
            entryKey: 'entry-2',
            title: 'Aster Vale',
            state: 'alreadyPresent',
            status: 'finished',
            error: null,
          },
        ],
      });
      const second = renderWithProviders(<AddDialog />, waiting, 'en');
      expect(screen.getByTestId('add-progress-item-status-entry-1')).toHaveTextContent(
        'Waiting',
      );
      expect(screen.getByTestId('add-progress-item-status-entry-2')).toHaveTextContent(
        'Already in the library',
      );
      second.unmount();
    });

    it('shows the failed item reason and the final summary when the dialog stays open', () => {
      const summary = {
        runId: 4,
        undoable: true,
        added: 1,
        alreadyPresent: 0,
        skipped: 1,
        failed: 1,
        items: [
          {
            target: { row: 'entry-1' as const },
            entryKey: 'entry-1',
            title: 'Hades',
            state: 'added' as const,
            status: 'planned' as const,
            error: null,
          },
          {
            target: { row: 'entry-2' as const },
            entryKey: 'entry-2',
            title: 'Aster Vale',
            state: 'failed' as const,
            status: null,
            error: 'rejected' as const,
          },
          {
            target: { row: 'entry-3' as const },
            entryKey: 'entry-3',
            title: 'Neon Courier 2088',
            state: 'skipped' as const,
            status: null,
            error: null,
          },
        ],
      };
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = makeAddSnapshot({
        progress: { done: 3, total: 3 },
        items: summary.items,
        summary,
      });
      renderWithProviders(<AddDialog />, snapshot, 'en');

      expect(screen.getByTestId('add-summary-headline')).toHaveTextContent(
        'Saved 1 · already in your library 0 · skipped 1 · failed 1',
      );
      expect(screen.getByTestId('add-summary-item-status-entry-2')).toHaveTextContent(
        'Failed: GameNode rejected the session',
      );
      fireEvent.click(screen.getByTestId('add-dialog-close-btn'));
      expect(libraryInputSpy).toHaveBeenCalledWith('closeAdd');
    });

    it('renders the progress and the new-collection copy in pt-BR', () => {
      const running = makeAddSnapshot({
        appliedStatus: 'planned',
        progress: { done: 1, total: 2 },
        items: [
          {
            target: { row: 'entry-1' },
            entryKey: 'entry-1',
            title: 'Hades',
            state: 'added',
            status: 'planned',
            error: null,
          },
          {
            target: { row: 'entry-2' },
            entryKey: 'entry-2',
            title: 'Aster Vale',
            state: 'adding',
            status: null,
            error: null,
          },
        ],
      });
      const view = renderWithProviders(<AddDialog />, running, 'pt-BR');
      expect(screen.getByTestId('add-dialog-title')).toHaveTextContent(
        'Adicionando 2 jogos ao GameNode',
      );
      expect(screen.getByTestId('add-progress-caption')).toHaveTextContent(
        '1 de 2',
      );
      expect(screen.getByTestId('add-progress-item-status-entry-1')).toHaveTextContent(
        'Adicionado',
      );
      expect(screen.getByTestId('add-progress-item-status-entry-2')).toHaveTextContent(
        'Adicionando…',
      );
      expect(screen.getByTestId('add-progress-hide-btn')).toHaveTextContent(
        'Ocultar',
      );
      view.unmount();

      const editing = makeAddSnapshot({
        appliedStatus: 'planned',
        appliedFrom: 'Backlog',
        newCollection: {
          name: '',
          creating: false,
          canCreate: false,
          error: 'limitReached',
        },
      });
      renderWithProviders(<AddDialog />, editing, 'pt-BR');
      expect(screen.getByTestId('add-applied-from-notice')).toHaveTextContent(
        'Sem escolha, o GameNode usará Na lista, o status padrão de Backlog.',
      );
      expect(screen.getByTestId('add-new-collection-btn')).toHaveTextContent(
        'Nova coleção',
      );
      expect(screen.getByTestId('add-new-collection-error')).toHaveTextContent(
        'Você atingiu o limite de coleções do GameNode.',
      );
      expect(screen.getByTestId('add-collection-count-col-1')).toHaveTextContent(
        '9 jogos',
      );
    });

    it('renders confirm errors and blocked notices in en and pt-BR', () => {
      const unauth = makeAddSnapshot({
        error: 'rejected',
        blocked: 'needsVerifiedSession',
      });
      const { unmount } = renderWithProviders(<AddDialog />, unauth, 'en');
      expect(screen.getByTestId('add-error-notice')).toHaveTextContent(
        'GameNode rejected your session. Sign in again.',
      );
      expect(screen.getByTestId('add-blocked-notice')).toHaveTextContent(
        'Your GameNode session is not verified yet. Retry the connection.',
      );
      unmount();

      const pt = renderWithProviders(<AddDialog />, unauth, 'pt-BR');
      expect(screen.getByTestId('add-error-notice')).toHaveTextContent(
        'O GameNode recusou sua sessão. Entre novamente.',
      );
      expect(screen.getByTestId('add-blocked-notice')).toHaveTextContent(
        'Sua sessão do GameNode ainda não foi verificada. Tente conectar novamente.',
      );
      pt.unmount();

      const failed = renderWithProviders(
        <AddDialog />,
        makeAddSnapshot({ error: 'failed' }),
        'en',
      );
      expect(screen.getByTestId('add-error-notice')).toHaveTextContent(
        'Could not save the game. Your choices are kept; try again.',
      );
      failed.unmount();
    });
  });
});
