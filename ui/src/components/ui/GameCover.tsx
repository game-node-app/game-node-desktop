import { useState } from 'react';
import { cx } from './cx';

export interface GameCoverProps {
  /** Local `gamenode-asset://` URL or a remote catalog URL; null renders the placeholder. */
  src: string | null;
  title: string;
  /**
   * Draw the large title over the gradient placeholder. Grid tiles use it; list
   * thumbnails keep the placeholder plain because the title sits beside them.
   */
  overlayTitle?: boolean;
  className?: string;
  'data-testid'?: string;
}

/**
 * Cover image shared by `GameTile` and `GameRow`. Local covers arrive through
 * `coverAssetUrl`; catalog covers pass their remote URL. Images always use
 * lazy loading and asynchronous decoding so scrolling never blocks the UI
 * thread, and a failed or missing cover falls back to a titled placeholder.
 */
export function GameCover({
  src,
  title,
  overlayTitle = false,
  className,
  'data-testid': testId,
}: GameCoverProps) {
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const showImage = src != null && src !== '' && failedSrc !== src;

  return (
    <div
      data-testid={testId}
      className={cx('relative h-full w-full overflow-hidden bg-surface', className)}
    >
      {showImage ? (
        <img
          src={src}
          alt={title}
          loading="lazy"
          decoding="async"
          draggable={false}
          onError={() => setFailedSrc(src)}
          className="block h-full w-full object-cover"
        />
      ) : (
        <div
          data-testid="game-cover-placeholder"
          className="flex h-full w-full items-end bg-gradient-to-br from-brand/30 via-secondary to-canvas"
        >
          {overlayTitle ? (
            <span className="line-clamp-3 px-2.5 pb-3 text-lg font-bold leading-[1.05] text-white">
              {title}
            </span>
          ) : null}
        </div>
      )}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-b from-transparent to-black/55"
      />
    </div>
  );
}
