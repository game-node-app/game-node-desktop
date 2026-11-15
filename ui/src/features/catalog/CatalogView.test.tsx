import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import {
  commands,
  coverAssetUrl,
  artworkAssetUrl,
  createDefaultFilters,
  type CatalogResult,
  type LibraryView,
  type PaletteEntry,
} from '../../api';
import { resetUiStore, setActiveDestination, uiStore } from '../../store/uiStore';
import { CatalogView } from './CatalogView';
import { makeCatalog, makeCatalogLibrary, renderCatalog } from './catalogTestUtils';
import {
  catalogColumnsForWidth,
  catalogDisplayStatus,
  catalogFocusTarget,
} from './catalogPresentation';

function result(overrides: Partial<CatalogResult> = {}): CatalogResult {
  return {
    gameId: 1,
    title: 'Aster Vale',
    year: 2018,
    coverUrl: '//images.igdb.com/igdb/image/upload/t_thumb/aster.jpg',
    coverFile: '1.jpg',
    inLibrary: false,
    localKey: null,
    ...overrides,
  };
}

function spyInput() {
  return vi.spyOn(commands, 'libraryInput').mockResolvedValue(undefined);
}

function inputs(spy: ReturnType<typeof spyInput>) {
  return spy.mock.calls.map(([input]) => input);
}

function paletteEntry(key: string, title = 'Aster Vale'): PaletteEntry {
  return {
    key,
    title,
    providerName: 'Steam',
    installed: true,
    playtimeLabel: null,
  };
}

function library(
  catalog: ReturnType<typeof makeCatalog>,
  overrides: Partial<LibraryView> = {},
): LibraryView {
  return makeCatalogLibrary({ catalog, ...overrides });
}

beforeEach(() => {
  resetUiStore();
  setActiveDestination('catalog');
  vi.restoreAllMocks();
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get: () => 800,
  });
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get: () => 600,
  });
  if (!Element.prototype.scrollTo) {
    Element.prototype.scrollTo = (() => undefined) as typeof Element.prototype.scrollTo;
  }
});

afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = '';
});

describe('catalog presentation helpers', () => {
  it('turns the real width into the reference column count', () => {
    expect(catalogColumnsForWidth(800)).toBe(4);
    expect(catalogColumnsForWidth(1200)).toBe(6);
    expect(catalogColumnsForWidth(420)).toBe(2);
    expect(catalogColumnsForWidth(180)).toBe(1);
  });

  it('hides an answer for another query until the draft is answered', () => {
    expect(catalogDisplayStatus(null, '')).toBe('idle');
    expect(catalogDisplayStatus(makeCatalog({ status: 'loading' }), 'ce')).toBe(
      'loading',
    );
    expect(
      catalogDisplayStatus(
        makeCatalog({ query: 'cel', status: 'ready' }),
        'celeste',
      ),
    ).toBe('loading');
    expect(
      catalogDisplayStatus(
        makeCatalog({ query: 'cel', status: 'ready' }),
        'cel',
      ),
    ).toBe('ready');
    expect(
      catalogDisplayStatus(
        makeCatalog({ query: 'cel', status: 'offline' }),
        'cel',
      ),
    ).toBe('offline');
    expect(catalogDisplayStatus(makeCatalog({ status: 'idle' }), 'ce')).toBe(
      'loading',
    );
  });

  it('moves focus like the library tile grid', () => {
    expect(catalogFocusTarget(0, 'right', 4, 5)).toBe(1);
    expect(catalogFocusTarget(3, 'right', 4, 8)).toBe(3);
    expect(catalogFocusTarget(1, 'down', 4, 5)).toBe(4);
    expect(catalogFocusTarget(4, 'up', 4, 5)).toBe(0);
    expect(catalogFocusTarget(0, 'up', 4, 5)).toBe(0);
    expect(catalogFocusTarget(-1, 'down', 4, 5)).toBe(0);
  });
});

describe('CatalogView', () => {
  it('renders the header, autofocused 480 px search and idle state', () => {
    renderCatalog(<CatalogView />, library(makeCatalog()));

    expect(screen.getByTestId('catalog-view')).toBeInTheDocument();
    expect(
      screen.getByRole('heading', { name: 'Explore GameNode' }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Find games that aren't on this PC and add them to your GameNode library.",
      ),
    ).toBeInTheDocument();

    const search = screen.getByTestId('catalog-search-input');
    expect(search).toHaveFocus();
    expect(search).toHaveAttribute(
      'placeholder',
      'Search the GameNode catalog',
    );
    expect(search).toHaveAttribute('data-library-search');
    expect(
      screen.getByText('Type at least 2 characters'),
    ).toBeInTheDocument();
    expect(screen.queryByTestId('catalog-detail')).not.toBeInTheDocument();
  });

  it('sends no request for a short query and debounces the rest by 300 ms', async () => {
    vi.useFakeTimers();
    const input = spyInput();
    renderCatalog(<CatalogView />, library(makeCatalog()));

    const search = screen.getByTestId('catalog-search-input');
    fireEvent.change(search, { target: { value: 'c' } });
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(inputs(input)).toEqual([]);
    expect(screen.getByText('Type at least 2 characters')).toBeInTheDocument();

    fireEvent.change(search, { target: { value: 'ce' } });
    await act(async () => {
      vi.advanceTimersByTime(299);
    });
    expect(inputs(input)).toEqual([]);
    await act(async () => {
      vi.advanceTimersByTime(1);
    });
    expect(inputs(input)).toEqual([{ catalogSearch: 'ce' }]);

    fireEvent.change(search, { target: { value: 'cel' } });
    await act(async () => {
      vi.advanceTimersByTime(100);
    });
    fireEvent.change(search, { target: { value: 'celeste' } });
    await act(async () => {
      vi.advanceTimersByTime(300);
    });
    expect(inputs(input)).toEqual([
      { catalogSearch: 'ce' },
      { catalogSearch: 'celeste' },
    ]);
  });

  it('does not re-send a query prefilled on mount', async () => {
    vi.useFakeTimers();
    const input = spyInput();
    renderCatalog(
      <CatalogView />,
      library(makeCatalog({ query: 'hades', status: 'loading' })),
    );

    expect(screen.getByTestId('catalog-search-input')).toHaveValue('hades');
    expect(screen.getByTestId('catalog-loading')).toBeInTheDocument();
    expect(screen.getAllByTestId('catalog-skeleton')).toHaveLength(12);

    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(inputs(input)).toEqual([]);
  });

  it('adopts an external prefill without sending it again', async () => {
    const input = spyInput();
    const view = renderCatalog(<CatalogView />, library(makeCatalog()));

    view.update(
      library(makeCatalog({ query: 'celeste', status: 'loading' })),
    );
    const search = screen.getByTestId('catalog-search-input');
    await waitFor(() => expect(search).toHaveValue('celeste'));

    await new Promise((resolve) => setTimeout(resolve, 350));
    expect(inputs(input)).toEqual([]);
  });

  it('keeps local typing when an external query echo arrives mid-debounce', async () => {
    vi.useFakeTimers();
    const input = spyInput();
    const view = renderCatalog(<CatalogView />, library(makeCatalog()));

    const search = screen.getByTestId('catalog-search-input');
    fireEvent.change(search, { target: { value: 'hollow' } });
    view.update(library(makeCatalog({ query: 'hades', status: 'loading' })));

    expect(search).toHaveValue('hollow');
    await act(async () => {
      vi.advanceTimersByTime(300);
    });
    expect(inputs(input)).toEqual([{ catalogSearch: 'hollow' }]);
  });

  it('shows the no-results state and clears the search', async () => {
    const input = spyInput();
    renderCatalog(
      <CatalogView />,
      library(makeCatalog({ query: 'zzz', status: 'ready', results: [] })),
    );

    expect(screen.getByText('No results for “zzz”')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('catalog-empty-clear'));

    const search = screen.getByTestId('catalog-search-input');
    expect(search).toHaveValue('');
    expect(search).toHaveFocus();
    await waitFor(() => expect(inputs(input)).toContainEqual('catalogClear'));
  });

  it('keeps the query on an error and retries it', () => {
    const input = spyInput();
    renderCatalog(
      <CatalogView />,
      library(makeCatalog({ query: 'hades', status: 'error' })),
    );

    expect(screen.getByText("Couldn't search GameNode")).toBeInTheDocument();
    expect(screen.getByTestId('catalog-search-input')).toHaveValue('hades');
    fireEvent.click(screen.getByTestId('catalog-retry'));
    expect(inputs(input)).toEqual([{ catalogSearch: 'hades' }]);
  });

  it('explains offline unverified search and sends nothing until retry', () => {
    const input = spyInput();
    renderCatalog(
      <CatalogView />,
      library(makeCatalog({ query: 'hades', status: 'offline' })),
    );

    expect(
      screen.getByText(
        "GameNode can't be reached. Search needs a connection.",
      ),
    ).toBeInTheDocument();
    expect(inputs(input)).toEqual([]);
    fireEvent.click(screen.getByTestId('catalog-retry'));
    expect(inputs(input)).toEqual([{ catalogSearch: 'hades' }]);
  });

  it('shows library and PC badges, the year, no play overlay and the incomplete caption', () => {
    renderCatalog(
      <CatalogView />,
      library(
        makeCatalog({
          query: 'aster',
          status: 'ready',
          incomplete: true,
          results: [
            result({ gameId: 1, inLibrary: true, localKey: 'steam:1' }),
            result({
              gameId: 2,
              title: 'Brass Meridian',
              year: null,
              coverUrl: null,
              coverFile: null,
            }),
          ],
        }),
      ),
    );

    const first = screen.getByTestId('catalog-tile-1');
    expect(within(first).getByText('In your library')).toBeInTheDocument();
    expect(within(first).getByText('On this PC')).toBeInTheDocument();
    expect(within(first).getByText('2018')).toBeInTheDocument();
    // Catalog results carry no local match or playtime fact: the tile must not
    // render the library match dot/label or an unknown-playtime dash.
    expect(first).not.toHaveTextContent('Matched');
    expect(within(first).queryByText('—')).not.toBeInTheDocument();
    expect(
      within(first).getByTestId('game-tile-catalog:1'),
    ).toHaveAttribute(
      'aria-label',
      'Aster Vale · 2018 · In your library · On this PC',
    );
    expect(
      within(first).queryByRole('button', { name: 'Play' }),
    ).not.toBeInTheDocument();

    const second = screen.getByTestId('catalog-tile-2');
    expect(second).not.toHaveTextContent('Matched');
    expect(within(second).queryByText('—')).not.toBeInTheDocument();
    expect(within(second).queryByText('2018')).not.toBeInTheDocument();
    expect(
      within(second).getByTestId('game-tile-catalog:2'),
    ).toHaveAttribute('aria-label', 'Brass Meridian');

    const cover = within(first).getByRole('img', { name: 'Aster Vale' });
    expect(cover).toHaveAttribute('src', coverAssetUrl('1.jpg'));
    expect(cover).toHaveAttribute('loading', 'lazy');
    expect(cover).toHaveAttribute('decoding', 'async');

    expect(
      screen.getByTestId('catalog-incomplete'),
    ).toHaveTextContent(
      'Results may be incomplete. Refine the search.',
    );
  });

  it('never shows an older answer while a newer query is being typed', async () => {
    vi.useFakeTimers();
    const input = spyInput();
    const view = renderCatalog(
      <CatalogView />,
      library(
        makeCatalog({
          query: 'cel',
          status: 'ready',
          results: [result({ gameId: 1, title: 'Celeste' })],
        }),
      ),
    );

    const search = screen.getByTestId('catalog-search-input');
    fireEvent.change(search, { target: { value: 'celeste' } });

    expect(screen.queryByTestId('catalog-tile-1')).not.toBeInTheDocument();
    expect(screen.getByTestId('catalog-loading')).toBeInTheDocument();

    await act(async () => {
      vi.advanceTimersByTime(300);
    });
    expect(inputs(input)).toContainEqual({ catalogSearch: 'celeste' });

    view.update(
      library(
        makeCatalog({
          query: 'celeste',
          status: 'ready',
          results: [result({ gameId: 2, title: 'Celeste Deluxe' })],
        }),
      ),
    );
    await act(async () => {
      vi.advanceTimersByTime(0);
    });
    expect(screen.getByTestId('catalog-tile-2')).toBeInTheDocument();

    // A late snapshot for the old query must not replace the newer draft or
    // bring its tiles back.
    view.update(
      library(
        makeCatalog({
          query: 'cel',
          status: 'ready',
          results: [result({ gameId: 1, title: 'Celeste' })],
        }),
      ),
    );
    expect(screen.queryByTestId('catalog-tile-1')).not.toBeInTheDocument();
    expect(screen.getByTestId('catalog-search-input')).toHaveValue('celeste');
  });

  it('navigates tiles with the keyboard and clears detail then search with Escape', async () => {
    const input = spyInput();
    renderCatalog(
      <CatalogView />,
      library(
        makeCatalog({
          query: 'cat',
          status: 'ready',
          results: [
            result({ gameId: 1, title: 'One' }),
            result({ gameId: 2, title: 'Two' }),
            result({ gameId: 3, title: 'Three' }),
            result({ gameId: 4, title: 'Four' }),
            result({ gameId: 5, title: 'Five' }),
          ],
        }),
      ),
    );

    const root = screen.getByTestId('catalog-view');
    const search = screen.getByTestId('catalog-search-input');
    await waitFor(() =>
      expect(screen.getByTestId('catalog-tile-1')).toHaveAttribute(
        'data-focused',
        'true',
      ),
    );

    fireEvent.keyDown(root, { key: 'ArrowRight' });
    expect(screen.getByTestId('catalog-tile-2')).toHaveAttribute(
      'data-focused',
      'true',
    );

    fireEvent.keyDown(root, { key: 'ArrowDown' });
    expect(screen.getByTestId('catalog-tile-5')).toHaveAttribute(
      'data-focused',
      'true',
    );

    fireEvent.keyDown(root, { key: 'Enter' });
    expect(screen.getByTestId('catalog-detail-title')).toHaveTextContent('Five');

    fireEvent.keyDown(root, { key: 'F10', shiftKey: true });
    await waitFor(() =>
      expect(screen.getByTestId('catalog-context-menu')).toBeInTheDocument(),
    );
    fireEvent.keyDown(root, { key: 'Escape' });
    await waitFor(() =>
      expect(
        screen.queryByTestId('catalog-context-menu'),
      ).not.toBeInTheDocument(),
    );

    fireEvent.keyDown(root, { key: 'Escape' });
    expect(screen.queryByTestId('catalog-detail')).not.toBeInTheDocument();
    expect(search).toHaveValue('cat');

    fireEvent.keyDown(root, { key: 'Escape' });
    expect(search).toHaveValue('');
    await waitFor(() => expect(inputs(input)).toContainEqual('catalogClear'));

    search.blur();
    fireEvent.keyDown(root, { key: '/' });
    expect(search).toHaveFocus();

    search.blur();
    fireEvent.keyDown(root, { key: 'f', ctrlKey: true });
    expect(search).toHaveFocus();
  });

  it('opens the catalog detail with Add only while the game is absent', async () => {
    const input = spyInput();
    renderCatalog(
      <CatalogView />,
      library(
        makeCatalog({
          query: 'aster',
          status: 'ready',
          results: [result()],
        }),
      ),
    );

    fireEvent.click(screen.getByTestId('game-tile-catalog:1'));
    expect(screen.getByTestId('catalog-detail-title')).toHaveTextContent(
      'Aster Vale',
    );
    expect(screen.getByTestId('catalog-detail-year')).toHaveTextContent('2018');
    expect(screen.queryByTestId('catalog-detail-view')).not.toBeInTheDocument();
    expect(screen.queryByTestId('catalog-detail-show')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('catalog-detail-add'));
    await waitFor(() =>
      expect(inputs(input)).toContainEqual({
        openAddForGame: {
          gameId: 1,
          title: 'Aster Vale',
          coverUrl: '//images.igdb.com/igdb/image/upload/t_thumb/aster.jpg',
        },
      }),
    );
  });

  it('offers View on GameNode once in the library and reports browser errors', async () => {
    const openPage = vi
      .spyOn(commands, 'openGameNodePage')
      .mockRejectedValue(new Error('no browser'));
    renderCatalog(
      <CatalogView />,
      library(
        makeCatalog({
          query: 'aster',
          status: 'ready',
          results: [result({ inLibrary: true })],
        }),
      ),
    );

    fireEvent.click(screen.getByTestId('game-tile-catalog:1'));
    expect(screen.queryByTestId('catalog-detail-add')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('catalog-detail-view'));
    await waitFor(() =>
      expect(openPage).toHaveBeenCalledWith({ game: { id: 1 } }),
    );

    await waitFor(() =>
      expect(
        uiStore.state.toasts.some(
          (toast) =>
            toast.kind === 'danger' &&
            toast.title === 'The link could not be opened.',
        ),
      ).toBe(true),
    );
  });

  it('shows the same actions in the tile context menu', async () => {
    const input = spyInput();
    renderCatalog(
      <CatalogView />,
      library(
        makeCatalog({
          query: 'aster',
          status: 'ready',
          results: [result({ localKey: 'steam:1' })],
        }),
      ),
    );

    fireEvent.contextMenu(screen.getByTestId('game-tile-catalog:1'), {
      clientX: 120,
      clientY: 90,
    });
    await waitFor(() =>
      expect(screen.getByTestId('catalog-context-menu')).toBeInTheDocument(),
    );
    expect(screen.getByTestId('catalog-context-add')).toBeInTheDocument();
    expect(screen.getByTestId('catalog-context-show')).toBeInTheDocument();
    expect(
      screen.queryByTestId('catalog-context-view'),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('catalog-context-add'));
    await waitFor(() =>
      expect(inputs(input)).toContainEqual({
        openAddForGame: {
          gameId: 1,
          title: 'Aster Vale',
          coverUrl: '//images.igdb.com/igdb/image/upload/t_thumb/aster.jpg',
        },
      }),
    );
  });

  it('offers View on GameNode in the context menu once in the library', async () => {
    const openPage = vi
      .spyOn(commands, 'openGameNodePage')
      .mockResolvedValue(undefined);
    renderCatalog(
      <CatalogView />,
      library(
        makeCatalog({
          query: 'aster',
          status: 'ready',
          results: [result({ inLibrary: true })],
        }),
      ),
    );

    fireEvent.contextMenu(screen.getByTestId('game-tile-catalog:1'), {
      clientX: 120,
      clientY: 90,
    });
    await waitFor(() =>
      expect(screen.getByTestId('catalog-context-menu')).toBeInTheDocument(),
    );
    expect(screen.queryByTestId('catalog-context-add')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('catalog-context-view'));
    await waitFor(() =>
      expect(openPage).toHaveBeenCalledWith({ game: { id: 1 } }),
    );
  });

  it('reveals the local row before selecting it and opening its details', async () => {
    const input = spyInput();
    renderCatalog(
      <CatalogView />,
      library(
        makeCatalog({
          query: 'aster',
          status: 'ready',
          results: [result({ inLibrary: true, localKey: 'steam:1' })],
        }),
        {
          search: 'zzz',
          rows: [],
          paletteIndex: [paletteEntry('steam:1')],
          layout: {
            ...makeCatalogLibrary().layout,
            scope: 'favorites',
            collapsedGroups: { store: ['steam'] },
            filters: {
              ...createDefaultFilters(),
              stores: ['steam'],
            },
          },
        },
      ),
    );

    fireEvent.click(screen.getByTestId('game-tile-catalog:1'));
    fireEvent.click(screen.getByTestId('catalog-detail-show'));

    await waitFor(() =>
      expect(uiStore.state.activeDestination).toBe('library'),
    );
    const sent = inputs(input).filter(
      (item) => !(typeof item === 'object' && 'catalogFocus' in item),
    );
    expect(sent).toEqual([
      { setSearch: '' },
      { setScope: 'all' },
      { setAllGroupsCollapsed: false },
      { setFilters: createDefaultFilters() },
      { select: { key: 'steam:1', mode: 'replace' } },
      'openDetails',
    ]);
  });

  it('reveals a hidden local row with Show hidden instead of unhiding it', async () => {
    const input = spyInput();
    renderCatalog(
      <CatalogView />,
      library(
        makeCatalog({
          query: 'hidden',
          status: 'ready',
          results: [result({ inLibrary: true, localKey: 'steam:hidden' })],
        }),
        {
          rows: [],
          paletteIndex: [],
        },
      ),
    );

    fireEvent.click(screen.getByTestId('game-tile-catalog:1'));
    fireEvent.click(screen.getByTestId('catalog-detail-show'));

    await waitFor(() =>
      expect(uiStore.state.activeDestination).toBe('library'),
    );
    const sent = inputs(input);
    expect(sent).toContainEqual({
      setFilters: { ...createDefaultFilters(), showHidden: true },
    });
    expect(sent).toContainEqual({
      select: { key: 'steam:hidden', mode: 'replace' },
    });
    expect(sent).toContainEqual('openDetails');
  });

  it('renders pt-BR copy from the existing catalog keys', () => {
    renderCatalog(<CatalogView />, library(makeCatalog()), 'pt-BR');

    expect(
      screen.getByRole('heading', { name: 'Explorar no GameNode' }),
    ).toBeInTheDocument();
    expect(screen.getByText('Digite pelo menos 2 caracteres')).toBeInTheDocument();
    expect(screen.getByTestId('catalog-search-input')).toHaveAttribute(
      'placeholder',
      'Buscar no catálogo do GameNode',
    );
  });
});


it('loads catalog artwork for the detail selection and releases it on close', async () => {
  const input = spyInput();
  const catalog = makeCatalog({
    query: 'aster', status: 'ready', results: [result()],
  });
  const view = renderCatalog(<CatalogView />, library(catalog));
  fireEvent.click(screen.getByTestId('game-tile-catalog:1'));
  expect(inputs(input)).toContainEqual({ catalogFocus: 1 });
  expect(within(screen.getByTestId('catalog-detail')).getByRole('img'))
    .toHaveAttribute('src', coverAssetUrl('1.jpg'));
  view.update(library({ ...catalog, results: [result({ hasArtwork: true })] }));
  await waitFor(() => {
    const artwork = within(screen.getByTestId('catalog-detail')).getByRole('img');
    expect(artwork).toHaveAttribute('src', artworkAssetUrl(1));
    expect(artwork).toHaveAttribute('loading', 'lazy');
    expect(artwork).toHaveAttribute('decoding', 'async');
  });
  fireEvent.click(screen.getByTestId('catalog-detail-close'));
  expect(inputs(input).at(-1)).toEqual({ catalogFocus: null });
});

it('updates a placeholder when the downloaded catalog cover arrives', async () => {
  const catalog = makeCatalog({
    query: 'aster', status: 'ready', results: [result({ coverFile: null })],
  });
  const view = renderCatalog(<CatalogView />, library(catalog));
  const tile = screen.getByTestId('catalog-tile-1');
  expect(within(tile).queryByRole('img')).not.toBeInTheDocument();
  view.update(library({ ...catalog, results: [result()] }));
  await waitFor(() => {
    expect(within(tile).getByRole('img')).toHaveAttribute('src', coverAssetUrl('1.jpg'));
  });
});
