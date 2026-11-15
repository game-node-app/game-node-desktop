import { describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { Row } from '../../api';
import { GameTile } from './GameTile';
import { renderUiWithI18n } from './testUtils';

const LAST_PLAYED = Math.floor(new Date(2026, 10, 13, 12, 0, 0).getTime() / 1000);

function makeRow(overrides: Partial<Row> = {}): Row {
  return {
    playtime: { seconds: 42 * 3600 + 18 * 60, source: 'store', lastPlayedAt: LAST_PLAYED },
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
    addedAt: LAST_PLAYED,
    isFavorite: false,
    hidden: false,
    gamenode: { status: 'playing', collectionIds: [], entryId: 'entry-1' },
    ...overrides,
  };
}

describe('GameTile', () => {
  it('renders the fixed 172×270 tile with a lazy cover and short meta', () => {
    renderUiWithI18n(<GameTile row={makeRow()} />);

    const tile = screen.getByTestId('game-tile-steam:1');
    expect(tile).toHaveClass('h-[270px]', 'w-[172px]', 'p-[6px]', 'rounded-md');
    expect(within(tile).getByText('Emberfall: Iron Trail')).toHaveClass(
      'truncate',
      'text-sm',
      'font-medium',
    );
    expect(within(tile).getByText('Steam')).toBeInTheDocument();
    expect(within(tile).getByText('42 h')).toBeInTheDocument();

    const cover = within(tile).getByRole('img', { name: 'Emberfall: Iron Trail' });
    expect(cover).toHaveAttribute('loading', 'lazy');
    expect(cover).toHaveAttribute('decoding', 'async');
    expect(cover.getAttribute('src')).toContain('/cover/emberfall-cover.svg');
    expect(screen.queryByTestId('game-cover-placeholder')).not.toBeInTheDocument();
  });

  it('shows installed, family, favorite and hidden badges', () => {
    const row = makeRow({
      installed: true,
      shared: true,
      owned: false,
      isFavorite: true,
      hidden: true,
    });
    renderUiWithI18n(<GameTile row={row} />);

    const tile = screen.getByTestId('game-tile-steam:1');
    expect(within(tile).getByText('Installed')).toBeInTheDocument();
    expect(within(tile).getByText('Family')).toBeInTheDocument();
    expect(within(tile).getByText('Hidden')).toBeInTheDocument();
    expect(within(tile).getByRole('img', { name: 'Favorite' })).toBeInTheDocument();
    expect(tile).toHaveClass('opacity-50');
    expect(tile).toHaveAttribute('data-hidden', 'true');

    const hiddenBadge = within(tile).getByText('Hidden').parentElement;
    expect(hiddenBadge?.querySelector('svg')).not.toBeNull();
  });

  it('replaces the installed badge with the running badge and its session time', () => {
    renderUiWithI18n(
      <GameTile row={makeRow({ running: true, installed: true })} runningTime="0:42:10" />,
    );

    const tile = screen.getByTestId('game-tile-steam:1');
    expect(within(tile).getByText('Running · 0:42:10')).toBeInTheDocument();
    expect(within(tile).queryByText('Installed')).not.toBeInTheDocument();
    const runningBadge = within(tile).getByText('Running · 0:42:10');
    expect(runningBadge.parentElement).toHaveClass('bg-success', 'text-canvas');
  });

  it('draws the title over the placeholder only when there is no real cover', () => {
    const { rerender } = renderUiWithI18n(
      <GameTile row={makeRow({ coverFile: null })} />,
    );

    const placeholder = screen.getByTestId('game-cover-placeholder');
    const overlay = within(placeholder).getByText('Emberfall: Iron Trail');
    expect(overlay).toHaveClass('line-clamp-3', 'text-lg', 'font-bold');
    expect(
      within(screen.getByTestId('game-tile-steam:1')).getAllByText(
        'Emberfall: Iron Trail',
      ),
    ).toHaveLength(2);

    rerender(<GameTile row={makeRow()} />);
    expect(screen.queryByTestId('game-cover-placeholder')).not.toBeInTheDocument();
    expect(
      within(screen.getByTestId('game-tile-steam:1')).queryByText('Emberfall: Iron Trail'),
    ).toBeInTheDocument();
  });

  it('replaces the library metadata row when the caller supplies catalog facts', () => {
    renderUiWithI18n(
      <GameTile
        row={makeRow()}
        meta="2024"
        accessibleName="Emberfall: Iron Trail · 2024"
      />,
    );

    const tile = screen.getByTestId('game-tile-steam:1');
    expect(tile).toHaveAttribute('aria-label', 'Emberfall: Iron Trail · 2024');
    expect(within(tile).getByText('2024')).toBeInTheDocument();
    expect(within(tile).queryByText('Steam')).not.toBeInTheDocument();
    expect(within(tile).queryByText('42 h')).not.toBeInTheDocument();
    expect(tile).not.toHaveTextContent('Matched');
  });

  it('keeps the library match dot and playtime when no metadata slot is given', () => {
    renderUiWithI18n(<GameTile row={makeRow()} />);

    const tile = screen.getByTestId('game-tile-steam:1');
    expect(within(tile).getByText('Steam')).toBeInTheDocument();
    expect(within(tile).getByText('42 h')).toBeInTheDocument();
    expect(within(tile).getByText('Matched')).toHaveClass('sr-only');
    expect(tile).toHaveAttribute(
      'aria-label',
      'Emberfall: Iron Trail · Installed · Matched',
    );
  });

  it('uses a remote cover URL for catalog results', () => {
    renderUiWithI18n(
      <GameTile
        row={makeRow({ coverFile: null, key: 'catalog:9', title: 'Celeste' })}
        coverUrl="https://cdn.gamenode.app/covers/celeste.jpg"
      />,
    );

    const cover = screen.getByRole('img', { name: 'Celeste' });
    expect(cover).toHaveAttribute(
      'src',
      'https://cdn.gamenode.app/covers/celeste.jpg',
    );
    expect(cover).toHaveAttribute('loading', 'lazy');
    expect(cover).toHaveAttribute('decoding', 'async');
  });

  it('marks the selected and focused states with brand rings', () => {
    const { rerender } = renderUiWithI18n(<GameTile row={makeRow()} selected />);
    let tile = screen.getByTestId('game-tile-steam:1');
    expect(tile).toHaveAttribute('aria-selected', 'true');
    expect(tile).toHaveClass('bg-accent-subtle', 'ring-[1.5px]', 'ring-brand');
    expect(tile).toHaveAttribute('data-selected', 'true');

    rerender(<GameTile row={makeRow()} selected focused />);
    tile = screen.getByTestId('game-tile-steam:1');
    expect(tile).toHaveAttribute('aria-selected', 'true');
    expect(tile).toHaveClass('ring-2', 'ring-brand');
    expect(tile).toHaveAttribute('data-focused', 'true');
    expect(tile).toHaveAttribute('tabindex', '0');
  });

  it('offers the hover play action without selecting the tile', async () => {
    const onPrimaryAction = vi.fn();
    const onSelect = vi.fn();
    const { rerender } = renderUiWithI18n(
      <GameTile
        row={makeRow()}
        primaryAction="play"
        onPrimaryAction={onPrimaryAction}
        onSelect={onSelect}
      />,
    );

    await userEvent.click(screen.getByRole('button', { name: 'Play' }));
    expect(onPrimaryAction).toHaveBeenCalledTimes(1);
    expect(onSelect).not.toHaveBeenCalled();

    rerender(
      <GameTile
        row={makeRow()}
        primaryAction="install"
        onPrimaryAction={onPrimaryAction}
        onSelect={onSelect}
      />,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Install' }));
    expect(onPrimaryAction).toHaveBeenCalledTimes(2);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('forwards pointer and keyboard callbacks from the tile root', () => {
    const onSelect = vi.fn();
    const onContextMenu = vi.fn();
    const onDoubleClick = vi.fn();
    const onKeyDown = vi.fn();
    renderUiWithI18n(
      <GameTile
        row={makeRow()}
        onSelect={onSelect}
        onContextMenu={onContextMenu}
        onDoubleClick={onDoubleClick}
        onKeyDown={onKeyDown}
      />,
    );

    const tile = screen.getByTestId('game-tile-steam:1');
    fireEvent.click(tile);
    fireEvent.contextMenu(tile);
    fireEvent.doubleClick(tile);
    fireEvent.keyDown(tile, { key: 'Enter' });
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onContextMenu).toHaveBeenCalledTimes(1);
    expect(onDoubleClick).toHaveBeenCalledTimes(1);
    expect(onKeyDown).toHaveBeenCalledTimes(1);
  });

  it('renders its copy in Brazilian Portuguese', () => {
    renderUiWithI18n(
      <GameTile
        row={makeRow({ shared: true, owned: false, hidden: true })}
      />,
      'pt-BR',
    );

    const tile = screen.getByTestId('game-tile-steam:1');
    expect(within(tile).getByText('Instalado')).toBeInTheDocument();
    expect(within(tile).getByText('Família')).toBeInTheDocument();
    expect(within(tile).getByText('Oculto')).toBeInTheDocument();
  });
});
