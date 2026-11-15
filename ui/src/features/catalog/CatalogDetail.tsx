import React from 'react';
import { IconBooks, IconExternalLink, IconPlus, IconX } from '@tabler/icons-react';
import type { CatalogResult } from '../../api';
import { Button, GameCover, IconButton, KeyValueRow } from '../../components/ui';
import { useI18n } from '../../i18n';

export interface CatalogDetailProps {
  result: CatalogResult;
  width: number;
  onClose: () => void;
  onAdd: (result: CatalogResult) => void;
  onView: (result: CatalogResult) => void;
  onShow: (result: CatalogResult) => void;
}

/**
 * The catalog detail variant in the shared right-hand detail slot. It has the
 * same 196 px hero, a 15/600 title, the release year as a `KeyValueRow`, and
 * only the actions that apply: Add while the game is not in the library, View
 * on GameNode once it is, and Show in library when a local row is matched.
 */
export const CatalogDetail: React.FC<CatalogDetailProps> = ({
  result,
  width,
  onClose,
  onAdd,
  onView,
  onShow,
}) => {
  const { t } = useI18n();

  return (
    <aside
      data-testid="catalog-detail"
      aria-label={result.title}
      style={{ width: `${width}px` }}
      className="flex h-full shrink-0 flex-col overflow-y-auto border-l border-border-subtle bg-secondary"
    >
      <div className="relative h-[196px] w-full shrink-0 overflow-hidden bg-surface">
        <GameCover src={result.coverUrl} title={result.title} />
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 bg-gradient-to-b from-transparent to-secondary"
        />
        <div className="absolute right-[12px] top-[12px]">
          <IconButton
            icon={IconX}
            label={t('Detail::Close details')}
            variant="overlay"
            size="sm"
            data-testid="catalog-detail-close"
            onClick={onClose}
          />
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-[16px] p-[16px]">
        <h2
          data-testid="catalog-detail-title"
          title={result.title}
          className="m-0 line-clamp-2 text-title font-semibold leading-tight text-text-primary"
        >
          {result.title}
        </h2>

        {result.year != null ? (
          <div data-testid="catalog-detail-year">
            <KeyValueRow
              label={t('Library::Release year')}
              value={String(result.year)}
              last
            />
          </div>
        ) : null}

        <div className="flex flex-col gap-[8px]">
          {!result.inLibrary ? (
            <Button
              variant="primary"
              fullWidth
              leftIcon={IconPlus}
              data-testid="catalog-detail-add"
              onClick={() => onAdd(result)}
            >
              {t('Catalog::Add to GameNode…')}
            </Button>
          ) : (
            <Button
              variant="secondary"
              fullWidth
              leftIcon={IconExternalLink}
              data-testid="catalog-detail-view"
              onClick={() => onView(result)}
            >
              {t('Catalog::View on GameNode')}
            </Button>
          )}

          {result.localKey != null ? (
            <Button
              variant="ghost"
              fullWidth
              leftIcon={IconBooks}
              data-testid="catalog-detail-show"
              onClick={() => onShow(result)}
            >
              {t('Catalog::Show in library')}
            </Button>
          ) : null}
        </div>
      </div>
    </aside>
  );
};
