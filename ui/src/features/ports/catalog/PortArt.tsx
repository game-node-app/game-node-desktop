import React, { useCallback, useState } from 'react';
import { portArtUrl, type PortArtKind } from '../../../api/ports';

export interface PortArtProps {
  catalogId: string;
  title: string;
  kind?: PortArtKind;
  className?: string;
}

/**
 * Artwork URL that has already loaded in this session. The virtualized views remount
 * images as rows scroll, and a known image should not fade in again.
 */
const loadedArt = new Set<string>();

/** Test helper: forget which artwork has loaded. */
export function resetLoadedPortArt(): void {
  loadedArt.clear();
}

/**
 * `port-art` cover used by the catalog tiles and the detail pane. Mirrors the library's
 * `CoverImage`: lazy decoding through the custom protocol, and a titled placeholder when
 * the cache holds no art instead of a broken-image icon (desktop-port-catalog spec).
 */
export const PortArt: React.FC<PortArtProps> = ({
  catalogId,
  title,
  kind = 'capsule',
  className,
}) => {
  const src = portArtUrl(catalogId, kind);
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const [loadedSrc, setLoadedSrc] = useState<string | null>(null);
  const failed = failedSrc === src;
  const loaded = loadedSrc === src || loadedArt.has(src);

  const markLoaded = useCallback((url: string) => {
    loadedArt.add(url);
    setLoadedSrc(url);
  }, []);

  if (failed) {
    return (
      <div
        data-testid="port-art-placeholder"
        className={`flex h-full w-full select-none items-center justify-center overflow-hidden break-words rounded-[inherit] bg-surface p-2 text-center text-xs text-text-default ${
          className ?? ''
        }`}
      >
        <span className="line-clamp-3 font-medium">{title}</span>
      </div>
    );
  }

  return (
    <img
      // Cached images can finish before React attaches onLoad.
      ref={(img) => {
        if (img && img.complete && img.naturalWidth > 0 && !loadedArt.has(src)) {
          markLoaded(src);
        }
      }}
      data-testid="port-art"
      src={src}
      alt={title}
      loading="lazy"
      decoding="async"
      onLoad={() => markLoaded(src)}
      onError={() => setFailedSrc(src)}
      className={`block h-full w-full rounded-[inherit] bg-surface object-cover transition-opacity duration-150 ${
        loaded ? 'opacity-100' : 'opacity-0'
      } ${className ?? ''}`}
    />
  );
};
