import React, { useState } from 'react';
import { coverAssetUrl } from '../../api';

export interface CoverImageProps {
  coverFile?: string | null;
  title: string;
  detectedName?: string | null;
  className?: string;
  style?: React.CSSProperties;
}

export const CoverImage: React.FC<CoverImageProps> = ({
  coverFile,
  title,
  detectedName,
  className,
  style,
}) => {
  const [hasError, setHasError] = useState(false);
  const src = coverAssetUrl(coverFile);

  if (src && !hasError) {
    return (
      <img
        src={src}
        alt={title}
        loading="lazy"
        decoding="async"
        onError={() => setHasError(true)}
        className={className}
        style={{
          width: '100%',
          height: '100%',
          objectFit: 'cover',
          display: 'block',
          borderRadius: 'inherit',
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
