import { useState } from 'react';
import { CURATED_SIGN_IN_COVERS, resolveMuralCovers } from './coverWall';

const COLUMNS = 5;
const ROWS = 5;
const COLUMN_PITCH = 162;
const ROW_PITCH = 212;

interface MuralPosition {
  key: string;
  left: number;
  top: number;
}

function positions(): MuralPosition[] {
  return Array.from({ length: COLUMNS * ROWS }, (_, index) => {
    const column = index % COLUMNS;
    const row = Math.floor(index / COLUMNS);
    return {
      key: `${column}-${row}`,
      // Odd columns start above the pane and even columns lower, per the design reference.
      left: -30 + column * COLUMN_PITCH,
      top: (column % 2 === 0 ? -60 : 30) + row * ROW_PITCH,
    };
  });
}

export interface CoverMuralProps {
  /**
   * Cover file names from the host, most recent first. Real cache files keep
   * their custom-protocol URL; mock placeholders and unused slots are filled
   * with bundled real IGDB cover art.
   */
  files: string[];
}

/**
 * Static, decorative cover wall behind the sign-in art pane. Positions are the only runtime
 * inline styles; everything else comes from the theme tokens. Artwork is always real: the
 * user's cached covers first, then the bundled IGDB samples.
 */
export function CoverMural({ files }: CoverMuralProps) {
  const covers = resolveMuralCovers(files);
  const [failedSrcs, setFailedSrcs] = useState<Record<string, true>>({});

  const markFailed = (src: string) => {
    setFailedSrcs((prev) => (prev[src] ? prev : { ...prev, [src]: true }));
  };

  return (
    <div
      data-testid="sign-in-cover-mural"
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 overflow-hidden"
    >
      {positions().map(({ key, left, top }, index) => {
        const cover = covers[index]!;
        const cacheFailed = cover.source === 'cache' && failedSrcs[cover.src];
        const fallback =
          CURATED_SIGN_IN_COVERS[
            CURATED_SIGN_IN_COVERS.length - 1 - (index % CURATED_SIGN_IN_COVERS.length)
          ]!;
        const effectiveSrc = cacheFailed ? fallback.src : cover.src;
        const effectiveSource = cacheFailed ? 'curated' : cover.source;
        const hideBroken = failedSrcs[effectiveSrc];

        return (
          <div
            key={key}
            data-testid="sign-in-cover-tile"
            data-source={effectiveSource}
            className="absolute h-[200px] w-[150px] overflow-hidden rounded-md bg-raised opacity-[0.55]"
            style={{ left, top }}
          >
            {!hideBroken ? (
              <img
                data-testid="sign-in-cover-image"
                src={effectiveSrc}
                alt=""
                loading="lazy"
                decoding="async"
                onError={() => markFailed(effectiveSrc)}
                className="h-full w-full object-cover"
              />
            ) : null}
          </div>
        );
      })}
      <div className="pointer-events-none absolute inset-0 bg-signin-side" />
      <div className="pointer-events-none absolute inset-0 bg-signin-bottom" />
    </div>
  );
}
