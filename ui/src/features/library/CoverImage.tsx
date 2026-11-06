import React, { useCallback, useState } from 'react';
import { coverAssetUrl } from '../../api';

export interface CoverImageProps {
  coverFile?: string | null;
  title: string;
  detectedName?: string | null;
  className?: string;
  style?: React.CSSProperties;
}

/**
 * Cover URLs that have already finished loading in this session. The virtualized
 * grid/list remounts <img> elements as rows scroll in and out; for known covers
 * the browser serves the decoded image from cache, so we skip the fade-in and
 * avoid a blank flash.
 */
const loadedCovers = new Set<string>();

/** Test helper: forget which covers have loaded. */
export function resetLoadedCovers(): void {
  loadedCovers.clear();
}

export const CoverImage: React.FC<CoverImageProps> = ({
  coverFile,
  title,
  detectedName,
  className,
  style,
}) => {
  const src = coverAssetUrl(coverFile);
  // Keyed by URL so a failure never outlives the src it happened for: when the
  // cover changes (e.g. arrives after a failed attempt) the new src is retried.
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const [loadedSrc, setLoadedSrc] = useState<string | null>(null);
  const hasError = src != null && failedSrc === src;
  const loaded = src != null && (loadedSrc === src || loadedCovers.has(src));

  const markLoaded = useCallback((url: string) => {
    loadedCovers.add(url);
    setLoadedSrc(url);
  }, []);

  if (src && !hasError) {
    return (
      <img
        // Cached images can finish before React attaches onLoad.
        ref={(img) => {
          if (img && img.complete && img.naturalWidth > 0 && !loadedCovers.has(src)) {
            markLoaded(src);
          }
        }}
        src={src}
        alt={title}
        loading="lazy"
        decoding="async"
        onLoad={() => markLoaded(src)}
        onError={() => setFailedSrc(src)}
        className={`block h-full w-full rounded-[inherit] bg-surface object-cover ${className ?? ''}`}
        style={{
          opacity: loaded ? 1 : 0,
          transition: loaded ? 'opacity 120ms ease' : undefined,
          ...style,
        }}
      />
    );
  }

  return (
    <div
      data-testid="cover-placeholder"
      className={`flex h-full w-full select-none flex-col items-center justify-center overflow-hidden break-words rounded-[inherit] bg-surface p-2 text-center text-xs text-text-dimmed ${
        className ?? ''
      }`}
      style={style}
    >
      <span className="font-medium text-text-default">{title}</span>
      {detectedName && detectedName !== title && (
        <span className="mt-1 max-w-full truncate text-[10px] opacity-80">
          {detectedName}
        </span>
      )}
    </div>
  );
};
