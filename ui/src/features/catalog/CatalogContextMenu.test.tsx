import { describe, expect, it, vi } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { coverAssetUrl, type CatalogResult } from '../../api';
import { CatalogContextMenu } from './CatalogContextMenu';
import { makeCatalogLibrary, renderCatalog } from './catalogTestUtils';

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

function renderMenu(
  overrides: Partial<CatalogResult> = {},
  x = 120,
  y = 90,
) {
  const handlers = {
    onClose: vi.fn(),
    onAdd: vi.fn(),
    onView: vi.fn(),
    onShow: vi.fn(),
  };
  renderCatalog(
    <CatalogContextMenu result={result(overrides)} x={x} y={y} {...handlers} />,
    makeCatalogLibrary(),
  );
  return handlers;
}

describe('CatalogContextMenu', () => {
  // jsdom is 1024x768, like the desktop window tests target.
  it('anchors at a near-footer pointer without subtracting a guessed menu height', () => {
    renderMenu({}, 120, 731);

    const anchor = screen.getByTestId('catalog-context-menu-anchor');
    expect(anchor.style.position).toBe('fixed');
    expect(anchor.style.left).toBe('120px');
    expect(anchor.style.top).toBe('731px');
    expect(anchor.style.width).toBe('0px');
    expect(anchor.style.height).toBe('0px');
  });

  it('clamps a pointer outside the viewport and keeps the menu width on screen', () => {
    renderMenu({}, 5000, 5000);

    const anchor = screen.getByTestId('catalog-context-menu-anchor');
    expect(anchor.style.left).toBe('736px');
    expect(anchor.style.top).toBe('760px');
  });

  it('offers Add and Show for an absent local result with its cached cover', () => {
    const handlers = renderMenu({ localKey: 'steam:1' }, 40, 40);

    expect(screen.getByTestId('catalog-context-add')).toBeInTheDocument();
    expect(screen.getByTestId('catalog-context-show')).toBeInTheDocument();
    expect(screen.queryByTestId('catalog-context-view')).not.toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Aster Vale' })).toHaveAttribute(
      'src',
      coverAssetUrl('1.jpg'),
    );

    fireEvent.click(screen.getByTestId('catalog-context-add'));
    expect(handlers.onClose).toHaveBeenCalled();
    expect(handlers.onAdd).toHaveBeenCalledTimes(1);
    expect(handlers.onAdd).toHaveBeenCalledWith(
      expect.objectContaining({ gameId: 1, title: 'Aster Vale' }),
    );
  });
});
