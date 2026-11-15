import { describe, expect, it, vi, beforeEach } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import {
  createDefaultFilters,
  type LibraryFilters,
  type Row,
} from '../../api';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { useGlobalShortcuts } from '../shell/useGlobalShortcuts';
import { LibraryToolbar } from './LibraryToolbar';
import { makeLibrary, renderControls, setInputs, spyLibraryInput } from './controlsTestUtils';

/** Mounts the single global shortcut layer next to the toolbar under test. */
function ToolbarWithGlobalShortcuts() {
  useGlobalShortcuts();
  return <LibraryToolbar />;
}

function activeFilters(change: (next: LibraryFilters) => void): LibraryFilters {
  const next = createDefaultFilters();
  change(next);
  return next;
}

function toolbarLibrary(overrides: Parameters<typeof makeLibrary>[0] = {}) {
  const base = makeLibrary({
    total: 248,
    rows: new Array<Row>(42),
    providers: [
      {
        id: 'steam',
        name: 'Steam',
        count: 164,
        installedCount: 100,
        ownedCount: 160,
        sharedCount: 0,
        sharedLabel: null,
      },
    ],
  });
  return makeLibrary({
    ...overrides,
    layout: {
      ...base.layout,
      ...(overrides.layout ?? {}),
    },
  });
}

describe('LibraryToolbar', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('removes the old inline banners', () => {
    renderControls(
      <LibraryToolbar />,
      toolbarLibrary({
        scanning: true,
        failedProviders: ['Steam'],
        access: 'offline',
      }),
    );

    expect(screen.queryByTestId('banner-scanning')).not.toBeInTheDocument();
    expect(screen.queryByTestId('banner-failed-providers')).not.toBeInTheDocument();
    expect(screen.queryByTestId('banner-offline')).not.toBeInTheDocument();
    expect(screen.queryByTestId('banner-unverified')).not.toBeInTheDocument();
    expect(screen.queryByTestId('library-header')).not.toBeInTheDocument();
    expect(screen.queryByTestId('library-active-filters')).not.toBeInTheDocument();
  });

  it('sends search text and clears it', () => {
    const input = spyLibraryInput();
    const library = toolbarLibrary();
    library.search = 'ember';
    renderControls(<LibraryToolbar />, library);

    const search = screen.getByTestId('library-search-input');
    fireEvent.change(search, { target: { value: 'hollow' } });
    expect(setInputs(input)).toContainEqual({ setSearch: 'hollow' });

    fireEvent.click(screen.getByTestId('library-clear-search-btn'));
    expect(setInputs(input)).toContainEqual({ setSearch: '' });
    expect(search).toHaveFocus();
  });

  it('focuses the search with Ctrl+F through the global shortcut layer', () => {
    renderControls(<ToolbarWithGlobalShortcuts />, toolbarLibrary());
    const search = screen.getByTestId('library-search-input');

    fireEvent.keyDown(document.body, { key: 'f', ctrlKey: true });
    expect(search).toHaveFocus();
  });

  it('shows the active-facet count and opens the shared filter popover', async () => {
    const library = toolbarLibrary();
    library.layout.filters = activeFilters((next) => {
      next.stores = ['steam'];
      next.favoritesOnly = true;
    });
    renderControls(<LibraryToolbar />, library);

    expect(screen.getByTestId('library-filters-count')).toHaveTextContent('2');

    fireEvent.click(screen.getByTestId('library-filters-btn'));
    expect(await screen.findByTestId('library-filters-popover')).toBeInTheDocument();
    expect(uiStore.state.filtersOpen).toBe(true);
  });

  it('shows the current sort and group labels', () => {
    const library = toolbarLibrary();
    library.layout.sort = { key: 'lastPlayed', direction: 'descending' };
    renderControls(<LibraryToolbar />, library);

    expect(screen.getByTestId('library-sort-btn')).toHaveTextContent('Recently played');
    expect(screen.getByTestId('library-group-btn')).toHaveTextContent('No grouping');
  });

  it('sends the grid/list choice', () => {
    const input = spyLibraryInput();
    const library = toolbarLibrary();
    library.layout.view = 'grid';
    renderControls(<LibraryToolbar />, library);

    fireEvent.click(screen.getByRole('radio', { name: 'List' }));
    expect(setInputs(input)).toContainEqual({ setView: 'list' });
  });

  it('toggles the details pane and shows its pressed state', () => {
    const input = spyLibraryInput();
    const library = toolbarLibrary();
    library.layout.detailOpen = true;
    renderControls(<LibraryToolbar />, library);

    const toggle = screen.getByTestId('library-toggle-details-btn');
    expect(toggle).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(toggle);
    expect(setInputs(input)).toContainEqual({ setDetailOpen: false });
  });

  it('renders toolbar copy in pt-BR', () => {
    const library = toolbarLibrary();
    library.layout.sort = { key: 'lastPlayed', direction: 'descending' };
    library.layout.filters = activeFilters((next) => {
      next.stores = ['steam'];
    });
    renderControls(<LibraryToolbar />, library, 'pt-BR');

    expect(screen.getByTestId('library-filters-btn')).toHaveTextContent('Filtros');
    expect(screen.getByTestId('library-sort-btn')).toHaveTextContent('Jogado recentemente');
    expect(screen.getByTestId('library-group-btn')).toHaveTextContent('Sem grupos');
    expect(screen.getByLabelText('Painel de detalhes')).toBeInTheDocument();
  });
});
