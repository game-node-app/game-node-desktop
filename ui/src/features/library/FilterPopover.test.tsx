import { describe, expect, it, vi, beforeEach } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {
  createDefaultFilterCounts,
  createDefaultFilters,
  type FilterCounts,
  type LibraryFilters,
  type Row,
} from '../../api';
import { openFilters, openPalette, resetUiStore, uiStore } from '../../store/uiStore';
import { useGlobalShortcuts } from '../shell/useGlobalShortcuts';
import { FilterPopover, FILTER_POPOVER_MIDDLEWARES } from './FilterPopover';
import { makeLibrary, renderControls, setInputs, spyLibraryInput } from './controlsTestUtils';

function filters(change: (next: LibraryFilters) => void = () => undefined): LibraryFilters {
  const next = createDefaultFilters();
  change(next);
  return next;
}

function counts(change: (next: FilterCounts) => void = () => undefined): FilterCounts {
  const next = createDefaultFilterCounts();
  next.availability = { installed: 37, owned: 248, shared: 11 };
  next.stores = [
    { id: 'steam', name: 'Steam', count: 164 },
    { id: 'epic', name: 'Epic Games Store', count: 52 },
    { id: 'manual', name: 'DRM-free games', count: 32 },
  ];
  next.matchStates = { matched: 221, ambiguous: 9, unmatched: 14, noGame: 4 };
  next.gamenodeStatus = {
    playing: 18,
    finished: 64,
    planned: 41,
    ongoing: 3,
    dropped: 7,
    inLibrary: 12,
    notInLibrary: 118,
    available: true,
  };
  next.playtime = { any: 248, atLeast10h: 41, atLeast100h: 4 };
  next.played = { any: 248, played: 232, unplayed: 16 };
  next.lastPlayed = {
    any: 248,
    last7Days: 22,
    last30Days: 47,
    last90Days: 61,
    lastYear: 130,
    olderThanYear: 118,
  };
  next.release = [
    { value: { decade: 2020 }, count: 30 },
    { value: { decade: 2000 }, count: 5 },
    { value: 'earlier', count: 2 },
    { value: 'unknown', count: 7 },
  ];
  next.added = {
    any: 248,
    last7Days: 8,
    last30Days: 21,
    last90Days: 40,
    lastYear: 90,
    olderThanYear: 158,
  };
  next.favoritesOnly = 12;
  next.runningOnly = 1;
  next.showHidden = 4;
  change(next);
  return next;
}

function popoverLibrary(nextFilters: LibraryFilters = filters(), nextCounts: FilterCounts = counts()) {
  const base = makeLibrary({ total: 248, rows: [] as Row[] });
  return makeLibrary({
    total: 248,
    rows: [],
    layout: { ...base.layout, filters: nextFilters },
    filterCounts: nextCounts,
  });
}

function PopoverWithGlobalShortcuts() {
  useGlobalShortcuts();
  return (
    <FilterPopover>
      <button type="button" data-testid="filters-trigger">
        Filters
      </button>
    </FilterPopover>
  );
}

function renderPopover(library = popoverLibrary(), locale: 'en' | 'pt-BR' = 'en') {
  return renderControls(<PopoverWithGlobalShortcuts />, library, locale);
}

async function openPopover() {
  fireEvent.click(screen.getByTestId('filters-trigger'));
  const popover = await screen.findByTestId('library-filters-popover');
  await waitFor(() => expect(popover).toBeVisible());
  return popover;
}

describe('FilterPopover', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('renders every section and its engine-provided counts even with no rows', async () => {
    renderPopover();
    const popover = await openPopover();

    // Mantine numbers are rem; the px intent is passed as a calc string so the
    // reference 760×565 stays exact under the 14px root font size.
    expect(popover.style.width).toBe('calc(760px)');
    expect(popover).toHaveClass('h-[565px]', 'w-[760px]');

    for (const id of [
      'availability',
      'stores',
      'shortcuts',
      'match',
      'gamenode-status',
      'playtime',
      'played',
      'last-played',
      'release',
      'added',
    ]) {
      expect(screen.getByTestId(`filter-section-${id}`)).toBeInTheDocument();
    }

    expect(screen.getByTestId('filter-option-availability-installed')).toHaveTextContent('37');
    expect(screen.getByTestId('filter-option-stores-steam')).toHaveTextContent('164');
    expect(screen.getByTestId('filter-option-shortcuts-favorites')).toHaveTextContent('12');
    expect(screen.getByTestId('filter-option-match-matched')).toHaveTextContent('221');
    expect(screen.getByTestId('filter-option-gamenode-status-playing')).toHaveTextContent('18');
    expect(screen.getByTestId('filter-section-playtime')).toHaveTextContent('10 h+41');
    expect(screen.getByTestId('filter-section-playtime')).toHaveTextContent('100 h+4');
    expect(screen.getByTestId('filter-section-played')).toHaveTextContent('Never played16');
    expect(screen.getByTestId('filter-option-last-played-last30Days')).toHaveTextContent('47');
    expect(screen.getByTestId('filter-option-release-decade:2020')).toHaveTextContent('30');
    expect(screen.getByTestId('filter-option-added-last30Days')).toHaveTextContent('21');
  });

  it('constrains the whole surface to the available height and keeps the footer reachable', async () => {
    renderPopover();
    const popover = await openPopover();
    const header = screen.getByTestId('filter-popover-header');
    const body = screen.getByTestId('filter-popover-body');
    const footer = screen.getByTestId('filter-popover-footer');

    // Mantine's size middleware writes the real available height as the
    // dropdown's max-height, so a trigger low in a 600 px window cannot push
    // the footer past the viewport.
    expect(FILTER_POPOVER_MIDDLEWARES).toEqual({
      flip: true,
      shift: true,
      size: { padding: { top: 8, bottom: 8 } },
    });
    expect(popover).toHaveClass('flex', 'h-[565px]', 'w-[760px]', 'flex-col');
    expect(popover).not.toHaveClass('max-h-[calc(100vh_-_120px)]');

    // Header and footer stay at their intrinsic height; only the body shrinks
    // and scrolls when the surface is capped.
    expect(header).toHaveClass('shrink-0');
    expect(body).toHaveClass('min-h-0', 'flex-1', 'overflow-y-auto');
    expect(footer).toHaveClass('shrink-0');
    expect(
      header.compareDocumentPosition(body) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      body.compareDocumentPosition(footer) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(footer).toContainElement(screen.getByTestId('filter-clear-all-btn'));
    expect(footer).toContainElement(screen.getByTestId('filter-done-btn'));
  });

  it('toggles the availability facts that own the filter, not the sidebar scope', async () => {
    const input = spyLibraryInput();
    renderPopover();
    await openPopover();
    const view = screen.getByTestId('filter-option-availability-installed');

    fireEvent.click(within(view).getByRole('checkbox'));

    const inputs = setInputs(input);
    expect(inputs).toContainEqual({
      setFilters: expect.objectContaining({
        availability: { installed: true, owned: false, shared: false },
      }),
    });
    expect(inputs.some((entry) => typeof entry === 'object' && 'setScope' in entry)).toBe(false);
  });

  it('adds and removes several stores as alternatives', async () => {
    const input = spyLibraryInput();
    renderPopover(popoverLibrary(filters((next) => { next.stores = ['steam']; })));
    await openPopover();
    const view = screen.getByTestId('filter-option-stores-epic');

    fireEvent.click(within(view).getByRole('checkbox'));
    expect(setInputs(input)).toContainEqual({
      setFilters: expect.objectContaining({ stores: ['steam', 'epic'] }),
    });
  });

  it('selects and clears a single-choice last-session chip', async () => {
    const input = spyLibraryInput();
    renderPopover(popoverLibrary(filters((next) => { next.lastPlayed = 'last30Days'; })));
    await openPopover();
    const chip = screen.getByTestId('filter-option-last-played-last30Days');

    expect(chip).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(chip);
    expect(setInputs(input)).toContainEqual({
      setFilters: expect.objectContaining({ lastPlayed: 'any' }),
    });
  });

  it('selects and clears a release decade as a single-choice chip', async () => {
    const input = spyLibraryInput();
    renderPopover();
    await openPopover();

    fireEvent.click(screen.getByTestId('filter-option-release-decade:2020'));
    expect(setInputs(input)).toContainEqual({
      setFilters: expect.objectContaining({ release: { decade: 2020 } }),
    });
  });

  it('combines several match states as alternatives', async () => {
    const input = spyLibraryInput();
    renderPopover(popoverLibrary(filters((next) => { next.matchStates = ['matched']; })));
    await openPopover();

    fireEvent.click(
      within(screen.getByTestId('filter-option-match-ambiguous')).getByRole('checkbox'),
    );
    expect(setInputs(input)).toContainEqual({
      setFilters: expect.objectContaining({ matchStates: ['matched', 'ambiguous'] }),
    });
  });

  it('offers only the release decades and extras that the counts return', async () => {
    renderPopover();
    await openPopover();

    expect(screen.getByTestId('filter-option-release-decade:2020')).toHaveTextContent('2020s');
    expect(screen.getByTestId('filter-option-release-decade:2000')).toHaveTextContent('2000s');
    expect(screen.getByTestId('filter-option-release-earlier')).toHaveTextContent('Before 1990');
    expect(screen.getByTestId('filter-option-release-unknown')).toHaveTextContent('Unknown');
    expect(screen.queryByTestId('filter-option-release-decade:2010')).not.toBeInTheDocument();
  });

  it('hides sections and non-matching options with the filter search', async () => {
    renderPopover();
    await openPopover();

    fireEvent.change(screen.getByTestId('library-filter-search'), { target: { value: 'steam' } });

    expect(screen.queryByTestId('filter-section-availability')).not.toBeInTheDocument();
    expect(screen.queryByTestId('filter-section-shortcuts')).not.toBeInTheDocument();
    expect(screen.getByTestId('filter-section-stores')).toBeInTheDocument();
    expect(screen.getByTestId('filter-option-stores-steam')).toBeInTheDocument();
    expect(screen.queryByTestId('filter-option-stores-epic')).not.toBeInTheDocument();
  });

  it('shows the no-match message when the filter search matches nothing', async () => {
    renderPopover();
    await openPopover();

    fireEvent.change(screen.getByTestId('library-filter-search'), { target: { value: 'zzz' } });

    expect(screen.getByTestId('filter-popover-empty')).toHaveTextContent(
      'No filter matches “zzz”',
    );
    expect(screen.queryByTestId('filter-section-availability')).not.toBeInTheDocument();
  });

  it('disables the GameNode status facet with a reason when the account library is unread', async () => {
    const nextCounts = counts();
    nextCounts.gamenodeStatus.available = false;
    renderPopover(popoverLibrary(filters(), nextCounts));
    await openPopover();

    expect(screen.getByTestId('filter-section-gamenode-status-disabled')).toHaveTextContent(
      'Could not check your GameNode library. Try again.',
    );
    expect(screen.getByTestId('filter-option-gamenode-status-playing')).toHaveAttribute(
      'title',
      'Could not check your GameNode library. Try again.',
    );
    expect(
      within(screen.getByTestId('filter-option-gamenode-status-playing')).getByRole('checkbox'),
    ).toBeDisabled();
  });

  it('enables the GameNode status facet once the account library is available', async () => {
    renderPopover();
    await openPopover();

    expect(screen.queryByTestId('filter-section-gamenode-status-disabled')).not.toBeInTheDocument();
    expect(
      within(screen.getByTestId('filter-option-gamenode-status-playing')).getByRole('checkbox'),
    ).toBeEnabled();
  });

  it('only lists Ongoing when it has rows', async () => {
    const withoutOngoing = counts();
    withoutOngoing.gamenodeStatus.ongoing = 0;
    const first = renderPopover(popoverLibrary(filters(), withoutOngoing));
    await openPopover();
    expect(screen.queryByTestId('filter-option-gamenode-status-ongoing')).not.toBeInTheDocument();
    first.unmount();

    resetUiStore();
    renderPopover();
    await openPopover();
    expect(screen.getByTestId('filter-option-gamenode-status-ongoing')).toBeInTheDocument();
  });

  it('opens with Ctrl+Shift+F and keeps Ctrl combos working in text fields', async () => {
    renderPopover();
    fireEvent.keyDown(document.body, { key: 'F', ctrlKey: true, shiftKey: true });
    expect(await screen.findByTestId('library-filters-popover')).toBeInTheDocument();
    expect(uiStore.state.filtersOpen).toBe(true);

    fireEvent.click(screen.getByTestId('filter-done-btn'));
    expect(uiStore.state.filtersOpen).toBe(false);

    const text = document.createElement('input');
    document.body.append(text);
    text.focus();
    fireEvent.keyDown(text, { key: 'f', ctrlKey: true, shiftKey: true });
    expect(uiStore.state.filtersOpen).toBe(true);
    text.remove();
    fireEvent.click(screen.getByTestId('filter-done-btn'));
    expect(uiStore.state.filtersOpen).toBe(false);

    openPalette();
    fireEvent.keyDown(document.body, { key: 'f', ctrlKey: true, shiftKey: true });
    expect(uiStore.state.filtersOpen).toBe(false);
  });

  it('ignores a Ctrl+Shift+F event another handler consumed', () => {
    renderPopover();
    const event = new KeyboardEvent('keydown', {
      key: 'f',
      ctrlKey: true,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    event.preventDefault();
    document.body.dispatchEvent(event);
    expect(uiStore.state.filtersOpen).toBe(false);
  });

  it('closes with Escape and returns focus to the Filters button', async () => {
    renderPopover();
    const trigger = screen.getByTestId('filters-trigger');
    trigger.focus();
    await openPopover();

    fireEvent.keyDown(screen.getByTestId('library-filter-search'), { key: 'Escape', code: 'Escape' });

    await waitFor(() => expect(uiStore.state.filtersOpen).toBe(false));
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it('toggles a facet from the keyboard', async () => {
    const input = spyLibraryInput();
    renderPopover();
    await openPopover();

    const checkbox = within(screen.getByTestId('filter-option-stores-steam')).getByRole('checkbox');
    checkbox.focus();
    await userEvent.keyboard(' ');

    expect(setInputs(input)).toContainEqual({
      setFilters: expect.objectContaining({ stores: ['steam'] }),
    });
  });

  it('applies changes immediately, clears every facet and closes with Done', async () => {
    const input = spyLibraryInput();
    renderPopover();
    await openPopover();

    fireEvent.click(
      within(screen.getByTestId('filter-option-shortcuts-favorites')).getByRole('switch'),
    );
    expect(setInputs(input)).toContainEqual({
      setFilters: expect.objectContaining({ favoritesOnly: true }),
    });

    fireEvent.click(screen.getByTestId('filter-clear-all-btn'));
    expect(setInputs(input)).toContainEqual({ setFilters: createDefaultFilters() });

    fireEvent.click(screen.getByTestId('filter-done-btn'));
    expect(uiStore.state.filtersOpen).toBe(false);
  });

  it('renders the three-column copy in pt-BR', async () => {
    renderPopover(popoverLibrary(), 'pt-BR');
    await openPopover();

    expect(screen.getByTestId('filter-section-availability')).toHaveTextContent('Disponibilidade');
    expect(screen.getByTestId('filter-section-stores')).toHaveTextContent('Loja');
    expect(screen.getByTestId('filter-section-playtime')).toHaveTextContent('Tempo de jogo');
    expect(screen.getByTestId('filter-section-played')).toHaveTextContent('Histórico');
    expect(screen.getByTestId('filter-section-last-played')).toHaveTextContent('Última sessão');
    expect(screen.getByTestId('filter-section-release')).toHaveTextContent('Lançamento');
    expect(screen.getByTestId('filter-section-added')).toHaveTextContent('Adicionado');
    expect(screen.getByTestId('filter-option-release-decade:2020')).toHaveTextContent('Anos 2020');
    expect(screen.getByTestId('filter-clear-all-btn')).toHaveTextContent('Limpar tudo');
    expect(screen.getByTestId('filter-done-btn')).toHaveTextContent('Concluído');
  });

  it('honours an externally opened shared state', async () => {
    renderPopover();
    act(() => openFilters());
    expect(await screen.findByTestId('library-filters-popover')).toBeInTheDocument();
  });
});
