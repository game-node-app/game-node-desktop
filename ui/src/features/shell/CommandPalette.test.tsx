import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { commands } from '../../api/commands';
import {
  createDefaultFilters,
  createDefaultLibraryLayout,
  type LibraryView,
  type PaletteEntry,
  type Row,
} from '../../api/types';
import {
  openPalette,
  resetUiStore,
  setActiveDestination,
  uiStore,
} from '../../store/uiStore';
import { CommandPalette } from './CommandPalette';
import { makeLibrary, renderControls, setInputs } from '../library/controlsTestUtils';

function row(overrides: Partial<Row> & Pick<Row, 'key' | 'title'>): Row {
  return {
    detectedName: overrides.title,
    matchedName: null,
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
    ...overrides,
  };
}

const EMBER: PaletteEntry = {
  key: 'steam:1',
  title: 'Emberfall',
  providerName: 'Steam',
  installed: true,
  playtimeLabel: '12 h',
};

function game(index: number): PaletteEntry {
  return {
    key: `steam:${index}`,
    title: `Game ${index}`,
    providerName: 'Steam',
    installed: true,
    playtimeLabel: null,
  };
}

function paletteLibrary(overrides: Partial<LibraryView> = {}): LibraryView {
  return makeLibrary({
    rows: [row({ key: 'steam:1', title: 'Emberfall' })],
    paletteIndex: [EMBER, ...Array.from({ length: 7 }, (_, index) => game(index + 2))],
    ...overrides,
  });
}

function openPaletteWith(library = paletteLibrary()) {
  renderControls(<CommandPalette />, library);
  act(() => openPalette());
  return screen.getByTestId('command-palette-input');
}

describe('CommandPalette', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('groups games, actions and settings with at most five of each', () => {
    openPaletteWith();

    const dialog = screen.getByTestId('command-palette');
    expect(dialog).toBeInTheDocument();
    // 600 px wide, top-anchored at 96 px, both exact px under the 14px root.
    expect(dialog).toHaveClass('w-[600px]', 'top-[96px]');
    const games = within(screen.getByTestId('palette-group-games')).getAllByRole('option');
    const actions = within(screen.getByTestId('palette-group-actions')).getAllByRole('option');
    const settings = within(screen.getByTestId('palette-group-settings')).getAllByRole('option');

    expect(games).toHaveLength(5);
    expect(games[0]).toHaveTextContent('Emberfall');
    expect(games[0]).toHaveTextContent('Steam · Installed · 12 h');
    expect(actions.length).toBeGreaterThan(0);
    expect(actions.length).toBeLessThanOrEqual(5);
    expect(settings).toHaveLength(5);
  });

  it('filters the engine palette index by title substrings', () => {
    const input = openPaletteWith();
    fireEvent.change(input, { target: { value: 'ember' } });

    const games = within(screen.getByTestId('palette-group-games')).getAllByRole('option');
    expect(games).toHaveLength(1);
    expect(games[0]).toHaveTextContent('Emberfall');
    expect(screen.queryByTestId('palette-group-actions')).not.toBeInTheDocument();
  });

  it('moves the active option with the arrow keys', () => {
    const input = openPaletteWith();
    const first = screen.getByTestId('palette-option-0');
    expect(first).toHaveAttribute('aria-selected', 'true');
    expect(input).toHaveAttribute('aria-activedescendant', first.id);

    fireEvent.keyDown(input, { key: 'ArrowDown' });
    const second = screen.getByTestId('palette-option-1');
    expect(second).toHaveAttribute('aria-selected', 'true');
    expect(input).toHaveAttribute('aria-activedescendant', second.id);

    fireEvent.keyDown(input, { key: 'ArrowUp' });
    expect(first).toHaveAttribute('aria-selected', 'true');
  });

  it('selects a visible installed game and launches it with Enter', async () => {
    const inputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const launch = vi.spyOn(commands, 'launchSelected').mockResolvedValue();
    const input = openPaletteWith();

    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => expect(launch).toHaveBeenCalledWith('play'));
    expect(uiStore.state.paletteOpen).toBe(false);
    expect(setInputs(inputSpy)).toEqual([{ select: { key: 'steam:1', mode: 'replace' } }]);
  });

  it('reveals a game hidden by the current view before selecting and launching it', async () => {
    const inputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const launch = vi.spyOn(commands, 'launchSelected').mockResolvedValue();
    const filters = createDefaultFilters();
    filters.stores = ['steam'];
    const library = paletteLibrary({
      rows: [],
      search: 'hollow',
      layout: {
        ...createDefaultLibraryLayout(),
        scope: 'installed',
        group: 'store',
        collapsedGroups: { store: ['steam'] },
        filters,
      },
    });
    const input = openPaletteWith(library);

    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => expect(launch).toHaveBeenCalledWith('play'));
    expect(setInputs(inputSpy)).toEqual([
      { setSearch: '' },
      { setFilters: createDefaultFilters() },
      { setScope: 'all' },
      { setAllGroupsCollapsed: false },
      { select: { key: 'steam:1', mode: 'replace' } },
    ]);
    const inputOrder = inputSpy.mock.invocationCallOrder;
    expect(Math.max(...inputOrder)).toBeLessThan(launch.mock.invocationCallOrder[0]);
  });

  it('opens the details of a game that is not installed', async () => {
    const inputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = paletteLibrary({
      rows: [],
      paletteIndex: [{ ...EMBER, installed: false, playtimeLabel: null }],
    });
    const input = openPaletteWith(library);

    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() =>
      expect(setInputs(inputSpy)).toContainEqual('openDetails'),
    );
    expect(uiStore.state.activeDestination).toBe('library');
    expect(uiStore.state.paletteOpen).toBe(false);
  });

  it('opens the game context menu with Shift+F10 on the library, never at the origin', async () => {
    act(() => setActiveDestination('catalog'));
    const input = openPaletteWith();

    fireEvent.keyDown(input, { key: 'F10', shiftKey: true });

    await waitFor(() => expect(uiStore.state.contextMenu.open).toBe(true));
    expect(uiStore.state.contextMenu.entryKey).toBe('steam:1');
    expect(uiStore.state.paletteOpen).toBe(false);
    // The library screen owns the context menu surface; the palette must activate it.
    expect(uiStore.state.activeDestination).toBe('library');
    expect(
      uiStore.state.contextMenu.x === 0 && uiStore.state.contextMenu.y === 0,
    ).toBe(false);
  });

  it('offers Search on GameNode when no game matches and applies the query once', async () => {
    const inputSpy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const input = openPaletteWith();

    fireEvent.change(input, { target: { value: 'zelda' } });
    expect(setInputs(inputSpy)).toEqual([]);

    const action = screen.getByRole('option', { name: /Search “zelda” on GameNode/ });
    fireEvent.click(action);

    await waitFor(() => expect(uiStore.state.activeDestination).toBe('catalog'));
    expect(setInputs(inputSpy)).toEqual([{ catalogSearch: 'zelda' }]);
    expect(uiStore.state.paletteOpen).toBe(false);
  });

  it('runs the add, rescan, shortcuts and view actions', () => {
    const pickLocal = vi.spyOn(commands, 'pickLocalExecutable').mockResolvedValue();
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue();
    const input = vi.spyOn(commands, 'libraryInput').mockResolvedValue();

    openPaletteWith();
    fireEvent.click(screen.getByRole('option', { name: /Add DRM-free game…/ }));
    expect(pickLocal).toHaveBeenCalledWith(null);
    expect(uiStore.state.paletteOpen).toBe(false);

    act(() => openPalette());
    fireEvent.click(screen.getByRole('option', { name: /Rescan library/ }));
    expect(rescan).toHaveBeenCalledTimes(1);

    act(() => openPalette());
    fireEvent.click(screen.getByRole('option', { name: /Open keyboard shortcuts/ }));
    expect(uiStore.state.shortcutsOpen).toBe(true);

    act(() => openPalette());
    fireEvent.click(screen.getByRole('option', { name: /Toggle grid \/ list/ }));
    expect(uiStore.state.activeDestination).toBe('library');
    expect(setInputs(input)).toContainEqual({ setView: 'list' });
  });

  it('opens a Settings topic from a settings result', () => {
    const input = openPaletteWith();
    fireEvent.change(input, { target: { value: 'playtime' } });

    fireEvent.click(
      screen.getByRole('option', { name: /Settings › Account › Playtime source/ }),
    );

    expect(uiStore.state.settingsOpen).toBe(true);
    expect(uiStore.state.settingsTopic).toBe('account');
    expect(uiStore.state.paletteOpen).toBe(false);
  });

  it('reaches Settings › Stores › Family member from "family"', () => {
    openPaletteWith();
    fireEvent.change(screen.getByTestId('command-palette-input'), {
      target: { value: 'family' },
    });

    fireEvent.click(
      screen.getByRole('option', { name: /Settings › Stores › Family member/ }),
    );

    expect(uiStore.state.settingsOpen).toBe(true);
    expect(uiStore.state.settingsTopic).toBe('stores');
    expect(uiStore.state.paletteOpen).toBe(false);
  });

  it('reaches the same setting from "família" in pt-BR', () => {
    renderControls(<CommandPalette />, paletteLibrary(), 'pt-BR');
    act(() => openPalette());
    fireEvent.change(screen.getByTestId('command-palette-input'), {
      target: { value: 'família' },
    });

    fireEvent.click(
      screen.getByRole('option', { name: /Configurações › Lojas › Membro da família/ }),
    );

    expect(uiStore.state.settingsOpen).toBe(true);
    expect(uiStore.state.settingsTopic).toBe('stores');
  });

  it('traps Tab, closes with Escape and restores focus to the trigger', async () => {
    const user = userEvent.setup();

    function Harness() {
      return (
        <>
          <button type="button" onClick={openPalette}>
            Open palette
          </button>
          <CommandPalette />
        </>
      );
    }

    renderControls(<Harness />, paletteLibrary());
    const trigger = screen.getByRole('button', { name: 'Open palette' });
    await user.click(trigger);

    const input = screen.getByTestId('command-palette-input');
    await waitFor(() => expect(input).toHaveFocus());

    for (let index = 0; index < 12; index += 1) {
      await user.tab();
    }
    const dialog = screen.getByTestId('command-palette');
    expect(dialog).toContainElement(document.activeElement as HTMLElement);

    await user.keyboard('{Escape}');
    await waitFor(() =>
      expect(screen.queryByTestId('command-palette')).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    expect(uiStore.state.paletteOpen).toBe(false);
  });

  it('closes without running anything when the backdrop is clicked', () => {
    openPaletteWith();
    fireEvent.click(screen.getByTestId('command-palette-layer').firstElementChild!);
    expect(uiStore.state.paletteOpen).toBe(false);
  });

  it('renders the palette copy in pt-BR', () => {
    renderControls(<CommandPalette />, paletteLibrary(), 'pt-BR');
    act(() => openPalette());
    expect(screen.getByTestId('command-palette-input')).toHaveAttribute(
      'placeholder',
      'Buscar jogos, ações e configurações',
    );
    expect(screen.getByText('Jogos')).toBeInTheDocument();
    expect(screen.getByText('Ações')).toBeInTheDocument();
  });
});
