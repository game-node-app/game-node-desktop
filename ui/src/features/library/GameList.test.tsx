import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { type LibraryView, type Row, type RowGroup, type Sort } from '../../api';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { GAME_ROW_MIN_WIDTH, GAME_ROW_TITLE_MIN_WIDTH } from '../../components/ui/GameRow';
import { GAME_LIST_HEADER_HEIGHT, GameList } from './GameList';
import { buildListItems } from './libraryGroups';
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

function listLibrary(rows: Row[], overrides: Partial<LibraryView> = {}): LibraryView {
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

function withSort(sort: Sort): Partial<LibraryView> {
  return { layout: { ...makeLibrary().layout, sort } };
}

function columnHeader(testId: string): HTMLElement {
  const header = screen.getByTestId(testId).closest('[role="columnheader"]');
  if (!(header instanceof HTMLElement)) throw new Error(`No column header for ${testId}`);
  return header;
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

describe('GameList virtual items', () => {
  it('keeps engine group order and never mixes groups', () => {
    const rows = [
      makeRow({ key: 's1' }),
      makeRow({ key: 's2' }),
      makeRow({ key: 's3' }),
      makeRow({ key: 'e1', provider: 'epic' }),
      makeRow({ key: 'e2', provider: 'epic' }),
    ];
    const groups = [
      rowGroup('steam', 'Steam', 3, 0),
      rowGroup('epic', 'Epic Games Store', 2, 3),
    ];

    const items = buildListItems(rows, groups);
    expect(
      items.map((item) =>
        item.kind === 'group' ? `group:${item.group.id}` : item.row.key,
      ),
    ).toEqual(['group:steam', 's1', 's2', 's3', 'group:epic', 'e1', 'e2']);
  });

  it('keeps collapsed headers and omits their rows', () => {
    const rows = [makeRow({ key: 'e1', provider: 'epic' })];
    const groups = [
      rowGroup('steam', 'Steam', 3, 0, true),
      rowGroup('epic', 'Epic Games Store', 1, 0),
    ];

    const items = buildListItems(rows, groups);
    expect(
      items.map((item) =>
        item.kind === 'group' ? `group:${item.group.id}` : item.row.key,
      ),
    ).toEqual(['group:steam', 'group:epic', 'e1']);
  });
});

describe('GameList', () => {
  it('renders the sticky 34 px header and the reference columns', () => {
    renderControls(<GameList />, listLibrary([makeRow({ key: 'a' })]));

    const header = screen.getByTestId('game-list-header');
    expect(header).toHaveClass('h-[34px]', 'sticky', 'gap-[12px]', 'px-[12px]');
    expect(screen.getByTestId('game-list-sort-title')).toHaveTextContent('Title');
    expect(screen.getByTestId('game-list-sort-provider')).toHaveTextContent('Store');
    expect(screen.getByTestId('game-list-sort-gameNodeStatus')).toHaveTextContent(
      'On GameNode',
    );
    expect(screen.getByTestId('game-list-sort-matchStatus')).toHaveTextContent('Match');
    expect(screen.getByTestId('game-list-sort-playtime')).toHaveTextContent('Time');
    expect(screen.getByTestId('game-list-sort-lastPlayed')).toHaveTextContent(
      'Last session',
    );
    expect(columnHeader('game-list-sort-provider')).toHaveClass('w-[130px]');
    expect(columnHeader('game-list-sort-gameNodeStatus')).toHaveClass('w-[120px]');
    expect(columnHeader('game-list-sort-matchStatus')).toHaveClass('w-[130px]');
    expect(columnHeader('game-list-sort-playtime')).toHaveClass('w-[90px]');
    expect(columnHeader('game-list-sort-lastPlayed')).toHaveClass('w-[110px]');
    expect(screen.getByRole('checkbox', { name: 'Select all' })).toBeInTheDocument();
    expect(screen.getByTestId('game-row-a')).toBeInTheDocument();
  });

  it('shares one horizontal scroll surface between the header and the rows', () => {
    renderControls(<GameList />, listLibrary([makeRow({ key: 'a' })]));

    const container = screen.getByTestId('game-list-container');
    const scrollArea = screen.getByTestId('game-list-scroll-area');
    const surface = screen.getByTestId('game-list-surface');
    const header = screen.getByTestId('game-list-header');
    const sizer = screen.getByTestId('game-list-sizer');
    const row = screen.getByTestId('game-row-a');

    expect(GAME_LIST_HEADER_HEIGHT).toBe(34);
    expect(container).toHaveClass('overflow-hidden');
    expect(scrollArea).toHaveClass('overflow-auto');
    expect(scrollArea).toContainElement(surface);
    expect(surface).toHaveClass('w-full', `min-w-[${GAME_ROW_MIN_WIDTH}px]`);
    expect(surface).toContainElement(header);
    expect(surface).toContainElement(sizer);
    expect(sizer).toContainElement(row);

    expect(header).toHaveClass('sticky', 'top-0', 'h-[34px]', 'w-full');
    expect(sizer).toHaveClass('relative', 'w-full');

    // The title floor used by the surface arithmetic is the header's basis.
    expect(GAME_ROW_TITLE_MIN_WIDTH).toBe(200);
    expect(columnHeader('game-list-sort-title')).toHaveClass('basis-[200px]');

    // The trailing header and the row favorite column live on the same
    // surface, so the single scrollport reaches them together.
    expect(surface).toContainElement(columnHeader('game-list-sort-lastPlayed'));
    const favorite = within(row).getByRole('button', { name: 'Add to favorites' });
    expect(sizer).toContainElement(favorite);

    // Nothing between the rows and the scrollport clips horizontally.
    for (let node = favorite.parentElement; node && node !== scrollArea; node = node.parentElement) {
      expect(node).not.toHaveClass('overflow-hidden');
    }
  });

  it('offsets virtual items below the sticky header', () => {
    renderControls(
      <GameList />,
      listLibrary([makeRow({ key: 'a' }), makeRow({ key: 'b', title: 'B' })]),
    );

    expect(screen.getByTestId('game-row-a').parentElement?.style.transform).toBe(
      'translateY(0px)',
    );
    expect(screen.getByTestId('game-row-b').parentElement?.style.transform).toBe(
      'translateY(52px)',
    );
  });

  it('keeps the shared surface with grouped rows and pt-BR copy', () => {
    const rows = [
      makeRow({ key: 's1', title: 'Steam Um' }),
      makeRow({ key: 'e1', title: 'Epic Um', provider: 'epic' }),
    ];
    const groups = [
      rowGroup('steam', 'Steam', 1, 0),
      rowGroup('epic', 'Epic Games Store', 1, 1),
    ];

    renderControls(
      <GameList />,
      listLibrary(rows, {
        groups,
        layout: { ...makeLibrary().layout, group: 'store' },
      }),
      'pt-BR',
    );

    expect(screen.getByTestId('game-list-sort-title')).toHaveTextContent('Título');
    expect(screen.getByTestId('game-list-sort-lastPlayed')).toHaveTextContent(
      'Última sessão',
    );
    expect(screen.getByTestId('game-list-surface')).toHaveClass(
      `min-w-[${GAME_ROW_MIN_WIDTH}px]`,
    );
    expect(screen.getByTestId('game-list-header')).toHaveClass('sticky', 'top-0');
    expect(
      within(screen.getByTestId('game-row-s1')).getByRole('button', {
        name: 'Adicionar aos favoritos',
      }),
    ).toBeInTheDocument();
  });

  it('renders 36 px group headers with their counts and toggles them', async () => {
    const input = spyLibraryInput();
    const rows = [
      makeRow({ key: 's1', title: 'Steam One' }),
      makeRow({ key: 's2', title: 'Steam Two' }),
      makeRow({ key: 'e1', title: 'Epic One', provider: 'epic' }),
    ];
    const groups = [
      rowGroup('steam', 'Steam', 2, 0),
      rowGroup('epic', 'Epic Games Store', 1, 2),
    ];

    renderControls(
      <GameList />,
      listLibrary(rows, {
        groups,
        layout: { ...makeLibrary().layout, group: 'store' },
      }),
    );

    const steam = await screen.findByTestId('group-header-steam');
    const epic = screen.getByTestId('group-header-epic');
    expect(steam).toHaveAttribute('data-group-header', 'steam');
    expect(within(steam).getByRole('button')).toHaveClass('h-[36px]');
    expect(within(steam).getByText('2 games')).toBeInTheDocument();
    expect(within(epic).getByText('1 game')).toBeInTheDocument();
    expect(
      steam.compareDocumentPosition(epic) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    fireEvent.click(within(steam).getByRole('button'));
    expect(setInputs(input)).toContainEqual({ toggleGroup: { id: 'steam' } });
  });

  it('hides a collapsed group rows while keeping its header', async () => {
    const rows = [makeRow({ key: 'e1', provider: 'epic' })];
    const groups = [
      rowGroup('steam', 'Steam', 3, 0, true),
      rowGroup('epic', 'Epic Games Store', 1, 0),
    ];

    renderControls(
      <GameList />,
      listLibrary(rows, { groups, layout: { ...makeLibrary().layout, group: 'store' } }),
    );

    const steam = await screen.findByTestId('group-header-steam');
    expect(within(steam).getByRole('button')).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByTestId('game-row-s1')).not.toBeInTheDocument();
    expect(screen.getByTestId('game-row-e1')).toBeInTheDocument();
  });

  it('sorts a new column by its natural direction', () => {
    const input = spyLibraryInput();
    renderControls(<GameList />, listLibrary([makeRow({ key: 'a' })], withSort({
      key: 'title',
      direction: 'ascending',
    })));

    fireEvent.click(screen.getByTestId('game-list-sort-playtime'));
    expect(setInputs(input)).toContainEqual({
      setSort: { key: 'playtime', direction: 'descending' },
    });

    fireEvent.click(screen.getByTestId('game-list-sort-provider'));
    expect(setInputs(input)).toContainEqual({
      setSort: { key: 'provider', direction: 'ascending' },
    });
  });

  it('flips the direction when the sorted header is clicked again', () => {
    const input = spyLibraryInput();
    renderControls(<GameList />, listLibrary([makeRow({ key: 'a' })], withSort({
      key: 'playtime',
      direction: 'descending',
    })));

    const time = columnHeader('game-list-sort-playtime');
    expect(time).toHaveAttribute('aria-sort', 'descending');
    expect(time).toHaveAttribute('data-sort', 'descending');
    expect(time.querySelector('svg')).not.toBeNull();
    expect(screen.getByTestId('game-list-sort-playtime')).toHaveClass('text-text-primary');

    fireEvent.click(screen.getByTestId('game-list-sort-playtime'));
    expect(setInputs(input)).toContainEqual({
      setSort: { key: 'playtime', direction: 'ascending' },
    });
  });

  it('selects every visible row from the header when none are selected', () => {
    const input = spyLibraryInput();
    renderControls(<GameList />, listLibrary([makeRow({ key: 'a' }), makeRow({ key: 'b' })]));

    const all = screen.getByRole('checkbox', { name: 'Select all' });
    expect(all).not.toBeChecked();
    fireEvent.click(all);
    expect(setInputs(input)).toContainEqual('selectAll');
  });

  it('shows the header checkbox as indeterminate for a partial selection', () => {
    const input = spyLibraryInput();
    renderControls(
      <GameList />,
      listLibrary([makeRow({ key: 'a' }), makeRow({ key: 'b' })], { selected: ['a'] }),
    );

    const all = screen.getByRole('checkbox', { name: 'Select all' });
    expect(all).toHaveAttribute('aria-checked', 'mixed');
    fireEvent.click(all);
    expect(setInputs(input)).toContainEqual('selectAll');
  });

  it('clears the selection from the header when every visible row is selected', () => {
    const input = spyLibraryInput();
    renderControls(
      <GameList />,
      listLibrary([makeRow({ key: 'a' }), makeRow({ key: 'b' })], {
        selected: ['a', 'b'],
      }),
    );

    const all = screen.getByRole('checkbox', { name: 'Select all' });
    expect(all).toBeChecked();
    fireEvent.click(all);
    expect(setInputs(input)).toContainEqual('clearSelection');
  });

  it('toggles the favorite column without selecting the row', () => {
    const input = spyLibraryInput();
    renderControls(
      <GameList />,
      listLibrary([
        makeRow({ key: 'a', title: 'A' }),
        makeRow({ key: 'b', title: 'B', isFavorite: true }),
      ]),
    );

    fireEvent.click(
      within(screen.getByTestId('game-row-a')).getByRole('button', {
        name: 'Add to favorites',
      }),
    );
    fireEvent.click(
      within(screen.getByTestId('game-row-b')).getByRole('button', {
        name: 'Remove from favorites',
      }),
    );

    const inputs = setInputs(input);
    expect(inputs).toContainEqual({
      setFavorite: { key: 'a', favorite: true, now: expect.any(Number) },
    });
    expect(inputs).toContainEqual({
      setFavorite: { key: 'b', favorite: false, now: expect.any(Number) },
    });
    expect(
      inputs.filter((value) => typeof value === 'object' && 'select' in value),
    ).toHaveLength(0);
  });

  it('sends row replace, range, toggle, checkbox toggle and context targets', () => {
    const input = spyLibraryInput();
    renderControls(
      <GameList />,
      listLibrary([makeRow({ key: 'a' }), makeRow({ key: 'b', title: 'B' })], {
        selected: ['a'],
        focused: 'a',
      }),
    );

    fireEvent.click(screen.getByTestId('game-row-a'));
    fireEvent.click(screen.getByTestId('game-row-b'), { shiftKey: true });
    fireEvent.click(screen.getByTestId('game-row-b'), { ctrlKey: true });
    fireEvent.click(within(screen.getByTestId('game-row-b')).getByRole('checkbox'));

    const inputs = setInputs(input);
    expect(inputs).toContainEqual({ select: { key: 'a', mode: 'replace' } });
    expect(inputs).toContainEqual({ select: { key: 'b', mode: 'range' } });
    expect(inputs).toContainEqual({ select: { key: 'b', mode: 'toggle' } });
    expect(
      inputs.filter(
        (value) =>
          typeof value === 'object' &&
          'select' in value &&
          value.select.key === 'b' &&
          value.select.mode === 'replace',
      ),
    ).toHaveLength(0);

    fireEvent.contextMenu(screen.getByTestId('game-row-b'));
    expect(setInputs(input)).toContainEqual({ select: { key: 'b', mode: 'replace' } });
    expect(uiStore.state.contextMenu.entryKey).toBe('b');
  });

  it('shows the live session time and leaves an unknown start without a clock', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-11-15T12:00:00Z'));

    renderControls(
      <GameList />,
      listLibrary([
        makeRow({
          key: 'running',
          title: 'Running Game',
          running: true,
          sessionStartedAt: NOW - 2530,
        }),
        makeRow({
          key: 'unknown',
          title: 'Unknown Start',
          running: true,
          sessionStartedAt: null,
        }),
      ]),
    );

    const running = screen.getByTestId('game-row-running');
    expect(within(running).getByText('Running · 0:42:10')).toBeInTheDocument();

    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(within(running).getByText('Running · 0:42:11')).toBeInTheDocument();

    const unknown = screen.getByTestId('game-row-unknown');
    expect(within(unknown).getByText('Running')).toBeInTheDocument();
    expect(within(unknown).queryByText(/Running ·/)).not.toBeInTheDocument();
  });

  it('mounts a bounded slice of a 1000-row list', async () => {
    const rows = Array.from({ length: 1000 }, (_, index) =>
      makeRow({ key: `row-${index}`, title: `Game ${index}` }),
    );
    renderControls(<GameList />, listLibrary(rows, { focused: 'row-0' }));

    await waitFor(() => expect(screen.getAllByRole('listitem').length).toBeGreaterThan(0));
    expect(screen.getAllByRole('listitem').length).toBeLessThan(50);

    const sizer = screen.getByTestId('game-list-sizer');
    expect(Number.parseFloat(sizer.style.height)).toBe(52_000);
  });

  it('reveals an explicitly requested row through the virtualizer', () => {
    const scrollTo = vi.fn();
    Element.prototype.scrollTo = scrollTo as unknown as typeof Element.prototype.scrollTo;
    const rows = Array.from({ length: 1000 }, (_, index) =>
      makeRow({ key: `row-${index}`, title: `Game ${index}` }),
    );

    renderControls(
      <GameList scrollToKey="row-500" />,
      listLibrary(rows, { focused: 'row-0' }),
    );

    expect(scrollTo).toHaveBeenCalled();
  });

  it('marks the focused row tabbable and keeps the row height fixed', () => {
    renderControls(
      <GameList />,
      listLibrary([makeRow({ key: 'a' }), makeRow({ key: 'b', title: 'B' })], {
        focused: 'b',
      }),
    );

    const focused = screen.getByTestId('game-row-b');
    expect(focused).toHaveAttribute('data-focused', 'true');
    expect(focused).toHaveAttribute('tabindex', '0');
    expect(focused).toHaveClass('h-[52px]');
    expect(screen.getByTestId('game-row-a')).toHaveAttribute('tabindex', '-1');
  });

  it('shows the GameNode status pill only when the account library was read', () => {
    const base = makeLibrary();
    renderControls(
      <GameList />,
      makeLibrary({
        ...listLibrary([makeRow({ key: 'a' })]),
        filterCounts: {
          ...base.filterCounts,
          gamenodeStatus: { ...base.filterCounts.gamenodeStatus, available: true },
        },
      }),
    );

    expect(
      within(screen.getByTestId('game-row-a')).getByText('Playing'),
    ).toBeInTheDocument();
  });
});
