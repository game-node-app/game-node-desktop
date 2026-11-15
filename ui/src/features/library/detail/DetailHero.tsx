import { IconUsersGroup, IconX } from '@tabler/icons-react';
import { artworkAssetUrl, getMatchedGameId, type DetailView } from '../../../api';
import { IconButton } from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { CoverImage } from '../CoverImage';

export interface DetailHeroProps {
  detail: DetailView;
  sharedLabel: string | null;
  onClose: () => void;
}

/** The 196px detail hero: artwork or cover, a bottom fade, title, meta and the close button. */
export function DetailHero({ detail, sharedLabel, onClose }: DetailHeroProps) {
  const { t } = useI18n();
  const gameId = getMatchedGameId(detail.row.status);
  const artwork =
    detail.hasArtwork && gameId != null ? artworkAssetUrl(gameId) : null;

  return (
    <div
      data-testid="detail-artwork-container"
      className="relative h-[196px] w-full shrink-0 overflow-hidden bg-surface"
    >
      {artwork ? (
        <img
          data-testid="detail-artwork-img"
          src={artwork}
          alt={detail.row.title}
          loading="lazy"
          decoding="async"
          className="block h-full w-full object-cover"
        />
      ) : (
        <CoverImage
          coverFile={detail.row.coverFile}
          title={detail.row.title}
          detectedName={detail.row.detectedName}
        />
      )}

      <div className="pointer-events-none absolute inset-0 bg-gradient-to-b from-transparent to-secondary" />

      <div className="absolute right-[12px] top-[12px]">
        <IconButton
          icon={IconX}
          label={t('Detail::Close details')}
          variant="overlay"
          size="sm"
          data-testid="detail-close-btn"
          onClick={onClose}
        />
      </div>

      <div className="absolute bottom-[16px] left-[16px] right-[16px] flex flex-col gap-[4px]">
        <h2
          data-testid="detail-title"
          className="truncate text-xl font-semibold leading-tight text-text-primary"
          title={detail.row.title}
        >
          {detail.row.title}
        </h2>
        <div className="flex min-w-0 flex-wrap items-center gap-[8px] text-xs text-text-secondary">
          <span className="truncate">
            {[detail.row.providerName, detail.releaseYear]
              .filter((part) => part != null && part !== '')
              .join(' · ')}
          </span>
          {sharedLabel && detail.row.shared && !detail.row.owned && (
            <span
              data-testid="detail-shared-label"
              className="inline-flex items-center gap-[4px] rounded-full bg-accent-subtle px-[8px] py-[2px] text-[11px] font-medium text-brand"
            >
              <IconUsersGroup size={12} stroke={2} aria-hidden="true" />
              {t('Library::Shared through {label}', { label: sharedLabel })}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
