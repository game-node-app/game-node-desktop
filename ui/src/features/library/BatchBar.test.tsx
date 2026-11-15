import { describe, expect, it, beforeEach, vi } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import type { Row } from '../../api';
import { resetUiStore } from '../../store/uiStore';
import { BatchBar } from './BatchBar';
import { makeLibrary, renderControls, setInputs, spyLibraryInput } from './controlsTestUtils';
import { currentBatchMatchQueue, resetBatchMatchQueue } from './batchMatchQueue';

function row(overrides: Partial<Row> = {}): Row {
  return {
    key: 'steam:1',
    title: 'Game',
    detectedName: 'Game',
    matchedName: 'Game',
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    running: false,
    status: { matched: { gameId: 1, source: 'automatic' } },
    coverFile: null,
    playtime: { seconds: 3600, source: 'store', lastPlayedAt: null },
    releaseYear: 2020,
    addedAt: 0,
    isFavorite: false,
    hidden: false,
    gamenode: null,
    ...overrides,
  };
}

const baseActions = {
  selected: 2,
  play: { disabled: 'multipleSelected' as const },
  install: { disabled: 'multipleSelected' as const },
  matchGame: { disabled: 'multipleSelected' as const },
  addToGamenode: 'available' as const,
  changeMatch: false,
};

describe('BatchBar', () => {
  beforeEach(() => {
    resetUiStore();
    resetBatchMatchQueue();
    vi.restoreAllMocks();
  });

  it('stays hidden with fewer than two selected games', () => {
    const library = makeLibrary({
      selected: ['steam:1'],
      rows: [row({ key: 'steam:1', title: 'One' })],
      actions: baseActions,
    });
    renderControls(<BatchBar />, library);
    expect(screen.queryByTestId('batch-bar')).not.toBeInTheDocument();
  });

  it('shows the count and sends openAdd for Add to GameNode', () => {
    const spy = spyLibraryInput();
    const library = makeLibrary({
      selected: ['steam:1', 'steam:2'],
      rows: [
        row({ key: 'steam:1', title: 'One' }),
        row({ key: 'steam:2', title: 'Two' }),
      ],
      actions: baseActions,
    });
    renderControls(<BatchBar />, library);

    const bar = screen.getByTestId('batch-bar');
    expect(bar).toHaveTextContent('2');
    // The fixed 52 px height and 878 px width of the reference are floors, not
    // clipping boxes: long translations wrap inside the available width.
    expect(bar).toHaveClass('min-h-[52px]', 'max-w-[calc(100%-32px)]', 'flex-wrap');
    expect(bar).not.toHaveClass('h-[52px]');
    expect(bar).not.toHaveClass('w-[878px]');
    expect(screen.getByTestId('batch-bar-count')).toHaveTextContent('2');
    expect(screen.getByText('games selected')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('batch-bar-add'));
    expect(setInputs(spy)).toEqual(['openAdd']);
  });

  it('sends the selected keys and the chosen status', async () => {
    const spy = spyLibraryInput();
    const library = makeLibrary({
      selected: ['steam:1', 'steam:2'],
      rows: [
        row({ key: 'steam:1', title: 'One', gamenode: { status: 'planned', collectionIds: [], entryId: 'e1' } }),
        row({ key: 'steam:2', title: 'Two' }),
      ],
      actions: baseActions,
    });
    renderControls(<BatchBar />, library);

    fireEvent.click(screen.getByTestId('batch-bar-status'));
    fireEvent.click(await screen.findByTestId('batch-bar-status-finished'));

    expect(setInputs(spy)).toEqual([
      { setStatus: { keys: ['steam:1', 'steam:2'], status: 'finished' } },
    ]);
  });

  it('favors only the matched selected games', () => {
    const spy = spyLibraryInput();
    const library = makeLibrary({
      selected: ['steam:1', 'steam:2', 'steam:3'],
      rows: [
        row({ key: 'steam:1', title: 'One' }),
        row({ key: 'steam:2', title: 'Two' }),
        row({ key: 'steam:3', title: 'Three', status: 'unmatched', isFavorite: true }),
      ],
      actions: baseActions,
    });
    renderControls(<BatchBar />, library);

    expect(screen.getByTestId('batch-bar-favorite')).toHaveTextContent('Add to favorites');
    fireEvent.click(screen.getByTestId('batch-bar-favorite'));

    const inputs = setInputs(spy);
    expect(inputs).toHaveLength(1);
    const input = inputs[0] as { setFavorites: { keys: string[]; favorite: boolean; now: number } };
    expect(input.setFavorites.keys).toEqual(['steam:1', 'steam:2']);
    expect(input.setFavorites.favorite).toBe(true);
    expect(typeof input.setFavorites.now).toBe('number');
  });

  it('reads Unfavorite when every matched selected game is a favorite', () => {
    const spy = spyLibraryInput();
    const library = makeLibrary({
      selected: ['steam:1', 'steam:2'],
      rows: [
        row({ key: 'steam:1', title: 'One', isFavorite: true }),
        row({ key: 'steam:2', title: 'Two', isFavorite: true }),
      ],
      actions: baseActions,
    });
    renderControls(<BatchBar />, library);

    expect(screen.getByTestId('batch-bar-favorite')).toHaveTextContent('Remove from favorites');
    fireEvent.click(screen.getByTestId('batch-bar-favorite'));
    const input = setInputs(spy)[0] as {
      setFavorites: { keys: string[]; favorite: boolean };
    };
    expect(input.setFavorites.favorite).toBe(false);
    expect(input.setFavorites.keys).toEqual(['steam:1', 'steam:2']);
  });

  it('starts the match queue with the unmatched and ambiguous keys', () => {
    const library = makeLibrary({
      access: 'verified',
      selected: ['steam:1', 'steam:2', 'steam:3'],
      rows: [
        row({
          key: 'steam:1',
          title: 'One',
          status: { ambiguous: { candidates: [1, 2] } },
        }),
        row({ key: 'steam:2', title: 'Two', status: 'unmatched' }),
        row({ key: 'steam:3', title: 'Three' }),
      ],
      actions: baseActions,
    });
    renderControls(<BatchBar />, library);

    fireEvent.click(screen.getByTestId('batch-bar-match'));

    const queue = currentBatchMatchQueue();
    expect(queue.current).toBe('steam:1');
    expect(queue.pending).toEqual(['steam:2']);
    expect(queue.originalKeys).toEqual(['steam:1', 'steam:2', 'steam:3']);
    expect(queue.phase).toBe('opening');
  });

  it('disables Match with a reason when no selected game needs review', () => {
    const library = makeLibrary({
      access: 'verified',
      selected: ['steam:1', 'steam:2'],
      rows: [
        row({ key: 'steam:1', title: 'One' }),
        row({ key: 'steam:2', title: 'Two' }),
      ],
      actions: baseActions,
    });
    renderControls(<BatchBar />, library);

    expect(screen.getByTestId('batch-bar-match')).toBeDisabled();
    expect(screen.getByText('Not matched')).toBeInTheDocument();
  });

  it('shows the engine reason for a disabled Add to GameNode', () => {
    const library = makeLibrary({
      selected: ['steam:1', 'steam:2'],
      rows: [
        row({ key: 'steam:1', title: 'One' }),
        row({ key: 'steam:2', title: 'Two' }),
      ],
      actions: { ...baseActions, addToGamenode: { disabled: 'offline' } },
    });
    renderControls(<BatchBar />, library);

    expect(screen.getByTestId('batch-bar-add')).toBeDisabled();
    expect(screen.getByText('GameNode cannot be reached right now.')).toBeInTheDocument();
  });

  it('keeps every action reachable in Brazilian Portuguese', () => {
    const library = makeLibrary({
      selected: ['steam:1', 'steam:2'],
      rows: [row({ key: 'steam:1', title: 'One' }), row({ key: 'steam:2', title: 'Two' })],
      actions: baseActions,
    });
    renderControls(<BatchBar />, library, 'pt-BR');

    const bar = screen.getByTestId('batch-bar');
    expect(bar).toHaveTextContent('jogos selecionados');
    for (const id of [
      'batch-bar-add',
      'batch-bar-status',
      'batch-bar-favorite',
      'batch-bar-match',
      'batch-bar-clear',
    ]) {
      expect(within(bar).getByTestId(id)).toBeInTheDocument();
    }
  });

  it('clears the selection', () => {
    const spy = spyLibraryInput();
    const library = makeLibrary({
      selected: ['steam:1', 'steam:2'],
      rows: [row({ key: 'steam:1', title: 'One' }), row({ key: 'steam:2', title: 'Two' })],
      actions: baseActions,
    });
    renderControls(<BatchBar />, library);

    fireEvent.click(screen.getByTestId('batch-bar-clear'));
    expect(setInputs(spy)).toEqual(['clearSelection']);
  });

  it('disables Change status when no selected game is in the GameNode library', async () => {
    const library = makeLibrary({
      selected: ['steam:1', 'steam:2'],
      rows: [row({ key: 'steam:1', title: 'One' }), row({ key: 'steam:2', title: 'Two' })],
      actions: baseActions,
    });
    renderControls(<BatchBar />, library);

    expect(screen.getByTestId('batch-bar-status')).toBeDisabled();
    await waitFor(() =>
      expect(screen.getByText('Add to GameNode first')).toBeInTheDocument(),
    );
  });
});
