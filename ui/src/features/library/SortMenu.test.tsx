import { describe, expect, it, vi, beforeEach } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { resetUiStore } from '../../store/uiStore';
import type { SortDirection, SortKey } from '../../api';
import { SortMenu } from './SortMenu';
import { makeLibrary, renderControls, setInputs, spyLibraryInput } from './controlsTestUtils';

function sortLibrary(key: SortKey, direction: SortDirection) {
  const base = makeLibrary();
  return makeLibrary({
    layout: { ...base.layout, sort: { key, direction } },
  });
}

async function openSortMenu() {
  fireEvent.click(screen.getByTestId('library-sort-btn'));
  return screen.findByTestId('library-sort-menu');
}

describe('SortMenu', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('shows the checked key and both Title directions', async () => {
    renderControls(<SortMenu />, sortLibrary('lastPlayed', 'descending'));
    await openSortMenu();

    expect(screen.getByTestId('library-sort-btn')).toHaveTextContent('Recently played');
    expect(await screen.findByRole('menuitemcheckbox', { name: 'Recently played' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    expect(await screen.findByRole('menuitemcheckbox', { name: 'Title (A–Z)' })).toHaveAttribute(
      'aria-checked',
      'false',
    );
    expect(await screen.findByRole('menuitemcheckbox', { name: 'Title (Z–A)' })).toHaveAttribute(
      'aria-checked',
      'false',
    );
  });

  it('checks the matching Title direction and shows its label', async () => {
    renderControls(<SortMenu />, sortLibrary('title', 'ascending'));
    await openSortMenu();

    expect(await screen.findByRole('menuitemcheckbox', { name: 'Title (A–Z)' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    expect(await screen.findByRole('menuitemcheckbox', { name: 'Title (Z–A)' })).toHaveAttribute(
      'aria-checked',
      'false',
    );
  });

  it.each([
    ['Title (A–Z)', 'title', 'ascending'],
    ['Title (Z–A)', 'title', 'descending'],
    ['Recently played', 'lastPlayed', 'descending'],
    ['Playtime', 'playtime', 'descending'],
    ['Recently added', 'dateAdded', 'descending'],
    ['Release year', 'releaseYear', 'descending'],
    ['Match status', 'matchStatus', 'ascending'],
    ['Store', 'provider', 'ascending'],
    ['GameNode status', 'gameNodeStatus', 'ascending'],
  ] as const)('choosing %s sends its key and natural direction', async (label, key, direction) => {
    const input = spyLibraryInput();
    renderControls(<SortMenu />, sortLibrary('title', 'ascending'));
    await openSortMenu();

    fireEvent.click(await screen.findByRole('menuitemcheckbox', { name: label }));

    expect(setInputs(input)).toContainEqual({ setSort: { key, direction } });
  });

  it('flips the direction and keeps the menu open', async () => {
    const input = spyLibraryInput();
    renderControls(<SortMenu />, sortLibrary('lastPlayed', 'descending'));
    await openSortMenu();

    fireEvent.click(screen.getByTestId('sort-direction-toggle'));

    expect(setInputs(input)).toContainEqual({
      setSort: { key: 'lastPlayed', direction: 'ascending' },
    });
    expect(screen.getByTestId('library-sort-menu')).toBeInTheDocument();
  });

  it('renders pt-BR sort labels and the Shift S hint', async () => {
    renderControls(<SortMenu />, sortLibrary('lastPlayed', 'descending'), 'pt-BR');
    await openSortMenu();

    expect(screen.getByText('Ordenar por')).toBeInTheDocument();
    expect(await screen.findByRole('menuitemcheckbox', { name: 'Jogado recentemente' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    expect(screen.getByTestId('sort-direction-toggle')).toHaveTextContent('Ordem decrescente');
    expect(screen.getByTestId('sort-direction-toggle')).toHaveTextContent('Shift');
    expect(screen.getByTestId('sort-direction-toggle')).toHaveTextContent('S');
  });
});
