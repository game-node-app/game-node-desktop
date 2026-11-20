import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { commands, type LibraryView, type Row, type RowGroup } from '../../api';
import { uiStore, resetUiStore } from '../../store/uiStore';
import { GameGrid, tilePrimaryAction } from './GameGrid';
import {
  buildGridItems,
  columnsForWidth,
  GRID_GROUP_HEADER_HEIGHT,
  GRID_LINE_HEIGHT,
  gridItemHeight,
  gridLineHeight,
  gridTileHeight,
  gridTileWidth,
} from './libraryGroups';
import { makeLibrary, renderControls, spyLibraryInput, setInputs } from './controlsTestUtils';

const NOW = Math.floor(new Date('2026-11-15T12:00:00Z').getTime() / 1000);

let clientWidth = 800;
let clientHeight = 600;

function makeRow(overrides: Partial<Row> = {}): Row {
  return {
    key: 'steam:1',
    title: 'Emberfall: Iron Trail',
    detectedName: 'Emberfall',
    matchedName: 'Emberfall: Iron Trail',
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    running: false,
    status: { matched: { gameId: 1, source: 'automatic' } },
    coverFile: 'emberfall-cover.svg',
    playtime: { seconds: 42 * 3600, source: 'store', lastPlayedAt: NOW - 86_400 },
    releaseYear: 2024,
    addedAt: NOW - 10 * 86_400,
    isFavorite: false,
    hidden: false,
    gamenode: { status: 'playing', collectionIds: [], entryId: 'entry-1' },
    ...overrides,
  };
}

function rowGroup(
  id: string,
  labelKey: string,
  count: number,
  firstIndex: number,
  collapsed = false,
): RowGroup {
  return { id, labelKey, count, collapsed, firstIndex };
}

function gridLibrary(
  rows: Row[],
  overrides: Partial<LibraryView> = {},
): LibraryView {
  const base = makeLibrary();
  return makeLibrary({
    rows,
    total: rows.length,
    selected: [],
    focused: rows[0]?.key ?? null,
    ...overrides,
    layout: { ...base.layout, ...(overrides.layout ?? {}) },
  });
}

function setClientSize(width: number, height: number) {
  clientWidth = width;
  clientHeight = height;
}

beforeEach(() => {
  resetUiStore();
  vi.restoreAllMocks();
  setClientSize(800, 600);
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get: () => clientWidth,
  });
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get: () => clientHeight,
  });
  if (!Element.prototype.scrollTo) {
    Element.prototype.scrollTo = (() => undefined) as typeof Element.prototype.scrollTo;
  }
});

afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = '';
});

describe('GameGrid geometry helpers', () => {
  it('turns a real width into the reference column count', () => {
    expect(columnsForWidth(800)).toBe(4);
    expect(columnsForWidth(1200)).toBe(6);
    expect(columnsForWidth(420)).toBe(2);
    expect(columnsForWidth(180)).toBe(1);
  });

  it('chunks lines inside a group and never across groups', () => {
    const rows = [
      makeRow({ key: 's1' }),
      makeRow({ key: 's2' }),
      makeRow({ key: 's3' }),
      makeRow({ key: 'e1', provider: 'epic', providerName: 'Epic Games Store' }),
      makeRow({ key: 'e2', provider: 'epic', providerName: 'Epic Games Store' }),
    ];
    const groups = [
      rowGroup('steam', 'Steam', 3, 0),
      rowGroup('epic', 'Epic Games Store', 2, 3),
    ];

    const items = buildGridItems(rows, groups, 2);
    expect(items.map((item) => item.kind)).toEqual([
      'group',
      'line',
      'line',
      'group',
      'line',
    ]);
    const lines = items.filter((item) => item.kind === 'line');
    expect(lines[0].kind === 'line' && lines[0].rows.map((row) => row.key)).toEqual([
      's1',
      's2',
    ]);
    expect(lines[0].kind === 'line' && lines[0].groupId).toBe('steam');
    expect(lines[1].kind === 'line' && lines[1].rows.map((row) => row.key)).toEqual([
      's3',
    ]);
    expect(lines[1].kind === 'line' && lines[1].groupId).toBe('steam');
    expect(lines[2].kind === 'line' && lines[2].rows.map((row) => row.key)).toEqual([
      'e1',
      'e2',
    ]);
    expect(lines[2].kind === 'line' && lines[2].groupId).toBe('epic');
    for (const line of lines) {
      expect(line.kind === 'line' && line.rows.length).toBeLessThanOrEqual(2);
    }
  });

  it('keeps a collapsed group header but contributes no line', () => {
    const rows = [makeRow({ key: 'e1' }), makeRow({ key: 'e2' })];
    const groups = [
      rowGroup('steam', 'Steam', 3, 0, true),
      rowGroup('epic', 'Epic Games Store', 2, 0),
    ];

    const items = buildGridItems(rows, groups, 2);
    expect(items.map((item) => item.kind)).toEqual(['group', 'group', 'line']);
    expect(items[0].kind === 'group' && items[0].group.collapsed).toBe(true);
    expect(items[2].kind === 'line' && items[2].rows.map((row) => row.key)).toEqual([
      'e1',
      'e2',
    ]);
  });

  it('uses the reference group and line heights', () => {
    expect(GRID_GROUP_HEADER_HEIGHT).toBe(36);
    expect(GRID_LINE_HEIGHT).toBe(286);
  });

  it('derives the tile and line sizes from the cover-size preference', () => {
    expect(gridTileWidth('small')).toBe(140);
    expect(gridTileWidth('medium')).toBe(172);
    expect(gridTileWidth('large')).toBe(216);

    expect(gridTileHeight('small')).toBe(227);
    expect(gridTileHeight('medium')).toBe(270);
    expect(gridTileHeight('large')).toBe(329);
    expect(gridLineHeight('small')).toBe(243);
    expect(gridLineHeight('medium')).toBe(286);
    expect(gridLineHeight('large')).toBe(345);

    const line = buildGridItems([makeRow()], [], 1)[0];
    expect(gridItemHeight(line, 'large')).toBe(345);
    expect(gridItemHeight(line)).toBe(286);
  });

  it('fits fewer covers per row as the cover size grows', () => {
    expect(columnsForWidth(1200, 'small')).toBe(7);
    expect(columnsForWidth(1200, 'medium')).toBe(6);
    expect(columnsForWidth(1200, 'large')).toBe(5);
  });
});

describe('tilePrimaryAction', () => {
  it('offers Play installed, Install owned-but-missing, and nothing while running', () => {
    expect(tilePrimaryAction(makeRow({ installed: true }))).toBe('play');
    expect(
      tilePrimaryAction(makeRow({ installed: false, owned: true })),
    ).toBe('install');
    expect(
      tilePrimaryAction(makeRow({ installed: false, owned: false, shared: true })),
    ).toBe('install');
    expect(
      tilePrimaryAction(makeRow({ installed: false, owned: true, provider: 'drm-free' })),
    ).toBeUndefined();
    expect(tilePrimaryAction(makeRow({ running: true }))).toBeUndefined();
  });
});

describe('GameGrid', () => {
  it('renders the engine group order with full-width, counted headers', async () => {
    const rows = [
      makeRow({ key: 's1', title: 'Steam One' }),
      makeRow({ key: 's2', title: 'Steam Two' }),
      makeRow({ key: 'e1', title: 'Epic One', provider: 'epic', providerName: 'Epic Games Store' }),
    ];
    const groups = [
      rowGroup('steam', 'Steam', 2, 0),
      rowGroup('epic', 'Epic Games Store', 1, 2),
    ];
    setClientSize(420, 600);

    renderControls(
      <GameGrid />,
      gridLibrary(rows, { groups, layout: { ...makeLibrary().layout, group: 'store' } }),
    );

    await waitFor(() => expect(screen.getByTestId('group-header-steam')).toBeInTheDocument());
    const steam = screen.getByTestId('group-header-steam');
    const epic = screen.getByTestId('group-header-epic');
    expect(steam).toHaveClass('w-full');
    expect(steam).toHaveAttribute('data-group-header', 'steam');
    expect(screen.getByTestId('game-tile-s1').parentElement).toHaveAttribute(
      'data-row-key',
      's1',
    );
    expect(within(steam).getByText('Steam')).toBeInTheDocument();
    expect(within(steam).getByText('2 games')).toBeInTheDocument();
    expect(within(epic).getByText('1 game')).toBeInTheDocument();
    expect(
      steam.compareDocumentPosition(epic) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByTestId('game-tile-s1')).toBeInTheDocument();
    expect(screen.getByTestId('game-tile-e1')).toBeInTheDocument();
  });

  it('keeps a collapsed group header, count and no games, and toggles it', async () => {
    const input = spyLibraryInput();
    const rows = [makeRow({ key: 'e1' }), makeRow({ key: 'e2' })];
    const groups = [
      rowGroup('steam', 'Steam', 3, 0, true),
      rowGroup('epic', 'Epic Games Store', 2, 0),
    ];
    setClientSize(420, 600);

    renderControls(
      <GameGrid />,
      gridLibrary(rows, { groups, layout: { ...makeLibrary().layout, group: 'store' } }),
    );

    const steam = await screen.findByTestId('group-header-steam');
    const toggle = within(steam).getByRole('button');
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(within(steam).getByText('3 games')).toBeInTheDocument();
    expect(screen.queryByTestId('game-tile-s1')).not.toBeInTheDocument();

    fireEvent.click(toggle);
    expect(setInputs(input)).toContainEqual({ toggleGroup: { id: 'steam' } });
  });

  it('sends the first measured column count even without a resize', async () => {
    const input = spyLibraryInput();
    renderControls(<GameGrid />, gridLibrary([makeRow({ key: 'a' })]));

    await waitFor(() =>
      expect(setInputs(input)).toContainEqual({ setColumns: 4 }),
    );
  });

  it.each([
    ['small', 7, 'h-[227px]', 'w-[140px]', 156],
    ['medium', 6, 'h-[270px]', 'w-[172px]', 188],
    ['large', 5, 'h-[329px]', 'w-[216px]', 232],
  ] as const)(
    'renders %s covers with the matching column count and tile size',
    async (coverSize, columns, heightClass, widthClass, step) => {
      const input = spyLibraryInput();
      const rows = [makeRow({ key: 'a' }), makeRow({ key: 'b' }), makeRow({ key: 'c' })];
      setClientSize(1200, 600);

      renderControls(
        <GameGrid />,
        gridLibrary(rows, {
          selected: ['a'],
          layout: { ...makeLibrary().layout, coverSize },
        }),
      );

      await waitFor(() =>
        expect(setInputs(input)).toContainEqual({ setColumns: columns }),
      );

      const tile = screen.getByTestId('game-tile-a');
      expect(tile).toHaveClass(heightClass, widthClass);
      expect(tile).toHaveAttribute('data-selected', 'true');
      expect(tile.parentElement).toHaveStyle({
        transform: 'translate(0px, 0px)',
      });
      expect(screen.getByTestId('game-tile-b').parentElement).toHaveStyle({
        transform: `translate(${step}px, 0px)`,
      });
      expect(screen.getAllByRole('listitem')).toHaveLength(3);
    },
  );

  it('sends SetColumns from the measured width, debounced and only on change', async () => {
    let resizeCallback!: (entries: ResizeObserverEntry[]) => void;
    class MockResizeObserver {
      constructor(callback: (entries: ResizeObserverEntry[]) => void) {
        resizeCallback = callback;
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    const original = window.ResizeObserver;
    window.ResizeObserver = MockResizeObserver as unknown as typeof ResizeObserver;
    const input = spyLibraryInput();

    try {
      renderControls(<GameGrid />, gridLibrary([makeRow({ key: 'a' })]));

      expect(
        setInputs(input).filter((value) => 'setColumns' in (value as object)),
      ).toHaveLength(0);

      act(() => {
        resizeCallback([
          {
            contentRect: { width: 1200 } as DOMRectReadOnly,
            target: { clientWidth: 1200 } as Element,
          } as ResizeObserverEntry,
        ]);
      });

      await waitFor(() =>
        expect(setInputs(input)).toContainEqual({ setColumns: 6 }),
      );
      expect(
        setInputs(input).filter((value) => 'setColumns' in (value as object)),
      ).toHaveLength(1);
    } finally {
      window.ResizeObserver = original;
    }
  });

  it('recomputes the column count on a window resize', async () => {
    const input = spyLibraryInput();
    renderControls(<GameGrid />, gridLibrary([makeRow({ key: 'a' })]));

    setClientSize(420, 600);
    fireEvent(window, new Event('resize'));

    await waitFor(() => expect(setInputs(input)).toContainEqual({ setColumns: 2 }));
  });

  it('shows real badges, the shared session clock and the hover action', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-11-15T12:00:00Z'));
    const input = spyLibraryInput();
    const openEntry = vi.spyOn(commands, 'openEntry').mockResolvedValue(undefined);

    const rows = [
      makeRow({
        key: 'running',
        title: 'Running Game',
        installed: true,
        running: true,
        sessionStartedAt: NOW - 2530,
      }),
      makeRow({
        key: 'badges',
        title: 'Badge Game',
        installed: true,
        shared: true,
        owned: false,
        isFavorite: true,
        hidden: true,
      }),
      makeRow({
        key: 'install',
        title: 'Install Game',
        installed: false,
        owned: true,
      }),
      makeRow({
        key: 'unknown',
        title: 'Unknown Start',
        running: true,
        sessionStartedAt: null,
      }),
    ];

    renderControls(<GameGrid />, gridLibrary(rows));

    const running = screen.getByTestId('game-tile-running');
    expect(within(running).getByText('Running · 0:42:10')).toBeInTheDocument();
    expect(within(running).queryByText('Installed')).not.toBeInTheDocument();
    expect(within(running).queryByRole('button', { name: 'Play' })).not.toBeInTheDocument();

    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(within(running).getByText('Running · 0:42:11')).toBeInTheDocument();

    const badges = screen.getByTestId('game-tile-badges');
    expect(within(badges).getByText('Installed')).toBeInTheDocument();
    expect(within(badges).getByText('Family')).toBeInTheDocument();
    expect(within(badges).getByText('Hidden')).toBeInTheDocument();
    expect(within(badges).getByRole('img', { name: 'Favorite' })).toBeInTheDocument();

    const unknown = screen.getByTestId('game-tile-unknown');
    expect(within(unknown).getByText('Running')).toBeInTheDocument();
    expect(within(unknown).queryByText(/Running ·/)).not.toBeInTheDocument();

    await act(async () => {
      fireEvent.click(within(screen.getByTestId('game-tile-install')).getByRole('button', { name: 'Install' }));
    });
    expect(setInputs(input)).toContainEqual({
      select: { key: 'install', mode: 'replace' },
    });
    expect(openEntry).toHaveBeenCalledWith('install', 'install');

    await act(async () => {
      fireEvent.click(within(screen.getByTestId('game-tile-badges')).queryByRole('button', { name: 'Play' })!);
    });
    expect(openEntry).toHaveBeenCalledWith('badges', 'play');
  });

  it('sends replace, range and toggle selections plus the context menu target', () => {
    const input = spyLibraryInput();
    const rows = [makeRow({ key: 'a' }), makeRow({ key: 'b' })];
    renderControls(<GameGrid />, gridLibrary(rows, { selected: ['a'], focused: 'a' }));

    fireEvent.click(screen.getByTestId('game-tile-a'));
    fireEvent.click(screen.getByTestId('game-tile-b'), { shiftKey: true });
    fireEvent.click(screen.getByTestId('game-tile-b'), { ctrlKey: true });

    const inputs = setInputs(input);
    expect(inputs).toContainEqual({ select: { key: 'a', mode: 'replace' } });
    expect(inputs).toContainEqual({ select: { key: 'b', mode: 'range' } });
    expect(inputs).toContainEqual({ select: { key: 'b', mode: 'toggle' } });

    fireEvent.contextMenu(screen.getByTestId('game-tile-b'));
    expect(setInputs(input)).toContainEqual({ select: { key: 'b', mode: 'replace' } });
    expect(uiStore.state.contextMenu.open).toBe(true);
    expect(uiStore.state.contextMenu.entryKey).toBe('b');
  });

  it('mounts a bounded slice of a 1000-row grid', async () => {
    const rows = Array.from({ length: 1000 }, (_, index) =>
      makeRow({ key: `row-${index}`, title: `Game ${index}` }),
    );
    renderControls(<GameGrid />, gridLibrary(rows, { focused: 'row-0' }));

    await waitFor(() => expect(screen.getAllByRole('listitem').length).toBeGreaterThan(0));
    const mounted = screen.getAllByRole('listitem');
    expect(mounted.length).toBeLessThan(60);

    const sizer = screen.getByTestId('game-grid-sizer');
    expect(Number.parseFloat(sizer.style.height)).toBeGreaterThan(60_000);
  });

  it('reveals an explicitly requested row through the virtualizer', () => {
    const scrollTo = vi.fn();
    Element.prototype.scrollTo = scrollTo as unknown as typeof Element.prototype.scrollTo;
    const rows = Array.from({ length: 1000 }, (_, index) =>
      makeRow({ key: `row-${index}`, title: `Game ${index}` }),
    );

    renderControls(
      <GameGrid scrollToKey="row-500" />,
      gridLibrary(rows, { focused: 'row-0' }),
    );

    expect(scrollTo).toHaveBeenCalled();
  });

  it('keeps the focused tile marked and tabbable', () => {
    const rows = [makeRow({ key: 'a' }), makeRow({ key: 'b' })];
    renderControls(<GameGrid />, gridLibrary(rows, { focused: 'b' }));

    const focused = screen.getByTestId('game-tile-b');
    expect(focused).toHaveAttribute('data-focused', 'true');
    expect(focused).toHaveAttribute('tabindex', '0');
    expect(screen.getByTestId('game-tile-a')).toHaveAttribute('tabindex', '-1');
  });
});
