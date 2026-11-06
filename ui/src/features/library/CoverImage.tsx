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
        className={className}
        style={{
          width: '100%',
          height: '100%',
          objectFit: 'cover',
          display: 'block',
          borderRadius: 'inherit',
          backgroundColor: 'var(--color-surface, #262525)',
          opacity: loaded ? 1 : 0,
          transition: loaded ? 'opacity 120ms ease' : undefined,
          ...style,
        }}
      />
    );
  }

  return (
    <div
      className={className}
      data-testid="cover-placeholder"
      style={{
        width: '100%',
        height: '100%',
        backgroundColor: 'var(--color-surface, #262525)',
        color: 'var(--color-text-dimmed, #808080)',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        textAlign: 'center',
        padding: '8px',
        fontSize: 'var(--font-size-xs, 12px)',
        overflow: 'hidden',
        wordBreak: 'break-word',
        borderRadius: 'inherit',
        userSelect: 'none',
        ...style,
      }}
    >
      <span style={{ fontWeight: 500, color: 'var(--color-text-default, #D9D9D9)' }}>
        {title}
      </span>
      {detectedName && detectedName !== title && (
        <span
          style={{
            fontSize: '10px',
            marginTop: '4px',
            opacity: 0.8,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            maxWidth: '100%',
          }}
        >
          {detectedName}
        </span>
      )}
    </div>
  );
};
