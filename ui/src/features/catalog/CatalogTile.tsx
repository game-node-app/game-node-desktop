import React from 'react';
import { IconBookmark, IconDeviceDesktop } from '@tabler/icons-react';
import type { CatalogResult } from '../../api';
import { GameTile, Pill } from '../../components/ui';
import { useI18n } from '../../i18n';
import { catalogResultRow } from './catalogPresentation';

export interface CatalogTileProps {
  result: CatalogResult;
  selected?: boolean;
  focused?: boolean;
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void;
  onContextMenu?: (event: React.MouseEvent<HTMLDivElement>) => void;
  onDoubleClick?: (event: React.MouseEvent<HTMLDivElement>) => void;
}

/**
 * One catalog result on the shared presentational `GameTile`. The tile keeps
 * the library geometry, cached lazy cover, selection/focus rings and hover
 * surface. Catalog-only facts are a local overlay and the tile's `meta` slot,
 * so the shared primitive stays free of catalog-specific props. The projected
 * `Row` is inert presentation: the tile renders the release year instead of
 * the library match dot and playtime, and its accessible name carries only the
 * facts the catalog actually has.
 */
export const CatalogTile: React.FC<CatalogTileProps> = ({
  result,
  selected = false,
  focused = false,
  onSelect,
  onContextMenu,
  onDoubleClick,
}) => {
  const { t } = useI18n();
  const year = result.year != null ? String(result.year) : null;
  const badges = [
    result.inLibrary ? t('Catalog::In your library') : null,
    result.localKey != null ? t('Catalog::On this PC') : null,
  ].filter((label): label is string => label != null);
  const accessibleName = [result.title, year, ...badges]
    .filter((part): part is string => part != null)
    .join(' · ');

  return (
    <div
      data-testid={`catalog-tile-${result.gameId}`}
      data-catalog-game-id={result.gameId}
      data-selected={selected ? 'true' : undefined}
      data-focused={focused ? 'true' : undefined}
      className="relative rounded-md hover:ring-1 hover:ring-border-strong"
    >
      <GameTile
        row={catalogResultRow(result)}
        selected={selected}
        focused={focused}
        meta={year}
        accessibleName={accessibleName}
        onSelect={onSelect}
        onContextMenu={onContextMenu}
        onDoubleClick={onDoubleClick}
        data-testid={`game-tile-catalog:${result.gameId}`}
      />

      {/* The facts above are already in the tile's accessible name. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute left-[12px] top-[12px] flex max-w-[148px] flex-col items-start gap-[4px]"
      >
        {result.inLibrary ? (
          <Pill tone="neutral" icon={IconBookmark}>
            {t('Catalog::In your library')}
          </Pill>
        ) : null}
        {result.localKey != null ? (
          <Pill tone="success" icon={IconDeviceDesktop}>
            {t('Catalog::On this PC')}
          </Pill>
        ) : null}
      </div>
    </div>
  );
};
