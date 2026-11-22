import { describe, expect, it, vi, beforeEach } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import {
  createDefaultFilters,
  type LibraryFilters,
  type Row,
} from '../../api';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { FilterChipsRow } from './FilterChipsRow';
import { makeLibrary, renderControls, setInputs, spyLibraryInput } from './controlsTestUtils';

function filters(change: (next: LibraryFilters) => void): LibraryFilters {
  const next = createDefaultFilters();
  change(next);
  return next;
}

function libraryWith(next: LibraryFilters) {
  const base = makeLibrary();
  return makeLibrary({
    total: 248,
    rows: new Array<Row>(42),
    layout: { ...base.layout, filters: next },
    filterCounts: {
      ...base.filterCounts,
      stores: [
        { id: 'steam', name: 'Steam', count: 164 },
        { id: 'epic', name: 'Epic Games Store', count: 52 },
      ],
    },
  });
}

describe('FilterChipsRow', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('localizes a known built-in store in the active filter chip', () => {
    const active = filters((next) => {
      next.stores = ['packagekit'];
    });
    const base = makeLibrary();
    const library = makeLibrary({
      providers: [
        {
          id: 'packagekit',
          name: 'Gerenciador de pacotes Linux',
          count: 5,
          installedCount: 5,
          ownedCount: 0,
          sharedCount: 0,
          sharedLabel: null,
        },
      ],
      layout: { ...base.layout, filters: active },
      filterCounts: {
        ...base.filterCounts,
        stores: [
          {
            id: 'packagekit',
            name: 'Gerenciador de pacotes Linux',
            count: 5,
          },
        ],
      },
    });
    renderControls(<FilterChipsRow />, library);

    expect(screen.getByTestId('library-filter-chip-stores')).toHaveTextContent(
      'Store:Linux Package Manager',
    );
  });

  it('renders one joined chip per active facet with copy.md labels', () => {
    const active = filters((next) => {
      next.availability = { installed: true, owned: true, shared: false };
      next.stores = ['steam', 'epic'];
      next.matchStates = ['matched', 'ambiguous'];
      next.gamenodeStatus = ['playing', 'notInLibrary'];
      next.playtime = 'atLeast10h';
      next.played = 'unplayed';
      next.lastPlayed = 'last30Days';
      next.release = { decade: 2020 };
      next.added = 'last7Days';
      next.favoritesOnly = true;
      next.runningOnly = true;
      next.showHidden = true;
    });
    renderControls(<FilterChipsRow />, libraryWith(active));

    expect(screen.getByTestId('library-filter-chip-availability')).toHaveTextContent(
      'Availability:Installed on this PC, In library',
    );
    expect(screen.getByTestId('library-filter-chip-stores')).toHaveTextContent(
      'Store:Steam, Epic Games Store',
    );
    expect(screen.getByTestId('library-filter-chip-match')).toHaveTextContent(
      'Match:Matched, Needs review',
    );
    expect(screen.getByTestId('library-filter-chip-gamenodeStatus')).toHaveTextContent(
      'GameNode status:Playing, Not on GameNode',
    );
    expect(screen.getByTestId('library-filter-chip-playtime')).toHaveTextContent(
      'Playtime:10 h+',
    );
    expect(screen.getByTestId('library-filter-chip-played')).toHaveTextContent(
      'History:Never played',
    );
    expect(screen.getByTestId('library-filter-chip-lastPlayed')).toHaveTextContent(
      'Last session:30 days',
    );
    expect(screen.getByTestId('library-filter-chip-release')).toHaveTextContent('Release:2020s');
    expect(screen.getByTestId('library-filter-chip-added')).toHaveTextContent('Added:This week');
    expect(screen.getByTestId('library-filter-chip-favorites')).toHaveTextContent('Favorites only');
    expect(screen.getByTestId('library-filter-chip-running')).toHaveTextContent('Running only');
    expect(screen.getByTestId('library-filter-chip-showHidden')).toHaveTextContent('Show hidden');
  });

  it('removes only the selected facet', () => {
    const input = spyLibraryInput();
    const active = filters((next) => {
      next.stores = ['steam', 'epic'];
      next.favoritesOnly = true;
    });
    renderControls(<FilterChipsRow />, libraryWith(active));

    const chip = screen.getByTestId('library-filter-chip-stores');
    fireEvent.click(within(chip).getByRole('button'));

    expect(setInputs(input)).toEqual([
      {
        setFilters: expect.objectContaining({
          stores: [],
          favoritesOnly: true,
        }),
      },
    ]);
  });

  it('clears all availability facts with one chip removal', () => {
    const input = spyLibraryInput();
    const active = filters((next) => {
      next.availability = { installed: true, owned: false, shared: true };
    });
    renderControls(<FilterChipsRow />, libraryWith(active));

    const chip = screen.getByTestId('library-filter-chip-availability');
    fireEvent.click(within(chip).getByRole('button'));

    expect(setInputs(input)).toEqual([
      {
        setFilters: expect.objectContaining({
          availability: { installed: false, owned: false, shared: false },
        }),
      },
    ]);
  });

  it('Clear filters resets every facet without touching the search text', () => {
    const input = spyLibraryInput();
    const active = filters((next) => {
      next.stores = ['steam'];
    });
    renderControls(<FilterChipsRow />, libraryWith(active));

    fireEvent.click(screen.getByTestId('library-clear-filters-btn'));

    const inputs = setInputs(input);
    expect(inputs).toHaveLength(1);
    expect(inputs[0]).toEqual({ setFilters: createDefaultFilters() });
    expect(inputs[0]).not.toHaveProperty('setSearch');
  });

  it('offers Showing X of Y and Save as view', () => {
    const active = filters((next) => {
      next.favoritesOnly = true;
    });
    renderControls(<FilterChipsRow />, libraryWith(active));

    expect(screen.getByTestId('library-filtered-count')).toHaveTextContent('Showing 42 of 248');

    fireEvent.click(screen.getByTestId('library-save-view-btn'));
    expect(uiStore.state.saveViewOpen).toBe(true);
  });

  it('renders nothing when no facet is active', () => {
    renderControls(<FilterChipsRow />, libraryWith(createDefaultFilters()));
    expect(screen.queryByTestId('library-active-filters')).not.toBeInTheDocument();
  });

  it('renders the pt-BR chip and footer copy', () => {
    const active = filters((next) => {
      next.stores = ['steam'];
      next.favoritesOnly = true;
      next.showHidden = true;
    });
    renderControls(<FilterChipsRow />, libraryWith(active), 'pt-BR');

    expect(screen.getByTestId('library-filter-chip-stores')).toHaveTextContent('Loja:Steam');
    expect(screen.getByTestId('library-filter-chip-favorites')).toHaveTextContent(
      'Somente favoritos',
    );
    expect(screen.getByTestId('library-filter-chip-showHidden')).toHaveTextContent(
      'Mostrar ocultos',
    );
    expect(screen.getByTestId('library-filtered-count')).toHaveTextContent('Mostrando 42 de 248');
    expect(screen.getByTestId('library-clear-filters-btn')).toHaveTextContent('Limpar filtros');
    expect(screen.getByTestId('library-save-view-btn')).toHaveTextContent('Salvar como visão');
  });
});
