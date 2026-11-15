import { describe, expect, it, beforeEach, vi } from 'vitest';
import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import type { DetailView, LibraryView, Row } from '../../api';
import { commands } from '../../api';
import { openContextMenu, resetUiStore, uiStore } from '../../store/uiStore';
import { ContextMenu } from './ContextMenu';
import { openContextMenuForElement, contextMenuPointFor } from './contextMenuAnchor';
import { makeLibrary, renderControls, setInputs, spyLibraryInput } from './controlsTestUtils';

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
    status: { matched: { gameId: 42, source: 'automatic' } },
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

function detailFor(target: Row, overrides: Partial<DetailView> = {}): DetailView {
  return {
    running: target.running,
    row: target,
    installDir: 'C:\\Games\\Target',
    releaseYear: target.releaseYear,
    hasArtwork: false,
    membership: target.gamenode ? { inLibrary: target.gamenode.status } : 'notInLibrary',
    achievementBackend: true,
    achievements: null,
    suggestions: [],
    ...overrides,
  };
}

const singleActions = {
  selected: 1,
  play: 'available' as const,
  install: 'available' as const,
  matchGame: 'available' as const,
  addToGamenode: { disabled: { alreadyInLibrary: 'playing' } } as const,
  changeMatch: true,
};

const multiActions = {
  selected: 3,
  play: { disabled: 'multipleSelected' as const },
  install: { disabled: 'multipleSelected' as const },
  matchGame: { disabled: 'multipleSelected' as const },
  addToGamenode: 'available' as const,
  changeMatch: false,
};

function open(library: LibraryView, entryKey = 'steam:1') {
  openContextMenu(120, 90, entryKey);
  return renderControls(<ContextMenu />, library);
}

async function reopen(entryKey = 'steam:1') {
  act(() => openContextMenu(120, 90, entryKey));
  await waitFor(() => expect(screen.getByTestId('library-context-menu')).toBeInTheDocument());
}

describe('ContextMenu', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('offers the single-game sections for an installed Steam game in the library', () => {
    const target = row({
      gamenode: { status: 'playing', collectionIds: [], entryId: 'entry-1' },
    });
    const library = makeLibrary({
      rows: [target],
      selected: ['steam:1'],
      detail: detailFor(target),
      actions: singleActions,
      collections: [{ id: 'backlog', name: 'Backlog', count: 4 }],
    });
    open(library);

    expect(screen.getByTestId('library-context-menu')).toBeInTheDocument();
    expect(screen.getByTestId('context-menu-play')).toBeInTheDocument();
    expect(screen.getByTestId('context-menu-open-folder')).toBeInTheDocument();
    expect(screen.getByTestId('context-menu-edit-gamenode')).toBeInTheDocument();
    expect(screen.getByTestId('context-menu-match')).toHaveTextContent('Change match…');
    expect(screen.getByTestId('context-menu-favorite')).toHaveTextContent('Add to favorites');
    expect(screen.getByTestId('context-menu-collection')).toHaveTextContent('Add to collection');
    expect(screen.getByTestId('context-menu-tools')).toHaveTextContent('Game tools…');
    expect(screen.getByTestId('context-menu-achievements')).toBeInTheDocument();
    expect(screen.getByTestId('context-menu-hide')).toHaveTextContent('Hide from library');
    expect(screen.queryByTestId('context-menu-install')).not.toBeInTheDocument();
    expect(screen.queryByTestId('context-menu-remove')).not.toBeInTheDocument();
  });

  it('anchors at the pointer near the footer without correcting for a menu height', () => {
    const target = row();
    const library = makeLibrary({
      rows: [target],
      selected: ['steam:1'],
      detail: detailFor(target),
      actions: singleActions,
    });
    // jsdom is 1024x768: y=731 is inside the viewport, but the old
    // `viewportHeight - 420` bound would have pulled the menu up to y=348.
    openContextMenu(120, 731, 'steam:1');
    renderControls(<ContextMenu />, library);

    const anchor = screen.getByTestId('library-context-menu-anchor');
    expect(anchor.style.position).toBe('fixed');
    expect(anchor.style.left).toBe('120px');
    expect(anchor.style.top).toBe('731px');
    expect(anchor.style.width).toBe('0px');
    expect(anchor.style.height).toBe('0px');

    // Past the right edge the menu width still keeps the whole dropdown on screen.
    act(() => openContextMenu(5000, 731, 'steam:1'));
    expect(screen.getByTestId('library-context-menu-anchor').style.left).toBe(
      `${window.innerWidth - 280 - 8}px`,
    );
  });

  it('opens the install folder and the GameNode page for the single entry', async () => {
    const folderSpy = vi.spyOn(commands, 'openInstallFolder').mockResolvedValue(undefined);
    const pageSpy = vi.spyOn(commands, 'openGameNodePage').mockResolvedValue(undefined);
    const target = row({
      gamenode: { status: 'playing', collectionIds: [], entryId: 'entry-1' },
    });
    const library = makeLibrary({
      rows: [target],
      selected: ['steam:1'],
      detail: detailFor(target),
      actions: singleActions,
    });
    open(library);

    fireEvent.click(screen.getByTestId('context-menu-open-folder'));
    expect(folderSpy).toHaveBeenCalledWith('C:\\Games\\Target');

    await reopen();
    fireEvent.click(screen.getByTestId('context-menu-edit-gamenode'));
    expect(pageSpy).toHaveBeenCalledWith({ game: { id: 42 } });
  });

  it('dispatches Play, Install, Match, Add and Game tools exactly once', async () => {
    const spy = spyLibraryInput();
    const launchSpy = vi.spyOn(commands, 'launchSelected').mockResolvedValue(undefined);
    const toolsSpy = vi.spyOn(commands, 'openTools').mockResolvedValue(undefined);
    const target = row({ installed: false, status: 'unmatched', gamenode: null });
    const library = makeLibrary({
      rows: [target],
      selected: ['steam:1'],
      detail: detailFor(target),
      actions: {
        ...singleActions,
        addToGamenode: 'available',
        changeMatch: false,
      },
    });
    open(library);

    fireEvent.click(screen.getByTestId('context-menu-play'));
    expect(launchSpy).toHaveBeenCalledWith('play');

    await reopen();
    fireEvent.click(screen.getByTestId('context-menu-install'));
    expect(launchSpy).toHaveBeenCalledWith('install');

    await reopen();
    fireEvent.click(screen.getByTestId('context-menu-match'));
    expect(setInputs(spy).at(-1)).toBe('openMatch');

    await reopen();
    fireEvent.click(screen.getByTestId('context-menu-add'));
    expect(setInputs(spy).at(-1)).toBe('openAdd');

    await reopen();
    fireEvent.click(screen.getByTestId('context-menu-tools'));
    expect(toolsSpy).toHaveBeenCalledWith('steam:1');
  });

  it('reports a failed web link instead of silently succeeding', async () => {
    vi.spyOn(commands, 'openGameNodePage').mockRejectedValue('The link could not be opened.');
    const target = row({
      gamenode: { status: 'playing', collectionIds: [], entryId: 'entry-1' },
    });
    const library = makeLibrary({
      rows: [target],
      selected: ['steam:1'],
      detail: detailFor(target),
      actions: singleActions,
    });
    open(library);

    fireEvent.click(screen.getByTestId('context-menu-edit-gamenode'));
    await waitFor(() =>
      expect(uiStore.state.toasts).toEqual([
        expect.objectContaining({ kind: 'danger', title: 'The link could not be opened.' }),
      ]),
    );
  });

  it('preserves the DRM-free actions and omits Install', () => {
    const target = row({
      key: 'manual:5',
      title: 'Local Game',
      provider: 'manual',
      providerName: 'DRM-free',
      status: 'unmatched',
      gamenode: null,
    });
    const library = makeLibrary({
      rows: [target],
      selected: ['manual:5'],
      detail: detailFor(target),
      actions: {
        ...singleActions,
        install: { disabled: 'noInstallRule' },
        addToGamenode: { disabled: 'notMatched' },
        changeMatch: false,
      },
    });
    open(library, 'manual:5');

    expect(screen.getByTestId('context-menu-play')).toBeInTheDocument();
    expect(screen.queryByTestId('context-menu-install')).not.toBeInTheDocument();
    expect(screen.getByTestId('context-menu-save-protection')).toBeInTheDocument();
    expect(screen.getByTestId('context-menu-settings')).toHaveTextContent('Game settings…');
    expect(screen.getByTestId('context-menu-locate')).toHaveTextContent('Locate executable…');
    expect(screen.getByTestId('context-menu-remove')).toHaveTextContent('Remove from library…');
    expect(screen.getByTestId('context-menu-match')).toHaveTextContent('Match…');
    expect(screen.getByTestId('context-menu-add')).toBeDisabled();
    expect(screen.getByTestId('context-menu-collection-disabled')).toHaveTextContent(
      'Add to GameNode first',
    );
    expect(screen.queryByTestId('context-menu-favorite')).not.toBeInTheDocument();
  });

  it('sends the DRM-free local actions with the registration id', async () => {
    const historySpy = vi.spyOn(commands, 'loadLocalHistory').mockResolvedValue(undefined);
    const locateSpy = vi.spyOn(commands, 'pickLocalExecutable').mockResolvedValue(undefined);
    const target = row({
      key: 'manual:5',
      title: 'Local Game',
      provider: 'manual',
      providerName: 'DRM-free',
      status: 'unmatched',
    });
    const library = makeLibrary({
      rows: [target],
      selected: ['manual:5'],
      detail: detailFor(target),
      actions: { ...singleActions, install: { disabled: 'noInstallRule' } },
    });
    open(library, 'manual:5');

    fireEvent.click(screen.getByTestId('context-menu-save-protection'));
    expect(uiStore.state.saveProtectionEntryKey).toBe('manual:5');
    expect(historySpy).toHaveBeenCalledWith('5');

    await reopen('manual:5');
    fireEvent.click(screen.getByTestId('context-menu-locate'));
    expect(locateSpy).toHaveBeenCalledWith('5');

    await reopen('manual:5');
    fireEvent.click(screen.getByTestId('context-menu-remove'));
    expect(uiStore.state.removeModalEntryKey).toBe('manual:5');
  });

  it('offers Match and a disabled Add for an unmatched store entry', () => {
    const target = row({ status: 'unmatched', gamenode: null });
    const library = makeLibrary({
      rows: [target],
      selected: ['steam:1'],
      detail: detailFor(target),
      actions: {
        ...singleActions,
        matchGame: 'available',
        addToGamenode: { disabled: 'notMatched' },
        changeMatch: false,
      },
    });
    open(library);

    expect(screen.getByTestId('context-menu-match')).toHaveTextContent('Match…');
    expect(screen.getByTestId('context-menu-add')).toBeDisabled();
    expect(screen.getByTestId('context-menu-add')).toHaveTextContent(
      'Match this game to a GameNode game first.',
    );
    expect(screen.queryByTestId('context-menu-favorite')).not.toBeInTheDocument();
    expect(screen.getByTestId('context-menu-collection-disabled')).toHaveTextContent(
      'Add to GameNode first',
    );
  });

  it('adds one entry to a collection from the submenu', async () => {
    const spy = spyLibraryInput();
    const target = row({
      gamenode: { status: 'playing', collectionIds: [], entryId: 'entry-1' },
    });
    const library = makeLibrary({
      rows: [target],
      selected: ['steam:1'],
      detail: detailFor(target),
      actions: singleActions,
      collections: [{ id: 'backlog', name: 'Backlog', count: 1 }],
    });
    open(library);

    fireEvent.click(screen.getByTestId('context-menu-collection'));
    fireEvent.click(await screen.findByTestId('context-menu-collection-backlog'));

    expect(setInputs(spy).at(-1)).toEqual({
      addToCollection: { keys: ['steam:1'], collectionId: 'backlog' },
    });
  });

  it('shows Show in library for a hidden entry and unhides exactly it', () => {
    const spy = spyLibraryInput();
    const target = row({ hidden: true });
    const library = makeLibrary({
      rows: [target],
      selected: ['steam:1'],
      detail: detailFor(target),
      actions: singleActions,
    });
    open(library);

    fireEvent.click(screen.getByTestId('context-menu-show'));
    expect(setInputs(spy)).toEqual([{ setHidden: { keys: ['steam:1'], hidden: false } }]);
  });

  it('offers only the multi-selection subset and sends the batch inputs', async () => {
    const spy = spyLibraryInput();
    const rows = [
      row({
        key: 'steam:1',
        title: 'One',
        isFavorite: true,
        gamenode: { status: 'playing', collectionIds: [], entryId: 'e1' },
      }),
      row({
        key: 'steam:2',
        title: 'Two',
        isFavorite: true,
        gamenode: { status: 'planned', collectionIds: [], entryId: 'e2' },
      }),
      row({
        key: 'steam:3',
        title: 'Three',
        isFavorite: true,
        gamenode: { status: 'finished', collectionIds: [], entryId: 'e3' },
      }),
    ];
    const library = makeLibrary({
      rows,
      selected: ['steam:1', 'steam:2', 'steam:3'],
      actions: multiActions,
      collections: [{ id: 'backlog', name: 'Backlog', count: 3 }],
    });
    open(library, 'steam:1');

    expect(screen.queryByTestId('context-menu-play')).not.toBeInTheDocument();
    expect(screen.queryByTestId('context-menu-tools')).not.toBeInTheDocument();
    expect(screen.queryByTestId('context-menu-match')).not.toBeInTheDocument();
    expect(screen.queryByTestId('context-menu-change-status')).toBeInTheDocument();
    expect(screen.getByTestId('context-menu-add')).toBeInTheDocument();
    expect(screen.getByTestId('context-menu-hide')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('context-menu-favorite'));
    expect(setInputs(spy).at(-1)).toEqual({
      setFavorites: {
        keys: ['steam:1', 'steam:2', 'steam:3'],
        favorite: false,
        now: expect.any(Number),
      },
    });

    await reopen();
    fireEvent.click(screen.getByTestId('context-menu-change-status'));
    fireEvent.click(await screen.findByTestId('context-menu-status-finished'));
    expect(setInputs(spy).at(-1)).toEqual({
      setStatus: { keys: ['steam:1', 'steam:2', 'steam:3'], status: 'finished' },
    });

    await reopen();
    fireEvent.click(screen.getByTestId('context-menu-hide'));
    expect(setInputs(spy).at(-1)).toEqual({
      setHidden: { keys: ['steam:1', 'steam:2', 'steam:3'], hidden: true },
    });
  });

  it('adds the selected in-library games to a collection', async () => {
    const spy = spyLibraryInput();
    const rows = [
      row({ key: 'steam:1', title: 'One', gamenode: { status: 'playing', collectionIds: [], entryId: 'e1' } }),
      row({ key: 'steam:2', title: 'Two', status: 'unmatched' }),
      row({ key: 'steam:3', title: 'Three', gamenode: { status: 'planned', collectionIds: ['backlog'], entryId: 'e3' } }),
    ];
    const library = makeLibrary({
      rows,
      selected: ['steam:1', 'steam:2', 'steam:3'],
      actions: multiActions,
      collections: [{ id: 'backlog', name: 'Backlog', count: 3 }],
    });
    open(library, 'steam:1');

    fireEvent.click(screen.getByTestId('context-menu-collection'));
    fireEvent.click(await screen.findByTestId('context-menu-collection-backlog'));

    expect(setInputs(spy).at(-1)).toEqual({
      addToCollection: { keys: ['steam:1', 'steam:3'], collectionId: 'backlog' },
    });
  });

  it('opens the achievements modal for a provider with an achievement backend', async () => {
    const target = row();
    const library = makeLibrary({
      rows: [target],
      selected: ['steam:1'],
      detail: detailFor(target),
      actions: singleActions,
    });
    open(library);

    fireEvent.click(screen.getByTestId('context-menu-achievements'));
    await waitFor(() => expect(screen.getByText('Game')).toBeInTheDocument());
    expect(screen.queryByTestId('library-context-menu')).not.toBeInTheDocument();
  });
});

describe('context menu keyboard anchor', () => {
  beforeEach(() => {
    resetUiStore();
  });

  it('anchors at the centre of a rectangle instead of the viewport origin', () => {
    expect(contextMenuPointFor({ left: 100, top: 50, width: 200, height: 100 })).toEqual({
      x: 200,
      y: 100,
    });
    expect(contextMenuPointFor(null, { width: 1000, height: 600 })).toEqual({ x: 500, y: 300 });
  });

  it('opens from a focused container with Shift+F10 at the focused row centre', () => {
    function FocusedLibrary() {
      return (
        <div
          data-testid="focused-library"
          tabIndex={0}
          onKeyDown={(e) => {
            if (e.key === 'F10' && e.shiftKey) {
              openContextMenuForElement(e.currentTarget, 'steam:1');
              e.preventDefault();
            }
          }}
        />
      );
    }

    renderControls(<FocusedLibrary />);
    const focused = screen.getByTestId('focused-library');
    focused.getBoundingClientRect = () =>
      ({ left: 300, top: 200, width: 400, height: 100 }) as DOMRect;
    focused.focus();

    fireEvent.keyDown(focused, { key: 'F10', shiftKey: true });

    expect(uiStore.state.contextMenu).toEqual({
      open: true,
      x: 500,
      y: 250,
      entryKey: 'steam:1',
    });
    expect(uiStore.state.contextMenu.x).not.toBe(0);
    expect(uiStore.state.contextMenu.y).not.toBe(0);
  });
});
