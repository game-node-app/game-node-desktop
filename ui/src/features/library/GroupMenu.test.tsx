import { describe, expect, it, vi, beforeEach } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import type { GroupBy, RowGroup } from '../../api';
import { resetUiStore } from '../../store/uiStore';
import { GroupMenu } from './GroupMenu';
import { makeLibrary, renderControls, setInputs, spyLibraryInput } from './controlsTestUtils';

function groupLibrary(group: GroupBy, groups: RowGroup[] = []) {
  const base = makeLibrary();
  return makeLibrary({
    layout: { ...base.layout, group },
    groups,
  });
}

function rowGroup(id: string, collapsed: boolean): RowGroup {
  return {
    id,
    labelKey: id,
    count: 3,
    collapsed,
    firstIndex: 0,
  };
}

async function openGroupMenu() {
  fireEvent.click(screen.getByTestId('library-group-btn'));
  return screen.findByTestId('library-group-menu');
}

describe('GroupMenu', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('shows every grouping with its checked state', async () => {
    renderControls(<GroupMenu />, groupLibrary('store'));
    await openGroupMenu();

    expect(screen.getByTestId('library-group-btn')).toHaveTextContent('Group: Store');
    expect(await screen.findByRole('menuitemcheckbox', { name: 'Store' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    for (const label of [
      'No grouping',
      'GameNode status',
      'Match',
      'Installation',
      'Release decade',
    ]) {
      expect(await screen.findByRole('menuitemcheckbox', { name: label })).toHaveAttribute(
        'aria-checked',
        'false',
      );
    }
  });

  it('sends the chosen grouping', async () => {
    const input = spyLibraryInput();
    renderControls(<GroupMenu />, groupLibrary('none'));
    await openGroupMenu();

    fireEvent.click(await screen.findByRole('menuitemcheckbox', { name: 'Installation' }));

    expect(setInputs(input)).toContainEqual({ setGroup: 'installation' });
  });

  it('collapses all groups and expands them again', async () => {
    const input = spyLibraryInput();
    const groups = [rowGroup('steam', false), rowGroup('epic', false)];
    renderControls(<GroupMenu />, groupLibrary('store', groups));
    await openGroupMenu();

    const collapse = screen.getByTestId('group-collapse-all');
    expect(collapse).toHaveTextContent('Collapse all groups');
    fireEvent.click(collapse);
    expect(setInputs(input)).toContainEqual({ setAllGroupsCollapsed: true });
  });

  it('offers Expand all when every group is collapsed', async () => {
    const input = spyLibraryInput();
    const groups = [rowGroup('steam', true), rowGroup('epic', true)];
    renderControls(<GroupMenu />, groupLibrary('store', groups));
    await openGroupMenu();

    const collapse = screen.getByTestId('group-collapse-all');
    expect(collapse).toHaveTextContent('Expand all groups');
    fireEvent.click(collapse);
    expect(setInputs(input)).toContainEqual({ setAllGroupsCollapsed: false });
  });

  it('disables Collapse all with a reason while no grouping is active', async () => {
    renderControls(<GroupMenu />, groupLibrary('none'));
    await openGroupMenu();

    const collapse = screen.getByTestId('group-collapse-all');
    expect(collapse).toBeDisabled();
    expect(collapse).toHaveAttribute('title', 'No grouping');
    expect(collapse).toHaveTextContent('No grouping');
  });

  it('renders pt-BR grouping labels', async () => {
    renderControls(<GroupMenu />, groupLibrary('store', [rowGroup('steam', false)]), 'pt-BR');
    await openGroupMenu();

    expect(screen.getByText('Agrupar por')).toBeInTheDocument();
    expect(screen.getByTestId('library-group-btn')).toHaveTextContent('Agrupar: Loja');
    expect(await screen.findByRole('menuitemcheckbox', { name: 'Sem grupos' })).toBeInTheDocument();
    expect(screen.getByTestId('group-collapse-all')).toHaveTextContent('Recolher todos os grupos');
  });
});
