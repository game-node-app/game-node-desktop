import React, { useEffect } from 'react';
import { Menu } from '@mantine/core';
import { IconBooks, IconExternalLink, IconPlus } from '@tabler/icons-react';
import { coverAssetUrl, type CatalogResult } from '../../api';
import { GameCover, MenuSurface } from '../../components/ui';
import { useI18n } from '../../i18n';
import { contextMenuAnchorStyle } from '../library/contextMenuAnchor';

const MENU_WIDTH = 280;

export interface CatalogContextMenuProps {
  /** The result the menu acts on; `null` renders nothing. */
  result: CatalogResult | null;
  x: number;
  y: number;
  onClose: () => void;
  onAdd: (result: CatalogResult) => void;
  onView: (result: CatalogResult) => void;
  onShow: (result: CatalogResult) => void;
}

/**
 * The tile context menu on the same Mantine menu surface as the library menu.
 * It offers exactly the catalog actions: Add while absent from the library,
 * View on GameNode once present, and Show in library when this PC has a match.
 */
export const CatalogContextMenu: React.FC<CatalogContextMenuProps> = ({
  result,
  x,
  y,
  onClose,
  onAdd,
  onView,
  onShow,
}) => {
  const { t } = useI18n();

  useEffect(() => {
    if (!result) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [result, onClose]);

  if (!result) return null;

  const anchorStyle = contextMenuAnchorStyle({ x, y, menuWidth: MENU_WIDTH });

  const run = (action: (target: CatalogResult) => void) => () => {
    onClose();
    action(result);
  };

  return (
    <MenuSurface
      opened
      onClose={onClose}
      position="bottom-start"
      offset={0}
      floatingStrategy="fixed"
      zIndex={1000}
      trapFocus={false}
      returnFocus={false}
      closeOnEscape={false}
      transitionProps={{ duration: 0 }}
      classNames={{ dropdown: 'w-[280px]' }}
    >
      {/* Zero-size anchor at the right-click or keyboard-open coordinates. */}
      <Menu.Target>
        <div data-testid="catalog-context-menu-anchor" aria-hidden="true" style={anchorStyle} />
      </Menu.Target>

      <Menu.Dropdown
        data-testid="catalog-context-menu"
        aria-label={t('Catalog::Explore GameNode')}
      >
        <div className="flex items-center gap-[10px] px-[10px] py-[8px]">
          <div className="flex h-[32px] w-[24px] shrink-0 items-center justify-center overflow-hidden rounded-xs bg-surface">
            <GameCover src={coverAssetUrl(result.coverFile)} title={result.title} />
          </div>
          <div className="min-w-0 flex-1">
            <div className="truncate text-[13px] font-semibold text-text-primary">
              {result.title}
            </div>
            {result.year != null ? (
              <div className="truncate text-[11px] font-normal text-text-dimmed">
                {result.year}
              </div>
            ) : null}
          </div>
        </div>

        <Menu.Divider />

        {!result.inLibrary ? (
          <Menu.Item
            data-testid="catalog-context-add"
            leftSection={<IconPlus size={16} stroke={2} aria-hidden="true" />}
            onClick={run(onAdd)}
          >
            {t('Catalog::Add to GameNode…')}
          </Menu.Item>
        ) : (
          <Menu.Item
            data-testid="catalog-context-view"
            leftSection={<IconExternalLink size={16} stroke={2} aria-hidden="true" />}
            onClick={run(onView)}
          >
            {t('Catalog::View on GameNode')}
          </Menu.Item>
        )}

        {result.localKey != null ? (
          <Menu.Item
            data-testid="catalog-context-show"
            leftSection={<IconBooks size={16} stroke={2} aria-hidden="true" />}
            onClick={run(onShow)}
          >
            {t('Catalog::Show in library')}
          </Menu.Item>
        ) : null}
      </Menu.Dropdown>
    </MenuSurface>
  );
};
