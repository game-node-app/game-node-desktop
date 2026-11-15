import { describe, expect, it, vi } from 'vitest';
import type { ComponentProps } from 'react';
import { fireEvent, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { EntryStatus, MatchStatus, Row } from '../../api';
import { GAME_ROW_LAYOUT, GAME_ROW_MIN_WIDTH, GameRow } from './GameRow';
import { renderUiWithI18n } from './testUtils';

const NOW = Math.floor(new Date(2026, 10, 14, 12, 0, 0).getTime() / 1000);
const SECONDS_PER_DAY = 24 * 60 * 60;
const sooner = (daysAgo: number) => NOW - daysAgo * SECONDS_PER_DAY;

function makeRow(overrides: Partial<Row> = {}): Row {
  return {
    playtime: {
      seconds: 42 * 3600 + 18 * 60,
      source: 'store',
      lastPlayedAt: sooner(1),
    },
    running: false,
    key: 'steam:1',
    title: 'Emberfall: Iron Trail',
    detectedName: 'Emberfall',
    matchedName: 'Emberfall: Iron Trail',
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    status: { matched: { gameId: 1, source: 'automatic' } },
    coverFile: 'emberfall-cover.svg',
    releaseYear: 2024,
    addedAt: sooner(3),
    isFavorite: false,
    hidden: false,
    gamenode: { status: 'playing', collectionIds: [], entryId: 'entry-1' },
    ...overrides,
  };
}

function renderRow(
  overrides: Partial<Row> = {},
  props: Partial<ComponentProps<typeof GameRow>> = {},
  locale: 'en' | 'pt-BR' = 'en',
) {
  return renderUiWithI18n(
    <GameRow row={makeRow(overrides)} nowSeconds={NOW} {...props} />,
    locale,
  );
}

describe('GameRow', () => {
  it('renders the 52 px row with the reference columns and long meta', () => {
    renderRow();

    const row = screen.getByTestId('game-row-steam:1');
    expect(row).toHaveClass('h-[52px]', 'gap-[12px]', 'px-[11px]', 'rounded-control');
    expect(GAME_ROW_LAYOUT).toEqual({
      height: 52,
      paddingX: 12,
      gap: 12,
      columns: {
        checkbox: 16,
        cover: 30,
        store: 130,
        gameNodeStatus: 120,
        match: 130,
        playtime: 90,
        lastSession: 110,
        favorite: 28,
      },
    });
    expect(within(row).getByText('Emberfall: Iron Trail')).toBeInTheDocument();
    expect(within(row).getByText('Installed')).toBeInTheDocument();
    expect(within(row).getByText('Steam')).toBeInTheDocument();
    expect(within(row).getByText('Playing')).toBeInTheDocument();
    expect(within(row).getByText('Matched')).toBeInTheDocument();
    expect(within(row).getByText('42 h 18 min')).toBeInTheDocument();
    expect(within(row).getByText('Yesterday')).toBeInTheDocument();

    const cover = within(row).getByRole('img', { name: 'Emberfall: Iron Trail' });
    expect(cover).toHaveAttribute('loading', 'lazy');
    expect(cover).toHaveAttribute('decoding', 'async');
  });

  it('derives the horizontal surface width from the shared column geometry', () => {
    const fixedWidth = Object.values(GAME_ROW_LAYOUT.columns).reduce(
      (total, width) => total + width,
      0,
    );
    // 12 px content insets, one gap per fixed column (the title adds the 9th
    // child), every fixed column and the 200 px title floor from the header.
    expect(GAME_ROW_MIN_WIDTH).toBe(12 * 2 + 12 * 8 + fixedWidth + 200);
    expect(GAME_ROW_MIN_WIDTH).toBe(974);
    // The border lives inside the 12 px inset, so the row content starts at
    // the same x as the header content.
    expect(GAME_ROW_LAYOUT.paddingX - 1).toBe(11);
  });

  it('identifies a not-installed entry with text and an icon', () => {
    renderRow({ installed: false, owned: true });

    const row = screen.getByTestId('game-row-steam:1');
    const flag = within(row).getByText('Not installed');
    expect(flag).toBeInTheDocument();
    expect(flag.parentElement?.querySelector('svg')).not.toBeNull();
    expect(within(row).queryByText('Installed')).not.toBeInTheDocument();
  });

  it('identifies family-shared and hidden entries without relying on opacity alone', () => {
    renderRow({ installed: false, owned: false, shared: true, hidden: true });

    const row = screen.getByTestId('game-row-steam:1');
    expect(within(row).getByText('Family shared')).toBeInTheDocument();
    const hidden = within(row).getByText('Hidden');
    expect(hidden.parentElement?.querySelector('svg')).not.toBeNull();
    expect(row).toHaveClass('opacity-50');
    expect(row).toHaveAttribute('data-hidden', 'true');
  });

  it('shows the live running flag in place of the install state', () => {
    renderRow({ running: true, installed: true }, { runningTime: '0:42:10' });

    const row = screen.getByTestId('game-row-steam:1');
    expect(within(row).getByText('Running · 0:42:10')).toBeInTheDocument();
    expect(within(row).queryByText('Installed')).not.toBeInTheDocument();
    expect(within(row).queryByText('Not installed')).not.toBeInTheDocument();
  });

  it('marks the selected and focused states with the brand ring', () => {
    const { rerender } = renderRow({}, { selected: true });
    let row = screen.getByTestId('game-row-steam:1');
    expect(row).toHaveAttribute('aria-selected', 'true');
    expect(row).toHaveClass('bg-accent-subtle', 'border-brand');

    rerender(
      <GameRow row={makeRow()} nowSeconds={NOW} selected focused />,
    );
    row = screen.getByTestId('game-row-steam:1');
    expect(row).toHaveClass('ring-2', 'ring-brand');
    expect(row).toHaveAttribute('data-focused', 'true');
    expect(row).toHaveAttribute('tabindex', '0');
  });

  it('toggles selection from the checkbox without selecting the row', async () => {
    const onToggleSelected = vi.fn();
    const onSelect = vi.fn();
    const { rerender } = renderUiWithI18n(
      <GameRow
        row={makeRow()}
        nowSeconds={NOW}
        onSelect={onSelect}
        onToggleSelected={onToggleSelected}
      />,
    );

    const checkbox = screen.getByRole('checkbox', { name: 'Emberfall: Iron Trail' });
    expect(checkbox).not.toBeChecked();
    await userEvent.click(checkbox);
    expect(onToggleSelected).toHaveBeenLastCalledWith(true);
    expect(onSelect).not.toHaveBeenCalled();

    rerender(
      <GameRow
        row={makeRow()}
        nowSeconds={NOW}
        selected
        onSelect={onSelect}
        onToggleSelected={onToggleSelected}
      />,
    );
    await userEvent.click(screen.getByRole('checkbox', { name: 'Emberfall: Iron Trail' }));
    expect(onToggleSelected).toHaveBeenLastCalledWith(false);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('toggles the favorite from its column button', async () => {
    const onToggleFavorite = vi.fn();
    const { rerender } = renderUiWithI18n(
      <GameRow row={makeRow()} nowSeconds={NOW} onToggleFavorite={onToggleFavorite} />,
    );

    await userEvent.click(screen.getByRole('button', { name: 'Add to favorites' }));
    expect(onToggleFavorite).toHaveBeenCalledTimes(1);

    rerender(
      <GameRow
        row={makeRow({ isFavorite: true })}
        nowSeconds={NOW}
        onToggleFavorite={onToggleFavorite}
      />,
    );
    await userEvent.click(
      screen.getByRole('button', { name: 'Remove from favorites' }),
    );
    expect(onToggleFavorite).toHaveBeenCalledTimes(2);
  });

  it('disables the favorite with a reason when the entry is unmatched', () => {
    renderRow(
      { status: 'unmatched', isFavorite: false, gamenode: null },
      { onToggleFavorite: vi.fn() },
    );

    const favorite = screen.getByRole('button', { name: 'Add to favorites' });
    expect(favorite).toBeDisabled();
    expect(favorite).toHaveAttribute(
      'title',
      'Match the game first to use favorites.',
    );
  });

  it.each([
    ['playing', 'Playing', 'bg-accent-subtle', 'text-brand'],
    ['finished', 'Finished', 'bg-success-subtle', 'text-success'],
    ['planned', 'Planned', 'bg-raised', 'text-text-default'],
    ['ongoing', 'Ongoing', 'bg-raised', 'text-text-default'],
    ['dropped', 'Dropped', 'bg-danger-subtle', 'text-danger'],
  ] as Array<[EntryStatus, string, string, string]>)(
    'renders the %s GameNode status pill with its tone',
    (status, label, background, text) => {
      renderRow({ gamenode: { status, collectionIds: [], entryId: 'entry-1' } });

      const pill = screen.getByTestId('game-row-steam:1');
      const textNode = within(pill).getByText(label);
      expect(textNode.parentElement).toHaveClass(background, text);
    },
  );

  it('renders In library when the cached entry has no known status', () => {
    renderRow({ gamenode: { status: null, collectionIds: [], entryId: 'entry-1' } });

    const row = screen.getByTestId('game-row-steam:1');
    expect(within(row).getByText('In library')).toBeInTheDocument();
  });

  it('keeps the status cell empty until the account library has been read', () => {
    renderRow({}, { gameNodeStatusAvailable: false });

    const row = screen.getByTestId('game-row-steam:1');
    expect(within(row).queryByText('Playing')).not.toBeInTheDocument();
    expect(within(row).queryByText('—')).not.toBeInTheDocument();
  });

  it('shows a dash when the matched entry is not in the GameNode library', () => {
    renderRow({ gamenode: null });

    const row = screen.getByTestId('game-row-steam:1');
    expect(within(row).getByText('—')).toBeInTheDocument();
  });

  it.each([
    ['ambiguous', 'Review', 'bg-warning'],
    [{ ambiguous: { candidates: [10, 11] } }, 'Review', 'bg-warning'],
    ['unmatched', 'No match', 'bg-text-dimmed'],
    ['noGame', 'Not a game', 'bg-text-muted'],
  ] as Array<[MatchStatus, string, string]>)(
    'renders the %s match label and dot',
    (status, label, dotClass) => {
      renderRow({ status, gamenode: null });

      const row = screen.getByTestId('game-row-steam:1');
      const labelNode = within(row).getByText(label);
      expect(labelNode.previousElementSibling).toHaveClass(dotClass);
    },
  );

  it('renders a matched dot and an unknown playtime with a dash', () => {
    renderRow({
      playtime: { seconds: null, source: 'internal', lastPlayedAt: null },
    });

    const row = screen.getByTestId('game-row-steam:1');
    const matched = within(row).getByText('Matched');
    expect(matched.previousElementSibling).toHaveClass('bg-success');
    expect(within(row).getByText('—')).toBeInTheDocument();
    expect(within(row).getByText('Never')).toBeInTheDocument();
  });

  it('formats a whole number of hours without minutes', () => {
    renderRow({ playtime: { seconds: 118 * 3600, source: 'store', lastPlayedAt: sooner(1) } });

    expect(
      within(screen.getByTestId('game-row-steam:1')).getByText('118 h'),
    ).toBeInTheDocument();
  });

  it.each([
    [0, 'Now'],
    [1, 'Yesterday'],
    [3, '3 days ago'],
    [8, 'Last week'],
  ] as Array<[number, string]>)('formats a last session %i days ago as %s', (days, label) => {
    renderRow({
      playtime: { seconds: 3600, source: 'store', lastPlayedAt: sooner(days) },
    });

    expect(
      within(screen.getByTestId('game-row-steam:1')).getByText(label),
    ).toBeInTheDocument();
  });

  it('formats older sessions as a month and year or a year', () => {
    const { rerender } = renderRow({
      playtime: {
        seconds: 3600,
        source: 'store',
        lastPlayedAt: Math.floor(new Date(2026, 8, 10, 12, 0, 0).getTime() / 1000),
      },
    });
    expect(
      within(screen.getByTestId('game-row-steam:1')).getByText(/Sep 2026/),
    ).toBeInTheDocument();

    rerender(
      <GameRow
        row={makeRow({
          playtime: {
            seconds: 3600,
            source: 'store',
            lastPlayedAt: Math.floor(new Date(2025, 5, 10, 12, 0, 0).getTime() / 1000),
          },
        })}
        nowSeconds={NOW}
      />,
    );
    expect(
      within(screen.getByTestId('game-row-steam:1')).getByText('2025'),
    ).toBeInTheDocument();
  });

  it('forwards row callbacks from the pointer', () => {
    const onSelect = vi.fn();
    const onContextMenu = vi.fn();
    const onDoubleClick = vi.fn();
    renderRow({}, { onSelect, onContextMenu, onDoubleClick });

    const row = screen.getByTestId('game-row-steam:1');
    fireEvent.click(row);
    fireEvent.contextMenu(row);
    fireEvent.doubleClick(row);
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onContextMenu).toHaveBeenCalledTimes(1);
    expect(onDoubleClick).toHaveBeenCalledTimes(1);
  });

  it('renders its copy in Brazilian Portuguese', () => {
    renderRow({}, {}, 'pt-BR');

    const row = screen.getByTestId('game-row-steam:1');
    expect(within(row).getByText('Instalado')).toBeInTheDocument();
    expect(within(row).getByText('Jogando')).toBeInTheDocument();
    expect(within(row).getByText('Associado')).toBeInTheDocument();
    expect(within(row).getByText('Ontem')).toBeInTheDocument();
  });
});
