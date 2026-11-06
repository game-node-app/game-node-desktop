import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  commands,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Snapshot,
  type MatchDialogView,
  type AddDialogView,
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

  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
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
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: true,
        },
        search: '',
        access: 'verified',
        total: 1,
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
            },
          ],
          noResults: false,
          canClear: true,
          current: { matched: { gameId: 102, source: 'automatic' } },
          ...overrides,
        },
      };
      return snap;
    };

    it('renders search input, candidates, results, and handles pick match, mark no game, and clear match', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = makeMatchSnapshot();

      renderWithProviders(<MatchDialog />, snapshot, 'en');

      // Header info
      expect(screen.getByTestId('match-dialog-title')).toHaveTextContent(
        'Match to a GameNode game',
      );
      expect(screen.getByTestId('match-dialog-subtitle')).toHaveTextContent(
        'Hades II Early Access from Steam',
      );

      // Search input & submit
      const searchInput = screen.getByTestId('match-search-input');
      expect(searchInput).toHaveValue('Hades');

      fireEvent.change(searchInput, { target: { value: 'Hades II' } });
      expect(libraryInputSpy).toHaveBeenCalledWith({ setMatchQuery: 'Hades II' });

      const searchSubmitBtn = screen.getByTestId('match-search-submit-btn');
      fireEvent.click(searchSubmitBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith('searchMatch');

      // Candidates & Results
      expect(screen.getByTestId('candidate-101')).toHaveTextContent('Hades II');
      expect(screen.getByTestId('candidate-101')).toHaveTextContent('2024');
      expect(screen.getByTestId('result-102')).toHaveTextContent('Hades');
      expect(screen.getByTestId('result-102')).toHaveTextContent('2020');
      expect(screen.getByTestId('match-current-badge')).toHaveTextContent('Current');

      // Pick match
      const pickBtn = screen.getByTestId('match-pick-btn-101');
      fireEvent.click(pickBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith({
        pickMatch: {
          gameId: 101,
          now: expect.any(Number),
        },
      });

      // Mark no game
      const notAGameBtn = screen.getByTestId('match-not-a-game-btn');
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

      // Close button
      const closeBtn = screen.getByTestId('match-close-btn');
      fireEvent.click(closeBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith('closeMatch');

      // Escape key closes match
      fireEvent.keyDown(window, { key: 'Escape' });
      expect(libraryInputSpy).toHaveBeenCalledWith('closeMatch');
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
        'Sugestões do GameNode',
      );
      expect(screen.getByTestId('match-options-container')).toHaveTextContent(
        'Resultados da busca',
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
      expect(screen.getByTestId('match-close-btn')).toHaveTextContent('Fechar');
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
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          filter: 'all',
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: true,
        },
        search: '',
        access: 'verified',
        total: 2,
        providers: [],
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
            { entryKey: 'entry-1', title: 'Hades', matched: true },
            { entryKey: 'entry-2', title: 'Unknown Indie Game', matched: false },
          ],
          editingExisting: false,
          status: 'playing',
          appliedStatus: 'playing',
          appliedFrom: null,
          collections: {
            loaded: [
              { id: 'col-1', name: 'Favorites', defaultStatus: null, picked: false },
              { id: 'col-2', name: 'Backlog', defaultStatus: 'planned', picked: true },
            ],
          },
          canConfirm: true,
          ...overrides,
        },
      };
      return snap;
    };

    it('renders targets, status picker, collections checklist, and handles confirm & close', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = makeAddSnapshot({
        appliedStatus: 'planned',
        appliedFrom: 'Backlog',
      });

      renderWithProviders(<AddDialog />, snapshot, 'en');

      // Title & multiple targets
      expect(screen.getByTestId('add-dialog-title')).toHaveTextContent(
        'Add to GameNode',
      );
      expect(screen.getByTestId('add-multiple-targets')).toHaveTextContent('2 games');
      expect(screen.getByTestId('add-target-entry-1')).toHaveTextContent('Hades');
      expect(screen.getByTestId('add-target-entry-2')).toHaveTextContent(
        'Unknown Indie Game (no GameNode match, will be skipped)',
      );

      // Status buttons
      const finishedBtn = screen.getByTestId('add-status-btn-finished');
      fireEvent.click(finishedBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith({ setAddStatus: 'finished' });

      // Applied status override notice
      expect(screen.getByTestId('add-applied-from-notice')).toHaveTextContent(
        'GameNode will use Planned, the default status of Backlog.',
      );

      // Collections checklist toggle
      const checkboxCol1 = screen.getByTestId('add-collection-checkbox-col-1');
      expect(checkboxCol1).not.toBeChecked();
      fireEvent.click(checkboxCol1);
      expect(libraryInputSpy).toHaveBeenCalledWith({ toggleAddCollection: 'col-1' });

      // Collection with defaultStatus label
      expect(screen.getByTestId('add-collection-item-col-2')).toHaveTextContent(
        'Sets the status to Planned',
      );

      // Confirm button
      const confirmBtn = screen.getByTestId('add-dialog-confirm-btn');
      fireEvent.click(confirmBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith('confirmAdd');

      // Cancel button
      const cancelBtn = screen.getByTestId('add-dialog-cancel-btn');
      fireEvent.click(cancelBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith('closeAdd');
    });

    it('handles collections loading state and progress indicator', () => {
      const loadingSnap = makeAddSnapshot({
        collections: 'loading',
        progress: { done: 1, total: 3 },
      });

      renderWithProviders(<AddDialog />, loadingSnap, 'en');
      expect(screen.getByTestId('add-collections-loading')).toHaveTextContent(
        'Loading your collections…',
      );
      expect(screen.getByTestId('add-progress-caption')).toHaveTextContent(
        'Saving 2 of 3…',
      );
    });

    it('handles collections failed state with retry', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const failedSnap = makeAddSnapshot({
        collections: { failed: 'failed' },
      });

      renderWithProviders(<AddDialog />, failedSnap, 'en');

      expect(screen.getByTestId('add-collections-failed')).toBeInTheDocument();
      const retryBtn = screen.getByTestId('add-collections-retry-btn');
      fireEvent.click(retryBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith('retryCollections');
    });

    it('renders summary mode with item outcomes and supports pt-BR', () => {
      const libraryInputSpy = vi.spyOn(commands, 'libraryInput');
      const snapshot = makeAddSnapshot({
        appliedStatus: 'planned',
        appliedFrom: 'Backlog',
        summary: {
          added: 1,
          alreadyPresent: 1,
          skipped: 0,
          failed: 0,
          items: [
            { entryKey: 'entry-1', title: 'Hades', outcome: 'added' },
            { entryKey: 'entry-2', title: 'Dead Cells', outcome: 'alreadyPresent' },
          ],
        },
      });

      // In English
      const { unmount } = renderWithProviders(<AddDialog />, snapshot, 'en');

      expect(screen.getByTestId('add-summary-headline')).toHaveTextContent(
        'Saved 1 · already in your library 1 · skipped 0 · failed 0',
      );
      expect(screen.getByTestId('add-summary-item-entry-1')).toHaveTextContent(
        'Saved as Playing',
      );
      expect(screen.getByTestId('add-summary-item-entry-2')).toHaveTextContent(
        'Already in your library',
      );

      const closeSummaryBtn = screen.getByTestId('add-dialog-close-btn');
      fireEvent.click(closeSummaryBtn);
      expect(libraryInputSpy).toHaveBeenCalledWith('closeAdd');

      unmount();

      // In Portuguese
      renderWithProviders(<AddDialog />, snapshot, 'pt-BR');

      expect(screen.getByTestId('add-summary-headline')).toHaveTextContent(
        'Salvos 1 · já na sua biblioteca 1 · ignorados 0 · com falha 0',
      );
      expect(screen.getByTestId('add-summary-item-entry-1')).toHaveTextContent(
        'Salvo como Jogando',
      );
      expect(screen.getByTestId('add-summary-item-entry-2')).toHaveTextContent(
        'Já está na sua biblioteca',
      );
      expect(screen.getByTestId('add-dialog-close-btn')).toHaveTextContent('Fechar');
    });

    it('renders unauthorized/failed error notices and blocked notices in en and pt-BR', () => {
      const unauthSnap = makeAddSnapshot({
        error: 'unauthorized',
        blocked: 'needsVerifiedSession',
      });

      // In English
      const { unmount } = renderWithProviders(<AddDialog />, unauthSnap, 'en');
      expect(screen.getByTestId('add-error-notice')).toHaveTextContent(
        'GameNode rejected your session. Sign in again.',
      );
      expect(screen.getByTestId('add-blocked-notice')).toHaveTextContent(
        'Your GameNode session is not verified yet. Retry the connection.',
      );

      unmount();

      // In Portuguese
      const { unmount: unmountPt } = renderWithProviders(<AddDialog />, unauthSnap, 'pt-BR');
      expect(screen.getByTestId('add-error-notice')).toHaveTextContent(
        'O GameNode recusou sua sessão. Entre novamente.',
      );
      expect(screen.getByTestId('add-blocked-notice')).toHaveTextContent(
        'Sua sessão do GameNode ainda não foi verificada. Tente conectar novamente.',
      );

      unmountPt();

      // Failed error
      const failedSnap = makeAddSnapshot({
        error: 'failed',
      });
      const { unmount: unmountFailedEn } = renderWithProviders(<AddDialog />, failedSnap, 'en');
      expect(screen.getByTestId('add-error-notice')).toHaveTextContent(
        'Could not save the game. Your choices are kept; try again.',
      );

      unmountFailedEn();

      renderWithProviders(<AddDialog />, failedSnap, 'pt-BR');
      expect(screen.getByTestId('add-error-notice')).toHaveTextContent(
        'Não foi possível salvar o jogo. Suas escolhas foram mantidas; tente novamente.',
      );
    });
  });
});
