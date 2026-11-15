import { describe, expect, it, vi, beforeEach } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { commands, type Actions, type ProviderCount } from '../../api';
import { resetUiStore } from '../../store/uiStore';
import { LibraryHeader } from './LibraryHeader';
import { makeLibrary, renderControls } from './controlsTestUtils';

function provider(id: string, name: string): ProviderCount {
  return {
    id,
    name,
    count: 10,
    installedCount: 5,
    ownedCount: 10,
    sharedCount: 0,
    sharedLabel: null,
  };
}

function actions(addToGamenode: Actions['addToGamenode']): Actions {
  return {
    selected: 1,
    play: 'available',
    install: 'available',
    matchGame: 'available',
    addToGamenode,
    changeMatch: false,
  };
}

describe('LibraryHeader', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it.each([
    {
      name: 'scope',
      library: makeLibrary({ titleSource: { kind: 'scope', id: 'all' } }),
      title: 'All games',
    },
    {
      name: 'scope Recent',
      library: makeLibrary({ titleSource: { kind: 'scope', id: 'recent' } }),
      title: 'Recently played',
    },
    {
      name: 'store',
      library: makeLibrary({
        titleSource: { kind: 'store', id: 'steam' },
        providers: [provider('steam', 'Steam')],
      }),
      title: 'Steam',
    },
    {
      name: 'collection',
      library: makeLibrary({
        titleSource: { kind: 'collection', id: 'c1' },
        collections: [{ id: 'c1', name: 'Backlog curto', count: 2 }],
      }),
      title: 'Backlog curto',
    },
    {
      name: 'saved view',
      library: makeLibrary({
        titleSource: { kind: 'savedView', id: '3' },
        savedViews: [{ id: 3, name: 'Steam backlog', count: 5, active: true }],
      }),
      title: 'Steam backlog',
    },
  ])('names the $name title source', ({ library, title }) => {
    renderControls(<LibraryHeader />, library);
    expect(screen.getByTestId('library-header-title')).toHaveTextContent(title);
  });

  it('shows the total, installed and store counts in the normal subtitle', () => {
    renderControls(
      <LibraryHeader />,
      makeLibrary({
        total: 248,
        installedTotal: 37,
        providers: [provider('steam', 'Steam'), provider('epic', 'Epic'), provider('gog', 'GOG')],
      }),
    );
    expect(screen.getByTestId('library-header-subtitle')).toHaveTextContent(
      '248 games · 37 installed on this PC · 3 stores',
    );
  });

  it('shows the grouped subtitle with the group name and selected count', () => {
    renderControls(
      <LibraryHeader />,
      makeLibrary({
        total: 248,
        layout: { ...makeLibrary().layout, group: 'store' },
        selected: ['a', 'b', 'c'],
      }),
    );
    expect(screen.getByTestId('library-header-subtitle')).toHaveTextContent(
      '248 games · grouped by Store · 3 selected',
    );
  });

  it('appends the selected count when no grouping is active', () => {
    renderControls(
      <LibraryHeader />,
      makeLibrary({
        total: 248,
        installedTotal: 37,
        providers: [provider('steam', 'Steam')],
        selected: ['a', 'b'],
      }),
    );
    expect(screen.getByTestId('library-header-subtitle')).toHaveTextContent(
      '248 games · 37 installed on this PC · 1 stores · 2 selected',
    );
  });

  it('shows the spinner while scanning and requests a rescan', () => {
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue(undefined);
    renderControls(<LibraryHeader />, makeLibrary({ scanning: true }));

    const button = screen.getByTestId('library-header-rescan');
    expect(button).toHaveAttribute('data-scanning', 'true');
    expect(button).toHaveAttribute('aria-busy', 'true');
    expect(button.querySelector('.tabler-icon-loader-2')).not.toBeNull();

    fireEvent.click(button);
    expect(rescan).toHaveBeenCalledTimes(1);
  });

  it('opens the add dialog when the selection can be added', () => {
    const input = vi.spyOn(commands, 'libraryInput').mockResolvedValue(undefined);
    renderControls(<LibraryHeader />, makeLibrary({ actions: actions('available') }));

    const button = screen.getByTestId('library-header-add');
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(input).toHaveBeenCalledWith('openAdd');
  });

  it('disables Add to GameNode with an accessible reason when nothing is selected', () => {
    renderControls(<LibraryHeader />, makeLibrary({ actions: actions({ disabled: 'nothingSelected' }) }));

    const button = screen.getByTestId('library-header-add');
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute('aria-describedby', 'library-add-disabled-reason');
    expect(screen.getByText('Select a game first.')).toHaveClass('sr-only');
  });

  it('renders the localized header copy in pt-BR', () => {
    renderControls(
      <LibraryHeader />,
      makeLibrary({
        total: 248,
        installedTotal: 37,
        providers: [provider('steam', 'Steam')],
        titleSource: { kind: 'scope', id: 'recent' },
      }),
      'pt-BR',
    );

    expect(screen.getByTestId('library-header-title')).toHaveTextContent('Jogados recentemente');
    expect(screen.getByTestId('library-header-subtitle')).toHaveTextContent(
      '248 jogos · 37 instalados neste PC · 1 lojas',
    );
    expect(screen.getByTestId('library-header-rescan')).toHaveTextContent('Verificar novamente');
    expect(screen.getByTestId('library-header-add')).toHaveTextContent('Adicionar ao GameNode');
  });
});
